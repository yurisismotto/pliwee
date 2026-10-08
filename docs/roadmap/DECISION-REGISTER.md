# Pliwee architecture decision register

**Updated:** 2026-10-07

This file is the readiness view of decisions that can block roadmap
implementation.

The ADR index remains the source of truth for decisions already accepted.
The roadmap remains the source of product direction.

States:

- RESOLVED — an Accepted ADR/SPEC fixes the decision;
- OPEN — owner/ADR/SPEC work is required before implementation;
- RESEARCH — measured feasibility comes before a decision;
- DEFERRED — intentionally outside the current implementation wave.

| Decision | State | Canonical owner |
| --- | --- | --- |
| Any-to-any peer topology | RESOLVED | ADR-0022 |
| Symmetric pairing between device kinds | RESOLVED | ADR-0022 |
| Pairwise vs transitive trust | RESOLVED | ADR-0023: pairwise |
| Android advertising policy | RESOLVED | ADR-0022 |
| Duplicate-session selection | RESOLVED | ADR-0022 |
| Grant widening without reconnect | RESOLVED | ADR-0024: widening waits for reconnect |
| HELLO capability metadata | RESOLVED | ADR-0024: ids only |
| Basic trusted-peer presence model | RESOLVED | ADR-0024: derived local state |
| Presence auto-grant | RESOLVED | no presence capability; battery.v1 unchanged |
| Availability reason classes | RESOLVED | ADR-0024 |
| ADR-0022 implementation details | RESOLVED | MULTI-DEVICE-MESH-V2.md |
| Revocation propagation | OPEN | #19; V2 currently stays local |
| Remote-control capability ids and boundaries | OPEN | #28 ADR |
| Remote-control authorization granularity | OPEN | #28/#31 ADR |
| Destructive-action confirmation and lock policy | OPEN | #28/#31 ADR |
| Command identity, idempotency, replay and expiry | OPEN | #28/#31 ADR |
| Scheduled-action ownership/persistence/clock | OPEN | #32 ADR |
| Automation trigger vocabulary | OPEN | #33 ADR |
| Android notification-listener use for media control | OPEN | #28/#29 ADR |
| Remote-control wire schemas and platform semantics | OPEN | #29-#31 SPEC |
| Remote-control threat model | OPEN | #28 security update |
| OTP treatment for mirrored notifications | OPEN | #9 ADR |
| Android TV navigation mechanism | RESEARCH | #11 |
| share-open URL safety | OPEN | #13/#17 SPEC |
| Partial-transfer resume | DEFERRED | #24; retry-from-start remains V2 baseline |
| Authentication handoff | RESEARCH | #25 V2.1 |
| Nearby transport choice | RESEARCH | #26 V2.1 |
| Communication outside the LAN / relay | DEFERRED | #27 V2.1 ADR |

## Readiness rule

A roadmap issue is not itself an implementation contract.

Before autonomous coding, implementation work is cut into bounded child issues
that have:

- no unresolved OPEN decision in their scope;
- every required ADR Accepted;
- every required SPEC present;
- explicit Scope and Out of scope;
- observable acceptance criteria;
- test/evidence requirements;
- dependency links.

A BLOCKED or OWNER_DECISION_REQUIRED result belongs to that issue. It does not
stop unrelated autonomous work in the same night session.

SECURITY and FAILED_INFRA remain session-fatal.
