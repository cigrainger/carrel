//! Schema migration runner.

use cozo::DbInstance;

use crate::error::Result;

mod v001_initial;
mod v002_search;
mod v003_annotations;
mod v004_identity;
mod v005_causal_notes;
mod v006_private_state;
mod v007_portable_legacy_notes;
mod v008_private_library;
mod v009_private_sharing;
mod v010_current_image_scope;

/// The latest schema version known to this crate.
pub const CURRENT_SCHEMA_VERSION: u32 = 10;

/// Apply all migrations newer than `current_version`.
pub fn migrate(db: &DbInstance, current_version: u32) -> Result<()> {
    if current_version < 1 {
        v001_initial::up(db)?;
    }
    if current_version < 2 {
        v002_search::up(db)?;
    }
    if current_version < 3 {
        v003_annotations::up(db)?;
    }
    if current_version < 4 {
        v004_identity::up(db)?;
    }
    if current_version < 5 {
        v005_causal_notes::up(db)?;
    }
    if current_version < 6 {
        v006_private_state::up(db)?;
    }
    if current_version < 7 {
        v007_portable_legacy_notes::up(db)?;
    }
    if current_version < 8 {
        v008_private_library::up(db)?;
    }
    if current_version < 9 {
        v009_private_sharing::up(db)?;
    }
    if current_version < 10 {
        v010_current_image_scope::up(db)?;
    }

    Ok(())
}
