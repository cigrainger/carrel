//! Local, rebuildable text derived from the current readable content blob.

use std::collections::BTreeMap;

use cozo::DataValue;

use crate::{Result, Store, StoreError};

/// Increment when plain-text extraction changes and existing text needs rebuilding.
pub const SEARCH_EXTRACTOR_VERSION: i64 = 1;

/// Current readable content whose local text index needs rebuilding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchContent {
    /// Stable article identity.
    pub item_id: String,
    /// Content-addressed readable HTML blob.
    pub blob_id: String,
}

impl Store {
    /// Find available content references with missing or outdated derived text.
    pub fn pending_search_content(&self) -> Result<Vec<SearchContent>> {
        let rows = self.query_with_params(r#"
            current[id, blob] := *item_content{item_id: id, format: 'html_readable', blob_id: blob},
                *item{id}, not *entity_tombstone{id}
            indexed[id, blob] := *item_search{item_id: id, blob_id: blob, extractor_version: $version}
            ?[id, blob] := current[id, blob], not indexed[id, blob]
        "#, BTreeMap::from([("version".into(), DataValue::from(SEARCH_EXTRACTOR_VERSION))]))?;
        rows.rows
            .into_iter()
            .map(|row| {
                let [DataValue::Str(id), DataValue::Str(blob)] = row.as_slice() else {
                    return Err(StoreError::UnexpectedValue {
                        context: "search content reference",
                        value: format!("{row:?}"),
                    });
                };
                Ok(SearchContent {
                    item_id: id.to_string(),
                    blob_id: blob.to_string(),
                })
            })
            .collect()
    }

    /// Index derived text only if its source blob is still current and alive.
    ///
    /// Returns false when content changed or the item was removed during extraction.
    pub fn put_search_text(&self, item_id: &str, blob_id: &str, text: &str) -> Result<bool> {
        // Touch the unchanged content row to include its key in transaction
        // conflict detection. Merely reading it would allow a concurrent content
        // replacement to race a derived write into a different relation.
        let rows = self.query_with_params(r#"
            {
                ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] :=
                    *item{id: item_id}, item_id = $id, not *entity_tombstone{id: item_id},
                    *item_content{item_id, format, blob_id, fetched_at, extracted_with, byte_size},
                    format = 'html_readable', blob_id = $blob
                :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
            }
            {
            ?[item_id, blob_id, text, extractor_version] :=
                *item{id: item_id}, item_id = $id, not *entity_tombstone{id: item_id},
                *item_content{item_id, format: 'html_readable', blob_id}, blob_id = $blob,
                text = $text, extractor_version = $version
            :put item_search {item_id => blob_id, text, extractor_version}
            :returning
            }
        "#, BTreeMap::from([
            ("id".into(), DataValue::from(item_id)),
            ("blob".into(), DataValue::from(blob_id)),
            ("text".into(), DataValue::from(text)),
            ("version".into(), DataValue::from(SEARCH_EXTRACTOR_VERSION)),
        ]))?;
        Ok(!rows.rows.is_empty())
    }
}
