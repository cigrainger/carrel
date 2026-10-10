//! Persistent private docs, blobs, and gossip on one authorized Iroh endpoint.

use std::path::Path;

use carrel_core::identity::Keypair;
use carrel_store::Store;
use iroh::{
    Endpoint, EndpointAddr, SecretKey,
    endpoint::{Builder, presets},
    protocol::Router,
};
use iroh_blobs::{BlobsProtocol, api::Store as BlobStore, store::fs::FsStore};
use iroh_docs::{Author, protocol::Docs};
use iroh_gossip::net::Gossip;
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

use crate::Result;
use crate::authorization::AuthorizationGate;
use crate::error::transport_error;

/// Hosts private protocols for one explicitly bound and authorized installation.
///
/// Protocol storage is an asynchronous mirror/cache. Callers still verify each
/// incoming fact and write it through the source-of-truth store's typed APIs.
pub struct SelfTransport {
    router: Router,
    docs: Docs,
    blobs: BlobStore,
    authorization: AuthorizationGate,
    authorization_task: JoinHandle<()>,
}

impl SelfTransport {
    /// Start the private endpoint using the existing independent device key.
    ///
    /// `cache_dir` must be a dedicated sync-cache directory. Protocol stores are
    /// persistent; access is checked independently of doc/blob capabilities.
    pub async fn start(store: Store, cache_dir: &Path, device: &Keypair) -> Result<Self> {
        Self::start_with_builder(store, cache_dir, device, Endpoint::builder(presets::N0)).await
    }

    async fn start_with_builder(
        store: Store,
        cache_dir: &Path,
        device: &Keypair,
        builder: Builder,
    ) -> Result<Self> {
        let gate = AuthorizationGate::new(store.clone(), *device.public());
        let mut identity_changes = store.subscribe_identity_changes();
        gate.validate_local().await?;
        tokio::fs::create_dir_all(cache_dir)
            .await
            .map_err(|error| transport_error("create sync cache", error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(cache_dir, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(|error| transport_error("protect sync cache", error))?;
        }
        let docs_dir = cache_dir.join("docs");
        tokio::fs::create_dir_all(&docs_dir)
            .await
            .map_err(|error| transport_error("create docs cache", error))?;
        let blobs = FsStore::load(cache_dir.join("blobs"))
            .await
            .map_err(|error| transport_error("open blobs mirror", error))?;
        let seed = Zeroizing::new(device.to_seed_bytes());
        let endpoint = match builder
            .secret_key(SecretKey::from_bytes(&seed))
            .hooks(gate.clone())
            .bind()
            .await
        {
            Ok(endpoint) => endpoint,
            Err(error) => {
                let _ = blobs.shutdown().await;
                return Err(transport_error("bind private endpoint", error));
            }
        };
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let docs = match Docs::persistent(docs_dir)
            .spawn(endpoint.clone(), (*blobs).clone(), gossip.clone())
            .await
        {
            Ok(docs) => docs,
            Err(error) => {
                let _ = gossip.shutdown().await;
                endpoint.close().await;
                let _ = blobs.shutdown().await;
                return Err(transport_error("open docs mirror", error));
            }
        };
        let author = Author::from_bytes(&seed);
        let author_id = author.id();
        let author_setup = async {
            docs.author_import(author).await?;
            docs.author_set_default(author_id).await
        }
        .await;
        if let Err(error) = author_setup {
            iroh::protocol::ProtocolHandler::shutdown(&docs).await;
            let _ = gossip.shutdown().await;
            let _ = blobs.shutdown().await;
            endpoint.close().await;
            return Err(transport_error("bind private mirror author", error));
        }
        let watcher_gate = gate.clone();
        let watcher_endpoint = endpoint.clone();
        let router = Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, BlobsProtocol::new(&blobs, None))
            .accept(iroh_gossip::ALPN, gossip)
            .accept(iroh_docs::ALPN, docs.clone())
            .spawn();
        let authorization_task = tokio::spawn(async move {
            // Recheck once after setup in case authorization changed while the
            // persistent protocol stores were opening.
            if watcher_gate.close_unauthorized().await.is_err() {
                watcher_endpoint.close().await;
                return;
            }
            while identity_changes.changed().await.is_ok() {
                if watcher_gate.close_unauthorized().await.is_err() {
                    watcher_endpoint.close().await;
                    return;
                }
            }
        });
        Ok(Self {
            router,
            docs,
            blobs: (*blobs).clone(),
            authorization: gate,
            authorization_task,
        })
    }

    /// The authenticated endpoint, including its outgoing authorization hooks.
    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    /// Address used to connect to this installation's certified device identity.
    pub fn address(&self) -> EndpointAddr {
        self.endpoint().addr()
    }

    /// Private mirror API for the bridge; never a second source of truth.
    pub fn docs(&self) -> &Docs {
        &self.docs
    }

    /// Private content mirror; downloads retain Iroh's content-hash verification.
    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// Close unauthorized live connections after a local authorization action.
    ///
    /// Store wake-ups also perform this check automatically. Awaiting it lets a
    /// device-management command finish only after current access is closed.
    pub async fn refresh_authorization(&self) -> Result<()> {
        match self.authorization.close_unauthorized().await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.endpoint().close().await;
                Err(error)
            }
        }
    }

    /// Stop protocol tasks and flush persistent mirrors before closing sockets.
    pub async fn shutdown(self) -> Result<()> {
        self.authorization_task.abort();
        self.router
            .shutdown()
            .await
            .map_err(|error| transport_error("stop private transport", error))
    }
}

#[cfg(test)]
mod tests;

impl Drop for SelfTransport {
    fn drop(&mut self) {
        self.authorization_task.abort();
    }
}
