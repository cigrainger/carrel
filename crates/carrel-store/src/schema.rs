//! Cozo schema scripts for Carrel's documented data model.

/// All base relation names in the current schema.
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
    "item_search",
    "note_legacy_v2",
    "annotation_removal",
    "self_identity",
    "device_certificate",
    "device_revocation",
    "device_local_block",
    "note_legacy_v3",
    "note_incoming",
    "self_state",
    "self_state_incoming",
    "self_state_outbox",
    "self_item_revision",
    "self_fact_outbox",
    "self_fact_history",
    "self_library_incoming",
    "self_item_dirty",
    "self_blob_incoming",
    "self_document",
    "self_peer_address",
];

/// Portable item history, immutable wire queue, and resumable source adoption.
pub const PRIVATE_LIBRARY_RELATIONS: &[(&str, &str)] = &[
    (
        "self_item_revision",
        ":create self_item_revision {id: String, revision_id: String => encoded: Bytes, parents: [String]}",
    ),
    (
        "self_fact_outbox",
        ":create self_fact_outbox {key: String => encoded: Bytes}",
    ),
    (
        "self_fact_history",
        ":create self_fact_history {key: String => encoded: Bytes}",
    ),
    (
        "self_library_incoming",
        ":create self_library_incoming {key: String => encoded: Bytes, kind: String, target: String?}",
    ),
    (
        "self_item_dirty",
        ":create self_item_dirty {id: String => token: String}",
    ),
    (
        "self_blob_incoming",
        ":create self_blob_incoming {blob_id: String => byte_size: Int}",
    ),
    (
        "self_document",
        ":create self_document {singleton: Int => namespace: Bytes, capability: Bytes}",
    ),
    (
        "self_peer_address",
        ":create self_peer_address {device_pubkey: Bytes => address: Bytes}",
    ),
];

/// Item ancestry and verified incoming/outgoing bytes cannot be replaced.
pub const PRIVATE_LIBRARY_GUARDS: &[&str] = &[
    r#"::set_triggers self_document on put {
        invalid[slot] := _new[slot, namespace, capability], slot != 0
        invalid[slot] := _new[slot, namespace, capability], _old[slot, old_namespace, old_capability],
            [namespace, capability] != [old_namespace, old_capability]
        ?[slot] := invalid[slot] :assert none
    } on rm { ?[slot] := _old[slot, namespace, capability] :assert none }"#,
    r#"::set_triggers self_blob_incoming on put {
        ?[id] := _new[id, size], _old[id, old_size], size != old_size :assert none
    }"#,
    r#"::set_triggers self_fact_history on put {
        ?[key] := _new[key, encoded], _old[key, old_encoded], encoded != old_encoded :assert none
    } on rm { ?[key] := _old[key, encoded] :assert none }"#,
    r#"::set_triggers self_item_revision on put {
        invalid[id, rev] := _new[id, rev, encoded, parents], _old[id, rev, old_encoded, old_parents],
            [encoded, parents] != [old_encoded, old_parents]
        invalid[id, rev] := _new[id, rev, encoded, parents], parent in parents, not *self_item_revision{id, revision_id: parent}
        invalid[id, rev] := _new[id, rev, encoded, parents], parent in parents, parent == rev
        invalid[id, rev] := _new[id, rev, encoded, parents], *entity_tombstone{id @ "END"}
        ?[id, rev] := invalid[id, rev] :assert none
    } on rm { ?[id, rev] := _old[id, rev, encoded, parents] :assert none }"#,
    r#"::set_triggers self_fact_outbox on put {
        ?[key] := _new[key, encoded], _old[key, old_encoded], encoded != old_encoded :assert none
    }"#,
    r#"::set_triggers self_library_incoming on put {
        ?[key] := _new[key, encoded, kind, target], _old[key, old_encoded, old_kind, old_target],
            [encoded, kind, target] != [old_encoded, old_kind, old_target] :assert none
    }"#,
];

/// Every source metadata write gets a new capture token; incoming projections
/// remove their token in the same transaction as the signed history and queue.
pub const PRIVATE_ITEM_DIRTY_TRIGGERS: &[&str] = &[
    r#"::set_triggers item on put {
        ?[id] := _new[id, kind, title, creators, url, published, language, summary, discovered], *entity_tombstone{id @ "END"}
        :assert none
    } on put {
        ?[id, token] := _new[id, kind, title, creators, url, published, language, summary, discovered], token = to_string(rand_uuid_v4())
        :put self_item_dirty {id => token}
    }"#,
    r#"::set_triggers item_article on put {
        ?[id, token] := _new[id, feed, words, minutes, site, byline], token = to_string(rand_uuid_v4())
        :put self_item_dirty {id => token}
    }"#,
    r#"::set_triggers item_identifier on put {
        ?[id, token] := _new[id, scheme, value, canonical, discovered], token = to_string(rand_uuid_v4())
        :put self_item_dirty {id => token}
    }"#,
    r#"::set_triggers item_content on put {
        ?[id, token] := _new[id, format, blob, fetched, extractor, size], token = to_string(rand_uuid_v4())
        :put self_item_dirty {id => token}
    }"#,
];

/// Individually resumable private history and mirror work-queue relations.
pub const PRIVATE_STATE_RELATIONS: &[(&str, &str)] = &[
    (
        "self_state",
        ":create self_state {register: String, revision_id: String => encoded: Bytes, parents: [String], kind: String, target: String}",
    ),
    (
        "self_state_incoming",
        ":create self_state_incoming {register: String, revision_id: String => encoded: Bytes, parents: [String], kind: String, target: String}",
    ),
    (
        "self_state_outbox",
        ":create self_state_outbox {register: String, revision_id: String}",
    ),
];

/// Journal ancestry and signed bytes cannot be replaced or erased.
pub const PRIVATE_STATE_GUARDS: &[&str] = &[
    r#"::set_triggers self_state on put {
        invalid[reg, rev] := _new[reg, rev, encoded, parents, kind, target],
            _old[reg, rev, old_encoded, old_parents, old_kind, old_target],
            [encoded, parents, kind, target] != [old_encoded, old_parents, old_kind, old_target]
        invalid[reg, rev] := _new[reg, rev, encoded, parents, kind, target], parent in parents,
            not *self_state{register: reg, revision_id: parent}
        invalid[reg, rev] := _new[reg, rev, encoded, parents, kind, target], parent in parents, parent == rev
        invalid[reg, rev] := _new[reg, rev, encoded, parents, kind, target], kind != 'subscription', not *item{id: target}
        invalid[reg, rev] := _new[reg, rev, encoded, parents, kind, target], kind != 'subscription', *entity_tombstone{id: target @ "END"}
        ?[reg, rev] := invalid[reg, rev]
        :assert none
    } on rm { ?[reg, rev] := _old[reg, rev, encoded, parents, kind, target] :assert none }"#,
    r#"::set_triggers self_state_incoming on put {
        ?[reg, rev] := _new[reg, rev, encoded, parents, kind, target],
            _old[reg, rev, old_encoded, old_parents, old_kind, old_target],
            [encoded, parents, kind, target] != [old_encoded, old_parents, old_kind, old_target]
        :assert none
    }"#,
    r#"::set_triggers read_state on put {
        managed[id] := *self_state{target: id, kind}, (kind == 'read' || kind == 'position')
        ?[id] := _new[id, state, progress, label, position, at], _old[id, old_state, old_progress, old_label, old_position, old_at],
            to_int(at) < to_int(old_at), not managed[id]
        :assert none
    }"#,
];

/// Staging table renamed to `note` after preserving every old signed revision.
pub const CREATE_CAUSAL_NOTES: &str = r#"
    :create note_v5 {
        id: String, revision_id: String => body: String, target_kind: String?, target_id: String?,
        created_by: Bytes, created_at: Validity, updated_at: Validity, updated_by: Bytes,
        signature_version: Int, parents: [String], signature: Bytes
    }
"#;

/// Verified private revisions waiting for parent or target dependencies.
pub const CREATE_INCOMING_NOTES: &str = r#"
    :create note_incoming {
        id: String, revision_id: String => encoded: Bytes, parents: [String], target_kind: String?, target_id: String?
    }
"#;

/// Reject replacement, missing parents, changed metadata, and resurrection.
pub const CREATE_CAUSAL_NOTE_GUARDS: &str = r#"
    ::set_triggers note on put {
        alive_item[id] := *item{id}, not *entity_tombstone{id}
        alive_highlight[id] := *highlight{id, item_id}, alive_item[item_id], not *entity_tombstone{id}
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig], *entity_tombstone{id}
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig], kind = 'item', not alive_item[target]
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig], kind = 'highlight', not alive_highlight[target]
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig],
            (is_null(kind) != is_null(target) || (!is_null(kind) && kind != 'item' && kind != 'highlight'))
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig], parent in parents, parent == rev
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig], parent in parents, not *note{id, revision_id: parent}
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig],
            *note{id, target_kind: old_kind, target_id: old_target, created_by: old_creator, created_at: old_created},
            [kind, target, creator, created] != [old_kind, old_target, old_creator, old_created]
        invalid[id] := _new[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig],
            _old[id, rev, old_body, old_kind, old_target, old_creator, old_created, old_updated, old_updater, old_version, old_parents, old_sig],
            [body, kind, target, creator, created, updated, updater, version, parents, sig] !=
            [old_body, old_kind, old_target, old_creator, old_created, old_updated, old_updater, old_version, old_parents, old_sig]
        ?[id] := invalid[id]
        :assert none
    } on rm { ?[id, rev] := _old[id, rev, body, kind, target, creator, created, updated, updater, version, parents, sig] :assert none }
"#;

/// Staging retries cannot replace an already verified immutable message.
pub const CREATE_INCOMING_NOTE_GUARDS: &str = r#"
    ::set_triggers note_incoming on put {
        ?[id, rev] := _new[id, rev, encoded, parents, kind, target], _old[id, rev, old_encoded, old_parents, old_kind, old_target],
            [encoded, parents, kind, target] != [old_encoded, old_parents, old_kind, old_target]
        :assert none
    }
"#;

/// Individually resumable relations for verified self-device trust.
pub const SELF_IDENTITY_RELATIONS: &[(&str, &str)] = &[
    (
        "self_identity",
        ":create self_identity { singleton: Int => master_pubkey: Bytes, device_pubkey: Bytes }",
    ),
    (
        "device_certificate",
        ":create device_certificate { master_pubkey: Bytes, device_pubkey: Bytes => device_cert: Bytes }",
    ),
    (
        "device_revocation",
        ":create device_revocation { master_pubkey: Bytes, device_pubkey: Bytes, revoked_at: Validity => revocation_cert: Bytes }",
    ),
    (
        "device_local_block",
        ":create device_local_block { master_pubkey: Bytes, device_pubkey: Bytes => reason: String }",
    ),
];

/// Guard immutable identity bindings, certificates, revocations, and blocks.
pub const SELF_IDENTITY_GUARDS: &[&str] = &[
    r#"::set_triggers self_identity on put {
        invalid[slot] := _new[slot, master, device], slot != 0
        invalid[slot] := _new[slot, master, device], master == device
        invalid[slot] := _new[slot, master, device], _old[slot, old_master, old_device], [master, device] != [old_master, old_device]
        ?[slot] := invalid[slot]
        :assert none
    } on rm { ?[slot] := _old[slot, master, device] :assert none }"#,
    r#"::set_triggers device_certificate on put {
        ?[master, device] := _new[master, device, cert], _old[master, device, old_cert], cert != old_cert
        :assert none
    } on rm { ?[master, device] := _old[master, device, cert] :assert none }"#,
    r#"::set_triggers device_revocation on put {
        ?[master, device, at] := _new[master, device, at, cert], _old[master, device, at, old_cert], cert != old_cert
        :assert none
    } on rm { ?[master, device, at] := _old[master, device, at, cert] :assert none }"#,
    r#"::set_triggers device_local_block on put {
        ?[master, device] := _new[master, device, reason], _old[master, device, old_reason], reason != old_reason
        :assert none
    } on rm { ?[master, device] := _old[master, device, reason] :assert none }"#,
];

/// Staging relation for migration 3, renamed to `note` after legacy copy.
pub const CREATE_NOTE_REVISIONS: &str = r#"
    :create note_v3 {
        id: String, updated_at: Validity =>
        body: String, target_kind: String?, target_id: String?,
        created_by: Bytes, created_at: Validity, updated_by: Bytes,
        signature_version: Int, signature: Bytes
    }
"#;

/// Private signed removal history, alongside permanent entity tombstones.
pub const CREATE_ANNOTATION_REMOVALS: &str = r#"
    :create annotation_removal {
        id: String, removed_at: Validity => kind: String, removed_by: Bytes, signature: Bytes
    }
"#;

/// Reject highlight mutation and writes to removed entities or targets.
pub const CREATE_HIGHLIGHT_TRIGGERS: &str = r#"
    ::set_triggers highlight on put {
        invalid[id] := _new[id, item_id, quoted_text, location, label, color, author, at, signature], *entity_tombstone{id}
        invalid[id] := _new[id, item_id, quoted_text, location, label, color, author, at, signature], *entity_tombstone{id: item_id}
        invalid[id] := _new[id, item_id, quoted_text, location, label, color, author, at, signature], not *item{id: item_id}
        invalid[id] := _new[id, item_id, quoted_text, location, label, color, author, at, signature],
            _old[id, old_item, old_quote, old_location, old_label, old_color, old_author, old_at, old_signature],
            [item_id, quoted_text, location, label, color, author, at, signature] !=
            [old_item, old_quote, old_location, old_label, old_color, old_author, old_at, old_signature]
        ?[id] := invalid[id]
        :assert none
    }
"#;

/// Keep note revisions immutable and their original metadata stable.
pub const CREATE_NOTE_TRIGGERS: &str = r#"
    ::set_triggers note on put {
        alive_item[id] := *item{id}, not *entity_tombstone{id}
        alive_highlight[id] := *highlight{id, item_id}, alive_item[item_id], not *entity_tombstone{id}
        invalid[id] := _new[id, revision, body, kind, target, author, created, updater, version, signature], *entity_tombstone{id}
        invalid[id] := _new[id, revision, body, kind, target, author, created, updater, version, signature], kind = 'item', not alive_item[target]
        invalid[id] := _new[id, revision, body, kind, target, author, created, updater, version, signature], kind = 'highlight', not alive_highlight[target]
        invalid[id] := _new[id, revision, body, kind, target, author, created, updater, version, signature],
            (is_null(kind) != is_null(target) || (!is_null(kind) && kind != 'item' && kind != 'highlight'))
        invalid[id] := _new[id, revision, body, kind, target, author, created, updater, version, signature],
            *note{id, updated_at: previous, target_kind: old_kind, target_id: old_target, created_by: old_author, created_at: old_created},
            previous != revision, [kind, target, author, created] != [old_kind, old_target, old_author, old_created]
        invalid[id] := _new[id, revision, body, kind, target, author, created, updater, version, signature],
            _old[id, revision, old_body, old_kind, old_target, old_author, old_created, old_updater, old_version, old_signature],
            [body, kind, target, author, created, updater, version, signature] !=
            [old_body, old_kind, old_target, old_author, old_created, old_updater, old_version, old_signature]
        ?[id] := invalid[id]
        :assert none
    }
"#;

/// Derived readable-text cache, created by migration 2.
pub const CREATE_SEARCH_SCHEMA: &str = r#"
    :create item_search { item_id: String => blob_id: String, text: String, extractor_version: Int }
"#;

/// Title index, maintained transactionally by Cozo when items change.
pub const CREATE_TITLE_SEARCH_INDEX: &str = r#"
    ::fts create item:title_words { extractor: title, tokenizer: Simple, filters: [Lowercase] }
"#;

/// Body index, maintained transactionally by Cozo when derived text changes.
pub const CREATE_BODY_SEARCH_INDEX: &str = r#"
    ::fts create item_search:body_words { extractor: text, tokenizer: Simple, filters: [Lowercase] }
"#;

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
