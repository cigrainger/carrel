use std::time::Duration;
use std::{future::poll_fn, pin::Pin};

use carrel_core::identity::{DeviceCert, Keypair, RevocationCert};
use carrel_store::Store;
use futures_core::Stream;
use iroh::endpoint::{ConnectError, ConnectWithOptsError, Connection, ConnectionError, presets};
use iroh_docs::api::protocol::{AddrInfoOptions, ShareMode};
use iroh_docs::store::Query;

use super::*;
use crate::SyncError;

const DEADLINE: Duration = Duration::from_secs(10);

struct Pair {
    a: Store,
    b: Store,
    master: Keypair,
    ka: Keypair,
    kb: Keypair,
}

fn pair() -> Pair {
    let master = Keypair::from_seed(&[110; 32]);
    let ka = Keypair::from_seed(&[111; 32]);
    let kb = Keypair::from_seed(&[112; 32]);
    let a = Store::open_in_memory().unwrap();
    let b = Store::open_in_memory().unwrap();
    a.migrate().unwrap();
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

async fn local_transport(store: &Store, path: &Path, key: &Keypair) -> SelfTransport {
    SelfTransport::start_with_builder(
        store.clone(),
        path,
        key,
        Endpoint::builder(presets::Minimal),
    )
    .await
    .unwrap()
}

async fn raw_endpoint(key: &Keypair) -> Endpoint {
    let seed = Zeroizing::new(key.to_seed_bytes());
    Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::from_bytes(&seed))
        .bind()
        .await
        .unwrap()
}

async fn denied(connection: &Connection) {
    let reason = tokio::time::timeout(DEADLINE, connection.closed())
        .await
        .expect("authorization close deadline");
    assert!(matches!(reason, ConnectionError::ApplicationClosed(close)
        if close.error_code == iroh::endpoint::VarInt::from_u32(403)
            && close.reason.as_ref() == b"private device authorization required"));
}

async fn await_connections(transport: &SelfTransport, count: usize) {
    tokio::time::timeout(DEADLINE, async {
        while transport.authorization.live_connection_count() < count {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("completed authorized handshakes");
}

async fn await_content(
    transport: &SelfTransport,
    doc: &iroh_docs::api::Doc,
    key: &[u8],
    expected: &[u8],
) {
    tokio::time::timeout(DEADLINE, async {
        loop {
            if let Some(entry) = doc.get_one(Query::key_exact(key)).await.unwrap()
                && let Ok(bytes) = transport.blobs().get_bytes(entry.content_hash()).await
                && bytes.as_ref() == expected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("replicated document and verified content");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_protocols_reject_uncertified_tls_peers_even_with_known_capabilities() {
    let p = pair();
    let dir = tempfile::tempdir().unwrap();
    let a = local_transport(&p.a, dir.path(), &p.ka).await;
    let doc = a.docs().create().await.unwrap();
    let author = a.docs().author_default().await.unwrap();
    let hash = doc
        .set_bytes(
            author,
            b"private/key".to_vec(),
            b"Private test-only content".to_vec(),
        )
        .await
        .unwrap();
    let mut ticket = doc
        .share(ShareMode::Write, AddrInfoOptions::Id)
        .await
        .unwrap();
    ticket.nodes = vec![a.address()];
    let outsider = raw_endpoint(&Keypair::from_seed(&[113; 32])).await;
    for alpn in [iroh_docs::ALPN, iroh_blobs::ALPN, iroh_gossip::ALPN] {
        let connection = outsider.connect(a.address(), alpn).await.unwrap();
        denied(&connection).await;
    }
    // The attacker knows the actual private content hash, not just an ALPN.
    // Its unrestricted downloader must still fail at the serving endpoint.
    let attacker_blobs = iroh_blobs::store::mem::MemStore::default();
    let result = tokio::time::timeout(
        DEADLINE,
        attacker_blobs
            .downloader(&outsider)
            .download(hash, [a.endpoint().id()]),
    )
    .await
    .expect("denied blob download terminates");
    assert!(result.is_err());
    assert!(attacker_blobs.get_bytes(hash).await.is_err());
    // Run an unrestricted SDK host with the stolen write capability. This
    // checks actual document synchronization, not merely ticket parsing.
    let attacker_gossip = Gossip::builder().spawn(outsider.clone());
    let attacker_docs = Docs::memory()
        .spawn(
            outsider.clone(),
            (*attacker_blobs).clone(),
            attacker_gossip.clone(),
        )
        .await
        .unwrap();
    let attacker_router = Router::builder(outsider.clone())
        .accept(iroh_docs::ALPN, attacker_docs.clone())
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(&attacker_blobs, None))
        .accept(iroh_gossip::ALPN, attacker_gossip)
        .spawn();
    let stolen = attacker_docs
        .import_namespace(ticket.capability)
        .await
        .unwrap();
    let mut events = stolen.subscribe().await.unwrap();
    stolen.start_sync(ticket.nodes).await.unwrap();
    tokio::time::timeout(DEADLINE, async {
        loop {
            let event = poll_fn(|cx| Pin::new(&mut events).poll_next(cx))
                .await
                .unwrap()
                .unwrap();
            if let iroh_docs::engine::LiveEvent::SyncFinished(sync) = event {
                assert_eq!(sync.peer, a.endpoint().id());
                assert!(sync.result.is_err());
                break;
            }
        }
    })
    .await
    .expect("denied document synchronization terminates");
    assert!(
        stolen
            .get_one(Query::key_exact(b"private/key"))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(a.authorization.live_connection_count(), 0);
    stolen.close().await.unwrap();
    attacker_router.shutdown().await.unwrap();
    a.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outgoing_private_dials_are_rejected_before_connecting_to_an_untrusted_peer() {
    let p = pair();
    let dir = tempfile::tempdir().unwrap();
    let a = local_transport(&p.a, dir.path(), &p.ka).await;
    let outsider = raw_endpoint(&Keypair::from_seed(&[113; 32])).await;
    for alpn in [
        iroh_docs::ALPN,
        iroh_blobs::ALPN,
        iroh_gossip::ALPN,
        b"unregistered/private".as_slice(),
    ] {
        let error = a
            .endpoint()
            .connect(outsider.addr(), alpn)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ConnectError::Connect {
                source: ConnectWithOptsError::LocallyRejected { .. },
                ..
            }
        ));
    }
    outsider.close().await;
    a.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_revocation_closes_existing_connections_for_every_private_protocol() {
    let p = pair();
    let dir = tempfile::tempdir().unwrap();
    let a = local_transport(&p.a, dir.path(), &p.ka).await;
    let b = raw_endpoint(&p.kb).await;
    let mut live = Vec::new();
    for alpn in [iroh_docs::ALPN, iroh_blobs::ALPN, iroh_gossip::ALPN] {
        live.push(b.connect(a.address(), alpn).await.unwrap());
    }
    await_connections(&a, 3).await;
    p.a.clone()
        .accept_self_revocation(&RevocationCert::new_at_micros(
            &p.master,
            p.kb.public(),
            i64::MAX,
        ))
        .unwrap();
    // No explicit refresh: a typed store write must wake the live host itself.
    for connection in &live {
        denied(connection).await;
    }
    assert_eq!(a.authorization.live_connection_count(), 0);
    let replay = DeviceCert::new_at_micros(&p.master, p.kb.public(), 1, Some("B".into()));
    p.a.accept_self_device_certificate(&replay).unwrap();
    for alpn in [iroh_docs::ALPN, iroh_blobs::ALPN, iroh_gossip::ALPN] {
        let connection = b.connect(a.address(), alpn).await.unwrap();
        denied(&connection).await;
        let error = a.endpoint().connect(b.addr(), alpn).await.unwrap_err();
        assert!(matches!(
            error,
            ConnectError::Connect {
                source: ConnectWithOptsError::LocallyRejected { .. },
                ..
            }
        ));
    }
    b.close().await;
    a.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoking_the_local_installation_closes_its_live_access_and_prevents_restart() {
    let p = pair();
    let dir = tempfile::tempdir().unwrap();
    let a = local_transport(&p.a, dir.path(), &p.ka).await;
    let b = raw_endpoint(&p.kb).await;
    let connection = b.connect(a.address(), iroh_docs::ALPN).await.unwrap();
    await_connections(&a, 1).await;
    p.a.accept_self_revocation(&RevocationCert::new_at_micros(&p.master, p.ka.public(), 2))
        .unwrap();
    assert!(matches!(
        a.refresh_authorization().await,
        Err(SyncError::DeviceUnauthorized)
    ));
    denied(&connection).await;
    b.close().await;
    a.shutdown().await.unwrap();
    assert!(matches!(
        SelfTransport::start(p.a, dir.path(), &p.ka).await,
        Err(SyncError::DeviceUnauthorized)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_authorized_hosts_exchange_document_blobs_and_reopen_persistent_storage() {
    let p = pair();
    let adir = tempfile::tempdir().unwrap();
    let bdir = tempfile::tempdir().unwrap();
    let a = local_transport(&p.a, adir.path(), &p.ka).await;
    let b = local_transport(&p.b, bdir.path(), &p.kb).await;
    assert_eq!(a.endpoint().id().as_bytes(), p.ka.public().as_bytes());
    assert_eq!(b.endpoint().id().as_bytes(), p.kb.public().as_bytes());
    let aa = a.docs().author_default().await.unwrap();
    let ba = b.docs().author_default().await.unwrap();
    assert_eq!(aa.as_bytes(), p.ka.public().as_bytes());
    assert_eq!(ba.as_bytes(), p.kb.public().as_bytes());
    let doc_a = a.docs().create().await.unwrap();
    let id = doc_a.id();
    doc_a
        .set_bytes(
            aa,
            b"a/test".to_vec(),
            "Offline café 日本語".as_bytes().to_vec(),
        )
        .await
        .unwrap();
    let mut ticket = doc_a
        .share(ShareMode::Write, AddrInfoOptions::Id)
        .await
        .unwrap();
    ticket.nodes = vec![a.address()];
    let doc_b = b.docs().import(ticket).await.unwrap();
    await_content(&b, &doc_b, b"a/test", "Offline café 日本語".as_bytes()).await;
    doc_b
        .set_bytes(
            ba,
            b"b/test".to_vec(),
            b"Independent device contribution".to_vec(),
        )
        .await
        .unwrap();
    await_content(&a, &doc_a, b"b/test", b"Independent device contribution").await;
    doc_b.close().await.unwrap();
    b.shutdown().await.unwrap();
    let reopened = local_transport(&p.b, bdir.path(), &p.kb).await;
    let restored = reopened.docs().open(id).await.unwrap().unwrap();
    await_content(
        &reopened,
        &restored,
        b"a/test",
        "Offline café 日本語".as_bytes(),
    )
    .await;
    await_content(
        &reopened,
        &restored,
        b"b/test",
        b"Independent device contribution",
    )
    .await;
    assert_eq!(reopened.docs().author_default().await.unwrap(), ba);
    restored.start_sync(vec![a.address()]).await.unwrap();
    doc_a
        .set_bytes(
            aa,
            b"a/after-restart".to_vec(),
            b"New content after reconnection".to_vec(),
        )
        .await
        .unwrap();
    await_content(
        &reopened,
        &restored,
        b"a/after-restart",
        b"New content after reconnection",
    )
    .await;
    restored
        .set_bytes(
            ba,
            b"b/after-restart".to_vec(),
            b"The restarted device can write".to_vec(),
        )
        .await
        .unwrap();
    await_content(
        &a,
        &doc_a,
        b"b/after-restart",
        b"The restarted device can write",
    )
    .await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(bdir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    restored.close().await.unwrap();
    reopened.shutdown().await.unwrap();
    doc_a.close().await.unwrap();
    a.shutdown().await.unwrap();
}

#[tokio::test]
async fn starting_transport_never_selects_an_identity_or_replaces_a_device_key() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let device = Keypair::from_seed(&[111; 32]);
    assert!(matches!(
        SelfTransport::start(store.clone(), dir.path(), &device).await,
        Err(SyncError::IdentityUnbound)
    ));
    assert_eq!(store.self_identity().unwrap(), None);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let p = pair();
    assert!(matches!(
        SelfTransport::start(p.a, dir.path(), &p.kb).await,
        Err(SyncError::DeviceKeyMismatch)
    ));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authorization_query_failure_closes_live_connections_and_surfaces_the_store_error() {
    let p = pair();
    let dir = tempfile::tempdir().unwrap();
    let a = local_transport(&p.a, dir.path(), &p.ka).await;
    let b = raw_endpoint(&p.kb).await;
    let connection = b.connect(a.address(), iroh_blobs::ALPN).await.unwrap();
    await_connections(&a, 1).await;
    // Simulate a source-of-truth query failure, without inventing a cached ACL.
    p.a.query("::remove device_certificate").unwrap();
    assert!(matches!(
        a.refresh_authorization().await,
        Err(SyncError::Store(_))
    ));
    denied(&connection).await;
    b.close().await;
    a.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_cache_and_docs_storage_fail_without_leaking_the_open_blob_store() {
    let p = pair();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ordinary-file");
    std::fs::write(&file, b"Keep this file").unwrap();
    let failure = SelfTransport::start(p.a.clone(), &file, &p.ka).await;
    assert!(matches!(
        failure,
        Err(SyncError::Transport {
            operation: "create sync cache",
            ..
        })
    ));
    assert_eq!(std::fs::read(&file).unwrap(), b"Keep this file");
    let cache = dir.path().join("cache");
    let docs_path = cache.join("docs/docs.redb");
    std::fs::create_dir_all(docs_path.parent().unwrap()).unwrap();
    std::fs::write(&docs_path, b"Not a Redb database").unwrap();
    let failure = SelfTransport::start_with_builder(
        p.a.clone(),
        &cache,
        &p.ka,
        Endpoint::builder(presets::Minimal),
    )
    .await;
    assert!(matches!(
        failure,
        Err(SyncError::Transport {
            operation: "open docs mirror",
            ..
        })
    ));
    assert_eq!(std::fs::read(&docs_path).unwrap(), b"Not a Redb database");
    std::fs::remove_file(&docs_path).unwrap();
    // The previous partial start must have released its persistent blob store.
    let recovered = local_transport(&p.a, &cache, &p.ka).await;
    recovered.shutdown().await.unwrap();
}
