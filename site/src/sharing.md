# Sharing with a friend

Carrel has two different connections: **Devices** pairs your own installations;
**People** connects distinct readers. A friend should create their own identity.

## Set up your identity

Open Settings → Devices → **Set up sync**, and choose your own recovery
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

Paste the other person's contact code, give them a local name, compare the full
public key with the key they sent, and **Follow**. Each person follows the other.
A bare key can be saved before the connection code arrives.

## Send an article

In the reader, choose **Share article**. Select a person explicitly and add an
optional note. Saving records the share locally; background exchange retries
while both apps are open. A saved contact code or locally saved share does not
by itself prove delivery. Receipt in **Library → Shared with you** does.

The chosen article, its cached images and the vouch note are shared. Your
personal highlights, notes, reading state and subscriptions remain private.
The recipient gets an archived article they can read offline and annotate
independently.

## Recover or stop a connection

People shows quiet connection errors and **Try connection again**. If an address
changes, obtain a fresh contact code and review its public key before updating.
The first flow uses one certified desktop per followed person; automatic
friend-device rotation is future work.

People → Sent articles retains history, with **Load older** for earlier shares.
**Withdraw share** retracts your vouch after exchange; it cannot erase a copy
already received. Removing a person stops new exchange and preserves earlier
archives. Assume a recipient can keep anything you deliberately send them.

For a practical trial, follow the
[two-desktop checklist](https://github.com/cigrainger/carrel/blob/main/docs/PRIVATE_SHARING_PLAN.md#first-friend-trial),
including offline reading, restart, withdrawal and private-note separation.
