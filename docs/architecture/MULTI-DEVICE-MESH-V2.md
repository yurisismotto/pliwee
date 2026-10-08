# Multi-device Mesh V2 — implementation specification

**Status:** Approved implementation baseline · 2026-10-07

Normative architecture:

- ADR-0007 — TLS 1.3 and SPKI pinning
- ADR-0008 — capabilities and per-peer grants
- ADR-0010 — envelope and replay rules
- ADR-0017 — capability roles
- ADR-0022 — any-to-any topology and symmetric pairing
- ADR-0023 — pairwise Pliwee Space trust
- ADR-0024 — capability negotiation and trusted-peer presence

This SPEC supplies the implementation details ADR-0022 deliberately left out.
Where this document and an Accepted ADR disagree, the ADR wins.

## 1. Protocol version

Canonical Pliwee peers continue to use ALPN `pliwee/1`.

The envelope schema remains the shared schema for protocol versions 1 and 2.

A V2 implementation advertises:

- minimum protocol version: 1
- maximum protocol version: 2

Version 2 behaviour is used only when both peers negotiate version 2.

A peer negotiated at version 1 receives no V2-only message.

## 2. Session close reason

Protocol version 2 may send a SessionClose control body when intentionally
closing a superseded duplicate control session.

The next free Envelope body field is 25.

SessionClose has one version-2 reason in this specification:

- UNSPECIFIED = 0
- SUPERSEDED = 1

A V1 peer is closed without a SessionClose body.

SUPERSEDED is diagnostic only. It is never an authorization input.

## 3. Liveness constants

V2 keeps the existing session liveness behaviour:

- liveness tick: 15 seconds;
- probe after 60 seconds without inbound traffic;
- declare the session dead after 180 seconds without inbound traffic.

Any inbound valid frame resets the silence observation.

Changing these values later is an operational SPEC change, not an architecture
change, provided the bounded-liveness property remains.

## 4. Dial scheduling and duplicate sessions

The ADR-0022 defaults are normative:

- preferred-dialer delay: random 1.5 to 3 seconds;
- stale-session probe timeout: 3 seconds;
- more than 5 supersessions for one peer within 60 seconds rejects new
  sessions from that peer for 60 seconds.

Fingerprint ordering uses the raw 32-byte SPKI fingerprints as unsigned bytes
in lexicographic order.

Exactly one authenticated control session per peer survives duplicate
resolution.

Capability traffic is paused while a duplicate is being resolved.

## 5. Address hints

Address hints are routing hints, never identity.

For each trusted peer the local store may persist at most 8 recently
successful address hints.

Each hint contains:

- IP address or platform-normalized host representation;
- TCP port;
- informational last-success wall-clock value used only for display/eviction.

The SPKI pin remains the authority when dialing any hint.

Per-peer exponential backoff is runtime state and is not persisted across
process restart.

A new network resets runtime backoff.

A pin failure for one address does not suppress other known addresses.

## 6. Android advertising

Android advertising follows ADR-0022 exactly.

Advertising may become allowed automatically only on the network where the
device successfully completed an explicit pairing.

On every other network it remains off until enabled locally.

Merely establishing a trusted session never enables advertising.

The stored network key is local, opaque platform state. It is never sent to a
peer and never participates in trust.

## 7. Text pairing code

The pairing token remains exactly 160 random bits.

Text mode renders the 20 token bytes using RFC 4648 base32:

- uppercase alphabet A-Z and 2-7;
- no padding;
- exactly 32 data symbols.

A transcription check symbol is appended.

The check symbol is the base32 symbol selected by the first five bits of:

SHA-256(
  "pliwee/text-code-check/v1"
  || token
)

Display form is eight groups of four data symbols followed by the check
symbol.

Separators are visual only.

Input may ignore ASCII spaces and hyphens and may fold lowercase ASCII to
uppercase.

Every other character, a wrong length, or a wrong check symbol is rejected
before a network connection begins.

The check symbol is transcription protection only. It is not authentication.

The 160-bit token remains the pairing secret.

The code is never accepted from argv, logged or offered through a Pliwee copy
action.

## 8. Pairing comparison code

The comparison digest is exactly ADR-0022's value:

SHA-256(
  "pliwee/pairing-compare/v1"
  || len32(responder_fp)
  || responder_fp
  || len32(initiator_fp)
  || initiator_fp
)

Use the first 66 digest bits as six consecutive 11-bit unsigned indices.

The fixed 2048-entry word list is the canonical English BIP-39 word list in
its published order.

Implementations vendor the exact list in the repository. They do not download
it at runtime.

Rendering is:

word1 word2 word3 word4 word5 word6

Words are lowercase ASCII separated by one ASCII space.

V2 does not localize the list. A future localized representation needs its own
versioned SPEC because two screens must render the same comparison value.

Human confirmation remains mandatory on both V2 peers.

## 9. Pairing roles

Issuer is always:

- the device whose user opened the pairing window;
- TLS server;
- pairing-proof responder.

Joiner is always:

- the device on which the code is scanned or entered;
- TLS client;
- pairing-proof initiator.

QR mode keeps the current pliwee1 payload.

Text mode carries only the token. Address selection is a routing hint.

No network message may open a pairing window.

## 10. Android inbound TLS verifier

Before Android may advertise or accept ordinary inbound peer sessions it must
prove all of these properties:

- TLS 1.3 only;
- mandatory client certificate;
- full handshake or refusal of attempted resumption;
- canonical ALPN;
- peer SPKI extracted and checked against the local trust store, or against
  the active pairing proof path while a local pairing window exists;
- connection, handshake, protocol and frame resource limits applied before
  unbounded allocation;
- an unpaired peer cannot use capability traffic.

Failure of any property keeps the Android listener disabled.

## 11. Trust-store migration

Existing pairings remain valid.

New fields are additive and default empty.

No migration changes:

- identity key;
- peer SPKI fingerprint;
- device id;
- revoked state;
- capability grants.

Address hints are non-authoritative and may be discarded safely.

A failed migration must leave the old trust data readable or fail closed. It
must never silently trust a new peer.

## 12. Capability negotiation

HELLO and HELLO_ACK advertise versioned capability ids only.

Capability-specific metadata stays inside the capability.

Grant narrowing is immediate.

Grant widening takes effect on the next authenticated connection.

Basic trusted-peer presence follows ADR-0024 and is derived from local session
state.

## 13. Required implementation tests

Before the V2 topology is considered implemented, automated tests cover:

- V1 to V2 negotiation remains version 1;
- V2 to V2 negotiates version 2;
- unknown V2-only body sent to V1 is impossible;
- Android listener rejects a client without a certificate;
- unpaired device cannot use the protocol;
- self-connection is rejected;
- simultaneous dialing converges to one session;
- stale old session loses to a live replacement;
- churn bound is enforced;
- address hint with wrong SPKI never authenticates;
- text code round-trip and checksum rejection;
- text pairing pins nothing before the confirmation MAC succeeds;
- comparison code is identical on both peers;
- existing V1 pairing records migrate without re-pairing;
- discovery data never creates trusted presence.

Physical multi-device validation remains a higher-tier gate and is never
reported PASS when it was not executed.
