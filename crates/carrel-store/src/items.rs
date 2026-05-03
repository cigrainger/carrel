//! Typed item read APIs used by CLI and future application commands.

use std::collections::BTreeMap;

use carrel_core::shape::Shape;
use cozo::{DataValue, Num};
use time::OffsetDateTime;

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

/// Readable content reference for shape recomputation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemReadableContentRef {
    /// Stable item id.
    pub item_id: String,
    /// Blob id containing readable HTML.
    pub blob_id: String,
}

/// Stored shape facts plus their detection timestamp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemShapeRecord {
    /// Stable item id.
    pub item_id: String,
    /// Detected structural shape facts.
    pub shape: Shape,
    /// Detection timestamp in Unix microseconds.
    pub detected_at_micros: i64,
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

    /// Persist shape facts for an item.
    pub fn put_item_shape(&self, item_id: &str, shape: &Shape) -> Result<()> {
        self.query_with_params(
            r#"
            ?[item_id, has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at] :=
                item_id = $item_id,
                has_video_embed = $has_video_embed,
                has_audio_embed = $has_audio_embed,
                is_link_roundup = $is_link_roundup,
                is_long_form = $is_long_form,
                is_short = $is_short,
                has_code = $has_code,
                has_math = $has_math,
                detected_at = $detected_at
            :put item_shape {item_id => has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at}
            "#,
            BTreeMap::from([
                ("item_id".to_string(), DataValue::from(item_id)),
                (
                    "has_video_embed".to_string(),
                    DataValue::Bool(shape.has_video_embed),
                ),
                (
                    "has_audio_embed".to_string(),
                    DataValue::Bool(shape.has_audio_embed),
                ),
                (
                    "is_link_roundup".to_string(),
                    DataValue::Bool(shape.is_link_roundup),
                ),
                (
                    "is_long_form".to_string(),
                    DataValue::Bool(shape.is_long_form),
                ),
                ("is_short".to_string(), DataValue::Bool(shape.is_short)),
                ("has_code".to_string(), DataValue::Bool(shape.has_code)),
                ("has_math".to_string(), DataValue::Bool(shape.has_math)),
                ("detected_at".to_string(), validity_now()),
            ]),
        )?;
        Ok(())
    }

    /// Return stored shape facts for an item, if present.
    pub fn get_item_shape(&self, item_id: &str) -> Result<Option<ItemShapeRecord>> {
        let rows = self.query_with_params(
            r#"
            ?[item_id, has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at] :=
                *item_shape{item_id, has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at},
                item_id = $item_id
            :limit 1
            "#,
            BTreeMap::from([("item_id".to_string(), DataValue::from(item_id))]),
        )?;

        rows.rows
            .first()
            .map(|row| decode_shape_row(row))
            .transpose()
    }

    /// List items that have a readable HTML content blob.
    pub fn list_items_with_readable_content(&self) -> Result<Vec<ItemReadableContentRef>> {
        let rows = self.query(
            r#"
            ?[item_id, blob_id] :=
                *item_content{item_id, format, blob_id},
                format = 'html_readable'
            :sort item_id
            "#,
        )?;

        rows.rows
            .iter()
            .map(|row| {
                Ok(ItemReadableContentRef {
                    item_id: value_as_string(required(row, 0, "item_content.item_id")?)?,
                    blob_id: value_as_string(required(row, 1, "item_content.blob_id")?)?,
                })
            })
            .collect()
    }
}

fn decode_shape_row(row: &[DataValue]) -> Result<ItemShapeRecord> {
    Ok(ItemShapeRecord {
        item_id: value_as_string(required(row, 0, "item_shape.item_id")?)?,
        shape: Shape {
            has_video_embed: value_as_bool(required(row, 1, "item_shape.has_video_embed")?)?,
            has_audio_embed: value_as_bool(required(row, 2, "item_shape.has_audio_embed")?)?,
            is_link_roundup: value_as_bool(required(row, 3, "item_shape.is_link_roundup")?)?,
            is_long_form: value_as_bool(required(row, 4, "item_shape.is_long_form")?)?,
            is_short: value_as_bool(required(row, 5, "item_shape.is_short")?)?,
            has_code: value_as_bool(required(row, 6, "item_shape.has_code")?)?,
            has_math: value_as_bool(required(row, 7, "item_shape.has_math")?)?,
        },
        detected_at_micros: value_as_validity_micros(required(row, 8, "item_shape.detected_at")?)?,
    })
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

fn value_as_bool(value: &DataValue) -> Result<bool> {
    match value {
        DataValue::Bool(value) => Ok(*value),
        other => Err(unexpected("bool", other)),
    }
}

fn value_as_validity_micros(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Validity(value) => Ok(value.timestamp.0.0),
        other => Err(unexpected("validity", other)),
    }
}

fn unexpected(context: &'static str, value: &DataValue) -> StoreError {
    StoreError::UnexpectedValue {
        context,
        value: format!("{value:?}"),
    }
}

fn validity_now() -> DataValue {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    let micros = i64::try_from(nanos / 1_000).unwrap_or(i64::MAX);
    DataValue::Validity(cozo::Validity::from((micros, true)))
}
