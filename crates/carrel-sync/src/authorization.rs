//! Authorization shared by every protocol on the private endpoint.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use carrel_core::identity::PublicKey;
use carrel_store::Store;
use iroh::endpoint::{
    AfterHandshakeOutcome, BeforeConnectOutcome, Connection, EndpointHooks, VarInt,
    WeakConnectionHandle,
};
use iroh::{EndpointAddr, EndpointId};

use crate::error::transport_error;
use crate::{Result, SyncError};

const DENIED_CODE: u32 = 403;
const DENIED_REASON: &[u8] = b"private device authorization required";

#[derive(Clone)]
pub(crate) struct AuthorizationGate {
    store: Store,
    local: PublicKey,
    connections: Arc<Mutex<Vec<WeakConnectionHandle>>>,
}

impl std::fmt::Debug for AuthorizationGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorizationGate")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

impl AuthorizationGate {
    #[cfg(test)]
    pub(crate) fn live_connection_count(&self) -> usize {
        self.connections
            .lock()
            .unwrap()
            .iter()
            .filter(|handle| {
                handle
                    .upgrade()
                    .is_some_and(|connection| connection.close_reason().is_none())
            })
            .count()
    }

    pub(crate) fn new(store: Store, local: PublicKey) -> Self {
        Self {
            store,
            local,
            connections: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn active_devices(&self) -> Result<BTreeSet<PublicKey>> {
        let binding = self
            .store
            .self_identity()?
            .ok_or(SyncError::IdentityUnbound)?;
        if binding.device_pubkey != self.local {
            return Err(SyncError::DeviceKeyMismatch);
        }
        let active: BTreeSet<_> = self
            .store
            .self_devices()?
            .into_iter()
            .filter(|device| device.is_authorized())
            .map(|device| device.certificate.device_pubkey)
            .collect();
        if !active.contains(&self.local) {
            return Err(SyncError::DeviceUnauthorized);
        }
        Ok(active)
    }

    pub(crate) async fn validate_local(&self) -> Result<()> {
        let gate = self.clone();
        tokio::task::spawn_blocking(move || gate.active_devices().map(|_| ()))
            .await
            .map_err(|error| transport_error("validate private device", error))?
    }

    fn allows(&self, remote: EndpointId, alpn: &[u8]) -> bool {
        private_protocol(alpn)
            && self
                .active_devices()
                .is_ok_and(|active| active.contains(&key(remote)))
    }

    fn register(&self, connection: &Connection) -> bool {
        // The revocation watcher takes this same lock before reading Cozo. A
        // handshake accepted just before a write is therefore included in its
        // close pass; a handshake after the write reads the revoked state.
        let mut connections = self.connections.lock().expect("private connection lock");
        if !self.allows(connection.remote_id(), connection.alpn()) {
            return false;
        }
        connections.retain(|handle| {
            handle
                .upgrade()
                .is_some_and(|connection| connection.close_reason().is_none())
        });
        connections.push(connection.weak_handle());
        true
    }

    pub(crate) async fn close_unauthorized(&self) -> Result<()> {
        let gate = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut connections = gate.connections.lock().expect("private connection lock");
            // Store failures deny access rather than leave an older allow-list.
            let active = gate.active_devices();
            connections.retain(|handle| {
                let Some(connection) = handle.upgrade() else {
                    return false;
                };
                if !active
                    .as_ref()
                    .is_ok_and(|active| active.contains(&key(connection.remote_id())))
                {
                    connection.close(VarInt::from_u32(DENIED_CODE), DENIED_REASON);
                    return false;
                }
                connection.close_reason().is_none()
            });
            active.map(|_| ())
        })
        .await
        .map_err(|error| transport_error("refresh private authorization", error))?
    }
}

impl EndpointHooks for AuthorizationGate {
    async fn before_connect<'a>(
        &'a self,
        remote: &'a EndpointAddr,
        alpn: &'a [u8],
    ) -> BeforeConnectOutcome {
        let gate = self.clone();
        let remote = remote.id;
        let alpn = alpn.to_vec();
        if tokio::task::spawn_blocking(move || gate.allows(remote, &alpn))
            .await
            .unwrap_or(false)
        {
            BeforeConnectOutcome::Accept
        } else {
            BeforeConnectOutcome::Reject
        }
    }

    async fn after_handshake<'a>(&'a self, connection: &'a Connection) -> AfterHandshakeOutcome {
        let gate = self.clone();
        let checking = connection.weak_handle();
        if tokio::task::spawn_blocking(move || {
            checking
                .upgrade()
                .is_some_and(|connection| gate.register(&connection))
        })
        .await
        .unwrap_or(false)
        {
            AfterHandshakeOutcome::Accept
        } else {
            AfterHandshakeOutcome::Reject {
                error_code: VarInt::from_u32(DENIED_CODE),
                reason: DENIED_REASON.to_vec(),
            }
        }
    }
}

fn private_protocol(alpn: &[u8]) -> bool {
    [iroh_docs::ALPN, iroh_blobs::ALPN, iroh_gossip::ALPN].contains(&alpn)
}

fn key(endpoint: EndpointId) -> PublicKey {
    PublicKey::from_bytes(*endpoint.as_bytes())
}
