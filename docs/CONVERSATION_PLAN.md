# Private identities and conversations

Names, optional pictures and private conversations build on Carrel's existing
master identities, certified desktops and deliberate article vouches. Named
groups select people for a share; each sent share fixes its own audience. This
is discussion around something a person chose to share, without a public stream,
notifications, reply counts or popularity signals.

## Recognizable people

Settings → People offers an optional display name and local PNG/JPEG import.
Pictures are decoded with allocation/dimension limits, cropped to a small square,
and encoded as fresh PNG pixels without source metadata. A signed profile binds
the picture's digest and bounded bytes to its author and one recipient. No avatar
URL, remote service or generic blob request is introduced. Initials are the
fallback. The verified public key remains the trust anchor; a reader's local
contact alias takes precedence over the proposed name.

Profiles use logical generations, with a digest of their signed presentation
breaking equal-generation ties. Recipient/device/time fields do not change that
digest, so forwarding cannot change the selected name/picture. A later explicit
save observes the greatest known generation.
Current follows receive recipient-scoped copies, including after adding a new
contact. Group forwarding cannot forward a profile addressed to someone else.
An own-device profile change is copied idempotently for the contacts configured
on each active desktop, without copying the local contact configuration itself.
Desktop names remain separate from personal presentation.

## A fixed audience for each vouch

People includes a name and member checklist for each local group. The share
composer offers multiple people/groups and spells out their deduplicated audience
before Send. With several people, explicitly choose a shared group conversation
or independent one-person sends. Separate roots contain no other recipients and
their replies stay separate. The local batch manifest and every proof/journal
entry commit atomically, so retry or failure cannot deliver a partial selection.
It retains the selected membership while background contacts/groups refresh.
If the group changes before the first save, the user reviews it again without
losing the note. Retrying a completed save returns its original proof even if
the group has since changed.

Participant names/pictures open native person cards, including the introduction's
original sharer and verified key. Profiles stay recipient-scoped; unknown people
use a key fallback. Connect guides the existing two-way contact-code exchange
while keeping the thread and reply draft mounted. Exchange codes privately,
review a signed code matching that exact participant, then choose Follow.
Both people choose independently. A wrong key, forged code or outsider is refused.
Opening or reviewing a card does not follow anyone or distribute connection codes.

Version 1 one-person vouches retain their original canonical bytes and signatures.
Version 2 signs a sorted, unique recipient set. Group names and contact aliases
stay local; only participant keys and deliberately shared content travel. Up to
16 recipients plus the sharer can participate. A thread belongs to that immutable
share ID, never merely an article URL or mutable group. Changing/removing the
group affects future selections only. Two shares of the same article remain
separate conversations, even when their audiences overlap.
Replies also bind the root's exact signed digest, so an equivocating sharer cannot
relay replies between different article/note proofs under the same draft ID.

Members can exchange verified replies through the sharer. Direct exchange is
also possible between mutually followed members. Everyone need not connect to
everyone first, but relaying waits while the necessary desktop is offline.
Forwarding never follows someone, discloses their contact card, grants a private
archive capability or widens the root's audience. Removing a contact stops this
desktop's direct exchange; it cannot change another participant's retained copy
or an earlier group's fixed audience.

## Reading and replying

Conversation opens from a received vouch, article Info or People → Sent articles.
Article Info retains received conversation links after withdrawal. The native
dialog keeps the article/list mounted behind it and restores initiating focus.
The original vouch opens the conversation, followed by chronological replies,
shallow parent context and a composer naming the participants.

Send is explicit, including Cmd+Enter. It commits locally without waiting for
delivery. Failures retain the text, parent and retry identity. Unsent replies
survive closing/reopening the conversation during the current app session;
only Send makes a durable queued message. Late saves acknowledge exactly their
submitted draft and preserve newer composition. Palette and native-dialog guards
leave drafts intact. Background refresh does not replace the composer.

Replies are immutable. Their author can deliberately remove a message; its
placeholder and ancestry remain, while its body is omitted from presentation.
Removal wins over delayed delivery and replay regardless of clocks. Editing is
deferred rather than silently replacing a concurrent draft.

The original sharer can withdraw the vouch, closing the conversation. Accepted
history remains readable. Offline replies may arrive later and remain in that
history, but cannot reopen it. New local replies are refused, and an unfinished
draft stays available for copying. Withdrawal cannot erase copies already received.

## Cozo and signed provenance

Core conversation types define bounded certified records; ShareFact is the
social wire allow-list. Cozo stores the original canonical signed bytes.
Its reply index supports recursive ancestry from a selected message through its
parents. Provenance shows the original master, signing desktop and record identity.

The private note editor also offers Revision provenance. A recursive Cozo query
starts from every current head and follows signed causal parents, retaining both
branches after conflict resolution. Unsigned legacy migration links are excluded.
These are working typed store reads, not speculative graph or ranking features.
Reader-chosen cross-cutting views and near-duplicate article grouping remain
separate possible work.

Missing roots/parents wait durably in a bounded dependency queue and are neither
rendered nor served. Promotion checks the original scope, parent thread and
author-removal permission. Pending cycles and cross-thread parents are rejected.
No speculative conversation or arbitrary private blob is fetched.

Accepted social history also enters PrivateFact::Social in the private journal,
atomically with its social projection. Only an authorized self-device may forward
it, and the bound master must be a participant. Original external signatures are
preserved. Contacts, named groups and connection hints remain local configuration;
reading a conversation on another own desktop does not silently follow anyone.
Friend connections still pin one certified desktop and require explicit contact
code replacement for another; automatic friend-device rotation is future work.

Schema migration 11 adds projections/staging and journals original legacy sharing
bytes without re-signing, rewriting archives or following people. Migration 12
adds the local atomic-send manifest, preserving the existing social wire protocol.
Both readers
need alpha.3 or later for profiles, group vouches and conversations. Alpha.2
packages do not support these records.

## Verification and first trial

Core, store and real in-process peer checks cover fixed audiences, signatures,
out-of-order delivery, cycles, cross-thread parents, removal, withdrawal, replay,
private cache exclusion, own-device forwarding, revocation and interrupted migration.
Native checks cover bounded picture normalization and stale group selections.
Temporary WebKit review exercises the actual CSR build with mocked native commands;
it does not establish live Tauri file-picker or physical network behavior.

For a first trial, use the same current build on each desktop and complete the
[friend checklist](PRIVATE_SHARING_PLAN.md#first-friend-trial), then:

1. Set distinct names/pictures. Check that a local alias still wins and that a
   newly followed person receives the current profile.
2. Create a group with another reader. Review its recipients in the composer,
   select an overlapping person/group and choose one conversation. Check that each
   person participates once. Send separately as well and verify independent audiences.
3. Reply offline, reconnect and restart. Check parent context, author removal,
   history and the deliberate Send behavior. Keep private article notes separate.
4. With three readers, relay a reply through the sharer. Share the same article
   to a different audience and confirm that neither conversation leaks across.
5. Edit the group. Confirm that the earlier conversation keeps its participants.
   Withdraw it, then check retained article Info/history and the disabled composer.
6. Pair another own desktop. Check conversation history without an automatic
   contact follow, and queue a reply through own-device sync. Import connection
   information explicitly to exchange directly with friends from that desktop.
7. Open an unfamiliar participant's card. Check its introduction and key, exchange
   codes privately, try a mismatched code, then deliberately follow each other.
   Check that the draft/passage survives, and other participants receive no code.

Physical desktops, native file selection and manual release packaging remain
separate checks; alpha.3 release notes distinguish them from automated evidence.
