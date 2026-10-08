# ADR-0024 — P0 capability negotiation and trusted-peer presence baseline

**Status:** Accepted · 2026-10-07

Accepted by the project owner during the P0 autonomous-development readiness
review on 2026-10-07.

This ADR closes the remaining architecture questions that prevented roadmap
issues #5 and #8 from being decomposed into implementation work. It keeps the
security model of ADR-0008, ADR-0017, ADR-0022 and ADR-0023.

It does not implement the features.

## Context

The V2 foundation needs capability negotiation, per-peer grants and presence
to work between arbitrary trusted peers.

The roadmap intentionally left several questions open:

- whether widening a grant should take effect without reconnecting;
- whether HELLO should carry capability-specific metadata;
- whether presence is itself a capability;
- how the UI describes why an operation is unavailable;
- whether presence should become auto-granted.

Those questions prevented an autonomous worker from treating #5 and #8 as
implementation contracts.

## Decision

### D1 — Grant widening still requires reconnect

ADR-0008 remains unchanged.

Narrowing a grant is immediate and fail-closed.

Widening a grant takes effect on the next authenticated session. V2 does not
add a runtime mechanism that expands a peer's authorization.

This deliberately chooses safety and simplicity over instantaneous widening.

### D2 — HELLO advertises capability ids, not capability-specific state

HELLO and HELLO_ACK continue to advertise the versioned capability ids the
device supports.

Capability-specific state, roles and runtime availability stay inside the
capability that owns them.

The transport does not gain capability-specific fields.

ADR-0017 remains the model for runtime roles.

### D3 — Foundation capability identifiers

Existing identifiers remain unchanged:

- battery.v1
- clipboard.v1
- files.v1
- notifications.v1

The following identifiers are reserved for their feature specifications:

- share-open.v1
- text-input.v1

There is no presence.v1 in the V2 foundation.

The remote-control family is not named here. Its identifiers remain owned by
#28 and its future ADR/SPEC.

### D4 — Basic presence is derived trusted-peer state

Basic presence is not an authorization-bearing capability.

For a peer already present in the local trust store, a device may expose local
observations such as:

- connected;
- connecting;
- reachable;
- unreachable;
- last successful authenticated contact;
- sanitized descriptive platform metadata.

These observations never create trust and never grant a capability.

An unauthenticated DNS-SD record is discovery, not trusted presence.

Last-seen is local diagnostic state, not a synchronized activity history.

### D5 — Battery remains separate

Battery data remains battery.v1.

This ADR does not add another auto-granted capability. The existing
battery.v1 policy remains unchanged.

Presence must not be used as a back door for arbitrary peer metadata.

### D6 — Availability reasons

A controller or UI may explain an unavailable operation using these semantic
classes:

- peer_unsupported — the peer did not advertise the capability;
- not_granted — the local effective grant does not permit it;
- platform_unsupported — the local platform adapter cannot provide it;
- runtime_unavailable — the capability exists but cannot act in the current
  runtime state.

These are explanatory states, not authority.

A capability may define a wire representation for its own runtime
availability. There is no generic transport-level availability authority.

## Consequences

The V2 foundation can be implemented without adding runtime grant widening,
generic capability metadata to HELLO, or a presence permission.

The effective capability set remains:

supported by both peers
AND locally granted
AND valid for the capability's current role/runtime state.

Roadmap issues remain planning artifacts. Implementation must be decomposed
into bounded issues whose ADRs and SPECs already exist.

## Compatibility

No V1 grant becomes broader.

No existing capability identifier changes.

A V1 peer continues to negotiate only the identifiers and protocol version it
already understands.

## Security

This ADR intentionally avoids three new authority paths:

- discovery does not become presence authority;
- presence does not become authorization;
- a runtime message cannot widen a grant.
