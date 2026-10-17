//! Signed, bounded conversation records. A thread's audience never changes.
use crate::identity::{
    DeviceCert, Keypair, PublicKey, Signature, canonical_bytes, sign_canonical, verify_canonical,
};
use crate::sharing::{Share, ShareError};
use serde::{Deserialize, Serialize};

/// Maximum recipients of one deliberate share (in addition to its author).
pub const MAX_RECIPIENTS: usize = 16;
/// Maximum normalized avatar payload. No URL or cache capability is accepted.
pub const MAX_AVATAR_BYTES: usize = 128 * 1024;

/// The complete immutable audience of an original signed vouch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadScope {
    /// Original vouch, never an article URL.
    pub share_id: String,
    /// Digest of the exact canonical signed root, preventing forked openings.
    pub root_digest: String,
    /// Master that opened the conversation.
    pub sharer: PublicKey,
    /// Sorted unique recipient masters, excluding the sharer.
    pub recipients: Vec<PublicKey>,
}
impl ThreadScope {
    /// Derive scope only from a verified root.
    pub fn from_share(share: &Share) -> Result<Self, ShareError> {
        share.verify()?;
        Ok(Self {
            share_id: share.id.clone(),
            root_digest: blake3::hash(&canonical_bytes(share).map_err(|_| ShareError::Encoding)?)
                .to_hex()
                .to_string(),
            sharer: share.certificate.master_pubkey,
            recipients: share.recipients(),
        })
    }
    /// Whether this identity belongs to the fixed participant set.
    pub fn contains(&self, key: &PublicKey) -> bool {
        self.sharer == *key || self.recipients.contains(key)
    }
    /// Validate the bounded canonical participant set.
    pub fn verify(&self) -> Result<(), ShareError> {
        if !crate::annotation::note::valid_revision_id(&self.share_id)
            || !crate::annotation::note::valid_revision_id(&self.root_digest)
        {
            return Err(ShareError::Invalid);
        }
        validate_recipients(&self.recipients, self.sharer)
    }
    /// Participants in canonical order, including the author.
    pub fn participants(&self) -> Vec<PublicKey> {
        let mut keys = self.recipients.clone();
        keys.push(self.sharer);
        keys.sort();
        keys
    }
}
/// Validate an explicitly chosen recipient set without silently normalizing input.
pub fn validate_recipients(keys: &[PublicKey], author: PublicKey) -> Result<(), ShareError> {
    author.validate().map_err(|_| ShareError::Invalid)?;
    if keys.is_empty()
        || keys.len() > MAX_RECIPIENTS
        || keys.contains(&author)
        || keys.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ShareError::Invalid);
    }
    for key in keys {
        key.validate().map_err(|_| ShareError::Invalid)?;
    }
    Ok(())
}

/// A normalized PNG with a digest over its exact encoded bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Avatar {
    /// BLAKE3 digest of the PNG payload.
    pub digest: String,
    /// A small raster payload, not a URL.
    pub png: Vec<u8>,
}
impl Avatar {
    /// Check framing, dimensions and digest before a native bounded decode.
    pub fn verify(&self) -> Result<(), ShareError> {
        if self.png.len() < 33
            || self.png.len() > MAX_AVATAR_BYTES
            || !self.png.starts_with(b"\x89PNG\r\n\x1a\n")
            || &self.png[12..16] != b"IHDR"
            || self.digest != blake3::hash(&self.png).to_hex().as_str()
        {
            return Err(ShareError::Invalid);
        }
        let width = u32::from_be_bytes(
            self.png[16..20]
                .try_into()
                .map_err(|_| ShareError::Invalid)?,
        );
        let height = u32::from_be_bytes(
            self.png[20..24]
                .try_into()
                .map_err(|_| ShareError::Invalid)?,
        );
        if width == 0 || height == 0 || width > 256 || height > 256 {
            return Err(ShareError::Invalid);
        }
        Ok(())
    }
}

/// Explicit allow-list of new social payloads; none grants private state access.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub enum ConversationContent {
    /// Self-chosen presentation, sent only to this explicitly followed person.
    Profile {
        /// Sole contact allowed to receive this profile.
        recipient: PublicKey,
        /// Logical presentation generation, independent of wall clocks.
        generation: u64,
        /// Optional self-chosen name; empty clears it.
        name: String,
        /// Normalized picture; absent clears it.
        avatar: Option<Avatar>,
    },
    /// Immutable message with a stable idempotency token.
    Reply {
        /// Original vouch and fixed participant set.
        scope: ThreadScope,
        /// Retry-stable message identity.
        id: String,
        /// Local draft token, carried through failures.
        request_id: String,
        /// Optional earlier reply in this same conversation.
        parent: Option<String>,
        /// Plain text, never automatically copied from private notes.
        body: String,
    },
    /// Permanent author removal; preserves the message's ancestry.
    Remove {
        /// Original conversation audience.
        scope: ThreadScope,
        /// Message owned by the signing master.
        reply_id: String,
    },
    /// Permanent closure by the root's sharer.
    Withdraw {
        /// Conversation closed by its original sharer.
        scope: ThreadScope,
    },
}

/// Versioned certified-device signature over the entire typed content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationRecord {
    /// Currently version 1.
    pub version: u8,
    /// Original device certificate, retained when forwarded.
    pub certificate: DeviceCert,
    /// Display time only; never authorizes or reverses removal.
    pub created_at_micros: i64,
    /// Complete signed payload.
    pub content: ConversationContent,
    /// Device signature.
    pub signature: Signature,
}
impl ConversationRecord {
    /// Stable presentation revision, independent of its recipient/device delivery copy.
    pub fn profile_revision_id(&self) -> Result<Option<String>, ShareError> {
        self.verify()?;
        if let ConversationContent::Profile {
            generation,
            name,
            avatar,
            ..
        } = &self.content
        {
            let bytes = canonical_bytes(&(
                "carrel-profile-presentation-v1",
                self.certificate.master_pubkey,
                generation,
                name,
                avatar,
            ))
            .map_err(|_| ShareError::Encoding)?;
            Ok(Some(blake3::hash(&bytes).to_hex().to_string()))
        } else {
            Ok(None)
        }
    }
    /// Sign a validated local action.
    pub fn sign(
        content: ConversationContent,
        created_at_micros: i64,
        certificate: DeviceCert,
        device: &Keypair,
    ) -> Result<Self, ShareError> {
        if certificate.device_pubkey != *device.public() {
            return Err(ShareError::Invalid);
        }
        let mut record = Self {
            version: 1,
            certificate,
            created_at_micros,
            content,
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
            "carrel-conversation-v1",
            self.version,
            &self.certificate,
            self.created_at_micros,
            &self.content,
        )
    }
    fn validate(&self) -> Result<(), ShareError> {
        self.certificate.verify()?;
        if self.version != 1 {
            return Err(ShareError::Invalid);
        }
        if let Some(scope) = self.scope() {
            scope.verify()?;
            if !scope.contains(&self.certificate.master_pubkey) {
                return Err(ShareError::Invalid);
            }
        }
        match &self.content {
            ConversationContent::Profile {
                recipient,
                generation,
                name,
                avatar,
            } => {
                recipient.validate().map_err(|_| ShareError::Invalid)?;
                if *generation == 0
                    || *generation > i64::MAX as u64
                    || name.len() > 256
                    || name.trim() != name
                    || name.chars().any(char::is_control)
                {
                    return Err(ShareError::Invalid);
                }
                if let Some(avatar) = avatar {
                    avatar.verify()?;
                }
            }
            ConversationContent::Reply {
                id,
                request_id,
                parent,
                body,
                ..
            } => {
                if *id != Share::draft_id(&self.certificate, request_id)?
                    || body.trim().is_empty()
                    || body.len() > 64 * 1024
                    || parent
                        .as_ref()
                        .is_some_and(|p| !crate::annotation::note::valid_revision_id(p) || p == id)
                {
                    return Err(ShareError::Invalid);
                }
            }
            ConversationContent::Remove { reply_id, .. } => {
                if !crate::annotation::note::valid_revision_id(reply_id) {
                    return Err(ShareError::Invalid);
                }
            }
            ConversationContent::Withdraw { scope } => {
                if scope.sharer != self.certificate.master_pubkey {
                    return Err(ShareError::Invalid);
                }
            }
        }
        Ok(())
    }
    /// Validate all bounds and the certified signature.
    pub fn verify(&self) -> Result<(), ShareError> {
        self.validate()?;
        verify_canonical(
            &self.certificate.device_pubkey,
            &self.payload(),
            &self.signature,
        )
        .map_err(|_| ShareError::Signature)
    }
    /// Fixed thread, absent for a recipient-scoped profile.
    pub fn scope(&self) -> Option<&ThreadScope> {
        match &self.content {
            ConversationContent::Profile { .. } => None,
            ConversationContent::Reply { scope, .. }
            | ConversationContent::Remove { scope, .. }
            | ConversationContent::Withdraw { scope } => Some(scope),
        }
    }
    /// Signed recipients permitted to receive this record.
    pub fn recipients(&self) -> Vec<PublicKey> {
        match &self.content {
            ConversationContent::Profile { recipient, .. } => vec![*recipient],
            _ => self
                .scope()
                .expect("thread content")
                .participants()
                .into_iter()
                .filter(|key| *key != self.certificate.master_pubkey)
                .collect(),
        }
    }
    /// Indexed wire discriminant.
    pub fn kind(&self) -> &'static str {
        match self.content {
            ConversationContent::Profile { .. } => "profile",
            ConversationContent::Reply { .. } => "reply",
            ConversationContent::Remove { .. } => "reply_removal",
            ConversationContent::Withdraw { .. } => "retraction",
        }
    }
    /// Immutable record key. Replies retain their draft ID through retries.
    pub fn key(&self) -> Result<String, ShareError> {
        self.verify()?;
        if let ConversationContent::Reply { id, .. } = &self.content {
            return Ok(format!("reply/{id}"));
        }
        Ok(format!(
            "{}/{}",
            self.kind(),
            blake3::hash(&canonical_bytes(self).map_err(|_| ShareError::Encoding)?).to_hex()
        ))
    }
}
