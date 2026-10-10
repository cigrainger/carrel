//! One-use invitations on an isolated, bounded protocol with no private readers.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use carrel_core::identity::{
    DeviceCert, Keypair, PublicKey, RevocationCert, Signature, sign_canonical, verify_canonical,
};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Builder, Connection, presets},
};
use iroh_docs::{
    DocTicket,
    api::protocol::{AddrInfoOptions, ShareMode},
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

use crate::{Result, SyncError, bridge::SelfBridge, error::transport_error};

const ALPN: &[u8] = b"carrel/pair/1";
const MAX_MESSAGE: usize = 256 * 1024;
const LIFETIME: Duration = Duration::from_secs(300);
const EXCHANGE: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize)]
struct Invitation {
    version: u8,
    endpoint: EndpointAddr,
    master: PublicKey,
    token: [u8; 32],
    expires: i64,
}

#[derive(Serialize, Deserialize)]
struct Request {
    token: [u8; 32],
    name: String,
    device: PublicKey,
    signature: Signature,
}

impl Request {
    fn payload(&self, client: [u8; 32], inviter: [u8; 32]) -> impl Serialize + '_ {
        (
            "carrel-pair-device-v1",
            self.token,
            &self.name,
            self.device,
            client,
            inviter,
        )
    }
}

/// A verified grant, including historical authorization proofs for this Root.
#[derive(Serialize, Deserialize)]
pub struct PairingGrant {
    /// Certificate for the joining installation's original operational key.
    pub certificate: DeviceCert,
    /// Known original device certificates.
    pub devices: Vec<DeviceCert>,
    /// Permanent master-signed removals.
    pub revocations: Vec<RevocationCert>,
    /// Explicit private document capability and inviter's address.
    pub ticket: DocTicket,
    token: [u8; 32],
    signature: Signature,
}

impl PairingGrant {
    /// Local atomic setup material from a previously verified pairing session.
    pub fn local_setup(&self) -> Result<carrel_store::private_transport::PairedIdentity> {
        if !matches!(self.ticket.capability, iroh_docs::Capability::Write(_)) {
            return Err(SyncError::InvalidInvitation);
        }
        let peers = self
            .ticket
            .nodes
            .iter()
            .map(|address| {
                Ok((
                    PublicKey::from_bytes(*address.id.as_bytes()),
                    serde_json::to_vec(address).map_err(|_| SyncError::InvalidInvitation)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(carrel_store::private_transport::PairedIdentity {
            certificate: self.certificate.clone(),
            document: carrel_store::private_transport::PrivateDocument {
                namespace: *self.ticket.capability.id().as_bytes(),
                capability: serde_json::to_vec(&self.ticket.capability)
                    .map_err(|_| SyncError::InvalidInvitation)?,
            },
            certificates: self.devices.clone(),
            revocations: self.revocations.clone(),
            peers,
        })
    }
    fn payload(&self) -> impl Serialize + '_ {
        (
            "carrel-pair-grant-v1",
            &self.certificate,
            &self.devices,
            &self.revocations,
            &self.ticket,
            self.token,
        )
    }

    fn verify(&self, invitation: &Invitation, device: &PublicKey) -> Result<()> {
        if self.certificate.master_pubkey != invitation.master
            || self.certificate.device_pubkey != *device
            || !same_token(&self.token, &invitation.token)
            || self.devices.len() > 512
            || self.revocations.len() > 512
        {
            return Err(SyncError::InvalidInvitation);
        }
        self.certificate
            .verify()
            .map_err(|_| SyncError::InvalidInvitation)?;
        verify_canonical(&invitation.master, &self.payload(), &self.signature)
            .map_err(|error| transport_error("verify pairing grant", error))?;
        for cert in &self.devices {
            cert.verify().map_err(|_| SyncError::InvalidInvitation)?;
            if cert.master_pubkey != invitation.master {
                return Err(SyncError::InvalidInvitation);
            }
        }
        for proof in &self.revocations {
            proof.verify().map_err(|_| SyncError::InvalidInvitation)?;
            if proof.master_pubkey != invitation.master || proof.device_pubkey == *device {
                return Err(SyncError::InvalidInvitation);
            }
        }
        let inviter_known = self.ticket.nodes.iter().any(|node| {
            self.devices.iter().any(|cert| {
                cert.device_pubkey.as_bytes() == node.id.as_bytes()
                    && !self
                        .revocations
                        .iter()
                        .any(|proof| proof.device_pubkey == cert.device_pubkey)
            })
        });
        if !inviter_known {
            return Err(SyncError::InvalidInvitation);
        }
        Ok(())
    }
}

/// A temporary invitation. Dropping it cancels the service and releases the master.
pub struct PairingInvitation {
    /// Copyable invitation text; a short-lived private credential.
    pub text: String,
    endpoint: Endpoint,
    task: JoinHandle<()>,
}

impl PairingInvitation {
    /// Start a five-minute, one-use invitation using an explicitly unlocked master.
    pub async fn create(bridge: Arc<SelfBridge>, master: Keypair) -> Result<Self> {
        let mut invitation =
            Self::with_builder(bridge, master, Endpoint::builder(presets::N0)).await?;
        // Include the relay as well as LAN addresses when internet connectivity
        // is available. LAN pairing remains possible without a relay.
        let _ = tokio::time::timeout(Duration::from_secs(5), invitation.endpoint.online()).await;
        let mut contents = parse_invitation(&invitation.text)?;
        contents.endpoint = invitation.endpoint.addr();
        invitation.text = format!(
            "carrel-pair:{}",
            serde_json::to_string(&contents).map_err(|_| SyncError::InvalidInvitation)?
        );
        Ok(invitation)
    }

    /// Customize invitation addressing for local deployments and tests.
    pub async fn with_builder(
        bridge: Arc<SelfBridge>,
        master: Keypair,
        builder: Builder,
    ) -> Result<Self> {
        let binding = bridge
            .store
            .self_identity()?
            .ok_or(SyncError::IdentityUnbound)?;
        if *master.public() != binding.master_pubkey {
            return Err(SyncError::DeviceKeyMismatch);
        }
        bridge.host.refresh_authorization().await?;
        let endpoint = builder
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|error| transport_error("open invitation", error))?;
        let token = Zeroizing::new(Keypair::generate().to_seed_bytes());
        let invitation = Invitation {
            version: 1,
            endpoint: endpoint.addr(),
            master: *master.public(),
            token: *token,
            expires: time::OffsetDateTime::now_utc().unix_timestamp() + LIFETIME.as_secs() as i64,
        };
        let text = format!(
            "carrel-pair:{}",
            serde_json::to_string(&invitation).map_err(|_| SyncError::InvalidInvitation)?
        );
        let server = endpoint.clone();
        let task = tokio::spawn(async move {
            let consumed = AtomicBool::new(false);
            let _ = tokio::time::timeout(LIFETIME, async {
                while let Some(incoming) = server.accept().await {
                    let result = tokio::time::timeout(EXCHANGE, async {
                        let connection = incoming
                            .await
                            .map_err(|error| transport_error("accept invitation", error))?;
                        let result = serve(
                            &bridge,
                            &master,
                            &token,
                            &consumed,
                            *server.id().as_bytes(),
                            connection.clone(),
                        )
                        .await;
                        if result.is_err() {
                            connection.close(1u32.into(), b"pairing rejected");
                        }
                        result
                    })
                    .await;
                    // Invalid requests cannot consume an invitation. Once a valid
                    // request issues a grant, even a lost reply consumes it.
                    if consumed.load(Ordering::Acquire) || matches!(result, Ok(Ok(true))) {
                        break;
                    }
                }
            })
            .await;
            server.close().await;
        });
        Ok(Self {
            text,
            endpoint,
            task,
        })
    }

    /// Stop accepting requests immediately.
    pub async fn cancel(&self) {
        self.task.abort();
        self.endpoint.close().await;
    }

    /// Whether the invitation expired, was cancelled, or issued its single grant.
    pub fn finished(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for PairingInvitation {
    fn drop(&mut self) {
        self.task.abort();
        let endpoint = self.endpoint.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                endpoint.close().await;
            });
        }
    }
}

async fn serve(
    bridge: &SelfBridge,
    master: &Keypair,
    token: &[u8; 32],
    consumed: &AtomicBool,
    inviter: [u8; 32],
    connection: Connection,
) -> Result<bool> {
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|error| transport_error("accept pairing request", error))?;
    let bytes = Zeroizing::new(
        recv.read_to_end(4096)
            .await
            .map_err(|_| SyncError::InvalidInvitation)?,
    );
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| SyncError::InvalidInvitation)?;
    if !same_token(&request.token, token) || request.name.len() > 256 {
        return Err(SyncError::InvalidInvitation);
    }
    verify_canonical(
        &request.device,
        &request.payload(*connection.remote_id().as_bytes(), inviter),
        &request.signature,
    )
    .map_err(|_| SyncError::InvalidInvitation)?;
    let device = request.device;
    if device == *master.public() {
        return Err(SyncError::InvalidInvitation);
    }
    let existing = bridge.store.self_devices()?;
    let certificate = if let Some(row) = existing
        .iter()
        .find(|row| row.certificate.device_pubkey == device)
    {
        if !row.is_authorized() {
            return Err(SyncError::DeviceUnauthorized);
        }
        row.certificate.clone()
    } else {
        DeviceCert::new(master, &device, Some(request.name))
    };
    consumed.store(true, Ordering::Release);
    bridge.store.accept_self_device_certificate(&certificate)?;
    bridge.host.refresh_authorization().await?;
    let mut ticket = bridge
        .doc
        .share(ShareMode::Write, AddrInfoOptions::Id)
        .await
        .map_err(|error| transport_error("grant private document", error))?;
    ticket.nodes = vec![bridge.host.address()];
    let devices = bridge.store.self_devices()?;
    let mut grant = PairingGrant {
        certificate,
        devices: devices
            .iter()
            .filter(|row| !row.locally_blocked)
            .map(|row| row.certificate.clone())
            .collect(),
        revocations: bridge.store.self_revocation_proofs()?,
        ticket,
        token: *token,
        signature: Signature::from_bytes([0; 64]),
    };
    let signature =
        sign_canonical(master, &grant.payload()).map_err(|_| SyncError::InvalidInvitation)?;
    grant.signature = signature;
    let reply =
        Zeroizing::new(serde_json::to_vec(&grant).map_err(|_| SyncError::InvalidInvitation)?);
    if reply.len() > MAX_MESSAGE {
        return Ok(true);
    }
    // At this point the invitation is consumed. A failed response cannot make
    // it authorize a second key.
    let _ = async {
        send.write_all(&reply)
            .await
            .map_err(|error| transport_error("send pairing grant", error))?;
        send.finish()
            .map_err(|error| transport_error("finish pairing grant", error))?;
        let (mut ack_send, mut ack_recv) = connection
            .accept_bi()
            .await
            .map_err(|error| transport_error("accept pairing confirmation", error))?;
        let bytes = ack_recv
            .read_to_end(4096)
            .await
            .map_err(|_| SyncError::InvalidInvitation)?;
        let address: EndpointAddr =
            serde_json::from_slice(&bytes).map_err(|_| SyncError::InvalidInvitation)?;
        if address.id.as_bytes() != device.as_bytes() {
            return Err(SyncError::InvalidInvitation);
        }
        bridge.store.set_private_peer_address(&device, bytes)?;
        bridge
            .doc
            .start_sync(vec![address])
            .await
            .map_err(|error| transport_error("start paired sync", error))?;
        ack_send
            .write_all(b"ok")
            .await
            .map_err(|error| transport_error("confirm paired sync", error))?;
        ack_send
            .finish()
            .map_err(|error| transport_error("finish paired sync", error))?;
        connection.closed().await;
        Ok::<_, SyncError>(())
    }
    .await;
    Ok(true)
}

/// An authenticated grant awaiting the joining desktop's persistent setup.
pub struct PairingSession {
    /// Verified grant; consuming it does not require or reveal a master seed.
    pub grant: PairingGrant,
    endpoint: Endpoint,
    connection: Connection,
}

impl Drop for PairingSession {
    fn drop(&mut self) {
        let endpoint = self.endpoint.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                endpoint.close().await;
            });
        }
    }
}

impl PairingSession {
    /// Paste an invitation and prove possession of this installation's device key.
    pub async fn join(text: &str, device: &Keypair, name: String) -> Result<Self> {
        Self::with_builder(text, device, name, Endpoint::builder(presets::N0)).await
    }

    /// Join through a custom address transport while retaining proof verification.
    pub async fn with_builder(
        text: &str,
        device: &Keypair,
        name: String,
        builder: Builder,
    ) -> Result<Self> {
        let invitation = parse_invitation(text)?;
        if name.len() > 256 {
            return Err(SyncError::InvalidInvitation);
        }
        let endpoint = builder
            .bind()
            .await
            .map_err(|error| transport_error("open pairing client", error))?;
        let result = tokio::time::timeout(EXCHANGE, async {
            let connection = endpoint
                .connect(invitation.endpoint.clone(), ALPN)
                .await
                .map_err(|error| transport_error("connect invitation", error))?;
            let (mut send, mut recv) = connection
                .open_bi()
                .await
                .map_err(|error| transport_error("open pairing request", error))?;
            let mut request = Request {
                token: invitation.token,
                name,
                device: *device.public(),
                signature: Signature::from_bytes([0; 64]),
            };
            let signature = sign_canonical(
                device,
                &request.payload(
                    *endpoint.id().as_bytes(),
                    *invitation.endpoint.id.as_bytes(),
                ),
            )
            .map_err(|_| SyncError::InvalidInvitation)?;
            request.signature = signature;
            let bytes = Zeroizing::new(
                serde_json::to_vec(&request).map_err(|_| SyncError::InvalidInvitation)?,
            );
            send.write_all(&bytes)
                .await
                .map_err(|error| transport_error("send pairing request", error))?;
            send.finish()
                .map_err(|error| transport_error("finish pairing request", error))?;
            let bytes = Zeroizing::new(
                recv.read_to_end(MAX_MESSAGE)
                    .await
                    .map_err(|_| SyncError::InvalidInvitation)?,
            );
            let grant: PairingGrant =
                serde_json::from_slice(&bytes).map_err(|_| SyncError::InvalidInvitation)?;
            grant.verify(&invitation, device.public())?;
            Ok::<_, SyncError>((grant, connection))
        })
        .await
        .map_err(|_| SyncError::InvalidInvitation)
        .and_then(|result| result);
        match result {
            Ok((grant, connection)) => Ok(Self {
                grant,
                endpoint,
                connection,
            }),
            Err(error) => {
                endpoint.close().await;
                Err(error)
            }
        }
    }

    /// Confirm only after binding the same identity and document durably.
    pub async fn confirm(&self, address: EndpointAddr) -> Result<()> {
        if address.id.as_bytes() != self.grant.certificate.device_pubkey.as_bytes() {
            return Err(SyncError::DeviceKeyMismatch);
        }
        let result = tokio::time::timeout(EXCHANGE, async {
            let (mut send, mut recv) = self
                .connection
                .open_bi()
                .await
                .map_err(|error| transport_error("open pairing confirmation", error))?;
            let bytes = serde_json::to_vec(&address).map_err(|_| SyncError::InvalidInvitation)?;
            send.write_all(&bytes)
                .await
                .map_err(|error| transport_error("send pairing confirmation", error))?;
            send.finish()
                .map_err(|error| transport_error("finish pairing confirmation", error))?;
            if recv
                .read_to_end(16)
                .await
                .map_err(|_| SyncError::InvalidInvitation)?
                != b"ok"
            {
                return Err(SyncError::InvalidInvitation);
            }
            Ok(())
        })
        .await
        .map_err(|_| SyncError::InvalidInvitation)
        .and_then(|result| result);
        self.endpoint.close().await;
        result
    }
}

fn parse_invitation(text: &str) -> Result<Invitation> {
    if text.len() > 16 * 1024 {
        return Err(SyncError::InvalidInvitation);
    }
    let value: Invitation = serde_json::from_str(
        text.trim()
            .strip_prefix("carrel-pair:")
            .ok_or(SyncError::InvalidInvitation)?,
    )
    .map_err(|_| SyncError::InvalidInvitation)?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if value.version != 1
        || value.expires <= now
        || value.expires > now + LIFETIME.as_secs() as i64 + 60
    {
        return Err(SyncError::InvalidInvitation);
    }
    value
        .master
        .validate()
        .map_err(|_| SyncError::InvalidInvitation)?;
    Ok(value)
}

fn same_token(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::SelfTransport;
    use carrel_store::{Store, blobs::BlobStore};

    async fn source(dir: &std::path::Path, root: &Keypair, key: &Keypair) -> Arc<SelfBridge> {
        let store = Store::open(&dir.join("source")).unwrap();
        store.migrate().unwrap();
        store
            .bind_self_identity(
                &DeviceCert::new(root, key.public(), Some("First desktop".into())),
                key,
            )
            .unwrap();
        let host = Arc::new(
            SelfTransport::start_with_builder(
                store.clone(),
                &dir.join("mirror"),
                key,
                Endpoint::builder(presets::Minimal)
                    .bind_addr("127.0.0.1:0")
                    .unwrap(),
            )
            .await
            .unwrap(),
        );
        Arc::new(
            SelfBridge::resume(store, BlobStore::open(dir.join("cache")), host)
                .await
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn one_invitation_pairs_original_keys_and_persists_the_same_document() {
        let dir = tempfile::tempdir().unwrap();
        let root = Keypair::generate();
        let akey = Keypair::generate();
        let bkey = Keypair::generate();
        let a = source(&dir.path().join("a"), &root, &akey).await;
        let invitation = PairingInvitation::with_builder(
            a.clone(),
            root,
            Endpoint::builder(presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
        .unwrap();
        let bstore = Store::open(&dir.path().join("b/source")).unwrap();
        bstore.migrate().unwrap();
        let session = PairingSession::with_builder(
            &invitation.text,
            &bkey,
            "Second desktop".into(),
            Endpoint::builder(presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
        .unwrap();
        assert_ne!(
            session.grant.certificate.device_pubkey,
            akey.public().to_owned()
        );
        assert!(bstore.self_identity().unwrap().is_none());
        bstore
            .bind_paired_identity(&session.grant.local_setup().unwrap(), &bkey)
            .unwrap();
        let bhost = Arc::new(
            SelfTransport::start_with_builder(
                bstore.clone(),
                &dir.path().join("b/mirror"),
                &bkey,
                Endpoint::builder(presets::Minimal)
                    .bind_addr("127.0.0.1:0")
                    .unwrap(),
            )
            .await
            .unwrap(),
        );
        let b = SelfBridge::import(
            bstore.clone(),
            BlobStore::open(dir.path().join("b/cache")),
            bhost,
            session.grant.ticket.capability.clone(),
        )
        .await
        .unwrap();
        session.confirm(b.host.address()).await.unwrap();
        assert_eq!(a.doc.id(), b.doc.id());
        assert_eq!(
            a.store.private_peer_addresses().unwrap()[0].0,
            *bkey.public()
        );
        assert_eq!(
            bstore.private_document().unwrap().unwrap().namespace,
            *a.doc.id().as_bytes()
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while !invitation.finished() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let second = Keypair::generate();
        assert!(
            PairingSession::with_builder(
                &invitation.text,
                &second,
                "Intruder".into(),
                Endpoint::builder(presets::Minimal)
                    .bind_addr("127.0.0.1:0")
                    .unwrap()
            )
            .await
            .is_err()
        );
        assert!(
            !a.store
                .self_devices()
                .unwrap()
                .iter()
                .any(|row| row.certificate.device_pubkey == *second.public())
        );
        drop(session);
        drop(invitation);
        b.doc.close().await.unwrap();
        b.host.endpoint().close().await;
        a.doc.close().await.unwrap();
        a.host.endpoint().close().await;
    }

    #[tokio::test]
    async fn wrong_token_cannot_consume_an_invitation_and_cancellation_closes_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = Keypair::generate();
        let key = Keypair::generate();
        let a = source(dir.path(), &root, &key).await;
        let invitation = PairingInvitation::with_builder(
            a.clone(),
            root,
            Endpoint::builder(presets::Minimal)
                .bind_addr("127.0.0.1:0")
                .unwrap(),
        )
        .await
        .unwrap();
        let mut altered = parse_invitation(&invitation.text).unwrap();
        altered.token[0] ^= 1;
        let text = format!("carrel-pair:{}", serde_json::to_string(&altered).unwrap());
        let foreign = Keypair::generate();
        assert!(
            PairingSession::with_builder(
                &text,
                &foreign,
                "Bad token".into(),
                Endpoint::builder(presets::Minimal)
                    .bind_addr("127.0.0.1:0")
                    .unwrap()
            )
            .await
            .is_err()
        );
        assert!(!invitation.finished());
        assert!(
            !a.store
                .self_devices()
                .unwrap()
                .iter()
                .any(|row| row.certificate.device_pubkey == *foreign.public())
        );
        invitation.cancel().await;
        assert!(
            PairingSession::with_builder(
                &invitation.text,
                &foreign,
                "Cancelled".into(),
                Endpoint::builder(presets::Minimal)
                    .bind_addr("127.0.0.1:0")
                    .unwrap()
            )
            .await
            .is_err()
        );
        a.doc.close().await.unwrap();
        a.host.endpoint().close().await;
    }

    #[test]
    fn malformed_expired_and_unbounded_invitations_are_rejected() {
        assert!(parse_invitation("carrel-pair:{}").is_err());
        assert!(parse_invitation(&"x".repeat(16 * 1024 + 1)).is_err());
        let key = Keypair::generate();
        let mut invitation = Invitation {
            version: 1,
            endpoint: EndpointAddr::new(
                iroh::PublicKey::from_bytes(key.public().as_bytes()).unwrap(),
            ),
            master: *key.public(),
            token: [0; 32],
            expires: time::OffsetDateTime::now_utc().unix_timestamp() - 1,
        };
        let encode =
            |value: &Invitation| format!("carrel-pair:{}", serde_json::to_string(value).unwrap());
        assert!(parse_invitation(&encode(&invitation)).is_err());
        invitation.expires += 100000;
        assert!(parse_invitation(&encode(&invitation)).is_err());
    }

    #[tokio::test]
    async fn isolated_pairing_endpoint_accepts_a_connection() {
        let server = Endpoint::builder(presets::Minimal)
            .bind_addr("127.0.0.1:0")
            .unwrap()
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let client = Endpoint::builder(presets::Minimal)
            .bind_addr("127.0.0.1:0")
            .unwrap()
            .bind()
            .await
            .unwrap();
        let address = server.addr();
        let port = address.ip_addrs().next().unwrap().port();
        let address = EndpointAddr::new(address.id)
            .with_ip_addr(std::net::SocketAddr::from(([127, 0, 0, 1], port)));
        let accept = tokio::spawn(async move {
            let connection = server.accept().await.unwrap().await.unwrap();
            connection.close(0u32.into(), b"done");
            server.close().await;
        });
        let connection =
            tokio::time::timeout(Duration::from_secs(5), client.connect(address, ALPN))
                .await
                .unwrap()
                .unwrap();
        connection.closed().await;
        accept.await.unwrap();
        client.close().await;
    }
}
