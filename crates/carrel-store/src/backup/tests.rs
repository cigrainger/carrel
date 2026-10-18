use super::*;
use crate::notes::{EditNoteInput, PutNoteInput};
use carrel_core::identity::DeviceCert;

const PASSWORD: &str = "a long private backup phrase";
struct Fixture {
    dir: TempDir,
    store: Store,
    keys: Keystore,
    blobs: BlobStore,
    device: Keypair,
}
impl Fixture {
    fn new() -> Self {
        Self::at_schema(CURRENT_SCHEMA_VERSION)
    }
    fn at_schema(version: u32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        crate::migrations::migrate_through(&store.db, 0, version).unwrap();
        let keys = Keystore::open(&dir.path().join("keys"));
        let device = Keypair::generate();
        keys.create_device(&device).unwrap();
        let blobs = BlobStore::open(dir.path().join("blobs"));
        Self {
            dir,
            store,
            keys,
            blobs,
            device,
        }
    }
    fn save(&self) -> PathBuf {
        let path = self.dir.path().join("reading.carrel-backup");
        self.store
            .create_backup(
                &self.blobs,
                &self.keys,
                File::create(&path).unwrap(),
                PASSWORD,
                "fixture",
                123,
            )
            .unwrap();
        path
    }
}

#[test]
fn full_rocksdb_archive_restores_unicode_offline_content_history_and_local_ownership() {
    let f = Fixture::new();
    f.store.add_feed("https://example.test/feed").unwrap();
    let body = b"<p>Caf\xc3\xa9 and durable notes</p>";
    let hash = f.blobs.put_blocking(body).unwrap();
    // Even content retained only for an older highlight belongs to the backup.
    let historical = f.blobs.put_blocking(b"an older article body").unwrap();
    let note = f
        .store
        .put_note(
            PutNoteInput {
                body: "My private note — 文".into(),
                target_kind: None,
                target_id: None,
                created_at_micros: Some(1),
            },
            &f.device,
        )
        .unwrap();
    let revision = f
        .store
        .edit_note(
            EditNoteInput {
                id: note.id.clone(),
                body: "Second version".into(),
                expected_revision_id: note.revision_id.clone(),
                now_micros: Some(2),
            },
            &f.device,
        )
        .unwrap();
    let original = f.store.note_revisions(&note.id).unwrap();
    let path = f.save();
    let cipher = fs::read(&path).unwrap();
    assert!(!cipher.windows(7).any(|w| w == b"private"));
    let prepared = open_backup(&path, PASSWORD, f.dir.path()).unwrap();
    assert_eq!(prepared.preview.created_at_micros, 123);
    let restored = Store::open(&prepared.path().join("store")).unwrap();
    let restored_keys = Keystore::open(&prepared.path().join("keys"));
    let fresh = restored_keys.load_device().unwrap();
    assert_ne!(fresh.public(), f.device.public());
    assert!(restored.self_identity().unwrap().is_none());
    assert_eq!(restored.note_revisions(&note.id).unwrap(), original);
    assert_eq!(
        restored.list_feeds().unwrap(),
        f.store.list_feeds().unwrap()
    );
    let recovered_blobs = BlobStore::open(prepared.path().join("blobs"));
    assert_eq!(recovered_blobs.get_blocking(&hash).unwrap().as_ref(), body);
    assert_eq!(
        recovered_blobs.get_blocking(&historical).unwrap().as_ref(),
        b"an older article body"
    );
    let input = EditNoteInput {
        id: note.id.clone(),
        body: "Edited after recovery".into(),
        expected_revision_id: revision.revision_id.clone(),
        now_micros: Some(3),
    };
    assert!(
        restored
            .edit_note(input.clone(), &Keypair::generate())
            .is_err()
    );
    restored
        .edit_note(input, &fresh)
        .unwrap()
        .verify_signature()
        .unwrap();
    let master = Keypair::generate();
    restored
        .authorize_restored_identity(&master, &fresh, "Recovered")
        .unwrap();
    assert!(restored.self_device_authorized(fresh.public()).unwrap());
    assert!(
        restored
            .device_certificate(master.public(), f.device.public())
            .unwrap()
            .is_some()
    );
    drop(restored);
    let reopened = Store::open(&prepared.path().join("store")).unwrap();
    assert_eq!(
        reopened.get_note(&note.id).unwrap().unwrap().body,
        "Edited after recovery"
    );
    assert_eq!(
        f.store.get_note(&note.id).unwrap().unwrap().body,
        "Second version"
    );
}

#[test]
fn master_and_paired_backups_distinguish_recovery_and_refuse_wrong_identity_or_cloned_key() {
    for holds_master in [false, true] {
        let f = Fixture::new();
        let master = Keypair::generate();
        f.store
            .bind_self_identity(
                &DeviceCert::new(&master, f.device.public(), Some("Original".into())),
                &f.device,
            )
            .unwrap();
        if holds_master {
            f.keys
                .create_master(&master, "original master password")
                .unwrap();
        }
        let prepared = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
        assert_eq!(prepared.preview.has_master_key, holds_master);
        assert_eq!(prepared.preview.master, Some(master.public().to_hex()));
        let store = Store::open(&prepared.path().join("store")).unwrap();
        let keys = Keystore::open(&prepared.path().join("keys"));
        let fresh = keys.load_device().unwrap();
        assert_ne!(fresh.public(), f.device.public());
        let wrong = Keypair::generate();
        assert!(
            store
                .bind_self_identity(&DeviceCert::new(&wrong, fresh.public(), None), &fresh)
                .is_err()
        );
        assert!(
            store
                .bind_self_identity(
                    &DeviceCert::new(&master, f.device.public(), None),
                    &f.device
                )
                .is_err()
        );
        assert!(store.self_identity().unwrap().is_none());
        if holds_master {
            assert_eq!(
                keys.load_master("original master password")
                    .unwrap()
                    .public(),
                master.public()
            );
            assert!(
                keys.load_master(PASSWORD).is_err(),
                "backup password cannot reset the identity password"
            );
            store
                .authorize_restored_identity(&master, &fresh, "Recovered")
                .unwrap();
        } else {
            assert!(keys.encrypted_master().unwrap().is_none());
            store
                .bind_self_identity(
                    &DeviceCert::new(&master, fresh.public(), Some("Newly authorized".into())),
                    &fresh,
                )
                .unwrap();
        }
    }
}

#[test]
fn wrong_password_corruption_truncation_and_extra_ciphertext_never_leave_a_staged_archive() {
    let f = Fixture::new();
    let path = f.save();
    let bytes = fs::read(&path).unwrap();
    assert!(open_backup(&path, "wrong password", f.dir.path()).is_err());
    for length in [0, 10, bytes.len() / 2, bytes.len() - 1] {
        let damaged = f.dir.path().join("damaged");
        fs::write(&damaged, &bytes[..length]).unwrap();
        assert!(open_backup(&damaged, PASSWORD, f.dir.path()).is_err());
    }
    let mut damaged = bytes.clone();
    let index = damaged.len() - 30;
    damaged[index] ^= 1;
    let bad = f.dir.path().join("bad");
    fs::write(&bad, damaged).unwrap();
    assert!(open_backup(&bad, PASSWORD, f.dir.path()).is_err());
    let mut trailing = bytes;
    trailing.extend_from_slice(b"extra data");
    fs::write(&bad, trailing).unwrap();
    assert!(open_backup(&bad, PASSWORD, f.dir.path()).is_err());
    assert!(!fs::read_dir(f.dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("restore-")
    }));
}

#[test]
fn empty_library_with_hidden_history_or_identity_is_not_a_restore_destination() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    assert!(store.is_empty_for_restore().unwrap());
    store.query("?[id,tombstoned_at,reason] <- [['gone',[1,true],null]] :put entity_tombstone {id,tombstoned_at=>reason}").unwrap();
    assert!(!store.is_empty_for_restore().unwrap());
    let f = Fixture::new();
    let master = Keypair::generate();
    f.store
        .bind_self_identity(
            &DeviceCert::new(&master, f.device.public(), None),
            &f.device,
        )
        .unwrap();
    assert!(!f.store.is_empty_for_restore().unwrap());
}

#[test]
fn malformed_manifest_paths_duplicates_versions_lengths_and_passwords_are_rejected() {
    let valid = || Manifest {
        version: 1,
        preview: BackupPreview {
            app_version: "fixture".into(),
            schema_version: CURRENT_SCHEMA_VERSION,
            created_at_micros: 1,
            master: None,
            has_master_key: false,
            cache_incomplete: false,
        },
        source_device: None,
        entries: vec![Entry {
            name: "database.sqlite".into(),
            bytes: 32,
            digest: "a".repeat(64),
        }],
    };
    assert!(validate_manifest(&valid()).is_ok());
    let mut legacy = valid();
    legacy.preview.schema_version = 13;
    assert!(validate_manifest(&legacy).is_ok());
    for version in [0, 12, CURRENT_SCHEMA_VERSION + 1, u32::MAX] {
        let mut unknown = valid();
        unknown.preview.schema_version = version;
        assert!(matches!(
            validate_manifest(&unknown),
            Err(BackupError::Unsupported)
        ));
    }
    for name in [
        "../database.sqlite",
        "/database.sqlite",
        "blobs/../../outside",
        "keys/device.key",
        "other",
        "blobs/ABC",
    ] {
        let mut manifest = valid();
        manifest.entries[0].name = name.into();
        assert!(validate_manifest(&manifest).is_err());
    }
    let mut manifest = valid();
    manifest.preview.schema_version += 1;
    assert!(validate_manifest(&manifest).is_err());
    let mut manifest = valid();
    manifest.version = 2;
    assert!(validate_manifest(&manifest).is_err());
    let mut manifest = valid();
    manifest.entries[0].bytes = u64::MAX;
    assert!(validate_manifest(&manifest).is_err());
    let mut manifest = valid();
    manifest.entries.push(Entry {
        name: "database.sqlite".into(),
        bytes: 32,
        digest: "b".repeat(64),
    });
    assert!(validate_manifest(&manifest).is_err());
    for password in ["", "short"] {
        assert!(check_passphrase(password).is_err());
    }
    assert!(check_passphrase(&"x".repeat(4097)).is_err());
}

#[test]
fn injected_database_triggers_are_rejected_before_recovery() {
    for version in [13, CURRENT_SCHEMA_VERSION] {
        let f = Fixture::at_schema(version);
        f.store.query("::set_triggers item on put { ?[id] := _new[id,kind,title,creators,primary_url,published_at,language,summary,discovered_at] :rm item {id} }").unwrap();
        assert!(matches!(
            open_backup(&f.save(), PASSWORD, f.dir.path()),
            Err(BackupError::Integrity)
        ));
    }
}

#[test]
fn schema_13_backup_is_validated_then_migrated_without_changing_signed_history() {
    let f = Fixture::at_schema(13);
    f.store
        .add_feed("https://example.test/legacy-feed")
        .unwrap();
    let note = f
        .store
        .put_note(
            PutNoteInput {
                body: "A note made before the consent upgrade — 文".into(),
                target_kind: None,
                target_id: None,
                created_at_micros: Some(1),
            },
            &f.device,
        )
        .unwrap();
    let prepared = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    assert_eq!(prepared.preview.schema_version, 13);
    let restored = Store::open(&prepared.path().join("store")).unwrap();
    assert_eq!(
        restored.current_schema_version().unwrap(),
        CURRENT_SCHEMA_VERSION
    );
    assert_eq!(restored.get_note(&note.id).unwrap(), Some(note));
    assert_eq!(
        restored.list_feeds().unwrap(),
        f.store.list_feeds().unwrap()
    );
    assert!(restored.self_identity().unwrap().is_none());
    drop(restored);
    validate_staged(prepared.path()).unwrap();
    assert_eq!(f.store.current_schema_version().unwrap(), 13);
}

#[test]
fn catalog_validation_requires_the_exact_declared_schema_and_historical_guards() {
    for version in [13, CURRENT_SCHEMA_VERSION] {
        let f = Fixture::at_schema(version);
        validate_catalog_version(&f.store, version).unwrap();
        let other = if version == 13 {
            CURRENT_SCHEMA_VERSION
        } else {
            13
        };
        assert!(matches!(
            validate_catalog_version(&f.store, other),
            Err(BackupError::Integrity)
        ));
        // A valid version number cannot hide an extra relation. This check runs
        // before a restored database can execute its migration-time triggers.
        f.store.query(":create unexpected { id: String }").unwrap();
        assert!(matches!(
            validate_catalog_version(&f.store, version),
            Err(BackupError::Integrity)
        ));
    }
}

#[test]
fn missing_cached_images_are_explicit_and_available_images_restore_offline() {
    let f = Fixture::new();
    let image = BlobId::from_bytes(b"image");
    f.store.query_with_params("?[item_id,blob_id,images] <- [['article',$body,$images]] :put item_current_image_scope {item_id,blob_id=>images}",BTreeMap::from([("body".into(),DataValue::from("a".repeat(64))),("images".into(),DataValue::Json(cozo::JsonData(serde_json::json!([{"blob_id":image.to_string(),"byte_size":5}]))))])).unwrap();
    let preview = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    assert!(preview.preview.cache_incomplete);
    drop(preview);
    f.blobs.put_blocking(b"image").unwrap();
    let restored = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    assert!(!restored.preview.cache_incomplete);
    assert_eq!(
        BlobStore::open(restored.path().join("blobs"))
            .get_blocking(&image)
            .unwrap()
            .as_ref(),
        b"image"
    );
}

#[test]
fn a_body_referenced_only_by_a_historical_highlight_is_reported_and_retained() {
    use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
    let f = Fixture::new();
    f.store.query("?[id,kind,title,creators,primary_url,published_at,language,summary,discovered_at] <- [['article','article','Revised essay',[],'https://example.test/essay',null,null,null,validity(1)]] :put item {id=>kind,title,creators,primary_url,published_at,language,summary,discovered_at}").unwrap();
    let body = b"<p>An earlier passage</p>";
    let hash = BlobId::from_bytes(body);
    let highlight = f
        .store
        .put_highlight(
            crate::highlights::PutHighlightInput {
                item_id: "article".into(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: Some(hash.to_string()),
                    exact: "An earlier passage".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 18,
                }),
                location_label: None,
                color: None,
                created_at_micros: Some(1),
            },
            &f.device,
        )
        .unwrap();
    let missing = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    assert!(missing.preview.cache_incomplete);
    drop(missing);
    f.blobs.put_blocking(body).unwrap();
    let complete = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    assert!(!complete.preview.cache_incomplete);
    let restored = Store::open(&complete.path().join("store")).unwrap();
    assert_eq!(
        restored.get_highlight(&highlight.id).unwrap(),
        Some(highlight)
    );
    assert_eq!(
        BlobStore::open(complete.path().join("blobs"))
            .get_blocking(&hash)
            .unwrap()
            .as_ref(),
        body
    );
}

#[test]
fn hostile_scrypt_cost_or_an_unbounded_header_is_rejected_before_staging() {
    let f = Fixture::new();
    let path = f.save();
    let bytes = fs::read(&path).unwrap();
    let start = bytes.windows(10).position(|w| w == b"-> scrypt ").unwrap();
    let end = start + bytes[start..].iter().position(|b| *b == b'\n').unwrap();
    let line = std::str::from_utf8(&bytes[start..end]).unwrap();
    let (prefix, _) = line.rsplit_once(' ').unwrap();
    let mut hostile = bytes[..start].to_vec();
    hostile.extend_from_slice(format!("{prefix} 63").as_bytes());
    hostile.extend_from_slice(&bytes[end..]);
    let path = f.dir.path().join("hostile");
    fs::write(&path, hostile).unwrap();
    assert!(open_backup(&path, PASSWORD, f.dir.path()).is_err());
    fs::write(
        &path,
        format!("age-encryption.org/v1\n{}", "x".repeat(8192)),
    )
    .unwrap();
    assert!(open_backup(&path, PASSWORD, f.dir.path()).is_err());
}

#[test]
fn recovery_preserves_conflicting_notes_private_folders_and_withdrawn_group_conversations() {
    use crate::sharing::ShareDraft;
    use carrel_core::{
        annotation::{HighlightLocation, HtmlTextSelector},
        conversation::ConversationContent,
        private_state::{
            ReadIntent, ReadingPosition, StateKey, StateKind, StateValue, SubscriptionState,
        },
        sharing::{ContactCard, Share, ShareFact, public_article_snapshot},
    };
    let f = Fixture::new();
    let master = Keypair::generate();
    let certificate = DeviceCert::new(&master, f.device.public(), Some("Original".into()));
    f.store.bind_self_identity(&certificate, &f.device).unwrap();
    let peer = Store::open_in_memory().unwrap();
    peer.migrate().unwrap();
    let peer_key = Keypair::generate();
    let peer_certificate = DeviceCert::new(&master, peer_key.public(), None);
    peer.bind_self_identity(&peer_certificate, &peer_key)
        .unwrap();
    peer.accept_self_device_certificate(&certificate).unwrap();
    f.store
        .accept_self_device_certificate(&peer_certificate)
        .unwrap();
    let root = f
        .store
        .put_note(
            PutNoteInput {
                body: "An original thought".into(),
                target_kind: None,
                target_id: None,
                created_at_micros: Some(1),
            },
            &f.device,
        )
        .unwrap();
    peer.accept_self_note_revision(&root, f.device.public())
        .unwrap();
    for (store, key, body) in [
        (&f.store, &f.device, "Offline draft A — 文"),
        (&peer, &peer_key, "Offline draft B — café"),
    ] {
        store
            .edit_note(
                EditNoteInput {
                    id: root.id.clone(),
                    body: body.into(),
                    expected_revision_id: root.revision_id.clone(),
                    now_micros: Some(2),
                },
                key,
            )
            .unwrap();
    }
    let remote = peer.note_heads(&root.id).unwrap().remove(0);
    f.store
        .accept_self_note_revision(&remote, peer_key.public())
        .unwrap();
    let heads = f.store.note_heads(&root.id).unwrap();
    assert_eq!(heads.len(), 2);
    let subscription = StateKey {
        kind: StateKind::Subscription,
        target: "https://example.test/removed".into(),
    };
    for subscribed in [true, false] {
        f.store
            .write_private_state(
                subscription.clone(),
                StateValue::Subscription(SubscriptionState {
                    subscribed,
                    title: Some("Removed source".into()),
                    folder: Some("Research/Essays".into()),
                    auto_mark_read: false,
                }),
                if subscribed { 3 } else { 4 },
                &f.device,
            )
            .unwrap();
    }
    let body = b"<p>An archived conversation starter</p>";
    let hash = f.blobs.put_blocking(body).unwrap();
    f.store.query_with_params("{ ?[id,kind,title,creators,primary_url,published_at,language,summary,discovered_at] <- [['article','article','Essay',[],'https://example.test/essay',null,null,null,validity(1)]] :put item {id=>kind,title,creators,primary_url,published_at,language,summary,discovered_at} } { ?[item_id,format,blob_id,fetched_at,extracted_with,byte_size] <- [['article','html_readable',$blob,validity(1),'fixture',$size]] :put item_content {item_id,format=>blob_id,fetched_at,extracted_with,byte_size} }", BTreeMap::from([("blob".into(),DataValue::from(hash.to_string())),("size".into(),DataValue::from(body.len() as i64))])).unwrap();
    let highlight = f
        .store
        .put_highlight(
            crate::highlights::PutHighlightInput {
                item_id: "article".into(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: Some(hash.to_string()),
                    exact: "An archived conversation starter".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 32,
                }),
                location_label: None,
                color: None,
                created_at_micros: Some(2),
            },
            &f.device,
        )
        .unwrap();
    let mut state_keys = Vec::new();
    for value in [
        StateValue::Read(ReadIntent::Reading),
        StateValue::Star(true),
        StateValue::Position(ReadingPosition {
            progress: Some(0.42),
            progress_label: Some("Middle".into()),
            last_position: Some(serde_json::json!({"scroll":400})),
        }),
    ] {
        let key = StateKey {
            kind: value.kind(),
            target: "article".into(),
        };
        f.store
            .write_private_state(key.clone(), value, 4, &f.device)
            .unwrap();
        state_keys.push(key);
    }
    let mut audience = Vec::new();
    let mut friends = Vec::new();
    for name in ["Alice", "Bo"] {
        let friend = Keypair::generate();
        let device = Keypair::generate();
        let card = ContactCard::sign(
            DeviceCert::new(&friend, device.public(), None),
            "connection".into(),
            &device,
        )
        .unwrap();
        f.store
            .follow_social_contact(*friend.public(), name, Some(&card))
            .unwrap();
        audience.push(*friend.public());
        friends.push((friend, device, card));
    }
    audience.sort();
    f.store
        .save_social_group("essays", "My private group", audience.clone())
        .unwrap();
    let share = f
        .store
        .create_social_share_for(
            ShareDraft {
                request_id: "backup-conversation".into(),
                recipient: audience[0],
                article: public_article_snapshot(
                    f.store.saved_article_snapshot("article").unwrap().unwrap(),
                    Vec::new(),
                )
                .unwrap(),
                note: Some("For both of you".into()),
            },
            audience.clone(),
            5,
            &f.device,
        )
        .unwrap();
    let reply = f
        .store
        .reply_to_social_share(&share.id, "reply", None, "A retained reply", 6, &f.device)
        .unwrap();
    let ConversationContent::Reply { id: reply_id, .. } = &reply.content else {
        panic!()
    };
    f.store
        .remove_social_reply(&share.id, reply_id, 7, &f.device)
        .unwrap();
    f.store
        .retract_social_share(&share.id, 8, &f.device)
        .unwrap();
    let (friend, friend_device, card) = &friends[0];
    let received = Share::sign(
        "incoming-backup".into(),
        *master.public(),
        share.article.clone(),
        Some("Thought you would enjoy this".into()),
        9,
        card.certificate.clone(),
        friend_device,
    )
    .unwrap();
    f.store
        .accept_social_record(
            &ShareFact::Share(Box::new(received.clone())),
            friend.public(),
            10,
            &f.device,
        )
        .unwrap();
    f.store
        .leave_social_conversation(&received.id, 11, &f.device)
        .unwrap();
    f.store
        .set_allow_unfollowed_group_conversations(false, 12, &f.device)
        .unwrap();
    let controls = f.store.query("?[register,revision_id,encoded] := *self_state{register,revision_id,encoded} :sort register,revision_id").unwrap().rows;
    let history = f
        .store
        .social_history(None, 100)
        .unwrap()
        .into_iter()
        .map(|fact| fact.to_bytes().unwrap())
        .collect::<Vec<_>>();
    let pending = f
        .store
        .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
        .unwrap()
        .rows;
    let backup = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    assert!(!backup.preview.cache_incomplete);
    let restored = Store::open(&backup.path().join("store")).unwrap();
    let fresh = Keystore::open(&backup.path().join("keys"))
        .load_device()
        .unwrap();
    assert_eq!(restored.note_heads(&root.id).unwrap(), heads);
    let recovered_highlight = restored.get_highlight(&highlight.id).unwrap().unwrap();
    assert_eq!(recovered_highlight, highlight);
    recovered_highlight.verify_signature().unwrap();
    for key in state_keys {
        assert_eq!(
            restored.private_state_value(&key).unwrap(),
            f.store.private_state_value(&key).unwrap()
        );
    }
    for revision in restored.note_ancestry(&root.id).unwrap() {
        revision.verify_signature().unwrap();
    }
    assert_eq!(
        restored.private_state_heads(&subscription).unwrap(),
        f.store.private_state_heads(&subscription).unwrap()
    );
    assert_eq!(
        restored
            .query("?[key,encoded] := *self_fact_outbox{key,encoded} :sort key")
            .unwrap()
            .rows,
        pending
    );
    restored
        .authorize_restored_identity(&master, &fresh, "Recovered")
        .unwrap();
    assert!(!restored.allow_unfollowed_group_conversations().unwrap());
    assert_eq!(
        restored.conversation_participation(&received.id).unwrap(),
        carrel_core::conversation::ConversationParticipation::Left
    );
    assert_eq!(restored.query("?[register,revision_id,encoded] := *self_state{register,revision_id,encoded} :sort register,revision_id").unwrap().rows, controls);
    assert_eq!(restored.social_groups().unwrap()[0].members, audience);
    assert_eq!(restored.social_contacts().unwrap().len(), 2);
    assert_eq!(
        restored
            .social_history(None, 100)
            .unwrap()
            .into_iter()
            .map(|fact| fact.to_bytes().unwrap())
            .collect::<Vec<_>>(),
        history
    );
    let conversation = restored.social_conversation(&share.id).unwrap();
    assert_eq!(conversation.share, share);
    assert!(conversation.closed);
    assert_eq!(conversation.replies.len(), 1);
    assert!(conversation.replies[0].removed);
    assert_eq!(conversation.replies[0].record, reply);
    assert!(
        restored
            .received_item_conversations("article")
            .unwrap()
            .iter()
            .any(|thread| thread.id == received.id)
    );
    assert!(
        restored
            .query("?[singleton] <- [[0]] :rm self_identity {singleton}")
            .is_err(),
        "ordinary immutable identity guards remain installed"
    );
}

#[test]
fn backing_up_before_resuming_a_recovered_identity_preserves_the_intermediate_note_author() {
    let f = Fixture::new();
    let master = Keypair::generate();
    let original = DeviceCert::new(&master, f.device.public(), Some("Original".into()));
    f.store.bind_self_identity(&original, &f.device).unwrap();
    let first = open_backup(&f.save(), PASSWORD, f.dir.path()).unwrap();
    let store = Store::open(&first.path().join("store")).unwrap();
    let keys = Keystore::open(&first.path().join("keys"));
    let intermediate = keys.load_device().unwrap();
    let note = store
        .put_note(
            PutNoteInput {
                body: "Written locally after the first recovery — 文".into(),
                target_kind: None,
                target_id: None,
                created_at_micros: Some(1),
            },
            &intermediate,
        )
        .unwrap();
    assert!(
        store
            .device_certificate(master.public(), intermediate.public())
            .unwrap()
            .is_none()
    );
    let path = f.dir.path().join("second.carrel-backup");
    store
        .create_backup(
            &BlobStore::open(first.path().join("blobs")),
            &keys,
            File::create(&path).unwrap(),
            PASSWORD,
            "fixture",
            2,
        )
        .unwrap();
    let second = open_backup(&path, PASSWORD, f.dir.path()).unwrap();
    let restored = Store::open(&second.path().join("store")).unwrap();
    let fresh = Keystore::open(&second.path().join("keys"))
        .load_device()
        .unwrap();
    restored
        .authorize_restored_identity(&master, &fresh, "Recovered again")
        .unwrap();
    assert!(
        restored
            .device_certificate(master.public(), intermediate.public())
            .unwrap()
            .is_some()
    );
    assert_eq!(
        restored
            .device_certificate(master.public(), f.device.public())
            .unwrap(),
        Some(original)
    );
    assert_eq!(restored.get_note(&note.id).unwrap().unwrap(), note);
    restored
        .edit_note(
            EditNoteInput {
                id: note.id,
                body: "Still editable".into(),
                expected_revision_id: note.revision_id,
                now_micros: Some(3),
            },
            &fresh,
        )
        .unwrap()
        .verify_signature()
        .unwrap();
}
