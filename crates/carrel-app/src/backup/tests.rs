use super::*;
use carrel_core::identity::Keypair;
use carrel_store::{backup::open_backup, blobs::BlobStore};

const PASSWORD: &str = "a long private backup phrase";

#[test]
fn failed_backup_write_preserves_the_previous_file_and_removes_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("archive.carrel-backup");
    fs::write(&destination, "previous complete backup").unwrap();
    let failure = write_backup(&destination, |file| {
        file.write_all(b"partial new ciphertext")
            .map_err(file_error)?;
        Err(file_error(std::io::Error::other("simulated disk failure")))
    });
    assert!(failure.is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"previous complete backup");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    write_backup(&destination, |file| {
        file.write_all(b"new complete backup").map_err(file_error)
    })
    .unwrap();
    assert_eq!(fs::read(&destination).unwrap(), b"new complete backup");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
fn paths(root: &Path) -> InstallPaths {
    fs::create_dir_all(root).unwrap();
    InstallPaths {
        root: root.into(),
        store: root.join("store"),
        blobs: root.join("blobs"),
        keys: root.join("keys"),
        keymap_config: root.join("config/keymap.toml"),
    }
}
fn empty(root: &Path) -> (InstallPaths, Store) {
    let paths = paths(root);
    let store = Store::open(&paths.store).unwrap();
    store.migrate().unwrap();
    fs::create_dir(&paths.blobs).unwrap();
    Keystore::open(&paths.keys)
        .create_device(&Keypair::generate())
        .unwrap();
    (paths, store)
}
fn source(root: &Path) -> PathBuf {
    let (paths, store) = empty(root);
    store.add_feed("https://example.test/feed").unwrap();
    let blobs = BlobStore::open(&paths.blobs);
    blobs.put_blocking(b"Offline archived text").unwrap();
    let path = root.join("private.carrel-backup");
    store
        .create_backup(
            &blobs,
            &Keystore::open(&paths.keys),
            File::create(&path).unwrap(),
            PASSWORD,
            "native fixture",
            123,
        )
        .unwrap();
    path
}

#[test]
fn restore_resumes_after_each_directory_rename_and_preserves_the_source_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    for boundary in 0..=6 {
        let (target, store) = empty(&dir.path().join(format!("target-{boundary}")));
        let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
        prepare(&target, &store, archive).unwrap();
        drop(store);
        let mut record = read_record(&target.root).unwrap();
        record.activating = true;
        save_record(&target.root, &record).unwrap();
        let stage = target.root.join(&record.stage);
        let previous = target.root.join(&record.previous);
        for (index, name) in PARTS.iter().enumerate() {
            if boundary > index * 2 {
                fs::rename(target.root.join(name), previous.join(name)).unwrap();
            }
            if boundary > index * 2 + 1 {
                fs::rename(stage.join(name), target.root.join(name)).unwrap();
            }
        }
        assert!(activate_pending(&target).unwrap().is_some());
        let restored = Store::open(&target.store).unwrap();
        assert_eq!(
            restored.list_feeds().unwrap()[0].url,
            "https://example.test/feed"
        );
        assert!(!pending(&target));
        assert!(backup.exists());
        assert_eq!(
            restored.restore_origin().unwrap().unwrap().device_pubkey,
            *Keystore::open(&target.keys).load_device().unwrap().public()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&target.store, &target.keys, &target.blobs] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }
    }
}

#[test]
fn interrupted_final_cleanup_is_recoverable_with_missing_owned_directories() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    for removed in 1..=2 {
        let (target, store) = empty(&dir.path().join(format!("target-{removed}")));
        let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
        prepare(&target, &store, archive).unwrap();
        drop(store);
        let mut record = read_record(&target.root).unwrap();
        record.activating = true;
        save_record(&target.root, &record).unwrap();
        let stage = target.root.join(&record.stage);
        let previous = target.root.join(&record.previous);
        move_parts(&target.root, &stage, &previous).unwrap();
        record.installed = true;
        save_record(&target.root, &record).unwrap();
        fs::remove_dir_all(&stage).unwrap();
        if removed == 2 {
            fs::remove_dir_all(&previous).unwrap();
        }
        activate_pending(&target).unwrap();
        assert!(!pending(&target));
        assert!(!stage.exists());
        assert!(!previous.exists());
        assert_eq!(
            Store::open(&target.store).unwrap().list_feeds().unwrap()[0].url,
            "https://example.test/feed"
        );
    }
}

fn sync_failure() -> AppError {
    file_error(std::io::Error::other(
        "simulated journal directory sync failure",
    ))
}

#[test]
fn published_staging_survives_a_journal_directory_sync_failure_and_can_activate() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    let (target, store) = empty(&dir.path().join("target"));
    let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let stage = archive.path().to_path_buf();
    assert!(
        prepare_with_sync(&target, &store, archive, |path| {
            if path == target.root.as_path() {
                Err(sync_failure())
            } else {
                sync_dir(path)
            }
        })
        .is_err()
    );
    let record = read_record(&target.root).unwrap();
    assert!(!record.activating && !record.installed && !record.cancelling);
    assert!(stage.exists());
    assert!(target.root.join(&record.previous).exists());
    assert!(store.is_empty_for_restore().unwrap());
    drop(store);
    activate_pending(&target).unwrap();
    assert!(!pending(&target));
    assert_eq!(
        Store::open(&target.store).unwrap().list_feeds().unwrap()[0].url,
        "https://example.test/feed"
    );
    assert!(backup.exists());
}

#[test]
fn a_sync_failure_before_publication_removes_staging_and_keeps_the_empty_installation() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    let (target, store) = empty(&dir.path().join("target"));
    let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let stage = archive.path().to_path_buf();
    assert!(prepare_with_sync(&target, &store, archive, |_| Err(sync_failure())).is_err());
    assert!(!stage.exists());
    assert!(!pending(&target));
    assert!(eligible(&store, &target).unwrap());
}

#[test]
fn failed_cancellation_journal_sync_leaves_staging_for_startup_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    let (target, store) = empty(&dir.path().join("target"));
    let original_key = *Keystore::open(&target.keys).load_device().unwrap().public();
    let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
    prepare(&target, &store, archive).unwrap();
    assert!(cancel_prepared_with_sync(&target, |_| Err(sync_failure())).is_err());
    let record = read_record(&target.root).unwrap();
    assert!(record.cancelling);
    assert!(target.root.join(&record.stage).exists());
    assert!(target.root.join(&record.previous).exists());
    drop(store);
    assert!(
        activate_pending(&target)
            .unwrap()
            .unwrap()
            .contains("cancelled")
    );
    assert!(!pending(&target));
    assert!(!target.root.join(&record.stage).exists());
    assert!(!target.root.join(&record.previous).exists());
    assert!(
        Store::open(&target.store)
            .unwrap()
            .is_empty_for_restore()
            .unwrap()
    );
    assert_eq!(
        *Keystore::open(&target.keys).load_device().unwrap().public(),
        original_key
    );
}

#[test]
fn cancellation_resumes_before_during_and_after_removing_its_owned_directories() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    for boundary in 0..=3 {
        let (target, store) = empty(&dir.path().join(format!("target-{boundary}")));
        let original_key = *Keystore::open(&target.keys).load_device().unwrap().public();
        let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
        prepare(&target, &store, archive).unwrap();
        store.add_feed("https://keep.test/feed").unwrap();
        drop(store);
        let unrelated = target.root.join("restore-unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("sentinel"), "keep").unwrap();
        let mut record = read_record(&target.root).unwrap();
        record.cancelling = true;
        save_record(&target.root, &record).unwrap();
        let stage = target.root.join(&record.stage);
        let previous = target.root.join(&record.previous);
        if boundary == 1 {
            fs::remove_dir_all(stage.join("blobs")).unwrap();
        } else if boundary >= 2 {
            fs::remove_dir_all(&stage).unwrap();
        }
        if boundary == 3 {
            fs::remove_dir_all(&previous).unwrap();
        }
        assert!(
            activate_pending(&target)
                .unwrap()
                .unwrap()
                .contains("cancelled")
        );
        assert!(!pending(&target));
        assert!(!stage.exists());
        assert!(!previous.exists());
        assert_eq!(
            fs::read_to_string(unrelated.join("sentinel")).unwrap(),
            "keep"
        );
        assert_eq!(
            Store::open(&target.store).unwrap().list_feeds().unwrap()[0].url,
            "https://keep.test/feed"
        );
        assert_eq!(
            *Keystore::open(&target.keys).load_device().unwrap().public(),
            original_key
        );
    }
}

#[test]
fn overlapping_preparations_cannot_replace_the_first_published_journal() {
    use std::{sync::mpsc, time::Duration};
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    let (target, store) = empty(&dir.path().join("target"));
    let first = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let second = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let first_path = first.path().to_path_buf();
    let second_path = second.path().to_path_buf();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let target_ref = &target;
        let store_ref = &store;
        let first_path_ref = &first_path;
        let first_worker = scope.spawn(move || {
            prepare_with_sync(target_ref, store_ref, first, |path| {
                if path == first_path_ref.as_path() {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
                }
                sync_dir(path)
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(ACTIVATION_OPERATION.try_lock().is_err());
        let second_worker = scope.spawn(|| {
            started_tx.send(()).unwrap();
            prepare(&target, &store, second)
        });
        started_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        release_tx.send(()).unwrap();
        first_worker.join().unwrap().unwrap();
        assert!(second_worker.join().unwrap().is_err());
    });
    let record = read_record(&target.root).unwrap();
    assert_eq!(record.stage, basename(&first_path).unwrap());
    assert!(first_path.exists());
    assert!(!second_path.exists());
    cancel_prepared(&target).unwrap();
    assert!(!first_path.exists());
    assert!(!pending(&target));
    assert!(eligible(&store, &target).unwrap());
}

#[test]
fn cancellation_waits_for_an_overlapping_preparation_to_publish() {
    use std::{sync::mpsc, time::Duration};
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    let (target, store) = empty(&dir.path().join("target"));
    let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let stage = archive.path().to_path_buf();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let target_ref = &target;
        let store_ref = &store;
        let stage_ref = &stage;
        let prepare_worker = scope.spawn(move || {
            prepare_with_sync(target_ref, store_ref, archive, |path| {
                if path == stage_ref.as_path() {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
                }
                sync_dir(path)
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let cancel_worker = scope.spawn(|| {
            let result = cancel_prepared(&target);
            done_tx.send(()).unwrap();
            result
        });
        let early_completion = done_rx.recv_timeout(Duration::from_millis(100));
        release_tx.send(()).unwrap();
        prepare_worker.join().unwrap().unwrap();
        cancel_worker.join().unwrap().unwrap();
        assert!(matches!(
            early_completion,
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    });
    assert!(!stage.exists());
    assert!(!pending(&target));
    assert!(eligible(&store, &target).unwrap());
}

#[test]
fn existing_journals_remain_readable_and_cancellation_cannot_overlap_activation() {
    let dir = tempfile::tempdir().unwrap();
    let mut encoded = serde_json::json!({
        "version": 1,
        "stage": "restore-stage",
        "previous": "restore-previous",
        "activating": false,
        "installed": false,
    });
    fs::write(
        dir.path().join(RECORD),
        serde_json::to_vec(&encoded).unwrap(),
    )
    .unwrap();
    assert!(!read_record(dir.path()).unwrap().cancelling);
    for (activating, installed, cancelling, valid) in [
        (false, false, false, true), // Prepared.
        (true, false, false, true),  // Activating.
        (true, true, false, true),   // Installed; cleanup may resume.
        (false, false, true, true),  // Cancelling; cleanup may resume.
        (false, true, false, false),
        (false, true, true, false),
        (true, false, true, false),
        (true, true, true, false),
    ] {
        encoded["activating"] = activating.into();
        encoded["installed"] = installed.into();
        encoded["cancelling"] = cancelling.into();
        fs::write(
            dir.path().join(RECORD),
            serde_json::to_vec(&encoded).unwrap(),
        )
        .unwrap();
        assert_eq!(read_record(dir.path()).is_ok(), valid);
    }
}

#[test]
fn new_data_after_preview_or_preparation_prevents_any_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    for prepared in [false, true] {
        let (target, store) = empty(&dir.path().join(format!("target-{prepared}")));
        let archive = open_backup(&backup, PASSWORD, &target.root).unwrap();
        if prepared {
            prepare(&target, &store, archive).unwrap();
            store.add_feed("https://keep.test/feed").unwrap();
            drop(store);
            assert!(
                activate_pending(&target)
                    .unwrap()
                    .unwrap()
                    .contains("cancelled")
            );
        } else {
            store.add_feed("https://keep.test/feed").unwrap();
            assert!(prepare(&target, &store, archive).is_err());
            drop(store);
        }
        let retained = Store::open(&target.store).unwrap();
        assert_eq!(
            retained.list_feeds().unwrap()[0].url,
            "https://keep.test/feed"
        );
        assert!(!pending(&target));
    }
}

#[test]
fn cancellation_and_late_reviews_remove_only_their_owned_staging_directory() {
    let dir = tempfile::tempdir().unwrap();
    let backup = source(&dir.path().join("source"));
    let target = paths(&dir.path().join("target"));
    let mut review = BackupReview::default();
    drop(review.begin("first".into()));
    let first = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let first_path = first.path().to_path_buf();
    drop(review.begin("second".into()));
    let second = open_backup(&backup, PASSWORD, &target.root).unwrap();
    let second_path = second.path().to_path_buf();
    assert!(review.accept("second", second).is_some());
    assert!(review.accept("first", first).is_none());
    assert!(!first_path.exists());
    assert!(second_path.exists());
    drop(review.cancel("first"));
    assert!(second_path.exists());
    drop(review.cancel("second"));
    assert!(!second_path.exists());
}

#[test]
fn unsafe_activation_paths_and_orphan_cache_are_not_treated_as_an_empty_installation() {
    let dir = tempfile::tempdir().unwrap();
    let (target, store) = empty(&dir.path().join("target"));
    assert!(eligible(&store, &target).unwrap());
    let retained_key = target.keys.join("other-recovery-key");
    fs::write(&retained_key, "keep this key").unwrap();
    assert!(!eligible(&store, &target).unwrap());
    fs::remove_file(retained_key).unwrap();
    fs::write(target.blobs.join("retained"), "keep this data").unwrap();
    assert!(!eligible(&store, &target).unwrap());
    drop(store);
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), "keep").unwrap();
    save_record(
        &target.root,
        &Activation {
            version: 1,
            stage: "../outside".into(),
            previous: "restore-previous".into(),
            activating: true,
            installed: false,
            cancelling: false,
        },
    )
    .unwrap();
    assert!(activate_pending(&target).is_err());
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).unwrap(),
        "keep"
    );
    // Cleanup must validate names even when the referenced path does not exist.
    let hostile = Activation {
        version: 1,
        stage: "../missing".into(),
        previous: "restore-previous".into(),
        activating: true,
        installed: true,
        cancelling: false,
    };
    save_record(&target.root, &hostile).unwrap();
    assert!(read_record(&target.root).is_err());
}
