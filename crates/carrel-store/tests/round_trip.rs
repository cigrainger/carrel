use std::collections::BTreeMap;

use carrel_store::Store;
use cozo::{DataValue, JsonData};
use serde_json::json;

#[test]
fn round_trips_one_row_through_each_schema_relation() {
    let store = Store::open_in_memory().unwrap();
    store.migrate().unwrap();

    let bytes = DataValue::Bytes(vec![7; 32]);
    let signature = DataValue::Bytes(vec![9; 64]);
    let json_value = DataValue::Json(JsonData(json!({"kind": "quote", "exact": "sample"})));

    store
        .query_with_params(
            r#"
            {
                ?[pubkey, pet_name, self_described_name, is_self, added_at, last_seen] :=
                    pubkey = $peer,
                    pet_name = 'Chris',
                    self_described_name = 'Chris',
                    is_self = true,
                    added_at = 'ASSERT',
                    last_seen = 'ASSERT'
                :put peer {pubkey => pet_name, self_described_name, is_self, added_at, last_seen}
            }
            {
                ?[master_pubkey, device_pubkey, authorized_at, revoked_at, device_name, device_cert] :=
                    master_pubkey = $peer,
                    device_pubkey = $device,
                    authorized_at = 'ASSERT',
                    revoked_at = null,
                    device_name = 'laptop',
                    device_cert = $signature
                :put device_authorization {master_pubkey, device_pubkey => authorized_at, revoked_at, device_name, device_cert}
            }
            {
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    id = 'item-1',
                    kind = 'article',
                    title = 'A Quiet Web',
                    creators = ['Ada'],
                    primary_url = 'https://example.com/a-quiet-web',
                    published_at = 'ASSERT',
                    language = 'en',
                    summary = 'summary',
                    discovered_at = 'ASSERT'
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
            }
            {
                ?[item_id, scheme, value, is_canonical, discovered_at] :=
                    item_id = 'item-1',
                    scheme = 'url',
                    value = 'https://example.com/a-quiet-web',
                    is_canonical = true,
                    discovered_at = 'ASSERT'
                :put item_identifier {item_id, scheme, value => is_canonical, discovered_at}
            }
            {
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    item_id = 'item-1',
                    feed_url = 'https://example.com/feed.xml',
                    word_count = 1200,
                    estimated_read_minutes = 6,
                    site_name = 'Example',
                    byline = 'Ada'
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
            }
            {
                ?[item_id, isbn, publisher, page_count, cover_blob_id, is_physical, edition, pub_year] :=
                    item_id = 'book-1',
                    isbn = '9780000000001',
                    publisher = 'Small Press',
                    page_count = 240,
                    cover_blob_id = 'blob-cover',
                    is_physical = true,
                    edition = 'first',
                    pub_year = 2026
                :put item_book {item_id => isbn, publisher, page_count, cover_blob_id, is_physical, edition, pub_year}
            }
            {
                ?[item_id, venue, abstract, citation_count, pdf_blob_id, arxiv_id, doi] :=
                    item_id = 'paper-1',
                    venue = 'Journal',
                    abstract = 'Abstract',
                    citation_count = 3,
                    pdf_blob_id = 'blob-pdf',
                    arxiv_id = '2605.00001',
                    doi = '10.0000/example'
                :put item_paper {item_id => venue, abstract, citation_count, pdf_blob_id, arxiv_id, doi}
            }
            {
                ?[item_id, podcast_title, episode_number, duration_seconds, audio_url, audio_blob_id, transcript_blob_id] :=
                    item_id = 'podcast-1',
                    podcast_title = 'Quiet Reading',
                    episode_number = 1,
                    duration_seconds = 1800,
                    audio_url = 'https://example.com/audio.mp3',
                    audio_blob_id = 'blob-audio',
                    transcript_blob_id = 'blob-transcript'
                :put item_podcast_episode {item_id => podcast_title, episode_number, duration_seconds, audio_url, audio_blob_id, transcript_blob_id}
            }
            {
                ?[item_id, duration_seconds, thumbnail_url, thumbnail_blob_id, platform, embed_url] :=
                    item_id = 'video-1',
                    duration_seconds = 900,
                    thumbnail_url = 'https://example.com/thumb.jpg',
                    thumbnail_blob_id = 'blob-thumb',
                    platform = 'peertube',
                    embed_url = 'https://example.com/embed'
                :put item_video {item_id => duration_seconds, thumbnail_url, thumbnail_blob_id, platform, embed_url}
            }
            {
                ?[item_id, has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at] :=
                    item_id = 'item-1',
                    has_video_embed = false,
                    has_audio_embed = false,
                    is_link_roundup = false,
                    is_long_form = false,
                    is_short = false,
                    has_code = true,
                    has_math = false,
                    detected_at = 'ASSERT'
                :put item_shape {item_id => has_video_embed, has_audio_embed, is_link_roundup, is_long_form, is_short, has_code, has_math, detected_at}
            }
            {
                ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] :=
                    item_id = 'item-1',
                    format = 'html_readable',
                    blob_id = 'blob-html',
                    fetched_at = 'ASSERT',
                    extracted_with = 'readability',
                    byte_size = 42
                :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size}
            }
            {
                ?[item_id, state, progress, progress_label, last_position, updated_at] :=
                    item_id = 'item-1',
                    state = 'reading',
                    progress = 0.5,
                    progress_label = 'halfway',
                    last_position = $json_value,
                    updated_at = 'ASSERT'
                :put read_state {item_id => state, progress, progress_label, last_position, updated_at}
            }
            {
                ?[item_id, tag, added_at, retracted_at] :=
                    item_id = 'item-1',
                    tag = 'commons',
                    added_at = 'ASSERT',
                    retracted_at = null
                :put item_tag {item_id, tag => added_at, retracted_at}
            }
            {
                ?[item_id, starred, updated_at] :=
                    item_id = 'item-1',
                    starred = true,
                    updated_at = 'ASSERT'
                :put item_star {item_id => starred, updated_at}
            }
            {
                ?[id, name, description, is_ordered, created_at] :=
                    id = 'list-1',
                    name = 'Later',
                    description = 'queue',
                    is_ordered = true,
                    created_at = 'ASSERT'
                :put reading_list {id => name, description, is_ordered, created_at}
            }
            {
                ?[list_id, item_id, position, added_at, removed_at] :=
                    list_id = 'list-1',
                    item_id = 'item-1',
                    position = 1,
                    added_at = 'ASSERT',
                    removed_at = null
                :put reading_list_item {list_id, item_id => position, added_at, removed_at}
            }
            {
                ?[id, item_id, quoted_text, location, location_label, color, created_by, created_at, signature] :=
                    id = 'highlight-1',
                    item_id = 'item-1',
                    quoted_text = 'quiet web',
                    location = $json_value,
                    location_label = 'p. 1',
                    color = 'yellow',
                    created_by = $peer,
                    created_at = 'ASSERT',
                    signature = $signature
                :put highlight {id => item_id, quoted_text, location, location_label, color, created_by, created_at, signature}
            }
            {
                ?[id, body, target_kind, target_id, created_by, created_at, updated_at, signature] :=
                    id = 'note-1',
                    body = 'note body',
                    target_kind = 'item',
                    target_id = 'item-1',
                    created_by = $peer,
                    created_at = 'ASSERT',
                    updated_at = 'ASSERT',
                    signature = $signature
                :put note {id => body, target_kind, target_id, created_by, created_at, updated_at, signature}
            }
            {
                ?[id, source_id, target_id, relation, note, created_by, created_at, signature] :=
                    id = 'connection-1',
                    source_id = 'note-1',
                    target_id = 'item-1',
                    relation = 'related_to',
                    note = 'because',
                    created_by = $peer,
                    created_at = 'ASSERT',
                    signature = $signature
                :put connection {id => source_id, target_id, relation, note, created_by, created_at, signature}
            }
            {
                ?[follower, followed, started_at, stopped_at, audience_tags] :=
                    follower = $peer,
                    followed = $friend,
                    started_at = 'ASSERT',
                    stopped_at = null,
                    audience_tags = ['tech']
                :put follow {follower, followed => started_at, stopped_at, audience_tags}
            }
            {
                ?[id, name, kind, created_at] :=
                    id = 'audience-1',
                    name = 'tech friends',
                    kind = 'private',
                    created_at = 'ASSERT'
                :put audience {id => name, kind, created_at}
            }
            {
                ?[audience_id, peer_pubkey, added_at] :=
                    audience_id = 'audience-1',
                    peer_pubkey = $friend,
                    added_at = 'ASSERT'
                :put audience_member {audience_id, peer_pubkey => added_at}
            }
            {
                ?[id, item_id, shared_by, note, audiences, created_at, retracted_at, signature] :=
                    id = 'share-1',
                    item_id = 'item-1',
                    shared_by = $peer,
                    note = 'worth your time',
                    audiences = ['audience-1'],
                    created_at = 'ASSERT',
                    retracted_at = null,
                    signature = $signature
                :put share {id => item_id, shared_by, note, audiences, created_at, retracted_at, signature}
            }
            {
                ?[share_id, highlight_id, added_at] :=
                    share_id = 'share-1',
                    highlight_id = 'highlight-1',
                    added_at = 'ASSERT'
                :put share_highlight {share_id, highlight_id => added_at}
            }
            {
                ?[share_id, reactor, kind, created_at, signature] :=
                    share_id = 'share-1',
                    reactor = $friend,
                    kind = 'saved',
                    created_at = 'ASSERT',
                    signature = $signature
                :put share_reaction {share_id, reactor => kind, created_at, signature}
            }
            {
                ?[id, share_id, parent_reply_id, body, author, created_at, signature] :=
                    id = 'reply-1',
                    share_id = 'share-1',
                    parent_reply_id = null,
                    body = 'reply',
                    author = $friend,
                    created_at = 'ASSERT',
                    signature = $signature
                :put share_reply {id => share_id, parent_reply_id, body, author, created_at, signature}
            }
            {
                ?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] :=
                    url = 'https://example.com/feed.xml',
                    title = 'Example Feed',
                    description = 'feed',
                    last_fetched = 'ASSERT',
                    last_modified_header = 'Sun, 03 May 2026 00:00:00 GMT',
                    etag_header = 'etag',
                    fetch_interval_seconds = 3600,
                    consecutive_failures = 0,
                    folder = 'feeds',
                    auto_mark_read = false
                :put feed {url => title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read}
            }
            {
                ?[id, tombstoned_at, reason] :=
                    id = 'old-item',
                    tombstoned_at = 'ASSERT',
                    reason = 'removed'
                :put entity_tombstone {id, tombstoned_at => reason}
            }
            "#,
            BTreeMap::from([
                ("peer".to_string(), bytes.clone()),
                ("device".to_string(), DataValue::Bytes(vec![8; 32])),
                ("friend".to_string(), DataValue::Bytes(vec![6; 32])),
                ("signature".to_string(), signature),
                ("json_value".to_string(), json_value),
            ]),
        )
        .unwrap();

    assert_single_value(&store, "item", "id", DataValue::from("item-1"));
    assert_single_value(&store, "share", "id", DataValue::from("share-1"));
    assert_single_value(
        &store,
        "feed",
        "url",
        DataValue::from("https://example.com/feed.xml"),
    );
    assert_single_value(
        &store,
        "entity_tombstone",
        "id",
        DataValue::from("old-item"),
    );
}

fn assert_single_value(store: &Store, relation: &str, column: &str, expected: DataValue) {
    let rows = store
        .query(&format!("?[value] := *{relation}{{{column}: value}}"))
        .unwrap();

    assert!(
        rows.rows.iter().any(|row| row.first() == Some(&expected)),
        "relation {relation} did not contain expected {column}"
    );
}
