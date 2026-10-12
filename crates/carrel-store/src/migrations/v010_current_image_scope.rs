//! Scope cached images to the current body without rewriting signed archives.
use crate::{Result, StoreError, schema};
use carrel_core::private_library::{BlobReference, MAX_READING_BLOB_BYTES};
use cozo::{DataValue, DbInstance, JsonData, ScriptMutability, Validity};
use std::collections::BTreeMap;

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    let relations = run("::relations")?;
    let exists = |name: &str| {
        relations
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(name)))
    };
    if !exists("item_current_image_scope") {
        run(schema::CURRENT_IMAGE_SCOPE)?;
    }
    // Development hot reload can already have applied an earlier version-9
    // prototype. Preserve that relation and migrate its grants conservatively.
    if exists("item_image_scope") {
        let old = run("?[item_id,blob_id,images] := *item_image_scope{item_id,blob_id,images}")?;
        for row in old.rows {
            let mut images: Vec<BlobReference> = match &row[2] {
                DataValue::Json(value) => {
                    serde_json::from_value(value.0.clone()).map_err(|_| {
                        StoreError::InvalidInput {
                            context: "image scope migration",
                            message: "invalid prototype scope".into(),
                        }
                    })?
                }
                // ID-only prototypes cannot assert lengths; keep their original
                // rows and require a verified self snapshot or transfer to grant.
                DataValue::List(_) => Vec::new(),
                _ => {
                    return Err(StoreError::InvalidInput {
                        context: "image scope migration",
                        message: "invalid prototype scope shape".into(),
                    });
                }
            };
            if images.len() > 2048
                || images.iter().any(|image| {
                    image.blob_id.parse::<crate::blobs::BlobId>().is_err()
                        || image.byte_size > MAX_READING_BLOB_BYTES
                })
            {
                return Err(StoreError::InvalidInput {
                    context: "image scope migration",
                    message: "invalid prototype image reference".into(),
                });
            }
            images.sort_by(|a, b| {
                a.blob_id
                    .cmp(&b.blob_id)
                    .then(a.byte_size.cmp(&b.byte_size))
            });
            images.dedup();
            if images
                .windows(2)
                .any(|pair| pair[0].blob_id == pair[1].blob_id)
            {
                return Err(StoreError::InvalidInput {
                    context: "image scope migration",
                    message: "conflicting prototype image lengths".into(),
                });
            }
            db.run_script("?[item_id,blob_id,images] := not *item_current_image_scope{item_id:$item,blob_id:$blob},item_id=$item,blob_id=$blob,images=$images :put item_current_image_scope {item_id,blob_id => images}",BTreeMap::from([("item".into(),row[0].clone()),("blob".into(),row[1].clone()),("images".into(),DataValue::Json(JsonData(serde_json::to_value(images).map_err(|_|StoreError::InvalidInput{context:"image scope migration",message:"scope cannot be encoded".into()})?)))]),ScriptMutability::Mutable)?;
        }
    }
    let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
    db.run_script("?[version,applied_at,description] <- [[10,$now,'current-body image grants with portable signed scope']] :put schema_version {version => applied_at,description}",BTreeMap::from([("now".into(),DataValue::Validity(Validity::from((now,true))))]),ScriptMutability::Mutable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    fn version_nine() -> Store {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .query("?[version] <- [[10]] :rm schema_version {version}")
            .unwrap();
        store.query("::remove item_current_image_scope").unwrap();
        store
    }
    #[test]
    fn resumes_additive_scope_migration_and_keeps_id_only_prototypes_conservative() {
        let store = version_nine();
        store
            .query(":create item_image_scope {item_id:String,blob_id:String => images:[String]}")
            .unwrap();
        store.query("?[item_id,blob_id,images] <- [['item','body',['private-digest']]] :put item_image_scope {item_id,blob_id=>images}").unwrap();
        store.query(schema::CURRENT_IMAGE_SCOPE).unwrap();
        store.migrate().unwrap();
        store.migrate().unwrap();
        assert_eq!(store.current_schema_version().unwrap(), 10);
        assert_eq!(
            store.current_image_scope("item", "body").unwrap(),
            Some(vec![])
        );
        assert_eq!(
            store
                .query("?[images] := *item_image_scope{images}")
                .unwrap()
                .rows
                .len(),
            1
        );
    }
    #[test]
    fn migrates_verified_shaped_json_and_records_version_only_after_validation() {
        let store = version_nine();
        store
            .query(":create item_image_scope {item_id:String,blob_id:String => images:Json}")
            .unwrap();
        let image = BlobReference {
            blob_id: "c".repeat(64),
            byte_size: 42,
        };
        let put = |images: serde_json::Value| {
            store.query_with_params("?[item_id,blob_id,images] <- [['item','body',$images]] :put item_image_scope {item_id,blob_id=>images}",BTreeMap::from([("images".into(),DataValue::Json(JsonData(images)))]))
        };
        put(serde_json::json!([{"blob_id":"wrong","byte_size":42}])).unwrap();
        assert!(store.migrate().is_err());
        assert_eq!(store.current_schema_version().unwrap(), 9);
        put(serde_json::to_value(vec![image.clone(), image.clone()]).unwrap()).unwrap();
        store.migrate().unwrap();
        assert_eq!(
            store.current_image_scope("item", "body").unwrap(),
            Some(vec![image])
        );
        assert_eq!(store.current_schema_version().unwrap(), 10);
    }
}
