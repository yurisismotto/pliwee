# ADR-0006 — Device identity and pairing

**Status:** Accepted · 2026-08-29

> **Superseding note — 2026-09-24, Pliwee rebrand Wave 5 (branch `feature/pliwee-rebrand-wave5`), [ADR-0020](ADR-0020-rename-to-pliwee.md) §D4.** The daemon now shows `pliwee1:` codes only; the app accepts `pliwee1:` and `omnibridge1:`, and the scheme fixes the connection's identity profile. New identities carry `CN=pliwee:<device-id>`; existing ones are never regenerated. `omnibridge1:` and the `omnibridge/pairing-{proof,confirm}/v1` domains below are the legacy profile's values. The original text below is left as it was decided.

> **Superseding note — 2026-10-04, owner acceptance of [ADR-0022](ADR-0022-any-to-any-topology-and-symmetric-pairing.md) ([#41](https://github.com/yurisismotto/pliwee/issues/41), branch `feature/issue-41-worker`), §D5.** The pairing roles below — *the desktop issues the token and shows the QR, the phone scans* — and the consequence *"Pairing needs a camera"* are superseded: any device may be the **issuer** (opens the window locally, listens, TLS server, responder) or the **joiner** (dials, TLS client, initiator), and the code is carried by QR or, as a fallback, by a text code. In text-code mode the joiner pins the issuer's SPKI only after the confirmation MAC verifies under the token, instead of from the code before connecting. Human confirmation is mandatory in both modes, and a pairing does not become trusted until the required local confirmation succeeds; between two V2 devices both sides compare a comparison code of at least 64 bits (six words from a 2048-word list) derived from both fingerprints. When both peers negotiate V2, both devices confirm; when either peer is V1, the QR-only compatibility flow retains ADR-0006's issuer-side mandatory confirmation (ADR-0022 §Migration). Identity, the token, the proof, the confirmation MAC, the order verify → consume → confirm, and mutual pinning are kept unchanged. Accepted is not implemented: until ADR-0022 lands, the tree still behaves as described below. The original text below is left as it was decided.

## Context

Principle 6 forbids treating an IP address, hostname or MAC address as
identity. Each device needs a cryptographic identity that survives network
changes, and there must be an explicit, human-verified way to establish trust
between two devices for the first time.

## Decision

### Identity

* **Key: ECDSA P-256.** One key per device, generated on first run.
* **On Android:** generated inside the Android Keystore, StrongBox when the
  device has a secure element, otherwise the TEE. Non-exportable.
* **On Fedora:** PKCS#8 DER at `$XDG_DATA_HOME/omnibridge/identity.key`,
  mode 0600 in a 0700 directory. The daemon **refuses to start** if those
  modes are loose.
* **Fingerprint: `SHA-256(DER SubjectPublicKeyInfo)`,** lowercase hex. The
  key is fingerprinted, not the certificate.
* **Device id:** 128 random bits, hex. Not derived from any hardware
  identifier, and not a credential.

### Pairing

1. Desktop opens a window and generates a 160-bit single-use token.
2. QR shows `omnibridge1:<desktop-fingerprint>:<token>:<device-id>:<addresses>`.
3. Phone scans, **pins the fingerprint**, then connects.
4. Desktop replies `PAIRING_REQUIRED` with a fresh 32-byte nonce.
5. Phone sends `proof = HMAC-SHA256(token, domain ‖ len(desktop_fp) ‖
   len(phone_fp) ‖ len(nonce))`.
6. Desktop verifies in constant time, consumes the token, then asks a human to
   confirm the phone's fingerprint.
7. Desktop replies with a confirmation MAC under a different domain
   separator; the phone verifies it.
8. Both store the other's fingerprint. The token is destroyed.

Token: 160 bits, 120-second TTL judged by the issuer's monotonic clock, single
use, and the session aborts after 3 failed proofs.

## Alternatives

### Key algorithm

**Ed25519.** Better on its own merits — faster, no nonce hazard, simpler
implementation. **Rejected**, and this is the decision most likely to be
questioned: Android Keystore does not offer hardware-backed Ed25519 across the
fleet, and an Ed25519 key is not usable as a TLS client-certificate key
through `javax.net.ssl`'s `KeyManager`. Choosing it would mean either an
exportable software key on the phone or no TLS client authentication. Keeping
the private key inside the TEE outranks the primitive's elegance. P-256 is
supported by both `ring` and Conscrypt, so both ends use the same thing.

**RSA-2048/3072.** Universally supported. Rejected: larger keys, slower
operations on a phone, no advantage.

### Fingerprinting the certificate instead of the SPKI

Rejected. A certificate carries an expiry and must eventually be reissued.
Pinning the certificate would break every existing pairing on renewal; pinning
the key lets a device reissue freely. This is standard practice (HPKP,
Android's `network-security-config`).

### Pairing method

**Numeric comparison / SAS**, as Bluetooth does. Good UX and no camera.
Rejected for v1 because a short code needs an interactive commitment protocol
to be safe, which is more machinery than a QR code that already carries the
full fingerprint out of band.

**PIN typed on both sides.** Rejected: a PIN short enough to type is short
enough to guess, so it would need aggressive rate limiting and would still be
weaker than a 160-bit token in a QR.

**SPAKE2 or a PAKE.** Cryptographically the "right" answer for a low-entropy
secret. Rejected as unnecessary: our secret is *high* entropy (160 bits), and
the desktop's key is already pinned out of band via the QR, so there is no
low-entropy exchange to protect. Adding a PAKE would add a dependency and a
novel failure mode for no gain.

**Sending the token over the (already pinned) TLS channel.** Simplest
possible. Rejected: it makes the token a bearer secret, so any recipient of
the message could reuse it. The HMAC proof binds it to both identities and to
a nonce, which is what kills cross-device and cross-session replay
(`a_proof_for_a_different_device_is_rejected`,
`a_proof_for_a_previous_nonce_is_rejected`).

**TLS exporter (RFC 5705) channel binding** instead of binding to
fingerprints. Cryptographically tidier. Rejected: `SSLSocket` on Android does
not expose keying-material export in its public API, so it would require
bundling Conscrypt explicitly. Binding to both certificate fingerprints
achieves the same goal — the proof is useless outside the exact pair of
identities — with no extra dependency.

### Desktop key storage

**Secret Service / gnome-keyring.** The idiomatic desktop answer. **Rejected:**
a `systemd --user` daemon can start before any keyring is unlocked, and a
daemon that blocks at boot waiting for a keyring is worse than useless. It
also makes the daemon untestable without a session bus.

**TPM2-sealed key.** Genuinely better — it would make the desktop key
non-exportable the way the phone's already is. Deferred, not rejected: it adds
a hard dependency on a working TPM and a fallback path, which is more than
this Sprint should carry. Recorded as the top security debt.

*Reviewed again at the close of the foundation Sprint and deliberately left
deferred.* Making TPM2 a prerequisite now would cost portability — the daemon
must still run on a machine with no TPM, inside a VM, or on a container host —
so the work is not "add TPM2" but "add TPM2 **and** keep the file path working
**and** decide what happens when a sealed key becomes unsealable after a
firmware update". That is a self-contained piece of work with its own failure
modes, not a line item in a naming-and-verification Sprint.

What the file path must therefore keep doing, and does today
(`desktop/core/src/store.rs`):

* the private key is written `0600` inside a `0700` directory, created with
  those modes rather than relaxed to them afterwards;
* both modes are **verified on load**, and the daemon refuses to start if the
  key is group- or world-accessible, rather than warning and continuing;
* the key is written through a temp file and renamed, so an interrupted write
  cannot leave a truncated key behind.

This is an honest local-attacker boundary: it stops another user account on
the same machine, and it does not stop malware running as the user. TPM2 is
what would move that line, and it stays the top security debt.

## Consequences

* Identity survives IP, hostname, SSID and DHCP changes.
* Pairing needs a camera and physical co-presence, once.
* A phone's identity cannot be extracted even from a rooted device, so a lost
  phone is a revocation problem rather than a permanent compromise.
* The desktop key is protected by filesystem permissions only. That is a
  weaker guarantee than the phone's, and it is stated plainly rather than
  glossed.
* Reinstalling the app or deleting the desktop's data directory destroys the
  identity and requires re-pairing. This is correct — a new key is a new
  device — but it must be explained in the UI.

## Security implications

* The pairing token is never a credential: it is never sent on the wire, it is
  single-use, it expires, and it is zeroed on drop.
* The proof binds the token to both identities and a server nonce, so it
  cannot be replayed to another responder, by another initiator, or in another
  session.
* The confirmation MAC is domain-separated from the proof, so a captured proof
  cannot be replayed back as a confirmation.
* Constant-time comparison on both sides (`subtle::ConstantTimeEq`,
  `MessageDigest.isEqual`).
* A human confirms the fingerprint. This is the last defence against an
  attacker who obtained the token, and it depends on the user actually looking
  — the weakest assumption in the model.
