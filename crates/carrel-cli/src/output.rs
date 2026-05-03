//! Consistent terminal output helpers.

use cozo::{DataValue, NamedRows, Num, Validity};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{CliError, Result};

/// Print a named row set as either JSON or a compact table.
pub fn print_rows(rows: &NamedRows, as_json: bool) -> Result<()> {
    if as_json {
        print_json(&rows_to_json(rows)?)
    } else {
        print_table(rows);
        Ok(())
    }
}

/// Print a JSON value to stdout.
pub fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn rows_to_json(rows: &NamedRows) -> Result<Value> {
    let mut values = Vec::with_capacity(rows.rows.len());
    for row in &rows.rows {
        let mut object = Map::new();
        for (index, header) in rows.headers.iter().enumerate() {
            let value = row
                .get(index)
                .ok_or_else(|| CliError::internal("row had fewer values than headers"))?;
            object.insert(header.clone(), value_to_json(value));
        }
        values.push(Value::Object(object));
    }
    Ok(Value::Array(values))
}

fn print_table(rows: &NamedRows) {
    let rendered_rows = rows
        .rows
        .iter()
        .map(|row| row.iter().map(render_value).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    let widths = rows
        .headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rendered_rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(String::len)
                .fold(header.len(), usize::max)
        })
        .collect::<Vec<_>>();

    print_cells(&rows.headers, &widths);
    if rows.rows.is_empty() {
        println!("(no rows)");
        return;
    }

    for row in rendered_rows {
        print_cells(&row, &widths);
    }
}

fn print_cells(cells: &[String], widths: &[usize]) {
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            print!("  ");
        }
        let width = widths.get(index).copied().unwrap_or(cell.len());
        print!("{cell:<width$}");
    }
    println!();
}

fn render_value(value: &DataValue) -> String {
    match value {
        DataValue::Null => "null".to_string(),
        DataValue::Bool(value) => value.to_string(),
        DataValue::Num(Num::Int(value)) => value.to_string(),
        DataValue::Num(Num::Float(value)) => value.to_string(),
        DataValue::Str(value) => value.to_string(),
        DataValue::Bytes(value) => hex::encode(value),
        DataValue::Validity(value) => format_validity(value),
        DataValue::List(values) => {
            let values = values.iter().map(value_to_json).collect::<Vec<_>>();
            Value::Array(values).to_string()
        }
        other => value_to_json(other).to_string(),
    }
}

fn value_to_json(value: &DataValue) -> Value {
    match value {
        DataValue::Bytes(bytes) => Value::String(hex::encode(bytes)),
        DataValue::Validity(validity) => Value::String(format_validity(validity)),
        DataValue::List(values) => Value::Array(values.iter().map(value_to_json).collect()),
        DataValue::Set(values) => Value::Array(values.iter().map(value_to_json).collect()),
        DataValue::Json(value) => value.0.clone(),
        DataValue::Bot => Value::Null,
        other => Value::from(other.clone()),
    }
}

/// Render a Cozo validity as an RFC 3339 timestamp, with retractions marked.
pub fn format_validity(validity: &Validity) -> String {
    let micros = validity.timestamp.0.0;
    let seconds = micros.div_euclid(1_000_000);
    let subsec_nanos = u32::try_from(micros.rem_euclid(1_000_000) * 1_000)
        .expect("microsecond remainder fits in nanoseconds");

    let timestamp = OffsetDateTime::from_unix_timestamp(seconds)
        .and_then(|time| time.replace_nanosecond(subsec_nanos))
        .map_err(|error| error.to_string())
        .and_then(|time| time.format(&Rfc3339).map_err(|error| error.to_string()))
        .unwrap_or_else(|_| micros.to_string());

    if validity.is_assert.0 {
        timestamp
    } else {
        format!("{timestamp} (retracted)")
    }
}

#[cfg(test)]
mod tests {
    use cozo::{DataValue, NamedRows, Validity};

    use super::{format_validity, rows_to_json};

    #[test]
    fn json_rows_use_headers_and_hex_bytes() {
        let rows = NamedRows::new(
            vec!["name".to_string(), "pubkey".to_string()],
            vec![vec![
                DataValue::from("Chris"),
                DataValue::Bytes(vec![0xab, 0xcd]),
            ]],
        );

        let json = rows_to_json(&rows).unwrap();

        assert_eq!(json[0]["name"], "Chris");
        assert_eq!(json[0]["pubkey"], "abcd");
    }

    #[test]
    fn validity_formats_as_utc_time() {
        let rendered = format_validity(&Validity::from((1_700_000_000_000_000, true)));

        assert_eq!(rendered, "2023-11-14T22:13:20Z");
    }
}
