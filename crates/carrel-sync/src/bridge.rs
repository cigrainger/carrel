//! Private Cozo facts mirrored into Iroh; incoming bytes never become raw SQL.

use std::{future::poll_fn, pin::pin, sync::Arc, time::Duration};

use carrel_core::identity::PublicKey;
use carrel_core::private_library::{
    BlobReference, MAX_PRIVATE_FACT_BYTES, MAX_READING_BLOB_BYTES, PrivateFact,
};
use carrel_store::{
    Store,
    blobs::{BlobId, BlobStore},
    private_library::PrivateReceive,
};
use futures_core::Stream;
use iroh::EndpointAddr;
use iroh_blobs::{
    Hash,
    get::fsm::{AtInitial, ConnectedNext, EndBlobNext, RequestCounters},
    protocol::GetRequest,
};
use iroh_docs::{
    AuthorId, Capability, Entry, NamespaceId,
    api::Doc,
    store::{DownloadPolicy, Query},
};

use crate::{Result, SyncError, error::transport_error, transport::SelfTransport};

const TRANSFER_DEADLINE: Duration = Duration::from_secs(30);

/// One bounded publication page; failures retain their exact durable queue keys.
#[derive(Debug, Default)]
pub struct Publication {
    /// Continue after this key; reset when the next page is empty.
    pub after: Option<String>,
    /// Whether anything was newly exposed in the mirror.
    pub changed: bool,
    /// Metadata-only errors; callers can retry without discarding other work.
    pub errors: Vec<SyncError>,
}

/// Result of one native-cache download page.
#[derive(Debug, Default)]
pub struct CacheTransfer {
    /// Continue after this digest; reset after an empty page.
    pub after: Option<String>,
    /// Fresh cache content needs derived local search/reader refresh.
    pub changed: bool,
    /// Failed references stay durable for retry.
    pub errors: Vec<SyncError>,
}

/// An authorized private document coupled to its source-of-truth store and cache.
pub struct SelfBridge {
    pub(crate) store: Store,
    cache: BlobStore,
    pub(crate) host: Arc<SelfTransport>,
    pub(crate) doc: Doc,
    author: AuthorId,
}

impl SelfBridge {
    /// The separately filtered social service, if enabled by the native shell.
    pub fn sharing(&self) -> Option<Arc<crate::sharing::SocialBridge>> {
        self.host.sharing()
    }
    /// Recover the bound document even when the expendable Iroh cache was lost.
    pub async fn resume(store: Store, cache: BlobStore, host: Arc<SelfTransport>) -> Result<Self> {
        let source = store.clone();
        let binding = tokio::task::spawn_blocking(move || source.private_document())
            .await
            .map_err(|error| transport_error("read private document binding", error))??;
        if let Some(binding) = binding {
            let capability: Capability =
                serde_json::from_slice(&binding.capability).map_err(|_| SyncError::InvalidEntry)?;
            let namespace = NamespaceId::from(binding.namespace);
            if capability.id() != namespace {
                return Err(SyncError::InvalidEntry);
            }
            return Self::import(store, cache, host, capability).await;
        }
        Self::create(store, cache, host).await
    }
    /// Create a fresh independent document capability, never from the Root seed.
    pub async fn create(store: Store, cache: BlobStore, host: Arc<SelfTransport>) -> Result<Self> {
        let doc = host
            .docs()
            .create()
            .await
            .map_err(|error| transport_error("create private document", error))?;
        Self::from_doc(store, cache, host, doc).await
    }

    /// Import a granted capability without joining peers or automatic downloads.
    pub async fn import(
        store: Store,
        cache: BlobStore,
        host: Arc<SelfTransport>,
        capability: Capability,
    ) -> Result<Self> {
        let doc = host
            .docs()
            .import_namespace(capability)
            .await
            .map_err(|error| transport_error("import private document", error))?;
        Self::from_doc(store, cache, host, doc).await
    }

    /// Reopen the explicitly bound document after restart.
    pub async fn open(
        store: Store,
        cache: BlobStore,
        host: Arc<SelfTransport>,
        namespace: NamespaceId,
    ) -> Result<Self> {
        let doc = host
            .docs()
            .open(namespace)
            .await
            .map_err(|error| transport_error("open private document", error))?
            .ok_or(SyncError::InvalidEntry)?;
        Self::from_doc(store, cache, host, doc).await
    }

    async fn from_doc(
        store: Store,
        cache: BlobStore,
        host: Arc<SelfTransport>,
        doc: Doc,
    ) -> Result<Self> {
        // SDK defaults fetch every content body. Do this before joining any
        // peer so unverified headers cannot trigger unbounded private downloads.
        doc.set_download_policy(DownloadPolicy::NothingExcept(vec![]))
            .await
            .map_err(|error| transport_error("disable automatic private downloads", error))?;
        let local_store = store.clone();
        let binding = tokio::task::spawn_blocking(move || local_store.self_identity())
            .await
            .map_err(|error| transport_error("read private binding", error))??
            .ok_or(SyncError::IdentityUnbound)?;
        if binding.device_pubkey.as_bytes() != host.endpoint().id().as_bytes() {
            return Err(SyncError::DeviceKeyMismatch);
        }
        host.refresh_authorization().await?;
        let ticket = doc
            .share(
                iroh_docs::api::protocol::ShareMode::Write,
                iroh_docs::api::protocol::AddrInfoOptions::Id,
            )
            .await
            .map_err(|error| transport_error("retain private document binding", error))?;
        let document = carrel_store::private_transport::PrivateDocument {
            namespace: *doc.id().as_bytes(),
            capability: serde_json::to_vec(&ticket.capability)
                .map_err(|_| SyncError::InvalidEntry)?,
        };
        let source = store.clone();
        if let Err(error) =
            tokio::task::spawn_blocking(move || source.bind_private_document(&document))
                .await
                .map_err(|error| transport_error("bind private document", error))?
        {
            let _ = doc.close().await;
            return Err(error.into());
        }
        Ok(Self {
            store,
            cache,
            host,
            doc,
            author: AuthorId::from(*binding.device_pubkey.as_bytes()),
        })
    }

    /// Native orchestration subscribes to SDK events, while this bridge owns writes.
    pub fn document(&self) -> &Doc {
        &self.doc
    }

    /// Address of the certified private endpoint, separate from invitation endpoints.
    pub fn address(&self) -> EndpointAddr {
        self.host.address()
    }

    /// Apply device-management authorization changes to open private connections.
    pub async fn refresh_authorization(&self) -> Result<()> {
        self.host.refresh_authorization().await
    }

    /// Publish a bounded page, keeping incomplete cache work available for retry.
    pub async fn publish(&self, after: Option<String>) -> Result<Publication> {
        let rows = self
            .store_call(move |store| store.private_fact_outbox(after.as_deref(), 100))
            .await?;
        let mut result = Publication::default();
        for (key, fact) in rows {
            result.after = Some(key.clone());
            match self.publish_fact(&fact).await {
                Ok((changed, complete)) => {
                    result.changed |= changed;
                    if complete {
                        self.store_call(move |store| store.acknowledge_private_fact(&key))
                            .await?;
                    }
                }
                Err(error) => result.errors.push(error),
            }
        }
        Ok(result)
    }

    async fn publish_fact(&self, fact: &PrivateFact) -> Result<(bool, bool)> {
        let owned = fact.clone();
        if !self
            .store_call(move |store| store.private_fact_authorized(&owned))
            .await?
        {
            return Err(SyncError::DeviceUnauthorized);
        }
        let mut complete = true;
        let mut changed = false;
        if let PrivateFact::Item(record) = fact {
            for reference in record.item.blobs() {
                let id = parse_blob(&reference)?;
                let key = format!("blob/{}", reference.blob_id);
                let hash = Hash::from_bytes(*id.as_bytes());
                let known = self
                    .doc
                    .get_exact(self.author, key.as_bytes(), false)
                    .await
                    .map_err(|error| transport_error("read mirrored resource", error))?;
                if known.is_some_and(|entry| {
                    entry.content_hash() == hash && entry.content_len() == reference.byte_size
                }) && let Ok(bytes) = self.host.blobs().get_bytes(hash).await
                    && verify_blob(&bytes, hash, reference.byte_size).is_ok()
                {
                    continue;
                }
                let cache = self.cache.clone();
                let bytes = tokio::task::spawn_blocking(move || cache.get_blocking(&id))
                    .await
                    .map_err(|error| transport_error("read native cache", error))?;
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(carrel_store::blobs::BlobError::NotFound { .. }) => {
                        complete = false;
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                };
                verify_blob(&bytes, hash, reference.byte_size)?;
                let tag = self
                    .host
                    .blobs()
                    .add_slice(&bytes)
                    .await
                    .map_err(|error| transport_error("mirror cached resource", error))?;
                if tag.hash != hash {
                    return Err(SyncError::BlobIntegrity);
                }
                self.doc
                    .set_hash(self.author, key, hash, reference.byte_size)
                    .await
                    .map_err(|error| transport_error("publish cached resource", error))?;
                changed = true;
            }
        }
        // Metadata and independent state still travel when a reading cache is
        // unavailable. The original queue record remains until content is ready.
        let bytes = fact.to_bytes()?;
        let hash = Hash::from_bytes(*BlobId::from_bytes(&bytes).as_bytes());
        let key = format!("fact/{}", fact.key()?);
        let known = self
            .doc
            .get_exact(self.author, key.as_bytes(), false)
            .await
            .map_err(|error| transport_error("read mirrored fact", error))?;
        let retained = self
            .host
            .blobs()
            .get_bytes(hash)
            .await
            .is_ok_and(|cached| verify_blob(&cached, hash, bytes.len() as u64).is_ok());
        if !retained
            || !known.is_some_and(|entry| {
                entry.content_hash() == hash && entry.content_len() == bytes.len() as u64
            })
        {
            self.doc
                .set_bytes(self.author, key, bytes)
                .await
                .map_err(|error| transport_error("publish private fact", error))?;
            changed = true;
        }
        Ok((changed, complete))
    }

    /// Replay retained SDK entries once after startup or a new authorization.
    /// Every author is considered; an invalid writer cannot shadow a valid one.
    pub async fn replay(&self, provider: EndpointAddr) -> Result<Vec<SyncError>> {
        let mut errors = Vec::new();
        let sender = PublicKey::from_bytes(*self.host.endpoint().id().as_bytes());
        for prefix in ["fact/certificate/", "fact/revocation/", "fact/"] {
            let entries = self
                .doc
                .get_many(Query::all().key_prefix(prefix))
                .await
                .map_err(|error| transport_error("replay private headers", error))?;
            let mut entries = pin!(entries);
            while let Some(entry) = poll_fn(|cx| entries.as_mut().poll_next(cx)).await {
                let entry = entry.map_err(|error| transport_error("read private header", error))?;
                if let Err(error) = self.receive_as(&entry, provider.clone(), sender).await {
                    errors.push(error);
                }
            }
        }
        self.promote_dependencies().await?;
        Ok(errors)
    }

    /// Validate a verified SDK header before requesting any of its fact bytes.
    pub async fn receive(&self, entry: &Entry, provider: EndpointAddr) -> Result<PrivateReceive> {
        let sender = PublicKey::from_bytes(*provider.id.as_bytes());
        self.receive_as(entry, provider, sender).await
    }

    async fn receive_as(
        &self,
        entry: &Entry,
        provider: EndpointAddr,
        sender: PublicKey,
    ) -> Result<PrivateReceive> {
        if entry.namespace() != self.doc.id()
            || entry.content_len() == 0
            || entry.content_len() > MAX_PRIVATE_FACT_BYTES as u64
        {
            return Err(SyncError::InvalidEntry);
        }
        let key = std::str::from_utf8(entry.key()).map_err(|_| SyncError::InvalidEntry)?;
        let Some(portable_key) = key.strip_prefix("fact/") else {
            return Err(SyncError::InvalidEntry);
        };
        validate_fact_key(portable_key)?;
        let header_author = PublicKey::from_bytes(*entry.author().as_bytes());
        let devices = self.store_call(|store| store.self_devices()).await?;
        if !devices
            .iter()
            .any(|device| device.certificate.device_pubkey == header_author)
        {
            return Err(SyncError::DeviceUnauthorized);
        }
        let active_sender = devices
            .iter()
            .any(|device| device.certificate.device_pubkey == sender && device.is_authorized());
        if !active_sender {
            return Err(SyncError::DeviceUnauthorized);
        }
        let bytes = self.fact_bytes(entry, provider).await?;
        let fact = PrivateFact::from_bytes(&bytes)?;
        if fact.key()? != portable_key {
            return Err(SyncError::InvalidEntry);
        }
        self.store_call(move |store| store.accept_self_private_fact(&fact, &sender))
            .await
    }

    async fn fact_bytes(&self, entry: &Entry, provider: EndpointAddr) -> Result<Vec<u8>> {
        if let Ok(bytes) = self.host.blobs().get_bytes(entry.content_hash()).await
            && verify_blob(&bytes, entry.content_hash(), entry.content_len()).is_ok()
        {
            return Ok(bytes.to_vec());
        }
        let bytes = self
            .download(
                provider,
                entry.content_hash(),
                entry.content_len(),
                MAX_PRIVATE_FACT_BYTES as u64,
            )
            .await?;
        self.host
            .blobs()
            .add_slice(&bytes)
            .await
            .map_err(|error| transport_error("retain verified private bytes", error))?;
        Ok(bytes)
    }

    /// Complete only declared cache references, without delaying other hashes.
    pub async fn transfer_cache(
        &self,
        after: Option<String>,
        providers: &[EndpointAddr],
    ) -> Result<CacheTransfer> {
        let references = self
            .store_call(move |store| store.private_blob_incoming(after.as_deref(), 100))
            .await?;
        let mut result = CacheTransfer::default();
        for reference in references {
            result.after = Some(reference.blob_id.clone());
            match self.transfer_blob(&reference, providers).await {
                Ok(changed) => {
                    result.changed |= changed;
                    self.store_call(move |store| store.acknowledge_private_blob(&reference))
                        .await?;
                }
                Err(error) => result.errors.push(error),
            }
        }
        Ok(result)
    }

    async fn transfer_blob(
        &self,
        reference: &BlobReference,
        providers: &[EndpointAddr],
    ) -> Result<bool> {
        let id = parse_blob(reference)?;
        let hash = Hash::from_bytes(*id.as_bytes());
        let cache = self.cache.clone();
        if let Ok(bytes) = tokio::task::spawn_blocking(move || cache.get_blocking(&id))
            .await
            .map_err(|error| transport_error("inspect native cache", error))?
            && verify_blob(&bytes, hash, reference.byte_size).is_ok()
        {
            return Ok(false);
        }
        let mut failure = SyncError::BlobIntegrity;
        for provider in providers {
            let sender = PublicKey::from_bytes(*provider.id.as_bytes());
            if !self
                .store_call(move |store| store.self_device_authorized(&sender))
                .await?
            {
                continue;
            }
            match self
                .download(
                    provider.clone(),
                    hash,
                    reference.byte_size,
                    MAX_READING_BLOB_BYTES,
                )
                .await
            {
                Ok(bytes) => {
                    let cache = self.cache.clone();
                    let installed = tokio::task::spawn_blocking(move || cache.put_blocking(&bytes))
                        .await
                        .map_err(|error| transport_error("install native cache", error))??;
                    if installed != id {
                        return Err(SyncError::BlobIntegrity);
                    }
                    return Ok(true);
                }
                Err(error) => failure = error,
            }
        }
        Err(failure)
    }

    async fn download(
        &self,
        provider: EndpointAddr,
        hash: Hash,
        expected: u64,
        maximum: u64,
    ) -> Result<Vec<u8>> {
        if expected > maximum {
            return Err(SyncError::BlobIntegrity);
        }
        tokio::time::timeout(TRANSFER_DEADLINE, async {
            let connection = self
                .host
                .endpoint()
                .connect(provider, iroh_blobs::ALPN)
                .await
                .map_err(|error| transport_error("connect private content peer", error))?;
            let connected = AtInitial::new(
                connection,
                GetRequest::blob(hash),
                RequestCounters::default(),
            )
            .next()
            .await
            .map_err(|error| transport_error("request private content", error))?;
            let ConnectedNext::StartRoot(root) = connected
                .next()
                .await
                .map_err(|error| transport_error("read private content response", error))?
            else {
                return Err(SyncError::BlobIntegrity);
            };
            let (content, size) = root
                .next()
                .next()
                .await
                .map_err(|error| transport_error("read private content size", error))?;
            // Inspect the header before any body allocation or filesystem write.
            // A peer cannot turn a 1 KiB signed reference into a multi-TiB fetch.
            if size != expected || size > maximum {
                return Err(SyncError::BlobIntegrity);
            }
            let (end, bytes) = content
                .concatenate_into_vec()
                .await
                .map_err(|error| transport_error("verify private content proof", error))?;
            let EndBlobNext::Closing(closing) = end.next() else {
                return Err(SyncError::BlobIntegrity);
            };
            closing
                .next()
                .await
                .map_err(|error| transport_error("finish private content", error))?;
            verify_blob(&bytes, hash, expected)?;
            Ok(bytes)
        })
        .await
        .map_err(|error| transport_error("private content transfer deadline", error))?
    }

    /// Targets and ancestry can become ready independently of SDK delivery order.
    pub async fn promote_dependencies(&self) -> Result<bool> {
        self.store_call(|store| {
            let mut changed = false;
            loop {
                let applied = store.promote_pending_items()?;
                if applied.is_empty() {
                    break;
                }
                changed = true;
            }
            loop {
                let applied = store.promote_pending_highlights()?;
                if applied.is_empty() {
                    break;
                }
                changed = true;
            }
            loop {
                let result = store.promote_pending_notes()?;
                if result.applied.is_empty() {
                    break;
                }
                changed = true;
            }
            loop {
                let result = store.promote_pending_state()?;
                if result.applied.is_empty() {
                    break;
                }
                changed = true;
            }
            Ok(changed)
        })
        .await
    }

    async fn store_call<T: Send + 'static>(
        &self,
        call: impl FnOnce(Store) -> carrel_store::Result<T> + Send + 'static,
    ) -> Result<T> {
        let store = self.store.clone();
        Ok(tokio::task::spawn_blocking(move || call(store))
            .await
            .map_err(|error| transport_error("private store worker", error))??)
    }
}

fn parse_blob(reference: &BlobReference) -> Result<BlobId> {
    if reference.byte_size > MAX_READING_BLOB_BYTES {
        return Err(SyncError::BlobIntegrity);
    }
    reference.blob_id.parse().map_err(SyncError::Blob)
}
fn verify_blob(bytes: &[u8], hash: Hash, expected: u64) -> Result<()> {
    if bytes.len() as u64 != expected || BlobId::from_bytes(bytes).as_bytes() != hash.as_bytes() {
        return Err(SyncError::BlobIntegrity);
    }
    Ok(())
}
fn validate_fact_key(key: &str) -> Result<()> {
    let Some((kind, digest)) = key.split_once('/') else {
        return Err(SyncError::InvalidEntry);
    };
    if !matches!(
        kind,
        "item" | "state" | "note" | "highlight" | "removal" | "certificate" | "revocation"
    ) || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SyncError::InvalidEntry);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
