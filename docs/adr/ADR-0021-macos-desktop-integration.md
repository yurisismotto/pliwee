# ADR-0021 — macOS desktop integration

Status: **Proposed (2026-10-03)** — awaiting the project owner's decision.
Implemented on branch `feature/macos-desktop-v1`; the evidence is
[`reports/macos/MACOS-DESKTOP-V1.md`](../reports/macos/MACOS-DESKTOP-V1.md).

**Relationship to earlier work.** Builds on the Wave 0 platform abstraction
([`reports/foundation/wave-0-platform-abstraction.md`](../reports/foundation/wave-0-platform-abstraction.md))
and follows the direction of research doc
[10 — macOS feasibility](../research/platform-expansion/10-MACOS-FEASIBILITY.md)
§12, which this ADR turns into decisions. It leaves **PLAT-DEC-004** (Secure
Enclave) and **PLAT-DEC-009** (declared clipboard polling) open, and says
where each would land. It changes no `.proto` file and no Android code.

---

## Context

Pliwee's desktop side was Linux-only in practice and Linux-shaped in
structure, but Wave 0 had already split it at the right seams: a portable
core and runtime, a control contract with no I/O (`pliwee-control`), and a
platform adapter (`pliwee-linux`) that the agent, the CLI and the GTK app are
composed with. Measured on an Apple Silicon Mac before any change, the whole
workspace except the GTK crate compiled for `aarch64-apple-darwin`.

What was missing was a macOS adapter, a way to compose the agent with it, a
macOS user interface, and a lifecycle.

## Decisions

### D1 — A native SwiftUI app that is a client of the agent

`Pliwee.app` is SwiftUI, with AppKit only where SwiftUI has no equivalent
(window lifecycle, the floating file-offer prompt, `NSOpenPanel`). It reaches
the agent over the agent's existing control socket with the existing
newline-delimited JSON protocol.

It implements **none** of: the wire protocol, pairing, TLS, identity, trust,
SPKI pinning, proof of possession, capability logic, policy. Those stay in the
Rust agent, which is the same code that runs on Linux.

Because Swift cannot share `pliwee-control`'s types the way the CLI and the GTK
app do, it shares a fixture instead: `desktop/control/tests/fixtures/
control-protocol.json` is generated from the Rust types and both test suites
check themselves against it.

*Rejected:* a GTK port (not native, and would carry GTK and libadwaita onto a
platform with its own toolkit); Swift↔Rust FFI into the agent (two runtimes in
one process, and the UI would hold the agent's lifetime); re-implementing
any protocol or security logic in Swift (research doc 10 §4.2: a second copy
of the pinning code is the one thing the shared core exists to prevent).

### D2 — `pliwee-macos` beside `pliwee-linux`, and `pliwee-unix` under both

`desktop/platform-macos` (`pliwee-macos`) is the macOS row of the adapter
table: paths, the Keychain secret store, the IOKit battery, the `NSPasteboard`
clipboard, the computer name. Every dependency and every line is
`cfg(target_os = "macos")`; on Linux it is an empty crate.

The control-socket transport moved from `pliwee-linux` to a new
`desktop/platform-unix` (`pliwee-unix`): it is POSIX, the live-owner rule in
`bind` is a security property, and the alternatives were a macOS crate
depending on "the Linux adapter" or a second copy of that rule.
`pliwee-linux` re-exports it, so its API is unchanged.

`pliweed` and `pliwee` choose their adapter by `target_os` in exactly one
place each (`daemon/src/platform/mod.rs`, `cli/src/main.rs`), enforced by
`core/tests/portable_boundary.rs`. Every target that is not macOS keeps the
Linux adapter it had.

`unsafe` is confined to two declared modules of `pliwee-macos` (IOKit's power
source API; one AppKit `extern` static), each block with a `// SAFETY:`
comment; the boundary test lists them and fails on `unsafe` anywhere else.
`pliwee-core` and the other portable crates keep `forbid`.

### D3 — The control socket lives at `~/Library/Application Support/Pliwee/run/control.sock`

| Candidate | Why not |
| --- | --- |
| `/tmp/pliwee-<uid>/` | `/tmp` is shared; another user can create the directory first |
| `$TMPDIR` (`/var/folders/…/T/`) | differs between a shell, an app and a `launchd` job unless each asks `confstr`; cleaned by the system on its own schedule |
| **`~/Library/Application Support/Pliwee/run/`** | **chosen** — under the user's home, so no other user can pre-create it; one answer for every process of the user from `$HOME` alone; never cleaned; `run/` keeps it out of the identity's own directory |

The cost is length: `sun_path` holds 103 bytes on macOS, and this path leaves
room for a home directory of 51 bytes. A longer one is refused by name rather
than truncated, moved, or left to `bind`'s bare `InvalidInput`. Both the Rust
and the Swift side assert the same literal paths.

### D4 — The agent is a per-user `launchd` agent registered by `SMAppService`

`pliweed` is bundled at `Contents/MacOS/pliweed` and described by
`Contents/Library/LaunchAgents/io.github.yurisismotto.pliwee.daemon.plist`. The
app registers it with `SMAppService.agent(plistName:)`:

* **no root**: a per-user agent in the user's Aqua session;
* **off by default**: nothing listens until the user turns it on — the same
  default as the Linux packages;
* **predictable**: `RunAtLoad` and `KeepAlive {SuccessfulExit: false}` with a
  5-second throttle, the launchd form of `Restart=on-failure`/`RestartSec=5`;
* **independent of the window and of the app**: closing the window keeps the
  app; quitting the app keeps the agent, as `pliweed` outlives the GTK app;
* **easy to remove**: turning it off unregisters it; deleting the app removes
  the plist with it;
* **visible**: it is listed in System Settings › General › Login Items.

*Open at Login* for the menu-bar app is `SMAppService.mainApp`, a separate
switch: the service runs with or without the menu-bar item.

*Rejected:* a `LaunchDaemon` (root, outside the user's session: no
pasteboard, no Downloads, no person — research doc 10 §7); a plist copied into
`~/Library/LaunchAgents` (less discoverable, leaves files behind, and needs
`launchctl` scripting `SMAppService` replaces); a child process of the app
(couples the agent's life to the menu-bar app's, which the Linux design
deliberately does not).

### D5 — The identity's private key is a login-keychain item

`KeychainSecretStore` implements Wave 0's `SecretStore` seam: the key is a
generic-password item (service `io.github.yurisismotto.pliwee`, account
`identity@<data dir>`), `state.json` stays a 0600 file. The trait's
load-bearing rule holds: only `errSecItemNotFound` is absence; every refusal
— locked, cancelled, denied, interaction not allowed — is an error, so a
keychain that says no can never cause an identity to be regenerated. A `HOME`
with no keychain fails by name instead of blocking in `SecItemAdd`.

The key is still a **software** key (`key_backing = software`). A Secure
Enclave key is PLAT-DEC-004 and needs the custom `rustls` signer that research
doc 10 §5 describes; the `IdentityBackend` seam is where it goes.

### D6 — The wire protocol is unchanged; macOS reports `PLATFORM_UNSPECIFIED`

`core.proto` has no macOS value. Reporting `PLATFORM_LINUX` would be false;
adding `PLATFORM_MACOS = 3` is additive and safe by ADR-0004's rules, but it is
a protocol change and deserves its own decision with the Android side. The
Android app already renders `UNSPECIFIED` as "no platform shown" rather than a
wrong label (`UiMapping.platformLabel`). The value is supplied on every load
and never persisted, so adding the enum value later costs nothing.

### D7 — `clipboard.v1` without automatic sending; `notifications.v1` without a sink

* **Clipboard.** `NSPasteboard` read and write; a sensitive clip is written as
  one pasteboard item that also carries `org.nspasteboard.ConcealedType`, the
  convention clipboard managers honour (a hint, as on every platform).
  `NSPasteboard` has no change notification, and the `ClipboardBackend`
  contract forbids satisfying `watch_changes` by polling. Amending that is
  PLAT-DEC-009, which is open; until it is decided, automatic sending reports
  unavailable with its reason and manual sending works. Accepting PLAT-DEC-009
  would add a `changeCount` watch to `pliwee-macos` and nothing else.
* **Notifications.** Displaying a phone's notifications needs
  `UNUserNotificationCenter`, a signed bundle and a user authorisation. The
  agent registers `notifications.v1` with the portable `NoSink`, so it
  announces no `SINK` role and phones do not send it notifications it would
  never show.

### D8 — A Swift package and a build script, not an Xcode project

`macos/` is a Swift package (`PliweeKit`, the testable half; `Pliwee`, the
app) that builds with the Command Line Tools alone and opens in Xcode
unchanged. `macos/scripts/build-app.sh` assembles the bundle. Without a
Developer ID the bundle is signed ad hoc and is not distributable; signing
with `--sign` and notarizing are documented, and not simulated.

## Consequences

* Linux behaviour is unchanged: `platform/linux.rs` is the old `main.rs`
  code moved, and every Linux test still runs on Linux. The Linux source
  bundle now also vendors eight macOS-only crates, as it already vendors
  `windows-sys`; none is compiled there. The 1.88 Rust floor is unchanged.
* Three development-only frictions come from ad-hoc signing, and a Developer
  ID removes all three: a rebuilt `pliweed` triggers a keychain access prompt;
  launchd refuses a rebuilt `pliweed` (`EX_CONFIG`) until it is registered
  again — the app's **Restart Service** does that; and `AssociatedBundleIdentifiers`
  is ignored without a Team ID, so Login Items names the agent `pliweed`.
* Discovery uses the shared `mdns-sd` responder beside `mDNSResponder`. Run
  from a terminal, the two interoperate: the system's own `dns-sd` browser
  sees the advertisement (POC-MAC-02's question, answered). Run as the
  launchd agent, the advertisement was **not** observed on the network
  during this work. Local network privacy applies to agents; it first could
  not attribute an anonymous ad-hoc `pliweed` to anything, which signing the
  helper with a stable identifier fixed (`nehelper` then caches it), and what
  remains is the user's one-time local-network consent, which could not be
  observed or answered in this session. Pairing by QR code does not depend
  on discovery: the code carries the Mac's addresses.
  *Note, 2026-10-03 (closure round, status unchanged):* with the consent
  given, the advertisement of the agent run by the background service was
  observed and resolved by the system's Bonjour client on the LAN interfaces,
  endpoint matching the launchd agent; no code change was needed
  ([report §10.2](../reports/macos/MACOS-DESKTOP-V1.md)). Observation from a
  second host (a phone) remains to be done. The closure round recommends
  keeping this ADR *Proposed* until the Fedora regression has run (§10.7).
* Open: PLAT-DEC-004 (Secure Enclave), PLAT-DEC-009 (declared clipboard
  polling), a notification sink, `PLATFORM_MACOS`, quarantine attribute on
  received files (SEC-006), Developer ID signing and notarization in CI.
