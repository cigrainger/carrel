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
fn multi_selection_deduplicates_and_separate_sends_never_disclose_other_recipients() {
    use carrel_core::sharing::{SendDraft, SendMode};
    let a = person(121);
    let b = person(123);
    let c = person(125);
    for friend in [&b, &c] {
        connect(&a, friend);
        connect(friend, &a);
    }
    source(
        &a.store,
        "original",
        "https://example.org/batch",
        "Batch article",
    );
    a.store
        .save_social_group(
            "essays",
            "Private group label",
            vec![*b.master.public(), *c.master.public()],
        )
        .unwrap();
    let selection = a
        .store
        .review_social_audience(vec![*b.master.public()], &["essays".into()])
        .unwrap();
    assert_eq!(selection.recipients(*a.master.public()).unwrap().len(), 2);
    let intent = SendDraft {
        request_id: "separate-batch".into(),
        item_id: "original".into(),
        note: Some("Worth reading".into()),
        selection: selection.clone(),
        mode: SendMode::Separately,
    };
    let article = draft(&a, *b.master.public(), "unused").article;
    let roots = a
        .store
        .create_social_send(intent.clone(), article.clone(), 100, &a.device)
        .unwrap();
    assert_eq!(roots.len(), 2);
    assert_ne!(roots[0].id, roots[1].id);
    for root in &roots {
        assert_eq!(root.version, 1);
        assert!(root.audience.is_empty());
        let (recipient, other) = if root.recipient == *b.master.public() {
            (&b, &c)
        } else {
            (&c, &b)
        };
        let fact = ShareFact::Share(Box::new(root.clone()));
        recipient
            .store
            .accept_social_record(&fact, a.master.public(), 101)
            .unwrap();
        assert!(
            other
                .store
                .accept_social_record(&fact, a.master.public(), 101)
                .is_err()
        );
        let bytes = fact.to_bytes().unwrap();
        assert!(
            !bytes
                .windows(32)
                .any(|window| window == other.master.public().as_bytes())
        );
        assert!(
            !bytes
                .windows(19)
                .any(|window| window == b"Private group label")
        );
    }
    let mut together = intent;
    together.request_id = "together-batch".into();
    together.mode = SendMode::Conversation;
    let roots = a
        .store
        .create_social_send(together, article, 102, &a.device)
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].version, 2);
    assert_eq!(
        roots[0].recipients(),
        selection.recipients(*a.master.public()).unwrap()
    );
    for recipient in [&b, &c] {
        recipient
            .store
            .accept_social_record(
                &ShareFact::Share(Box::new(roots[0].clone())),
                a.master.public(),
                103,
            )
            .unwrap();
    }
}

#[test]
fn send_retry_preserves_original_review_after_group_edits_and_refuses_changed_intent() {
    use carrel_core::sharing::{SendDraft, SendMode};
    let a = person(131);
    let b = person(133);
    let c = person(135);
    for friend in [&b, &c] {
        connect(&a, friend);
    }
    source(
        &a.store,
        "original",
        "https://example.org/retry",
        "Retry article",
    );
    a.store
        .save_social_group(
            "essays",
            "Essays",
            vec![*b.master.public(), *c.master.public()],
        )
        .unwrap();
    let selection = a
        .store
        .review_social_audience(vec![], &["essays".into()])
        .unwrap();
    let intent = SendDraft {
        request_id: "stable-batch".into(),
        item_id: "original".into(),
        note: Some("A note".into()),
        selection,
        mode: SendMode::Separately,
    };
    let article = draft(&a, *b.master.public(), "unused").article;
    a.store
        .save_social_group("essays", "Changed", vec![*b.master.public()])
        .unwrap();
    assert!(
        a.store
            .create_social_send(intent.clone(), article.clone(), 100, &a.device)
            .is_err()
    );
    assert!(a.store.social_history(None, 100).unwrap().is_empty());
    a.store
        .save_social_group(
            "essays",
            "Essays",
            vec![*b.master.public(), *c.master.public()],
        )
        .unwrap();
    let original = a
        .store
        .create_social_send(intent.clone(), article, 101, &a.device)
        .unwrap();
    a.store.remove_social_group("essays").unwrap();
    assert_eq!(
        a.store
            .social_send_retry(intent.clone(), a.device.public())
            .unwrap(),
        Some(original)
    );
    let mut changed = intent.clone();
    changed.mode = SendMode::Conversation;
    assert!(
        a.store
            .social_send_retry(changed, a.device.public())
            .is_err()
    );
    let mut changed = intent.clone();
    changed.note = Some("Different note".into());
    assert!(
        a.store
            .social_send_retry(changed, a.device.public())
            .is_err()
    );
    let mut changed = intent;
    changed.selection.groups[0].members.pop();
    assert!(
        a.store
            .social_send_retry(changed, a.device.public())
            .is_err()
    );
}

#[test]
fn a_failed_send_transaction_leaves_no_roots_or_retry_manifest() {
    use carrel_core::sharing::{SendDraft, SendMode};
    let a = person(141);
    let b = person(143);
    let c = person(145);
    for friend in [&b, &c] {
        connect(&a, friend);
    }
    source(
        &a.store,
        "original",
        "https://example.org/atomic",
        "Atomic article",
    );
    let selection = a
        .store
        .review_social_audience(vec![*b.master.public(), *c.master.public()], &[])
        .unwrap();
    let intent = SendDraft {
        request_id: "atomic-batch".into(),
        item_id: "original".into(),
        note: None,
        selection,
        mode: SendMode::Separately,
    };
    let article = draft(&a, *b.master.public(), "unused").article;
    let history_before = a
        .store
        .query("?[key,encoded] := *self_fact_history{key,encoded} :sort key")
        .unwrap()
        .rows;
    a.store
        .query(
            "::set_triggers self_fact_outbox on put { ?[key] := _new[key,encoded] :assert none }",
        )
        .unwrap();
    assert!(
        a.store
            .create_social_send(intent.clone(), article, 100, &a.device)
            .is_err()
    );
    assert!(a.store.social_history(None, 100).unwrap().is_empty());
    assert!(
        a.store
            .social_send_retry(intent, a.device.public())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        a.store
            .query("?[key,encoded] := *self_fact_history{key,encoded} :sort key")
            .unwrap()
            .rows,
        history_before
    );
}

#[test]
fn completed_send_manifest_and_original_proofs_survive_restart() {
    use carrel_core::sharing::{SendDraft, SendMode};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("send-store");
    let master = Keypair::from_seed(&[151; 32]);
    let device = Keypair::from_seed(&[152; 32]);
    let friend = person(153);
    let cert = DeviceCert::new_at_micros(&master, device.public(), 1, None);
    let (intent, roots) = {
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        store.bind_self_identity(&cert, &device).unwrap();
        store
            .follow_social_contact(*friend.master.public(), "Friend", None)
            .unwrap();
        source(
            &store,
            "original",
            "https://example.org/restart",
            "Restart article",
        );
        let selection = store
            .review_social_audience(vec![*friend.master.public()], &[])
            .unwrap();
        let intent = SendDraft {
            request_id: "restart-batch".into(),
            item_id: "original".into(),
            note: None,
            selection,
            mode: SendMode::Separately,
        };
        let article = public_article_snapshot(
            store.saved_article_snapshot("original").unwrap().unwrap(),
            vec![],
        )
        .unwrap();
        let roots = store
            .create_social_send(intent.clone(), article, 100, &device)
            .unwrap();
        (intent, roots)
    };
    let reopened = Store::open(&path).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(
        reopened.social_send_retry(intent, device.public()).unwrap(),
        Some(roots)
    );
}

#[test]
fn fixed_group_conversations_keep_ancestry_removals_and_private_self_history() {
    use carrel_core::{
        conversation::{ConversationContent as Content, ConversationRecord, ThreadScope},
        private_library::PrivateFact,
    };
    let a = person(21);
    let b = person(23);
    let c = person(25);
    let stranger = person(27);
    for friend in [&b, &c, &stranger] {
        connect(&a, friend);
        connect(friend, &a);
    }
    source(
        &a.store,
        "original",
        "https://example.org/thread",
        "Shared article",
    );
    let mut members = vec![*b.master.public(), *c.master.public()];
    members.sort();
    a.store
        .save_social_group("essays", "Essays", members.clone())
        .unwrap();
    let root = a
        .store
        .create_social_share_for(
            draft(&a, members[0], "group-root"),
            members.clone(),
            100,
            &a.device,
        )
        .unwrap();
    let root_fact = ShareFact::Share(Box::new(root.clone()));
    for recipient in [&b, &c] {
        recipient
            .store
            .accept_social_record(&root_fact, a.master.public(), 101)
            .unwrap();
    }
    assert!(
        stranger
            .store
            .accept_social_record(&root_fact, a.master.public(), 101)
            .is_err()
    );
    let first = b
        .store
        .reply_to_social_share(
            &root.id,
            "first",
            None,
            "One thoughtful reply — 文",
            102,
            &b.device,
        )
        .unwrap();
    let Content::Reply { id: first_id, .. } = &first.content else {
        panic!()
    };
    let second = b
        .store
        .reply_to_social_share(
            &root.id,
            "second",
            Some(first_id.clone()),
            "Reply with context",
            103,
            &b.device,
        )
        .unwrap();
    let Content::Reply { id: second_id, .. } = &second.content else {
        panic!()
    };
    assert_eq!(
        a.store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(second.clone())),
                b.master.public(),
                104
            )
            .unwrap(),
        SocialReceive::Pending
    );
    assert!(
        a.store
            .social_conversation(&root.id)
            .unwrap()
            .replies
            .is_empty()
    );
    a.store
        .accept_social_record(
            &ShareFact::Conversation(Box::new(first.clone())),
            b.master.public(),
            105,
        )
        .unwrap();
    assert!(a.store.promote_pending_social().unwrap());
    assert_eq!(
        a.store.social_reply_ancestry(&root.id, second_id).unwrap(),
        vec![second.clone(), first.clone()]
    );
    assert_eq!(
        b.store
            .reply_to_social_share(
                &root.id,
                "second",
                Some(first_id.clone()),
                "Reply with context",
                999,
                &b.device
            )
            .unwrap(),
        second
    );
    assert!(
        b.store
            .reply_to_social_share(
                &root.id,
                "second",
                Some(first_id.clone()),
                "Changed retry",
                999,
                &b.device
            )
            .is_err()
    );
    let scope = ThreadScope::from_share(&root).unwrap();
    let removal = ConversationRecord::sign(
        Content::Remove {
            scope: scope.clone(),
            reply_id: first_id.clone(),
        },
        -1,
        b.card.certificate.clone(),
        &b.device,
    )
    .unwrap();
    // C receives a verified member's records through the root sharer, without following B.
    assert_eq!(
        c.store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(removal.clone())),
                a.master.public(),
                106
            )
            .unwrap(),
        SocialReceive::Pending
    );
    c.store
        .accept_social_record(
            &ShareFact::Conversation(Box::new(first.clone())),
            a.master.public(),
            107,
        )
        .unwrap();
    c.store.promote_pending_social().unwrap();
    assert!(c.store.social_conversation(&root.id).unwrap().replies[0].removed);
    assert!(
        a.store
            .remove_social_reply(&root.id, first_id, 108, &a.device)
            .is_err()
    );
    assert!(
        stranger
            .store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(first.clone())),
                a.master.public(),
                108
            )
            .is_err()
    );
    let other = a
        .store
        .create_social_share(
            draft(&a, *b.master.public(), "other-audience"),
            109,
            &a.device,
        )
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(other.clone())),
            a.master.public(),
            110,
        )
        .unwrap();
    assert!(
        b.store
            .reply_to_social_share(
                &other.id,
                "cross-thread",
                Some(first_id.clone()),
                "Wrong thread",
                111,
                &b.device
            )
            .is_err()
    );
    // Changing/removing a selection cannot change the signed participants of a sent vouch.
    a.store
        .save_social_group("essays", "New essays", vec![*stranger.master.public()])
        .unwrap();
    a.store.remove_social_group("essays").unwrap();
    assert_eq!(
        a.store
            .social_conversation(&root.id)
            .unwrap()
            .share
            .recipients(),
        members
    );
    a.store
        .retract_social_share(&root.id, -10, &a.device)
        .unwrap();
    assert!(a.store.social_conversation(&root.id).unwrap().closed);
    let withdrawal = a
        .store
        .social_history(None, 100)
        .unwrap()
        .into_iter()
        .find(|fact| fact.kind() == "retraction" && fact.share_id() == root.id)
        .unwrap();
    b.store
        .accept_social_record(&withdrawal, a.master.public(), 113)
        .unwrap();
    let history = b
        .store
        .received_item_conversations(&carrel_store::ids::id_for_external(
            "https://example.org/thread",
        ))
        .unwrap();
    assert!(
        history
            .iter()
            .any(|thread| thread.id == root.id && thread.closed),
        "withdrawn conversations remain reachable from article Info"
    );
    assert!(
        a.store
            .reply_to_social_share(&root.id, "closed", None, "Too late", 112, &a.device)
            .is_err()
    );
    a.store
        .accept_social_record(
            &ShareFact::Conversation(Box::new(second.clone())),
            b.master.public(),
            113,
        )
        .unwrap();
    assert!(a.store.social_conversation(&root.id).unwrap().closed);
    // Another certified own desktop can receive original external signatures out of order.
    let own = Store::open_in_memory().unwrap();
    own.migrate().unwrap();
    let own_key = Keypair::from_seed(&[29; 32]);
    let own_cert = DeviceCert::new_at_micros(&a.master, own_key.public(), 1, None);
    own.bind_self_identity(&own_cert, &own_key).unwrap();
    own.accept_self_device_certificate(&a.card.certificate)
        .unwrap();
    own.accept_self_private_fact(
        &PrivateFact::Social(Box::new(ShareFact::Conversation(Box::new(second.clone())))),
        a.device.public(),
    )
    .unwrap();
    own.accept_self_private_fact(&PrivateFact::Social(Box::new(root_fact)), a.device.public())
        .unwrap();
    own.accept_self_private_fact(
        &PrivateFact::Social(Box::new(ShareFact::Conversation(Box::new(first)))),
        a.device.public(),
    )
    .unwrap();
    own.promote_pending_social().unwrap();
    assert_eq!(
        own.social_reply_ancestry(&root.id, second_id)
            .unwrap()
            .len(),
        2
    );
    own.accept_self_revocation(&carrel_core::identity::RevocationCert::new_at_micros(
        &a.master,
        a.device.public(),
        200,
    ))
    .unwrap();
    assert!(
        own.accept_self_private_fact(
            &PrivateFact::Social(Box::new(ShareFact::Share(Box::new(root.clone())))),
            a.device.public()
        )
        .is_err(),
        "a revoked live forwarding device cannot deliver social history"
    );
    assert_eq!(
        own.social_conversation(&root.id).unwrap().replies.len(),
        2,
        "revocation preserves previously accepted conversation work"
    );
}

#[test]
fn profiles_are_recipient_scoped_and_local_aliases_win_over_remote_names() {
    let a = person(51);
    let b = person(53);
    let c = person(55);
    connect(&a, &b);
    connect(&b, &a);
    connect(&c, &a);
    a.store
        .save_social_profile("Alice 文", None, 42, &a.device)
        .unwrap();
    let facts = a.store.social_history(None, 100).unwrap();
    let profile = facts
        .iter()
        .find(|f| f.kind() == "profile" && f.recipient() == *b.master.public())
        .unwrap();
    b.store
        .accept_social_record(profile, a.master.public(), 43)
        .unwrap();
    assert_eq!(b.store.social_name(a.master.public()).unwrap(), "My friend");
    assert!(
        c.store
            .accept_social_record(profile, a.master.public(), 43)
            .is_err()
    );
    assert_eq!(
        b.store
            .social_profile(a.master.public())
            .unwrap()
            .unwrap()
            .created_at_micros,
        42
    );
    a.store
        .save_social_profile("Later name", None, -1, &a.device)
        .unwrap();
    for fact in a
        .store
        .social_history(None, 100)
        .unwrap()
        .into_iter()
        .filter(|f| f.recipients().contains(b.master.public()))
    {
        b.store
            .accept_social_record(&fact, a.master.public(), 44)
            .unwrap();
    }
    assert!(
        matches!(b.store.social_profile(a.master.public()).unwrap().unwrap().content,carrel_core::conversation::ConversationContent::Profile{name,..} if name=="Later name")
    );
    let prefix = a.master.public().to_hex()[..12].to_owned();
    b.store
        .follow_social_contact(*a.master.public(), &prefix, None)
        .unwrap();
    assert_eq!(
        b.store.social_name(a.master.public()).unwrap(),
        prefix,
        "an explicit key-like alias must never be guessed away"
    );
    b.store
        .follow_social_contact(*c.master.public(), "", None)
        .unwrap();
    c.store
        .follow_social_contact(*b.master.public(), "Reader", None)
        .unwrap();
    c.store
        .save_social_profile("Carol", None, 45, &c.device)
        .unwrap();
    let profile = c
        .store
        .social_history(None, 100)
        .unwrap()
        .into_iter()
        .find(|fact| fact.kind() == "profile" && fact.recipient() == *b.master.public())
        .unwrap();
    b.store
        .accept_social_record(&profile, c.master.public(), 46)
        .unwrap();
    assert_eq!(
        b.store.social_name(c.master.public()).unwrap(),
        "Carol",
        "a new contact without an alias uses the signed profile name"
    );
}

#[test]
fn cyclic_and_cross_thread_pending_parents_are_rejected_without_becoming_visible() {
    use carrel_core::conversation::{
        ConversationContent as Content, ConversationRecord, ThreadScope,
    };
    let a = person(71);
    let b = person(73);
    connect(&a, &b);
    connect(&b, &a);
    source(
        &a.store,
        "original",
        "https://example.org/pending",
        "Pending",
    );
    let root = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "root"), 1, &a.device)
        .unwrap();
    let scope = ThreadScope::from_share(&root).unwrap();
    let first_id = carrel_core::sharing::Share::draft_id(&a.card.certificate, "first").unwrap();
    let second_id = carrel_core::sharing::Share::draft_id(&a.card.certificate, "second").unwrap();
    let first = ConversationRecord::sign(
        Content::Reply {
            scope: scope.clone(),
            id: first_id.clone(),
            request_id: "first".into(),
            parent: Some(second_id.clone()),
            body: "First".into(),
        },
        2,
        a.card.certificate.clone(),
        &a.device,
    )
    .unwrap();
    assert_eq!(
        b.store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(first.clone())),
                a.master.public(),
                3
            )
            .unwrap(),
        SocialReceive::Pending
    );
    let second = ConversationRecord::sign(
        Content::Reply {
            scope,
            id: second_id,
            request_id: "second".into(),
            parent: Some(first_id),
            body: "Would make a cycle".into(),
        },
        4,
        a.card.certificate.clone(),
        &a.device,
    )
    .unwrap();
    assert!(
        b.store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(second)),
                a.master.public(),
                5
            )
            .is_err()
    );
    // Shares and messages have separate typed namespaces even if a caller
    // deliberately reuses a request token under the same device.
    let same = a
        .store
        .reply_to_social_share(
            &root.id,
            "root",
            None,
            "Separate message namespace",
            10,
            &a.device,
        )
        .unwrap();
    let Content::Reply { id: same_id, .. } = &same.content else {
        panic!()
    };
    assert_eq!(same_id, &root.id);
    a.store
        .reply_to_social_share(
            &root.id,
            "reply-to-same",
            Some(same_id.clone()),
            "Parent lookup uses the reply namespace",
            11,
            &a.device,
        )
        .unwrap();
    b.store
        .accept_social_record(
            &ShareFact::Share(Box::new(root.clone())),
            a.master.public(),
            6,
        )
        .unwrap();
    let fork = carrel_core::sharing::Share::sign(
        root.request_id.clone(),
        root.recipient,
        root.article.clone(),
        Some("A different opening under the same draft ID".into()),
        root.created_at_micros,
        a.card.certificate.clone(),
        &a.device,
    )
    .unwrap();
    assert_eq!(fork.id, root.id);
    let forked_reply = ConversationRecord::sign(
        Content::Reply {
            scope: ThreadScope::from_share(&fork).unwrap(),
            id: carrel_core::sharing::Share::draft_id(&a.card.certificate, "forked").unwrap(),
            request_id: "forked".into(),
            parent: None,
            body: "A reply to an incompatible original proof".into(),
        },
        6,
        a.card.certificate.clone(),
        &a.device,
    )
    .unwrap();
    assert!(
        b.store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(forked_reply)),
                a.master.public(),
                6
            )
            .is_err()
    );
    assert!(!b.store.promote_pending_social().unwrap());
    assert!(
        b.store
            .social_conversation(&root.id)
            .unwrap()
            .replies
            .is_empty()
    );
    let other = a
        .store
        .create_social_share(draft(&a, *b.master.public(), "different"), 7, &a.device)
        .unwrap();
    let Content::Reply { id, .. } = &first.content else {
        panic!()
    };
    let cross = ConversationRecord::sign(
        Content::Reply {
            scope: ThreadScope::from_share(&other).unwrap(),
            id: carrel_core::sharing::Share::draft_id(&a.card.certificate, "cross").unwrap(),
            request_id: "cross".into(),
            parent: Some(id.clone()),
            body: "Not this audience".into(),
        },
        8,
        a.card.certificate.clone(),
        &a.device,
    )
    .unwrap();
    assert!(
        b.store
            .accept_social_record(
                &ShareFact::Conversation(Box::new(cross)),
                a.master.public(),
                9
            )
            .is_err()
    );
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
