use carrel_core::annotation::note::NoteContent;
use carrel_core::identity::{DeviceCert, Keypair, RevocationCert};
use carrel_store::notes::{EditNoteInput, NoteReceive, PutNoteInput, ResolveNoteInput};
use carrel_store::{Store, StoreError};

struct Pair {
    a: Store,
    b: Store,
    master: Keypair,
    ka: Keypair,
    kb: Keypair,
}

fn pair() -> Pair {
    let master = Keypair::from_seed(&[50; 32]);
    let ka = Keypair::from_seed(&[51; 32]);
    let kb = Keypair::from_seed(&[52; 32]);
    let a = Store::open_in_memory().unwrap();
    a.migrate().unwrap();
    let b = Store::open_in_memory().unwrap();
    b.migrate().unwrap();
    let ca = DeviceCert::new_at_micros(&master, ka.public(), 1, Some("A".into()));
    let cb = DeviceCert::new_at_micros(&master, kb.public(), 1, Some("B".into()));
    a.bind_self_identity(&ca, &ka).unwrap();
    b.bind_self_identity(&cb, &kb).unwrap();
    a.accept_self_device_certificate(&cb).unwrap();
    b.accept_self_device_certificate(&ca).unwrap();
    Pair {
        a,
        b,
        master,
        ka,
        kb,
    }
}

fn root(p: &Pair, kind: Option<&str>, target: Option<&str>) -> carrel_store::notes::NoteRecord {
    p.a.put_note(
        PutNoteInput {
            body: "Original café note".into(),
            target_kind: kind.map(str::to_owned),
            target_id: target.map(str::to_owned),
            created_at_micros: Some(1),
        },
        &p.ka,
    )
    .unwrap()
}

fn edit(
    store: &Store,
    key: &Keypair,
    record: &carrel_store::notes::NoteRecord,
    body: &str,
) -> carrel_store::notes::NoteRecord {
    store
        .edit_note(
            EditNoteInput {
                id: record.id.clone(),
                body: body.into(),
                expected_revision_id: record.revision_id.clone(),
                now_micros: Some(2),
            },
            key,
        )
        .unwrap()
}

#[test]
fn independent_devices_preserve_equal_time_drafts_and_converge_after_explicit_resolution() {
    let p = pair();
    let base = root(&p, None, None);
    assert_eq!(
        p.b.accept_self_note_revision(&base, p.ka.public()).unwrap(),
        NoteReceive::Applied
    );
    let first = edit(&p.a, &p.ka, &base, "Offline A draft");
    let second = edit(&p.b, &p.kb, &base, "Offline B draft");
    assert_eq!(first.updated_at_micros, second.updated_at_micros);
    p.a.accept_self_note_revision(&second, p.kb.public())
        .unwrap();
    p.b.accept_self_note_revision(&first, p.ka.public())
        .unwrap();
    let heads = p.a.note_heads(&base.id).unwrap();
    assert_eq!(heads.len(), 2);
    assert_eq!(heads, p.b.note_heads(&base.id).unwrap());
    assert!(matches!(
        p.a.get_note(&base.id),
        Err(StoreError::AnnotationConflict { .. })
    ));
    assert!(matches!(
        p.a.edit_note(
            EditNoteInput {
                id: base.id.clone(),
                body: "unreviewed overwrite".into(),
                expected_revision_id: first.revision_id.clone(),
                now_micros: None
            },
            &p.ka
        ),
        Err(StoreError::AnnotationConflict { .. })
    ));
    let resolved =
        p.a.resolve_note(
            ResolveNoteInput {
                id: base.id.clone(),
                body: "A deliberately merged draft".into(),
                expected_revision_ids: heads.iter().map(|head| head.revision_id.clone()).collect(),
                now_micros: Some(-100),
            },
            &p.ka,
        )
        .unwrap();
    assert_eq!(resolved.updated_at_micros, 3);
    p.b.accept_self_note_revision(&resolved, p.ka.public())
        .unwrap();
    assert_eq!(p.a.get_note(&base.id).unwrap(), Some(resolved.clone()));
    assert_eq!(p.b.get_note(&base.id).unwrap(), Some(resolved));
    assert_eq!(p.a.note_revisions(&base.id).unwrap().len(), 4);
    assert_eq!(
        p.a.note_revisions(&base.id).unwrap(),
        p.b.note_revisions(&base.id).unwrap()
    );
    assert_eq!(
        p.a.accept_self_note_revision(&second, p.kb.public())
            .unwrap(),
        NoteReceive::Existing
    );
    assert_eq!(p.a.note_heads(&base.id).unwrap().len(), 1);
}

#[test]
fn an_additional_branch_makes_an_open_resolution_stale_without_losing_any_version() {
    let p = pair();
    let base = root(&p, None, None);
    p.b.accept_self_note_revision(&base, p.ka.public()).unwrap();
    let first = edit(&p.a, &p.ka, &base, "A");
    let second = edit(&p.b, &p.kb, &base, "B");
    p.a.accept_self_note_revision(&second, p.kb.public())
        .unwrap();
    let reviewed: Vec<_> =
        p.a.note_heads(&base.id)
            .unwrap()
            .iter()
            .map(|record| record.revision_id.clone())
            .collect();
    let c = Keypair::from_seed(&[53; 32]);
    p.a.accept_self_device_certificate(&DeviceCert::new_at_micros(&p.master, c.public(), 1, None))
        .unwrap();
    let third = NoteContent {
        id: base.id.clone(),
        body: "C arrived during review".into(),
        target_kind: None,
        target_id: None,
        created_by: base.created_by,
        created_at_micros: 1,
        updated_at_micros: 2,
        parents: vec![base.revision_id.clone()],
    }
    .sign(&c)
    .unwrap();
    p.a.accept_self_note_revision(&third, c.public()).unwrap();
    assert!(matches!(
        p.a.resolve_note(
            ResolveNoteInput {
                id: base.id.clone(),
                body: "Stale merged draft".into(),
                expected_revision_ids: reviewed,
                now_micros: Some(3)
            },
            &p.ka
        ),
        Err(StoreError::AnnotationConflict { .. })
    ));
    let heads = p.a.note_heads(&base.id).unwrap();
    assert_eq!(heads.len(), 3);
    assert!(heads.contains(&first) && heads.contains(&second) && heads.contains(&third));
}

#[test]
fn out_of_order_children_survive_restart_and_promote_after_their_parents() {
    let p = pair();
    let base = root(&p, None, None);
    let child = edit(&p.a, &p.ka, &base, "Child before parent");
    let dir = tempfile::tempdir().unwrap();
    let cb = DeviceCert::new_at_micros(&p.master, p.kb.public(), 1, Some("B".into()));
    let ca = DeviceCert::new_at_micros(&p.master, p.ka.public(), 1, Some("A".into()));
    {
        let store = Store::open(dir.path()).unwrap();
        store.migrate().unwrap();
        store.bind_self_identity(&cb, &p.kb).unwrap();
        store.accept_self_device_certificate(&ca).unwrap();
        assert_eq!(
            store
                .accept_self_note_revision(&child, p.ka.public())
                .unwrap(),
            NoteReceive::Pending
        );
        assert_eq!(
            store
                .accept_self_note_revision(&child, p.ka.public())
                .unwrap(),
            NoteReceive::Pending
        );
        assert_eq!(store.get_note(&base.id).unwrap(), None);
    }
    let store = Store::open(dir.path()).unwrap();
    store.migrate().unwrap();
    assert_eq!(store.promote_pending_notes().unwrap().applied.len(), 0);
    store
        .accept_self_note_revision(&base, p.ka.public())
        .unwrap();
    let report = store.promote_pending_notes().unwrap();
    assert_eq!(report.applied, vec![base.id.clone()]);
    assert!(report.rejected.is_empty());
    assert_eq!(store.get_note(&base.id).unwrap(), Some(child));
    assert_eq!(
        store
            .query("?[id] := *note_incoming{id}")
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[test]
fn pending_article_dependencies_do_not_starve_later_ready_notes_and_removal_dominates() {
    let p = pair();
    p.a.query("?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <- [['item', 'article', 'Article', [], null, null, null, null, validity(1)]] :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}").unwrap();
    let article_note = root(&p, Some("item"), Some("item"));
    assert_eq!(
        p.b.accept_self_note_revision(&article_note, p.ka.public())
            .unwrap(),
        NoteReceive::Pending
    );
    // Blocked records sort before the ready work and exceed a promotion batch.
    // Readiness must be applied before the limit, rather than paging the queue.
    for index in 0..105 {
        let blocked = NoteContent {
            id: format!("!blocked-{index:04}"),
            body: format!("Waiting for article {index}"),
            target_kind: Some("item".into()),
            target_id: Some("item".into()),
            created_by: *p.ka.public(),
            created_at_micros: 1,
            updated_at_micros: 1,
            parents: Vec::new(),
        }
        .sign(&p.ka)
        .unwrap();
        assert_eq!(
            p.b.accept_self_note_revision(&blocked, p.ka.public())
                .unwrap(),
            NoteReceive::Pending
        );
    }
    let base = root(&p, None, None);
    let child = edit(&p.a, &p.ka, &base, "Independent ready child");
    p.b.accept_self_note_revision(&child, p.ka.public())
        .unwrap();
    p.b.accept_self_note_revision(&base, p.ka.public()).unwrap();
    assert_eq!(
        p.b.promote_pending_notes().unwrap().applied,
        vec![base.id.clone()]
    );
    p.b.query("?[id, tombstoned_at, reason] <- [['item', validity(2), null]] :put entity_tombstone {id, tombstoned_at => reason}").unwrap();
    assert!(p.b.promote_pending_notes().unwrap().applied.is_empty());
    assert_eq!(p.b.get_note(&article_note.id).unwrap(), None);
    assert_eq!(
        p.b.accept_self_note_revision(&article_note, p.ka.public())
            .unwrap(),
        NoteReceive::Removed
    );
    assert_eq!(p.b.get_note(&base.id).unwrap(), Some(child));
    assert_eq!(
        p.b.query("?[id] := *note_incoming{id}").unwrap().rows.len(),
        0
    );
}

#[test]
fn forged_foreign_and_revoked_senders_are_rejected_but_certified_historical_work_survives() {
    let p = pair();
    let base = root(&p, None, None);
    let stranger = Keypair::from_seed(&[54; 32]);
    assert!(matches!(
        p.b.accept_self_note_revision(&base, stranger.public()),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    let mut forged = base.clone();
    forged.body.push('!');
    assert!(matches!(
        p.b.accept_self_note_revision(&forged, p.ka.public()),
        Err(StoreError::NoteRevision(_))
    ));
    p.b.accept_self_revocation(&RevocationCert::new_at_micros(&p.master, p.ka.public(), 3))
        .unwrap();
    assert!(matches!(
        p.b.accept_self_note_revision(&base, p.ka.public()),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert_eq!(
        p.b.accept_self_note_revision(&base, p.kb.public()).unwrap(),
        NoteReceive::Applied
    );
    assert_eq!(p.b.get_note(&base.id).unwrap(), Some(base));
}

#[test]
fn resolutions_require_the_complete_sorted_unique_reviewed_head_set() {
    let p = pair();
    let base = root(&p, None, None);
    p.b.accept_self_note_revision(&base, p.ka.public()).unwrap();
    edit(&p.a, &p.ka, &base, "A");
    let other = edit(&p.b, &p.kb, &base, "B");
    p.a.accept_self_note_revision(&other, p.kb.public())
        .unwrap();
    let heads = p.a.note_heads(&base.id).unwrap();
    let ids: Vec<_> = heads
        .iter()
        .map(|record| record.revision_id.clone())
        .collect();
    for expected in [
        Vec::new(),
        vec![ids[0].clone()],
        vec![ids[0].clone(), ids[0].clone()],
        vec![ids[1].clone(), ids[0].clone()],
    ] {
        assert!(matches!(
            p.a.resolve_note(
                ResolveNoteInput {
                    id: base.id.clone(),
                    body: "Unreviewed".into(),
                    expected_revision_ids: expected,
                    now_micros: Some(3),
                },
                &p.ka
            ),
            Err(StoreError::AnnotationConflict { .. })
        ));
        assert_eq!(p.a.note_heads(&base.id).unwrap(), heads);
    }
    for body in [
        "\t \n".to_string(),
        "日".repeat(carrel_core::annotation::MAX_NOTE_BYTES),
    ] {
        assert!(matches!(
            p.a.resolve_note(
                ResolveNoteInput {
                    id: base.id.clone(),
                    body,
                    expected_revision_ids: ids.clone(),
                    now_micros: Some(3),
                },
                &p.ka
            ),
            Err(StoreError::InvalidInput { .. })
        ));
    }
    assert_eq!(p.a.note_heads(&base.id).unwrap(), heads);
}

#[test]
fn invalid_pending_ancestry_is_rejected_without_blocking_other_ready_work() {
    let p = pair();
    let base = root(&p, None, None);
    let invalid = NoteContent {
        id: base.id.clone(),
        body: "Changed creation metadata".into(),
        target_kind: None,
        target_id: None,
        created_by: base.created_by,
        created_at_micros: 0,
        updated_at_micros: 2,
        parents: vec![base.revision_id.clone()],
    }
    .sign(&p.ka)
    .unwrap();
    assert_eq!(
        p.b.accept_self_note_revision(&invalid, p.ka.public())
            .unwrap(),
        NoteReceive::Pending
    );
    let valid = edit(&p.a, &p.ka, &base, "Valid pending draft");
    assert_eq!(
        p.b.accept_self_note_revision(&valid, p.ka.public())
            .unwrap(),
        NoteReceive::Pending
    );
    p.b.accept_self_note_revision(&base, p.ka.public()).unwrap();
    let report = p.b.promote_pending_notes().unwrap();
    assert_eq!(report.applied, vec![base.id.clone()]);
    assert_eq!(report.rejected, vec![invalid.revision_id]);
    assert_eq!(p.b.get_note(&base.id).unwrap(), Some(valid));
}

#[test]
fn revocation_does_not_erase_already_verified_pending_history_and_tombstones_prevent_revival() {
    let p = pair();
    let base = root(&p, None, None);
    let child = edit(&p.a, &p.ka, &base, "Verified before revocation");
    p.b.accept_self_note_revision(&child, p.ka.public())
        .unwrap();
    p.b.accept_self_revocation(&RevocationCert::new_at_micros(&p.master, p.ka.public(), 3))
        .unwrap();
    p.b.accept_self_note_revision(&base, p.kb.public()).unwrap();
    assert_eq!(
        p.b.promote_pending_notes().unwrap().applied,
        vec![base.id.clone()]
    );
    assert_eq!(p.b.get_note(&base.id).unwrap(), Some(child.clone()));
    p.b.query_with_params("?[id, tombstoned_at, reason] <- [[$id, validity(1), null]] :put entity_tombstone {id, tombstoned_at => reason}", std::collections::BTreeMap::from([("id".into(), cozo::DataValue::from(base.id.as_str()))])).unwrap();
    assert_eq!(
        p.b.accept_self_note_revision(&child, p.kb.public())
            .unwrap(),
        NoteReceive::Removed
    );
    assert_eq!(p.b.get_note(&base.id).unwrap(), None);
    assert_eq!(p.b.note_revisions(&base.id).unwrap().len(), 2);
}

#[test]
fn a_trusted_forwarder_cannot_introduce_uncertified_creators_or_updaters() {
    let p = pair();
    let outsider = Keypair::from_seed(&[54; 32]);
    let foreign = NoteContent {
        id: "foreign".into(),
        body: "Another identity's draft".into(),
        target_kind: None,
        target_id: None,
        created_by: *outsider.public(),
        created_at_micros: 1,
        updated_at_micros: 1,
        parents: Vec::new(),
    }
    .sign(&outsider)
    .unwrap();
    assert!(matches!(
        p.b.accept_self_note_revision(&foreign, p.ka.public()),
        Err(StoreError::IdentityMasterMismatch)
    ));
    let base = root(&p, None, None);
    let foreign_update = NoteContent {
        id: base.id.clone(),
        body: "Uncertified update".into(),
        target_kind: None,
        target_id: None,
        created_by: base.created_by,
        created_at_micros: 1,
        updated_at_micros: 2,
        parents: vec![base.revision_id.clone()],
    }
    .sign(&outsider)
    .unwrap();
    assert!(matches!(
        p.b.accept_self_note_revision(&foreign_update, p.ka.public()),
        Err(StoreError::IdentityMasterMismatch)
    ));
    assert!(p.b.note_revisions(&base.id).unwrap().is_empty());
}

#[test]
fn delivery_order_retries_and_rolled_back_clocks_converge_on_the_same_reviewed_resolution() {
    let p = pair();
    let base = root(&p, None, None);
    p.b.accept_self_note_revision(&base, p.ka.public()).unwrap();
    let future =
        p.a.edit_note(
            EditNoteInput {
                id: base.id.clone(),
                body: "Future-clock draft".into(),
                expected_revision_id: base.revision_id.clone(),
                now_micros: Some(9_000_000),
            },
            &p.ka,
        )
        .unwrap();
    let other = edit(&p.b, &p.kb, &base, "Rolled-back clock draft");
    p.a.accept_self_note_revision(&other, p.kb.public())
        .unwrap();
    assert_eq!(p.a.note_heads(&base.id).unwrap().len(), 2);
    let resolved =
        p.a.resolve_note(
            ResolveNoteInput {
                id: base.id.clone(),
                body: "Reviewed resolution".into(),
                expected_revision_ids: p
                    .a
                    .note_heads(&base.id)
                    .unwrap()
                    .into_iter()
                    .map(|record| record.revision_id)
                    .collect(),
                now_micros: Some(-1_000_000),
            },
            &p.ka,
        )
        .unwrap();
    assert_eq!(resolved.updated_at_micros, 9_000_001);
    let records = [base.clone(), future, other, resolved.clone()];
    for order in [
        [0, 1, 2, 3],
        [3, 2, 1, 0],
        [1, 3, 0, 2],
        [2, 0, 3, 1],
        [0, 3, 2, 1],
        [3, 0, 1, 2],
    ] {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&p.master, p.kb.public(), 1, Some("B".into())),
                &p.kb,
            )
            .unwrap();
        store
            .accept_self_device_certificate(&DeviceCert::new_at_micros(
                &p.master,
                p.ka.public(),
                1,
                Some("A".into()),
            ))
            .unwrap();
        for index in order {
            for _ in 0..2 {
                store
                    .accept_self_note_revision(&records[index], p.ka.public())
                    .unwrap();
                assert!(store.promote_pending_notes().unwrap().rejected.is_empty());
            }
        }
        assert_eq!(store.get_note(&base.id).unwrap(), Some(resolved.clone()));
        assert_eq!(
            store.note_revisions(&base.id).unwrap(),
            p.a.note_revisions(&base.id).unwrap()
        );
        assert!(
            store
                .query("?[id] := *note_incoming{id}")
                .unwrap()
                .rows
                .is_empty()
        );
    }
}

#[test]
fn raw_store_operations_cannot_rewrite_ancestry_metadata_or_erase_signed_history() {
    use cozo::{DataValue, Validity};
    use std::collections::BTreeMap;

    let p = pair();
    let base = root(&p, None, None);
    let child = edit(&p.a, &p.ka, &base, "Signed child");
    let values = vec![
        DataValue::from(child.id.as_str()),
        DataValue::from(child.revision_id.as_str()),
        DataValue::from(child.body.as_str()),
        DataValue::Null,
        DataValue::Null,
        DataValue::Bytes(child.created_by.to_bytes().to_vec()),
        DataValue::Validity(Validity::from((child.created_at_micros, true))),
        DataValue::Validity(Validity::from((child.updated_at_micros, true))),
        DataValue::Bytes(child.updated_by.to_bytes().to_vec()),
        DataValue::from(3),
        DataValue::List(vec![DataValue::from(base.revision_id.as_str())]),
        DataValue::Bytes(child.signature.to_bytes().to_vec()),
    ];
    let write = "?[id, revision_id, body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature] <- $rows :put note {id, revision_id => body, target_kind, target_id, created_by, created_at, updated_at, updated_by, signature_version, parents, signature}";
    p.a.query_with_params(
        write,
        BTreeMap::from([(
            "rows".into(),
            DataValue::List(vec![DataValue::List(values.clone())]),
        )]),
    )
    .unwrap();
    for (index, replacement) in [
        (2, DataValue::from("changed")),
        (5, DataValue::Bytes(p.kb.public().to_bytes().to_vec())),
        (6, DataValue::Validity(Validity::from((0, true)))),
        (7, DataValue::Validity(Validity::from((10, true)))),
        (8, DataValue::Bytes(p.kb.public().to_bytes().to_vec())),
        (9, DataValue::from(2)),
        (10, DataValue::List(Vec::new())),
        (11, DataValue::Bytes(vec![0; 64])),
    ] {
        let mut changed = values.clone();
        changed[index] = replacement;
        assert!(
            p.a.query_with_params(
                write,
                BTreeMap::from([(
                    "rows".into(),
                    DataValue::List(vec![DataValue::List(changed)])
                )])
            )
            .is_err()
        );
    }
    assert!(
        p.a.query_with_params(
            "?[id, revision_id] <- [[$id, $rev]] :rm note {id, revision_id}",
            BTreeMap::from([
                ("id".into(), DataValue::from(base.id.as_str())),
                ("rev".into(), DataValue::from(base.revision_id.as_str())),
            ])
        )
        .is_err()
    );
    assert_eq!(p.a.get_note(&base.id).unwrap(), Some(child));
    assert_eq!(p.a.note_revisions(&base.id).unwrap().len(), 2);
}

#[test]
fn extreme_clocks_do_not_prevent_causal_edits_or_resolution() {
    for clock in [i64::MIN, i64::MAX] {
        let p = pair();
        let base =
            p.a.put_note(
                PutNoteInput {
                    body: "Boundary clock".into(),
                    target_kind: None,
                    target_id: None,
                    created_at_micros: Some(clock),
                },
                &p.ka,
            )
            .unwrap();
        p.b.accept_self_note_revision(&base, p.ka.public()).unwrap();
        let mut versions = Vec::new();
        for (store, key, body) in [(&p.a, &p.ka, "A"), (&p.b, &p.kb, "B")] {
            versions.push(
                store
                    .edit_note(
                        EditNoteInput {
                            id: base.id.clone(),
                            body: body.into(),
                            expected_revision_id: base.revision_id.clone(),
                            now_micros: Some(i64::MIN),
                        },
                        key,
                    )
                    .unwrap(),
            );
        }
        assert_ne!(versions[0].revision_id, versions[1].revision_id);
        assert_eq!(versions[0].updated_at_micros, clock.saturating_add(1));
        p.a.accept_self_note_revision(&versions[1], p.kb.public())
            .unwrap();
        p.b.accept_self_note_revision(&versions[0], p.ka.public())
            .unwrap();
        let resolved =
            p.a.resolve_note(
                ResolveNoteInput {
                    id: base.id.clone(),
                    body: "Reviewed at the boundary".into(),
                    expected_revision_ids: p
                        .a
                        .note_heads(&base.id)
                        .unwrap()
                        .into_iter()
                        .map(|record| record.revision_id)
                        .collect(),
                    now_micros: Some(i64::MIN),
                },
                &p.ka,
            )
            .unwrap();
        assert_eq!(resolved.updated_at_micros, clock.saturating_add(2));
        p.b.accept_self_note_revision(&resolved, p.ka.public())
            .unwrap();
        assert_eq!(p.b.get_note(&base.id).unwrap(), Some(resolved.clone()));
        let next =
            p.a.edit_note(
                EditNoteInput {
                    id: base.id.clone(),
                    body: resolved.body.clone(),
                    expected_revision_id: resolved.revision_id.clone(),
                    now_micros: Some(i64::MIN),
                },
                &p.ka,
            )
            .unwrap();
        assert_ne!(next.revision_id, resolved.revision_id);
        assert_eq!(p.a.note_revisions(&base.id).unwrap().len(), 5);
    }
}
