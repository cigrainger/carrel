# Security

Carrel is pre-alpha. Its fact verification, audience isolation and offline
conflict rules have automated coverage; that is not an independent security
audit or a guarantee that the application is ready for sensitive material.

## Reporting a vulnerability

Use GitHub's **Security → Report a vulnerability** for this repository, when
available. Describe the affected commit/version, boundary crossed and minimal
reproduction. If private reporting is unavailable, open a public issue asking
for a private reporting channel without disclosing the vulnerability itself.
Do not send private reading data, identity seeds, passphrases or contact codes.

Only the current pre-alpha is maintained. There is no promised response SLA or
backported maintenance branch. Fixes and affected versions are documented with
the relevant release.

## Important boundaries

- The local filesystem is trusted. The archive/database is not encrypted at rest.
  The master key file is passphrase-encrypted; operational device keys are local,
  owner-readable files used without a prompt during normal operation.
- Cozo is authoritative. `carrel-sync` exclusively owns the Iroh mirror and peer
  transport. Private device protocols and manually followed people have separate
  authorization gates. Signatures and declared content references are verified
  before incoming data is installed.
- Sharing deliberately sends the selected archived article, cached images and
  vouch note. Personal highlights, notes and reading state remain private.
- Removing a person or withdrawing a vouch cannot erase an article already
  received. A recipient can retain or redistribute content they received.
- Public source and SHA-256 checksums make artifacts inspectable and comparable.
  Current trial builds are not Developer ID signed or notarized.

See [ARCHITECTURE](docs/ARCHITECTURE.md), [SCHEMA](docs/SCHEMA.md) and the
[friend-trial checklist](docs/PRIVATE_SHARING_PLAN.md#first-friend-trial) for the
implemented protocol and remaining manual evidence.
