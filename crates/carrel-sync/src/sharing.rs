//! Recipient-filtered access to a distinct mirror; raw private protocols stay closed.
use crate::{Result, SyncError, error::transport_error};
use carrel_core::identity::{Keypair, PublicKey, canonical_bytes};
use carrel_core::sharing::{ContactCard, MAX_SHARE_BYTES, ShareFact};
use carrel_store::{
    Store,
    blobs::{BlobId, BlobStore as NativeCache},
    sharing::{SocialDocument, SocialReceive},
};
use futures_core::Stream;
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_blobs::api::Store as MirrorBlobs;
use iroh_docs::{
    AuthorId, Capability,
    api::Doc,
    protocol::Docs,
    store::{DownloadPolicy, Query, SortBy, SortDirection},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, future::poll_fn, pin::pin, sync::Arc, time::Duration};

/// Social requests have no docs/gossip/raw-blob access.
pub const ALPN: &[u8] = b"carrel/share/1";
pub mod runtime;
const DEADLINE: Duration = Duration::from_secs(30);
const MAX_REQUEST: usize = 64 * 1024;
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Request {
    card: ContactCard,
    operation: Operation,
}
#[derive(Serialize, Deserialize)]
enum Operation {
    Records { after: Option<String> },
    Blob { share_id: String, blob_id: String },
}
#[derive(Serialize, Deserialize)]
enum Response {
    Records {
        card: Box<ContactCard>,
        records: Vec<ShareFact>,
        after: Option<String>,
    },
    Denied,
}

/// Separate recipient-prefixed mirror and bounded native-cache service.
pub struct SocialBridge {
    store: Store,
    cache: NativeCache,
    doc: Doc,
    mirror: MirrorBlobs,
    author: AuthorId,
    device: Arc<Keypair>,
    endpoint: Endpoint,
    requests: tokio::sync::Semaphore,
    cache_cursors: std::sync::Mutex<
        std::collections::BTreeMap<PublicKey, carrel_store::sharing::PendingShareBlob>,
    >,
}

/// Partial cache progress stays visible even when another image needs retry.
#[derive(Default)]
pub struct SocialCacheTransfer {
    /// A body or image grant became ready.
    pub changed: bool,
    /// Metadata-only failures retained for retry, without any fact body.
    pub errors: Vec<SyncError>,
}
impl std::fmt::Debug for SocialBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocialBridge").finish_non_exhaustive()
    }
}
impl SocialBridge {
    /// Reconstruct the separate mirror from Cozo without publishing capabilities.
    pub async fn resume(
        store: Store,
        cache: NativeCache,
        docs: &Docs,
        mirror: MirrorBlobs,
        device: Keypair,
        endpoint: Endpoint,
    ) -> Result<Arc<Self>> {
        let source = store.clone();
        let binding = tokio::task::spawn_blocking(move || source.social_document())
            .await
            .map_err(|e| transport_error("read social mirror binding", e))??;
        let doc = if let Some(binding) = binding {
            let capability: Capability =
                serde_json::from_slice(&binding.capability).map_err(|_| SyncError::InvalidEntry)?;
            if capability.id().as_bytes() != &binding.namespace {
                return Err(SyncError::InvalidEntry);
            }
            docs.import_namespace(capability)
                .await
                .map_err(|e| transport_error("open social mirror", e))?
        } else {
            docs.create()
                .await
                .map_err(|e| transport_error("create social mirror", e))?
        };
        doc.set_download_policy(DownloadPolicy::NothingExcept(vec![]))
            .await
            .map_err(|e| transport_error("disable social mirror downloads", e))?;
        let ticket = doc
            .share(
                iroh_docs::api::protocol::ShareMode::Write,
                iroh_docs::api::protocol::AddrInfoOptions::Id,
            )
            .await
            .map_err(|e| transport_error("retain social mirror binding", e))?;
        let binding = SocialDocument {
            namespace: *ticket.capability.id().as_bytes(),
            capability: serde_json::to_vec(&ticket.capability)
                .map_err(|_| SyncError::InvalidEntry)?,
        };
        let source = store.clone();
        tokio::task::spawn_blocking(move || source.bind_social_document(&binding))
            .await
            .map_err(|e| transport_error("bind social mirror", e))??;
        let author = AuthorId::from(device.public().as_bytes());
        Ok(Arc::new(Self {
            store,
            cache,
            doc,
            mirror,
            author,
            device: Arc::new(device),
            endpoint,
            requests: tokio::sync::Semaphore::new(2),
            cache_cursors: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }))
    }
    /// Public connection information signed by the certified endpoint device.
    pub fn contact_card(&self) -> Result<ContactCard> {
        Ok(ContactCard::sign(
            self.store.social_local_certificate(self.device.public())?,
            serde_json::to_string(&self.endpoint.addr()).map_err(|_| SyncError::InvalidEntry)?,
            &self.device,
        )?)
    }
    /// Publish one bounded history page; cache loss is repaired by cycling history.
    pub async fn publish(&self, after: Option<String>) -> Result<Option<String>> {
        self.promote_received_dependencies().await?;
        let source = self.store.clone();
        let device = self.device.clone();
        tokio::task::spawn_blocking(move || source.publish_current_social_profiles(&device))
            .await
            .map_err(|e| transport_error("prepare social publication", e))??;
        let source = self.store.clone();
        let records = tokio::task::spawn_blocking(move || {
            source.social_publication_history(after.as_deref(), 50)
        })
        .await
        .map_err(|e| transport_error("read social publication", e))??;
        let mut next = None;
        for fact in records {
            let portable = fact.key()?;
            next = Some(portable.clone());
            let source = self.store.clone();
            let candidate = fact.clone();
            if !tokio::task::spawn_blocking(move || source.social_record_servable(&candidate))
                .await
                .map_err(|e| transport_error("check social publication consent", e))??
            {
                continue;
            }
            let bytes = fact.to_bytes()?;
            for recipient in fact.recipients() {
                let key = mirror_key(recipient, &portable);
                let known = self
                    .doc
                    .get_exact(self.author, key.as_bytes(), false)
                    .await
                    .map_err(|e| transport_error("read social mirror header", e))?;
                if let Some(entry) = known
                    && entry.content_len() == bytes.len() as u64
                    && let Ok(stored) = self.mirror.get_bytes(entry.content_hash()).await
                    && stored.as_ref() == bytes.as_slice()
                {
                    continue;
                }
                self.doc
                    .set_bytes(self.author, key, bytes.clone())
                    .await
                    .map_err(|e| transport_error("mirror signed share", e))?;
            }
        }
        Ok(next)
    }
    async fn records_for(
        &self,
        recipient: PublicKey,
        after: Option<String>,
    ) -> Result<(Vec<ShareFact>, Option<String>)> {
        let source = self.store.clone();
        tokio::task::spawn_blocking(move || source.authorize_social_recipient(&recipient))
            .await
            .map_err(|e| transport_error("authorize share recipient", e))??;
        self.promote_received_dependencies().await?;
        if after.as_ref().is_some_and(|key| key.len() > 256) {
            return Err(SyncError::InvalidEntry);
        }
        let prefix = mirror_key(recipient, "");
        let mut entries = pin!(
            self.doc
                .get_many(
                    Query::author(self.author)
                        .key_prefix(prefix.as_bytes())
                        .sort_by(SortBy::KeyAuthor, SortDirection::Asc)
                )
                .await
                .map_err(|e| transport_error("read recipient mirror", e))?
        );
        let local = self
            .store
            .self_identity()?
            .ok_or(SyncError::IdentityUnbound)?
            .master_pubkey;
        let mut found = BTreeMap::new();
        while let Some(entry) = poll_fn(|cx| entries.as_mut().poll_next(cx)).await {
            let entry = entry.map_err(|e| transport_error("read recipient header", e))?;
            if entry.content_len() > MAX_SHARE_BYTES as u64 {
                return Err(SyncError::InvalidEntry);
            }
            let key = std::str::from_utf8(entry.key())
                .map_err(|_| SyncError::InvalidEntry)?
                .strip_prefix(&prefix)
                .ok_or(SyncError::InvalidEntry)?
                .to_owned();
            if after.as_ref().is_some_and(|after| key <= *after) {
                continue;
            }
            let bytes = self
                .mirror
                .get_bytes(entry.content_hash())
                .await
                .map_err(|e| transport_error("read mirrored share", e))?;
            if bytes.len() as u64 != entry.content_len() {
                return Err(SyncError::BlobIntegrity);
            }
            let fact = ShareFact::from_bytes(&bytes)?;
            if (!fact.recipients().contains(&local) && fact.author() != local)
                || !fact.recipients().contains(&recipient)
                || fact.key()? != key
            {
                return Err(SyncError::InvalidEntry);
            }
            // A mirror entry may predate Leave or a resolved private opt-out.
            // Cozo owns the current permission even when these bytes are cached.
            let source = self.store.clone();
            let candidate = fact.clone();
            if !tokio::task::spawn_blocking(move || source.social_record_servable(&candidate))
                .await
                .map_err(|e| transport_error("check mirrored conversation consent", e))??
            {
                continue;
            }
            found.insert(key, fact);
            // Select a bounded page in SDK key order without reading all bodies.
            if found.len() >= 10 {
                break;
            }
        }
        let mut records = Vec::new();
        let mut bytes = 0;
        let mut next = None;
        for (key, fact) in found {
            let size = fact.to_bytes()?.len();
            if !records.is_empty() && bytes + size > MAX_SHARE_BYTES {
                break;
            }
            bytes += size;
            records.push(fact);
            next = Some(key);
        }
        Ok((records, next))
    }
    async fn serve(&self, connection: &Connection) -> Result<()> {
        let _permit = self
            .requests
            .try_acquire()
            .map_err(|_| SyncError::DeviceUnauthorized)?;
        connection.set_max_concurrent_bi_streams(VarInt::from_u32(1));
        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(|e| transport_error("accept share request", e))?;
        let bytes = recv
            .read_to_end(MAX_REQUEST)
            .await
            .map_err(|_| SyncError::InvalidEntry)?;
        let request: Request = decode(&bytes)?;
        let address = contact_address(&request.card)?;
        if address.id != connection.remote_id() {
            return Err(SyncError::DeviceUnauthorized);
        }
        let recipient = request.card.certificate.master_pubkey;
        let source = self.store.clone();
        let card = request.card;
        tokio::task::spawn_blocking(move || {
            source.authorize_social_recipient(&recipient)?;
            source.complete_social_contact(&card)
        })
        .await
        .map_err(|e| transport_error("verify recipient connection", e))??;
        match request.operation {
            Operation::Records { after } => {
                let (records, after) = self.records_for(recipient, after).await?;
                let response = canonical_bytes(&Response::Records {
                    card: Box::new(self.contact_card()?),
                    records,
                    after,
                })
                .map_err(|_| SyncError::InvalidEntry)?;
                if response.len() > MAX_RESPONSE {
                    return Err(SyncError::InvalidEntry);
                }
                send.write_all(&response)
                    .await
                    .map_err(|e| transport_error("send signed share page", e))?;
            }
            Operation::Blob { share_id, blob_id } => {
                if share_id.len() != 64 || blob_id.len() != 64 {
                    return Err(SyncError::InvalidEntry);
                }
                let source = self.store.clone();
                let id = share_id.clone();
                let digest = blob_id.clone();
                let reference = tokio::task::spawn_blocking(move || {
                    source.authorized_social_blob(&recipient, &id, &digest)
                })
                .await
                .map_err(|e| transport_error("authorize shared blob", e))??;
                let cache = self.cache.clone();
                let id = blob_id.parse::<BlobId>()?;
                let bytes = tokio::task::spawn_blocking(move || cache.get_blocking(&id))
                    .await
                    .map_err(|e| transport_error("read shared cache", e))??;
                if bytes.len() as u64 != reference.byte_size || BlobId::from_bytes(&bytes) != id {
                    return Err(SyncError::BlobIntegrity);
                }
                // Raw bytes follow a one-byte success marker; no generic blob API.
                send.write_all(&[1])
                    .await
                    .map_err(|e| transport_error("send shared blob status", e))?;
                send.write_all(&bytes)
                    .await
                    .map_err(|e| transport_error("send shared blob", e))?;
            }
        }
        send.finish()
            .map_err(|e| transport_error("finish share response", e))?;
        let _ = send.stopped().await;
        Ok(())
    }
    async fn request(
        &self,
        address: EndpointAddr,
        operation: Operation,
        max: usize,
    ) -> Result<Vec<u8>> {
        tokio::time::timeout(DEADLINE, async {
            let connection = self
                .endpoint
                .connect(address, ALPN)
                .await
                .map_err(|e| transport_error("connect to followed person", e))?;
            let (mut send, mut recv) = connection
                .open_bi()
                .await
                .map_err(|e| transport_error("open sharing request", e))?;
            let bytes = canonical_bytes(&Request {
                card: self.contact_card()?,
                operation,
            })
            .map_err(|_| SyncError::InvalidEntry)?;
            if bytes.len() > MAX_REQUEST {
                return Err(SyncError::InvalidEntry);
            }
            send.write_all(&bytes)
                .await
                .map_err(|e| transport_error("send sharing request", e))?;
            send.finish()
                .map_err(|e| transport_error("finish sharing request", e))?;
            let bytes = recv
                .read_to_end(max)
                .await
                .map_err(|_| SyncError::InvalidEntry)?;
            connection.close(VarInt::from_u32(0), b"sharing exchange complete");
            Ok(bytes)
        })
        .await
        .map_err(|_| transport_error("sharing exchange", "timed out"))?
    }
    /// Receive a bounded verified page from one explicitly followed master.
    pub async fn receive(
        &self,
        card: &ContactCard,
        after: Option<String>,
    ) -> Result<(Option<String>, bool)> {
        let expected = card.certificate.master_pubkey;
        let bytes = self
            .request(
                contact_address(card)?,
                Operation::Records { after },
                MAX_RESPONSE,
            )
            .await?;
        let Response::Records {
            card,
            records,
            after,
        } = decode(&bytes)?
        else {
            return Err(SyncError::DeviceUnauthorized);
        };
        if card.certificate.master_pubkey != expected {
            return Err(SyncError::DeviceUnauthorized);
        }
        if records.len() > 10 || after.as_ref().is_some_and(|key| key.len() > 256) {
            return Err(SyncError::InvalidEntry);
        }
        contact_address(&card)?;
        self.store.complete_social_contact(&card)?;
        // Finish durable peer receipts before an earlier reply can consume the
        // remaining queue space. This never admits history-only self replay.
        let mut changed = self.promote_received_dependencies().await?;
        for fact in records {
            let source = self.store.clone();
            let device = self.device.clone();
            let result = tokio::task::spawn_blocking(move || {
                source.accept_social_record(&fact, &expected, now_micros(), &device)
            })
            .await
            .map_err(|e| transport_error("accept signed share", e))??;
            // Deferred candidates stay on the sender. Advance through this page
            // to reach later roots/parents, then retry them on the next full scan.
            changed |= result == SocialReceive::Applied;
        }
        changed |= self.promote_received_dependencies().await?;
        if changed {
            self.cache_cursors
                .lock()
                .expect("shared cache cursor")
                .remove(&expected);
        }
        Ok((after, changed))
    }

    async fn promote_received_dependencies(&self) -> Result<bool> {
        let source = self.store.clone();
        let device = self.device.clone();
        let changed = tokio::task::spawn_blocking(move || {
            let mut changed = !source.promote_pending_state()?.applied.is_empty();
            changed |= source.adopt_legacy_social_conversations(now_micros(), &device)? > 0;
            changed |= source.admit_pending_social_roots(now_micros(), &device)? > 0;
            changed |= source.promote_pending_social()?;
            carrel_store::Result::Ok(changed)
        })
        .await
        .map_err(|e| transport_error("promote conversation dependencies", e))??;
        Ok(changed)
    }
    /// Transfer even already-known digests: the source must possess this share's bytes.
    pub async fn transfer_cache(&self, card: &ContactCard) -> Result<SocialCacheTransfer> {
        let author = card.certificate.master_pubkey;
        let after = self
            .cache_cursors
            .lock()
            .expect("shared cache cursor")
            .remove(&author);
        let source = self.store.clone();
        let pending = tokio::task::spawn_blocking(move || {
            let page = source.pending_social_blobs_after(&author, 5, after.as_ref())?;
            if page.is_empty() && after.is_some() {
                source.pending_social_blobs(&author, 5)
            } else {
                Ok(page)
            }
        })
        .await
        .map_err(|e| transport_error("read shared cache queue", e))??;
        if let Some(last) = pending.last() {
            self.cache_cursors
                .lock()
                .expect("shared cache cursor")
                .insert(author, last.clone());
        }
        let mut result = SocialCacheTransfer::default();
        for blob in pending {
            match self.transfer_blob(card, blob).await {
                Ok(()) => result.changed = true,
                Err(error) => result.errors.push(error),
            }
        }
        Ok(result)
    }
    async fn transfer_blob(
        &self,
        card: &ContactCard,
        blob: carrel_store::sharing::PendingShareBlob,
    ) -> Result<()> {
        let expected = blob.reference.byte_size as usize;
        let bytes = self
            .request(
                contact_address(card)?,
                Operation::Blob {
                    share_id: blob.share_id.clone(),
                    blob_id: blob.reference.blob_id.clone(),
                },
                expected + 1,
            )
            .await?;
        if bytes.first() != Some(&1) || bytes.len() != expected + 1 {
            return Err(SyncError::BlobIntegrity);
        }
        let id = blob.reference.blob_id.parse::<BlobId>()?;
        if BlobId::from_bytes(&bytes[1..]) != id {
            return Err(SyncError::BlobIntegrity);
        }
        let cache = self.cache.clone();
        let content = bytes[1..].to_vec();
        tokio::task::spawn_blocking(move || cache.put_blocking(&content))
            .await
            .map_err(|e| transport_error("install shared cache", e))??;
        let source = self.store.clone();
        tokio::task::spawn_blocking(move || {
            source.complete_social_blob(&blob.share_id, &blob.reference.blob_id)
        })
        .await
        .map_err(|e| transport_error("complete shared cache grant", e))??;
        Ok(())
    }
    /// Manually followed contacts with connection information, never discovered peers.
    pub fn contacts(&self) -> Result<Vec<ContactCard>> {
        Ok(self
            .store
            .social_contacts()?
            .into_iter()
            .filter(|contact| contact.followed)
            .filter_map(|contact| contact.card)
            .collect())
    }
    /// Close the private mirror handle during application shutdown.
    pub async fn close(&self) {
        let _ = self.doc.close().await;
    }
}
impl ProtocolHandler for SocialBridge {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        if !matches!(
            tokio::time::timeout(DEADLINE, self.serve(&connection)).await,
            Ok(Ok(()))
        ) {
            connection.close(VarInt::from_u32(403), b"sharing request denied");
        }
        Ok(())
    }
}
/// Validate public address information against its signed device identity.
pub fn contact_address(card: &ContactCard) -> Result<EndpointAddr> {
    card.verify()?;
    let address: EndpointAddr =
        serde_json::from_str(&card.address).map_err(|_| SyncError::InvalidEntry)?;
    if address.id.as_bytes() != card.certificate.device_pubkey.as_bytes() {
        return Err(SyncError::DeviceKeyMismatch);
    }
    Ok(address)
}
fn mirror_key(recipient: PublicKey, key: &str) -> String {
    format!("recipient/{}/{key}", recipient.to_hex())
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let mut cursor = std::io::Cursor::new(bytes);
    let value = ciborium::from_reader(&mut cursor).map_err(|_| SyncError::InvalidEntry)?;
    if cursor.position() != bytes.len() as u64 {
        return Err(SyncError::InvalidEntry);
    }
    Ok(value)
}
fn now_micros() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64
}

#[cfg(test)]
mod tests;
