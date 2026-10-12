//! Cozo private action journal, deterministic projections, and durable staging.

use std::collections::BTreeMap;

use carrel_core::feed::DEFAULT_FETCH_INTERVAL_SECONDS;
use carrel_core::identity::{Keypair, PublicKey};
use carrel_core::private_state::{
    ReadIntent, ReadingPosition, StateKey, StateKind, StateRevision, StateValue, SubscriptionState,
    resolve_heads,
};
use carrel_core::subscription::FeedSubscription;
use cozo::{DataValue, JsonData, Validity};

use crate::identity::bytes;
use crate::private_library::{QUEUE_PRIVATE_FACT, private_fact_params};
use crate::{Result, Store, StoreError};
use carrel_core::private_library::PrivateFact;

/// Result of a verified delivery on the authenticated self channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateReceive {
    /// History, projection, and mirror queue changed atomically.
    Applied,
    /// Identical history already exists.
    Existing,
    /// Verified history is durable, waiting for parents or its item.
    Pending,
    /// An item tombstone dominates the action.
    Removed,
}

/// Promotion results contain only register/revision metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatePromotions {
    /// Registers changed by ready history.
    pub applied: Vec<String>,
    /// Staged records discarded for invalid ancestry or removed targets.
    pub rejected: Vec<String>,
}

const HEAD_RULES: &str = r#"
    superseded[reg, parent] := *self_state{register: reg, parents}, parent in parents
    head[reg, rev] := *self_state{register: reg, revision_id: rev}, not superseded[reg, rev]
"#;
const WRITE_HISTORY: &str = r#"
    { ?[register, revision_id, encoded, parents, kind, target] <- [[$reg, $rev, $encoded, $parents, $kind, $target]]
      :put self_state {register, revision_id => encoded, parents, kind, target} }
    { ?[register, revision_id] <- [[$reg, $rev]] :put self_state_outbox {register, revision_id} }
    { ?[register, revision_id] <- [[$reg, $rev]] :rm self_state_incoming {register, revision_id} }
"#;

impl Store {
    /// Make an explicit local action after observing all current register heads.
    ///
    /// The operational key remains independent of the master. Before binding,
    /// writes remain local and signed; binding later supplies their trust root.
    pub fn write_private_state(
        &self,
        key: StateKey,
        value: StateValue,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<StateRevision> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(signer.public())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        if key.kind == StateKind::Read && matches!(value, StateValue::Read(_)) {
            self.adopt_read_context_unlocked(&key.target, signer)?;
        }
        self.write_state_unlocked(key, value, at_micros, signer)
    }

    fn write_state_unlocked(
        &self,
        key: StateKey,
        value: StateValue,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<StateRevision> {
        validate_key(&key)?;
        if self.target_status(&key)? != TargetStatus::Ready {
            return Err(StoreError::InvalidInput {
                context: "private state target",
                message: "item is unavailable".into(),
            });
        }
        let heads = self.private_state_heads(&key)?;
        let record = StateRevision::sign(
            key,
            value,
            heads.iter().map(|head| head.revision_id.clone()).collect(),
            at_micros,
            signer,
        )?;
        record.validate_parents(&heads)?;
        self.apply_state(&record, &heads)?;
        Ok(record)
    }

    /// Record a finite HTML cursor, preserving independently signed intent.
    pub fn record_read_progress(
        &self,
        target: &str,
        progress: f64,
        scroll_y: f64,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<()> {
        if !progress.is_finite() || !scroll_y.is_finite() {
            return Err(StoreError::InvalidInput {
                context: "read_progress",
                message: "expected finite progress and scroll position".into(),
            });
        }
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        self.require_state_signer(signer)?;
        self.adopt_read_context_unlocked(target, signer)?;
        let progress = progress.clamp(0.0, 1.0);
        self.write_state_unlocked(
            StateKey {
                kind: StateKind::Position,
                target: target.into(),
            },
            StateValue::Position(ReadingPosition {
                progress: Some(progress),
                progress_label: Some(format!("{:.0}%", progress * 100.0)),
                last_position: Some(serde_json::json!({"scroll_y": scroll_y.max(0.0)})),
            }),
            at_micros,
            signer,
        )?;
        Ok(())
    }

    /// Toggle from the latest local projection while holding the register lock.
    pub fn toggle_private_star(
        &self,
        target: &str,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<bool> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        self.require_state_signer(signer)?;
        let rows = self.query_with_params(
            "?[starred] := *item_star{item_id: $id, starred}",
            BTreeMap::from([("id".into(), DataValue::from(target))]),
        )?;
        let starred = !matches!(
            rows.rows.first().and_then(|row| row.first()),
            Some(DataValue::Bool(true))
        );
        self.write_state_unlocked(
            StateKey {
                kind: StateKind::Star,
                target: target.into(),
            },
            StateValue::Star(starred),
            at_micros,
            signer,
        )?;
        Ok(starred)
    }

    fn adopt_read_context_unlocked(&self, target: &str, signer: &Keypair) -> Result<()> {
        let rows = self.query_with_params("?[state, progress, progress_label, last_position, updated_at] := *read_state{item_id: $id, state, progress, progress_label, last_position, updated_at}", BTreeMap::from([("id".into(), DataValue::from(target))]))?;
        let Some(row) = rows.rows.first() else {
            return Ok(());
        };
        let read_key = StateKey {
            kind: StateKind::Read,
            target: target.into(),
        };
        let position_key = StateKey {
            kind: StateKind::Position,
            target: target.into(),
        };
        if !self.private_state_heads(&read_key)?.is_empty() {
            return Ok(());
        }
        let intent = decode_intent(&row[0])?;
        let at = validity_micros(&row[4])?;
        if self.private_state_heads(&position_key)?.is_empty() {
            let progress = match &row[1] {
                DataValue::Null => None,
                DataValue::Num(cozo::Num::Float(value)) => Some(*value),
                DataValue::Num(cozo::Num::Int(value)) => Some(*value as f64),
                _ => return Err(invalid("invalid legacy progress")),
            };
            let label = match &row[2] {
                DataValue::Null => None,
                value => Some(as_string(value)?),
            };
            let cursor = match &row[3] {
                DataValue::Null => None,
                DataValue::Json(JsonData(value)) => Some(value.clone()),
                _ => return Err(invalid("invalid legacy cursor")),
            };
            self.write_state_unlocked(
                position_key,
                StateValue::Position(ReadingPosition {
                    progress,
                    progress_label: label,
                    last_position: cursor,
                }),
                at,
                signer,
            )?;
        }
        // Implicit reading has only position ancestry. It must never become an
        // explicit intent merely because progress was written a second time.
        if intent != ReadIntent::Reading {
            self.write_state_unlocked(read_key, StateValue::Read(intent), at, signer)?;
        }
        Ok(())
    }

    /// Sign existing unsigned local values before the first private transfer.
    ///
    /// Each register is resumable and checked under the write lock. Existing
    /// signed history is never replaced by a new snapshot root. Migration itself
    /// deliberately has no key access and cannot perform this adoption.
    pub fn adopt_private_state(&self, at_micros: i64, signer: &Keypair) -> Result<usize> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        self.require_state_signer(signer)?;
        // Cozo 0.7.6 cannot negate a time-travel scan directly. Materializing
        // the permanent-removal rule also makes the privacy boundary explicit.
        let read_rows = self.query(r#"removed[id] := *entity_tombstone{id @ "END"}
            ?[id, state, progress, progress_label, last_position, updated_at] := *read_state{item_id: id, state, progress, progress_label, last_position, updated_at}, *item{id}, not removed[id] :sort id"#)?;
        let star_rows = self.query(r#"removed[id] := *entity_tombstone{id @ "END"}
            ?[id, starred, updated_at] := *item_star{item_id: id, starred, updated_at}, *item{id}, not removed[id] :sort id"#)?;
        let mut values = Vec::new();
        for row in read_rows.rows {
            let id = as_string(&row[0])?;
            let intent = decode_intent(&row[1])?;
            // "Reading" can originate solely from progress. Replicating it as
            // explicit intent would make scrolling compete with completion.
            if intent != ReadIntent::Reading {
                values.push((
                    StateKey {
                        kind: StateKind::Read,
                        target: id.clone(),
                    },
                    StateValue::Read(intent),
                    validity_micros(&row[5])?,
                ));
            }
            let progress = match &row[2] {
                DataValue::Null => None,
                DataValue::Num(cozo::Num::Float(value)) => Some(*value),
                DataValue::Num(cozo::Num::Int(value)) => Some(*value as f64),
                _ => return Err(invalid("invalid legacy progress")),
            };
            let label = match &row[3] {
                DataValue::Null => None,
                value => Some(as_string(value)?),
            };
            let cursor = match &row[4] {
                DataValue::Null => None,
                DataValue::Json(JsonData(value)) => Some(value.clone()),
                _ => return Err(invalid("invalid legacy cursor")),
            };
            values.push((
                StateKey {
                    kind: StateKind::Position,
                    target: id,
                },
                StateValue::Position(ReadingPosition {
                    progress,
                    progress_label: label,
                    last_position: cursor,
                }),
                validity_micros(&row[5])?,
            ));
        }
        for row in star_rows.rows {
            let DataValue::Bool(starred) = row[1] else {
                return Err(invalid("invalid legacy star"));
            };
            values.push((
                StateKey {
                    kind: StateKind::Star,
                    target: as_string(&row[0])?,
                },
                StateValue::Star(starred),
                validity_micros(&row[2])?,
            ));
        }
        for feed in self.list_feeds()? {
            values.push((
                StateKey {
                    kind: StateKind::Subscription,
                    target: feed.url,
                },
                StateValue::Subscription(SubscriptionState {
                    subscribed: true,
                    title: feed.title,
                    folder: feed.folder,
                    auto_mark_read: feed.auto_mark_read,
                }),
                at_micros,
            ));
        }
        let mut adopted = 0;
        // Preserve each cursor before materializing its explicit intent. An
        // invalid legacy cursor must fail without an intent write clearing it.
        values.sort_by_key(|(key, _, _)| (key.target.clone(), key.kind == StateKind::Read));
        for (key, value, display_time) in values {
            if self.private_state_heads(&key)?.is_empty() {
                self.write_state_unlocked(key, value, display_time, signer)?;
                adopted += 1;
            }
        }
        Ok(adopted)
    }

    fn require_state_signer(&self, signer: &Keypair) -> Result<()> {
        if !self
            .annotation_creator_permissions_unlocked(signer.public())?
            .contains(signer.public())
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        Ok(())
    }

    /// Atomically import new subscriptions, preserving every existing setting.
    pub fn import_private_subscriptions(
        &self,
        subscriptions: &[FeedSubscription],
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<()> {
        let mut canonical = BTreeMap::new();
        for subscription in subscriptions {
            let url = crate::feeds::canonical_feed_url(&subscription.url)?;
            canonical.entry(url).or_insert(subscription);
        }
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        self.require_state_signer(signer)?;
        let mut scripts = Vec::new();
        let mut params = BTreeMap::new();
        for (url, subscription) in canonical {
            if self.get_feed(&url)?.is_some() {
                continue;
            }
            let key = StateKey {
                kind: StateKind::Subscription,
                target: url,
            };
            let heads = self.private_state_heads(&key)?;
            let record = StateRevision::sign(
                key,
                StateValue::Subscription(SubscriptionState {
                    subscribed: true,
                    title: subscription.title.clone(),
                    folder: subscription.folder.clone(),
                    auto_mark_read: false,
                }),
                heads.iter().map(|head| head.revision_id.clone()).collect(),
                at_micros,
                signer,
            )?;
            record.validate_parents(&heads)?;
            let (mut script, record_params) = self.prepare_state(&record, &heads)?;
            let index = scripts.len();
            for (name, value) in record_params {
                let renamed = format!("{name}_{index}");
                script = script.replace(&format!("${name}"), &format!("${renamed}"));
                params.insert(renamed, value);
            }
            scripts.push(script);
        }
        if !scripts.is_empty() {
            self.query_with_params(&scripts.join("\n"), params)?;
        }
        Ok(())
    }

    /// Subscribe explicitly; a post-sync resubscription supersedes removal heads.
    pub fn add_private_feed(
        &self,
        url: &str,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<crate::feeds::FeedRecord> {
        let url = crate::feeds::canonical_feed_url(url)?;
        self.import_private_subscriptions(
            &[FeedSubscription {
                url: url.clone(),
                title: None,
                folder: None,
            }],
            at_micros,
            signer,
        )?;
        self.get_feed(&url)?
            .ok_or_else(|| invalid("subscription was removed concurrently"))
    }

    /// Change a present subscription's folder through the signed private journal.
    /// Removed subscriptions cannot be revived by a stale Settings form.
    pub fn set_private_feed_folder(
        &self,
        url: &str,
        folder: Option<&str>,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<crate::feeds::FeedRecord> {
        let url = crate::feeds::canonical_feed_url(url)?;
        let folder = folder
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        self.require_state_signer(signer)?;
        let existing = self
            .get_feed(&url)?
            .ok_or_else(|| invalid("source is no longer subscribed"))?;
        let key = StateKey {
            kind: StateKind::Subscription,
            target: url.clone(),
        };
        let mut value = match self.private_state_value(&key)? {
            Some(StateValue::Subscription(value)) => value,
            _ => SubscriptionState {
                subscribed: true,
                title: existing.title.clone(),
                folder: existing.folder.clone(),
                auto_mark_read: existing.auto_mark_read,
            },
        };
        if value.folder != folder {
            value.folder = folder;
            self.write_state_unlocked(key, StateValue::Subscription(value), at_micros, signer)?;
        }
        self.get_feed(&url)?
            .ok_or_else(|| invalid("source is no longer subscribed"))
    }

    /// Record an explicit removal without deleting any saved articles.
    pub fn remove_private_feed(&self, url: &str, at_micros: i64, signer: &Keypair) -> Result<bool> {
        let url = crate::feeds::canonical_feed_url(url)?;
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        self.require_state_signer(signer)?;
        let existing = self.get_feed(&url)?;
        let key = StateKey {
            kind: StateKind::Subscription,
            target: url,
        };
        if existing.is_none() && self.private_state_heads(&key)?.is_empty() {
            return Ok(false);
        }
        let value = SubscriptionState {
            subscribed: false,
            title: existing.as_ref().and_then(|feed| feed.title.clone()),
            folder: existing.as_ref().and_then(|feed| feed.folder.clone()),
            auto_mark_read: existing.as_ref().is_some_and(|feed| feed.auto_mark_read),
        };
        self.write_state_unlocked(key, StateValue::Subscription(value), at_micros, signer)?;
        Ok(existing.is_some())
    }

    /// Read current causal heads in canonical revision order.
    pub fn private_state_heads(&self, key: &StateKey) -> Result<Vec<StateRevision>> {
        let rows = self.query_with_params(&format!(r#"
            {HEAD_RULES}
            ?[rev, encoded] := *self_state{{register: $reg, revision_id: rev, encoded}}, head[$reg, rev]
            :sort rev
        "#), BTreeMap::from([("reg".into(), DataValue::from(key.register()))]))?;
        rows.rows
            .iter()
            .map(|row| Ok(StateRevision::from_bytes(bytes(&row[1])?)?))
            .collect()
    }

    /// Resolve a register without accepting clocks as causal evidence.
    pub fn private_state_value(&self, key: &StateKey) -> Result<Option<StateValue>> {
        Ok(resolve_heads(&self.private_state_heads(key)?)?)
    }

    /// Accept signed history only from a currently authorized self device.
    pub fn accept_self_state_revision(
        &self,
        record: &StateRevision,
        sender: &PublicKey,
    ) -> Result<StateReceive> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        record.verify()?;
        validate_key(&record.key)?;
        let devices = self.self_devices()?;
        if !devices
            .iter()
            .any(|row| row.certificate.device_pubkey == *sender && row.is_authorized())
            || !devices
                .iter()
                .any(|row| row.certificate.device_pubkey == record.author)
        {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        self.accept_state_unlocked(record)
    }

    fn accept_state_unlocked(&self, record: &StateRevision) -> Result<StateReceive> {
        match self.target_status(&record.key)? {
            TargetStatus::Removed => {
                self.discard_pending_state(record)?;
                return Ok(StateReceive::Removed);
            }
            TargetStatus::Ready | TargetStatus::Missing => {}
        }
        let existing = self.query_with_params(
            "?[encoded] := *self_state{register: $reg, revision_id: $rev, encoded}",
            record_params(record)?,
        )?;
        if let Some(row) = existing.rows.first() {
            if StateRevision::from_bytes(bytes(&row[0])?)? != *record {
                return Err(invalid("revision collision"));
            }
            return Ok(StateReceive::Existing);
        }
        let parents = self.state_parents(record)?;
        if parents.len() != record.parents.len()
            || self.target_status(&record.key)? == TargetStatus::Missing
        {
            self.query_with_params(
                "?[register, revision_id, encoded, parents, kind, target] <- [[$reg, $rev, $encoded, $parents, $kind, $target]] :put self_state_incoming {register, revision_id => encoded, parents, kind, target}", record_params(record)?)?;
            return Ok(StateReceive::Pending);
        }
        record.validate_parents(&parents)?;
        let heads = self.private_state_heads(&record.key)?;
        self.apply_state(record, &heads)?;
        Ok(StateReceive::Applied)
    }

    fn state_parents(&self, record: &StateRevision) -> Result<Vec<StateRevision>> {
        let mut params = record_params(record)?;
        params.insert(
            "wanted".into(),
            DataValue::List(
                record
                    .parents
                    .iter()
                    .map(|id| DataValue::List(vec![DataValue::from(id.as_str())]))
                    .collect(),
            ),
        );
        let rows = self.query_with_params(
            r#"
            wanted[rev] <- $wanted
            ?[rev, encoded] := wanted[rev], *self_state{register: $reg, revision_id: rev, encoded}
            :sort rev
        "#,
            params,
        )?;
        let incompatible = self.query_with_params(
            r#"
            wanted[rev] <- $wanted
            ?[rev] := wanted[rev], *self_state{register: reg, revision_id: rev}, reg != $reg
            :limit 1
        "#,
            {
                let mut params = record_params(record)?;
                params.insert(
                    "wanted".into(),
                    DataValue::List(
                        record
                            .parents
                            .iter()
                            .map(|id| DataValue::List(vec![DataValue::from(id.as_str())]))
                            .collect(),
                    ),
                );
                params
            },
        )?;
        if !incompatible.rows.is_empty() {
            return Err(carrel_core::private_state::StateRevisionError::MissingParents.into());
        }
        rows.rows
            .iter()
            .map(|row| Ok(StateRevision::from_bytes(bytes(&row[1])?)?))
            .collect()
    }

    /// Promote ready history after receiving any article/parent dependency batch.
    pub fn promote_pending_state(&self) -> Result<StatePromotions> {
        let _guard = self
            .private_state_lock
            .lock()
            .expect("private state write lock");
        let _identity_guard = self.identity_lock.lock().expect("identity write lock");
        let mut report = StatePromotions::default();
        loop {
            let rows = self.query(r#"
                missing[reg, rev] := *self_state_incoming{register: reg, revision_id: rev, parents},
                    parent in parents, not *self_state{revision_id: parent}
                removed[reg, rev] := *self_state_incoming{register: reg, revision_id: rev, kind, target}, kind != 'subscription', *entity_tombstone{id: target @ "END"}
                ready[reg, rev] := *self_state_incoming{register: reg, revision_id: rev, kind}, kind == 'subscription', not missing[reg, rev]
                ready[reg, rev] := *self_state_incoming{register: reg, revision_id: rev, kind, target}, kind != 'subscription', *item{id: target}, not missing[reg, rev]
                selected[reg, rev] := ready[reg, rev]
                selected[reg, rev] := removed[reg, rev]
                ?[reg, rev, encoded] := selected[reg, rev], *self_state_incoming{register: reg, revision_id: rev, encoded}
                :sort reg, rev
                :limit 100
            "#)?;
            if rows.rows.is_empty() {
                break;
            }
            for row in rows.rows {
                let record = StateRevision::from_bytes(bytes(&row[2])?)?;
                match self.accept_state_unlocked(&record) {
                    Ok(StateReceive::Applied) => report.applied.push(record.key.register()),
                    Ok(StateReceive::Existing) => {
                        self.discard_pending_state(&record)?;
                    }
                    Ok(StateReceive::Removed) => report.rejected.push(record.revision_id),
                    Ok(StateReceive::Pending) => {
                        return Err(invalid("ready record still has missing dependencies"));
                    }
                    Err(StoreError::StateRevision(_)) => {
                        self.discard_pending_state(&record)?;
                        report.rejected.push(record.revision_id);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(report)
    }

    /// Read a bounded deterministic batch of durable mirror work.
    pub fn private_state_outbox(&self, limit: usize) -> Result<Vec<StateRevision>> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("outbox batch must contain 1 to 1000 records"));
        }
        let rows = self.query_with_params(r#"
            ?[reg, rev, encoded] := *self_state_outbox{register: reg, revision_id: rev}, *self_state{register: reg, revision_id: rev, encoded}
            :sort reg, rev
            :limit $limit
        "#, BTreeMap::from([("limit".into(), DataValue::from(limit as i64))]))?;
        rows.rows
            .iter()
            .map(|row| Ok(StateRevision::from_bytes(bytes(&row[2])?)?))
            .collect()
    }

    /// Acknowledge only the exact revision durably written to the sync mirror.
    pub fn acknowledge_private_state(&self, key: &StateKey, revision: &str) -> Result<()> {
        self.query_with_params("?[register, revision_id] <- [[$reg, $rev]] :rm self_state_outbox {register, revision_id}",
            BTreeMap::from([("reg".into(), DataValue::from(key.register())), ("rev".into(), DataValue::from(revision))]))?;
        Ok(())
    }

    fn discard_pending_state(&self, record: &StateRevision) -> Result<()> {
        self.query_with_params("?[register, revision_id] <- [[$reg, $rev]] :rm self_state_incoming {register, revision_id}", record_params(record)?)?;
        Ok(())
    }

    fn target_status(&self, key: &StateKey) -> Result<TargetStatus> {
        if key.kind == StateKind::Subscription {
            return Ok(TargetStatus::Ready);
        }
        let params = BTreeMap::from([("id".into(), DataValue::from(key.target.as_str()))]);
        if !self
            .query_with_params(
                "?[id] := *entity_tombstone{id @ \"END\"}, id = $id",
                params.clone(),
            )?
            .rows
            .is_empty()
        {
            return Ok(TargetStatus::Removed);
        }
        Ok(
            if self
                .query_with_params("?[id] := *item{id}, id = $id", params)?
                .rows
                .is_empty()
            {
                TargetStatus::Missing
            } else {
                TargetStatus::Ready
            },
        )
    }

    fn apply_state(&self, record: &StateRevision, current: &[StateRevision]) -> Result<()> {
        let (script, params) = self.prepare_state(record, current)?;
        self.query_with_params(&script, params)?;
        Ok(())
    }

    fn prepare_state(
        &self,
        record: &StateRevision,
        current: &[StateRevision],
    ) -> Result<(String, BTreeMap<String, DataValue>)> {
        let mut next: Vec<_> = current
            .iter()
            .filter(|head| !record.parents.contains(&head.revision_id))
            .cloned()
            .collect();
        next.push(record.clone());
        let value = resolve_heads(&next)?.expect("one or more heads");
        let display_time = next
            .iter()
            .map(|head| head.at_micros)
            .max()
            .expect("nonempty heads");
        let mut params = record_params(record)?;
        params.extend(private_fact_params(&PrivateFact::State(record.clone()))?);
        let projection = match value {
            StateValue::Star(starred) => {
                params.insert("starred".into(), DataValue::Bool(starred));
                params.insert(
                    "at".into(),
                    DataValue::Validity(Validity::from((display_time, true))),
                );
                "{ ?[item_id, starred, updated_at] <- [[$target, $starred, $at]] :put item_star {item_id => starred, updated_at} }".to_string()
            }
            StateValue::Subscription(value) => {
                if value.subscribed {
                    // A missing chosen title leaves the publisher's local title
                    // intact when portable settings (such as folder) change.
                    let title = value.title.or(self
                        .get_feed(&record.key.target)?
                        .and_then(|feed| feed.title));
                    params.insert("title".into(), optional_string(title.as_deref()));
                    params.insert("folder".into(), optional_string(value.folder.as_deref()));
                    params.insert("auto".into(), DataValue::Bool(value.auto_mark_read));
                    params.insert(
                        "interval".into(),
                        DataValue::from(DEFAULT_FETCH_INTERVAL_SECONDS),
                    );
                    r#"{
                        cache[description, fetched, modified, etag, interval, failures] := *feed{url: $target, description, last_fetched: fetched, last_modified_header: modified, etag_header: etag, fetch_interval_seconds: interval, consecutive_failures: failures}
                        cache[description, fetched, modified, etag, interval, failures] := not *feed{url: $target}, description = null, fetched = null, modified = null, etag = null, interval = $interval, failures = 0
                        ?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] := cache[description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures], url = $target, title = $title, folder = $folder, auto_mark_read = $auto
                        :put feed {url => title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read}
                    }"#.to_string()
                } else {
                    "{ ?[url] <- [[$target]] :rm feed {url} }".to_string()
                }
            }
            StateValue::Read(_) | StateValue::Position(_) => {
                let other_kind = if record.key.kind == StateKind::Read {
                    StateKind::Position
                } else {
                    StateKind::Read
                };
                let other_heads = self.private_state_heads(&StateKey {
                    kind: other_kind,
                    target: record.key.target.clone(),
                })?;
                let other = resolve_heads(&other_heads)?;
                let display_time = other_heads
                    .iter()
                    .map(|head| head.at_micros)
                    .max()
                    .map_or(display_time, |at| at.max(display_time));
                let (intent, position) = if record.key.kind == StateKind::Read {
                    (Some(value), other)
                } else {
                    (other, Some(value))
                };
                self.read_projection_params(
                    &record.key.target,
                    intent,
                    position,
                    display_time,
                    &mut params,
                )?;
                "{ ?[item_id, state, progress, progress_label, last_position, updated_at] <- [[$target, $state, $progress, $label, $position, $at]] :put read_state {item_id => state, progress, progress_label, last_position, updated_at} }".to_string()
            }
        };
        Ok((
            format!("{WRITE_HISTORY}\n{projection}\n{QUEUE_PRIVATE_FACT}"),
            params,
        ))
    }

    fn read_projection_params(
        &self,
        target: &str,
        intent: Option<StateValue>,
        position: Option<StateValue>,
        at: i64,
        params: &mut BTreeMap<String, DataValue>,
    ) -> Result<()> {
        let rows = self.query_with_params("?[state, progress, progress_label, last_position, updated_at] := *read_state{item_id: $id, state, progress, progress_label, last_position, updated_at}", BTreeMap::from([("id".into(), DataValue::from(target))]))?;
        let old = rows.rows.first();
        let state = match intent {
            Some(StateValue::Read(intent)) => DataValue::from(intent.as_str()),
            None => old.map_or_else(|| DataValue::from("reading"), |row| row[0].clone()),
            _ => return Err(invalid("invalid read projection intent")),
        };
        let (mut progress, mut label, cursor) = match position {
            Some(StateValue::Position(position)) => (
                position.progress.map_or(DataValue::Null, DataValue::from),
                optional_string(position.progress_label.as_deref()),
                position
                    .last_position
                    .map_or(DataValue::Null, |cursor| DataValue::Json(JsonData(cursor))),
            ),
            None => (DataValue::Null, DataValue::Null, DataValue::Null),
            _ => return Err(invalid("invalid read projection position")),
        };
        if state == DataValue::from("read") {
            progress = DataValue::from(1.0);
            label = DataValue::from("100%");
        }
        // Current heads determine the display stamp independently of delivery
        // order. Causal ancestry permits an action with a backward clock; the
        // legacy raw-write timestamp guard applies only to untracked rows.
        params.extend([
            ("state".into(), state),
            ("progress".into(), progress),
            ("label".into(), label),
            ("position".into(), cursor),
            ("at".into(), DataValue::Validity(Validity::from((at, true)))),
        ]);
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TargetStatus {
    Ready,
    Missing,
    Removed,
}

fn validate_key(key: &StateKey) -> Result<()> {
    if key.kind == StateKind::Subscription
        && crate::feeds::canonical_feed_url(&key.target)? != key.target
    {
        return Err(invalid("subscription URL is not canonical"));
    }
    Ok(())
}

fn record_params(record: &StateRevision) -> Result<BTreeMap<String, DataValue>> {
    Ok(BTreeMap::from([
        ("reg".into(), DataValue::from(record.key.register())),
        ("rev".into(), DataValue::from(record.revision_id.as_str())),
        ("encoded".into(), DataValue::Bytes(record.to_bytes()?)),
        (
            "parents".into(),
            DataValue::List(
                record
                    .parents
                    .iter()
                    .map(|id| DataValue::from(id.as_str()))
                    .collect(),
            ),
        ),
        ("kind".into(), DataValue::from(record.key.kind.as_str())),
        ("target".into(), DataValue::from(record.key.target.as_str())),
    ]))
}

fn optional_string(value: Option<&str>) -> DataValue {
    value.map_or(DataValue::Null, DataValue::from)
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput {
        context: "private state",
        message: message.into(),
    }
}

fn as_string(value: &DataValue) -> Result<String> {
    if let DataValue::Str(value) = value {
        Ok(value.to_string())
    } else {
        Err(invalid("expected string"))
    }
}
fn decode_intent(value: &DataValue) -> Result<ReadIntent> {
    match as_string(value)?.as_str() {
        "unread" => Ok(ReadIntent::Unread),
        "reading" => Ok(ReadIntent::Reading),
        "skimmed" => Ok(ReadIntent::Skimmed),
        "abandoned" => Ok(ReadIntent::Abandoned),
        "read" => Ok(ReadIntent::Read),
        _ => Err(invalid("invalid legacy read intent")),
    }
}

fn validity_micros(value: &DataValue) -> Result<i64> {
    if let DataValue::Validity(value) = value {
        Ok((value.timestamp.0).0)
    } else {
        Err(invalid("expected display timestamp"))
    }
}
