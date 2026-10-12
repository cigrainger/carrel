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
