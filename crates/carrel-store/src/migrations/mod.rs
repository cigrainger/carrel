//! Schema migration runner.

use cozo::DbInstance;

use crate::error::Result;

mod v001_initial;
mod v002_search;

/// The latest schema version known to this crate.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Apply all migrations newer than `current_version`.
pub fn migrate(db: &DbInstance, current_version: u32) -> Result<()> {
    if current_version < 1 {
        v001_initial::up(db)?;
    }
    if current_version < 2 {
        v002_search::up(db)?;
    }

    Ok(())
}
