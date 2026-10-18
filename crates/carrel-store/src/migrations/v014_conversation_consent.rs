//! Snapshot only history accepted before the consent boundary; never sign here.
use crate::{Result, StoreError, schema};
use carrel_core::{conversation::ThreadScope, identity::canonical_bytes, sharing::ShareFact};
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str, params| db.run_script(script, params, ScriptMutability::Mutable);
    if !run(
        "?[done] := *schema_version{version:14}, done = true :limit 1",
        BTreeMap::new(),
    )?
    .rows
    .is_empty()
    {
        return Ok(());
    }
    // Keep schema 6's original trigger byte-for-byte for exact backup validation.
    let mut markers = Vec::new();
    for row in run(
        "?[key,encoded] := *social_record{key,kind:'share',encoded} :sort key",
        BTreeMap::new(),
    )?
    .rows
    {
        let Some(DataValue::Bytes(bytes)) = row.get(1) else {
            return Err(corrupt());
        };
        let fact = ShareFact::from_bytes(bytes)?;
        if row[0] != DataValue::from(fact.key()?) {
            return Err(corrupt());
        }
        let ShareFact::Share(root) = fact else {
            return Err(corrupt());
        };
        let scope = ThreadScope::from_share(&root)?;
        markers.push(DataValue::List(vec![
            DataValue::from(scope.share_id.as_str()),
            DataValue::Bytes(canonical_bytes(&scope).map_err(|_| corrupt())?),
        ]));
    }
    let exists = run("::relations", BTreeMap::new())?
        .rows
        .iter()
        .any(|r| r.first() == Some(&DataValue::from("social_conversation_legacy")));
    let create = if exists {
        String::new()
    } else {
        "{ :create social_conversation_legacy {share_id:String=>scope:Bytes} }".into()
    };
    // Creation, the exact snapshot and completion share one transaction. Retrying
    // a completed migration must never classify a subsequently received root.
    run(
        &format!(
            "{create}\n{{ {} }}\n{{ ?[share_id,scope] <- $markers :put social_conversation_legacy {{share_id=>scope}} }}\n{{ {} }}\n{{ ?[version,applied_at,description] <- [[14,$now,'private causal conversation consent']] :put schema_version {{version=>applied_at,description}} }}",
            schema::CONVERSATION_STATE_GUARD,
            schema::CONVERSATION_LEGACY_GUARD
        ),
        BTreeMap::from([
            ("markers".into(), DataValue::List(markers)),
            (
                "now".into(),
                DataValue::Validity(Validity::from((
                    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64,
                    true,
                ))),
            ),
        ]),
    )?;
    Ok(())
}

fn corrupt() -> StoreError {
    StoreError::InvalidInput {
        context: "conversation consent migration",
        message: "invalid accepted root".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use carrel_core::{
        identity::{DeviceCert, Keypair},
        private_library::SavedItem,
        sharing::Share,
    };

    fn legacy_root(store: &Store, token: &str) -> Share {
        let master = Keypair::from_seed(&[71; 32]);
        let device = Keypair::from_seed(&[72; 32]);
        let recipient = Keypair::from_seed(&[73; 32]);
        let root = Share::sign(
            token.into(),
            *recipient.public(),
            SavedItem {
                id: "a".repeat(64),
                kind: "article".into(),
                title: "Legacy 文".into(),
                creators: vec![],
                primary_url: Some("https://example.org/legacy".into()),
                published_at_micros: None,
                language: None,
                summary: None,
                discovered_at_micros: 0,
                article: None,
                identifiers: vec![],
                contents: vec![],
                resources: vec![],
                current_images: None,
            },
            None,
            1,
            DeviceCert::new_at_micros(&master, device.public(), 1, None),
            &device,
        )
        .unwrap();
        let fact = ShareFact::Share(Box::new(root.clone()));
        store
            .query_with_params(
                crate::sharing::PUT_RECORD,
                crate::sharing::record_params(&fact).unwrap(),
            )
            .unwrap();
        root
    }

    #[test]
    fn exact_version_runner_preserves_13_and_empty_14_remains_restorable() {
        let old = Store::open_in_memory().unwrap();
        super::super::migrate_through(&old.db, 0, 13).unwrap();
        assert_eq!(old.current_schema_version().unwrap(), 13);
        assert!(!old.relation_exists("social_conversation_legacy").unwrap());
        let old_guard = old.query("::show_triggers self_state").unwrap().rows;
        old.migrate().unwrap();
        assert_ne!(
            old.query("::show_triggers self_state").unwrap().rows,
            old_guard
        );
        assert!(old.is_empty_for_restore().unwrap());
        let other = Store::open_in_memory().unwrap();
        super::super::migrate_through(&other.db, 0, 13).unwrap();
        assert_eq!(
            other.query("::show_triggers self_state").unwrap().rows,
            old_guard
        );
        assert!(super::super::migrate_through(&other.db, 13, 15).is_err());
    }

    #[test]
    fn authorized_legacy_adoption_grandfathers_only_marked_roots_without_rewriting_proofs() {
        use carrel_core::{
            conversation::{
                ConversationControl, ConversationParticipation, conversation_control_key,
            },
            private_library::PrivateFact,
            private_state::{StateRevision, StateValue},
        };
        let store = Store::open_in_memory().unwrap();
        super::super::migrate_through(&store.db, 0, 13).unwrap();
        let root = legacy_root(&store, "adopt");
        let original = ShareFact::Share(Box::new(root.clone())).to_bytes().unwrap();
        let master = Keypair::from_seed(&[71; 32]);
        let device = Keypair::from_seed(&[72; 32]);
        let cert = DeviceCert::new_at_micros(&master, device.public(), 1, None);
        store.bind_self_identity(&cert, &device).unwrap();
        up(&store.db).unwrap();
        let remote = Keypair::from_seed(&[74; 32]);
        store
            .accept_self_device_certificate(&DeviceCert::new_at_micros(
                &master,
                remote.public(),
                1,
                None,
            ))
            .unwrap();
        let off = StateRevision::sign(
            conversation_control_key(),
            StateValue::ConversationControl(ConversationControl::Policy {
                allow_unfollowed_groups: false,
            }),
            vec![],
            i64::MAX,
            &remote,
        )
        .unwrap();
        store
            .accept_self_state_revision(&off, remote.public())
            .unwrap();
        assert_eq!(
            store.conversation_participation(&root.id).unwrap(),
            ConversationParticipation::Pending
        );
        assert_eq!(
            store.adopt_legacy_social_conversations(1, &device).unwrap(),
            1
        );
        assert_eq!(
            store.adopt_legacy_social_conversations(1, &device).unwrap(),
            0
        );
        assert_eq!(
            store.conversation_participation(&root.id).unwrap(),
            ConversationParticipation::Admitted
        );
        assert!(!store.allow_unfollowed_group_conversations().unwrap());
        assert_eq!(
            store
                .social_record(&format!("share/{}", root.id))
                .unwrap()
                .unwrap()
                .to_bytes()
                .unwrap(),
            original
        );
        let later = Share::sign(
            "unmarked".into(),
            root.recipient,
            root.article.clone(),
            None,
            2,
            cert,
            &device,
        )
        .unwrap();
        store
            .accept_self_private_fact(
                &PrivateFact::Social(Box::new(ShareFact::Share(Box::new(later.clone())))),
                remote.public(),
            )
            .unwrap();
        assert_eq!(
            store.adopt_legacy_social_conversations(3, &device).unwrap(),
            0
        );
        assert_eq!(
            store.conversation_participation(&later.id).unwrap(),
            ConversationParticipation::Pending
        );
        assert!(store.social_contacts().unwrap().is_empty());
        store
            .leave_social_conversation(&root.id, 4, &device)
            .unwrap();
        store.adopt_legacy_social_conversations(5, &device).unwrap();
        assert_eq!(
            store.conversation_participation(&root.id).unwrap(),
            ConversationParticipation::Left
        );
    }

    #[test]
    fn failed_boundary_rolls_back_markers_and_trigger_then_retry_freezes_only_old_roots() {
        let store = Store::open_in_memory().unwrap();
        super::super::migrate_through(&store.db, 0, 13).unwrap();
        let root = legacy_root(&store, "before");
        let original = store.query("::show_triggers self_state").unwrap().rows;
        store.query("::set_triggers schema_version on put { ?[version] := _new[version,at,description],version==14 :assert none }").unwrap();
        assert!(up(&store.db).is_err());
        assert_eq!(store.current_schema_version().unwrap(), 13);
        assert!(!store.relation_exists("social_conversation_legacy").unwrap());
        assert_eq!(
            store.query("::show_triggers self_state").unwrap().rows,
            original
        );
        store.query("::set_triggers schema_version").unwrap();
        up(&store.db).unwrap();
        assert_eq!(store.current_schema_version().unwrap(), 14);
        assert!(
            store.self_identity().unwrap().is_none(),
            "migration cannot choose an identity"
        );
        assert!(
            store
                .query("?[register] := *self_state{register}")
                .unwrap()
                .rows
                .is_empty(),
            "migration cannot sign consent"
        );
        let markers = store
            .query("?[share_id,scope] := *social_conversation_legacy{share_id,scope}")
            .unwrap();
        assert_eq!(markers.rows.len(), 1);
        assert_eq!(markers.rows[0][0], DataValue::from(root.id));
        legacy_root(&store, "after");
        up(&store.db).unwrap();
        assert_eq!(
            store
                .query("?[share_id,scope] := *social_conversation_legacy{share_id,scope}")
                .unwrap()
                .rows,
            markers.rows
        );
        assert!(store.query_with_params("?[share_id,scope] <- [['new',$scope]] :put social_conversation_legacy {share_id=>scope}",BTreeMap::from([("scope".into(),DataValue::Bytes(vec![0]))])).is_err());
        assert!(store.query("?[share_id] := *social_conversation_legacy{share_id} :rm social_conversation_legacy {share_id}").is_err());
    }
}
