//! Add conversation projections without changing any original signature.
use crate::{Result, StoreError};
use carrel_core::{private_library::PrivateFact, sharing::ShareFact};
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str, params| db.run_script(script, params, ScriptMutability::Mutable);
    for (name, script) in [
        (
            "social_pending",
            ":create social_pending {key:String=>encoded:Bytes}",
        ),
        (
            "social_reply",
            ":create social_reply {id:String=>key:String,share_id:String,parent:String?}",
        ),
        (
            "social_group",
            ":create social_group {id:String=>name:String,members:[Bytes]}",
        ),
    ] {
        if !run("::relations", BTreeMap::new())?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(name)))
        {
            run(script, BTreeMap::new())?;
        }
    }
    let mut after = String::new();
    loop {
        let rows = run(
            "?[key,encoded] := *social_record{key,encoded},key>$after :sort key :limit 50",
            BTreeMap::from([("after".into(), DataValue::from(after.as_str()))]),
        )?;
        if rows.rows.is_empty() {
            break;
        }
        for row in rows.rows {
            let DataValue::Str(key) = &row[0] else {
                return Err(corrupt());
            };
            after = key.to_string();
            let DataValue::Bytes(encoded) = &row[1] else {
                return Err(corrupt());
            };
            let fact = ShareFact::from_bytes(encoded)?;
            if key.as_str() != fact.key()? {
                return Err(corrupt());
            }
            let private = PrivateFact::Social(Box::new(fact.clone()));
            run(
                "{ ?[key,encoded] <- [[$key,$encoded]] :put self_fact_history {key=>encoded} } { ?[key,encoded] <- [[$key,$encoded]] :put self_fact_outbox {key=>encoded} }",
                BTreeMap::from([
                    ("key".into(), DataValue::from(private.key()?)),
                    ("encoded".into(), DataValue::Bytes(private.to_bytes()?)),
                ]),
            )?;
        }
    }
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    run(
        "?[version,applied_at,description] <- [[11,$now,'private profiles, fixed group audiences and conversation ancestry']] :put schema_version {version=>applied_at,description}",
        BTreeMap::from([(
            "now".into(),
            DataValue::Validity(Validity::from((now, true))),
        )]),
    )?;
    Ok(())
}
fn corrupt() -> StoreError {
    StoreError::InvalidInput {
        context: "conversation migration",
        message: "invalid social history".into(),
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

    #[test]
    fn interrupted_conversation_migration_keeps_legacy_bytes_and_rebuilds_private_history() {
        for prefix in 0..=3 {
            let store = Store::open_in_memory().unwrap();
            super::super::v001_initial::up(&store.db).unwrap();
            super::super::v002_search::up(&store.db).unwrap();
            super::super::v003_annotations::up(&store.db).unwrap();
            super::super::v004_identity::up(&store.db).unwrap();
            super::super::v005_causal_notes::up(&store.db).unwrap();
            super::super::v006_private_state::up(&store.db).unwrap();
            super::super::v007_portable_legacy_notes::up(&store.db).unwrap();
            super::super::v008_private_library::up(&store.db).unwrap();
            super::super::v009_private_sharing::up(&store.db).unwrap();
            super::super::v010_current_image_scope::up(&store.db).unwrap();
            let master = Keypair::from_seed(&[81; 32]);
            let device = Keypair::from_seed(&[82; 32]);
            let friend = Keypair::from_seed(&[83; 32]);
            let certificate = DeviceCert::new_at_micros(&master, device.public(), 1, None);
            store.bind_self_identity(&certificate, &device).unwrap();
            let root = Share::sign(
                "legacy".into(),
                *friend.public(),
                SavedItem {
                    id: "a".repeat(64),
                    kind: "article".into(),
                    title: "Café 文".into(),
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
                42,
                certificate,
                &device,
            )
            .unwrap();
            let fact = ShareFact::Share(Box::new(root));
            let original = fact.to_bytes().unwrap();
            store.query_with_params("?[key,encoded,author,recipient,share_id,kind,created_at] <- [[$key,$encoded,$author,$recipient,$share_id,$kind,$created]] :put social_record {key=>encoded,author,recipient,share_id,kind,created_at}",crate::sharing::record_params(&fact).unwrap()).unwrap();
            for script in [
                ":create social_pending {key:String=>encoded:Bytes}",
                ":create social_reply {id:String=>key:String,share_id:String,parent:String?}",
                ":create social_group {id:String=>name:String,members:[Bytes]}",
            ]
            .into_iter()
            .take(prefix)
            {
                store.query(script).unwrap();
            }
            assert_eq!(store.current_schema_version().unwrap(), 10);
            store.migrate().unwrap();
            assert_eq!(store.current_schema_version().unwrap(), 11);
            assert_eq!(
                store
                    .social_record(&fact.key().unwrap())
                    .unwrap()
                    .unwrap()
                    .to_bytes()
                    .unwrap(),
                original
            );
            let portable = PrivateFact::Social(Box::new(fact));
            let key = portable.key().unwrap();
            let rows = store
                .query_with_params(
                    "?[encoded] := *self_fact_history{key:$key,encoded}",
                    BTreeMap::from([("key".into(), DataValue::from(key))]),
                )
                .unwrap();
            assert_eq!(
                rows.rows[0][0],
                DataValue::Bytes(portable.to_bytes().unwrap())
            );
            assert!(
                store.social_contacts().unwrap().is_empty(),
                "migration never follows a person"
            );
            assert!(store.social_groups().unwrap().is_empty());
            up(&store.db).unwrap();
            assert_eq!(store.current_schema_version().unwrap(), 11);
        }
    }
}
