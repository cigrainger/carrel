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
mod v011_conversations;
mod v012_send_batches;
mod v013_restore_origin;
mod v014_conversation_consent;

/// The latest schema version known to this crate.
pub const CURRENT_SCHEMA_VERSION: u32 = 14;

/// Apply all migrations newer than `current_version`.
pub fn migrate(db: &DbInstance, current_version: u32) -> Result<()> {
    migrate_through(db, current_version, CURRENT_SCHEMA_VERSION)
}

/// Reconstruct an exact known historical catalog before validating a backup.
/// Historical migrations and their trigger definitions must remain unchanged.
pub(crate) fn migrate_through(
    db: &DbInstance,
    current_version: u32,
    target_version: u32,
) -> Result<()> {
    if target_version > CURRENT_SCHEMA_VERSION || current_version > target_version {
        return Err(crate::StoreError::InvalidSchemaVersion(i64::from(
            target_version,
        )));
    }
    if current_version < 1 && target_version >= 1 {
        v001_initial::up(db)?;
    }
    if current_version < 2 && target_version >= 2 {
        v002_search::up(db)?;
    }
    if current_version < 3 && target_version >= 3 {
        v003_annotations::up(db)?;
    }
    if current_version < 4 && target_version >= 4 {
        v004_identity::up(db)?;
    }
    if current_version < 5 && target_version >= 5 {
        v005_causal_notes::up(db)?;
    }
    if current_version < 6 && target_version >= 6 {
        v006_private_state::up(db)?;
    }
    if current_version < 7 && target_version >= 7 {
        v007_portable_legacy_notes::up(db)?;
    }
    if current_version < 8 && target_version >= 8 {
        v008_private_library::up(db)?;
    }
    if current_version < 9 && target_version >= 9 {
        v009_private_sharing::up(db)?;
    }
    if current_version < 10 && target_version >= 10 {
        v010_current_image_scope::up(db)?;
    }
    if current_version < 11 && target_version >= 11 {
        v011_conversations::up(db)?;
    }
    if current_version < 12 && target_version >= 12 {
        v012_send_batches::up(db)?;
    }
    if current_version < 13 && target_version >= 13 {
        v013_restore_origin::up(db)?;
    }
    if current_version < 14 && target_version >= 14 {
        v014_conversation_consent::up(db)?;
    }

    Ok(())
}
