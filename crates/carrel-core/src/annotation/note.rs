//! Signed note revisions with explicit causal ancestry.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

use super::MAX_NOTE_BYTES;
use crate::identity::{
    CanonicalVerifyError, Keypair, PublicKey, SerializeError, Signature, canonical_bytes,
    sign_canonical, verify_canonical,
};

/// Maximum immediate parents in one explicit note resolution.
pub const MAX_NOTE_PARENTS: usize = 1024;
/// Maximum encoded private note revision accepted at the boundary.
pub const MAX_NOTE_REVISION_BYTES: usize = 256 * 1024;

/// The immutable content to be signed for a new revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteContent {
    /// Stable note identity, retained across edits.
    pub id: String,
    /// Full Markdown body.
    pub body: String,
    /// Optional item/highlight target kind.
    pub target_kind: Option<String>,
    /// Optional stable target identity.
    pub target_id: Option<String>,
    /// Original creator, retained across edits.
    pub created_by: PublicKey,
    /// Original creation time, retained across edits.
    pub created_at_micros: i64,
    /// Display timestamp of this revision.
    pub updated_at_micros: i64,
    /// Sorted unique identities of the versions explicitly superseded.
    pub parents: Vec<String>,
}

impl NoteContent {
    /// Sign a version 3 revision, including its causal parents.
    pub fn sign(self, device: &Keypair) -> Result<NoteRevision, NoteRevisionError> {
        let mut revision = NoteRevision {
            id: self.id,
            body: self.body,
            target_kind: self.target_kind,
            target_id: self.target_id,
            created_by: self.created_by,
            created_at_micros: self.created_at_micros,
            updated_at_micros: self.updated_at_micros,
            updated_by: *device.public(),
            parents: self.parents,
            signature_version: 3,
            signature: Signature::from_bytes([0; 64]),
            revision_id: String::new(),
        };
        revision.validate_content()?;
        revision.signature = sign_canonical(device, &revision.payload())?;
        revision.revision_id = revision.computed_revision_id()?;
        Ok(revision)
    }
}

/// A signed immutable note version; concurrent heads remain separate records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRevision {
    /// Stable note identity.
    pub id: String,
    /// Content-addressed identity of this signed version.
    pub revision_id: String,
    /// Full Markdown body.
    pub body: String,
    /// Optional item/highlight target kind.
    pub target_kind: Option<String>,
    /// Optional stable target identity.
    pub target_id: Option<String>,
    /// Original creator.
    pub created_by: PublicKey,
    /// Original creation timestamp.
    pub created_at_micros: i64,
    /// This revision's display timestamp.
    pub updated_at_micros: i64,
    /// Operational key signing this revision.
    pub updated_by: PublicKey,
    /// Canonical payload format: legacy 1/2 or causal 3.
    pub signature_version: i64,
    /// Immediate causal predecessors; legacy ancestry is migration metadata.
    pub parents: Vec<String>,
    /// Signature over the canonical payload.
    pub signature: Signature,
}

impl NoteRevision {
    /// Verify the content, signature, and content-addressed revision identity.
    pub fn verify_signature(&self) -> Result<(), NoteRevisionError> {
        self.validate_content()?;
        verify_canonical(&self.updated_by, &self.payload(), &self.signature)?;
        if self.revision_id != self.computed_revision_id()? {
            return Err(NoteRevisionError::RevisionIdentity);
        }
        if self.parents.contains(&self.revision_id) {
            return Err(NoteRevisionError::Parents);
        }
        Ok(())
    }

    /// Derive a portable identity without changing legacy signing payloads.
    pub fn computed_revision_id(&self) -> Result<String, NoteRevisionError> {
        #[derive(Serialize)]
        struct Signed<'a> {
            domain: &'static str,
            payload: NotePayload<'a>,
            signature: &'a Signature,
        }
        let bytes = canonical_bytes(&Signed {
            domain: "carrel-note-revision-v1",
            payload: self.payload(),
            signature: &self.signature,
        })?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }

    /// Validate a successor against the exact already verified parent records.
    pub fn validate_parents(&self, parents: &[Self]) -> Result<(), NoteRevisionError> {
        if self.parents.len() != parents.len() {
            return Err(NoteRevisionError::MissingParents);
        }
        for (id, parent) in self.parents.iter().zip(parents) {
            parent.verify_signature()?;
            if id != &parent.revision_id {
                return Err(NoteRevisionError::MissingParents);
            }
            if self.id != parent.id
                || self.target_kind != parent.target_kind
                || self.target_id != parent.target_id
                || self.created_by != parent.created_by
                || self.created_at_micros != parent.created_at_micros
            {
                return Err(NoteRevisionError::ImmutableMetadata);
            }
        }
        Ok(())
    }

    /// Encode a verified revision as bounded canonical CBOR for private sync.
    pub fn to_bytes(&self) -> Result<Vec<u8>, NoteRevisionError> {
        self.verify_signature()?;
        let bytes = canonical_bytes(self)?;
        if bytes.len() > MAX_NOTE_REVISION_BYTES {
            return Err(NoteRevisionError::TooLarge);
        }
        Ok(bytes)
    }

    /// Decode one whole bounded record and verify it before storage.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, NoteRevisionError> {
        if bytes.len() > MAX_NOTE_REVISION_BYTES {
            return Err(NoteRevisionError::TooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        let revision: Self = ciborium::de::from_reader(&mut cursor)
            .map_err(|error| NoteRevisionError::Decode(error.to_string()))?;
        if cursor.position() != bytes.len() as u64 {
            return Err(NoteRevisionError::TrailingData);
        }
        revision.verify_signature()?;
        Ok(revision)
    }

    fn validate_content(&self) -> Result<(), NoteRevisionError> {
        if self.id.is_empty() || self.id.len() > 256 {
            return Err(NoteRevisionError::Identity);
        }
        if self.body.trim().is_empty() || self.body.len() > MAX_NOTE_BYTES {
            return Err(NoteRevisionError::Body);
        }
        if !matches!(
            (self.target_kind.as_deref(), self.target_id.as_deref()),
            (None, None) | (Some("item" | "highlight"), Some(_))
        ) || self
            .target_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
        {
            return Err(NoteRevisionError::Target);
        }
        if !matches!(self.signature_version, 1..=3)
            || (self.signature_version == 1 && self.updated_by != self.created_by)
        {
            return Err(NoteRevisionError::Version);
        }
        if self.parents.len() > MAX_NOTE_PARENTS
            || self.parents.iter().any(|id| !valid_revision_id(id))
            || self.parents.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(NoteRevisionError::Parents);
        }
        if self.signature_version == 3
            && self.parents.is_empty()
            && (self.updated_by != self.created_by
                || self.updated_at_micros != self.created_at_micros)
        {
            return Err(NoteRevisionError::RootAuthor);
        }
        Ok(())
    }

    fn payload(&self) -> NotePayload<'_> {
        NotePayload {
            relation: "note",
            id: &self.id,
            body: &self.body,
            target_kind: self.target_kind.as_deref(),
            target_id: self.target_id.as_deref(),
            created_by: self.created_by,
            created_at_micros: self.created_at_micros,
            updated_at_micros: self.updated_at_micros,
            updated_by: (self.signature_version >= 2).then_some(self.updated_by),
            signature_version: (self.signature_version >= 2).then_some(self.signature_version),
            parents: (self.signature_version == 3).then_some(&self.parents),
        }
    }
}

/// Whether a revision reference is a canonical lowercase BLAKE3 hex digest.
pub fn valid_revision_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Serialize)]
struct NotePayload<'a> {
    relation: &'static str,
    id: &'a str,
    body: &'a str,
    target_kind: Option<&'a str>,
    target_id: Option<&'a str>,
    created_by: PublicKey,
    created_at_micros: i64,
    updated_at_micros: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_by: Option<PublicKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parents: Option<&'a Vec<String>>,
}

/// Invalid or unverified note revision data.
#[derive(Debug, thiserror::Error)]
pub enum NoteRevisionError {
    /// A note's stable identity is empty or oversized.
    #[error("invalid note identity")]
    Identity,
    /// The body is empty or exceeds the UTF-8 byte limit.
    #[error("invalid note body")]
    Body,
    /// Target kind and identity do not form a supported pair.
    #[error("invalid note target")]
    Target,
    /// A signing version or legacy updater is unsupported.
    #[error("invalid note signing version or updater")]
    Version,
    /// Parent identities are malformed, duplicate, unordered, or excessive.
    #[error("invalid note causal parents")]
    Parents,
    /// A new causal root must be created and signed by its original author.
    #[error("note creation must be signed by its original creator")]
    RootAuthor,
    /// The caller did not provide the exact referenced parent records.
    #[error("note causal parents are missing")]
    MissingParents,
    /// A revision changes the original creator, target, creation time, or identity.
    #[error("note immutable metadata changed")]
    ImmutableMetadata,
    /// The revision's recorded hash does not match its signed content.
    #[error("note revision identity did not verify")]
    RevisionIdentity,
    /// The inner signature did not verify.
    #[error("note revision signature did not verify")]
    Signature(#[from] CanonicalVerifyError),
    /// Canonical signing material could not be encoded.
    #[error("failed to encode note revision")]
    Serialize(#[from] SerializeError),
    /// The private record exceeds the protocol bound.
    #[error("note revision exceeds {MAX_NOTE_REVISION_BYTES} bytes")]
    TooLarge,
    /// A bounded private record could not be decoded.
    #[error("failed to decode note revision: {0}")]
    Decode(String),
    /// Bytes remain after the first CBOR record.
    #[error("trailing bytes after note revision")]
    TrailingData,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn content() -> NoteContent {
        NoteContent {
            id: "stable-note".into(),
            body: "A café note".into(),
            target_kind: Some("item".into()),
            target_id: Some("article".into()),
            created_by: *Keypair::from_seed(&[1; 32]).public(),
            created_at_micros: 1,
            updated_at_micros: 1,
            parents: Vec::new(),
        }
    }

    fn successor(parent: &NoteRevision, device: &Keypair, body: &str) -> NoteRevision {
        let mut value = content();
        value.body = body.into();
        value.parents = vec![parent.revision_id.clone()];
        value.updated_at_micros = 2;
        value.sign(device).unwrap()
    }

    #[test]
    fn equal_time_offline_branches_have_distinct_signed_identities_and_merge_explicitly() {
        let a = Keypair::from_seed(&[1; 32]);
        let b = Keypair::from_seed(&[2; 32]);
        let base = content().sign(&a).unwrap();
        let first = successor(&base, &a, "Offline draft A");
        let second = successor(&base, &b, "Offline draft B");
        assert_eq!(first.updated_at_micros, second.updated_at_micros);
        assert_ne!(first.revision_id, second.revision_id);
        first.validate_parents(std::slice::from_ref(&base)).unwrap();
        second
            .validate_parents(std::slice::from_ref(&base))
            .unwrap();
        let mut parents = vec![first, second];
        parents.sort_by(|a, b| a.revision_id.cmp(&b.revision_id));
        let mut merge = content();
        merge.body = "Merged café draft".into();
        merge.updated_at_micros = 3;
        merge.parents = parents
            .iter()
            .map(|parent| parent.revision_id.clone())
            .collect();
        let merged = merge.sign(&b).unwrap();
        merged.validate_parents(&parents).unwrap();
        assert_eq!(
            NoteRevision::from_bytes(&merged.to_bytes().unwrap()).unwrap(),
            merged
        );
        assert_eq!(merged.created_by, *a.public());
        assert_eq!(merged.updated_by, *b.public());
    }

    #[test]
    fn parent_tampering_identity_tampering_and_immutable_metadata_changes_are_rejected() {
        let key = Keypair::from_seed(&[1; 32]);
        let base = content().sign(&key).unwrap();
        let original = successor(&base, &key, "Updated");
        let mut tampered = original.clone();
        tampered.parents = vec!["a".repeat(64)];
        assert!(matches!(
            tampered.verify_signature(),
            Err(NoteRevisionError::Signature(_))
        ));
        let mut tampered = original;
        tampered.revision_id = "a".repeat(64);
        assert!(matches!(
            tampered.verify_signature(),
            Err(NoteRevisionError::RevisionIdentity)
        ));
        for field in 0..5 {
            let mut changed = content();
            changed.parents = vec![base.revision_id.clone()];
            changed.updated_at_micros = 2;
            match field {
                0 => changed.id = "different-note".into(),
                1 => changed.created_by = *Keypair::from_seed(&[2; 32]).public(),
                2 => changed.created_at_micros = 99,
                3 => changed.target_kind = Some("highlight".into()),
                _ => changed.target_id = Some("different-article".into()),
            }
            let changed = changed.sign(&key).unwrap();
            assert!(matches!(
                changed.validate_parents(std::slice::from_ref(&base)),
                Err(NoteRevisionError::ImmutableMetadata)
            ));
        }
        assert!(matches!(
            successor(&base, &key, "Updated").validate_parents(&[]),
            Err(NoteRevisionError::MissingParents)
        ));
    }

    #[test]
    fn roots_parent_lists_targets_and_body_bounds_are_validated() {
        let key = Keypair::from_seed(&[1; 32]);
        let other = Keypair::from_seed(&[2; 32]);
        assert!(matches!(
            content().sign(&other),
            Err(NoteRevisionError::RootAuthor)
        ));
        let mut valid = content();
        valid.body = "é".repeat(MAX_NOTE_BYTES / 2);
        valid.sign(&key).unwrap().verify_signature().unwrap();
        for body in [" \t\n".into(), "é".repeat(MAX_NOTE_BYTES / 2) + "é"] {
            let mut bad = content();
            bad.body = body;
            assert!(matches!(bad.sign(&key), Err(NoteRevisionError::Body)));
        }
        for parents in [
            vec!["bad".into()],
            vec!["A".repeat(64)],
            vec!["a".repeat(64); 2],
            vec!["b".repeat(64), "a".repeat(64)],
            vec!["a".repeat(64); MAX_NOTE_PARENTS + 1],
        ] {
            let mut bad = content();
            bad.parents = parents;
            assert!(matches!(bad.sign(&key), Err(NoteRevisionError::Parents)));
        }
        let mut maximum = content();
        maximum.parents = (0..MAX_NOTE_PARENTS)
            .map(|id| format!("{id:064x}"))
            .collect();
        maximum.sign(&key).unwrap().to_bytes().unwrap();
        for (kind, id) in [
            (None, Some("article".into())),
            (Some("item".into()), None),
            (Some("book".into()), Some("article".into())),
            (Some("item".into()), Some(String::new())),
        ] {
            let mut bad = content();
            bad.target_kind = kind;
            bad.target_id = id;
            assert!(matches!(bad.sign(&key), Err(NoteRevisionError::Target)));
        }
        let mut bad = content();
        bad.id.clear();
        assert!(matches!(bad.sign(&key), Err(NoteRevisionError::Identity)));
    }

    #[test]
    fn legacy_payloads_verify_unchanged_and_ancestry_does_not_rewrite_their_signature() {
        let key = Keypair::from_seed(&[1; 32]);
        #[derive(Serialize)]
        struct Legacy<'a> {
            relation: &'static str,
            id: &'a str,
            body: &'a str,
            target_kind: Option<&'a str>,
            target_id: Option<&'a str>,
            created_by: PublicKey,
            created_at_micros: i64,
            updated_at_micros: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            updated_by: Option<PublicKey>,
            #[serde(skip_serializing_if = "Option::is_none")]
            signature_version: Option<i64>,
        }
        for version in [1, 2] {
            let payload = Legacy {
                relation: "note",
                id: "legacy",
                body: "Preserved 日本語",
                target_kind: None,
                target_id: None,
                created_by: *key.public(),
                created_at_micros: 1,
                updated_at_micros: 9,
                updated_by: (version == 2).then_some(*key.public()),
                signature_version: (version == 2).then_some(2),
            };
            let signature = sign_canonical(&key, &payload).unwrap();
            let mut note = NoteRevision {
                id: "legacy".into(),
                revision_id: String::new(),
                body: payload.body.into(),
                target_kind: None,
                target_id: None,
                created_by: *key.public(),
                created_at_micros: 1,
                updated_at_micros: 9,
                updated_by: *key.public(),
                signature_version: version,
                parents: Vec::new(),
                signature,
            };
            note.revision_id = note.computed_revision_id().unwrap();
            let before = note.revision_id.clone();
            note.parents = vec!["a".repeat(64)];
            note.verify_signature().unwrap();
            assert_eq!(note.computed_revision_id().unwrap(), before);
            assert_eq!(note.signature, signature);
        }
    }

    #[test]
    fn wire_decoding_is_bounded_exact_and_rejects_unsupported_signing_versions() {
        let key = Keypair::from_seed(&[1; 32]);
        let note = content().sign(&key).unwrap();
        let mut bytes = note.to_bytes().unwrap();
        bytes.push(0);
        assert!(matches!(
            NoteRevision::from_bytes(&bytes),
            Err(NoteRevisionError::TrailingData)
        ));
        assert!(NoteRevision::from_bytes(&bytes[..bytes.len() - 2]).is_err());
        assert!(NoteRevision::from_bytes(&[]).is_err());
        assert!(matches!(
            NoteRevision::from_bytes(&vec![0; MAX_NOTE_REVISION_BYTES + 1]),
            Err(NoteRevisionError::TooLarge)
        ));
        for version in [0, 4, i64::MAX] {
            let mut unsupported = note.clone();
            unsupported.signature_version = version;
            assert!(matches!(
                unsupported.verify_signature(),
                Err(NoteRevisionError::Version)
            ));
        }
    }

    proptest! {
        #[test]
        fn distinct_offline_bodies_cannot_collide_at_the_same_time(a in "\\PC{0,128}", b in "\\PC{0,128}") {
            prop_assume!(a != b);
            let key = Keypair::from_seed(&[1; 32]);
            let base = content().sign(&key).unwrap();
            let first = successor(&base, &key, &format!("Draft {a}"));
            let second = successor(&base, &key, &format!("Draft {b}"));
            prop_assert_ne!(first.revision_id, second.revision_id);
        }
    }
}
