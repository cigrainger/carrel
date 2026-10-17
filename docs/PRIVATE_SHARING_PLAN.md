# Private sharing with one trusted friend

This is the next authorized milestone after source folders and the bounded
reader review. It implements deliberate article sharing, not a general social
network. Four primary destinations remain. Library gains Shared with you;
Settings gains People. Article tags, group editors, public syndication,
discussion, reactions and shared lists remain outside this pass.

## Reading flow

Add a person in Settings → People by their public key. A copyable contact code
contains that same key, a certified device and signed connection information;
it grants no access to a private archive. Display the full public key before
adding it. A bare key can be stored before connection information is supplied.
The first private connection requires each person to pin the other's public key;
exchange can start when either has the other's signed connection code. This
does not expose either person's private self-sync document or archive.

The reader Share menu offers both link-copy actions. With a followed person,
Share with a person opens a native dialog over the current reader; otherwise
Add a person opens People settings. Choose one
person explicitly and optionally add a brief note. Saving writes a durable
signed share locally; background exchange retries while the apps are open.
Closing or retrying retains normal modal focus and reading context. Nothing
implicitly completes, stars or shares an article.

Received shares have their author's local name, note and article context in
Library → Shared with you. Opening one uses the existing archived reader and
return behavior. Personal reading state and annotations stay private. Removing
a person stops new exchange; saved articles remain available. Retracting a share
is explicit and cannot guarantee erasure of a copy already received.

## Trust boundary

The existing certified device identity also authenticates the Iroh endpoint.
Self docs, gossip and raw blob protocols retain their bound-master allow-list.
A separate `carrel/share/1` protocol accepts only bounded typed requests. Its
handler verifies a device certificate against the requested recipient master
and the authenticated TLS device before selecting any records or content.
Knowing a private namespace capability or blob hash never grants friend access.

Cozo owns immutable signed share and retraction records, contacts and local
receipt projections. `carrel-sync` mirrors only explicitly shared records into
a distinct Iroh document and owns all transport, verification and cache transfer.
That document is accessed by friends through the typed audience filter, not the
raw docs protocol. No remote Cozo scripts or self-state facts cross this boundary.

A share signs the exact recipient, article snapshot, note, author certificate
and time. It includes current readable content and only its referenced cached
images. It excludes subscription URLs, feed GUIDs, arrival history, older cached
bodies, private notes, highlights, tags, stars and reading state. Article identity
on receipt resolves through the original URL and preserves an existing archive.
Bounded blob requests must name a share and a reference signed into that share;
the requested digest and length are checked before native cache insertion.

## Verification

Core tests cover signatures, canonical identities, bounds and forged fields.
Store tests cover atomic receipts, duplicate/reordered delivery, stable existing
archives, latest-vouch pagination, retraction dominance and removed contacts. Three isolated
network identities cover intended delivery, another audience being refused,
private protocols and known private hashes being refused, and authorized offline reading.
Background exchange tests cover bare-key contact completion and reconstruction
of a lost mirror from persistent Cozo records after transport restart.
Temporary WebKit review covers the complete dialog and reader-return flow.

The sharing milestone passes 400 workspace tests, native and CSR lint and an
actual CSR build. Temporary WebKit checks cover explicit key review, person
selection, failed/retried and late saves, palette draft protection, withdrawal,
reader callbacks and anchors, source recovery, invitation cleanup, and minimum
window layouts in all four themes.

The completed source/reader review passed 378 tests, native and CSR lint,
actual WASM builds and temporary WebKit checks. An isolated native store ingested
all 74 articles from the real Elixir feed and verified publication ordering and
pagination for source and folder lists. Live native window interaction, native
file-picker selection and physical two-machine networking remain manual checks;
headless WebKit and in-process peers do not establish those results.

The first flow connects one certified desktop per followed person. Another device
or changed connection address can be imported explicitly; automatic friend-device
rotation and self-sync of contacts/outgoing vouches remain future scope. Received
article archives, image scope and personal reading facts use ordinary self-sync.

## First friend trial

Use the same Carrel build on both desktops. Each person sets up their own
identity through Settings → Devices → Create identity, with their own recovery
passphrase. Exchange contact codes from Settings → People. Desktop pairing
in Devices connects your own installations; People connects distinct readers.

1. On each desktop, Copy public key and Copy contact code. Exchange these
   directly. Paste the other person's code into People, give them a local name,
   review the complete public key against the key they sent, and Follow. Each
   person follows the other. A bare key can wait until a contact code is supplied.
2. Save an article with text and an image on desktop A. Add a private highlight
   and note, then leave it unread and unstarred. Share article to B with a
   different, explicit vouch note. A should report a locally saved share without
   changing the read/star state or moving the visible passage.
3. Keep both apps open, with Settings closed. On B, open Library → Shared with
   you. Confirm the sender's name, vouch, readable text and cached image. A's
   private highlight and note should be absent. If exchange needs recovery, open
   People, check the saved key/contact code and Try connection again. Contact
   code availability does not assert successful delivery; receipt in B's Library
   is the evidence.
4. After receipt, disconnect B from the network and reopen the article. Its text
   and completed cached images should remain readable. Mark it read and add a
   private note on B. A's read/star state and notes should remain independent.
5. Reconnect B, close and restart both apps, then share another article. Confirm
   background delivery again. If connection information changed, review the
   person's new contact code against the same public key and follow/update it.
6. On A, open People → Sent articles and Withdraw share. On B, the withdrawn
   vouch should disappear from Shared with you and reader Info after exchange;
   the archived article and B's personal notes should remain in Library. Load
   older sent articles reaches earlier vouches for the same deliberate review.
7. Remove the person in People. Further exchange should stop, with earlier
   archives retained. Closing Settings must resume the original reader/list and
   keyboard focus. If clipboard access was unavailable, complete-text selection
   and the platform's ordinary Copy action should still allow exchanging codes.

Record the two app versions, operating systems, direct/relay network conditions,
which step failed, and the quiet People error if present. Keep article bodies,
private notes, recovery passphrases and invitation text out of a shared bug report.
This checklist is pending manual evidence; it does not turn local peer tests or
headless browser review into a completed physical two-machine trial.

The readiness pass passes 401 workspace tests, formatting, native/CSR lint and
an actual CSR build. Temporary WebKit review covers clipboard success, denied
and missing access, stable selected text, 151-share history with paging retry
and withdrawal, contact confirmation updates, focus recovery, and late responses
after closure. The existing sharing and full reader regressions also pass.
Minimum-window Light/Dark screenshots were inspected. Native clipboard and
physical two-desktop behavior remain part of the manual trial above.
