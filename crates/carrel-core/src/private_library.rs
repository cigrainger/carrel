//! Signed saved-item snapshots and the explicit private fact allow-list.

use std::collections::BTreeMap;
use std::io::Cursor;

use serde::{Deserialize, Serialize};

use crate::annotation::{
    HighlightLocation,
    note::{NoteRevision, valid_revision_id},
};
use crate::identity::{
    DeviceCert, Keypair, PublicKey, RevocationCert, Signature, canonical_bytes, sign_canonical,
    verify_canonical,
};
use crate::private_state::StateRevision;

/// Maximum encoded fact at the private boundary; content blobs travel separately.
pub const MAX_PRIVATE_FACT_BYTES: usize = 2 * 1024 * 1024;
/// Maximum bytes of a referenced reading-content blob.
pub const MAX_READING_BLOB_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum bytes of cached resources in one saved-item snapshot.
pub const MAX_ITEM_RESOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// A bounded content-addressed cached resource.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlobReference {
    /// Canonical lowercase BLAKE3 digest.
    pub blob_id: String,
    /// Exact size checked before importing the blob.
    pub byte_size: u64,
}

/// One content format and its original extraction metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContentReference {
    /// Stored content format, such as `html_readable`.
    pub format: String,
    /// Verified content address and length.
    pub blob: BlobReference,
    /// Original cache timestamp; display metadata only.
    pub fetched_at_micros: i64,
    /// Extractor label, when known.
    pub extracted_with: Option<String>,
}

/// An external identifier retained across feed URL/GUID changes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ItemIdentifier {
    /// Identifier scheme, such as `url` or `feed_guid`.
    pub scheme: String,
    /// Identifier value.
    pub value: String,
    /// Whether the source considered this a canonical identifier.
    pub is_canonical: bool,
    /// Original observation time.
    pub discovered_at_micros: i64,
}

/// The supported article-specific metadata; HTTP subscription state is excluded.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArticleMetadata {
    /// Source feed URL, if known.
    pub feed_url: Option<String>,
    /// Cached article word count.
    pub word_count: Option<i64>,
    /// Estimated reading minutes.
    pub estimated_read_minutes: Option<i64>,
    /// Publisher label.
    pub site_name: Option<String>,
    /// Extracted display byline.
    pub byline: Option<String>,
}

/// A saved item's portable metadata and references, with no derived search index.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SavedItem {
    /// Stable item identity; signatures do not change it.
    pub id: String,
    /// Stored item kind.
    pub kind: String,
    /// Display title.
    pub title: String,
    /// Ordered creator display names.
    pub creators: Vec<String>,
    /// Primary external URL.
    pub primary_url: Option<String>,
    /// Publication time, if known.
    pub published_at_micros: Option<i64>,
    /// Language tag, if known.
    pub language: Option<String>,
    /// Full stored summary; rendered through the native HTML boundary.
    pub summary: Option<String>,
    /// Original library discovery time.
    pub discovered_at_micros: i64,
    /// Supported article details.
    pub article: Option<ArticleMetadata>,
    /// Sorted unique external identifiers.
    pub identifiers: Vec<ItemIdentifier>,
    /// Sorted unique content formats.
    pub contents: Vec<ContentReference>,
    /// Sorted unique cached resources, including inline images and older bodies.
    pub resources: Vec<BlobReference>,
    /// Authoritative current image references, separate from historical resources.
    /// Absent on legacy snapshots; omitted to preserve their signed payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_images: Option<Vec<BlobReference>>,
}

impl SavedItem {
    /// Normalize collection order before signing a local snapshot.
    pub fn normalize(&mut self) {
        self.identifiers
            .sort_by(|a, b| (&a.scheme, &a.value).cmp(&(&b.scheme, &b.value)));
        self.contents.sort_by(|a, b| a.format.cmp(&b.format));
        self.resources.sort_by(|a, b| a.blob_id.cmp(&b.blob_id));
        if let Some(images) = &mut self.current_images {
            images.sort_by(|a, b| a.blob_id.cmp(&b.blob_id));
        }
    }

    /// Validate bounded metadata and canonical, unambiguous references.
    pub fn validate(&self) -> Result<(), LibraryFactError> {
        identifier(&self.id)?;
        text(&self.kind, 64, false)?;
        text(&self.title, 16 * 1024, true)?;
        optional_text(&self.primary_url, 4096)?;
        optional_text(&self.language, 256)?;
        optional_text(&self.summary, 512 * 1024)?;
        if self.creators.len() > 256
            || self.identifiers.len() > 128
            || self.contents.len() > 32
            || self.resources.len() > 2048
        {
            return Err(LibraryFactError::Content);
        }
        for creator in &self.creators {
            text(creator, 4096, true)?;
        }
        if let Some(article) = &self.article {
            optional_text(&article.feed_url, 4096)?;
            optional_text(&article.site_name, 4096)?;
            optional_text(&article.byline, 16 * 1024)?;
            if article.word_count.is_some_and(|value| value < 0)
                || article
                    .estimated_read_minutes
                    .is_some_and(|value| value < 0)
            {
                return Err(LibraryFactError::Content);
            }
        }
        for id in &self.identifiers {
            text(&id.scheme, 64, false)?;
            text(&id.value, 16 * 1024, false)?;
        }
        if self
            .identifiers
            .windows(2)
            .any(|pair| (&pair[0].scheme, &pair[0].value) >= (&pair[1].scheme, &pair[1].value))
            || self
                .contents
                .windows(2)
                .any(|pair| pair[0].format >= pair[1].format)
            || self
                .resources
                .windows(2)
                .any(|pair| pair[0].blob_id >= pair[1].blob_id)
        {
            return Err(LibraryFactError::Content);
        }
        let mut sizes = BTreeMap::new();
        for content in &self.contents {
            text(&content.format, 64, false)?;
            optional_text(&content.extracted_with, 1024)?;
            check_blob(&content.blob, &mut sizes)?;
        }
        for blob in &self.resources {
            check_blob(blob, &mut sizes)?;
        }
        if let Some(images) = &self.current_images {
            if images.len() > 2048
                || images
                    .windows(2)
                    .any(|pair| pair[0].blob_id >= pair[1].blob_id)
                || !self
                    .contents
                    .iter()
                    .any(|body| body.format == "html_readable")
                || images.iter().any(|image| !self.resources.contains(image))
            {
                return Err(LibraryFactError::Content);
            }
            for image in images {
                check_blob(image, &mut sizes)?;
            }
        }
        if sizes
            .values()
            .try_fold(0u64, |sum, value| sum.checked_add(*value))
            .is_none_or(|size| size > MAX_ITEM_RESOURCE_BYTES)
        {
            return Err(LibraryFactError::Content);
        }
        Ok(())
    }

    /// Return unique references needed for a complete offline copy.
    pub fn blobs(&self) -> Vec<BlobReference> {
        let mut blobs = BTreeMap::new();
        for blob in self
            .contents
            .iter()
            .map(|content| &content.blob)
            .chain(&self.resources)
        {
            blobs.insert(blob.blob_id.clone(), blob.clone());
        }
        blobs.into_values().collect()
    }
}

/// One signed immutable saved-item revision with explicit observed predecessors.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ItemRevision {
    /// Portable item snapshot.
    pub item: SavedItem,
    /// Sorted unique predecessor digests for this item.
    pub parents: Vec<String>,
    /// Device that adopted or refreshed this snapshot.
    pub author: PublicKey,
    /// Observation time; never decides a conflict winner.
    pub at_micros: i64,
    /// Supported canonical payload version.
    pub version: u8,
    /// Content-addressed revision identity.
    pub revision_id: String,
    /// Signature over the complete snapshot and its parents.
    pub signature: Signature,
}

impl ItemRevision {
    /// Sign one local snapshot after normalizing its collections.
    pub fn sign(
        mut item: SavedItem,
        mut parents: Vec<String>,
        at_micros: i64,
        signer: &Keypair,
    ) -> Result<Self, LibraryFactError> {
        item.normalize();
        parents.sort();
        let version = if item.current_images.is_some() { 2 } else { 1 };
        let mut record = Self {
            item,
            parents,
            author: *signer.public(),
            at_micros,
            version,
            revision_id: String::new(),
            signature: Signature::from_bytes([0; 64]),
        };
        record.validate()?;
        record.signature = sign_canonical(signer, &record.payload()).map_err(signing)?;
        record.revision_id = record.computed_id()?;
        Ok(record)
    }

    /// Verify the payload, signature, and revision identity before storage.
    pub fn verify_signature(&self) -> Result<(), LibraryFactError> {
        self.validate()?;
        verify_canonical(&self.author, &self.payload(), &self.signature).map_err(signing)?;
        if self.revision_id != self.computed_id()? || self.parents.contains(&self.revision_id) {
            return Err(LibraryFactError::RevisionIdentity);
        }
        Ok(())
    }

    /// Validate the complete parent set against already verified stored records.
    pub fn validate_parents(&self, parents: &[Self]) -> Result<(), LibraryFactError> {
        if parents.len() != self.parents.len() {
            return Err(LibraryFactError::Parents);
        }
        for (expected, parent) in self.parents.iter().zip(parents) {
            parent.verify_signature()?;
            if expected != &parent.revision_id || parent.item.id != self.item.id {
                return Err(LibraryFactError::Parents);
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), LibraryFactError> {
        self.item.validate()?;
        if !matches!(self.version, 1 | 2)
            || (self.version == 1 && self.item.current_images.is_some())
            || self.parents.len() > 1024
            || self.parents.iter().any(|id| !valid_revision_id(id))
            || self.parents.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(LibraryFactError::Parents);
        }
        Ok(())
    }

    fn payload(&self) -> (&str, u8, &SavedItem, &[String], PublicKey, i64) {
        (
            "carrel-self-item-v1",
            self.version,
            &self.item,
            &self.parents,
            self.author,
            self.at_micros,
        )
    }

    fn computed_id(&self) -> Result<String, LibraryFactError> {
        Ok(blake3::hash(
            &canonical_bytes(&("carrel-self-item-id-v1", self.payload(), self.signature))
                .map_err(signing)?,
        )
        .to_hex()
        .to_string())
    }
}

/// Resolve external metadata deterministically without merging reading intent.
pub fn resolve_item_heads(heads: &[ItemRevision]) -> Result<Option<SavedItem>, LibraryFactError> {
    let Some(winner) = heads
        .iter()
        .max_by(|a, b| a.revision_id.cmp(&b.revision_id))
    else {
        return Ok(None);
    };
    let mut item = winner.item.clone();
    let mut identifiers: BTreeMap<(String, String), ItemIdentifier> = BTreeMap::new();
    let mut contents: BTreeMap<String, ContentReference> = BTreeMap::new();
    let mut resources: BTreeMap<String, BlobReference> = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    for head in heads {
        head.verify_signature()?;
        if head.item.id != item.id {
            return Err(LibraryFactError::Parents);
        }
        for blob in head.item.blobs() {
            if sizes
                .insert(blob.blob_id, blob.byte_size)
                .is_some_and(|size| size != blob.byte_size)
            {
                return Err(LibraryFactError::Content);
            }
        }
        for id in &head.item.identifiers {
            identifiers
                .entry((id.scheme.clone(), id.value.clone()))
                .and_modify(|known| {
                    known.is_canonical |= id.is_canonical;
                    known.discovered_at_micros =
                        known.discovered_at_micros.min(id.discovered_at_micros);
                })
                .or_insert_with(|| id.clone());
        }
        for content in &head.item.contents {
            contents
                .entry(content.format.clone())
                .and_modify(|known| {
                    if (
                        &content.blob.blob_id,
                        content.fetched_at_micros,
                        &content.extracted_with,
                    ) > (
                        &known.blob.blob_id,
                        known.fetched_at_micros,
                        &known.extracted_with,
                    ) {
                        *known = content.clone();
                    }
                })
                .or_insert_with(|| content.clone());
        }
        // Body references already live in `contents` and immutable revision
        // history. Adding them to resources changes a single-head projection
        // on every adoption and accumulates old bodies in future snapshots.
        for blob in &head.item.resources {
            if resources
                .get(&blob.blob_id)
                .is_some_and(|known| known.byte_size != blob.byte_size)
            {
                return Err(LibraryFactError::Content);
            }
            resources.insert(blob.blob_id.clone(), blob.clone());
        }
    }
    item.identifiers = identifiers.into_values().collect();
    item.contents = contents.into_values().collect();
    // A historical resource is not a current-image grant. Merge only scoped
    // references signed for the exact selected readable body.
    item.current_images = item
        .contents
        .iter()
        .find(|body| body.format == "html_readable")
        .and_then(|body| {
            let matching: Vec<_> = heads
                .iter()
                .filter(|head| {
                    head.item.contents.iter().any(|content| {
                        content.format == "html_readable"
                            && content.blob.blob_id == body.blob.blob_id
                    })
                })
                .filter_map(|head| head.item.current_images.as_ref())
                .collect();
            if matching.is_empty() {
                None
            } else {
                Some(
                    matching
                        .into_iter()
                        .flatten()
                        .map(|image| (image.blob_id.clone(), image.clone()))
                        .collect::<BTreeMap<_, _>>()
                        .into_values()
                        .collect(),
                )
            }
        });
    item.resources = resources.into_values().collect();
    Ok(Some(item))
}

/// Immutable highlight, using the exact original canonical signing payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrivateHighlight {
    /// Stable annotation identity.
    pub id: String,
    /// Target item.
    pub item_id: String,
    /// Complete selected text.
    pub quoted_text: String,
    /// Original selector.
    pub location: HighlightLocation,
    /// Human-readable location.
    pub location_label: Option<String>,
    /// Optional color token.
    pub color: Option<String>,
    /// Original operational author.
    pub created_by: PublicKey,
    /// Original creation timestamp.
    pub created_at_micros: i64,
    /// Original signature; adoption never re-signs annotations.
    pub signature: Signature,
}

impl PrivateHighlight {
    /// Verify bounded content and the original persisted payload.
    pub fn verify_signature(&self) -> Result<(), LibraryFactError> {
        identifier(&self.id)?;
        identifier(&self.item_id)?;
        text(&self.quoted_text, 128 * 1024, false)?;
        optional_text(&self.location_label, 4096)?;
        optional_text(&self.color, 128)?;
        self.location
            .validate()
            .map_err(|_| LibraryFactError::Content)?;
        if self.location.quoted_text() != self.quoted_text {
            return Err(LibraryFactError::Content);
        }
        #[derive(Serialize)]
        struct Payload<'a> {
            relation: &'static str,
            id: &'a str,
            item_id: &'a str,
            quoted_text: &'a str,
            location: &'a HighlightLocation,
            location_label: Option<&'a str>,
            color: Option<&'a str>,
            created_by: PublicKey,
            created_at_micros: i64,
        }
        verify_canonical(
            &self.created_by,
            &Payload {
                relation: "highlight",
                id: &self.id,
                item_id: &self.item_id,
                quoted_text: &self.quoted_text,
                location: &self.location,
                location_label: self.location_label.as_deref(),
                color: self.color.as_deref(),
                created_by: self.created_by,
                created_at_micros: self.created_at_micros,
            },
            &self.signature,
        )
        .map_err(signing)
    }
}

/// Signed permanent annotation removal, preserving the existing payload format.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrivateRemoval {
    /// Removed entity identity.
    pub id: String,
    /// Supported annotation kind.
    pub kind: String,
    /// Operational device authorizing removal.
    pub removed_by: PublicKey,
    /// Original removal timestamp.
    pub removed_at_micros: i64,
    /// Original canonical signature.
    pub signature: Signature,
}

impl PrivateRemoval {
    /// Verify bounded fields and the original annotation-removal signature.
    pub fn verify_signature(&self) -> Result<(), LibraryFactError> {
        identifier(&self.id)?;
        if !matches!(self.kind.as_str(), "highlight" | "note") {
            return Err(LibraryFactError::Content);
        }
        #[derive(Serialize)]
        struct Payload<'a> {
            relation: &'static str,
            id: &'a str,
            kind: &'a str,
            removed_by: PublicKey,
            removed_at_micros: i64,
        }
        verify_canonical(
            &self.removed_by,
            &Payload {
                relation: "annotation_removal",
                id: &self.id,
                kind: &self.kind,
                removed_by: self.removed_by,
                removed_at_micros: self.removed_at_micros,
            },
            &self.signature,
        )
        .map_err(signing)
    }
}

/// The complete explicit allow-list for private self synchronization.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", content = "fact", rename_all = "snake_case")]
// Pages are bounded; retain the existing value variants rather than allocate
// another box for every saved-item fact when its optional image scope grows.
#[allow(clippy::large_enum_variant)]
pub enum PrivateFact {
    /// Original verified social history for a participant's own desktops only.
    Social(Box<crate::sharing::ShareFact>),
    /// Saved metadata and signed cache references.
    Item(ItemRevision),
    /// Explicit reading, star, position, or subscription action.
    State(StateRevision),
    /// One original signed note version.
    Note(NoteRevision),
    /// Immutable highlight.
    Highlight(PrivateHighlight),
    /// Permanent annotation removal.
    Removal(PrivateRemoval),
    /// Master-signed device authorization.
    Certificate(DeviceCert),
    /// Master-signed permanent revocation.
    Revocation(RevocationCert),
}

impl PrivateFact {
    /// Explicit wire kind; social history still requires the participant-only self wrapper.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Social(_) => "social",
            Self::Item(_) => "item",
            Self::State(_) => "state",
            Self::Note(_) => "note",
            Self::Highlight(_) => "highlight",
            Self::Removal(_) => "removal",
            Self::Certificate(_) => "certificate",
            Self::Revocation(_) => "revocation",
        }
    }

    /// Verify the original signature and each fact's bounded content.
    pub fn verify(&self) -> Result<(), LibraryFactError> {
        match self {
            Self::Social(fact) => fact.verify().map_err(signing),
            Self::Item(fact) => fact.verify_signature(),
            Self::Highlight(fact) => fact.verify_signature(),
            Self::Removal(fact) => fact.verify_signature(),
            Self::State(fact) => fact.verify().map_err(signing),
            Self::Note(fact) => fact.verify_signature().map_err(signing),
            Self::Certificate(fact) => {
                optional_text(&fact.device_name, 4096)?;
                fact.verify().map_err(signing)
            }
            Self::Revocation(fact) => fact.verify().map_err(signing),
        }
    }

    /// Remove unsigned legacy ancestry before computing a portable wire identity.
    pub fn into_portable(self) -> Self {
        match self {
            Self::Note(fact) => Self::Note(fact.into_portable()),
            fact => fact,
        }
    }

    /// Bounded canonical private wire bytes, preserving original signatures.
    pub fn to_bytes(&self) -> Result<Vec<u8>, LibraryFactError> {
        self.verify()?;
        let bytes = canonical_bytes(&self.clone().into_portable()).map_err(signing)?;
        if bytes.len() > MAX_PRIVATE_FACT_BYTES {
            return Err(LibraryFactError::TooLarge);
        }
        Ok(bytes)
    }

    /// Decode exactly one bounded fact and verify it before using any field.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LibraryFactError> {
        if bytes.len() > MAX_PRIVATE_FACT_BYTES {
            return Err(LibraryFactError::TooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        let fact: Self =
            ciborium::de::from_reader(&mut cursor).map_err(|_| LibraryFactError::Decode)?;
        if cursor.position() != bytes.len() as u64 {
            return Err(LibraryFactError::Decode);
        }
        fact.verify()?;
        Ok(fact.into_portable())
    }

    /// Key binds the allow-list kind and exact portable bytes, with no private text.
    pub fn key(&self) -> Result<String, LibraryFactError> {
        Ok(format!(
            "{}/{}",
            self.kind(),
            blake3::hash(&self.to_bytes()?).to_hex()
        ))
    }
}

/// Invalid private library or wire data; messages never contain fact bodies.
#[derive(Debug, thiserror::Error)]
pub enum LibraryFactError {
    /// Metadata or cached references exceed supported bounds.
    #[error("invalid private library content")]
    Content,
    /// The signed causal parent set is invalid or incomplete.
    #[error("invalid private library ancestry")]
    Parents,
    /// A revision digest does not describe its signed payload.
    #[error("invalid private item revision identity")]
    RevisionIdentity,
    /// Signature or canonical encoding failed.
    #[error("private fact signature or encoding failed: {0}")]
    Signature(String),
    /// Encoded metadata is too large for the private boundary.
    #[error("private fact exceeds the encoded size limit")]
    TooLarge,
    /// Malformed, truncated, trailing, or unsupported private data.
    #[error("invalid private fact encoding")]
    Decode,
}

fn signing(error: impl std::fmt::Display) -> LibraryFactError {
    LibraryFactError::Signature(error.to_string())
}
fn identifier(value: &str) -> Result<(), LibraryFactError> {
    text(value, 256, false)
}
fn text(value: &str, limit: usize, empty: bool) -> Result<(), LibraryFactError> {
    if value.len() > limit || (!empty && value.is_empty()) {
        Err(LibraryFactError::Content)
    } else {
        Ok(())
    }
}
fn optional_text(value: &Option<String>, limit: usize) -> Result<(), LibraryFactError> {
    value
        .as_deref()
        .map_or(Ok(()), |value| text(value, limit, true))
}
fn check_blob<'a>(
    blob: &'a BlobReference,
    sizes: &mut BTreeMap<&'a str, u64>,
) -> Result<(), LibraryFactError> {
    if !valid_revision_id(&blob.blob_id) || blob.byte_size > MAX_READING_BLOB_BYTES {
        return Err(LibraryFactError::Content);
    }
    if sizes
        .insert(&blob.blob_id, blob.byte_size)
        .is_some_and(|old| old != blob.byte_size)
    {
        return Err(LibraryFactError::Content);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn item(title: &str, hash: char) -> SavedItem {
        SavedItem {
            id: "article".into(),
            kind: "article".into(),
            title: title.into(),
            creators: vec!["文".into()],
            primary_url: Some("https://example.com/article".into()),
            published_at_micros: None,
            language: Some("fr".into()),
            summary: Some("Café".into()),
            discovered_at_micros: 1,
            article: None,
            identifiers: Vec::new(),
            contents: vec![ContentReference {
                format: "html_readable".into(),
                blob: BlobReference {
                    blob_id: hash.to_string().repeat(64),
                    byte_size: 10,
                },
                fetched_at_micros: 2,
                extracted_with: None,
            }],
            resources: vec![BlobReference {
                blob_id: "f".repeat(64),
                byte_size: 5,
            }],
            current_images: None,
        }
    }

    #[test]
    fn wire_is_exact_bounded_verified_and_has_an_explicit_allow_list() {
        let key = Keypair::from_seed(&[71; 32]);
        let original = PrivateFact::Item(
            ItemRevision::sign(item("Café 文", 'a'), vec![], i64::MAX, &key).unwrap(),
        );
        let bytes = original.to_bytes().unwrap();
        assert_eq!(PrivateFact::from_bytes(&bytes).unwrap(), original);
        assert!(original.key().unwrap().starts_with("item/"));
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(matches!(
            PrivateFact::from_bytes(&trailing),
            Err(LibraryFactError::Decode)
        ));
        assert!(PrivateFact::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        assert!(PrivateFact::from_bytes(&[]).is_err());
        assert!(matches!(
            PrivateFact::from_bytes(&vec![0; MAX_PRIVATE_FACT_BYTES + 1]),
            Err(LibraryFactError::TooLarge)
        ));
        let mut unsupported = Vec::new();
        ciborium::ser::into_writer(&BTreeMap::from([("kind", "share")]), &mut unsupported).unwrap();
        assert!(matches!(
            PrivateFact::from_bytes(&unsupported),
            Err(LibraryFactError::Decode)
        ));
        let PrivateFact::Item(mut forged) = original else {
            unreachable!()
        };
        forged.item.summary = Some("replacement".into());
        let encoded = canonical_bytes(&PrivateFact::Item(forged)).unwrap();
        assert!(PrivateFact::from_bytes(&encoded).is_err());
    }

    #[test]
    fn legacy_archives_keep_v1_signatures_and_current_image_grants_follow_only_the_selected_body() {
        let key = Keypair::from_seed(&[92; 32]);
        let legacy = ItemRevision::sign(item("Legacy", 'a'), vec![], 1, &key).unwrap();
        assert_eq!(legacy.version, 1);
        assert!(
            !serde_json::to_string(&legacy.item)
                .unwrap()
                .contains("current_images")
        );
        let encoded = PrivateFact::Item(legacy.clone()).to_bytes().unwrap();
        assert_eq!(
            PrivateFact::from_bytes(&encoded).unwrap(),
            PrivateFact::Item(legacy.clone())
        );
        let mut scoped = item("Scoped", 'b');
        scoped.current_images = Some(scoped.resources.clone());
        let scoped = ItemRevision::sign(scoped, vec![], 2, &key).unwrap();
        assert_eq!(scoped.version, 2);
        PrivateFact::Item(scoped.clone()).to_bytes().unwrap();
        let mut refreshed = item("New body", 'c');
        refreshed.contents[0].fetched_at_micros = 3;
        let refreshed = ItemRevision::sign(refreshed, vec![], 3, &key).unwrap();
        let merged = resolve_item_heads(&[legacy.clone(), scoped.clone(), refreshed])
            .unwrap()
            .unwrap();
        assert!(merged.current_images.is_none());
        let merged = resolve_item_heads(&[legacy, scoped.clone()])
            .unwrap()
            .unwrap();
        assert_eq!(merged.current_images, scoped.item.current_images);
        let mut forged = scoped;
        forged.version = 1;
        assert!(PrivateFact::Item(forged).to_bytes().is_err());
    }

    #[test]
    fn parents_bind_item_identity_and_all_signed_fields_are_protected() {
        let key = Keypair::from_seed(&[72; 32]);
        let root = ItemRevision::sign(item("original", 'a'), vec![], 0, &key).unwrap();
        let child =
            ItemRevision::sign(item("new", 'b'), vec![root.revision_id.clone()], -1, &key).unwrap();
        child.validate_parents(std::slice::from_ref(&root)).unwrap();
        assert!(child.validate_parents(&[]).is_err());
        let mut other = root.item.clone();
        other.id = "other".into();
        let other = ItemRevision::sign(other, vec![], 0, &key).unwrap();
        assert!(child.validate_parents(&[other]).is_err());
        assert!(
            ItemRevision::sign(
                item("duplicate", 'a'),
                vec![root.revision_id.clone(); 2],
                0,
                &key
            )
            .is_err()
        );
        for field in 0..6 {
            let mut forged = child.clone();
            match field {
                0 => forged.item.title.push('!'),
                1 => forged.parents.clear(),
                2 => forged.at_micros += 1,
                3 => forged.version += 1,
                4 => forged.revision_id = "0".repeat(64),
                _ => forged.item.contents[0].blob.byte_size += 1,
            }
            assert!(forged.verify_signature().is_err());
        }
    }

    #[test]
    fn single_head_is_idempotent_and_invalid_cache_references_are_rejected() {
        let key = Keypair::from_seed(&[73; 32]);
        let record = ItemRevision::sign(item("original", 'a'), vec![], 0, &key).unwrap();
        assert_eq!(
            resolve_item_heads(std::slice::from_ref(&record)).unwrap(),
            Some(record.item.clone())
        );
        assert_eq!(record.item.blobs().len(), 2);
        for field in 0..5 {
            let mut invalid = record.item.clone();
            match field {
                0 => invalid.contents[0].blob.blob_id = "A".repeat(64),
                1 => invalid.contents[0].blob.byte_size = MAX_READING_BLOB_BYTES + 1,
                2 => invalid.resources.push(invalid.resources[0].clone()),
                3 => invalid.resources[0].blob_id = invalid.contents[0].blob.blob_id.clone(),
                _ => invalid.title = "x".repeat(16 * 1024 + 1),
            }
            assert!(ItemRevision::sign(invalid, vec![], 0, &key).is_err());
        }
        let mut inconsistent = record.item.clone();
        inconsistent.contents[0].blob.byte_size += 1;
        let other = ItemRevision::sign(inconsistent, vec![], 0, &key).unwrap();
        assert!(resolve_item_heads(&[record, other]).is_err());
    }

    proptest! {
        #[test]
        fn metadata_merge_is_commutative_and_duplicate_safe(a_time in any::<i64>(), b_time in any::<i64>(), a_title in ".{0,80}", b_title in ".{0,80}") {
            let a = ItemRevision::sign(item(&a_title, 'a'), vec![], a_time, &Keypair::from_seed(&[74;32])).unwrap();
            let b = ItemRevision::sign(item(&b_title, 'b'), vec![], b_time, &Keypair::from_seed(&[75;32])).unwrap();
            let expected = resolve_item_heads(&[a.clone(),b.clone()]).unwrap();
            prop_assert_eq!(resolve_item_heads(&[b.clone(),a.clone()]).unwrap(), expected.clone());
            prop_assert_eq!(resolve_item_heads(&[a,b.clone(),b]).unwrap(), expected);
        }
    }
}
