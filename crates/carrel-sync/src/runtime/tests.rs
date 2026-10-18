use carrel_core::{
    identity::{DeviceCert, Keypair, RevocationCert, canonical_bytes},
    private_library::PrivateFact,
    private_state::{StateKey, StateKind, StateValue, SubscriptionState},
};
use carrel_store::{Store, blobs::BlobStore};
use iroh::{Endpoint, endpoint::presets};
use iroh_docs::{
    Author, AuthorId,
    api::protocol::{AddrInfoOptions, ShareMode},
};

use super::*;
use crate::transport::SelfTransport;

async fn host(store: Store, path: &std::path::Path, key: &Keypair) -> Arc<SelfTransport> {
    Arc::new(
        SelfTransport::start_with_builder(
            store,
            path,
            key,
            Endpoint::builder(presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
        .unwrap(),
    )
}

async fn entry(source: &SelfBridge, author: AuthorId, fact: &PrivateFact) -> Entry {
    let key = format!("fact/{}", fact.key().unwrap());
    source
        .doc
        .set_bytes(author, key.as_bytes().to_vec(), fact.to_bytes().unwrap())
        .await
        .unwrap();
    source
        .doc
        .get_exact(author, key.as_bytes(), false)
        .await
        .unwrap()
        .unwrap()
}

fn subscription(url: &str, signer: &Keypair) -> PrivateFact {
    PrivateFact::State(
        carrel_core::private_state::StateRevision::sign(
            StateKey {
                kind: StateKind::Subscription,
                target: url.into(),
            },
            StateValue::Subscription(SubscriptionState {
                subscribed: true,
                title: None,
                folder: None,
                auto_mark_read: false,
            }),
            vec![],
            1,
            signer,
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn offline_first_provider_does_not_starve_a_live_own_writer_or_bypass_sender_checks() {
    let dir = tempfile::tempdir().unwrap();
    let root = Keypair::from_seed(&[221; 32]);
    let writer = Keypair::from_seed(&[222; 32]);
    let reader = Keypair::from_seed(&[223; 32]);
    let offline = Keypair::from_seed(&[224; 32]);
    let source_store = Store::open(&dir.path().join("source/store")).unwrap();
    let receiver_store = Store::open(&dir.path().join("receiver/store")).unwrap();
    for (store, key) in [(&source_store, &writer), (&receiver_store, &reader)] {
        store.migrate().unwrap();
        store
            .bind_self_identity(
                &DeviceCert::new_at_micros(&root, key.public(), 1, None),
                key,
            )
            .unwrap();
        for device in [&writer, &reader, &offline] {
            store
                .accept_self_device_certificate(&DeviceCert::new_at_micros(
                    &root,
                    device.public(),
                    1,
                    None,
                ))
                .unwrap();
        }
    }
    let source_host = host(
        source_store.clone(),
        &dir.path().join("source/mirror"),
        &writer,
    )
    .await;
    let receiver_host = host(
        receiver_store.clone(),
        &dir.path().join("receiver/mirror"),
        &reader,
    )
    .await;
    let source = SelfBridge::create(
        source_store,
        BlobStore::open(dir.path().join("source/cache")),
        source_host,
    )
    .await
    .unwrap();
    let ticket = source
        .doc
        .share(ShareMode::Write, AddrInfoOptions::Id)
        .await
        .unwrap();
    let receiver = SelfBridge::import(
        receiver_store,
        BlobStore::open(dir.path().join("receiver/cache")),
        receiver_host,
        ticket.capability,
    )
    .await
    .unwrap();
    // The first hint is certified but has no listening endpoint. Selecting it
    // for every retry used to prevent the live writer from ever being tried.
    let offline_address =
        EndpointAddr::new(iroh::PublicKey::from_bytes(offline.public().as_bytes()).unwrap());
    let providers = [offline_address.clone(), source.address()];
    let url = "https://provider-failover.example/feed";
    let fact = subscription(url, &writer);
    let header = entry(&source, AuthorId::from(*writer.public().as_bytes()), &fact).await;
    assert!(
        receiver
            .host
            .blobs()
            .get_bytes(header.content_hash())
            .await
            .is_err()
    );
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(10),
            receive_from_providers(&receiver, &header, None, &providers)
        )
        .await
        .expect("a stale first hint must not delay the live writer")
        .unwrap(),
        PrivateReceive::Applied
    );
    assert!(receiver.store.get_feed(url).unwrap().is_some());

    // Cached bytes do not turn an unknown or revoked provider into an own
    // sender. Failover must still enter the bridge's sender gate on every try.
    let unknown = Keypair::from_seed(&[225; 32]);
    let unknown_address =
        EndpointAddr::new(iroh::PublicKey::from_bytes(unknown.public().as_bytes()).unwrap());
    assert!(matches!(
        receive_from_providers(
            &receiver,
            &header,
            Some(unknown_address.id),
            &[unknown_address]
        )
        .await,
        Err(SyncError::DeviceUnauthorized)
    ));

    // Retained headers may be queued before a live insertion identifies a
    // relay. Update the announcer without duplicating the pending work.
    let historical_author = Author::from_bytes(&offline.to_seed_bytes());
    let historical_id = historical_author.id();
    source
        .host
        .docs()
        .author_import(historical_author)
        .await
        .unwrap();
    let announced_url = "https://announced-relay.example/feed";
    let announced = entry(
        &source,
        historical_id,
        &subscription(announced_url, &writer),
    )
    .await;
    let mut queue = VecDeque::new();
    let mut keys = BTreeMap::new();
    enqueue(&mut queue, &mut keys, announced.clone(), None);
    enqueue(&mut queue, &mut keys, announced, Some(source.address().id));
    assert_eq!(queue.len(), 1);
    let announced = queue.pop_front().unwrap();
    let announcer = keys[&(announced.author(), announced.key().to_vec())];
    assert_eq!(announcer, Some(source.address().id));
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(10),
            receive_from_providers(&receiver, &announced, announcer, &providers),
        )
        .await
        .expect("a live relay must take precedence over the offline header writer")
        .unwrap(),
        PrivateReceive::Applied
    );
    assert!(receiver.store.get_feed(announced_url).unwrap().is_some());

    receiver
        .store
        .accept_self_revocation(&RevocationCert::new_at_micros(&root, offline.public(), 2))
        .unwrap();
    assert!(matches!(
        receive_from_providers(
            &receiver,
            &header,
            None,
            std::slice::from_ref(&offline_address)
        )
        .await,
        Err(SyncError::DeviceUnauthorized)
    ));

    // A historical certified header writer may be offline/revoked while an
    // active own desktop relays the original valid fact. The author preference
    // must fall through after that sender is denied, without dropping the fact.
    let relayed_url = "https://own-relay.example/feed";
    let relayed = entry(&source, historical_id, &subscription(relayed_url, &writer)).await;
    assert_eq!(
        receive_from_providers(&receiver, &relayed, None, &providers)
            .await
            .unwrap(),
        PrivateReceive::Applied
    );
    assert!(receiver.store.get_feed(relayed_url).unwrap().is_some());

    // An authorized relay cannot legitimize an uncertified SDK author or a
    // changed original signature.
    let unknown_author = source.host.docs().author_create().await.unwrap();
    let denied_url = "https://uncertified-writer.example/feed";
    let denied = entry(&source, unknown_author, &subscription(denied_url, &writer)).await;
    assert!(matches!(
        receive_from_providers(&receiver, &denied, None, &providers).await,
        Err(SyncError::DeviceUnauthorized)
    ));
    assert!(
        receiver
            .host
            .blobs()
            .get_bytes(denied.content_hash())
            .await
            .is_err()
    );
    assert!(receiver.store.get_feed(denied_url).unwrap().is_none());
    let forged_url = "https://forged-signature.example/feed";
    let PrivateFact::State(mut forged) = subscription(forged_url, &writer) else {
        unreachable!()
    };
    forged.at_micros += 1;
    let forged_key = format!("fact/state/{}", "0".repeat(64));
    let writer_id = AuthorId::from(*writer.public().as_bytes());
    source
        .doc
        .set_bytes(
            writer_id,
            forged_key.as_bytes().to_vec(),
            canonical_bytes(&PrivateFact::State(forged)).unwrap(),
        )
        .await
        .unwrap();
    let forged_header = source
        .doc
        .get_exact(writer_id, forged_key.as_bytes(), false)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        receive_from_providers(&receiver, &forged_header, None, &providers).await,
        Err(SyncError::Fact(_))
    ));
    assert!(receiver.store.get_feed(forged_url).unwrap().is_none());

    for bridge in [source, receiver] {
        bridge.doc.close().await.unwrap();
        bridge.host.stop().await.unwrap();
    }
}
