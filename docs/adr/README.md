# Architecture Decision Records

Each ADR records a decision that would be expensive to reverse, the
alternatives that were actually considered, and what the decision costs.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](ADR-0001-monorepo-structure.md) | Monorepo structure | Accepted |
| [0002](ADR-0002-android-native-kotlin.md) | Native Kotlin on Android | Accepted |
| [0003](ADR-0003-rust-desktop-daemon.md) | Rust desktop daemon | Accepted |
| [0004](ADR-0004-protocol-buffers.md) | Protocol Buffers, compiled with protox | Accepted |
| [0005](ADR-0005-lan-discovery-mdns.md) | LAN discovery via mDNS/DNS-SD | Accepted · fixed connection direction superseded by 0022 once implemented |
| [0006](ADR-0006-device-identity-and-pairing.md) | Device identity and pairing | Accepted · pairing roles superseded by 0022 once implemented (identity, token, proof and pinning unchanged) |
| [0007](ADR-0007-tls-transport-and-pinning.md) | TLS 1.3 transport and SPKI pinning | Accepted |
| [0008](ADR-0008-capability-architecture.md) | Capability-based protocol architecture | Accepted |
| [0009](ADR-0009-android-background-execution.md) | Android background execution | Accepted · amended by 0022 once implemented (the service also hosts a listener) |
| [0010](ADR-0010-protocol-envelope-and-framing.md) | Protocol envelope and framing | Accepted |
| [0011](ADR-0011-project-naming-and-wire-identifiers.md) | Project naming and wire identifiers | Accepted · identifier tables superseded by 0018 |
| [0012](ADR-0012-bulk-transfer-and-frame-limit.md) | Bulk transfer and the 64 KiB frame limit | Accepted |
| [0013](ADR-0013-file-transfer-data-stream.md) | The `files.v1` authenticated data stream | Accepted · dial direction amended by 0022 once implemented |
| [0014](ADR-0014-clipboard-change-notification.md) | Detecting clipboard changes on the Linux desktop | Accepted |
| [0015](ADR-0015-notification-access.md) | Android notification access and the `notifications.v1` security boundary | Accepted |
| [0016](ADR-0016-notification-identity.md) | `notifications.v1` opaque notification identity | Accepted |
| [0017](ADR-0017-capability-roles.md) | Runtime-narrowable capability roles | Accepted |
| [0018](ADR-0018-rename-to-omnibridge.md) | Rename to OmniBridge | Accepted · supersedes 0011's identifier tables · identifier tables and no-dual-stack rule superseded by 0020 once implemented |
| [0019](ADR-0019-android-app-signing.md) | Android app signing: maintainer-owned key, Play App Signing, separate upload key | Accepted · signing identity amended by 0020 (custody model unchanged) |
| [0020](ADR-0020-rename-to-pliwee.md) | Rename to Pliwee: identity layers, legacy wire profile, state migration | Accepted · not yet implemented |
| 0021 | Reserved for macOS desktop integration (`feature/macos-desktop-v1`) | Proposed on macOS branch; not yet on `main` |
| [0022](ADR-0022-any-to-any-topology-and-symmetric-pairing.md) | Any-to-any topology and symmetric pairing | Accepted (2026-10-04) · supersedes 0005's fixed direction and 0006's pairing roles, amends 0009 and 0013 · not yet implemented; SPEC not yet written |
| [0023](ADR-0023-pliwee-space-trust-model.md) | Pliwee Space: pairwise trust for the multi-device group | Proposed (2026-10-04) · not accepted |
