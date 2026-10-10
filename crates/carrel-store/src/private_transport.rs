//! Local private document credentials and certified peer address hints.

use std::collections::BTreeMap;

use carrel_core::identity::{DeviceCert, Keypair, PublicKey, RevocationCert};
use carrel_core::private_library::PrivateFact;
use cozo::DataValue;

use crate::{Result, Store, StoreError, identity::bytes};

/// An explicitly chosen document, independent of identity key material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateDocument {
    /// Document namespace, checked against the capability by `carrel-sync`.
    pub namespace: [u8; 32],
    /// Bounded serialized write capability; confidential local configuration.
    pub capability: Vec<u8>,
}

/// A user-approved pairing grant, verified by the sync protocol before binding.
pub struct PairedIdentity {
    /// Certificate proving ownership of this installation's original key.
    pub certificate: DeviceCert,
    /// Explicit document credential from the verified grant.
    pub document: PrivateDocument,
    /// All historical operational certificates supplied by the Root.
    pub certificates: Vec<DeviceCert>,
    /// Permanent revocations, including ones preceding their certificate.
    pub revocations: Vec<RevocationCert>,
    /// Validated, bounded addresses of active certified peers.
    pub peers: Vec<(PublicKey, Vec<u8>)>,
}

impl Store {
    /// Commit identity, document, proofs and contacts in one transaction.
    /// A crash cannot leave a bound desktop which creates an unrelated document.
    /// This is an explicit local setup API, never an incoming fact handler.
    pub fn bind_paired_identity(&self, setup: &PairedIdentity, device: &Keypair) -> Result<()> {
        if setup.document.capability.is_empty()
            || setup.document.capability.len() > 4096
            || setup.certificates.len() > 512
            || setup.revocations.len() > 512
            || setup.peers.len() > 512
        {
            return Err(invalid("pairing grant bounds"));
        }
        let _guard = self.identity_lock.lock().expect("identity setup lock");
        let cert = &setup.certificate;
        cert.verify()?;
        if cert.device_pubkey != *device.public() || cert.master_pubkey == cert.device_pubkey {
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let binding = crate::identity::SelfIdentity {
            master_pubkey: cert.master_pubkey,
            device_pubkey: cert.device_pubkey,
        };
        if self
            .self_identity()?
            .is_some_and(|existing| existing != binding)
        {
            return Err(StoreError::IdentityBindingConflict);
        }
        let mut certificates = BTreeMap::new();
        let mut facts = BTreeMap::new();
        for proof in std::iter::once(cert).chain(&setup.certificates) {
            proof.verify()?;
            if proof.master_pubkey != binding.master_pubkey {
                return Err(StoreError::IdentityMasterMismatch);
            }
            let encoded = proof.to_bytes().map_err(serialization)?;
            if certificates
                .insert(proof.device_pubkey, encoded.clone())
                .is_some_and(|old| old != encoded)
            {
                return Err(StoreError::IdentityCertificateConflict);
            }
            let fact = PrivateFact::Certificate(proof.clone());
            facts.insert(fact.key()?, fact.to_bytes()?);
        }
        let mut removed = std::collections::BTreeSet::new();
        let mut revocations = Vec::new();
        for proof in &setup.revocations {
            proof.verify()?;
            if proof.master_pubkey != binding.master_pubkey {
                return Err(StoreError::IdentityMasterMismatch);
            }
            removed.insert(proof.device_pubkey);
            revocations.push(DataValue::List(vec![
                DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec()),
                DataValue::Bytes(proof.device_pubkey.as_bytes().to_vec()),
                DataValue::Validity(cozo::Validity::from((proof.revoked_at_micros, true))),
                DataValue::Bytes(proof.to_bytes().map_err(serialization)?),
            ]));
            let fact = PrivateFact::Revocation(proof.clone());
            facts.insert(fact.key()?, fact.to_bytes()?);
        }
        if removed.contains(device.public())
            || self.device_denied(&binding.master_pubkey, device.public())?
        {
            // The unbound case permits one's own original key; the bound case
            // additionally enforces permanent local revocation and blocking.
            return Err(StoreError::IdentityDeviceUnauthorized);
        }
        let mut peers = Vec::new();
        for (key, address) in &setup.peers {
            if address.is_empty()
                || address.len() > 4096
                || !certificates.contains_key(key)
                || removed.contains(key)
            {
                return Err(invalid("uncertified pairing peer"));
            }
            peers.push(DataValue::List(vec![
                DataValue::Bytes(key.as_bytes().to_vec()),
                DataValue::Bytes(address.clone()),
            ]));
        }
        let certificate_rows = certificates
            .into_iter()
            .map(|(key, encoded)| {
                DataValue::List(vec![
                    DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec()),
                    DataValue::Bytes(key.as_bytes().to_vec()),
                    DataValue::Bytes(encoded),
                ])
            })
            .collect();
        let fact_rows = facts
            .into_iter()
            .map(|(key, encoded)| {
                DataValue::List(vec![DataValue::from(key), DataValue::Bytes(encoded)])
            })
            .collect();
        self.query_with_params(r#"
            { ?[master_pubkey, device_pubkey, device_cert] <- $certificates :put device_certificate {master_pubkey, device_pubkey => device_cert} }
            { ?[master_pubkey, device_pubkey, revoked_at, revocation_cert] <- $revocations :put device_revocation {master_pubkey, device_pubkey, revoked_at => revocation_cert} }
            { ?[key, encoded] <- $facts :put self_fact_history {key => encoded} }
            { ?[key, encoded] <- $facts :put self_fact_outbox {key => encoded} }
            { ?[singleton, master_pubkey, device_pubkey] <- [[0, $master, $device]] :put self_identity {singleton => master_pubkey, device_pubkey} }
            { ?[singleton, namespace, capability] <- [[0, $namespace, $capability]] :put self_document {singleton => namespace, capability} }
            { ?[device_pubkey, address] <- $peers :put self_peer_address {device_pubkey => address} }
        "#, BTreeMap::from([
            ("certificates".into(), DataValue::List(certificate_rows)), ("revocations".into(), DataValue::List(revocations)),
            ("facts".into(), DataValue::List(fact_rows)), ("peers".into(), DataValue::List(peers)),
            ("master".into(), DataValue::Bytes(binding.master_pubkey.as_bytes().to_vec())), ("device".into(), DataValue::Bytes(binding.device_pubkey.as_bytes().to_vec())),
            ("namespace".into(), DataValue::Bytes(setup.document.namespace.to_vec())), ("capability".into(), DataValue::Bytes(setup.document.capability.clone())),
        ]))?;
        self.identity_changed();
        Ok(())
    }
    /// Read the durable document binding without inferring or creating one.
    pub fn private_document(&self) -> Result<Option<PrivateDocument>> {
        self.query(
            "?[namespace, capability] := *self_document{singleton: 0, namespace, capability}",
        )?
        .rows
        .first()
        .map(|row| {
            Ok(PrivateDocument {
                namespace: bytes(&row[0])?
                    .try_into()
                    .map_err(|_| invalid("namespace length"))?,
                capability: bytes(&row[1])?.to_vec(),
            })
        })
        .transpose()
    }

    /// Persist the first write capability; subsequent bindings must be identical.
    pub fn bind_private_document(&self, document: &PrivateDocument) -> Result<()> {
        if document.capability.is_empty() || document.capability.len() > 4096 {
            return Err(invalid("capability length"));
        }
        let _guard = self.identity_lock.lock().expect("identity write lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        self.query_with_params("?[singleton, namespace, capability] <- [[0, $namespace, $capability]] :put self_document {singleton => namespace, capability}", BTreeMap::from([
            ("namespace".into(), DataValue::Bytes(document.namespace.to_vec())),
            ("capability".into(), DataValue::Bytes(document.capability.clone())),
        ]))?;
        Ok(())
    }

    /// Save an address only for an active device in the explicitly chosen Root.
    pub fn set_private_peer_address(&self, device: &PublicKey, address: Vec<u8>) -> Result<()> {
        if address.is_empty() || address.len() > 4096 {
            return Err(invalid("peer address length"));
        }
        let _guard = self.identity_lock.lock().expect("identity write lock");
        let binding = self.self_identity()?.ok_or(StoreError::IdentityUnbound)?;
        self.require_private_sender(&binding.device_pubkey, None)?;
        self.require_private_sender(device, None)?;
        self.query_with_params("?[device_pubkey, address] <- [[$device, $address]] :put self_peer_address {device_pubkey => address}", BTreeMap::from([
            ("device".into(), DataValue::Bytes(device.as_bytes().to_vec())),
            ("address".into(), DataValue::Bytes(address)),
        ]))?;
        Ok(())
    }

    /// Return bounded address hints; every network dial rechecks authorization.
    pub fn private_peer_addresses(&self) -> Result<Vec<(PublicKey, Vec<u8>)>> {
        self.query("?[device_pubkey, address] := *self_peer_address{device_pubkey, address}")?
            .rows
            .iter()
            .map(|row| {
                Ok((
                    PublicKey::from_bytes(
                        bytes(&row[0])?
                            .try_into()
                            .map_err(|_| invalid("device key length"))?,
                    ),
                    bytes(&row[1])?.to_vec(),
                ))
            })
            .collect()
    }
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput {
        context: "private transport configuration",
        message: message.into(),
    }
}

fn serialization(error: carrel_core::identity::SerializeError) -> StoreError {
    StoreError::Signing {
        context: "paired identity material",
        message: error.to_string(),
    }
}
