//! Cozo schema scripts for Carrel's documented data model.

/// All relation names created by the initial schema migration.
pub const RELATIONS: &[&str] = &[
    "peer",
    "device_authorization",
    "item",
    "item_identifier",
    "item_article",
    "item_book",
    "item_paper",
    "item_podcast_episode",
    "item_video",
    "item_shape",
    "item_content",
    "read_state",
    "item_tag",
    "item_star",
    "reading_list",
    "reading_list_item",
    "highlight",
    "note",
    "connection",
    "follow",
    "audience",
    "audience_member",
    "share",
    "share_highlight",
    "share_reaction",
    "share_reply",
    "feed",
    "entity_tombstone",
    "schema_version",
];

/// Initial schema creation script.
pub const CREATE_SCHEMA: &str = r#"
{:create peer {
    pubkey: Bytes =>
    pet_name: String?,
    self_described_name: String?,
    is_self: Bool,
    added_at: Validity,
    last_seen: Validity?
}}

{:create device_authorization {
    master_pubkey: Bytes,
    device_pubkey: Bytes =>
    authorized_at: Validity,
    revoked_at: Validity?,
    device_name: String?,
    device_cert: Bytes
}}

{:create item {
    id: String =>
    kind: String,
    title: String,
    creators: [String],
    primary_url: String?,
    published_at: Validity?,
    language: String?,
    summary: String?,
    discovered_at: Validity
}}

{:create item_identifier {
    item_id: String,
    scheme: String,
    value: String =>
    is_canonical: Bool,
    discovered_at: Validity
}}

{:create item_article {
    item_id: String =>
    feed_url: String?,
    word_count: Int?,
    estimated_read_minutes: Int?,
    site_name: String?,
    byline: String?
}}

{:create item_book {
    item_id: String =>
    isbn: String?,
    publisher: String?,
    page_count: Int?,
    cover_blob_id: String?,
    is_physical: Bool,
    edition: String?,
    pub_year: Int?
}}

{:create item_paper {
    item_id: String =>
    venue: String?,
    abstract: String?,
    citation_count: Int?,
    pdf_blob_id: String?,
    arxiv_id: String?,
    doi: String?
}}

{:create item_podcast_episode {
    item_id: String =>
    podcast_title: String,
    episode_number: Int?,
    duration_seconds: Int?,
    audio_url: String,
    audio_blob_id: String?,
    transcript_blob_id: String?
}}

{:create item_video {
    item_id: String =>
    duration_seconds: Int?,
    thumbnail_url: String?,
    thumbnail_blob_id: String?,
    platform: String,
    embed_url: String?
}}

{:create item_shape {
    item_id: String =>
    has_video_embed: Bool,
    has_audio_embed: Bool,
    is_link_roundup: Bool,
    is_long_form: Bool,
    is_short: Bool,
    has_code: Bool,
    has_math: Bool,
    detected_at: Validity
}}

{:create item_content {
    item_id: String,
    format: String =>
    blob_id: String,
    fetched_at: Validity,
    extracted_with: String?,
    byte_size: Int
}}

{:create read_state {
    item_id: String =>
    state: String,
    progress: Float?,
    progress_label: String?,
    last_position: Json?,
    updated_at: Validity
}}

{:create item_tag {
    item_id: String,
    tag: String =>
    added_at: Validity,
    retracted_at: Validity?
}}

{:create item_star {
    item_id: String =>
    starred: Bool,
    updated_at: Validity
}}

{:create reading_list {
    id: String =>
    name: String,
    description: String?,
    is_ordered: Bool,
    created_at: Validity
}}

{:create reading_list_item {
    list_id: String,
    item_id: String =>
    position: Int?,
    added_at: Validity,
    removed_at: Validity?
}}

{:create highlight {
    id: String =>
    item_id: String,
    quoted_text: String,
    location: Json,
    location_label: String?,
    color: String?,
    created_by: Bytes,
    created_at: Validity,
    signature: Bytes
}}

{:create note {
    id: String =>
    body: String,
    target_kind: String?,
    target_id: String?,
    created_by: Bytes,
    created_at: Validity,
    updated_at: Validity,
    signature: Bytes
}}

{:create connection {
    id: String =>
    source_id: String,
    target_id: String,
    relation: String,
    note: String?,
    created_by: Bytes,
    created_at: Validity,
    signature: Bytes
}}

{:create follow {
    follower: Bytes,
    followed: Bytes =>
    started_at: Validity,
    stopped_at: Validity?,
    audience_tags: [String]?
}}

{:create audience {
    id: String =>
    name: String,
    kind: String,
    created_at: Validity
}}

{:create audience_member {
    audience_id: String,
    peer_pubkey: Bytes =>
    added_at: Validity
}}

{:create share {
    id: String =>
    item_id: String,
    shared_by: Bytes,
    note: String?,
    audiences: [String],
    created_at: Validity,
    retracted_at: Validity?,
    signature: Bytes
}}

{:create share_highlight {
    share_id: String,
    highlight_id: String =>
    added_at: Validity
}}

{:create share_reaction {
    share_id: String,
    reactor: Bytes =>
    kind: String,
    created_at: Validity,
    signature: Bytes
}}

{:create share_reply {
    id: String =>
    share_id: String,
    parent_reply_id: String?,
    body: String,
    author: Bytes,
    created_at: Validity,
    signature: Bytes
}}

{:create feed {
    url: String =>
    title: String?,
    description: String?,
    last_fetched: Validity?,
    last_modified_header: String?,
    etag_header: String?,
    fetch_interval_seconds: Int,
    consecutive_failures: Int,
    folder: String?,
    auto_mark_read: Bool
}}

{:create entity_tombstone {
    id: String,
    tombstoned_at: Validity =>
    reason: String?
}}

{:create schema_version {
    version: Int =>
    applied_at: Validity,
    description: String
}}
"#;

/// Trigger script for invariants that Cozo can enforce at mutation time.
pub const CREATE_AUDIENCE_TRIGGERS: &str = r#"
::set_triggers audience
    on put {
        ?[id] :=
            _new[id, new_name, new_kind, new_created_at],
            _old[id, old_name, old_kind, old_created_at],
            new_kind != old_kind
        :assert none
    }
"#;

/// Trigger script for read-state timestamp monotonicity.
pub const CREATE_READ_STATE_TRIGGERS: &str = r#"
::set_triggers read_state
    on put {
        ?[item_id] :=
            _new[item_id, new_state, new_progress, new_progress_label, new_last_position, new_updated_at],
            _old[item_id, old_state, old_progress, old_progress_label, old_last_position, old_updated_at],
            to_int(new_updated_at) < to_int(old_updated_at)
        :assert none
    }
"#;

/// Trigger script for tombstone enforcement on items.
pub const CREATE_ITEM_TRIGGERS: &str = r#"
::set_triggers item
    on put {
        ?[id] :=
            _new[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at],
            *entity_tombstone{id: id, reason @ "NOW"}
        :assert none
    }
"#;
