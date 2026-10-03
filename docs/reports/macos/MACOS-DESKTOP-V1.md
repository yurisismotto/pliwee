# Sprint report — macOS desktop integration v1

**Branch:** `feature/macos-desktop-v1` · **Measured on commit:** `9b04197`
(the documentation commit follows it) · **Date:** 2026-10-03

| | |
| --- | --- |
| Hardware | MacBook Pro, Apple Silicon (`arm64`), one internal battery |
| OS | macOS 27.0 (build 26A428) |
| Toolchains | Rust 1.99.0 / cargo 1.99.0 (rustup, stable); Swift 6.4 (`swiftlang-6.4.0.34.1`), **Command Line Tools only — no Xcode** |
| Decisions | [ADR-0021](../../adr/ADR-0021-macos-desktop-integration.md) (Proposed) |
| How it works | [architecture/MACOS.md](../../architecture/MACOS.md) |

Status vocabulary in this report: **EXECUTED/PASS**, **EXECUTED/FAIL**,
**NOT EXECUTED** (with the reason). Nothing is reported as passing that was
not run.

---

## 1. Summary

The first native macOS integration of Pliwee: a SwiftUI menu-bar application,
`Pliwee.app`, for the same Rust agent that runs on Linux, with a macOS
platform adapter where — and only where — macOS differs.

Measured end to end on this Mac, with the agent running as a launchd agent
registered by the app and a peer speaking the real protocol over real TLS
(`daemon/examples/fake_phone`): the app opened a pairing window, the peer
paired with a pinned server identity, the app put the fingerprint in front of
the person and accepted it, granted `files.v1`, and approved an incoming file
through its floating prompt; the agent verified and stored it.

What does not work yet, by decision and documented: automatic clipboard
sending (PLAT-DEC-009), notification mirroring (no sink), Secure Enclave keys
(PLAT-DEC-004), distribution (no Developer ID, no notarization). What could not
be confirmed here: discovery from the launchd agent reaching the network,
which waits on a local-network consent this session could not see.

**No Linux behaviour was changed on purpose.** The Linux daemon code was moved
into `daemon/src/platform/linux.rs` unchanged; the whole workspace except the
GTK crate type-checks and is clippy-clean for `x86_64-unknown-linux-gnu`; no
Linux test was edited except to classify one Linux-only file (§6).

---

## 2. Discovery: the tree before any change

| Question | Finding |
| --- | --- |
| Does the workspace build on macOS as it is? | Yes, except `pliwee-gui` (GTK/libadwaita through `pkg-config`). Measured: `cargo check --workspace --exclude pliwee-gui --all-targets` on `aarch64-apple-darwin`, no error. |
| Baseline tests on macOS | **912 passed, 44 failed, 24 ignored** (`--exclude pliwee-gui`). All 44 failures are `pliwee-linux`'s D-Bus harnesses refusing to run without `dbus-daemon` — the "tool exists" rule of AGENTS.md doing its job. |
| How the agent runs on Linux | `systemd --user` unit, off until enabled; composes `pliwee-runtime` with `pliwee-linux` in `daemon/src/main.rs`. |
| How the GUI and the CLI reach it | `pliwee-control`'s newline-delimited JSON over a Unix socket in `$XDG_RUNTIME_DIR/pliwee/`; one request per connection; `pair`, `send`, `watch_file_offers` are streams. |
| What would have gone wrong on macOS as it was | `nix_uid()` reads `/proc` and falls back to uid 0, so the socket would have been `/tmp/pliwee-0/…`, shared by every user. Data under `~/.local/share`. |
| `cfg(target_os)` in the shared crates | None: every platform split was already a Cargo feature (Wave 0). |
| Platform enum | `core.proto` has `UNSPECIFIED`, `ANDROID`, `LINUX`. Android renders an unknown value as "no platform shown". |

---

## 3. What was built

### Rust

| Crate / file | Change |
| --- | --- |
| `desktop/platform-unix` (`pliwee-unix`) | **New.** The Unix-socket control transport, moved out of `pliwee-linux`, plus three tests (0700 directory, client round trip, `sun_path` limit measured against the kernel). |
| `desktop/platform-linux` | Re-exports the transport (API unchanged); `default_endpoint()` → `default_control_transport()`; `START_HINT`. |
| `desktop/platform-macos` (`pliwee-macos`) | **New.** `paths`, `keychain` (`SecretStore`), `battery` (IOKit), `clipboard` (`NSPasteboard`), `host`. Empty on non-macOS targets. |
| `desktop/daemon/src/platform/{mod,linux,macos}.rs` | **New.** The one place the agent selects its platform; `linux.rs` is the old `main.rs` code, moved. |
| `desktop/daemon/src/main.rs` | Calls `platform::…`; names no platform. |
| `desktop/cli` | Picks the socket path and start hint from the adapter. |
| `desktop/control/tests/control_fixtures.rs` + `fixtures/control-protocol.json` | **New.** One example of every request (22), response (8) and event (7), checked in both directions. |
| `desktop/core/tests/portable_boundary.rs` | `unsafe` allowed only in two declared FFI files, each block justified; `target_os` only at the two selection points. |
| `desktop/daemon/tests/legacy_state_migration.rs` | Compiled out on macOS (§6). |

New external crates, all `cfg(target_os = "macos")`: `security-framework`,
`security-framework-sys`, `core-foundation`, `core-foundation-sys`, `objc2`,
`objc2-encode`, `objc2-foundation`, `objc2-app-kit` — highest MSRV 1.85; the
workspace floor stays 1.88 (`time-macros`).

### Swift — `macos/`

| | |
| --- | --- |
| `PliweeKit` | Control-protocol models, Unix-socket client, runtime paths, device/action/service-health rules, design tokens. No UI framework. |
| `Pliwee` | The app: `MenuBarExtra`, the main window (`NavigationSplitView`: Overview, Devices, Files, Clipboard, Settings), pairing sheet, file-offer panel, `SMAppService` wrapper, debug-only validation hooks. |
| `Resources/` | `Info.plist`, the LaunchAgent plist. |
| `scripts/` | `build-app.sh` (bundle), `test.sh` (Swift tests), `render-icon.swift` (icon from the existing SVG). |

---

## 4. Results

### 4.1 Automated

| Check | Command | Result |
| --- | --- | --- |
| Rust format | `cargo fmt --all --check` | **EXECUTED/PASS** |
| Clippy, macOS | `cargo clippy --workspace --exclude pliwee-gui --all-targets --all-features -- -D warnings` | **EXECUTED/PASS** |
| Clippy, Linux target (from macOS) | same, `--target x86_64-unknown-linux-gnu` (ring's C built freestanding with Apple clang) | **EXECUTED/PASS** — type-check and lints only; nothing linked or run |
| Rust tests, macOS | `cargo test --workspace --exclude pliwee-gui --exclude pliwee-linux` | **EXECUTED/PASS — 879 passed, 0 failed, 26 ignored** (71 test binaries) |
| `pliwee-linux` unit tests on macOS | `cargo test -p pliwee-linux --lib` | **EXECUTED/PASS — 42 passed** |
| Real keychain round trips | `cargo test -p pliwee-macos --test keychain -- --ignored` | **EXECUTED/PASS — 3 passed**; no item left behind (checked with `security dump-keychain`) |
| Real pasteboard round trip (plain and concealed) | `cargo test -p pliwee-macos -- --ignored the_real_pasteboard` | **EXECUTED/PASS** |
| Swift tests | `macos/scripts/test.sh` | **EXECUTED/PASS — 44 tests in 8 suites** |
| Contract test bites | rename one request key in Swift | **EXECUTED/PASS** — `everyRequestEncodesToExactlyTheAgentsJson` fails, then restored |
| `unsafe` gates bite | drop a `// SAFETY:` comment; add `unsafe` to `pliwee-runtime` | **EXECUTED/PASS** — both new boundary tests fail, then restored |
| Bundle, release, arm64 | `macos/scripts/build-app.sh` | **EXECUTED/PASS** — 3 executables, identity checks, `codesign --verify --strict --deep` |
| Bundle, universal | `build-app.sh --arch universal` | **EXECUTED/PASS** — every executable `x86_64 arm64`; signature verifies |
| Debug hooks absent from release | `strings` on the release `Pliwee`, with a positive control | **EXECUTED/PASS** — hook string absent; a UI string present; the debug binary does contain the hook string |
| GTK GUI on macOS | `cargo test -p pliwee-gui` | **NOT EXECUTED** — GTK 4 / libadwaita are Linux packages; the crate is unchanged |
| `pliwee-linux` integration tests on macOS | `cargo test -p pliwee-linux --tests` | **NOT EXECUTED** — need a session `dbus-daemon`; they refuse without one, by design |
| Linux runtime regression | `cargo test` on Fedora/Ubuntu/Debian | **NOT EXECUTED** — no Linux host in this session; CI (`desktop-quality`, `linux-distro-compat`) runs it on push |
| Running the x86_64 slice | `arch -x86_64 …/pliwee --version` | **NOT EXECUTED** — "Bad CPU type": Rosetta 2 is not installed on this Mac, and installing it was not done |

### 4.2 Manual, on this Mac

Driven through `Pliwee.app`'s debug-only validation hooks (§7) and checked
from outside with `launchctl`, `lsof`, the agent log, the unified log and the
CLI. Window pixels are the app's own snapshots of its window.

| Behaviour | Result | Evidence |
| --- | --- | --- |
| `Pliwee.app` opens | **EXECUTED/PASS** | process running; window visible; activation policy `regular` |
| Menu-bar icon appears | **EXECUTED/PASS** | one `NSStatusBarWindow` owned by the app |
| Menu opens and shows its items | **NOT EXECUTED** | opening a status-item menu needs a click (Accessibility) or a screenshot (Screen Recording), neither granted. The menu's *data* — status, headline, device rows, each action's ready/blocked state — was logged from the same model calls the menu makes |
| Status is updated | **EXECUTED/PASS** | `Not running` → `Starting…` → `Disconnected` → `Connected · 1 device` → `Disconnected` as the agent and the peer came and went |
| Open Pliwee opens/focuses the window | **EXECUTED/PASS** | `open` step: window visible, policy `regular` |
| Window renders correctly | **EXECUTED/PASS** | snapshots of all five pages with live data; two defects found and fixed (§5.9) |
| Closing the window keeps the app | **EXECUTED/PASS** | window hidden, policy `accessory`, status-bar window still present, app running |
| Daemon detected correctly | **EXECUTED/PASS** | real identity, port 55432, four capabilities, clipboard `nspasteboard`, notifications `none` |
| Quit ends the app, not the agent | **EXECUTED/PASS** | app process gone; `pliweed` still running under launchd |
| Turn service on (`SMAppService.agent.register`) | **EXECUTED/PASS** | `launchctl print`: `managed_by = com.apple.xpc.ServiceManagement`, `state = running` |
| Turn service off (`unregister`) | **EXECUTED/PASS** | agent stopped; launchd job gone |
| Pairing from the app | **EXECUTED/PASS** | sheet showed the agent's QR (decoded back to the exact payload); fingerprint question shown; accepted; peer: "TLS established … server identity pinned" |
| Grants from the app | **EXECUTED/PASS** | `files.v1`, `clipboard.v1` granted; CLI `devices` agrees |
| Incoming file approval | **EXECUTED/PASS** | prompt shown with name, size, type, verified fingerprint; Accept; peer: `completed`; CLI: stored at `~/Downloads/Pliwee/…` |
| Peer battery shown | **EXECUTED/PASS** | a separate run with the peer's session held open: the app showed "Connected · 87% · Charging". In the first end-to-end run the reading was *not* shown, because the peer's file-sending session replaced the session that had carried it — correct behaviour (a reading is dropped with its session), but not evidence, so it was measured again |
| Action availability | **EXECUTED/PASS** | Send Clipboard blocked with "has not negotiated the clipboard" (the peer offers only battery and files); Send File blocked "is offline" after it left |
| Keychain storage of the key | **EXECUTED/PASS** | no `identity.key` file; one login-keychain item; `state.json` 0600 in 0700 |
| Socket protection | **EXECUTED/PASS** | `srw-------` in a `drwx------` `run/` |
| Agent log file | **EXECUTED/PASS** | `~/Library/Logs/Pliwee/pliweed.log`, 0600 |
| `mdns-sd` beside `mDNSResponder` (POC-MAC-02) | **EXECUTED/PASS** from a terminal | the system's `dns-sd -B` lists `_pliwee._tcp` on Wi-Fi and loopback |
| Bonjour from the launchd agent | **EXECUTED/FAIL** (not observed) | the agent logs "advertising"; its instance never appeared in `dns-sd -B`. See §5.3 |
| Open at Login (`SMAppService.mainApp`) | **NOT EXECUTED** | registering would change this user's login items; the switch reads the real status (`notRegistered`) |
| Clipboard send from Mac to a phone | **NOT EXECUTED** end to end | the test peer does not implement `clipboard.v1`; the backend's real round trip passed (§4.1) |
| A physical Android phone | **NOT EXECUTED** | none attached to this session |

---

## 5. Findings

Each found by running the thing, each fixed or documented.

1. **A missing keychain hangs the agent.** With `HOME` pointing where there is
   no keychain, `SecItemAdd` blocks behind a system dialog; `sample` showed it
   inside `SecKeychainItemCreateFromContent`. The keychain store now checks
   that `$HOME/Library/Keychains` holds a keychain and refuses by name; the
   agent exits in ~1 s with `IDENTITY_TEMPORARILY_UNAVAILABLE` and never
   regenerates the identity.
2. **APFS let the CLI overwrite the app.** `pliwee` copied beside `Pliwee` in
   `Contents/MacOS/` replaced it on the case-insensitive filesystem. Only the
   executable count noticed. The CLI is in `Contents/Helpers/`; the script now
   names every executable and checks the app binary links SwiftUI.
3. **Local network privacy could not attribute the agent.** An ad-hoc
   `pliweed` is named `pliweed-<cdhash>`; `nehelper` logged "Failed to find
   pliweed-… using neagent". Helpers are now signed with
   `io.github.yurisismotto.pliwee.daemon` / `.cli`, after which `nehelper`
   caches a UUID for the agent. The advertisement was still not observed on
   the network; the remaining explanation is the user's one-time consent,
   which this session can neither see nor answer. **Recorded as unconfirmed.**
   An earlier sighting in this session was a stale `mDNSResponder` cache entry
   for a deleted identity, and was retracted.
4. **`SMAppService` reads `.notFound` before the first registration.**
   Background Task Management logs "record not found". The app maps
   `.notFound` from a bundle that carries the plist to "not registered", and
   reserves "cannot be managed" for `swift run`.
5. **launchd refuses a rebuilt ad-hoc agent.** After a rebuild:
   `last exit code = 78: EX_CONFIG`, `needs LWCR update` — the launch
   constraint recorded at registration pinned the old binary. Unregister and
   register again recovers. The app has **Restart Service** for this; a
   Developer ID signature avoids it.
6. **A rebuilt agent asks for the keychain item.** Expected for ad-hoc
   signatures; the agent now logs, before touching the keychain, that a
   prompt may be what it is waiting on.
7. **A universal build lipo'd x86_64 twice.** SwiftPM writes every
   architecture to one product path. Each slice is now staged.
8. **SwiftUI `@State` and `@Environment` need Xcode.** They are macros in this
   SDK and their plugin ships with Xcode only; the app uses neither. Swift
   Testing's plugin ships with the Command Line Tools but is not loaded by
   SwiftPM's default build system; `test.sh` loads it.
9. **UI defects from the snapshots.** A truncated pairing instruction; grant
   switches at ragged positions. Fixed with a shared `SwitchRow`.
10. **A Linux sentence in a shared log line.** "Sending by hand … is usually
    unavailable too" is true on GNOME and false on macOS; the line moved into
    the platform modules, Linux text unchanged.

---

## 6. Tests changed

* `core/tests/portable_boundary.rs`: `no_crate_actually_uses_unsafe_today` is
  now `no_crate_uses_unsafe_outside_the_declared_ffi_modules`, with the new
  crates in scope and an allow-list of two files; added
  `every_declared_ffi_module_exists_uses_unsafe_and_justifies_each_block` and
  `the_platform_is_chosen_in_one_place_per_binary`. The deny-list test covers
  the two new crates. No assertion was weakened for an existing crate.
* `daemon/tests/legacy_state_migration.rs`: `#![cfg(not(target_os = "macos"))]`.
  It measures the Linux adapter's OmniBridge migration through
  `~/.local/share` and `XDG_RUNTIME_DIR`, which macOS does not have; run on a
  Mac it measured the absence of a feature and failed after 60 s per test.
  Compiled out rather than skipped, so it cannot report a result for what did
  not run. Unchanged on Linux.
* `pliwee-linux`'s four transport tests moved with the code to `pliwee-unix`.

---

## 7. How the manual validation was driven

The macOS lifecycle — open, close, register, quit — cannot be clicked or
photographed by a session without Accessibility and Screen Recording access,
and neither was requested. Debug builds of the app therefore carry
`DebugHooks`: an environment variable gives it a script (`wait`, `snap:<page>`,
`open`, `close`, `agent:register|unregister|status`, `pair`, `confirm:yes`,
`grant:<cap>`, `offer:yes`, `report`, `quit`), it calls the same methods its
menu and buttons call, and it writes what happened to a log and its window to
PNGs. Release builds do not contain it (§4.1). The peer was
`daemon/examples/fake_phone`, with its own identity in a temporary file store.

---

## 8. State of this Mac

Installed for the work: the Rust toolchain via rustup in `~/.rustup` and
`~/.cargo` (shell profiles not modified) and the `x86_64-apple-darwin` and
`x86_64-unknown-linux-gnu` standard libraries.

Left by the measurements and cleaned afterwards: the test agent registration,
the test identity (paired only with the test peer) and its keychain item. Not
removable from this session: `~/Downloads/Pliwee/pliwee-e2e-hello.txt`, the
file the end-to-end test received — `~/Downloads` is privacy-protected
against the shell that ran the tests.

---

## 9. Open items

| Item | Where it lands |
| --- | --- |
| Discovery from the launchd agent | confirm with the local-network consent granted; then POC-MAC-02 closes |
| Clipboard auto-send | PLAT-DEC-009; a `changeCount` watch in `pliwee-macos` |
| Notification sink | `UNUserNotificationCenter`, after Developer ID signing |
| Secure Enclave key | PLAT-DEC-004; a `rustls` signer behind `IdentityBackend` |
| `PLATFORM_MACOS` | its own ADR, with the Android label |
| Quarantine attribute on received files | SEC-006 |
| Developer ID signing, notarization, a macOS CI job | release engineering |
| A physical-phone pass on a Mac | the certification this report is not |
