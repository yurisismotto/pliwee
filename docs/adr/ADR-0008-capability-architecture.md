# ADR-0008 — Capability-based protocol architecture

**Status:** Accepted · 2026-08-29

> **Amending note — 2026-10-04, owner acceptance of [ADR-0023](ADR-0023-pliwee-space-trust-model.md) ([#46](https://github.com/yurisismotto/pliwee/issues/46), branch `feature/issue-46-worker`), §Consequences and owner decision 5.** One rule is added to the capability architecture: **a capability MUST NOT re-emit peer-originated user data to another peer by default. Cross-peer relay is allowed only when an Accepted, capability-specific ADR and/or SPEC explicitly defines that relay, including user consent, authorization, provenance/source identity, destination selection, revocation, privacy/logging, and failure semantics.** Trust between devices does not imply data flow between them. The rule does not prohibit a future relay architecture such as [#27](https://github.com/yurisismotto/pliwee/issues/27); it makes relay an explicit, reviewed capability rather than an accidental property of multi-device connectivity. The tree has no relay path today; for `clipboard.v1` the absence of relay is enforced and tested (THREAT_MODEL T23, `CLIP-SEC-09`). Nothing else in this ADR changes. The original text below is left as it was decided.

## Context

The roadmap is long: clipboard, files, notifications, media control, URL
handoff, browser integration, quick settings. Each has a different schema, a
different risk profile, and will arrive at a different time. Two devices will
routinely run different versions and support different subsets.

The failure mode to avoid is a transport layer that grows a `match` over
feature names, so that adding clipboard means editing the connection code.

## Decision

Every feature beyond the handshake is a **capability**: a versioned string id,
a handler, and a schema the transport knows nothing about.

* The transport carries `CapabilityMessage { capability_id, payload }` and
  routes on the id. `payload` is opaque bytes.
* The version is part of the id (`battery.v1`). A breaking change ships as
  `battery.v2`, and both can be advertised during a migration. There is no
  version negotiation *within* a capability: two ids either match or they do
  not.
* Peers announce their capabilities in `HELLO` / `HELLO_ACK`.
* **Advertising is not authorization.** The effective set is
  `mutually supported ∩ granted by the local trust store`. Grants are stored
  per peer and are re-checked per message.
* `auto_grant` defaults to `["battery.v1"]` only — read-only telemetry with no
  side effects. Anything with side effects must be granted explicitly.

There is no capability name anywhere in `omnibridge-core`'s transport code.

## Alternatives

**A fixed message type per feature in the envelope.** Simplest, and it is what
the enum-based envelope already does for core messages. Rejected for features:
every new feature would change the shared `.proto` and force both sides to
regenerate, and an old peer receiving a new type could not distinguish "I do
not support this" from "this is malformed".

**A version field on each capability, negotiated per capability.** More
flexible. Rejected: it multiplies the negotiation state and buys nothing that
`name.vN` does not, since a capability whose wire format changed is a
different capability.

**Dynamic plugins (`.so` loading).** Rejected: loading arbitrary code into a
process that holds a private key is a large risk for a benefit nobody asked
for. Capabilities are compile-time.

**Granting everything a peer advertises.** Simpler, and it is what a lot of
similar software does. Rejected: it makes a compromised phone equivalent to a
compromised desktop the moment a high-risk capability exists.

## Consequences

* Adding clipboard means: a `.proto`, a `Capability` implementation on each
  side, and a registry entry. No transport change.
* Capabilities can be granted and revoked per device.
* An unsupported or ungranted capability produces a **non-fatal** error and
  the session survives, so one bad message does not cost the user their
  connection (`an_unknown_capability_id_is_refused_without_closing_the_session`).
* Slight overhead: an id string per message. Irrelevant at this size.

## Security implications

* Positive: a compromised peer is confined to what it was granted. With only
  `battery.v1` in existence, the blast radius today is a battery percentage.
* Positive: high-risk capabilities can default to off without special-casing
  them in the transport.
* Positive: a capability's payload is opaque to the transport, so parsing and
  validation happen in one place, at the capability boundary — where the
  battery percentage range check lives, for instance.
* Positive: a capability handler that throws is logged and contained; it
  cannot tear down the session
  (`a_malformed_capability_payload_does_not_kill_the_session`).
* Caveat: the *transport* is capability-agnostic, but the daemon still decides
  the auto-grant policy. That policy is the thing to review when a
  side-effecting capability is added.
