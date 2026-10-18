//! Background orchestration of the durable bridge, independent of the webview.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::poll_fn,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_core::Stream;
use iroh::{EndpointAddr, PublicKey};
use iroh_docs::{Entry, engine::LiveEvent, store::Query};
use tokio::task::JoinHandle;

use crate::{Result, SyncError, bridge::SelfBridge, error::transport_error};
use carrel_store::private_library::PrivateReceive;

/// An app-owned worker. Restart reconstructs work from Cozo and retained SDK headers.
pub struct SelfSyncWorker {
    /// The bounded bridge used for explicit pairing and device management.
    pub bridge: Arc<SelfBridge>,
    revision: Arc<AtomicU64>,
    task: JoinHandle<()>,
}

impl SelfSyncWorker {
    /// Subscribe before joining peers so new metadata cannot fall between setup steps.
    pub async fn start(bridge: Arc<SelfBridge>) -> Result<Self> {
        let events = bridge
            .doc
            .subscribe()
            .await
            .map_err(|error| transport_error("subscribe private document", error))?;
        let mut addresses = BTreeMap::new();
        for (device, bytes) in bridge.store.private_peer_addresses()? {
            let address: EndpointAddr =
                serde_json::from_slice(&bytes).map_err(|_| SyncError::InvalidEntry)?;
            if address.id.as_bytes() != device.as_bytes() {
                return Err(SyncError::InvalidEntry);
            }
            addresses.insert(address.id, address);
        }
        bridge
            .doc
            .start_sync(addresses.values().cloned().collect())
            .await
            .map_err(|error| transport_error("resume private peers", error))?;
        let revision = Arc::new(AtomicU64::new(0));
        let generation = revision.clone();
        let background = bridge.clone();
        let task = tokio::spawn(async move {
            tracing::debug!("private sync worker started");
            let mut events = pin!(events);
            let mut pending = VecDeque::new();
            let mut keys = BTreeMap::new();
            // Scan retained headers once, including work interrupted before its
            // body arrived. Subsequent work is driven by insert events and retries.
            for prefix in ["fact/certificate/", "fact/revocation/", "fact/"] {
                if let Ok(entries) = background
                    .doc
                    .get_many(Query::all().key_prefix(prefix))
                    .await
                {
                    let mut entries = pin!(entries);
                    while let Some(Ok(entry)) = poll_fn(|cx| entries.as_mut().poll_next(cx)).await {
                        enqueue(&mut pending, &mut keys, entry, None);
                    }
                }
            }
            let mut online = BTreeSet::<PublicKey>::new();
            let mut publish_after = None;
            let mut cache_after = None;
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            let mut reconcile_at = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                tokio::select! {
                    event = poll_fn(|cx| events.as_mut().poll_next(cx)) => match event {
                        Some(Ok(LiveEvent::InsertRemote {from, entry, ..})) => {
                            tracing::debug!(byte_size = entry.content_len(), "received private header");
                            online.insert(from);
                            if entry.key().starts_with(b"fact/") { enqueue(&mut pending, &mut keys, entry, Some(from)); }
                        },
                        Some(Ok(LiveEvent::NeighborUp(peer))) => { tracing::debug!("private peer connected"); online.insert(peer); },
                        Some(Ok(LiveEvent::NeighborDown(peer))) => { online.remove(&peer); },
                        Some(Ok(LiveEvent::SyncFinished(result))) => {
                            if result.result.is_ok() { online.insert(result.peer); }
                            tracing::debug!(succeeded = result.result.is_ok(), "private reconciliation finished");
                        },
                        Some(Ok(_)) => {},
                        Some(Err(_)) | None => { tracing::debug!("private event subscription ended"); break; },
                    },
                    _ = tick.tick() => {
                        tracing::debug!(pending = pending.len(), connected = online.len(), "private work tick");
                        // Address hints can be updated by a pairing confirmation.
                        if let Ok(peers) = background.store.private_peer_addresses() {
                            for (key, bytes) in peers {
                                if let Ok(address) = serde_json::from_slice::<EndpointAddr>(&bytes)
                                    && address.id.as_bytes() == key.as_bytes() { addresses.insert(address.id, address); }
                            }
                        }
                        // Gossip can take time to rebuild after an endpoint restarts.
                        // SDK set reconciliation also retries offline peers and moves
                        // new headers without scanning Cozo or cached article bodies.
                        if tokio::time::Instant::now() >= reconcile_at {
                            let _ = background.doc.start_sync(addresses.values().cloned().collect()).await;
                            reconcile_at = tokio::time::Instant::now() + Duration::from_secs(5);
                        }
                        if let Ok(result) = background.publish(publish_after.take()).await { publish_after = result.after; }
                        let providers: Vec<_> = online.iter().map(|peer| addresses.get(peer).cloned().unwrap_or_else(|| EndpointAddr::new(*peer))).collect();
                        let mut changed = false;
                        if !providers.is_empty() {
                            for _ in 0..pending.len().min(100) {
                                let entry = pending.pop_front().expect("bounded pending queue");
                                let announcer = keys.get(&(entry.author(), entry.key().to_vec())).copied().flatten();
                                match receive_from_providers(&background, &entry, announcer, &providers).await {
                                    Ok(result) => { tracing::debug!(?result, "private header materialized"); changed |= result == PrivateReceive::Applied; keys.remove(&(entry.author(), entry.key().to_vec())); },
                                    Err(SyncError::Transport {..} | SyncError::BlobIntegrity | SyncError::Blob(_) | SyncError::DeviceUnauthorized) => pending.push_back(entry),
                                    Err(error) => { tracing::debug!(error_kind = ?std::mem::discriminant(&error), "private header rejected"); keys.remove(&(entry.author(), entry.key().to_vec())); },
                                }
                            }
                            if let Ok(result) = background.transfer_cache(cache_after.take(), &providers).await { cache_after = result.after; changed |= result.changed; }
                        }
                        if let Ok(promoted) = background.promote_dependencies().await { changed |= promoted; }
                        if changed { generation.fetch_add(1, Ordering::Release); }
                    }
                }
            }
        });
        Ok(Self {
            bridge,
            revision,
            task,
        })
    }

    /// A quiet invalidation generation; it carries no article contents.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Stop the worker and leave the document before releasing the app.
    pub async fn stop(&self) {
        self.task.abort();
        let _ = self.bridge.doc.leave().await;
        let _ = self.bridge.doc.close().await;
        let _ = self.bridge.host.stop().await;
    }
}

impl Drop for SelfSyncWorker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn enqueue(
    queue: &mut VecDeque<Entry>,
    keys: &mut BTreeMap<(iroh_docs::AuthorId, Vec<u8>), Option<PublicKey>>,
    entry: Entry,
    announcer: Option<PublicKey>,
) {
    match keys.entry((entry.author(), entry.key().to_vec())) {
        std::collections::btree_map::Entry::Vacant(key) => {
            key.insert(announcer);
            queue.push_back(entry);
        }
        std::collections::btree_map::Entry::Occupied(mut key) => {
            if announcer.is_some() {
                key.insert(announcer);
            }
        }
    }
}

async fn receive_from_providers(
    bridge: &SelfBridge,
    entry: &Entry,
    announcer: Option<PublicKey>,
    providers: &[EndpointAddr],
) -> Result<PrivateReceive> {
    // A recovered desktop can publish while an older peer hint remains offline.
    // A live announcer may also relay a header written by an offline desktop.
    let mut candidates: Vec<_> = providers.iter().collect();
    candidates.sort_by_key(|provider| {
        (
            Some(provider.id) != announcer,
            provider.id.as_bytes() != entry.author().as_bytes(),
        )
    });
    let mut failure = SyncError::DeviceUnauthorized;
    for provider in candidates {
        match bridge.receive(entry, provider.clone()).await {
            Ok(result) => return Ok(result),
            Err(
                error @ (SyncError::Transport { .. }
                | SyncError::BlobIntegrity
                | SyncError::Blob(_)
                | SyncError::DeviceUnauthorized),
            ) => failure = error,
            Err(error) => return Err(error),
        }
    }
    Err(failure)
}

#[cfg(test)]
mod tests;
