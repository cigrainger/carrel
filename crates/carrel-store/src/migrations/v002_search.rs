//! Resumable creation of local, derived full-text indexes.

use std::collections::BTreeMap;

use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::{Result, schema};

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    let relations = run("::relations")?;
    if !relations
        .rows
        .iter()
        .any(|row| row.first() == Some(&DataValue::from("item_search")))
    {
        run(schema::CREATE_SEARCH_SCHEMA)?;
    }
    for (relation, index, script) in [
        // Cozo 0.7.6 gates deletion of replaced FTS terms on has_indices in
        // query/stored.rs. A primary-key index enables the correct update path.
        (
            "item",
            "search_updates",
            "::index create item:search_updates {id}",
        ),
        (
            "item_search",
            "search_updates",
            "::index create item_search:search_updates {item_id}",
        ),
        ("item", "title_words", schema::CREATE_TITLE_SEARCH_INDEX),
        (
            "item_search",
            "body_words",
            schema::CREATE_BODY_SEARCH_INDEX,
        ),
    ] {
        let indexes = run(&format!("::indices {relation}"))?;
        if !indexes
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(index)))
        {
            run(script)?;
        }
    }
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000) as i64;
    db.run_script(
        "?[version, applied_at, description] <- [[2, $now, 'local full-text search']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}
