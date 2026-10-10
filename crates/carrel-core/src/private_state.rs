//! Private state has causal ancestry; clocks are display metadata only.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

use crate::annotation::note::valid_revision_id;
use crate::identity::{
    CanonicalVerifyError, Keypair, PublicKey, SerializeError, Signature, canonical_bytes,
    sign_canonical, verify_canonical,
};

/// Maximum immediate predecessors of one explicit state action.
pub const MAX_STATE_PARENTS: usize = 1024;
/// Maximum encoded record at the private protocol boundary.
pub const MAX_STATE_BYTES: usize = 128 * 1024;

/// Independently merged kinds of private state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    /// Explicit completion intent, independent of scrolling.
    Read,
    /// Last reading cursor, independent of completion.
    Position,
    /// Saved-item intent.
    Star,
    /// Subscription membership and portable settings.
    Subscription,
}

impl StateKind {
    /// Stable schema name, also used in register identities.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Position => "position",
            Self::Star => "star",
            Self::Subscription => "subscription",
        }
    }
}

/// One independently resolved register.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct StateKey {
    /// The register's semantics.
    pub kind: StateKind,
    /// Stable item identity, or canonical URL for subscriptions.
    pub target: String,
}

impl StateKey {
    /// Stable key used to index the signed journal.
    pub fn register(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.target)
    }
}

/// Completion states retained from the local schema.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadIntent {
    /// Explicitly available to read again.
    Unread,
    /// In progress, without implying completion.
    Reading,
    /// Explicitly skimmed.
    Skimmed,
    /// Explicitly put aside.
    Abandoned,
    /// Explicitly finished.
    Read,
}

impl ReadIntent {
    /// Stable value of the query projection.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Reading => "reading",
            Self::Skimmed => "skimmed",
            Self::Abandoned => "abandoned",
            Self::Read => "read",
        }
    }

    fn conflict_priority(self) -> u8 {
        match self {
            Self::Unread => 0,
            Self::Reading => 1,
            Self::Skimmed => 2,
            Self::Abandoned => 3,
            Self::Read => 4,
        }
    }
}

/// Portable cursor; the format remains generic for future content types.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReadingPosition {
    /// Optional normalized fraction, without completion semantics.
    pub progress: Option<f64>,
    /// Optional presentation label.
    pub progress_label: Option<String>,
    /// Type-specific bounded JSON cursor.
    pub last_position: Option<serde_json::Value>,
}

/// User-controlled feed metadata; HTTP cache state stays on its device.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscriptionState {
    /// False is an explicit removal, retained in history.
    pub subscribed: bool,
    /// Portable title, including titles from OPML.
    pub title: Option<String>,
    /// Portable folder path.
    pub folder: Option<String>,
    /// Legacy stored setting, preserved without enabling automatic completion.
    pub auto_mark_read: bool,
}

/// The typed payload of a private action.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StateValue {
    /// Explicit completion intent.
    Read(ReadIntent),
    /// Last reading position.
    Position(ReadingPosition),
    /// Star or unstar intent.
    Star(bool),
    /// Subscription membership/settings intent.
    Subscription(SubscriptionState),
}

impl StateValue {
    /// Register kind required by this payload.
    pub const fn kind(&self) -> StateKind {
        match self {
            Self::Read(_) => StateKind::Read,
            Self::Position(_) => StateKind::Position,
            Self::Star(_) => StateKind::Star,
            Self::Subscription(_) => StateKind::Subscription,
        }
    }
}

/// An immutable, content-addressed private action.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StateRevision {
    /// Register identity, included in the signature.
    pub key: StateKey,
    /// Typed replacement value.
    pub value: StateValue,
    /// Operational key that originated the action.
    pub author: PublicKey,
    /// Display timestamp; never a conflict tie-breaker.
    pub at_micros: i64,
    /// Sorted complete set of observed heads superseded by the action.
    pub parents: Vec<String>,
    /// Canonical payload version.
    pub version: u8,
    /// Signature covering the payload and parents.
    pub signature: Signature,
    /// BLAKE3 digest of the signed payload and signature.
    pub revision_id: String,
}

impl StateRevision {
    /// Sign an explicit action with its observed causal predecessors.
    pub fn sign(
        key: StateKey,
        value: StateValue,
        parents: Vec<String>,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<Self, StateRevisionError> {
        let mut revision = Self {
            key,
            value,
            parents,
            at_micros,
            author: *signer.public(),
            version: 1,
            signature: Signature::from_bytes([0; 64]),
            revision_id: String::new(),
        };
        revision.validate_content()?;
        let signature = sign_canonical(signer, &revision.payload())?;
        revision.signature = signature;
        revision.revision_id = revision.computed_revision_id()?;
        Ok(revision)
    }

    /// Verify the complete signed content and its address.
    pub fn verify(&self) -> Result<(), StateRevisionError> {
        self.validate_content()?;
        verify_canonical(&self.author, &self.payload(), &self.signature)?;
        if self.revision_id != self.computed_revision_id()? {
            return Err(StateRevisionError::Identity);
        }
        if self.parents.contains(&self.revision_id) {
            return Err(StateRevisionError::Parents);
        }
        Ok(())
    }

    /// Verify exact predecessor records and register membership.
    pub fn validate_parents(&self, parents: &[Self]) -> Result<(), StateRevisionError> {
        if self.parents.len() != parents.len() {
            return Err(StateRevisionError::MissingParents);
        }
        for (id, parent) in self.parents.iter().zip(parents) {
            parent.verify()?;
            if id != &parent.revision_id || self.key != parent.key {
                return Err(StateRevisionError::MissingParents);
            }
        }
        Ok(())
    }

    /// Encode a bounded verified record for the private channel.
    pub fn to_bytes(&self) -> Result<Vec<u8>, StateRevisionError> {
        self.verify()?;
        let bytes = canonical_bytes(self)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StateRevisionError::TooLarge);
        }
        Ok(bytes)
    }

    /// Decode exactly one bounded record, rejecting invalid signatures.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StateRevisionError> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StateRevisionError::TooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        let revision: Self = ciborium::de::from_reader(&mut cursor)
            .map_err(|error| StateRevisionError::Decode(error.to_string()))?;
        if cursor.position() != bytes.len() as u64 {
            return Err(StateRevisionError::TrailingData);
        }
        revision.verify()?;
        Ok(revision)
    }

    fn computed_revision_id(&self) -> Result<String, StateRevisionError> {
        Ok(blake3::hash(&canonical_bytes(&(
            "carrel-self-state-id-v1",
            self.payload(),
            self.signature,
        ))?)
        .to_hex()
        .to_string())
    }

    fn payload(&self) -> impl Serialize + '_ {
        (
            "carrel-self-state-v1",
            self.version,
            &self.key,
            &self.value,
            self.author,
            self.at_micros,
            &self.parents,
        )
    }

    fn validate_content(&self) -> Result<(), StateRevisionError> {
        let target_limit = if self.key.kind == StateKind::Subscription {
            4096
        } else {
            256
        };
        if self.version != 1
            || self.key.kind != self.value.kind()
            || self.key.target.is_empty()
            || self.key.target.len() > target_limit
        {
            return Err(StateRevisionError::Content);
        }
        if self.parents.len() > MAX_STATE_PARENTS
            || self.parents.iter().any(|id| !valid_revision_id(id))
            || self.parents.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(StateRevisionError::Parents);
        }
        match &self.value {
            StateValue::Position(position) => {
                if position
                    .progress
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                    || position
                        .progress_label
                        .as_ref()
                        .is_some_and(|value| value.len() > 1024)
                {
                    return Err(StateRevisionError::Content);
                }
                if let Some(cursor) = &position.last_position
                    && (!valid_json_depth(cursor, 0)
                        || serde_json::to_vec(cursor)
                            .map_err(|_| StateRevisionError::Content)?
                            .len()
                            > 16 * 1024)
                {
                    return Err(StateRevisionError::Content);
                }
            }
            StateValue::Subscription(value) => {
                if !(self.key.target.starts_with("https://")
                    || self.key.target.starts_with("http://"))
                    || value.title.as_ref().is_some_and(|title| title.len() > 4096)
                    || value
                        .folder
                        .as_ref()
                        .is_some_and(|folder| folder.len() > 4096)
                {
                    return Err(StateRevisionError::Content);
                }
            }
            StateValue::Read(_) | StateValue::Star(_) => {}
        }
        Ok(())
    }
}

fn valid_json_depth(value: &serde_json::Value, depth: usize) -> bool {
    depth <= 32
        && match value {
            serde_json::Value::Array(values) => values
                .iter()
                .all(|value| valid_json_depth(value, depth + 1)),
            serde_json::Value::Object(values) => values
                .values()
                .all(|value| valid_json_depth(value, depth + 1)),
            _ => true,
        }
}

/// Resolve concurrent heads without erasing their history or interpreting clocks.
///
/// The caller supplies graph heads, not arbitrary ancestors. Input order has no
/// effect. A new explicit action can supersede all these heads to reverse intent.
pub fn resolve_heads(heads: &[StateRevision]) -> Result<Option<StateValue>, StateRevisionError> {
    let Some(first) = heads.first() else {
        return Ok(None);
    };
    for head in heads {
        head.verify()?;
        if head.key != first.key {
            return Err(StateRevisionError::Content);
        }
    }
    let value = match first.key.kind {
        StateKind::Read => StateValue::Read(
            heads
                .iter()
                .filter_map(|head| match head.value {
                    StateValue::Read(value) => Some(value),
                    _ => None,
                })
                .min_by_key(|value| value.conflict_priority())
                .ok_or(StateRevisionError::Content)?,
        ),
        StateKind::Star => StateValue::Star(
            heads
                .iter()
                .any(|head| head.value == StateValue::Star(true)),
        ),
        StateKind::Subscription => {
            let mut ordered: Vec<_> = heads.iter().collect();
            ordered.sort_by_key(|head| &head.revision_id);
            ordered.iter().find(|head| matches!(&head.value, StateValue::Subscription(value) if !value.subscribed))
                .unwrap_or(&ordered[0]).value.clone()
        }
        StateKind::Position => heads
            .iter()
            .max_by_key(|head| &head.revision_id)
            .ok_or(StateRevisionError::Content)?
            .value
            .clone(),
    };
    Ok(Some(value))
}

/// Invalid private state, signature, address, or causal ancestry.
#[derive(Debug, thiserror::Error)]
pub enum StateRevisionError {
    /// Invalid target, value, version, or bounded cursor.
    #[error("invalid private state content")]
    Content,
    /// Malformed, unordered, duplicated, or self-referential parents.
    #[error("invalid private state parents")]
    Parents,
    /// Parents are missing or belong to a different register.
    #[error("private state predecessors are missing or incompatible")]
    MissingParents,
    /// Content address does not match the signed data.
    #[error("private state revision identity did not verify")]
    Identity,
    /// Signature is invalid.
    #[error("private state signature did not verify")]
    Signature(#[from] CanonicalVerifyError),
    /// Canonical encoding failed.
    #[error("failed to encode private state")]
    Serialize(#[from] SerializeError),
    /// Record exceeds the protocol bound.
    #[error("private state exceeds {MAX_STATE_BYTES} bytes")]
    TooLarge,
    /// Record decoding failed.
    #[error("failed to decode private state: {0}")]
    Decode(String),
    /// Bytes remain after the first record.
    #[error("trailing bytes after private state")]
    TrailingData,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn key(kind: StateKind) -> StateKey {
        StateKey {
            kind,
            target: if kind == StateKind::Subscription {
                "https://example.com/feed"
            } else {
                "article"
            }
            .into(),
        }
    }
    fn revision(value: StateValue, seed: u8, at: i64) -> StateRevision {
        StateRevision::sign(
            key(value.kind()),
            value,
            vec![],
            at,
            &Keypair::from_seed(&[seed; 32]),
        )
        .unwrap()
    }
    fn sub(present: bool) -> StateValue {
        StateValue::Subscription(SubscriptionState {
            subscribed: present,
            title: Some("Café / 文".into()),
            folder: Some("Art/Design".into()),
            auto_mark_read: false,
        })
    }
    fn position(progress: f64) -> StateValue {
        StateValue::Position(ReadingPosition {
            progress: Some(progress),
            progress_label: Some("Halfway".into()),
            last_position: Some(serde_json::json!({"scroll_y": 200.0})),
        })
    }

    #[test]
    fn all_values_roundtrip_and_every_signed_field_rejects_tampering() {
        for value in [
            StateValue::Read(ReadIntent::Unread),
            StateValue::Read(ReadIntent::Reading),
            StateValue::Read(ReadIntent::Skimmed),
            StateValue::Read(ReadIntent::Abandoned),
            StateValue::Read(ReadIntent::Read),
            StateValue::Star(false),
            StateValue::Star(true),
            position(0.5),
            sub(false),
            sub(true),
        ] {
            let original = revision(value, 1, i64::MIN);
            assert_eq!(
                StateRevision::from_bytes(&original.to_bytes().unwrap()).unwrap(),
                original
            );
            let mut changes = vec![original.clone(); 7];
            changes[0].key.target.push('x');
            changes[1].author = *Keypair::from_seed(&[2; 32]).public();
            changes[2].at_micros = i64::MAX;
            changes[3].parents = vec!["a".repeat(64)];
            changes[4].version = 2;
            changes[5].revision_id = "b".repeat(64);
            changes[6].key.kind = if original.key.kind == StateKind::Read {
                StateKind::Star
            } else {
                StateKind::Read
            };
            for changed in changes {
                assert!(changed.verify().is_err());
            }
            let mut trailing = original.to_bytes().unwrap();
            trailing.push(0);
            assert!(matches!(
                StateRevision::from_bytes(&trailing),
                Err(StateRevisionError::TrailingData)
            ));
        }
        assert!(StateRevision::from_bytes(&[]).is_err());
        assert!(StateRevision::from_bytes(&vec![0; MAX_STATE_BYTES + 1]).is_err());
    }

    #[test]
    fn parents_are_exact_sorted_unique_and_register_scoped() {
        let first = revision(StateValue::Star(true), 1, 5);
        let second = StateRevision::sign(
            first.key.clone(),
            StateValue::Star(false),
            vec![first.revision_id.clone()],
            -5,
            &Keypair::from_seed(&[2; 32]),
        )
        .unwrap();
        second
            .validate_parents(std::slice::from_ref(&first))
            .unwrap();
        assert!(second.validate_parents(&[]).is_err());
        assert!(
            second
                .validate_parents(&[revision(StateValue::Read(ReadIntent::Read), 1, 5)])
                .is_err()
        );
        for parents in [
            vec!["A".repeat(64)],
            vec!["a".repeat(64), "a".repeat(64)],
            vec!["b".repeat(64), "a".repeat(64)],
            vec!["a".repeat(64); MAX_STATE_PARENTS + 1],
        ] {
            assert!(
                StateRevision::sign(
                    first.key.clone(),
                    first.value.clone(),
                    parents,
                    0,
                    &Keypair::from_seed(&[1; 32])
                )
                .is_err()
            );
        }
    }

    #[test]
    fn invalid_targets_and_cursor_bounds_fail_before_signing() {
        let signer = Keypair::from_seed(&[1; 32]);
        for progress in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.1] {
            assert!(
                StateRevision::sign(
                    key(StateKind::Position),
                    position(progress),
                    vec![],
                    0,
                    &signer
                )
                .is_err()
            );
        }
        for target in [String::new(), "x".repeat(257)] {
            assert!(
                StateRevision::sign(
                    StateKey {
                        kind: StateKind::Star,
                        target
                    },
                    StateValue::Star(true),
                    vec![],
                    0,
                    &signer
                )
                .is_err()
            );
        }
        for cursor in [
            serde_json::json!("x".repeat(16 * 1024)),
            (0..34).fold(serde_json::json!(0), |value, _| serde_json::json!([value])),
        ] {
            let value = StateValue::Position(ReadingPosition {
                progress: None,
                progress_label: None,
                last_position: Some(cursor),
            });
            assert!(
                StateRevision::sign(key(StateKind::Position), value, vec![], 0, &signer).is_err()
            );
        }
        let mut bad = key(StateKind::Subscription);
        bad.target = "file:///tmp/feed".into();
        assert!(StateRevision::sign(bad, sub(true), vec![], 0, &signer).is_err());
        assert!(
            resolve_heads(&[
                revision(StateValue::Star(true), 1, 0),
                revision(StateValue::Read(ReadIntent::Unread), 2, 0)
            ])
            .is_err()
        );
        assert_eq!(resolve_heads(&[]).unwrap(), None);
    }

    #[test]
    fn positions_are_deterministic_and_a_successor_can_move_backwards() {
        let a = revision(position(0.9), 1, i64::MAX);
        let b = revision(position(0.1), 2, i64::MIN);
        assert_eq!(
            resolve_heads(&[a.clone(), b.clone()]).unwrap(),
            resolve_heads(&[b.clone(), a.clone()]).unwrap()
        );
        let mut parents = vec![a.revision_id, b.revision_id];
        parents.sort();
        let c = StateRevision::sign(
            key(StateKind::Position),
            position(0.0),
            parents,
            0,
            &Keypair::from_seed(&[1; 32]),
        )
        .unwrap();
        assert_eq!(resolve_heads(&[c]).unwrap(), Some(position(0.0)));
    }

    proptest! {
        #[test]
        fn concurrent_intent_is_order_and_clock_independent(a in any::<i64>(), b in any::<i64>()) {
            for (first, second, expected) in [
                (StateValue::Read(ReadIntent::Read), StateValue::Read(ReadIntent::Unread), StateValue::Read(ReadIntent::Unread)),
                (StateValue::Star(false), StateValue::Star(true), StateValue::Star(true)),
                (sub(true), sub(false), sub(false)),
            ] {
                let x = revision(first, 1, a); let y = revision(second, 2, b);
                prop_assert_eq!(resolve_heads(&[x.clone(), y.clone()]).unwrap(), Some(expected.clone()));
                prop_assert_eq!(resolve_heads(&[y.clone(), x.clone(), y]).unwrap(), Some(expected));
            }
        }
    }
}
