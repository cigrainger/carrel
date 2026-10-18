use carrel_core::{
    conversation::{
        ConversationContent, ConversationParticipation, ReplyDraftInput, ReplyDraftSnapshot,
    },
    identity::{DeviceCert, Keypair, PublicKey, RevocationCert},
    private_library::{PrivateFact, SavedItem},
    sharing::{Share, ShareFact, ShareRetraction, public_article_snapshot},
};
use carrel_store::{
    Store, StoreError, backup::open_backup, blobs::BlobStore, keystore::Keystore,
    sharing::ShareDraft,
};
use cozo::DataValue;
use proptest::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
};

struct Fixture {
    store: Store,
    master: Keypair,
    device: Keypair,
    friend: Keypair,
    friend_device: Keypair,
    root: Share,
}

fn article() -> SavedItem {
    SavedItem {
        id: "a".repeat(64),
        kind: "article".into(),
        title: "An essay — 文".into(),
        creators: vec![],
        primary_url: Some("https://example.test/drafts".into()),
        published_at_micros: None,
        language: None,
        summary: None,
        // Signed share snapshots exclude the reader's private arrival history.
        discovered_at_micros: 0,
        article: None,
        identifiers: vec![],
        contents: vec![],
        resources: vec![],
        current_images: None,
    }
}

impl Fixture {
    fn new() -> Self {
        Self::with_store(Store::open_in_memory().unwrap())
    }

    fn with_store(store: Store) -> Self {
        store.migrate().unwrap();
        let master = Keypair::from_seed(&[81; 32]);
        let device = Keypair::from_seed(&[82; 32]);
        let friend = Keypair::from_seed(&[83; 32]);
        let friend_device = Keypair::from_seed(&[84; 32]);
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&master, device.public(), 1, None),
                &device,
            )
            .unwrap();
        store
            .follow_social_contact(*friend.public(), "Friend", None)
            .unwrap();
        let root = Share::sign(
            "root".into(),
            *master.public(),
            article(),
            Some("Opening".into()),
            2,
            DeviceCert::new_at_micros(&friend, friend_device.public(), 1, None),
            &friend_device,
        )
        .unwrap();
        store
            .accept_social_record(
                &ShareFact::Share(Box::new(root.clone())),
                friend.public(),
                2,
                &device,
            )
            .unwrap();
        Self {
            store,
            master,
            device,
            friend,
            friend_device,
            root,
        }
    }

    fn another_root(&self) -> Share {
        let root = Share::sign(
            "another-root".into(),
            *self.master.public(),
            article(),
            None,
            3,
            DeviceCert::new_at_micros(&self.friend, self.friend_device.public(), 1, None),
            &self.friend_device,
        )
        .unwrap();
        self.store
            .accept_social_record(
                &ShareFact::Share(Box::new(root.clone())),
                self.friend.public(),
                3,
                &self.device,
            )
            .unwrap();
        root
    }

    fn save(
        &self,
        revision: i64,
        body: &str,
        token: &str,
        parent: Option<String>,
    ) -> ReplyDraftSnapshot {
        self.store
            .save_reply_draft(
                &self.root.id,
                revision,
                input(body, token, parent),
                self.device.public(),
            )
            .unwrap()
    }

    fn snapshot(&self) -> ReplyDraftSnapshot {
        self.store.load_reply_draft(&self.root.id).unwrap()
    }
}

fn input(body: &str, token: &str, parent: Option<String>) -> ReplyDraftInput {
    ReplyDraftInput {
        body: body.into(),
        parent,
        request_id: token.into(),
    }
}

fn portable_state(store: &Store) -> Vec<Vec<Vec<DataValue>>> {
    [
        "?[key,encoded] := *social_record{key,encoded} :sort key",
        "?[id,key,share_id,parent] := *social_reply{id,key,share_id,parent} :sort id",
        "?[key,encoded] := *self_fact_history{key,encoded} :sort key",
        "?[key,encoded] := *self_fact_outbox{key,encoded} :sort key",
    ]
    .into_iter()
    .map(|query| store.query(query).unwrap().rows)
    .collect()
}

fn conversation_authority_state(store: &Store) -> Vec<Vec<Vec<DataValue>>> {
    let mut state = portable_state(store);
    for query in [
        "?[register,revision_id,encoded] := *self_state{register,revision_id,encoded} :sort register,revision_id",
        "?[register,revision_id,encoded] := *self_state_incoming{register,revision_id,encoded} :sort register,revision_id",
        "?[key,encoded] := *social_pending{key,encoded} :sort key",
        "?[share_id,item_id] := *social_receipt{share_id,item_id} :sort share_id",
        "?[master_pubkey,device_pubkey] := *self_identity{master_pubkey,device_pubkey}",
        "?[master_pubkey,device_pubkey,legacy_devices] := *restore_origin{master_pubkey,device_pubkey,legacy_devices}",
        "?[master,name,card,followed] := *social_contact{master,name,card,followed} :sort master",
        "?[master_pubkey,device_pubkey,device_cert] := *device_certificate{master_pubkey,device_pubkey,device_cert} :sort master_pubkey,device_pubkey",
    ] {
        state.push(store.query(query).unwrap().rows);
    }
    state
}

// Retain diagnostic proof rows without using a typed incoming path to grant trust.
fn retain_root(store: &Store, root: &Share, indexed_author: &PublicKey) {
    let fact = ShareFact::Share(Box::new(root.clone()));
    store.query_with_params("?[key,encoded,author,recipient,share_id,kind,created_at] <- [[$key,$encoded,$author,$recipient,$share,'share',$at]] :put social_record {key=>encoded,author,recipient,share_id,kind,created_at}", BTreeMap::from([
        ("key".into(), DataValue::from(fact.key().unwrap())),
        ("encoded".into(), DataValue::Bytes(fact.to_bytes().unwrap())),
        ("author".into(), DataValue::Bytes(indexed_author.as_bytes().to_vec())),
        ("recipient".into(), DataValue::Bytes(root.recipient.as_bytes().to_vec())),
        ("share".into(), DataValue::from(root.id.as_str())),
        ("at".into(), DataValue::from(root.created_at_micros)),
    ])).unwrap();
}

#[test]
fn restored_unbound_archive_exposes_received_and_own_drafts_without_sharing_authority() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::with_store(Store::open(&dir.path().join("source-store")).unwrap());
    let local_item = f.store.social_conversation(&f.root.id).unwrap().item_id;
    assert_ne!(local_item, f.root.article.id);
    let sent = f
        .store
        .create_social_share(
            ShareDraft {
                request_id: "own-root".into(),
                recipient: *f.friend.public(),
                article: public_article_snapshot(
                    f.store
                        .saved_article_snapshot(&local_item)
                        .unwrap()
                        .unwrap(),
                    Vec::new(),
                )
                .unwrap(),
                note: Some("My opening note".into()),
            },
            3,
            &f.device,
        )
        .unwrap();
    let received_parent = f
        .store
        .reply_to_social_share(
            &f.root.id,
            "received-parent",
            None,
            "Received context — 文",
            4,
            &f.device,
        )
        .unwrap();
    let own_parent = f
        .store
        .reply_to_social_share(&sent.id, "own-parent", None, "Own context", 4, &f.device)
        .unwrap();
    let (
        ConversationContent::Reply {
            id: received_parent_id,
            ..
        },
        ConversationContent::Reply {
            id: own_parent_id, ..
        },
    ) = (&received_parent.content, &own_parent.content)
    else {
        panic!()
    };
    let received_draft = f.save(
        0,
        "PRIVATE received draft — 文\n  exact text",
        "received-draft",
        Some(received_parent_id.clone()),
    );
    let own_draft = f
        .store
        .save_reply_draft(
            &sent.id,
            0,
            input(
                "PRIVATE own draft",
                "own-draft",
                Some(own_parent_id.clone()),
            ),
            f.device.public(),
        )
        .unwrap();
    f.store
        .remove_social_reply(&sent.id, own_parent_id, 5, &f.device)
        .unwrap();
    f.store
        .retract_social_share(&sent.id, 6, &f.device)
        .unwrap();
    f.store
        .leave_social_conversation(&f.root.id, 7, &f.device)
        .unwrap();
    let keys = Keystore::open(&dir.path().join("source-keys"));
    keys.create_device(&f.device).unwrap();
    let path = dir.path().join("drafts.carrel-backup");
    let password = "a long draft recovery passphrase";
    f.store
        .create_backup(
            &BlobStore::open(dir.path().join("source-blobs")),
            &keys,
            std::fs::File::create(&path).unwrap(),
            password,
            "draft-history-test",
            8,
        )
        .unwrap();
    let archive = open_backup(&path, password, dir.path()).unwrap();
    let store = Store::open(&archive.path().join("store")).unwrap();
    let fresh = Keystore::open(&archive.path().join("keys"))
        .load_device()
        .unwrap();
    assert!(store.self_identity().unwrap().is_none());
    assert_eq!(
        store.local_conversation_history_master().unwrap(),
        Some(*f.master.public())
    );
    let before = conversation_authority_state(&store);
    let revision = store.social_revision();
    let received = store.received_item_conversations(&local_item).unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].id, f.root.id);
    assert_eq!(received[0].from, "Friend");
    assert!(!received[0].closed);
    let listed = store.item_conversations(&local_item).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>(),
        vec![sent.id.as_str(), f.root.id.as_str()]
    );
    assert_eq!(listed[0].at_micros, sent.created_at_micros);
    assert!(listed[0].closed);
    assert!(!listed[1].closed);
    assert!(
        store
            .item_conversations(&f.root.article.id)
            .unwrap()
            .is_empty()
    );
    let mut recovered_drafts = Vec::new();
    for (root, parent, original) in [
        (&f.root, &received_parent, &received_draft),
        (&sent, &own_parent, &own_draft),
    ] {
        let thread = store.social_conversation(&root.id).unwrap();
        assert_eq!(thread.share, *root);
        assert_eq!(thread.item_id, local_item);
        assert_eq!(thread.replies.len(), 1);
        assert_eq!(thread.replies[0].record, *parent);
        assert_eq!(thread.replies[0].removed, root.id == sent.id);
        let ConversationContent::Reply { id: parent_id, .. } = &parent.content else {
            panic!()
        };
        assert_eq!(
            store.social_reply_ancestry(&root.id, parent_id).unwrap(),
            vec![parent.clone()]
        );
        let snapshot = store.load_reply_draft(&root.id).unwrap();
        let draft = snapshot.draft.as_ref().unwrap();
        let copied_text = draft.body.clone();
        assert_eq!(copied_text, original.draft.as_ref().unwrap().body);
        assert_eq!(draft.parent, original.draft.as_ref().unwrap().parent);
        assert_eq!(snapshot.revision, original.revision + 1);
        assert_eq!(&draft.retry_device, fresh.public());
        assert_ne!(
            draft.request_id,
            original.draft.as_ref().unwrap().request_id
        );
        assert!(matches!(
            store.reply_from_draft(&root.id, &snapshot, 9, &fresh),
            Err(StoreError::IdentityUnbound)
        ));
        assert!(matches!(
            store.reply_to_social_share(&root.id, "blocked", None, "Must not send", 9, &fresh),
            Err(StoreError::IdentityUnbound)
        ));
        assert!(matches!(
            store.leave_social_conversation(&root.id, 9, &fresh),
            Err(StoreError::IdentityUnbound)
        ));
        assert!(matches!(
            store.remove_social_reply(&root.id, parent_id, 9, &fresh),
            Err(StoreError::IdentityUnbound)
        ));
        assert!(matches!(
            store.conversation_participation(&root.id),
            Err(StoreError::IdentityUnbound)
        ));
        assert!(matches!(
            store.conversation_participant(&root.id, f.friend.public()),
            Err(StoreError::IdentityUnbound)
        ));
        assert!(matches!(
            store.social_record_servable(&ShareFact::Conversation(Box::new(parent.clone()))),
            Err(StoreError::IdentityUnbound)
        ));
        assert_eq!(store.load_reply_draft(&root.id).unwrap(), snapshot);
        recovered_drafts.push((root.id.clone(), snapshot));
    }
    assert!(matches!(
        store.follow_social_contact(*Keypair::from_seed(&[95; 32]).public(), "No follow", None),
        Err(StoreError::IdentityUnbound)
    ));
    assert!(matches!(
        store.authorize_social_recipient(f.friend.public()),
        Err(StoreError::IdentityUnbound)
    ));
    assert!(matches!(
        store.social_local_certificate(fresh.public()),
        Err(StoreError::IdentityUnbound)
    ));
    for key in [fresh.public(), f.device.public()] {
        assert!(matches!(
            store.self_device_authorized(key),
            Err(StoreError::IdentityUnbound)
        ));
    }
    assert!(matches!(
        store.accept_social_record(
            &ShareFact::Share(Box::new(f.root.clone())),
            f.friend.public(),
            9,
            &fresh
        ),
        Err(StoreError::IdentityUnbound)
    ));
    assert!(matches!(
        store.accept_self_social_record(
            &ShareFact::Share(Box::new(f.root.clone())),
            f.device.public()
        ),
        Err(StoreError::IdentityUnbound)
    ));
    assert_eq!(conversation_authority_state(&store), before);
    assert_eq!(store.social_revision(), revision);
    for (index, (id, snapshot)) in recovered_drafts.iter().enumerate() {
        let cleared = store.discard_reply_draft(id, snapshot.revision).unwrap();
        assert!(cleared.draft.is_none());
        assert_eq!(cleared.revision, snapshot.revision + 1);
        if index == 0 {
            assert_eq!(
                store.load_reply_draft(&recovered_drafts[1].0).unwrap(),
                recovered_drafts[1].1
            );
        }
    }
    assert_eq!(conversation_authority_state(&store), before);
    assert_eq!(f.snapshot(), received_draft);
    assert_eq!(f.store.load_reply_draft(&sent.id).unwrap(), own_draft);

    let stranger = Keypair::from_seed(&[94; 32]);
    let outside = Share::sign(
        "unrelated-master".into(),
        *stranger.public(),
        article(),
        None,
        9,
        DeviceCert::new_at_micros(&f.friend, f.friend_device.public(), 1, None),
        &f.friend_device,
    )
    .unwrap();
    retain_root(&store, &outside, f.friend.public());
    let before_outside_reads = conversation_authority_state(&store);
    assert!(store.social_conversation(&outside.id).is_err());
    assert!(
        store
            .social_reply_ancestry(&outside.id, received_parent_id)
            .is_err()
    );
    assert!(store.load_reply_draft(&outside.id).is_err());
    assert!(store.discard_reply_draft(&outside.id, 0).is_err());
    assert_eq!(store.item_conversations(&local_item).unwrap().len(), 2);
    assert_eq!(conversation_authority_state(&store), before_outside_reads);
    store
        .query_with_params(
            "?[share_id,item_id] <- [[$share,$item]] :put social_receipt {share_id=>item_id}",
            BTreeMap::from([
                ("share".into(), DataValue::from(outside.id.as_str())),
                ("item".into(), DataValue::from(local_item.as_str())),
            ]),
        )
        .unwrap();
    let before_outside_reads = conversation_authority_state(&store);
    assert!(store.received_item_conversations(&local_item).is_err());
    assert!(store.item_conversations(&local_item).is_err());
    assert_eq!(conversation_authority_state(&store), before_outside_reads);

    store
        .authorize_restored_identity(&f.master, &fresh, "Recovered")
        .unwrap();
    assert_eq!(
        store.conversation_participation(&f.root.id).unwrap(),
        ConversationParticipation::Left
    );
    assert!(store.social_conversation(&sent.id).unwrap().closed);
    assert!(
        store
            .reply_to_social_share(&f.root.id, "still-left", None, "Do not reopen", 10, &fresh)
            .is_err()
    );
    assert!(
        store
            .reply_to_social_share(
                &sent.id,
                "still-withdrawn",
                None,
                "Do not reopen",
                10,
                &fresh
            )
            .is_err()
    );
}

#[test]
fn item_conversations_use_canonical_addresses_and_verified_own_proofs() {
    let f = Fixture::new();
    let local_item = f.store.social_conversation(&f.root.id).unwrap().item_id;
    let mut snapshot = article();
    snapshot.primary_url = Some("https://example.test/drafts?utm_source=friend".into());
    let certificate = f.store.social_local_certificate(f.device.public()).unwrap();
    let sent = Share::sign(
        "old-local-id".into(),
        *f.friend.public(),
        snapshot,
        None,
        3,
        certificate.clone(),
        &f.device,
    )
    .unwrap();
    assert_ne!(sent.article.id, local_item);
    retain_root(&f.store, &sent, f.master.public());
    let own_conversation = f.store.social_conversation(&sent.id).unwrap();
    assert_eq!(own_conversation.share, sent);
    assert_eq!(own_conversation.item_id, local_item);
    let mut other_article = article();
    // A colliding signed item ID is insufficient without the local article address.
    other_article.id = local_item.clone();
    other_article.primary_url = Some("https://example.test/unrelated".into());
    let unrelated = Share::sign(
        "different-address".into(),
        *f.friend.public(),
        other_article,
        None,
        4,
        certificate,
        &f.device,
    )
    .unwrap();
    retain_root(&f.store, &unrelated, f.master.public());
    let before = conversation_authority_state(&f.store);
    let listed = f.store.item_conversations(&local_item).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>(),
        vec![sent.id.as_str(), f.root.id.as_str()]
    );
    assert_eq!(
        f.store
            .received_item_conversations(&local_item)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(conversation_authority_state(&f.store), before);

    // Own-device replay can also retain a receipt for an own sent root. It must
    // not turn that root into a received conversation or duplicate its listing.
    f.store
        .query_with_params(
            "?[share_id,item_id] <- [[$share,$item]] :put social_receipt {share_id=>item_id}",
            BTreeMap::from([
                ("share".into(), DataValue::from(sent.id.as_str())),
                ("item".into(), DataValue::from(local_item.as_str())),
            ]),
        )
        .unwrap();
    assert_eq!(
        f.store
            .received_item_conversations(&local_item)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.store.item_conversations(&local_item).unwrap().len(), 2);
    assert_eq!(
        f.store.social_conversation(&sent.id).unwrap().item_id,
        local_item
    );

    // Even a locally indexed own root must retain a matching signed master.
    let impostor = Share::sign(
        "wrong-index".into(),
        *f.master.public(),
        article(),
        None,
        5,
        DeviceCert::new_at_micros(&f.friend, f.friend_device.public(), 1, None),
        &f.friend_device,
    )
    .unwrap();
    retain_root(&f.store, &impostor, f.master.public());
    let before = conversation_authority_state(&f.store);
    assert!(f.store.item_conversations(&local_item).is_err());
    assert_eq!(conversation_authority_state(&f.store), before);
}

#[test]
fn saves_are_private_exact_and_idempotent_after_a_lost_acknowledgement() {
    let f = Fixture::new();
    let before = portable_state(&f.store);
    assert_eq!(f.snapshot(), ReplyDraftSnapshot::default());
    assert!(
        f.store
            .query("?[id] := *social_reply_draft{share_id:id}")
            .unwrap()
            .rows
            .is_empty()
    );
    let saved = f.save(0, "A thought — 文\n  exact spacing ", "one", None);
    assert_eq!(saved.revision, 1);
    assert_eq!(
        f.save(0, "A thought — 文\n  exact spacing ", "one", None),
        saved
    );
    assert_eq!(
        f.save(1, "A thought — 文\n  exact spacing ", "one", None),
        saved
    );
    let changed = f.save(1, "", "one", None);
    assert_eq!(changed.revision, 2);
    assert!(matches!(
        f.store
            .save_reply_draft(&f.root.id, 0, input("", "one", None), f.device.public()),
        Err(StoreError::ReplyDraftConflict {
            expected_revision: 0,
            current_revision: 2,
            ..
        })
    ));
    let whitespace = f.save(2, " \n\t", "one", None);
    assert_eq!(whitespace.draft.as_ref().unwrap().body, " \n\t");
    assert_eq!(
        portable_state(&f.store),
        before,
        "autosaving creates no signed facts or publication work"
    );
    assert!(
        f.store
            .reply_from_draft(&f.root.id, &whitespace, 4, &f.device)
            .is_err()
    );
    assert_eq!(f.snapshot(), whitespace);
}

#[test]
fn unsigned_draft_snapshots_are_outside_the_private_fact_wire_allow_list() {
    let f = Fixture::new();
    let before = portable_state(&f.store);
    let saved = f.save(0, "PRIVATE draft must not replicate", "one", None);
    for bytes in [
        carrel_core::identity::canonical_bytes(&saved).unwrap(),
        carrel_core::identity::canonical_bytes(&serde_json::json!({"ReplyDraft": &saved})).unwrap(),
    ] {
        assert!(PrivateFact::from_bytes(&bytes).is_err());
        assert!(ShareFact::from_bytes(&bytes).is_err());
    }
    assert_eq!(portable_state(&f.store), before);
    f.store
        .discard_reply_draft(&f.root.id, saved.revision)
        .unwrap();
    assert_eq!(portable_state(&f.store), before);
}

#[test]
fn discard_retains_a_counter_and_rejects_every_delayed_pre_discard_save() {
    let f = Fixture::new();
    let saved = f.save(0, "Keep until deliberate discard", "one", None);
    let cleared = f.store.discard_reply_draft(&f.root.id, 1).unwrap();
    assert_eq!(
        cleared,
        ReplyDraftSnapshot {
            revision: 2,
            draft: None
        }
    );
    assert_eq!(f.store.discard_reply_draft(&f.root.id, 1).unwrap(), cleared);
    for revision in [0, 1] {
        assert!(matches!(
            f.store.save_reply_draft(
                &f.root.id,
                revision,
                saved.draft.as_ref().unwrap().input(),
                f.device.public()
            ),
            Err(StoreError::ReplyDraftConflict { .. })
        ));
        assert_eq!(f.snapshot(), cleared);
    }
    assert!(
        f.store
            .reply_from_draft(&f.root.id, &saved, 4, &f.device)
            .is_err()
    );
    let newer = f.save(2, "A deliberate new composition", "two", None);
    assert!(matches!(
        f.store.discard_reply_draft(&f.root.id, 1),
        Err(StoreError::ReplyDraftConflict { .. })
    ));
    assert_eq!(f.snapshot(), newer);
    assert!(
        f.store
            .query_with_params(
                "?[share_id] <- [[$id]] :rm social_reply_draft {share_id}",
                BTreeMap::from([("id".into(), DataValue::from(f.root.id))])
            )
            .is_err()
    );
}

#[test]
fn explicit_discard_fences_unpersisted_composition_even_when_the_slot_is_empty() {
    let f = Fixture::new();
    let first = f.store.discard_reply_draft(&f.root.id, 0).unwrap();
    assert_eq!(
        first,
        ReplyDraftSnapshot {
            revision: 1,
            draft: None
        }
    );
    assert!(matches!(
        f.store.save_reply_draft(
            &f.root.id,
            0,
            input("Delayed initial composition", "one", None),
            f.device.public()
        ),
        Err(StoreError::ReplyDraftConflict { .. })
    ));
    let second = f
        .store
        .discard_reply_draft(&f.root.id, first.revision)
        .unwrap();
    assert_eq!(
        second,
        ReplyDraftSnapshot {
            revision: 2,
            draft: None
        }
    );
    assert!(matches!(
        f.store.save_reply_draft(
            &f.root.id,
            first.revision,
            input("Delayed new composition", "two", None),
            f.device.public()
        ),
        Err(StoreError::ReplyDraftConflict { .. })
    ));
    assert_eq!(f.snapshot(), second);
}

#[test]
fn completed_legacy_send_clears_only_its_exact_persisted_draft_without_requeue() {
    let f = Fixture::new();
    let saved = f.save(0, "Sent by an existing caller", "legacy", None);
    let proof = f
        .store
        .reply_to_social_share(
            &f.root.id,
            "legacy",
            None,
            "Sent by an existing caller",
            3,
            &f.device,
        )
        .unwrap();
    let fact = PrivateFact::Social(Box::new(ShareFact::Conversation(Box::new(proof.clone()))));
    f.store
        .acknowledge_private_fact(&fact.key().unwrap())
        .unwrap();
    let before = portable_state(&f.store);
    let acknowledged = f
        .store
        .reply_from_draft(&f.root.id, &saved, 4, &f.device)
        .unwrap();
    assert!(acknowledged.already_sent);
    assert_eq!(acknowledged.record, proof);
    assert_eq!(
        acknowledged.draft,
        ReplyDraftSnapshot {
            revision: 2,
            draft: None
        }
    );
    assert_eq!(portable_state(&f.store), before);
    let newer = f.save(2, "Continue composing", "next", None);
    let retry = f
        .store
        .reply_from_draft(&f.root.id, &saved, 5, &f.device)
        .unwrap();
    assert_eq!(retry.draft, newer);
    assert_eq!(retry.record, proof);
    assert_eq!(portable_state(&f.store), before);
}

#[test]
fn exact_send_atomically_clears_and_lost_ack_preserves_later_composition_without_requeue() {
    let f = Fixture::new();
    let saved = f.save(0, "Sent text — 文", "one", None);
    let sent = f
        .store
        .reply_from_draft(&f.root.id, &saved, 4, &f.device)
        .unwrap();
    assert!(!sent.already_sent);
    assert_eq!(
        sent.draft,
        ReplyDraftSnapshot {
            revision: 2,
            draft: None
        }
    );
    assert_eq!(f.snapshot(), sent.draft);
    assert_eq!(
        f.store
            .social_conversation(&f.root.id)
            .unwrap()
            .replies
            .len(),
        1
    );
    let fact = PrivateFact::Social(Box::new(ShareFact::Conversation(Box::new(
        sent.record.clone(),
    ))));
    let key = fact.key().unwrap();
    assert!(
        f.store
            .private_fact_outbox(None, 1000)
            .unwrap()
            .iter()
            .any(|(k, p)| k == &key && p == &fact)
    );
    f.store.acknowledge_private_fact(&key).unwrap();
    let after = portable_state(&f.store);
    let again = f
        .store
        .reply_from_draft(&f.root.id, &saved, 5, &f.device)
        .unwrap();
    assert!(again.already_sent);
    assert_eq!(again.record, sent.record);
    assert_eq!(again.draft, sent.draft);
    assert_eq!(portable_state(&f.store), after);
    assert!(matches!(
        f.store.save_reply_draft(
            &f.root.id,
            1,
            saved.draft.as_ref().unwrap().input(),
            f.device.public()
        ),
        Err(StoreError::ReplyDraftConflict { .. })
    ));
    assert!(
        f.store
            .save_reply_draft(
                &f.root.id,
                2,
                saved.draft.as_ref().unwrap().input(),
                f.device.public()
            )
            .is_err(),
        "a completed retry token cannot be reused"
    );
    let newer = f.save(2, "Newer composition survives", "two", None);
    f.store
        .leave_social_conversation(&f.root.id, 6, &f.device)
        .unwrap();
    let before_retry = portable_state(&f.store);
    let completed = f
        .store
        .reply_from_draft(&f.root.id, &saved, 7, &f.device)
        .unwrap();
    assert!(completed.already_sent);
    assert_eq!(completed.record, sent.record);
    assert_eq!(completed.draft, newer);
    assert_eq!(portable_state(&f.store), before_retry);
    assert_eq!(f.snapshot(), newer);
}

#[test]
fn stale_or_forged_same_revision_send_cannot_publish_or_clear_newer_durable_text() {
    let f = Fixture::new();
    let old = f.save(0, "Old", "one", None);
    let newer = f.save(1, "New", "one", None);
    let before = portable_state(&f.store);
    for submitted in [old, {
        let mut forged = newer.clone();
        forged.draft.as_mut().unwrap().body = "Changed only in memory".into();
        forged
    }] {
        assert!(matches!(
            f.store
                .reply_from_draft(&f.root.id, &submitted, 4, &f.device),
            Err(StoreError::ReplyDraftConflict { .. })
        ));
        assert_eq!(f.snapshot(), newer);
        assert_eq!(portable_state(&f.store), before);
    }
}

#[test]
fn failure_at_any_send_write_rolls_back_proof_index_journal_queue_and_clear() {
    for trigger in [
        "::set_triggers social_reply on put { ?[id] := _new[id,key,share_id,parent] :assert none }",
        "::set_triggers self_fact_history on put { ?[key] := _new[key,encoded] :assert none }",
        "::set_triggers self_fact_outbox on put { ?[key] := _new[key,encoded] :assert none }",
        "::set_triggers social_reply_draft on put { ?[id] := _new[id,rev,body,parent,request,device], body == null :assert none }",
    ] {
        let f = Fixture::new();
        let saved = f.save(0, "Must survive a failed transaction", "one", None);
        let before = portable_state(&f.store);
        f.store.query(trigger).unwrap();
        assert!(
            f.store
                .reply_from_draft(&f.root.id, &saved, 4, &f.device)
                .is_err()
        );
        assert_eq!(f.snapshot(), saved);
        assert_eq!(portable_state(&f.store), before);
    }
}

#[test]
fn parent_context_is_exact_and_retained_after_removal_and_leave() {
    let f = Fixture::new();
    let parent = f
        .store
        .reply_to_social_share(&f.root.id, "parent", None, "Parent", 3, &f.device)
        .unwrap();
    let ConversationContent::Reply { id: parent_id, .. } = parent.content else {
        panic!()
    };
    let saved = f.save(0, "Child draft", "child", Some(parent_id.clone()));
    f.store
        .remove_social_reply(&f.root.id, &parent_id, 4, &f.device)
        .unwrap();
    f.store
        .leave_social_conversation(&f.root.id, 5, &f.device)
        .unwrap();
    assert_eq!(f.snapshot(), saved);
    assert!(matches!(
        f.store.reply_from_draft(&f.root.id, &saved, 6, &f.device),
        Err(StoreError::ConversationUnavailable(
            ConversationParticipation::Left
        ))
    ));
    assert_eq!(f.snapshot(), saved);
    let updated = f.save(
        saved.revision,
        "Still selectable for copying",
        "child",
        Some(parent_id),
    );
    assert_eq!(updated.draft.unwrap().body, "Still selectable for copying");
    f.store
        .discard_reply_draft(&f.root.id, updated.revision)
        .unwrap();
}

#[test]
fn declined_and_withdrawn_roots_keep_drafts_for_copying_or_explicit_discard() {
    let f = Fixture::new();
    let saved = f.save(0, "Keep after withdrawal", "one", None);
    let withdrawal = ShareRetraction::sign(
        f.root.id.clone(),
        *f.master.public(),
        4,
        DeviceCert::new_at_micros(&f.friend, f.friend_device.public(), 1, None),
        &f.friend_device,
    )
    .unwrap();
    f.store
        .accept_social_record(
            &ShareFact::Retraction(Box::new(withdrawal)),
            f.friend.public(),
            4,
            &f.device,
        )
        .unwrap();
    assert_eq!(f.snapshot(), saved);
    assert!(
        f.store
            .reply_from_draft(&f.root.id, &saved, 5, &f.device)
            .is_err()
    );
    assert_eq!(f.snapshot(), saved);
    f.store
        .discard_reply_draft(&f.root.id, saved.revision)
        .unwrap();

    f.store
        .set_allow_unfollowed_group_conversations(false, 6, &f.device)
        .unwrap();
    let unknown = Keypair::from_seed(&[93; 32]);
    let mut recipients = vec![*f.master.public(), *unknown.public()];
    recipients.sort();
    let group = Share::sign_for(
        "declined-group".into(),
        recipients,
        article(),
        Some("Opening retained".into()),
        7,
        DeviceCert::new_at_micros(&f.friend, f.friend_device.public(), 1, None),
        &f.friend_device,
    )
    .unwrap();
    f.store
        .accept_social_record(
            &ShareFact::Share(Box::new(group.clone())),
            f.friend.public(),
            7,
            &f.device,
        )
        .unwrap();
    assert_eq!(
        f.store.conversation_participation(&group.id).unwrap(),
        ConversationParticipation::Declined
    );
    let saved = f
        .store
        .save_reply_draft(
            &group.id,
            0,
            input("Retain for copying", "declined", None),
            f.device.public(),
        )
        .unwrap();
    assert!(matches!(
        f.store.reply_from_draft(&group.id, &saved, 8, &f.device),
        Err(StoreError::ConversationUnavailable(
            ConversationParticipation::Declined
        ))
    ));
    assert_eq!(f.store.load_reply_draft(&group.id).unwrap(), saved);
    f.store
        .discard_reply_draft(&group.id, saved.revision)
        .unwrap();
}

#[test]
fn local_draft_reads_and_edits_refuse_an_original_root_outside_the_reading_identity() {
    let f = Fixture::new();
    let stranger = Keypair::from_seed(&[94; 32]);
    let root = Share::sign(
        "outside".into(),
        *stranger.public(),
        article(),
        None,
        4,
        DeviceCert::new_at_micros(&f.friend, f.friend_device.public(), 1, None),
        &f.friend_device,
    )
    .unwrap();
    let fact = ShareFact::Share(Box::new(root.clone()));
    // Diagnostic raw writes can retain a valid proof, but cannot confer local
    // participation in a root whose immutable audience excludes this reader.
    f.store.query_with_params("?[key,encoded,author,recipient,share_id,kind,created_at] <- [[$key,$encoded,$author,$recipient,$share,'share',4]] :put social_record {key=>encoded,author,recipient,share_id,kind,created_at}", BTreeMap::from([
        ("key".into(), DataValue::from(fact.key().unwrap())),
        ("encoded".into(), DataValue::Bytes(fact.to_bytes().unwrap())),
        ("author".into(), DataValue::Bytes(f.friend.public().as_bytes().to_vec())),
        ("recipient".into(), DataValue::Bytes(stranger.public().as_bytes().to_vec())),
        ("share".into(), DataValue::from(root.id.as_str())),
    ])).unwrap();
    assert!(f.store.load_reply_draft(&root.id).is_err());
    assert!(
        f.store
            .save_reply_draft(
                &root.id,
                0,
                input("Cannot claim another thread", "one", None),
                f.device.public()
            )
            .is_err()
    );
    assert!(f.store.discard_reply_draft(&root.id, 0).is_err());
}

#[test]
fn invalid_context_key_bounds_and_counter_overflow_preserve_the_draft() {
    let f = Fixture::new();
    let other = f.another_root();
    let parent = f
        .store
        .reply_to_social_share(
            &other.id,
            "other-parent",
            None,
            "Other thread",
            4,
            &f.device,
        )
        .unwrap();
    let ConversationContent::Reply { id, .. } = parent.content else {
        panic!()
    };
    let before = f.save(0, "Keep", "one", None);
    for value in [
        input("Wrong parent", "two", Some(id)),
        input(&"x".repeat(65_537), "two", None),
        input("Bad token", "bad token", None),
        input("Missing parent", "two", Some("f".repeat(64))),
    ] {
        assert!(
            f.store
                .save_reply_draft(&f.root.id, before.revision, value, f.device.public())
                .is_err()
        );
        assert_eq!(f.snapshot(), before);
    }
    assert!(matches!(
        f.store.save_reply_draft(
            &f.root.id,
            before.revision,
            input("Wrong key", "two", None),
            f.friend_device.public()
        ),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    assert!(f.store.load_reply_draft(&"f".repeat(64)).is_err());
    assert!(f.store.discard_reply_draft(&f.root.id, -1).is_err());
    f.store.query_with_params("?[share_id,revision,body,parent,request_id,retry_device] <- [[$id,$revision,'Keep',null,'one',$device]] :put social_reply_draft {share_id=>revision,body,parent,request_id,retry_device}", BTreeMap::from([("id".into(),DataValue::from(f.root.id.as_str())),("revision".into(),DataValue::from(i64::MAX)),("device".into(),DataValue::Bytes(f.device.public().as_bytes().to_vec()))])).unwrap();
    assert!(matches!(
        f.store.discard_reply_draft(&f.root.id, i64::MAX),
        Err(StoreError::ReplyDraftRevisionExhausted { .. })
    ));
    assert!(matches!(
        f.store.save_reply_draft(
            &f.root.id,
            i64::MAX,
            input("Changed", "two", None),
            f.device.public()
        ),
        Err(StoreError::ReplyDraftRevisionExhausted { .. })
    ));
    let at_limit = f.snapshot();
    assert!(matches!(
        f.store
            .reply_from_draft(&f.root.id, &at_limit, 5, &f.device),
        Err(StoreError::ReplyDraftRevisionExhausted { .. })
    ));
    assert_eq!(f.snapshot(), at_limit);
}

#[test]
fn database_guards_refuse_counter_rollback_equal_revision_changes_and_partial_tombstones() {
    let f = Fixture::new();
    let saved = f.save(0, "Keep", "one", None);
    for (revision, body, token, device) in [
        (0_i64, Some("Keep"), Some("one"), Some(*f.device.public())),
        (1, Some("Changed"), Some("one"), Some(*f.device.public())),
        (2, None, Some("one"), Some(*f.device.public())),
        (2, Some("Keep"), None, Some(*f.device.public())),
        (2, Some("Keep"), Some("one"), None),
    ] {
        let params = BTreeMap::from([
            ("id".into(), DataValue::from(f.root.id.as_str())),
            ("revision".into(), DataValue::from(revision)),
            (
                "body".into(),
                body.map(DataValue::from).unwrap_or(DataValue::Null),
            ),
            (
                "token".into(),
                token.map(DataValue::from).unwrap_or(DataValue::Null),
            ),
            (
                "device".into(),
                device
                    .map(|key| DataValue::Bytes(key.as_bytes().to_vec()))
                    .unwrap_or(DataValue::Null),
            ),
        ]);
        assert!(f.store.query_with_params("?[share_id,revision,body,parent,request_id,retry_device] <- [[$id,$revision,$body,null,$token,$device]] :put social_reply_draft {share_id=>revision,body,parent,request_id,retry_device}", params).is_err());
        assert_eq!(f.snapshot(), saved);
    }
}

#[test]
fn a_revoked_local_key_can_copy_and_discard_but_cannot_send() {
    let f = Fixture::new();
    let saved = f.save(0, "Retained private text", "one", None);
    f.store
        .accept_self_revocation(&RevocationCert::new_at_micros(
            &f.master,
            f.device.public(),
            4,
        ))
        .unwrap();
    assert_eq!(f.snapshot(), saved);
    assert!(matches!(
        f.store.reply_from_draft(&f.root.id, &saved, 5, &f.device),
        Err(StoreError::IdentityDeviceUnauthorized)
    ));
    f.store
        .discard_reply_draft(&f.root.id, saved.revision)
        .unwrap();
}

#[test]
fn restart_retains_body_parent_retry_identity_and_the_send_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::with_store(Store::open(dir.path()).unwrap());
    let parent = f
        .store
        .reply_to_social_share(&f.root.id, "parent", None, "Parent", 3, &f.device)
        .unwrap();
    let ConversationContent::Reply { id: parent_id, .. } = parent.content else {
        panic!()
    };
    let saved = f.save(0, "Restart — 文\nkeeps exact text", "one", Some(parent_id));
    let Fixture {
        store,
        device,
        root,
        ..
    } = f;
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load_reply_draft(&root.id).unwrap(), saved);
    let sent = store
        .reply_from_draft(&root.id, &saved, 4, &device)
        .unwrap();
    let fact = PrivateFact::Social(Box::new(ShareFact::Conversation(Box::new(
        sent.record.clone(),
    ))));
    store
        .acknowledge_private_fact(&fact.key().unwrap())
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load_reply_draft(&root.id).unwrap(), sent.draft);
    let before = portable_state(&store);
    let completed = store
        .reply_from_draft(&root.id, &saved, 5, &device)
        .unwrap();
    assert!(completed.already_sent);
    assert_eq!(completed.record, sent.record);
    assert_eq!(portable_state(&store), before);
}

#[test]
fn cloned_writers_serialize_send_against_a_later_autosave() {
    for _ in 0..8 {
        let f = Fixture::new();
        let submitted = f.save(0, "Submitted", "one", None);
        let barrier = Arc::new(Barrier::new(2));
        let writer = f.store.clone();
        let root = f.root.id.clone();
        let started = barrier.clone();
        let send_snapshot = submitted.clone();
        let send = std::thread::spawn(move || {
            started.wait();
            writer.reply_from_draft(&root, &send_snapshot, 4, &Keypair::from_seed(&[82; 32]))
        });
        let writer = f.store.clone();
        let root = f.root.id.clone();
        let save = std::thread::spawn(move || {
            barrier.wait();
            let device = Keypair::from_seed(&[82; 32]);
            writer.save_reply_draft(&root, 1, input("Newer", "one", None), device.public())
        });
        let sent = send.join().unwrap();
        let saved = save.join().unwrap();
        assert_ne!(sent.is_ok(), saved.is_ok());
        match (sent, saved) {
            (Ok(sent), Err(StoreError::ReplyDraftConflict { .. })) => {
                assert_eq!(f.snapshot(), sent.draft)
            }
            (Err(StoreError::ReplyDraftConflict { .. }), Ok(saved)) => {
                assert_eq!(f.snapshot(), saved);
                assert!(
                    f.store
                        .social_conversation(&f.root.id)
                        .unwrap()
                        .replies
                        .is_empty()
                );
            }
            _ => panic!("one exact operation must win without losing the other composition"),
        }
    }
}

#[test]
fn concurrent_initial_saves_have_one_cas_winner_and_preserve_its_exact_body() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|text| {
            let store = f.store.clone();
            let root = f.root.id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.save_reply_draft(
                    &root,
                    0,
                    input(text, text, None),
                    Keypair::from_seed(&[82; 32]).public(),
                )
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::ReplyDraftConflict { .. })))
            .count(),
        1
    );
    let winner = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(f.snapshot(), winner);
}

#[test]
fn exact_send_and_explicit_discard_share_one_terminal_revision() {
    for _ in 0..4 {
        let f = Fixture::new();
        let saved = f.save(0, "Either send or discard", "one", None);
        let barrier = Arc::new(Barrier::new(2));
        let store = f.store.clone();
        let root = f.root.id.clone();
        let started = barrier.clone();
        let send = std::thread::spawn(move || {
            started.wait();
            store.reply_from_draft(&root, &saved, 4, &Keypair::from_seed(&[82; 32]))
        });
        let store = f.store.clone();
        let root = f.root.id.clone();
        let discard = std::thread::spawn(move || {
            barrier.wait();
            store.discard_reply_draft(&root, 1)
        });
        let sent = send.join().unwrap();
        let cleared = discard.join().unwrap().unwrap();
        assert_eq!(
            cleared,
            ReplyDraftSnapshot {
                revision: 2,
                draft: None
            }
        );
        assert_eq!(f.snapshot(), cleared);
        let replies = f
            .store
            .social_conversation(&f.root.id)
            .unwrap()
            .replies
            .len();
        match sent {
            Ok(confirmation) => {
                assert_eq!(confirmation.draft, cleared);
                assert_eq!(replies, 1);
            }
            Err(StoreError::ReplyDraftConflict { .. }) => assert_eq!(replies, 0),
            Err(error) => panic!("unexpected send/discard result: {error}"),
        }
    }
}

#[test]
fn exact_send_and_leave_serialize_without_losing_an_unsent_draft() {
    for _ in 0..4 {
        let f = Fixture::new();
        let saved = f.save(0, "Keep if Leave commits first", "one", None);
        let barrier = Arc::new(Barrier::new(2));
        let store = f.store.clone();
        let root = f.root.id.clone();
        let snapshot = saved.clone();
        let started = barrier.clone();
        let send = std::thread::spawn(move || {
            started.wait();
            store.reply_from_draft(&root, &snapshot, 4, &Keypair::from_seed(&[82; 32]))
        });
        let store = f.store.clone();
        let root = f.root.id.clone();
        let leave = std::thread::spawn(move || {
            barrier.wait();
            store.leave_social_conversation(&root, 4, &Keypair::from_seed(&[82; 32]))
        });
        let sent = send.join().unwrap();
        leave.join().unwrap().unwrap();
        match sent {
            Ok(confirmation) => assert_eq!(f.snapshot(), confirmation.draft),
            Err(StoreError::ConversationUnavailable(ConversationParticipation::Left)) => {
                assert_eq!(f.snapshot(), saved)
            }
            Err(error) => panic!("unexpected send/leave result: {error}"),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn arbitrary_delayed_saves_and_discards_never_roll_back_or_resurrect(operations in prop::collection::vec((0u8..3, any::<bool>()), 1..16)) {
        let f = Fixture::new();
        for (index, (delay, discard)) in operations.into_iter().enumerate() {
            let before = f.snapshot();
            let expected = before.revision.saturating_sub(i64::from(delay));
            let result = if discard {
                f.store.discard_reply_draft(&f.root.id, expected)
            } else {
                f.store.save_reply_draft(&f.root.id, expected, input(&format!("composition {index}"), &format!("retry-{index}"), None), f.device.public())
            };
            let after = f.snapshot();
            prop_assert!(after.revision >= before.revision);
            if result.is_err() { prop_assert_eq!(&after, &before); }
            if after != before { prop_assert_eq!(after.revision, before.revision + 1); }
        }
    }
}
