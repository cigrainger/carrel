//! Journaled activation runs before opening the app store or starting workers.
use crate::{AppError, Result, config::InstallPaths};
use carrel_store::{
    Store,
    backup::{BackupError, RestoredArchive},
    keystore::Keystore,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const RECORD: &str = ".carrel-restore.json";
const PARTS: [&str; 3] = ["store", "keys", "blobs"];
// Native preparation/cancellation run on blocking workers. Serialize their
// eligibility checks and publication without holding the UI's review mutex.
static ACTIVATION_OPERATION: Mutex<()> = Mutex::new(());

/// Install only a fully written backup; errors before persistence preserve its predecessor.
pub(crate) fn write_backup(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let parent = path.parent().ok_or(BackupError::Format)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(file_error)?;
    write(temporary.as_file_mut())?;
    temporary.flush().map_err(file_error)?;
    temporary.as_file().sync_all().map_err(file_error)?;
    temporary
        .persist(path)
        .map_err(|error| file_error(error.error))?;
    sync_dir(parent)
}

#[derive(Default)]
pub(crate) struct BackupReview {
    pub request: Option<String>,
    pub archive: Option<RestoredArchive>,
}
impl BackupReview {
    pub fn begin(&mut self, token: String) -> Option<RestoredArchive> {
        self.request = Some(token);
        self.archive.take()
    }
    pub fn accept(
        &mut self,
        token: &str,
        archive: RestoredArchive,
    ) -> Option<carrel_store::backup::BackupPreview> {
        if self.request.as_deref() != Some(token) {
            return None;
        }
        let preview = archive.preview.clone();
        self.archive = Some(archive);
        Some(preview)
    }
    pub fn cancel(&mut self, token: &str) -> Option<RestoredArchive> {
        if self.request.as_deref() == Some(token) {
            self.request = None;
            self.archive.take()
        } else {
            None
        }
    }
    pub fn take(&mut self, token: &str) -> Result<RestoredArchive> {
        if self.request.as_deref() != Some(token) {
            return Err(BackupError::Format.into());
        }
        self.request = None;
        self.archive
            .take()
            .ok_or_else(|| BackupError::Format.into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    version: u8,
    stage: String,
    previous: String,
    activating: bool,
    installed: bool,
    #[serde(default)]
    cancelling: bool,
}

pub(crate) fn pending(paths: &InstallPaths) -> bool {
    paths.root.join(RECORD).exists()
}

pub(crate) fn eligible(store: &Store, paths: &InstallPaths) -> Result<bool> {
    if !store.is_empty_for_restore()? || Keystore::open(&paths.keys).encrypted_master()?.is_some() {
        return Ok(false);
    }
    if !Keystore::open(&paths.keys).contains_only_device_key()? {
        return Ok(false);
    }
    // Orphaned cache/transport data is still data; an empty visible list cannot erase it.
    for root in [&paths.blobs, &paths.root.join("sync")] {
        if root.exists() && fs::read_dir(root).map_err(file_error)?.next().is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn prepare(paths: &InstallPaths, store: &Store, archive: RestoredArchive) -> Result<()> {
    prepare_with_sync(paths, store, archive, sync_dir)
}

fn prepare_with_sync(
    paths: &InstallPaths,
    store: &Store,
    archive: RestoredArchive,
    mut sync_directory: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let _operation = ACTIVATION_OPERATION.lock().expect("restore operation lock");
    if pending(paths) || !eligible(store, paths)? {
        return Err(BackupError::NotEmpty.into());
    }
    carrel_store::backup::validate_staged(archive.path())?;
    sync_tree(archive.path(), &mut sync_directory)?;
    let previous = tempfile::Builder::new()
        .prefix("restore-previous-")
        .tempdir_in(&paths.root)
        .map_err(file_error)?;
    let record = Activation {
        version: 1,
        stage: basename(archive.path())?,
        previous: basename(previous.path())?,
        activating: false,
        installed: false,
        cancelling: false,
    };
    publish_record(&paths.root, &record)?;
    // Publication transfers ownership to the journal even if its directory sync
    // fails. Before publication, an error still drops all plaintext staging.
    let _ = archive.keep();
    let _ = previous.keep();
    sync_directory(&paths.root)
}

/// Resume interrupted renames before any active database handle is created.
pub(crate) fn activate_pending(paths: &InstallPaths) -> Result<Option<String>> {
    let _operation = ACTIVATION_OPERATION.lock().expect("restore operation lock");
    if !pending(paths) {
        return Ok(None);
    }
    let mut record = read_record(&paths.root)?;
    if record.cancelling {
        finish_cleanup(paths, &record)?;
        return Ok(Some(
            "Prepared restore cancelled. Your current installation was kept.".into(),
        ));
    }
    if record.installed {
        carrel_store::backup::validate_staged(&paths.root)?;
        finish_cleanup(paths, &record)?;
        return Ok(Some(
            "Backup restored. Open Settings → Devices to resume your identity and connections."
                .into(),
        ));
    }
    let stage = owned(&paths.root, &record.stage)?;
    let previous = owned(&paths.root, &record.previous)?;
    if !record.activating {
        let store = Store::open(&paths.store)?;
        let empty = eligible(&store, paths)?;
        drop(store);
        if !empty {
            cancel_record(paths, record, sync_dir)?;
            return Ok(Some("Restore cancelled because this installation now contains data. Your current archive was kept.".into()));
        }
        carrel_store::backup::validate_staged(&stage)?;
        record.activating = true;
        save_record(&paths.root, &record)?;
    }
    move_parts(&paths.root, &stage, &previous)?;
    carrel_store::backup::validate_staged(&paths.root)?;
    record.installed = true;
    save_record(&paths.root, &record)?;
    finish_cleanup(paths, &record)?;
    Ok(Some("Backup restored. Read locally, then open Settings → Devices to resume your identity and connections.".into()))
}

pub(crate) fn cancel_prepared(paths: &InstallPaths) -> Result<()> {
    cancel_prepared_with_sync(paths, sync_dir)
}

fn cancel_prepared_with_sync(
    paths: &InstallPaths,
    sync_directory: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let _operation = ACTIVATION_OPERATION.lock().expect("restore operation lock");
    if !pending(paths) {
        return Ok(());
    }
    cancel_record(paths, read_record(&paths.root)?, sync_directory)
}

fn cancel_record(
    paths: &InstallPaths,
    mut record: Activation,
    sync_directory: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    if record.activating {
        return Err(AppError::Config(
            "Restore activation has already started. Reopen Carrel to finish it.".into(),
        ));
    }
    record.cancelling = true;
    publish_record(&paths.root, &record)?;
    sync_directory(&paths.root)?;
    finish_cleanup(paths, &record)
}

fn move_parts(root: &Path, stage: &Path, previous: &Path) -> Result<()> {
    for name in PARTS {
        let source = stage.join(name);
        let live = root.join(name);
        let old = previous.join(name);
        if source.exists() {
            if live.exists() {
                if old.exists() {
                    return Err(BackupError::Integrity.into());
                }
                fs::rename(&live, &old).map_err(file_error)?;
                sync_dir(root)?;
                sync_dir(previous)?;
            }
            fs::rename(source, live).map_err(file_error)?;
            sync_dir(root)?;
            sync_dir(stage)?;
        } else if !live.is_dir() {
            return Err(BackupError::Integrity.into());
        }
    }
    Ok(())
}
fn basename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| BackupError::Format.into())
}
fn check_owned_name(name: &str) -> Result<()> {
    if !name.starts_with("restore-")
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(BackupError::Format.into());
    }
    Ok(())
}
fn owned(root: &Path, name: &str) -> Result<PathBuf> {
    check_owned_name(name)?;
    let path = root.join(name);
    let metadata = fs::symlink_metadata(&path).map_err(file_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupError::Integrity.into());
    }
    Ok(path)
}
fn save_record(root: &Path, record: &Activation) -> Result<()> {
    publish_record(root, record)?;
    sync_dir(root)
}
fn publish_record(root: &Path, record: &Activation) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root).map_err(file_error)?;
    serde_json::to_writer(&mut file, record).map_err(|_| BackupError::Format)?;
    file.flush().map_err(file_error)?;
    file.as_file().sync_all().map_err(file_error)?;
    file.persist(root.join(RECORD))
        .map_err(|e| file_error(e.error))?;
    Ok(())
}
fn read_record(root: &Path) -> Result<Activation> {
    let path = root.join(RECORD);
    let metadata = fs::symlink_metadata(&path).map_err(file_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err(BackupError::Format.into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(file_error)?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    let record: Activation = serde_json::from_slice(&bytes).map_err(|_| BackupError::Format)?;
    check_owned_name(&record.stage)?;
    check_owned_name(&record.previous)?;
    if record.version != 1
        || record.stage == record.previous
        || (record.installed && !record.activating)
        || (record.cancelling && (record.activating || record.installed))
    {
        return Err(BackupError::Format.into());
    }
    Ok(record)
}
fn finish_cleanup(paths: &InstallPaths, record: &Activation) -> Result<()> {
    // A published installed/cancelling record may have survived a failed sync.
    // Make its recovery instruction durable before deleting anything it owns.
    sync_dir(&paths.root)?;
    for name in [&record.stage, &record.previous] {
        match fs::symlink_metadata(paths.root.join(name)) {
            Ok(_) => fs::remove_dir_all(owned(&paths.root, name)?).map_err(file_error)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(file_error(error)),
        }
    }
    sync_dir(&paths.root)?;
    fs::remove_file(paths.root.join(RECORD)).map_err(file_error)?;
    sync_dir(&paths.root)
}
fn sync_tree(path: &Path, sync_directory: &mut impl FnMut(&Path) -> Result<()>) -> Result<()> {
    for entry in fs::read_dir(path).map_err(file_error)? {
        let entry = entry.map_err(file_error)?;
        let kind = entry.file_type().map_err(file_error)?;
        if kind.is_dir() {
            sync_tree(&entry.path(), sync_directory)?;
        } else if kind.is_file() {
            File::open(entry.path())
                .and_then(|f| f.sync_all())
                .map_err(file_error)?;
        } else {
            return Err(BackupError::Integrity.into());
        }
    }
    sync_directory(path)
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(file_error)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn file_error(source: std::io::Error) -> AppError {
    BackupError::Io(source).into()
}

#[cfg(test)]
mod tests;
