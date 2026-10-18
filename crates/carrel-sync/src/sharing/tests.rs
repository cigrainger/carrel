use super::*;
use crate::transport::SelfTransport;
use carrel_core::identity::DeviceCert;
use carrel_core::private_library::BlobReference;
use carrel_core::sharing::public_article_snapshot;
use carrel_store::sharing::ShareDraft;
use iroh::{SecretKey, endpoint::presets};

struct Person {
    store: Store,
    master: Keypair,
    device: Keypair,
    cache: NativeCache,
    host: SelfTransport,
    service: Arc<SocialBridge>,
}
async fn person(path: &std::path::Path, seed: u8) -> Person {
    let store = Store::open(&path.join("store")).unwrap();
    store.migrate().unwrap();
    let master = Keypair::from_seed(&[seed; 32]);
    let device = Keypair::from_seed(&[seed + 1; 32]);
    store
        .bind_self_identity(
            &DeviceCert::new_at_micros(&master, device.public(), 1, None),
            &device,
        )
        .unwrap();
    let cache = NativeCache::open(path.join("cache"));
    let host = SelfTransport::start_with_sharing_builder(
        store.clone(),
        &path.join("mirror"),
        &device,
        cache.clone(),
        Endpoint::builder(presets::Minimal)
            .bind_addr("127.0.0.1:0")
            .unwrap(),
    )
    .await
    .unwrap();
    let service = host.sharing().unwrap();
    Person {
        store,
        master,
        device,
        cache,
        host,
        service,
    }
}
fn follow(a: &Person, b: &Person) {
    a.store
        .follow_social_contact(
            *b.master.public(),
            "Trusted person",
            Some(&b.service.contact_card().unwrap()),
        )
        .unwrap();
}
fn article(
    a: &Person,
    url: &str,
    html: &str,
    images: Vec<BlobReference>,
    token: &str,
    recipient: PublicKey,
) -> carrel_core::sharing::Share {
    let id = carrel_store::ids::id_for_external(url);
    let blob = a.cache.put_blocking(html.as_bytes()).unwrap();
    a.store
        .save_archived_article(
            &carrel_core::article::ArchivedArticle {
                url: url.into(),
                aliases: vec![],
                title: Some("An article".into()),
                creators: vec![],
                published_at_micros: Some(42),
                language: Some("en".into()),
                summary: Some("A public excerpt".into()),
                site_name: None,
                byline: None,
                blob_id: blob.to_string(),
                byte_size: html.len() as i64,
                extracted_with: "test".into(),
                word_count: 20,
                estimated_read_minutes: 1,
                searchable_text: "Public article text".into(),
                shape: Default::default(),
            },
            99,
            None,
        )
        .unwrap();
    let snapshot = public_article_snapshot(
        a.store.saved_article_snapshot(&id).unwrap().unwrap(),
        images,
    )
    .unwrap();
    a.store
        .create_social_share(
            ShareDraft {
                request_id: token.into(),
                recipient,
                article: snapshot,
                note: Some("Read this for the argument in its middle section.".into()),
            },
            100,
            &a.device,
        )
        .unwrap()
}

async fn exchange(from: &Person, to: &Person) {
    let mut cursor = None;
    loop {
        cursor = from.service.publish(cursor).await.unwrap();
        if cursor.is_none() {
            break;
        }
    }
    let mut cursor = None;
    loop {
        let (next, _) = to
            .service
            .receive(&from.service.contact_card().unwrap(), cursor)
            .await
            .unwrap();
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
}

fn group_share(
    author: &Person,
    article: &carrel_core::sharing::Share,
    recipients: &[&Person],
    request: &str,
) -> carrel_core::sharing::Share {
    let mut recipients = recipients
        .iter()
        .map(|person| *person.master.public())
        .collect::<Vec<_>>();
    recipients.sort();
    author
        .store
        .create_social_share_for(
            ShareDraft {
                request_id: request.into(),
                recipient: recipients[0],
                article: article.article.clone(),
                note: Some("A deliberate introduction — 文".into()),
            },
            recipients,
            101,
            &author.device,
        )
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_reply_queue_reaches_a_later_root_page_and_retries_deferred_bodies_on_the_next_scan() {
    use carrel_core::conversation::{ConversationContent, ConversationRecord, ThreadScope};
    use carrel_store::sharing::SocialReceive;
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 241).await;
    let b = person(&dir.path().join("b"), 243).await;
    follow(&a, &b);
    follow(&b, &a);
    let root = article(
        &a,
        "https://example.org/backpressure",
        "<p>A retained article.</p>",
        vec![],
        "backpressure-root",
        *b.master.public(),
    );
    let scope = ThreadScope::from_share(&root).unwrap();
    let certificate = a.store.social_local_certificate(a.device.public()).unwrap();
    // These are genuine signed peer candidates received before their root;
    // none is accepted history or an admission on the receiving desktop.
    for index in 0..512 {
        let token = format!("staged-{index}");
        let fact = ShareFact::Conversation(Box::new(
            ConversationRecord::sign(
                ConversationContent::Reply {
                    scope: scope.clone(),
                    id: carrel_core::sharing::Share::draft_id(&certificate, &token).unwrap(),
                    request_id: token,
                    parent: None,
                    body: "An earlier signed reply".into(),
                },
                101,
                certificate.clone(),
                &a.device,
            )
            .unwrap(),
        ));
        assert_eq!(
            b.store
                .accept_social_record(&fact, a.master.public(), 101, &b.device)
                .unwrap(),
            SocialReceive::Pending
        );
    }
    let mut deferred = Vec::new();
    for index in 0..10 {
        deferred.push(
            a.store
                .reply_to_social_share(
                    &root.id,
                    &format!("page-{index}"),
                    None,
                    "A body on the first full page",
                    102,
                    &a.device,
                )
                .unwrap(),
        );
    }
    let mut cursor = None;
    loop {
        cursor = a.service.publish(cursor).await.unwrap();
        if cursor.is_none() {
            break;
        }
    }
    let card = a.service.contact_card().unwrap();
    let (after, _) = b.service.receive(&card, None).await.unwrap();
    assert!(after.as_ref().unwrap().starts_with("reply/"));
    assert!(
        b.store
            .social_record(&format!("share/{}", root.id))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        b.store
            .query("?[key] := *social_pending{key}")
            .unwrap()
            .rows
            .len(),
        512
    );
    for record in &deferred {
        assert!(
            b.store
                .social_record(&record.key().unwrap())
                .unwrap()
                .is_none()
        );
    }

    let (after, changed) = b.service.receive(&card, after).await.unwrap();
    assert!(changed);
    assert_eq!(
        b.store.social_conversation(&root.id).unwrap().replies.len(),
        512
    );
    assert!(
        b.store
            .query("?[key] := *social_pending{key}")
            .unwrap()
            .rows
            .is_empty()
    );
    let (after, _) = b.service.receive(&card, after).await.unwrap();
    assert!(
        after.is_none(),
        "the worker restarts a full scan after this page"
    );

    b.service.receive(&card, None).await.unwrap();
    for record in deferred {
        let fact = ShareFact::Conversation(Box::new(record));
        assert_eq!(
            b.store.social_record(&fact.key().unwrap()).unwrap(),
            Some(fact)
        );
    }
    assert_eq!(
        b.store.social_conversation(&root.id).unwrap().replies.len(),
        522
    );
}

#[tokio::test]
async fn introduction_opt_out_keeps_direct_and_all_followed_conversations_and_the_article() {
    use carrel_store::sharing::ConversationParticipation::{Admitted, Declined};
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 181).await;
    let b = person(&dir.path().join("b"), 183).await;
    let c = person(&dir.path().join("c"), 185).await;
    for recipient in [&b, &c] {
        follow(&a, recipient);
        follow(recipient, &a);
        assert!(
            recipient
                .store
                .allow_unfollowed_group_conversations()
                .unwrap()
        );
        recipient
            .store
            .set_allow_unfollowed_group_conversations(false, 1, &recipient.device)
            .unwrap();
    }
    follow(&c, &b);
    let html = "<p>The article survives declining its discussion.</p>";
    let direct = article(
        &a,
        "https://example.org/consent",
        html,
        vec![],
        "direct",
        *b.master.public(),
    );
    let group = group_share(&a, &direct, &[&b, &c], "unfamiliar-to-b");
    let staged = a
        .store
        .reply_to_social_share(
            &group.id,
            "before-root-delivery",
            None,
            "A reply whose mirror key precedes its root",
            102,
            &a.device,
        )
        .unwrap();
    exchange(&a, &b).await;
    exchange(&a, &c).await;
    assert_eq!(
        b.store.conversation_participation(&direct.id).unwrap(),
        Admitted
    );
    assert_eq!(
        b.store.conversation_participation(&group.id).unwrap(),
        Declined
    );
    assert_eq!(
        c.store.conversation_participation(&group.id).unwrap(),
        Admitted
    );
    assert_eq!(b.store.social_conversation(&group.id).unwrap().share, group);
    assert!(
        b.store
            .social_record(
                &ShareFact::Conversation(Box::new(staged.clone()))
                    .key()
                    .unwrap()
            )
            .unwrap()
            .is_none(),
        "a staged friend candidate cannot become own history during promotion"
    );
    assert!(
        b.store
            .social_conversation(&group.id)
            .unwrap()
            .replies
            .is_empty()
    );
    assert_eq!(
        c.store.social_conversation(&group.id).unwrap().replies[0].record,
        staged
    );
    let received = b
        .service
        .transfer_cache(&a.service.contact_card().unwrap())
        .await
        .unwrap();
    assert!(received.changed);
    assert!(received.errors.is_empty());
    let body = BlobId::from_bytes(html.as_bytes());
    assert_eq!(b.cache.get_blocking(&body).unwrap(), html.as_bytes());

    let reply = a
        .store
        .reply_to_social_share(&group.id, "group-reply", None, "Discussion", 103, &a.device)
        .unwrap();
    let fact = ShareFact::Conversation(Box::new(reply.clone()));
    // A still serves its addressed copy: B must consume it without retaining the
    // body or failing the page, while C accepts the same original signed reply.
    exchange(&a, &b).await;
    exchange(&a, &c).await;
    assert!(
        b.store
            .social_record(&fact.key().unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        c.store.social_conversation(&group.id).unwrap().replies[1].record,
        reply
    );
    b.store
        .set_allow_unfollowed_group_conversations(true, 103, &b.device)
        .unwrap();
    follow(&b, &c);
    exchange(&a, &b).await;
    assert_eq!(
        b.store.conversation_participation(&group.id).unwrap(),
        Declined
    );
    assert!(
        b.store
            .social_conversation(&group.id)
            .unwrap()
            .replies
            .is_empty()
    );
    assert!(
        b.store
            .reply_to_social_share(&group.id, "cannot-reopen", None, "Draft", 104, &b.device)
            .is_err()
    );
    assert!(
        b.store
            .reply_to_social_share(
                &direct.id,
                "direct-reply",
                None,
                "Direct still works",
                104,
                &b.device
            )
            .is_ok()
    );
    a.host.stop().await.unwrap();
    b.host.stop().await.unwrap();
    c.host.stop().await.unwrap();
}

#[tokio::test]
async fn leave_suppresses_author_and_relay_mirrors_but_keeps_removals_and_fixed_root_content() {
    use carrel_store::sharing::ConversationParticipation::Left;
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 191).await;
    let b = person(&dir.path().join("b"), 193).await;
    let c = person(&dir.path().join("c"), 195).await;
    let d = person(&dir.path().join("d"), 197).await;
    for recipient in [&b, &c, &d] {
        follow(&a, recipient);
        follow(recipient, &a);
    }
    let html = "<p>Leaving a conversation does not withdraw this article.</p>";
    let direct = article(
        &a,
        "https://example.org/leave",
        html,
        vec![],
        "fixed-article",
        *b.master.public(),
    );
    let group = group_share(&a, &direct, &[&b, &c, &d], "group");
    exchange(&a, &b).await;
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    let reply = b
        .store
        .reply_to_social_share(
            &group.id,
            "mirrored",
            None,
            "Retained history",
            102,
            &b.device,
        )
        .unwrap();
    let fact = ShareFact::Conversation(Box::new(reply.clone()));
    let portable = fact.key().unwrap();
    exchange(&b, &a).await;
    a.service.publish(None).await.unwrap();
    // C has accepted history; D has only the root. Both remain original recipients.
    exchange(&a, &c).await;
    assert_eq!(
        c.store.social_conversation(&group.id).unwrap().replies[0].record,
        reply
    );
    for (person, recipient) in [(&b, a.master.public()), (&a, c.master.public())] {
        assert!(
            person
                .service
                .doc
                .get_exact(
                    person.service.author,
                    mirror_key(*recipient, &portable).as_bytes(),
                    false
                )
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            person
                .service
                .records_for(*recipient, None)
                .await
                .unwrap()
                .0
                .contains(&fact)
        );
    }
    let queued = b
        .store
        .reply_to_social_share(
            &group.id,
            "not-yet-mirrored",
            None,
            "Queued body",
            103,
            &b.device,
        )
        .unwrap();
    let queued_fact = ShareFact::Conversation(Box::new(queued));
    for person in [&a, &b] {
        person
            .store
            .leave_social_conversation(&group.id, 104, &person.device)
            .unwrap();
        assert_eq!(
            person.store.conversation_participation(&group.id).unwrap(),
            Left
        );
        assert_eq!(
            person.store.social_conversation(&group.id).unwrap().replies[0].record,
            reply
        );
    }
    // Do not republish A before checking: its stale relay entry is still present.
    assert!(
        !a.service
            .records_for(*c.master.public(), None)
            .await
            .unwrap()
            .0
            .contains(&fact)
    );
    assert!(
        !b.service
            .records_for(*a.master.public(), None)
            .await
            .unwrap()
            .0
            .contains(&fact)
    );
    exchange(&b, &a).await;
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    assert!(
        b.service
            .doc
            .get_exact(
                b.service.author,
                mirror_key(*a.master.public(), &queued_fact.key().unwrap()).as_bytes(),
                false
            )
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        d.store
            .social_conversation(&group.id)
            .unwrap()
            .replies
            .is_empty()
    );
    for person in [&c, &d] {
        assert!(
            person
                .store
                .social_record(&queued_fact.key().unwrap())
                .unwrap()
                .is_none()
        );
    }
    assert!(!a.store.social_conversation(&group.id).unwrap().closed);
    let cache = c
        .service
        .transfer_cache(&a.service.contact_card().unwrap())
        .await
        .unwrap();
    assert!(cache.changed);
    assert!(cache.errors.is_empty());
    assert_eq!(
        c.cache
            .get_blocking(&BlobId::from_bytes(html.as_bytes()))
            .unwrap(),
        html.as_bytes()
    );

    let carrel_core::conversation::ConversationContent::Reply { id, .. } = &reply.content else {
        panic!()
    };
    b.store
        .remove_social_reply(&group.id, id, -1, &b.device)
        .unwrap();
    let removal = b.store.social_history(None, 100).unwrap().into_iter().find(|fact| {
        matches!(fact, ShareFact::Conversation(record) if matches!(&record.content, carrel_core::conversation::ConversationContent::Remove { reply_id, .. } if reply_id == id))
    }).unwrap();
    exchange(&b, &a).await;
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    let removal_key = removal.key().unwrap();
    assert_eq!(c.store.social_record(&removal_key).unwrap(), Some(removal));
    assert!(a.store.social_conversation(&group.id).unwrap().replies[0].removed);
    assert!(c.store.social_conversation(&group.id).unwrap().replies[0].removed);
    // D cannot validate removal ownership without the reply, and receives no body.
    assert!(d.store.social_record(&portable).unwrap().is_none());
    assert!(d.store.social_record(&removal_key).unwrap().is_none());
    a.store
        .retract_social_share(&group.id, -2, &a.device)
        .unwrap();
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    assert!(c.store.social_conversation(&group.id).unwrap().closed);
    assert!(d.store.social_conversation(&group.id).unwrap().closed);
    assert!(
        c.cache
            .get_blocking(&BlobId::from_bytes(html.as_bytes()))
            .is_ok()
    );
    a.host.stop().await.unwrap();
    b.host.stop().await.unwrap();
    c.host.stop().await.unwrap();
    d.host.stop().await.unwrap();
}

#[tokio::test]
async fn group_replies_relay_through_the_sharer_without_expanding_any_audience() {
    use carrel_core::conversation::{ConversationContent, ConversationRecord, ThreadScope};
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 61).await;
    let b = person(&dir.path().join("b"), 63).await;
    let c = person(&dir.path().join("c"), 65).await;
    let d = person(&dir.path().join("d"), 67).await;
    for friend in [&b, &c, &d] {
        follow(&a, friend);
        follow(friend, &a);
    }
    let private = a
        .cache
        .put_blocking(b"Private note and unshared archive")
        .unwrap();
    let direct = article(
        &a,
        "https://example.org/shared-twice",
        "<p>A deliberately shared article.</p>",
        vec![],
        "just-d",
        *d.master.public(),
    );
    let mut members = vec![*b.master.public(), *c.master.public()];
    members.sort();
    let group = a
        .store
        .create_social_share_for(
            ShareDraft {
                request_id: "book-club".into(),
                recipient: members[0],
                article: direct.article.clone(),
                note: Some("What did you make of this argument?".into()),
            },
            members,
            101,
            &a.device,
        )
        .unwrap();
    a.store
        .save_social_profile("Alice", None, 102, &a.device)
        .unwrap();
    exchange(&a, &b).await;
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    // A profile edited on an own desktop has no local contact configuration
    // there. A prepares only missing recipient copies after private delivery.
    let other_device = Keypair::from_seed(&[69; 32]);
    let other_certificate = DeviceCert::new_at_micros(&a.master, other_device.public(), 1, None);
    a.store
        .accept_self_device_certificate(&other_certificate)
        .unwrap();
    for (name, certificate, device) in [
        ("Another desktop", other_certificate.clone(), &other_device),
        (
            "Concurrent name",
            a.store.social_local_certificate(a.device.public()).unwrap(),
            &a.device,
        ),
    ] {
        let profile = carrel_core::conversation::ConversationRecord::sign(
            ConversationContent::Profile {
                recipient: *a.master.public(),
                generation: 2,
                name: name.into(),
                avatar: None,
            },
            102,
            certificate,
            device,
        )
        .unwrap();
        a.store
            .accept_self_private_fact(
                &carrel_core::private_library::PrivateFact::Social(Box::new(
                    ShareFact::Conversation(Box::new(profile)),
                )),
                a.device.public(),
            )
            .unwrap();
    }
    let presentation = a
        .store
        .social_profile(a.master.public())
        .unwrap()
        .unwrap()
        .profile_revision_id()
        .unwrap();
    exchange(&a, &b).await;
    exchange(&a, &c).await;
    assert_eq!(
        a.store
            .social_profile(a.master.public())
            .unwrap()
            .unwrap()
            .profile_revision_id()
            .unwrap(),
        presentation,
        "delivery copies cannot change a concurrent profile winner"
    );
    assert_eq!(
        b.store
            .social_profile(a.master.public())
            .unwrap()
            .unwrap()
            .profile_revision_id()
            .unwrap(),
        presentation
    );
    let records = a.store.social_history(None, 100).unwrap().len();
    exchange(&a, &b).await;
    assert_eq!(
        a.store.social_history(None, 100).unwrap().len(),
        records,
        "profile publication is idempotent"
    );
    assert!(d.store.social_conversation(&group.id).is_err());
    assert!(b.store.social_conversation(&direct.id).is_err());
    assert!(c.store.social_conversation(&direct.id).is_err());
    let transfer = c
        .service
        .transfer_cache(&a.service.contact_card().unwrap())
        .await
        .unwrap();
    assert!(transfer.changed);
    assert!(transfer.errors.is_empty());
    let reply = b
        .store
        .reply_to_social_share(
            &group.id,
            "offline-b",
            None,
            "An offline thought — 文",
            103,
            &b.device,
        )
        .unwrap();
    b.store
        .save_social_profile("Bob's private contact profile", None, 103, &b.device)
        .unwrap();
    let ConversationContent::Reply { id: reply_id, .. } = &reply.content else {
        panic!()
    };
    assert!(
        a.store
            .social_conversation(&group.id)
            .unwrap()
            .replies
            .is_empty()
    );
    exchange(&b, &a).await;
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    assert!(
        c.store.social_profile(b.master.public()).unwrap().is_none(),
        "a group relay cannot forward a profile addressed only to the sharer"
    );
    assert_eq!(
        c.store.social_conversation(&group.id).unwrap().replies[0].record,
        reply
    );
    assert!(
        c.store.social_contact(b.master.public()).unwrap().is_none(),
        "relay never follows another participant"
    );
    assert!(
        d.store
            .social_record(&format!("reply/{reply_id}"))
            .unwrap()
            .is_none()
    );
    assert!(
        c.service
            .request(
                a.host.address(),
                Operation::Blob {
                    share_id: group.id.clone(),
                    blob_id: private.to_string()
                },
                64
            )
            .await
            .is_err()
    );
    let answer = c
        .store
        .reply_to_social_share(
            &group.id,
            "c-answer",
            Some(reply_id.clone()),
            "That middle section changed my mind",
            104,
            &c.device,
        )
        .unwrap();
    exchange(&c, &a).await;
    exchange(&a, &b).await;
    let ConversationContent::Reply { id: answer_id, .. } = &answer.content else {
        panic!()
    };
    assert_eq!(
        b.store.social_reply_ancestry(&group.id, answer_id).unwrap(),
        vec![answer.clone(), reply.clone()]
    );
    // A queued reply from an own desktop is still forwardable when separately
    // signed control history admits this thread and normal audience checks pass.
    let own_reply = ConversationRecord::sign(
        ConversationContent::Reply {
            scope: ThreadScope::from_share(&group).unwrap(),
            id: carrel_core::sharing::Share::draft_id(&other_certificate, "own-desktop-reply")
                .unwrap(),
            request_id: "own-desktop-reply".into(),
            parent: None,
            body: "A thought queued on my other desktop".into(),
        },
        105,
        other_certificate,
        &other_device,
    )
    .unwrap();
    let own_reply = ShareFact::Conversation(Box::new(own_reply));
    a.store
        .accept_self_private_fact(
            &carrel_core::private_library::PrivateFact::Social(Box::new(own_reply.clone())),
            a.device.public(),
        )
        .unwrap();
    assert_eq!(
        a.store.conversation_participation(&group.id).unwrap(),
        carrel_store::sharing::ConversationParticipation::Admitted
    );
    assert!(a.store.social_record_servable(&own_reply).unwrap());
    exchange(&a, &b).await;
    exchange(&a, &c).await;
    exchange(&a, &d).await;
    for recipient in [&b, &c] {
        assert_eq!(
            recipient
                .store
                .social_record(&own_reply.key().unwrap())
                .unwrap(),
            Some(own_reply.clone())
        );
    }
    assert!(
        d.store
            .social_record(&own_reply.key().unwrap())
            .unwrap()
            .is_none()
    );
    b.store
        .remove_social_reply(&group.id, reply_id, -1, &b.device)
        .unwrap();
    exchange(&b, &a).await;
    exchange(&a, &c).await;
    assert!(c.store.social_conversation(&group.id).unwrap().replies[0].removed);
    a.store
        .retract_social_share(&group.id, -2, &a.device)
        .unwrap();
    exchange(&a, &c).await;
    assert!(c.store.social_conversation(&group.id).unwrap().closed);
    assert!(
        c.store
            .reply_to_social_share(
                &group.id,
                "late-local",
                None,
                "Must remain a draft",
                105,
                &c.device
            )
            .is_err()
    );
    assert!(
        c.store
            .query("?[id] := *read_state{item_id:id}")
            .unwrap()
            .rows
            .is_empty()
    );
    a.host.stop().await.unwrap();
    b.host.stop().await.unwrap();
    c.host.stop().await.unwrap();
    d.host.stop().await.unwrap();
}

#[tokio::test]
async fn background_exchange_pins_a_bare_key_and_repairs_its_mirror_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 41).await;
    let b = person(&dir.path().join("b"), 43).await;
    follow(&a, &b);
    b.store
        .follow_social_contact(*a.master.public(), "Known public key", None)
        .unwrap();
    let share = article(
        &a,
        "https://example.org/background",
        "<p>Read offline after restart.</p>",
        vec![],
        "background",
        *b.master.public(),
    );
    let aw = super::runtime::SocialSyncWorker::start(a.service.clone());
    let bw = super::runtime::SocialSyncWorker::start(b.service.clone());
    let item_id = share.article.id.clone();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if b.store
                .saved_article_snapshot(&item_id)
                .unwrap()
                .is_some_and(|item| !item.contents.is_empty())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        b.store
            .social_contact(a.master.public())
            .unwrap()
            .unwrap()
            .card
            .is_some()
    );
    assert!(b.store.social_revision() > 0);
    aw.stop();
    bw.stop();
    a.host.stop().await.unwrap();
    let namespace = a.store.social_document().unwrap().unwrap().namespace;
    let host = SelfTransport::start_with_sharing_builder(
        a.store.clone(),
        &dir.path().join("repaired-mirror"),
        &a.device,
        a.cache.clone(),
        Endpoint::builder(presets::Minimal)
            .bind_addr("127.0.0.1:0")
            .unwrap(),
    )
    .await
    .unwrap();
    let service = host.sharing().unwrap();
    assert_eq!(
        a.store.social_document().unwrap().unwrap().namespace,
        namespace
    );
    service.publish(None).await.unwrap();
    b.store
        .follow_social_contact(
            *a.master.public(),
            "Known public key",
            Some(&service.contact_card().unwrap()),
        )
        .unwrap();
    let (_, changed) = b
        .service
        .receive(&service.contact_card().unwrap(), None)
        .await
        .unwrap();
    assert!(!changed, "restart replays are idempotent");
    a.store
        .retract_social_share(&share.id, 200, &a.device)
        .unwrap();
    service.publish(None).await.unwrap();
    b.service
        .receive(&service.contact_card().unwrap(), None)
        .await
        .unwrap();
    assert!(
        b.store
            .received_item_shares(&item_id, 100)
            .unwrap()
            .is_empty()
    );
    assert!(b.store.saved_article_snapshot(&item_id).unwrap().is_some());
    host.stop().await.unwrap();
    b.host.stop().await.unwrap();
}

#[tokio::test]
async fn three_real_peers_exchange_only_the_intended_vouch_and_scoped_archive() {
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 31).await;
    let b = person(&dir.path().join("b"), 33).await;
    let c = person(&dir.path().join("c"), 35).await;
    follow(&a, &b);
    follow(&a, &c);
    follow(&b, &a);
    follow(&c, &a);
    let image=b"<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><rect width='1' height='1'/></svg>";
    let image_id = a.cache.put_blocking(image).unwrap();
    let private = a.cache.put_blocking(b"An unshared archived file").unwrap();
    a.host
        .blobs()
        .add_slice(b"An unshared archived file")
        .await
        .unwrap();
    let html = format!("<p>Offline reading works.</p><img src='blob://{image_id}' alt='Diagram'>");
    let share = article(
        &a,
        "https://example.org/essay",
        &html,
        vec![BlobReference {
            blob_id: image_id.to_string(),
            byte_size: image.len() as u64,
        }],
        "one-vouch",
        *b.master.public(),
    );
    a.service.publish(None).await.unwrap();
    let card = a.service.contact_card().unwrap();
    let (_, changed) = b.service.receive(&card, None).await.unwrap();
    assert!(changed);
    let (_, changed) = c.service.receive(&card, None).await.unwrap();
    assert!(!changed);
    assert!(
        c.store
            .query("?[id] := *social_receipt{share_id:id}")
            .unwrap()
            .rows
            .is_empty()
    );
    let id = carrel_store::ids::id_for_external("https://example.org/essay");
    assert!(
        b.store
            .get_item_detail(&id)
            .unwrap()
            .unwrap()
            .readable
            .is_none()
    );
    let transfer = b.service.transfer_cache(&card).await.unwrap();
    assert!(transfer.changed);
    assert!(transfer.errors.is_empty());
    let body = b
        .store
        .get_item_detail(&id)
        .unwrap()
        .unwrap()
        .readable
        .unwrap();
    assert_eq!(
        b.cache
            .get_blocking(&body.blob_id.parse().unwrap())
            .unwrap()
            .as_ref(),
        html.as_bytes()
    );
    assert_eq!(b.cache.get_blocking(&image_id).unwrap().as_ref(), image);
    assert_eq!(
        b.store
            .current_image_scope(&id, &body.blob_id)
            .unwrap()
            .unwrap()[0]
            .blob_id,
        image_id.to_string()
    );
    assert_eq!(
        b.store.received_item_shares(&id, 100).unwrap()[0]
            .share
            .note,
        share.note
    );
    assert!(
        b.service
            .request(
                a.host.address(),
                Operation::Blob {
                    share_id: share.id.clone(),
                    blob_id: private.to_string()
                },
                64
            )
            .await
            .is_err()
    );
    assert!(
        c.service
            .request(
                a.host.address(),
                Operation::Blob {
                    share_id: share.id.clone(),
                    blob_id: image_id.to_string()
                },
                image.len() + 1
            )
            .await
            .is_err()
    );

    // A malicious signer can name a digest that exists only in the recipient's
    // private cache. Presence alone must never turn that claim into a grant.
    let b_private = b
        .cache
        .put_blocking(b"Private cached bytes already present on B")
        .unwrap();
    let fake_html = format!("<p>Readable text.</p><img src='blob://{b_private}' alt='Unapproved'>");
    article(
        &a,
        "https://example.org/fake-image",
        &fake_html,
        vec![BlobReference {
            blob_id: b_private.to_string(),
            byte_size: 40,
        }],
        "fake-image",
        *b.master.public(),
    );
    a.service.publish(None).await.unwrap();
    b.service.receive(&card, None).await.unwrap();
    let partial = b.service.transfer_cache(&card).await.unwrap();
    assert!(partial.changed);
    assert!(!partial.errors.is_empty());
    let fake_id = carrel_store::ids::id_for_external("https://example.org/fake-image");
    let fake_body = b
        .store
        .get_item_detail(&fake_id)
        .unwrap()
        .unwrap()
        .readable
        .unwrap();
    assert!(
        b.store
            .current_image_scope(&fake_id, &fake_body.blob_id)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert!(
        b.cache.get_blocking(&b_private).is_ok(),
        "private cache presence never grants the image"
    );

    a.store
        .retract_social_share(&share.id, -1, &a.device)
        .unwrap();
    a.service.publish(None).await.unwrap();
    b.service.receive(&card, None).await.unwrap();
    assert!(b.store.received_item_shares(&id, 100).unwrap().is_empty());
    assert!(
        b.store
            .get_item_detail(&id)
            .unwrap()
            .unwrap()
            .readable
            .is_some()
    );
    a.store.unfollow_social_contact(b.master.public()).unwrap();
    a.host.refresh_authorization().await.unwrap();
    assert!(b.service.receive(&card, None).await.is_err());
    a.host.stop().await.unwrap();
    b.host.stop().await.unwrap();
    c.host.stop().await.unwrap();
}

#[tokio::test]
async fn a_followed_tls_friend_still_cannot_open_private_protocols_or_reuse_someone_elses_card() {
    let dir = tempfile::tempdir().unwrap();
    let a = person(&dir.path().join("a"), 41).await;
    let b = person(&dir.path().join("b"), 43).await;
    let c = person(&dir.path().join("c"), 45).await;
    follow(&a, &b);
    follow(&a, &c);
    let b_card = b.service.contact_card().unwrap();
    c.host.stop().await.unwrap();
    let seed = zeroize::Zeroizing::new(c.device.to_seed_bytes());
    let raw = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&seed))
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap();
    for alpn in [iroh_docs::ALPN, iroh_blobs::ALPN, iroh_gossip::ALPN] {
        let result = raw.connect(a.host.address(), alpn).await;
        if let Ok(connection) = result {
            let closed = tokio::time::timeout(Duration::from_secs(3), connection.closed()).await;
            assert!(closed.is_ok(), "private protocol rejected the friend");
        }
    }
    let connection = raw.connect(a.host.address(), ALPN).await.unwrap();
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    send.write_all(
        &canonical_bytes(&Request {
            card: b_card,
            operation: Operation::Records { after: None },
        })
        .unwrap(),
    )
    .await
    .unwrap();
    send.finish().unwrap();
    assert!(
        recv.read_to_end(MAX_RESPONSE).await.is_err(),
        "public card cannot be replayed from another TLS key"
    );
    raw.close().await;
    a.host.stop().await.unwrap();
    b.host.stop().await.unwrap();
}
