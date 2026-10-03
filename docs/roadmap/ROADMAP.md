# Pliwee roadmap

This is where Pliwee is heading after V1: from an Android ↔ desktop companion to
a platform on which any of a person's trusted devices can work directly with
any other.

**The roadmap is directional, not a promise.** It has no delivery dates. Items
move, change shape or are dropped as research, security review, platform APIs
and technical validation teach us more. An item listed here is an intent, and
its issue holds the constraints and open decisions — not a specification. Work
that changes an existing protocol starts with an ADR or a SPEC, never with code.

## How to read it

| State | Meaning |
| --- | --- |
| **Shipped** | Released, and covered by a certification or a release record |
| **In progress** | Being built on a branch now |
| **Planned** | Committed direction for its milestone; design still open |
| **Exploring** | Research first. May become planned, change shape, or be dropped |

Priorities rank items *within* a milestone: **P0** architectural foundation the
rest depends on · **P1** core features · **P2** UX and device management ·
**P3** later or exploratory, gated on research or an ADR.

Milestones on GitHub: [Pliwee V2](https://github.com/yurisismotto/pliwee/milestone/1) ·
[Pliwee V2.1](https://github.com/yurisismotto/pliwee/milestone/2) ·
[Pliwee V3 / Future](https://github.com/yurisismotto/pliwee/milestone/3).
Every roadmap issue carries the
[`roadmap`](https://github.com/yurisismotto/pliwee/labels/roadmap) label.

## The direction: any trusted device to any trusted device

V1 has one shape: an Android device paired with a desktop. V2 replaces that with
one model for every pair of devices:

```
Device A  ──  Pliwee protocol  ──  Device B
```

Galaxy phone ↔ Galaxy tablet, phone ↔ Fedora, tablet ↔ Fedora — and later
Windows, macOS, Android TV / Google TV and, possibly, iOS/iPadOS — without
writing Android→Fedora, Android→macOS and so on as separate integrations.

* **No mandatory hub.** A desktop is a peer, not the centre. Two phones meet
  directly.
* **A trust group, not a network.** A person's devices form a logical group of
  devices that trust each other. "Pliwee Space" is the working name in design
  documents; the name the app shows is not decided.
* **Capabilities decide what two devices can do together:** the intersection of
  what both support, narrowed by what each has granted the other.

## What does not change

These hold for every item below. Changing any of them needs an ADR of its own.

* **Being on the same Wi-Fi is not trust.** Discovery grants nothing
  ([ADR-0005](../adr/ADR-0005-lan-discovery-mdns.md)).
* **Identity is a key.** Each device has its own cryptographic identity, and
  trust starts with explicit pairing confirmed by a human, with proof of
  possession ([ADR-0006](../adr/ADR-0006-device-identity-and-pairing.md)).
* **TLS 1.3 with mutual authentication and SPKI pinning** on every connection
  ([ADR-0007](../adr/ADR-0007-tls-transport-and-pinning.md)).
* **Deny by default.** Every feature is a capability granted per device and
  revocable at any time
  ([ADR-0008](../adr/ADR-0008-capability-architecture.md)).
* **No account, no telemetry, nothing in logs that a user wrote.** Local-first,
  with the LAN as the transport; V2.1 opens that question only through an ADR.

## Shipped — V1

| Area | What shipped | Reference |
| --- | --- | --- |
| Desktop | Linux service, CLI and app; runtime certified on Fedora, Ubuntu 24.04 / 26.04 LTS and Debian 13 | [README](../../README.md#supported-platforms) |
| Android | Companion app | [android/README.md](../../android/README.md) |
| Security | Device identity, QR pairing with fingerprint confirmation, TLS 1.3 + SPKI pinning | [ADR-0006](../adr/ADR-0006-device-identity-and-pairing.md), [ADR-0007](../adr/ADR-0007-tls-transport-and-pinning.md) |
| Capabilities | `files.v1`, `clipboard.v1`, `notifications.v1` (Android → Linux), `battery.v1` | [Capabilities in detail](../../README.md#capabilities-in-detail) |
| Permissions | Per-device grants, deny by default; pairing revocation | [ADR-0008](../adr/ADR-0008-capability-architecture.md), [revoked-device cleanup](../reports/security/REVOKED-DEVICE-CLEANUP-V1.md) |
| Releases | Signed release artifacts | [Verifying a release download](../../README.md#verifying-a-release-download) |

## In progress

| Item | Where |
| --- | --- |
| macOS desktop (preview) | branch `feature/macos-desktop-v1`, ADR-0021 (proposed) |
| Android on Google Play | readiness audits in [`audits/android/`](../audits/android/) |

Windows is **planned** as a platform (Waves 5–6 of the
[platform-expansion plan](../research/platform-expansion/22-IMPLEMENTATION-ROADMAP.md)),
and not yet assigned to a milestone.

## V2 — Multi-device foundation

[Milestone](https://github.com/yurisismotto/pliwee/milestone/1). Every item is
**Planned**.

### P0 — Architectural foundation

| Item | Issue |
| --- | --- |
| Multi-device mesh: any-to-any communication between trusted devices (umbrella) | [#4](https://github.com/yurisismotto/pliwee/issues/4) |
| Capability negotiation between any two devices | [#5](https://github.com/yurisismotto/pliwee/issues/5) |
| Trust group ("Pliwee Space") | [#6](https://github.com/yurisismotto/pliwee/issues/6) |
| Per-device capability permissions on every device | [#7](https://github.com/yurisismotto/pliwee/issues/7) |
| Device presence: online/offline, last seen, platform, capabilities, battery | [#8](https://github.com/yurisismotto/pliwee/issues/8) |

### P1 — Core features

| Item | Issue |
| --- | --- |
| Notifications sync to every eligible device (Android → desktops first) | [#9](https://github.com/yurisismotto/pliwee/issues/9) |
| Android TV / Google TV as a Pliwee platform (umbrella) | [#10](https://github.com/yurisismotto/pliwee/issues/10) |
| Android TV remote control | [#11](https://github.com/yurisismotto/pliwee/issues/11) |
| Android TV text input | [#12](https://github.com/yurisismotto/pliwee/issues/12) |
| Android TV share / open | [#13](https://github.com/yurisismotto/pliwee/issues/13) |
| Android TV device presence | [#14](https://github.com/yurisismotto/pliwee/issues/14) |
| Selective multi-device clipboard | [#15](https://github.com/yurisismotto/pliwee/issues/15) |
| Selective multi-device file transfer | [#16](https://github.com/yurisismotto/pliwee/issues/16) |

### P2 — UX and device management

| Item | Issue |
| --- | --- |
| Generic share / open between devices | [#17](https://github.com/yurisismotto/pliwee/issues/17) |
| Device dashboard | [#18](https://github.com/yurisismotto/pliwee/issues/18) |
| Revoke / remove a trusted device across the trust group | [#19](https://github.com/yurisismotto/pliwee/issues/19) |
| Rename device | [#20](https://github.com/yurisismotto/pliwee/issues/20) |
| Per-device preferences | [#21](https://github.com/yurisismotto/pliwee/issues/21) |
| Clipboard sync modes: manual, automatic with selected devices, disabled | [#22](https://github.com/yurisismotto/pliwee/issues/22) |
| Transfer history — local and private by default | [#23](https://github.com/yurisismotto/pliwee/issues/23) |
| Transfer progress, cancel, retry and acknowledgement | [#24](https://github.com/yurisismotto/pliwee/issues/24) |

## V2.1

[Milestone](https://github.com/yurisismotto/pliwee/milestone/2). Each item
starts with an ADR.

| Item | State | Issue |
| --- | --- | --- |
| Authentication handoff, phone → Android TV / Google TV, through official sign-in mechanisms only — no password capture or injection | Exploring | [#25](https://github.com/yurisismotto/pliwee/issues/25) |
| Nearby transport: Wi-Fi Direct, Bluetooth, other nearby APIs, under the same logical protocol | Exploring | [#26](https://github.com/yurisismotto/pliwee/issues/26) |
| Remote relay foundation — architecture and threat model only, no service | Exploring | [#27](https://github.com/yurisismotto/pliwee/issues/27) |

## V3 / Future

[Milestone](https://github.com/yurisismotto/pliwee/milestone/3). **Exploring,
and not committed scope.** These have no issues yet on purpose; one is opened
when an item has a research question worth tracking.

* Full remote relay / internet connectivity
* Pliwee Mirror — screen mirroring
* Pliwee Find — locating a device
* iOS / iPadOS ([feasibility research](../research/platform-expansion/11-IOS-IPADOS-FEASIBILITY.md))
* Screen streaming
* Audio routing
* Remote desktop / remote control of a computer
* Folder synchronisation
* Cloud storage
* Backup

Earlier ideas for mirroring (`#39`), a visual device card (`#41`) and Find My
Device (`#42`) are recorded as issues in the archived
`yurisismotto/omnibridge-history` repository, which is kept unchanged.

## Dependencies

```
Multi-device mesh (#4)
 ├── Capability negotiation (#5)
 ├── Trust group / device identity (#6)
 ├── Device presence (#8)
 └── Per-device permissions (#7)

Selective clipboard (#15)   ── mesh + capability negotiation
Selective files (#16)       ── mesh + capability negotiation

Android TV (#10)
 ├── presence (#8) + capability negotiation (#5)
 ├── remote control (#11)
 ├── text input (#12)
 ├── share / open (#13)
 └── TV presence (#14)

Authentication handoff (#25) ── Android TV + security architecture
Nearby transport (#26)       ── transport abstraction (#4)
Remote relay (#27)           ── transport abstraction (#4) + security review
```

The issues record the same relationships as GitHub sub-issues and
"blocked by" links.

## Decisions needed before implementation

| Decision | Why it is needed | Issue |
| --- | --- | --- |
| Peer-to-peer topology | ADR-0005 fixes "desktop advertises, phone dials"; any-to-any has to supersede it | [#4](https://github.com/yurisismotto/pliwee/issues/4) |
| Pairing between any two device kinds | ADR-0006's flow is desktop-shows-QR, phone-scans; a TV has no camera | [#4](https://github.com/yurisismotto/pliwee/issues/4), [#10](https://github.com/yurisismotto/pliwee/issues/10) |
| Pairwise or transitive trust in a group | Transitive trust lets one compromised member enrol devices everywhere | [#6](https://github.com/yurisismotto/pliwee/issues/6) |
| Revocation propagation | Whether a member may revoke a device on the others' behalf | [#19](https://github.com/yurisismotto/pliwee/issues/19) |
| Widening a grant without reconnecting | ADR-0008 makes widening wait for the next connection | [#5](https://github.com/yurisismotto/pliwee/issues/5) |
| OTP filtering of notifications | The current design rules out Pliwee-side OTP detection ([NOTIFICATIONS.md](../architecture/NOTIFICATIONS.md)) | [#9](https://github.com/yurisismotto/pliwee/issues/9) |
| TV remote-control mechanism | System-wide input needs privileges principle 8 forbids; what official APIs allow must be measured | [#11](https://github.com/yurisismotto/pliwee/issues/11) |
| `share-open` URL safety | Incoming URLs are untrusted even from a trusted device | [#13](https://github.com/yurisismotto/pliwee/issues/13), [#17](https://github.com/yurisismotto/pliwee/issues/17) |
| Transfer resume | Needs durable partial state | [#24](https://github.com/yurisismotto/pliwee/issues/24) |
| Authentication handoff | Credentials must only go through official sign-in flows | [#25](https://github.com/yurisismotto/pliwee/issues/25) |
| Nearby transports | Permissions, Play-services dependency, hardware support | [#26](https://github.com/yurisismotto/pliwee/issues/26) |
| Leaving the LAN | Supersedes the "LAN is the only transport" and "no cloud" principles | [#27](https://github.com/yurisismotto/pliwee/issues/27) |

## Related planning documents

* [Platform-expansion research](../research/platform-expansion/README.md):
  the [implementation roadmap](../research/platform-expansion/22-IMPLEMENTATION-ROADMAP.md)
  and [backlog](../research/platform-expansion/25-IMPLEMENTATION-BACKLOG.md)
  order the *platform ports* (Linux, Windows, macOS, iOS). This document orders
  the *product*: what devices can do together. They are complementary.
* [Architecture decision records](../adr/README.md) and the
  [threat model](../security/THREAT_MODEL.md) — the constraints every item above
  works within.

## Changing the roadmap

Propose a change in an issue, or in a pull request that edits this file. When
an item's state changes, update its row here and its issue together.
