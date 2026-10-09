//! carrel-store: Cozo-backed persistence for Carrel facts.
//!
//! This crate owns the local store boundary. Higher layers use its typed API;
//! lower layers do not know that persistence exists.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod blobs;
mod error;
pub mod feeds;
pub mod highlights;
pub mod ids;
pub mod items;
pub mod keystore;
pub mod library;
mod migrations;
pub mod schema;
pub mod search;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};

pub use crate::error::{Result, StoreError};

/// The current schema version installed by this crate.
pub const CURRENT_SCHEMA_VERSION: u32 = migrations::CURRENT_SCHEMA_VERSION;

/// Handle to the local Carrel store.
///
/// `Store` is intentionally thin in this brief. Feature briefs will add typed
/// accessors as they need them; for now the CLI and tests can use raw CozoScript
/// through `query`.
#[derive(Clone)]
pub struct Store {
    db: DbInstance,
    // Serializes local annotation read/modify/write operations across clones.
    // The facts remain in Cozo; this lock contains no durable state.
    annotation_lock: Arc<Mutex<()>>,
}

impl Store {
    /// Open a persistent store at `path` using Cozo's RocksDB backend.
    pub fn open(path: &Path) -> Result<Self> {
        fs::create_dir_all(path).map_err(|source| StoreError::CreateStoreDir {
            path: path.to_path_buf(),
            source,
        })?;

        let db = DbInstance::new("rocksdb", path, "")?;
        Ok(Self {
            db,
            annotation_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Open an in-memory store for tests and short-lived tools.
    pub fn open_in_memory() -> Result<Self> {
        let db = DbInstance::new("mem", "", "")?;
        Ok(Self {
            db,
            annotation_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Return the newest applied schema version, or zero for an unmigrated DB.
    pub fn current_schema_version(&self) -> Result<u32> {
        if !self.relation_exists("schema_version")? {
            return Ok(0);
        }

        let rows = self.query(
            r#"
            ?[version] := *schema_version{version}
            :sort -version
            :limit 1
            "#,
        )?;

        let Some(row) = rows.rows.first() else {
            return Ok(0);
        };

        match row.first() {
            Some(DataValue::Num(cozo::Num::Int(version))) => {
                u32::try_from(*version).map_err(|_| StoreError::InvalidSchemaVersion(*version))
            }
            Some(other) => Err(StoreError::UnexpectedValue {
                context: "schema_version.version",
                value: format!("{other:?}"),
            }),
            None => Ok(0),
        }
    }

    /// Apply all pending schema migrations.
    pub fn migrate(&self) -> Result<()> {
        migrations::migrate(&self.db, self.current_schema_version()?)
    }

    /// Run a CozoScript query with no parameters.
    pub fn query(&self, datalog: &str) -> Result<NamedRows> {
        self.query_with_params(datalog, BTreeMap::new())
    }

    /// Run a CozoScript query with parameters.
    pub fn query_with_params(
        &self,
        datalog: &str,
        params: BTreeMap<String, DataValue>,
    ) -> Result<NamedRows> {
        Ok(self
            .db
            .run_script(datalog, params, ScriptMutability::Mutable)?)
    }

    fn relation_exists(&self, name: &str) -> Result<bool> {
        let rows = self.query("::relations")?;
        Ok(rows.rows.iter().any(|row| {
            matches!(
                row.first(),
                Some(DataValue::Str(relation_name)) if relation_name.as_str() == name
            )
        }))
    }
}
