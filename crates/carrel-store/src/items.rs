//! Typed item read APIs used by CLI and future application commands.

use std::collections::BTreeMap;

use cozo::{DataValue, Num};

use crate::{Result, Store, StoreError};

/// Base item metadata plus its readable content reference, if cached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDetail {
    /// Stable item id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Canonical URL, when known.
    pub primary_url: Option<String>,
    /// Short summary or feed description, when known.
    pub summary: Option<String>,
    /// Cached readable content metadata.
    pub readable: Option<ItemReadableContent>,
}

/// `item_content` metadata for the readable HTML blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemReadableContent {
    /// Blob id containing readable HTML.
    pub blob_id: String,
    /// Extractor that produced the content.
    pub extracted_with: Option<String>,
    /// Stored byte size.
    pub byte_size: i64,
}

impl Store {
    /// Return item metadata and readable content reference by id.
    pub fn get_item_detail(&self, id: &str) -> Result<Option<ItemDetail>> {
        let rows = self.query_with_params(
            r#"
            ?[id, title, primary_url, summary] :=
                *item{id, title, primary_url, summary},
                id = $id
            :limit 1
            "#,
            BTreeMap::from([("id".to_string(), DataValue::from(id))]),
        )?;

        let Some(row) = rows.rows.first() else {
            return Ok(None);
        };

        Ok(Some(ItemDetail {
            id: value_as_string(required(row, 0, "item.id")?)?,
            title: value_as_string(required(row, 1, "item.title")?)?,
            primary_url: optional_string(required(row, 2, "item.primary_url")?)?,
            summary: optional_string(required(row, 3, "item.summary")?)?,
            readable: self.get_readable_content(id)?,
        }))
    }

    fn get_readable_content(&self, id: &str) -> Result<Option<ItemReadableContent>> {
        let rows = self.query_with_params(
            r#"
            ?[blob_id, extracted_with, byte_size] :=
                *item_content{item_id, format, blob_id, extracted_with, byte_size},
                item_id = $id,
                format = 'html_readable'
            :limit 1
            "#,
            BTreeMap::from([("id".to_string(), DataValue::from(id))]),
        )?;

        rows.rows
            .first()
            .map(|row| {
                Ok(ItemReadableContent {
                    blob_id: value_as_string(required(row, 0, "item_content.blob_id")?)?,
                    extracted_with: optional_string(required(
                        row,
                        1,
                        "item_content.extracted_with",
                    )?)?,
                    byte_size: value_as_i64(required(row, 2, "item_content.byte_size")?)?,
                })
            })
            .transpose()
    }
}

fn required<'a>(
    row: &'a [DataValue],
    index: usize,
    context: &'static str,
) -> Result<&'a DataValue> {
    row.get(index).ok_or_else(|| StoreError::UnexpectedValue {
        context,
        value: "missing column".to_string(),
    })
}

fn value_as_string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        other => Err(unexpected("string", other)),
    }
}

fn optional_string(value: &DataValue) -> Result<Option<String>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Str(value) => Ok(Some(value.to_string())),
        other => Err(unexpected("optional string", other)),
    }
}

fn value_as_i64(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Num(Num::Int(value)) => Ok(*value),
        other => Err(unexpected("integer", other)),
    }
}

fn unexpected(context: &'static str, value: &DataValue) -> StoreError {
    StoreError::UnexpectedValue {
        context,
        value: format!("{value:?}"),
    }
}
