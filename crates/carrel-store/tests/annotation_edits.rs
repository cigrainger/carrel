use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};
use carrel_core::identity::Keypair;
use carrel_store::highlights::{EditNoteInput, PutHighlightInput, PutNoteInput};
use carrel_store::{Store, StoreError};

const TIME: i64 = 1_700_000_000_000_000;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    store.query(r#"
        ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
            [['item', 'article', 'A source', [], 'https://example.com/article', null, null, null, validity(1)]]
        :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
    "#).unwrap();
    store
}

fn highlight(store: &Store, key: &Keypair) -> carrel_store::highlights::HighlightRecord {
    store
        .put_highlight(
            PutHighlightInput {
                item_id: "item".into(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: None,
                    exact: "quiet web".into(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 9,
                }),
                location_label: None,
                color: None,
                created_at_micros: Some(TIME),
            },
            key,
        )
        .unwrap()
}

fn note(
    store: &Store,
    key: &Keypair,
    kind: Option<&str>,
    target: Option<&str>,
) -> carrel_store::highlights::NoteRecord {
    store
        .put_note(
            PutNoteInput {
                body: "Original café note\n\nSecond paragraph.".into(),
                target_kind: kind.map(str::to_owned),
                target_id: target.map(str::to_owned),
                created_at_micros: Some(TIME),
            },
            key,
        )
        .unwrap()
}

fn edit(id: &str, expected: &str, body: &str) -> EditNoteInput {
    EditNoteInput {
        id: id.into(),
        body: body.into(),
        expected_revision_id: expected.into(),
        now_micros: Some(TIME - 100),
    }
}

#[test]
fn self_device_removal_retains_author_and_never_restores_a_revoked_devices_work() {
    use carrel_core::identity::{DeviceCert, RevocationCert};
    let store = store();
    let master = Keypair::from_seed(&[41; 32]);
    let creator = Keypair::from_seed(&[42; 32]);
    let local = Keypair::from_seed(&[43; 32]);
    let original = highlight(&store, &creator);
    let attached = note(&store, &creator, Some("highlight"), Some(&original.id));
    let local_cert = DeviceCert::new_at_micros(&master, local.public(), 1, None);
    store.bind_self_identity(&local_cert, &local).unwrap();
    assert!(matches!(
        store.remove_highlight(&original.id, &local),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    let creator_cert = DeviceCert::new_at_micros(&master, creator.public(), 1, None);
    store.accept_self_device_certificate(&creator_cert).unwrap();
    store
        .accept_self_revocation(&RevocationCert::new_at_micros(&master, creator.public(), 2))
        .unwrap();
    let removal = store.remove_highlight(&original.id, &local).unwrap();
    removal.verify_signature().unwrap();
    assert_eq!(removal.removed_by, *local.public());
    assert_eq!(
        store.remove_highlight(&original.id, &local).unwrap(),
        removal
    );
    assert_eq!(store.get_highlight(&original.id).unwrap(), None);
    assert_eq!(store.get_note(&attached.id).unwrap(), None);
    store.accept_self_device_certificate(&creator_cert).unwrap();
    assert!(!store.self_device_authorized(creator.public()).unwrap());
    assert_eq!(store.get_highlight(&original.id).unwrap(), None);
    assert!(matches!(
        store.remove_highlight(&original.id, &creator),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    store
        .accept_self_revocation(&RevocationCert::new_at_micros(&master, local.public(), 3))
        .unwrap();
    assert!(matches!(
        store.remove_highlight(&original.id, &local),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    assert!(matches!(
        store.put_highlight(
            PutHighlightInput {
                item_id: "item".into(),
                location: original.location,
                location_label: None,
                color: None,
                created_at_micros: Some(TIME + 1),
            },
            &local
        ),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
}

#[test]
fn all_note_targets_keep_identity_metadata_and_signed_history_on_edit() {
    for kind in [None, Some("item"), Some("highlight")] {
        let store = store();
        let key = Keypair::generate();
        let highlight = highlight(&store, &key);
        let target = kind.map(|kind| {
            if kind == "item" {
                "item"
            } else {
                highlight.id.as_str()
            }
        });
        let original = note(&store, &key, kind, target);
        original.verify_signature().unwrap();
        let edited = store
            .edit_note(
                edit(&original.id, &original.revision_id, "Edited 日本語 note"),
                &key,
            )
            .unwrap();
        edited.verify_signature().unwrap();
        assert_eq!(edited.id, original.id);
        assert_eq!(edited.created_by, original.created_by);
        assert_eq!(edited.created_at_micros, original.created_at_micros);
        assert_eq!(edited.target_kind, original.target_kind);
        assert_eq!(edited.target_id, original.target_id);
        assert_eq!(edited.updated_at_micros, TIME + 1);
        assert_eq!(store.get_note(&original.id).unwrap(), Some(edited.clone()));
        let history = store
            .query("?[id, updated_at, body] := *note{id, updated_at, body}")
            .unwrap();
        assert_eq!(history.rows.len(), 2);
        let mut tampered = edited;
        tampered.body.push('!');
        assert!(tampered.verify_signature().is_err());
        let mut tampered = original;
        tampered.updated_by = *Keypair::generate().public();
        assert!(tampered.verify_signature().is_err());
    }
}

#[test]
fn stale_unauthorized_empty_missing_and_oversized_edits_leave_note_intact() {
    let store = store();
    let key = Keypair::generate();
    let original = note(&store, &key, Some("item"), Some("item"));
    assert!(matches!(
        store.edit_note(
            edit(&original.id, &original.revision_id, "other"),
            &Keypair::generate()
        ),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    assert!(matches!(
        store.edit_note(edit(&original.id, &"f".repeat(64), "stale"), &key),
        Err(StoreError::AnnotationConflict { .. })
    ));
    assert!(matches!(
        store.edit_note(edit("unknown", &"f".repeat(64), "missing"), &key),
        Err(StoreError::AnnotationUnavailable { .. })
    ));
    for body in [
        "  \t\n".into(),
        "é".repeat(carrel_core::annotation::MAX_NOTE_BYTES),
    ] {
        assert!(matches!(
            store.edit_note(edit(&original.id, &original.revision_id, &body), &key),
            Err(StoreError::InvalidInput { .. })
        ));
    }
    assert_eq!(store.get_note(&original.id).unwrap(), Some(original));
}

#[test]
fn concurrent_stale_editors_cannot_silently_overwrite_each_other() {
    let store = store();
    let key = std::sync::Arc::new(Keypair::generate());
    let original = note(&store, &key, Some("item"), Some("item"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|i| {
            let store = store.clone();
            let key = key.clone();
            let id = original.id.clone();
            let revision = original.revision_id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.edit_note(edit(&id, &revision, &format!("editor {i}")), &key)
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::AnnotationConflict { .. })))
            .count(),
        7
    );
    store
        .get_note(&original.id)
        .unwrap()
        .unwrap()
        .verify_signature()
        .unwrap();
}

#[test]
fn highlight_removal_is_signed_idempotent_and_dominates_notes_and_retries() {
    let store = store();
    let key = Keypair::generate();
    let original = highlight(&store, &key);
    let note = note(&store, &key, Some("highlight"), Some(&original.id));
    assert!(matches!(
        store.remove_highlight(&original.id, &Keypair::generate()),
        Err(StoreError::AnnotationUnauthorized { .. })
    ));
    assert!(store.get_highlight(&original.id).unwrap().is_some());
    let removal = store.remove_highlight(&original.id, &key).unwrap();
    removal.verify_signature().unwrap();
    assert_eq!(store.remove_highlight(&original.id, &key).unwrap(), removal);
    assert!(store.get_highlight(&original.id).unwrap().is_none());
    assert!(store.get_note(&note.id).unwrap().is_none());
    assert!(store.list_item_highlights("item").unwrap().is_empty());
    assert!(store.list_highlights(100).unwrap().is_empty());
    assert!(
        store
            .list_notes_for_target("highlight", &original.id)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .edit_note(edit(&note.id, &note.revision_id, "revive"), &key)
            .is_err()
    );
    assert!(
        store
            .put_note(
                PutNoteInput {
                    body: "new note".into(),
                    target_kind: Some("highlight".into()),
                    target_id: Some(original.id.clone()),
                    created_at_micros: None
                },
                &key
            )
            .is_err()
    );
    let mut tampered = removal;
    tampered.id = "another".into();
    assert!(tampered.verify_signature().is_err());
    assert_eq!(
        store.query("?[id] := *highlight{id}").unwrap().rows.len(),
        1
    );
    assert_eq!(store.query("?[id] := *note{id}").unwrap().rows.len(), 1);
}

#[test]
fn item_tombstone_hides_attached_annotations_and_new_note_writes() {
    let store = store();
    let key = Keypair::generate();
    let highlight = highlight(&store, &key);
    let item_note = note(&store, &key, Some("item"), Some("item"));
    let highlight_note = note(&store, &key, Some("highlight"), Some(&highlight.id));
    store.query("?[id, tombstoned_at, reason] <- [['item', validity(3), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
    assert!(store.get_highlight(&highlight.id).unwrap().is_none());
    assert!(store.get_note(&item_note.id).unwrap().is_none());
    assert!(store.get_note(&highlight_note.id).unwrap().is_none());
    assert!(
        store
            .put_note(
                PutNoteInput {
                    body: "new".into(),
                    target_kind: Some("item".into()),
                    target_id: Some("item".into()),
                    created_at_micros: None
                },
                &key
            )
            .is_err()
    );
}

#[test]
fn raw_writes_cannot_change_a_highlight_or_replace_a_signed_note_revision() {
    let store = store();
    let key = Keypair::generate();
    let original = highlight(&store, &key);
    let note = note(&store, &key, Some("highlight"), Some(&original.id));
    assert!(store.query(r#"
        ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] :=
            *highlight{id, item_id, location, location_label, color, created_by, created_at, signature}, quoted_text = 'changed quote'
        :put highlight {id => item_id, quoted_text, location, location_label, color, created_by, created_at, signature}
    "#).is_err());
    assert!(store.query(r#"
        ?[id, revision_id, updated_at, body, target_kind, target_id, created_by, created_at, updated_by, signature_version, parents, signature] :=
            *note{id, revision_id, updated_at, target_kind, target_id, created_by, created_at, updated_by, signature_version, parents, signature}, body = 'changed body'
        :put note {id, revision_id => body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature}
    "#).is_err());
    assert_eq!(store.get_highlight(&original.id).unwrap(), Some(original));
    assert_eq!(store.get_note(&note.id).unwrap(), Some(note));
}

#[test]
fn edited_notes_survive_persistent_store_restart() {
    let dir = tempfile::tempdir().unwrap();
    let key = Keypair::generate();
    let path = dir.path().join("store");
    let expected;
    {
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let original = note(&store, &key, None, None);
        expected = store
            .edit_note(
                edit(&original.id, &original.revision_id, "Persistent edit"),
                &key,
            )
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    assert_eq!(
        store.get_note(&expected.id).unwrap(),
        Some(expected.clone())
    );
    expected.verify_signature().unwrap();
}

#[test]
fn review_pages_and_export_cover_the_archive_with_all_latest_notes() {
    let store = store();
    let key = Keypair::generate();
    assert!(store.annotation_page(100, None).unwrap().items.is_empty());
    assert!(store.annotation_export_snapshot().unwrap().is_empty());
    let mut highlights = Vec::new();
    for i in 0..507 {
        highlights.push(
            store
                .put_highlight(
                    PutHighlightInput {
                        item_id: "item".into(),
                        location: HighlightLocation::HtmlText(HtmlTextSelector {
                            exact: format!("passage {i}"),
                            content_blob_id: None,
                            prefix: String::new(),
                            suffix: String::new(),
                            start: 0,
                            end: 20,
                        }),
                        location_label: None,
                        color: None,
                        created_at_micros: Some(TIME + (i / 3) * 1_000_000),
                    },
                    &key,
                )
                .unwrap(),
        );
    }
    let h = &highlights[500];
    let original = note(&store, &key, Some("highlight"), Some(&h.id));
    let edited = store
        .edit_note(
            edit(
                &original.id,
                &original.revision_id,
                "Full edited 日本語\n\nSecond paragraph.",
            ),
            &key,
        )
        .unwrap();
    store
        .put_note(
            PutNoteInput {
                body: "Another note".into(),
                target_kind: Some("highlight".into()),
                target_id: Some(h.id.clone()),
                created_at_micros: Some(TIME + 10),
            },
            &key,
        )
        .unwrap();
    store.remove_highlight(&highlights[506].id, &key).unwrap();
    let mut cursor = None;
    let mut all = Vec::new();
    loop {
        let page = store.annotation_page(100, cursor.as_ref()).unwrap();
        assert!(page.items.len() <= 100);
        cursor = page.next_cursor;
        all.extend(page.items);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(all.len(), 506);
    let snapshot = store.annotation_export_snapshot().unwrap();
    assert_eq!(snapshot, all);
    assert!(
        all.windows(2)
            .all(|rows| rows[0].source.highlight.created_at_micros
                > rows[1].source.highlight.created_at_micros
                || (rows[0].source.highlight.created_at_micros
                    == rows[1].source.highlight.created_at_micros
                    && rows[0].source.highlight.id < rows[1].source.highlight.id))
    );
    let review = all
        .iter()
        .find(|row| row.source.highlight.id == h.id)
        .unwrap();
    assert_eq!(review.notes.len(), 2);
    assert_eq!(review.notes[1], edited);
    assert_eq!(review.source.item_title, "A source");
    assert_eq!(review.site_name, None);
    store.query("?[id, tombstoned_at, reason] <- [['item', validity(3), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
    assert!(store.annotation_export_snapshot().unwrap().is_empty());
}
