# Sharing with a friend

Carrel has two different connections: **Devices** pairs your own installations;
**People** connects distinct readers. A friend should create their own identity.

## What travels between readers

Each person keeps their own archive and identity. You exchange contact codes,
compare public keys and explicitly follow each other. Carrel then exchanges
chosen articles peer to peer, with relay connectivity where needed. There is no
Carrel account or central service we operate.

A share carries the chosen article, its cached images and your share note. Your
friend receives a copy they can read offline and annotate independently. Your
personal highlights, notes, subscriptions and reading state stay private. Pairing
your own desktops is a separate connection that can sync that private state;
following a friend does not grant the same access.

Shares wait locally while a desktop is offline. Exchange retries while both
apps are open; a locally saved share is not a delivery receipt.

## Set up your identity

Open Settings → Devices → **Create identity**, and choose your own recovery
passphrase. Your master key stays encrypted on its originating desktop. Ordinary
reading uses that desktop's operational key. There is no account service or
password-reset service.

If you already have your identity on another computer, use the one-time
invitation flow in Devices to pair that installation. An invitation is for your
own desktop and is cancelled when dismissed. Do not use it as a friend contact.

## Add one person

Open Settings → People. **Copy public key** and **Copy contact code** let you
exchange details directly. If clipboard access is unavailable, select the
complete text and use ordinary Copy.

Paste the other person's contact code, optionally give them a local name, compare the full
public key with the key they sent, and **Follow**. Each person follows the other.
A bare key can be saved before the connection code arrives.

## Send an article

In the reader, open **Share article → Share with people…**. Select a person or group explicitly and add an
optional note. Saving records the share locally; background exchange retries
while both apps are open. A saved contact code or locally saved share does not
by itself prove delivery. Receipt in **Library → Shared with you** does.

The chosen article, its cached images and the vouch note are shared. Your
personal highlights, notes, reading state and subscriptions remain private.
The recipient gets an archived article they can read offline and annotate
independently.

## Names, pictures and groups

In People, set your optional display name and choose a PNG/JPEG picture. Carrel
normalizes the picture locally and sends a small signed copy only to people you
follow. It never fetches a remote avatar. A name you choose for a contact takes
precedence over their proposed name; the public key remains the identity.

Create a group by naming it and checking its members. The Share composer lists
the recipients before sending. Each sent share keeps those people permanently:
editing or removing the group affects future shares, not existing conversations.
If membership changes after selection, review the group again; the note stays
available. Groups are local selections, not accounts or shared directories.

## Continue the conversation

Open **Conversation** beside a received vouch or in People → Sent articles.
Article Info also lists received conversations, including withdrawn shares.
The vouch starts the exchange; replies stay visible only to that share's fixed
participants. A separate share of the same article has a separate conversation.

Choose Reply for shallow context, write your message and **Send reply** (or
Cmd+Enter). It queues locally while offline. Failed sends keep the draft and
retry identity. Unsent text survives reopening the conversation during the same
app session; only Send makes it durable. Closing resumes the article/list behind it.
Provenance and reply ancestry shows original author/device keys and the parent
chain. Private note editing has its own Revision provenance disclosure.

Members can receive replies through the sharer while that desktop is online;
mutually followed members can also connect directly. No one is automatically
followed. A participant may remove their own message. Withdrawing the original
share closes new participation while preserving history and the saved article.
An unfinished reply remains available for copying; removal cannot erase copies
already received. There are no reply counts or notification pings.

These additions require the same current source build on each desktop and are
newer than alpha.2 packages. Signed conversation/profile history follows your
private own-device sync; contacts, group selections and connection information
must still be set up explicitly for direct friend exchange on another desktop.
That desktop can read retained history and queue replies through own-device sync.

## Recover or stop a connection

People shows quiet connection errors and **Try connection again**. If an address
changes, obtain a fresh contact code and review its public key before updating.
The first flow uses one certified desktop per followed person; automatic
friend-device rotation is future work.

People → Sent articles retains history, with **Load older** for earlier shares.
**Withdraw share** retracts your vouch after exchange; it cannot erase a copy
already received. Removing a person stops new exchange and preserves earlier
archives. Earlier group conversations keep their original participants; removing
a contact stops this desktop's direct exchange. Assume a recipient can keep
anything you deliberately send them.

For a practical trial, follow the
[two-desktop checklist](https://github.com/cigrainger/carrel/blob/main/docs/PRIVATE_SHARING_PLAN.md#first-friend-trial),
including offline reading, restart, withdrawal and private-note separation.

## Sharing and copying links

The reader's Share control opens a small menu. **Copy link** copies the original
webpage address; **Copy Carrel link** returns to your saved article on a desktop
that already has it. Copying does not send the archive or grant access.

Choose **Share with people…** to send an archived article and optional note.
If you have not followed anyone, **Add a person…** opens People settings instead
of an empty composer. A followed person can be offline: the share waits locally.
Device pairing is for your own private library; connect with friends in People.
