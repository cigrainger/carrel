//! Versioned private archives inside standard age passphrase encryption.
//!
//! Paths are selected by the native shell. This module accepts only fixed archive
//! entries and verified content hashes; it never extracts arbitrary archive paths.
use crate::{
    CURRENT_SCHEMA_VERSION, Store,
    blobs::{BlobId, BlobStore},
    keystore::{Keystore, KeystoreError},
};
use carrel_core::identity::{Keypair, PublicKey};
use cozo::{DataValue, DbInstance};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    str::FromStr,
};
use tempfile::TempDir;

const MAGIC: &[u8] = b"CARREL-BACKUP\0\x01";
const END: &[u8] = b"CARREL-END\0";
const MAX_MANIFEST: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_002;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_WORK: u8 = 18;
#[cfg(not(test))]
const WORK: u8 = 18;
#[cfg(test)]
const WORK: u8 = 4;

/// Failure at the encrypted archive boundary; no secret bodies enter errors.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// A version or schema is not supported by this build.
    #[error("backup version is not supported by this build")]
    Unsupported,
    /// Structural bounds or fixed archive entries were violated.
    #[error("invalid backup format")]
    Format,
    /// A digest, key binding or final authenticated stream did not validate.
    #[error("backup integrity validation failed")]
    Integrity,
    /// Passphrase encryption could not be opened within the resource limit.
    #[error("could not unlock backup; check its passphrase and file integrity")]
    Unlock,
    /// A meaningful existing archive may never be replaced by restore.
    #[error("restore requires an empty installation")]
    NotEmpty,
    /// Passwords are deliberately bounded at the native boundary as well.
    #[error("use a backup passphrase of at least twelve characters")]
    Passphrase,
    /// Local storage operation failed.
    #[error("backup file operation failed")]
    Io(#[from] io::Error),
    /// Snapshot operation failed.
    #[error("backup database operation failed")]
    Store(#[from] crate::StoreError),
    /// Identity material was unusable.
    #[error("backup identity material is invalid")]
    Key(#[from] KeystoreError),
    /// Encryption setup failed.
    #[error("backup encryption failed")]
    Encrypt(#[from] age::EncryptError),
}
/// Result at the encrypted archive boundary.
pub type Result<T> = std::result::Result<T, BackupError>;
impl From<cozo::Error> for BackupError {
    fn from(error: cozo::Error) -> Self {
        Self::Store(error.into())
    }
}

/// User-facing preview, produced only after complete authentication and validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupPreview {
    /// Source application build.
    pub app_version: String,
    /// Source schema version.
    pub schema_version: u32,
    /// Explicit creation time, never a measure of freshness or reading activity.
    pub created_at_micros: i64,
    /// Bound source master, when configured.
    pub master: Option<String>,
    /// Recovery availability, not a promise that the master passphrase is known.
    pub has_master_key: bool,
    /// Already-missing cached references remain explicit.
    pub cache_incomplete: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    preview: BackupPreview,
    source_device: Option<String>,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    bytes: u64,
    digest: String,
}

/// Validated isolated replacement, deleted automatically unless explicitly prepared.
pub struct RestoredArchive {
    /// Preview is safe to render; it contains no passphrase or private key.
    pub preview: BackupPreview,
    pub(crate) directory: TempDir,
}
impl RestoredArchive {
    /// Private staging root, used only by native activation orchestration.
    pub fn path(&self) -> &Path {
        self.directory.path()
    }
    /// Keep the staged archive for a separately journaled startup activation.
    pub fn keep(self) -> PathBuf {
        self.directory.keep()
    }
}

impl Store {
    /// Export a transaction snapshot; source files and active database are never copied live.
    pub fn create_backup(
        &self,
        blobs: &BlobStore,
        keys: &Keystore,
        output: impl Write,
        passphrase: &str,
        app_version: &str,
        created_at_micros: i64,
    ) -> Result<BackupPreview> {
        check_passphrase(passphrase)?;
        let scratch = tempfile::tempdir()?;
        let database = scratch.path().join("database.sqlite");
        let (binding, source_device, master_bytes) = {
            // Existing compound writes take these locks in this order. The
            // transaction snapshot then remains valid while immutable blobs copy.
            let _private = self.private_state_lock.lock().expect("backup state lock");
            let _annotation = self.annotation_lock.lock().expect("backup annotation lock");
            let _identity = self.identity_lock.lock().expect("backup identity lock");
            let _social = self.social_lock.lock().expect("backup social lock");
            self.db.backup_db(&database)?;
            let device = match keys.load_device() {
                Ok(device) => Some(*device.public()),
                Err(KeystoreError::ReadKeyFile { source, .. })
                    if source.kind() == io::ErrorKind::NotFound =>
                {
                    None
                }
                Err(error) => return Err(error.into()),
            };
            let binding = self.self_identity()?;
            if binding.is_some_and(|b| Some(b.device_pubkey) != device) {
                return Err(BackupError::Integrity);
            }
            (binding, device, keys.encrypted_master()?)
        };
        if let Some(bytes) = &master_bytes {
            fs::write(scratch.path().join("master.enc"), bytes)?;
        }
        let snapshot = Store::from_database(DbInstance::new("sqlite", &database, "")?);
        let master = binding
            .map(|b| b.master_pubkey)
            .or(snapshot.restore_origin()?.and_then(|o| o.master_pubkey));
        if let (Some(expected), Some(_)) = (master, &master_bytes)
            && keys.master_public_key()? != expected
        {
            return Err(BackupError::Integrity);
        }
        let mut files = BTreeMap::from([("database.sqlite".to_string(), database.clone())]);
        if master_bytes.is_some() {
            files.insert("master.enc".into(), scratch.path().join("master.enc"));
        }
        for (id, path) in blob_files(blobs.root())? {
            files.insert(format!("blobs/{id}"), path);
        }
        if files.len() > MAX_ENTRIES {
            return Err(BackupError::Format);
        }
        let incomplete = cache_incomplete(&snapshot, &files)?;
        let preview = BackupPreview {
            app_version: app_version.into(),
            schema_version: snapshot.current_schema_version()?,
            created_at_micros,
            master: master.map(|key| key.to_hex()),
            has_master_key: master_bytes.is_some(),
            cache_incomplete: incomplete,
        };
        drop(snapshot);
        let entries = files
            .iter()
            .map(|(name, path)| file_entry(name, path))
            .collect::<Result<Vec<_>>>()?;
        let manifest = Manifest {
            version: 1,
            preview: preview.clone(),
            source_device: source_device.map(|k| k.to_hex()),
            entries,
        };
        validate_manifest(&manifest)?;
        let encoded = serde_json::to_vec(&manifest).map_err(|_| BackupError::Format)?;
        if encoded.len() > MAX_MANIFEST {
            return Err(BackupError::Format);
        }
        let mut recipient = age::scrypt::Recipient::new(passphrase.to_owned().into());
        recipient.set_work_factor(WORK);
        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))?;
        let mut writer = encryptor.wrap_output(output)?;
        writer.write_all(MAGIC)?;
        writer.write_all(&(encoded.len() as u32).to_le_bytes())?;
        writer.write_all(&encoded)?;
        for entry in &manifest.entries {
            copy_verified(&mut File::open(&files[&entry.name])?, &mut writer, entry)?;
        }
        writer.write_all(END)?;
        writer.finish()?;
        Ok(preview)
    }
}

/// Decrypt, validate, and prepare an independent local archive without touching the destination.
pub fn open_backup(
    path: &Path,
    passphrase: &str,
    staging_parent: &Path,
) -> Result<RestoredArchive> {
    if passphrase.len() > 4096 {
        return Err(BackupError::Passphrase);
    }
    let mut file = File::open(path)?;
    check_age_header(&mut file)?;
    file.rewind()?;
    let decryptor = age::Decryptor::new(BufReader::new(file)).map_err(|_| BackupError::Unlock)?;
    let mut identity = age::scrypt::Identity::new(passphrase.to_owned().into());
    identity.set_max_work_factor(MAX_WORK);
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| BackupError::Unlock)?;
    let mut magic = vec![0; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .map_err(|_| BackupError::Integrity)?;
    if magic != MAGIC {
        return Err(BackupError::Format);
    }
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_MANIFEST {
        return Err(BackupError::Format);
    }
    let mut encoded = vec![0; length];
    reader.read_exact(&mut encoded)?;
    let manifest: Manifest = serde_json::from_slice(&encoded).map_err(|_| BackupError::Format)?;
    validate_manifest(&manifest)?;
    let directory = tempfile::Builder::new()
        .prefix("restore-")
        .tempdir_in(staging_parent)?;
    private_directory(&directory.path().join("blobs"))?;
    for entry in &manifest.entries {
        let destination = entry_path(directory.path(), &entry.name)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(destination)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        copy_verified(&mut reader, &mut output, entry)?;
        output.sync_all()?;
    }
    let mut end = vec![0; END.len()];
    reader
        .read_exact(&mut end)
        .map_err(|_| BackupError::Integrity)?;
    let mut extra = [0; 1];
    if end != END
        || reader
            .read(&mut extra)
            .map_err(|_| BackupError::Integrity)?
            != 0
    {
        return Err(BackupError::Integrity);
    }
    private_directory(&directory.path().join("store"))?;
    let store = Store::open(&directory.path().join("store"))?;
    store
        .db
        .restore_backup(directory.path().join("database.sqlite"))?;
    // Inspect the source schema before executing any migration or trigger from
    // the restored database. A supported manifest cannot bless another catalog.
    validate_catalog_version(&store, manifest.preview.schema_version)?;
    let restored_files = manifest
        .entries
        .iter()
        .map(|entry| {
            Ok((
                entry.name.clone(),
                entry_path(directory.path(), &entry.name)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    if cache_incomplete(&store, &restored_files)? != manifest.preview.cache_incomplete {
        return Err(BackupError::Integrity);
    }
    if store.current_schema_version()? != manifest.preview.schema_version {
        return Err(BackupError::Integrity);
    }
    let actual_master = store
        .self_identity()?
        .map(|b| b.master_pubkey)
        .or(store.restore_origin()?.and_then(|o| o.master_pubkey));
    if actual_master.map(|k| k.to_hex()) != manifest.preview.master {
        return Err(BackupError::Integrity);
    }
    store.migrate()?;
    validate_catalog(&store)?;
    let fresh = Keypair::generate();
    let keys = Keystore::open(&directory.path().join("keys"));
    keys.create_device(&fresh)?;
    if manifest.preview.has_master_key {
        let bytes = fs::read(directory.path().join("master.enc"))?;
        let key = keys.import_encrypted_master(&bytes)?;
        if actual_master.is_some_and(|expected| expected != key) {
            return Err(BackupError::Integrity);
        }
        fs::remove_file(directory.path().join("master.enc"))?;
    }
    let source = manifest
        .source_device
        .as_deref()
        .map(parse_key)
        .transpose()?;
    store.prepare_restored_archive(source, &fresh)?;
    drop(store);
    fs::remove_file(directory.path().join("database.sqlite"))?;
    Ok(RestoredArchive {
        preview: manifest.preview,
        directory,
    })
}

fn check_passphrase(value: &str) -> Result<()> {
    if value.chars().count() < 12 || value.len() > 4096 {
        Err(BackupError::Passphrase)
    } else {
        Ok(())
    }
}

fn cache_incomplete(store: &Store, files: &BTreeMap<String, PathBuf>) -> Result<bool> {
    use carrel_core::private_library::{BlobReference, PrivateFact};
    let mut incomplete = false;
    for row in store
        .query("?[blob_id,byte_size] := *item_content{blob_id,byte_size}")?
        .rows
    {
        let (DataValue::Str(id), DataValue::Num(cozo::Num::Int(size))) = (&row[0], &row[1]) else {
            return Err(BackupError::Integrity);
        };
        incomplete |= missing_cached_file(
            id,
            Some((*size).try_into().map_err(|_| BackupError::Integrity)?),
            files,
        )?;
    }
    for row in store
        .query("?[images] := *item_current_image_scope{images}")?
        .rows
    {
        let DataValue::Json(json) = &row[0] else {
            return Err(BackupError::Integrity);
        };
        for reference in serde_json::from_value::<Vec<BlobReference>>(json.0.clone())
            .map_err(|_| BackupError::Integrity)?
        {
            incomplete |=
                missing_cached_file(&reference.blob_id, Some(reference.byte_size), files)?;
        }
    }
    // Legacy/local selectors may precede adoption into the portable journal.
    let mut after = String::new();
    loop {
        let rows = store.query_with_params(
            "?[id,location] := *highlight{id,location},id>$after :sort id :limit 100",
            BTreeMap::from([("after".into(), DataValue::from(after.as_str()))]),
        )?;
        if rows.rows.is_empty() {
            break;
        }
        for row in rows.rows {
            let (DataValue::Str(id), DataValue::Json(location)) = (&row[0], &row[1]) else {
                return Err(BackupError::Integrity);
            };
            let location = serde_json::from_value::<carrel_core::annotation::HighlightLocation>(
                location.0.clone(),
            )
            .map_err(|_| BackupError::Integrity)?;
            incomplete |= missing_highlight_body(location, files)?;
            after = id.to_string();
        }
    }
    visit_private_records(store, |fact| {
        let references = match fact {
            PrivateFact::Item(revision) => revision.item.blobs(),
            PrivateFact::Highlight(highlight) => {
                incomplete |= missing_highlight_body(highlight.location, files)?;
                Vec::new()
            }
            PrivateFact::Social(fact) => {
                if let carrel_core::sharing::ShareFact::Share(share) = *fact {
                    share.article.blobs()
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        };
        for reference in references {
            incomplete |=
                missing_cached_file(&reference.blob_id, Some(reference.byte_size), files)?;
        }
        Ok(())
    })?;
    Ok(incomplete)
}

fn missing_highlight_body(
    location: carrel_core::annotation::HighlightLocation,
    files: &BTreeMap<String, PathBuf>,
) -> Result<bool> {
    let carrel_core::annotation::HighlightLocation::HtmlText(selector) = location;
    selector
        .content_blob_id
        .map_or(Ok(false), |id| missing_cached_file(&id, None, files))
}

fn missing_cached_file(
    id: &str,
    expected_size: Option<u64>,
    files: &BTreeMap<String, PathBuf>,
) -> Result<bool> {
    BlobId::from_str(id).map_err(|_| BackupError::Integrity)?;
    let Some(path) = files.get(&format!("blobs/{id}")) else {
        return Ok(true);
    };
    if let Some(size) = expected_size
        && fs::metadata(path)?.len() != size
    {
        return Err(BackupError::Integrity);
    }
    Ok(false)
}

fn visit_private_records(
    store: &Store,
    mut inspect: impl FnMut(carrel_core::private_library::PrivateFact) -> Result<()>,
) -> Result<()> {
    let mut after = String::new();
    loop {
        let rows = store.query_with_params(
            "?[key,encoded] := *self_fact_history{key,encoded},key>$after :sort key :limit 100",
            BTreeMap::from([("after".into(), DataValue::from(after.as_str()))]),
        )?;
        if rows.rows.is_empty() {
            return Ok(());
        }
        for row in rows.rows {
            let (DataValue::Str(key), DataValue::Bytes(bytes)) = (&row[0], &row[1]) else {
                return Err(BackupError::Integrity);
            };
            let fact = carrel_core::private_library::PrivateFact::from_bytes(bytes)
                .map_err(|_| BackupError::Integrity)?;
            if fact.key().map_err(|_| BackupError::Integrity)? != key.as_str() {
                return Err(BackupError::Integrity);
            }
            inspect(fact)?;
            after = key.to_string();
        }
    }
}
fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn parse_key(value: &str) -> Result<PublicKey> {
    let bytes = hex::decode(value).map_err(|_| BackupError::Format)?;
    let key = PublicKey::from_bytes(bytes.try_into().map_err(|_| BackupError::Format)?);
    key.validate().map_err(|_| BackupError::Format)?;
    Ok(key)
}
fn validate_manifest(m: &Manifest) -> Result<()> {
    if m.version != 1 || !matches!(m.preview.schema_version, 13 | 14) {
        return Err(BackupError::Unsupported);
    }
    if m.entries.len() > MAX_ENTRIES || m.preview.app_version.len() > 128 {
        return Err(BackupError::Format);
    }
    if let Some(key) = &m.preview.master {
        parse_key(key)?;
    }
    if let Some(key) = &m.source_device {
        parse_key(key)?;
    }
    let mut total = 0u64;
    let mut names = BTreeSet::new();
    let mut previous = "";
    for e in &m.entries {
        entry_path(Path::new("."), &e.name)?;
        if e.name.as_str() <= previous
            || (e.bytes == 0 && !e.name.starts_with("blobs/"))
            || e.bytes > MAX_TOTAL
            || !valid_hash(&e.digest)
        {
            return Err(BackupError::Format);
        }
        if e.name == "master.enc" && e.bytes > 64 * 1024 {
            return Err(BackupError::Format);
        }
        if let Some(hash) = e.name.strip_prefix("blobs/")
            && hash != e.digest
        {
            return Err(BackupError::Integrity);
        }
        total = total.checked_add(e.bytes).ok_or(BackupError::Format)?;
        previous = &e.name;
        names.insert(e.name.as_str());
    }
    if total > MAX_TOTAL
        || !names.contains("database.sqlite")
        || names.contains("master.enc") != m.preview.has_master_key
    {
        return Err(BackupError::Format);
    }
    Ok(())
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn entry_path(root: &Path, name: &str) -> Result<PathBuf> {
    match name {
        "database.sqlite" | "master.enc" => Ok(root.join(name)),
        _ => {
            let hash = name
                .strip_prefix("blobs/")
                .filter(|h| valid_hash(h))
                .ok_or(BackupError::Format)?;
            Ok(root.join("blobs").join(&hash[..2]).join(hash))
        }
    }
}
fn blob_files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut output = Vec::new();
    if !root.exists() {
        return Ok(output);
    }
    for prefix in fs::read_dir(root)? {
        let prefix = prefix?;
        if !prefix
            .file_name()
            .to_str()
            .is_some_and(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            continue;
        }
        if !prefix.file_type()?.is_dir() {
            return Err(BackupError::Integrity);
        }
        for file in fs::read_dir(prefix.path())? {
            let file = file?;
            let name = file.file_name();
            let Some(hash) = name.to_str().filter(|s| valid_hash(s)) else {
                continue;
            };
            if !file.file_type()?.is_file()
                || !hash.starts_with(&prefix.file_name().to_string_lossy().to_string())
            {
                return Err(BackupError::Integrity);
            }
            output.push((hash.into(), file.path()));
            if output.len() > MAX_ENTRIES {
                return Err(BackupError::Format);
            }
        }
    }
    Ok(output)
}
fn file_entry(name: &str, path: &Path) -> Result<Entry> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(Entry {
        name: name.into(),
        bytes,
        digest: hasher.finalize().to_hex().to_string(),
    })
}
fn copy_verified(input: &mut impl Read, output: &mut impl Write, entry: &Entry) -> Result<()> {
    let mut remaining = entry.bytes;
    let mut buffer = [0u8; 64 * 1024];
    let mut hasher = blake3::Hasher::new();
    while remaining > 0 {
        let size = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..size])?;
        hasher.update(&buffer[..size]);
        output.write_all(&buffer[..size])?;
        remaining -= size as u64;
    }
    if hasher.finalize().to_hex().as_str() != entry.digest {
        return Err(BackupError::Integrity);
    }
    Ok(())
}
fn check_age_header(file: &mut File) -> Result<()> {
    if file.metadata()?.len() > MAX_TOTAL + MAX_MANIFEST as u64 + MAX_TOTAL / 1000 {
        return Err(BackupError::Format);
    }
    let mut reader = BufReader::new(file.take(8192));
    let mut line = String::new();
    let mut recipients = 0;
    reader.read_line(&mut line)?;
    if line != "age-encryption.org/v1\n" {
        return Err(BackupError::Format);
    }
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(BackupError::Format);
        }
        if line.starts_with("-> ") {
            if !line.starts_with("-> scrypt ") {
                return Err(BackupError::Format);
            }
            recipients += 1;
        }
        if line.starts_with("--- ") {
            return if recipients == 1 {
                Ok(())
            } else {
                Err(BackupError::Format)
            };
        }
    }
}
fn validate_catalog(store: &Store) -> Result<()> {
    validate_catalog_version(store, CURRENT_SCHEMA_VERSION)
}

fn validate_catalog_version(store: &Store, version: u32) -> Result<()> {
    if !matches!(version, 13 | 14) {
        return Err(BackupError::Unsupported);
    }
    if store.current_schema_version()? != version {
        return Err(BackupError::Integrity);
    }
    let expected = Store::open_in_memory()?;
    crate::migrations::migrate_through(&expected.db, 0, version)?;
    let catalog = expected.query("::relations")?;
    if store.query("::relations")?.rows != catalog.rows {
        return Err(BackupError::Integrity);
    }
    for row in catalog.rows {
        let DataValue::Str(name) = &row[0] else {
            return Err(BackupError::Integrity);
        };
        if name.contains(':') {
            continue;
        }
        for command in ["columns", "show_triggers"] {
            let query = format!("::{command} {name}");
            if store.query(&query)?.rows != expected.query(&query)?.rows {
                return Err(BackupError::Integrity);
            }
        }
    }
    // Verify every immutable portable record before restoring any network identity.
    visit_private_records(store, |_| Ok(()))?;
    Ok(())
}

/// Recheck an operation-owned staged archive before or during startup activation.
pub fn validate_staged(root: &Path) -> Result<()> {
    for name in ["store", "keys", "blobs"] {
        let metadata = fs::symlink_metadata(root.join(name))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(BackupError::Integrity);
        }
    }
    let store = Store::open(&root.join("store"))?;
    validate_catalog(&store)?;
    let origin = store.restore_origin()?.ok_or(BackupError::Integrity)?;
    let keys = Keystore::open(&root.join("keys"));
    let device = keys.load_device()?;
    if store.self_identity()?.is_some()
        || origin.device_pubkey != *device.public()
        || origin.legacy_devices.contains(device.public())
    {
        return Err(BackupError::Integrity);
    }
    if keys.encrypted_master()?.is_some()
        && origin
            .master_pubkey
            .is_some_and(|master| keys.master_public_key().ok() != Some(master))
    {
        return Err(BackupError::Integrity);
    }
    for (hash, path) in blob_files(&root.join("blobs"))? {
        if file_entry(&format!("blobs/{hash}"), &path)?.digest != hash {
            return Err(BackupError::Integrity);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
