//! Bounded signed article vouches, separate from private self facts.

use crate::identity::{
    DeviceCert, Keypair, PublicKey, Signature, canonical_bytes, sign_canonical, verify_canonical,
};
use crate::private_library::SavedItem;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// Maximum metadata frame; archived bytes travel separately.
pub const MAX_SHARE_BYTES: usize = 2 * 1024 * 1024;
/// Maximum copyable connection information.
pub const MAX_CONTACT_BYTES: usize = 32 * 1024;
/// Maximum UTF-8 share note length.
pub const MAX_SHARE_NOTE_BYTES: usize = 64 * 1024;

/// Public connection information certified by an explicitly chosen master.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactCard {
    /// Supported payload version.
    pub version: u8,
    /// Master-certified endpoint device.
    pub certificate: DeviceCert,
    /// Serialized endpoint address; the sync layer checks its endpoint identity.
    pub address: String,
    /// Device signature binding the address and certificate.
    pub signature: Signature,
}

impl ContactCard {
    /// Sign connection information without exposing a document capability.
    pub fn sign(
        certificate: DeviceCert,
        address: String,
        device: &Keypair,
    ) -> Result<Self, ShareError> {
        let mut card = Self {
            version: 1,
            certificate,
            address,
            signature: Signature::from_bytes([0; 64]),
        };
        if card.certificate.device_pubkey != *device.public() {
            return Err(ShareError::Invalid);
        }
        card.validate()?;
        let signature =
            sign_canonical(device, &card.payload()).map_err(|_| ShareError::Encoding)?;
        card.signature = signature;
        Ok(card)
    }
    fn payload(&self) -> impl Serialize + '_ {
        (
            "carrel-contact-v1",
            self.version,
            &self.certificate,
            &self.address,
        )
    }
    fn validate(&self) -> Result<(), ShareError> {
        self.certificate.verify()?;
        if self.version != 1 || self.address.is_empty() || self.address.len() > 16 * 1024 {
            return Err(ShareError::Invalid);
        }
        Ok(())
    }
    /// Verify the certified device's proof.
    pub fn verify(&self) -> Result<(), ShareError> {
        self.validate()?;
        verify_canonical(
            &self.certificate.device_pubkey,
            &self.payload(),
            &self.signature,
        )
        .map_err(|_| ShareError::Signature)
    }
    /// A public contact code, not a private pairing invitation.
    pub fn to_text(&self) -> Result<String, ShareError> {
        self.verify()?;
        let text = format!(
            "carrel-contact:{}",
            serde_json::to_string(self).map_err(|_| ShareError::Encoding)?
        );
        if text.len() > MAX_CONTACT_BYTES {
            return Err(ShareError::Invalid);
        }
        Ok(text)
    }
    /// Decode bounded contact information; following remains a separate local action.
    pub fn from_text(text: &str) -> Result<Self, ShareError> {
        if text.len() > MAX_CONTACT_BYTES {
            return Err(ShareError::Invalid);
        }
        let card: Self = serde_json::from_str(
            text.trim()
                .strip_prefix("carrel-contact:")
                .ok_or(ShareError::Invalid)?,
        )
        .map_err(|_| ShareError::Encoding)?;
        card.verify()?;
        Ok(card)
    }
}

/// One immutable vouch with a fixed, explicitly chosen recipient set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Share {
    /// Stable draft identity derived from its author and request token.
    pub id: String,
    /// Request token retained through retries of the same local draft.
    pub request_id: String,
    /// Version 1 recipient, or the first member of version 2's sorted audience.
    pub recipient: PublicKey,
    /// Version 2's fixed sorted recipient set; absent in original version 1 bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audience: Vec<PublicKey>,
    /// Public article metadata and current archived references only.
    pub article: SavedItem,
    /// Deliberately entered note, never an automatically copied private note.
    pub note: Option<String>,
    /// Original vouch time, used for display and inbox order.
    pub created_at_micros: i64,
    /// Certificate for the signing device.
    pub certificate: DeviceCert,
    /// Supported payload version.
    pub version: u8,
    /// Signature over all vouch fields.
    pub signature: Signature,
}

impl Share {
    /// A retry-stable ID; the token need not be a secret or an access credential.
    pub fn draft_id(certificate: &DeviceCert, request_id: &str) -> Result<String, ShareError> {
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
        {
            return Err(ShareError::Invalid);
        }
        let bytes = canonical_bytes(&(
            "carrel-share-draft-v1",
            certificate.master_pubkey,
            certificate.device_pubkey,
            request_id,
        ))
        .map_err(|_| ShareError::Encoding)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
    /// Sign a validated, deliberately prepared public snapshot.
    pub fn sign(
        request_id: String,
        recipient: PublicKey,
        article: SavedItem,
        note: Option<String>,
        created_at_micros: i64,
        certificate: DeviceCert,
        device: &Keypair,
    ) -> Result<Self, ShareError> {
        if certificate.device_pubkey != *device.public() {
            return Err(ShareError::Invalid);
        }
        let mut share = Self {
            id: Self::draft_id(&certificate, &request_id)?,
            request_id,
            recipient,
            audience: Vec::new(),
            article,
            note,
            created_at_micros,
            certificate,
            version: 1,
            signature: Signature::from_bytes([0; 64]),
        };
        share.validate()?;
        let signature =
            sign_canonical(device, &share.payload()).map_err(|_| ShareError::Encoding)?;
        share.signature = signature;
        if ShareFact::Share(Box::new(share.clone())).to_bytes()?.len() > MAX_SHARE_BYTES {
            return Err(ShareError::Invalid);
        }
        Ok(share)
    }
    /// Sign one vouch for a fixed set, preserving version 1 compatibility.
    pub fn sign_for(
        request_id: String,
        mut recipients: Vec<PublicKey>,
        article: SavedItem,
        note: Option<String>,
        at: i64,
        certificate: DeviceCert,
        device: &Keypair,
    ) -> Result<Self, ShareError> {
        recipients.sort();
        crate::conversation::validate_recipients(&recipients, certificate.master_pubkey)?;
        let mut share = Self::sign(
            request_id,
            recipients[0],
            article,
            note,
            at,
            certificate,
            device,
        )?;
        share.audience = recipients;
        share.version = 2;
        share.validate()?;
        let signature = sign_canonical(device, &(share.payload(), &share.audience))
            .map_err(|_| ShareError::Encoding)?;
        share.signature = signature;
        Ok(share)
    }
    /// All recipients named by the original proof.
    pub fn recipients(&self) -> Vec<PublicKey> {
        if self.version == 1 {
            vec![self.recipient]
        } else {
            self.audience.clone()
        }
    }
    fn payload(&self) -> impl Serialize + '_ {
        (
            "carrel-share-v1",
            self.version,
            &self.id,
            &self.request_id,
            self.recipient,
            &self.article,
            &self.note,
            self.created_at_micros,
            &self.certificate,
        )
    }
    fn validate(&self) -> Result<(), ShareError> {
        self.certificate.verify()?;
        self.recipient.validate().map_err(|_| ShareError::Invalid)?;
        validate_shared_article(&self.article)?;
        if !matches!(self.version, 1 | 2)
            || self.recipient == self.certificate.master_pubkey
            || (self.version == 1 && !self.audience.is_empty())
            || self.id != Self::draft_id(&self.certificate, &self.request_id)?
            || self
                .note
                .as_ref()
                .is_some_and(|note| note.len() > MAX_SHARE_NOTE_BYTES)
        {
            return Err(ShareError::Invalid);
        }
        if self.version == 2 {
            crate::conversation::validate_recipients(
                &self.audience,
                self.certificate.master_pubkey,
            )?;
            if self.audience.first() != Some(&self.recipient) {
                return Err(ShareError::Invalid);
            }
        }
        Ok(())
    }
    /// Verify signatures, recipient and the exclusion of private metadata.
    pub fn verify(&self) -> Result<(), ShareError> {
        self.validate()?;
        if self.version == 2 {
            return verify_canonical(
                &self.certificate.device_pubkey,
                &(self.payload(), &self.audience),
                &self.signature,
            )
            .map_err(|_| ShareError::Signature);
        }
        verify_canonical(
            &self.certificate.device_pubkey,
            &self.payload(),
            &self.signature,
        )
        .map_err(|_| ShareError::Signature)
    }
}

/// A permanent signed retraction; it never deletes a recipient's saved article.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShareRetraction {
    /// Original share being withdrawn.
    pub share_id: String,
    /// Original recipient.
    pub recipient: PublicKey,
    /// Display time only; removal is not ordered by clocks.
    pub created_at_micros: i64,
    /// Master-certified retracting device.
    pub certificate: DeviceCert,
    /// Supported payload version.
    pub version: u8,
    /// Signature over the retraction.
    pub signature: Signature,
}
impl ShareRetraction {
    /// Sign an explicit withdrawal; ownership is additionally checked by the store.
    pub fn sign(
        share_id: String,
        recipient: PublicKey,
        created_at_micros: i64,
        certificate: DeviceCert,
        device: &Keypair,
    ) -> Result<Self, ShareError> {
        if certificate.device_pubkey != *device.public() {
            return Err(ShareError::Invalid);
        }
        let mut record = Self {
            share_id,
            recipient,
            created_at_micros,
            certificate,
            version: 1,
            signature: Signature::from_bytes([0; 64]),
        };
        record.validate()?;
        let signature =
            sign_canonical(device, &record.payload()).map_err(|_| ShareError::Encoding)?;
        record.signature = signature;
        Ok(record)
    }
    fn payload(&self) -> impl Serialize + '_ {
        (
            "carrel-share-retract-v1",
            self.version,
            &self.share_id,
            self.recipient,
            self.created_at_micros,
            &self.certificate,
        )
    }
    fn validate(&self) -> Result<(), ShareError> {
        self.certificate.verify()?;
        self.recipient.validate().map_err(|_| ShareError::Invalid)?;
        if self.version != 1 || !crate::annotation::note::valid_revision_id(&self.share_id) {
            return Err(ShareError::Invalid);
        }
        Ok(())
    }
    /// Verify the original master certificate and device signature.
    pub fn verify(&self) -> Result<(), ShareError> {
        self.validate()?;
        verify_canonical(
            &self.certificate.device_pubkey,
            &self.payload(),
            &self.signature,
        )
        .map_err(|_| ShareError::Signature)
    }
}

/// The complete social wire allow-list; private self facts have no variant here.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ShareFact {
    /// Original immutable vouch.
    Share(Box<Share>),
    /// Explicit permanent withdrawal.
    Retraction(Box<ShareRetraction>),
    /// Private profile, reply, author removal or fixed-audience withdrawal.
    Conversation(Box<crate::conversation::ConversationRecord>),
}
impl ShareFact {
    /// Verify the complete signed fact.
    pub fn verify(&self) -> Result<(), ShareError> {
        match self {
            Self::Share(record) => record.verify(),
            Self::Retraction(record) => record.verify(),
            Self::Conversation(record) => record.verify(),
        }
    }
    /// Stable master responsible for this fact.
    pub fn author(&self) -> PublicKey {
        self.certificate().master_pubkey
    }
    /// Original signing certificate.
    pub fn certificate(&self) -> &DeviceCert {
        match self {
            Self::Share(record) => &record.certificate,
            Self::Retraction(record) => &record.certificate,
            Self::Conversation(record) => &record.certificate,
        }
    }
    /// Exact recipient.
    pub fn recipient(&self) -> PublicKey {
        match self {
            Self::Share(record) => record.recipient,
            Self::Retraction(record) => record.recipient,
            Self::Conversation(record) => record.recipients()[0],
        }
    }
    /// Referenced vouch identity.
    pub fn share_id(&self) -> &str {
        match self {
            Self::Share(record) => &record.id,
            Self::Retraction(record) => &record.share_id,
            Self::Conversation(record) => {
                record.scope().map_or("", |scope| scope.share_id.as_str())
            }
        }
    }
    /// Display time, never evidence overriding a retraction.
    pub fn created_at_micros(&self) -> i64 {
        match self {
            Self::Share(record) => record.created_at_micros,
            Self::Retraction(record) => record.created_at_micros,
            Self::Conversation(record) => record.created_at_micros,
        }
    }
    /// Indexed allow-list discriminant.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Share(_) => "share",
            Self::Retraction(_) => "retraction",
            Self::Conversation(record) => record.kind(),
        }
    }
    /// Immutable mirror key scoped by the recipient in the sync bridge.
    pub fn key(&self) -> Result<String, ShareError> {
        Ok(match self {
            Self::Share(record) => format!("share/{}", record.id),
            Self::Retraction(_) => {
                format!("retraction/{}", blake3::hash(&self.to_bytes()?).to_hex())
            }
            Self::Conversation(record) => record.key()?,
        })
    }
    /// Exact permitted destinations; the transport must not infer group membership.
    pub fn recipients(&self) -> Vec<PublicKey> {
        match self {
            Self::Share(record) => record.recipients(),
            Self::Retraction(record) => vec![record.recipient],
            Self::Conversation(record) => record.recipients(),
        }
    }
    /// Canonical bounded bytes for storage and transport.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ShareError> {
        self.verify()?;
        let bytes = canonical_bytes(self).map_err(|_| ShareError::Encoding)?;
        if bytes.len() > MAX_SHARE_BYTES {
            return Err(ShareError::Invalid);
        }
        Ok(bytes)
    }
    /// Decode only verified canonical bytes, rejecting trailing or unknown data.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ShareError> {
        if bytes.len() > MAX_SHARE_BYTES {
            return Err(ShareError::Invalid);
        }
        let mut cursor = Cursor::new(bytes);
        let fact: Self = ciborium::from_reader(&mut cursor).map_err(|_| ShareError::Encoding)?;
        fact.verify()?;
        if cursor.position() != bytes.len() as u64 || fact.to_bytes()? != bytes {
            return Err(ShareError::Encoding);
        }
        Ok(fact)
    }
}

/// Enforce the public subset even for correctly signed malicious inputs.
pub fn validate_shared_article(article: &SavedItem) -> Result<(), ShareError> {
    article.validate()?;
    let url = article.primary_url.as_deref().ok_or(ShareError::Invalid)?;
    if article.kind != "article"
        || !(url.starts_with("https://") || url.starts_with("http://"))
        || article.discovered_at_micros != 0
        || !article.identifiers.is_empty()
        || article
            .article
            .as_ref()
            .is_some_and(|meta| meta.feed_url.is_some())
        || article.contents.len() > 1
        || article
            .contents
            .iter()
            .any(|body| body.format != "html_readable" || body.fetched_at_micros != 0)
        || (article.contents.is_empty() && !article.resources.is_empty())
        || (!article.contents.is_empty()
            && article.current_images.as_ref() != Some(&article.resources))
    {
        return Err(ShareError::Invalid);
    }
    Ok(())
}

/// Prepare the public subset of an explicitly chosen current archive.
/// `images` must contain only resources referenced by its current readable body.
pub fn public_article_snapshot(
    mut article: SavedItem,
    images: Vec<crate::private_library::BlobReference>,
) -> Result<SavedItem, ShareError> {
    article.discovered_at_micros = 0;
    article.identifiers.clear();
    if let Some(metadata) = &mut article.article {
        metadata.feed_url = None;
    }
    article
        .contents
        .retain(|content| content.format == "html_readable");
    for content in &mut article.contents {
        content.fetched_at_micros = 0;
    }
    article.resources = if article.contents.is_empty() {
        Vec::new()
    } else {
        images
    };
    article.current_images = (!article.contents.is_empty()).then(|| article.resources.clone());
    article.normalize();
    validate_shared_article(&article)?;
    Ok(article)
}

/// Errors at the signed sharing boundary, with no fact bodies in diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    /// Unsupported shape, private metadata or exceeded bounds.
    #[error("invalid private share or contact information")]
    Invalid,
    /// Invalid or noncanonical encoded record.
    #[error("invalid sharing record encoding")]
    Encoding,
    /// Device signature does not verify.
    #[error("sharing signature verification failed")]
    Signature,
    /// Certificate does not verify.
    #[error(transparent)]
    Certificate(#[from] crate::identity::CertError),
    /// Invalid public article metadata or content references.
    #[error(transparent)]
    Article(#[from] crate::private_library::LibraryFactError),
}

#[cfg(test)]
mod tests;
