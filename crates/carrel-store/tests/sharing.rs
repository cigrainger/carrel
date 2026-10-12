use carrel_core::identity::{DeviceCert, Keypair, PublicKey};
use carrel_core::private_library::BlobReference;
use carrel_core::sharing::{ContactCard, ShareFact, ShareRetraction, public_article_snapshot};
use carrel_store::{
    Store,
    sharing::{ShareDraft, SocialReceive},
};

struct Person {
    store: Store,
    master: Keypair,
    device: Keypair,
    card: ContactCard,
}
fn person(seed: u8) -> Person {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    let master = Keypair::from_seed(&[seed; 32]);
    let device = Keypair::from_seed(&[seed + 1; 32]);
    let certificate = DeviceCert::new_at_micros(&master, device.public(), 1, None);
    store.bind_self_identity(&certificate, &device).unwrap();
    let card =
        ContactCard::sign(certificate, "public connection information".into(), &device).unwrap();
    Person {
        store,
        master,
        device,
        card,
    }
}
fn connect(a: &Person, b: &Person) {
    a.store
        .follow_social_contact(*b.master.public(), "My friend", Some(&b.card))
        .unwrap();
}
fn source(store: &Store, id: &str, url: &str, title: &str) {
    store.query_with_params("{ ?[id,kind,title,creators,primary_url,published_at,language,summary,discovered_at] <- [[$id,'article',$title,['Publisher'],$url,validity(42),'en','Public excerpt',validity(99)]] :put item {id => kind,title,creators,primary_url,published_at,language,summary,discovered_at} } { ?[item_id,feed_url,word_count,estimated_read_minutes,site_name,byline] <- [[$id,'https://private.example/feed',500,3,'Publisher','Author']] :put item_article {item_id => feed_url,word_count,estimated_read_minutes,site_name,byline} } { ?[item_id,format,blob_id,fetched_at,extracted_with,byte_size] <- [[$id,'html_readable',$blob,validity(98),'test',123]] :put item_content {item_id,format => blob_id,fetched_at,extracted_with,byte_size} }",std::collections::BTreeMap::from([("id".into(),cozo::DataValue::from(id)),("url".into(),cozo::DataValue::from(url)),("title".into(),cozo::DataValue::from(title)),("blob".into(),cozo::DataValue::from("b".repeat(64)))])).unwrap();
}
fn draft(a: &Person, recipient: PublicKey, token: &str) -> ShareDraft {
    let article = a.store.saved_article_snapshot("original").unwrap().unwrap();
    ShareDraft {
        request_id: token.into(),
        recipient,
        article: public_article_snapshot(
            article,
            vec![BlobReference {
                blob_id: "c".repeat(64),
                byte_size: 50,
            }],
        )
        .unwrap(),
        note: Some("A thoughtful vouch — 文".into()),
    }
}

#[test]
fn received_library_joins_latest_vouch_and_paginates_before_withdrawal() {
    use carrel_core::library::ItemFilter;
    let a = person(121);
    let b = person(123);
    connect(&a, &b);
    connect(&b, &a);
    source(&a.store, "original", "https://example.org/first", "First");
    let first = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "first"), 10, &a.device)
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(first.clone())),
            a.master.public(),
            20,
        )
        .unwrap();
    let mut second_draft = draft(&a, *b.master.public(), "second");
    second_draft.article.id = "second".into();
    source(&a.store, "second", "https://example.org/second", "Second");
    second_draft.article = public_article_snapshot(
        a.store.saved_article_snapshot("second").unwrap().unwrap(),
        vec![],
    )
    .unwrap();
    let second = a
        .store
        .create_social_share(second_draft, 30, &a.device)
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(second.clone())),
            a.master.public(),
            40,
        )
        .unwrap();
    let mut filter = ItemFilter::shared();
    filter.limit = Some(1);
    let page = b.store.list_item_page(&filter, 0).unwrap();
    assert_eq!(page.items[0].title, "Second");
    assert_eq!(page.items[0].shared.as_ref().unwrap().id, second.id);
    assert_eq!(page.items[0].read_state, "unread");
    assert!(!page.items[0].starred);
    filter.after = page.next_cursor;
    let next = b.store.list_item_page(&filter, 0).unwrap();
    assert_eq!(next.items[0].title, "First");
    assert!(next.next_cursor.is_none());
    let latest = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "latest"), 50, &a.device)
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(latest.clone())),
            a.master.public(),
            60,
        )
        .unwrap();
    let page = b.store.list_item_page(&ItemFilter::shared(), 0).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].shared.as_ref().unwrap().id, latest.id);
    let retraction = ShareRetraction::sign(
        latest.id.clone(),
        latest.recipient,
        70,
        a.card.certificate.clone(),
        &a.device,
    )
    .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Retraction(Box::new(retraction)),
            a.master.public(),
            80,
        )
        .unwrap();
    let page = b.store.list_item_page(&ItemFilter::shared(), 0).unwrap();
    assert_eq!(page.items[0].title, "Second");
    assert_eq!(page.items[1].shared.as_ref().unwrap().id, first.id);
    let pending = b.store.pending_social_blobs(a.master.public(), 1).unwrap();
    let older = b
        .store
        .pending_social_blobs_after(a.master.public(), 100, pending.last())
        .unwrap();
    assert!(!older.is_empty());
    assert!(
        !older
            .iter()
            .any(|blob| blob.share_id == pending[0].share_id
                && blob.reference == pending[0].reference)
    );
}

#[test]
fn sent_history_reaches_older_vouches_with_stable_ties_withdrawals_and_new_arrivals() {
    use carrel_store::sharing::SentShareCursor;
    let a = person(131);
    let b = person(133);
    let c = person(135);
    connect(&a, &b);
    connect(&a, &c);
    connect(&c, &a);
    source(
        &a.store,
        "original",
        "https://example.org/history",
        "Café 文",
    );
    let mut expected = Vec::new();
    for index in 0..151 {
        let at = match index {
            0 => i64::MIN,
            150 => i64::MAX,
            _ => (index / 3) as i64,
        };
        let share = a
            .store
            .create_social_share(
                draft(&a, *b.master.public(), &format!("history-{index}")),
                at,
                &a.device,
            )
            .unwrap();
        expected.push((at, format!("share/{}", share.id), share.id));
    }
    expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    a.store
        .retract_social_share(&expected[140].2, 3, &a.device)
        .unwrap();
    a.store.unfollow_social_contact(b.master.public()).unwrap();
    source(
        &c.store,
        "original",
        "https://example.org/foreign",
        "Another person's vouch",
    );
    let foreign = c
        .store
        .create_social_share(
            draft(&c, *a.master.public(), "foreign"),
            i64::MAX,
            &c.device,
        )
        .unwrap();
    a.store
        .accept_social_record(&ShareFact::Share(Box::new(foreign)), c.master.public(), 42)
        .unwrap();
    let first = a.store.sent_social_share_page(None, 50).unwrap();
    assert_eq!(first.shares.len(), 50);
    let cursor = first.next_cursor.unwrap();
    // A new prefix entry must not shift an already issued exclusive cursor.
    a.store
        .follow_social_contact(*b.master.public(), "Renamed friend", None)
        .unwrap();
    a.store
        .create_social_share(
            draft(&a, *b.master.public(), "new-prefix"),
            i64::MAX,
            &a.device,
        )
        .unwrap();
    let mut ids = first
        .shares
        .into_iter()
        .map(|sent| sent.share.id)
        .collect::<Vec<_>>();
    let mut after = Some(cursor);
    let mut found_withdrawal = false;
    while let Some(cursor) = after {
        let page = a.store.sent_social_share_page(Some(&cursor), 50).unwrap();
        assert!(page.shares.iter().all(|sent| sent.name == "Renamed friend"));
        for sent in page.shares {
            if sent.share.id == expected[140].2 {
                assert!(sent.retracted);
                found_withdrawal = true;
            }
            ids.push(sent.share.id);
        }
        after = page.next_cursor;
    }
    assert_eq!(
        ids,
        expected
            .into_iter()
            .map(|(_, _, id)| id)
            .collect::<Vec<_>>()
    );
    assert!(found_withdrawal);
    assert!(
        a.store
            .sent_social_share_page(
                Some(&SentShareCursor {
                    at_micros: 0,
                    key: "share/' :rm social_record".into()
                }),
                50
            )
            .is_err()
    );
    assert_eq!(
        a.store
            .sent_social_share_page(None, 0)
            .unwrap()
            .shares
            .len(),
        1
    );
    assert_eq!(
        a.store
            .sent_social_share_page(None, usize::MAX)
            .unwrap()
            .shares
            .len(),
        100
    );
    assert!(
        b.store
            .sent_social_share_page(None, 50)
            .unwrap()
            .shares
            .is_empty()
    );
}

#[test]
fn explicit_contacts_pin_master_identity_without_granting_private_device_access() {
    let a = person(61);
    let b = person(63);
    let contact = a
        .store
        .follow_social_contact(*b.master.public(), "  My friend  ", None)
        .unwrap();
    assert!(contact.card.is_none());
    assert_eq!(contact.name, "My friend");
    a.store.complete_social_contact(&b.card).unwrap();
    assert!(!a.store.self_device_authorized(b.device.public()).unwrap());
    assert_eq!(
        a.store.self_identity().unwrap().unwrap().master_pubkey,
        *a.master.public()
    );
    assert!(
        a.store
            .follow_social_contact(*a.master.public(), "me", Some(&a.card))
            .is_err()
    );
    assert!(
        a.store
            .follow_social_contact(*b.master.public(), "friend", Some(&a.card))
            .is_err()
    );
    let stranger = person(65);
    assert!(a.store.complete_social_contact(&stranger.card).is_err());
    a.store.unfollow_social_contact(b.master.public()).unwrap();
    assert!(a.store.complete_social_contact(&b.card).is_err());
    assert!(
        a.store
            .authorize_social_recipient(b.master.public())
            .is_err()
    );
}

#[test]
fn a_share_retry_is_idempotent_and_its_recipient_and_content_remain_explicit() {
    let a = person(71);
    let b = person(73);
    let c = person(75);
    connect(&a, &b);
    connect(&a, &c);
    source(
        &a.store,
        "original",
        "https://example.org/essay",
        "Original title",
    );
    let first = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "draft-1"), 10, &a.device)
        .unwrap();
    let retry = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "draft-1"), 999, &a.device)
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(a.store.social_history(None, 100).unwrap().len(), 1);
    assert!(
        a.store
            .create_social_share(draft(&a, *c.master.public(), "draft-1"), 11, &a.device)
            .is_err()
    );
    let mut changed = draft(&a, *b.master.public(), "draft-1");
    changed.note = Some("changed draft".into());
    assert!(a.store.create_social_share(changed, 11, &a.device).is_err());
    assert!(
        a.store
            .authorized_social_blob(c.master.public(), &first.id, &"b".repeat(64))
            .is_err()
    );
    assert_eq!(
        a.store
            .authorized_social_blob(b.master.public(), &first.id, &"b".repeat(64))
            .unwrap()
            .byte_size,
        123
    );
    assert!(
        a.store
            .authorized_social_blob(b.master.public(), &first.id, &"d".repeat(64))
            .is_err()
    );
    assert!(
        a.store
            .create_social_share(
                draft(&a, *b.master.public(), "new-draft"),
                12,
                &Keypair::from_seed(&[99; 32])
            )
            .is_err()
    );
    a.store.unfollow_social_contact(b.master.public()).unwrap();
    assert!(
        a.store
            .authorized_social_blob(b.master.public(), &first.id, &"b".repeat(64))
            .is_err()
    );
    assert!(
        a.store
            .create_social_share(draft(&a, *b.master.public(), "new-draft"), 12, &a.device)
            .is_err()
    );
}

#[test]
fn receipts_keep_existing_archive_identity_state_cache_and_annotations_private() {
    let a = person(81);
    let b = person(83);
    let c = person(85);
    connect(&a, &b);
    connect(&b, &a);
    connect(&c, &a);
    source(
        &a.store,
        "original",
        "https://example.org/essay",
        "Sender title",
    );
    source(
        &b.store,
        "old-saved",
        "https://example.org/essay",
        "My existing title",
    );
    b.store
        .write_private_state(
            carrel_core::private_state::StateKey {
                kind: carrel_core::private_state::StateKind::Read,
                target: "old-saved".into(),
            },
            carrel_core::private_state::StateValue::Read(
                carrel_core::private_state::ReadIntent::Read,
            ),
            100,
            &b.device,
        )
        .unwrap();
    b.store
        .toggle_private_star("old-saved", 101, &b.device)
        .unwrap();
    let before = b.store.get_item_detail("old-saved").unwrap().unwrap();
    let share = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "draft"), 10, &a.device)
        .unwrap();
    let fact = ShareFact::Share(Box::new(share.clone()));
    assert!(
        c.store
            .accept_social_record(&fact, a.master.public(), 20)
            .is_err()
    );
    assert_eq!(
        b.store
            .accept_social_record(&fact, a.master.public(), 20)
            .unwrap(),
        SocialReceive::Applied
    );
    assert_eq!(
        b.store
            .accept_social_record(&fact, a.master.public(), 21)
            .unwrap(),
        SocialReceive::Existing
    );
    assert_eq!(
        b.store.get_item_detail("old-saved").unwrap().unwrap(),
        before
    );
    let notes = b.store.received_item_shares("old-saved", 100).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].share.note, share.note);
    assert_eq!(notes[0].item_id, "old-saved");
    assert_eq!(notes[0].name, "My friend");
    assert_eq!(
        b.store
            .pending_social_blobs(a.master.public(), 100)
            .unwrap()
            .len(),
        2
    );
    b.store
        .complete_social_blob(&share.id, &"b".repeat(64))
        .unwrap();
    assert_eq!(
        b.store
            .pending_social_blobs(a.master.public(), 100)
            .unwrap()
            .len(),
        1
    );
    assert!(
        b.store.social_history(None, 100).unwrap().is_empty(),
        "received vouches are not republished"
    );
    b.store.unfollow_social_contact(a.master.public()).unwrap();
    assert_eq!(
        b.store.get_item_detail("old-saved").unwrap().unwrap(),
        before
    );
    assert!(
        b.store
            .accept_social_record(&fact, a.master.public(), 30)
            .is_err()
    );
}

#[test]
fn retractions_dominate_reordering_and_cannot_withdraw_another_authors_share() {
    let a = person(91);
    let b = person(93);
    let c = person(95);
    connect(&a, &b);
    connect(&b, &a);
    connect(&b, &c);
    source(
        &a.store,
        "original",
        "https://example.org/essay",
        "Sender title",
    );
    let share = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "draft"), 1000, &a.device)
        .unwrap();
    a.store
        .retract_social_share(&share.id, -1000, &a.device)
        .unwrap();
    let history = a.store.social_history(None, 100).unwrap();
    let retraction = history
        .iter()
        .find(|fact| matches!(fact, ShareFact::Retraction(_)))
        .unwrap();
    b.store
        .accept_social_record(retraction, a.master.public(), 1)
        .unwrap();
    assert_eq!(
        b.store
            .accept_social_record(
                &ShareFact::Share(Box::new(share.clone())),
                a.master.public(),
                2
            )
            .unwrap(),
        SocialReceive::Retracted
    );
    assert!(
        b.store
            .query("?[id] := *social_receipt{share_id:id}")
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(b.store.query("?[id] := *item{id}").unwrap().rows.is_empty());
    assert!(
        a.store
            .authorized_social_blob(b.master.public(), &share.id, &"b".repeat(64))
            .is_err()
    );
    let later = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "later"), 1001, &a.device)
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(later.clone())),
            a.master.public(),
            3,
        )
        .unwrap();
    let local = carrel_store::ids::id_for_external("https://example.org/essay");
    assert_eq!(b.store.received_item_shares(&local, 100).unwrap().len(), 1);
    let certificate = c.store.social_local_certificate(c.device.public()).unwrap();
    let forged_owner = ShareFact::Retraction(Box::new(
        ShareRetraction::sign(
            later.id.clone(),
            *b.master.public(),
            4,
            certificate,
            &c.device,
        )
        .unwrap(),
    ));
    assert!(
        b.store
            .accept_social_record(&forged_owner, c.master.public(), 4)
            .is_err()
    );
    a.store
        .retract_social_share(&later.id, -2000, &a.device)
        .unwrap();
    let retract = a
        .store
        .social_history(None, 100)
        .unwrap()
        .into_iter()
        .find(|fact| matches!(fact, ShareFact::Retraction(_)) && fact.share_id() == later.id)
        .unwrap();
    b.store
        .accept_social_record(&retract, a.master.public(), 5)
        .unwrap();
    assert!(
        b.store
            .received_item_shares(&local, 100)
            .unwrap()
            .is_empty()
    );
    assert!(
        b.store.get_item_detail(&local).unwrap().is_some(),
        "withdrawal preserves the recipient's archive"
    );
    assert!(
        b.store
            .pending_social_blobs(a.master.public(), 100)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejected_signatures_private_fields_and_storage_failure_leave_no_partial_receipt() {
    let a = person(101);
    let b = person(103);
    connect(&a, &b);
    connect(&b, &a);
    source(
        &a.store,
        "original",
        "https://example.org/essay",
        "Sender title",
    );
    let share = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "draft"), 1, &a.device)
        .unwrap();
    let mut forged = share.clone();
    forged.note = Some("tampered".into());
    assert!(
        b.store
            .accept_social_record(&ShareFact::Share(Box::new(forged)), a.master.public(), 2)
            .is_err()
    );
    b.store
        .query("::set_triggers social_receipt on put { ?[id] := _new[id,item] :assert none }")
        .unwrap();
    assert!(
        b.store
            .accept_social_record(&ShareFact::Share(Box::new(share)), a.master.public(), 2)
            .is_err()
    );
    for query in [
        "?[id] := *item{id}",
        "?[key] := *social_record{key}",
        "?[id] := *social_blob_pending{share_id:id}",
    ] {
        assert!(b.store.query(query).unwrap().rows.is_empty());
    }
}

#[test]
fn received_content_is_installed_only_after_transfer_and_scopes_survive_self_sync() {
    use carrel_core::private_library::PrivateFact;
    use carrel_store::private_library::ItemAdoption;
    let a = person(111);
    let b = person(113);
    connect(&a, &b);
    connect(&b, &a);
    source(
        &a.store,
        "original",
        "https://example.org/essay",
        "Sender title",
    );
    let share = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "scope"), 1, &a.device)
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(share.clone())),
            a.master.public(),
            2,
        )
        .unwrap();
    let id = carrel_store::ids::id_for_external("https://example.org/essay");
    assert!(
        b.store
            .get_item_detail(&id)
            .unwrap()
            .unwrap()
            .readable
            .is_none(),
        "metadata alone cannot borrow a cached body by its hash"
    );
    b.store
        .complete_social_blob(&share.id, &"b".repeat(64))
        .unwrap();
    assert_eq!(
        b.store.current_image_scope(&id, &"b".repeat(64)).unwrap(),
        Some(vec![])
    );
    let mut unapproved = public_article_snapshot(
        b.store.saved_article_snapshot(&id).unwrap().unwrap(),
        vec![BlobReference {
            blob_id: "c".repeat(64),
            byte_size: 50,
        }],
    )
    .unwrap();
    connect(&b, &a);
    assert!(
        b.store
            .create_social_share(
                ShareDraft {
                    request_id: "too-early".into(),
                    recipient: *a.master.public(),
                    article: unapproved.clone(),
                    note: None
                },
                3,
                &b.device
            )
            .is_err(),
        "an existing unrelated cache file is not an image grant"
    );
    b.store
        .complete_social_blob(&share.id, &"c".repeat(64))
        .unwrap();
    let scope = b
        .store
        .current_image_scope(&id, &"b".repeat(64))
        .unwrap()
        .unwrap();
    assert_eq!(
        scope,
        vec![BlobReference {
            blob_id: "c".repeat(64),
            byte_size: 50
        }]
    );
    let capture = b.store.capture_private_item(&id).unwrap().unwrap();
    let mut resources = scope.clone();
    resources.push(BlobReference {
        blob_id: "d".repeat(64),
        byte_size: 60,
    });
    let ItemAdoption::Recorded(record) = b
        .store
        .adopt_private_item(capture, resources, 4, &b.device)
        .unwrap()
    else {
        panic!("recorded scoped snapshot")
    };
    assert_eq!(record.version, 2);
    let second = Store::open_in_memory().unwrap();
    second.migrate().unwrap();
    let key = Keypair::from_seed(&[120; 32]);
    second
        .bind_self_identity(
            &DeviceCert::new_at_micros(&b.master, key.public(), 1, None),
            &key,
        )
        .unwrap();
    second
        .accept_self_device_certificate(
            &b.store.social_local_certificate(b.device.public()).unwrap(),
        )
        .unwrap();
    second
        .accept_self_private_fact(&PrivateFact::Item((*record).clone()), b.device.public())
        .unwrap();
    assert_eq!(
        second.current_image_scope(&id, &"b".repeat(64)).unwrap(),
        Some(scope)
    );
    let snapshot = second.saved_article_snapshot(&id).unwrap().unwrap();
    assert_eq!(snapshot.current_images.unwrap()[0].blob_id, "c".repeat(64));
    unapproved.resources = vec![BlobReference {
        blob_id: "d".repeat(64),
        byte_size: 60,
    }];
    unapproved.current_images = Some(unapproved.resources.clone());
    assert!(
        b.store
            .create_social_share(
                ShareDraft {
                    request_id: "historical-image".into(),
                    recipient: *a.master.public(),
                    article: unapproved,
                    note: None
                },
                5,
                &b.device
            )
            .is_err()
    );
}
