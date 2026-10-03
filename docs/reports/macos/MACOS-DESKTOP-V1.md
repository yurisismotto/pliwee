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

> **Closure round — 2026-10-03.** §10 records a validation round run after
> this report was written, against commit `2e41ea2`. It supersedes two rows of
> §4.2 in part — *Menu-bar icon appears* and *Bonjour from the launchd agent* —
> with dated notes at those rows; the original rows stand. Its verdict is in
> §10.9.

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
| ↳ *note, 2026-10-03 (§10.4)* | *narrowed* | *that observation was made inside the app's process. It proves the status item was **created**, not that it is **visible**: macOS 26+ lets a person hide an app's menu-bar item, and the window server, without Screen Recording access, exposes no status-layer window to check. Visible on screen is NOT EXECUTED* |
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
| ↳ *note, 2026-10-03 (§10.2)* | *superseded* | *re-measured with the agent started by the background service and the local-network consent given: the instance is advertised, resolved, and its endpoint is the launchd agent. EXECUTED/PASS* |
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

---

## 10. Closure round — 2026-10-03

A validation round, not a development round: run the gates again on the
committed tree, close the gaps that can be closed with evidence, and say
plainly which cannot. Same Mac as §1.

| | |
| --- | --- |
| Commit tested | `2e41ea2` on `feature/macos-desktop-v1`; working tree clean; `git diff --check` clean |
| Identity | `Yuri Converso Sismotto <yuri.sismotto@hotmail.com>` — the e-mail on every commit in the history |
| Code changes in this round | **none** |

### 10.1 Automated gates, re-run

| Gate | Command | Result |
| --- | --- | --- |
| Rust format | `cargo fmt --check` | **EXECUTED/PASS** |
| Rust tests, macOS | `cargo test --workspace --exclude pliwee-gui --exclude pliwee-linux` | **EXECUTED/PASS — 879 passed, 0 failed, 26 ignored** (71 binaries) |
| Clippy, as specified | `cargo clippy --workspace --exclude pliwee-gui --exclude pliwee-linux --all-targets --all-features -- -D warnings` | **EXECUTED/PASS** |
| Clippy, including `pliwee-linux` | the same without `--exclude pliwee-linux` (it compiles on macOS) | **EXECUTED/PASS** |
| `pliwee-linux` unit tests | `cargo test -p pliwee-linux --lib` | **EXECUTED/PASS — 42 passed** |
| macOS and Unix adapters | `cargo test -p pliwee-macos -p pliwee-unix` | **EXECUTED/PASS — 29 + 7 passed**; 4 ignored are the real keychain/pasteboard round trips, which passed in §4.1 and were not re-run, to leave the user's keychain and clipboard alone |
| Swift tests | `macos/scripts/test.sh` | **EXECUTED/PASS — 44 tests in 8 suites** |
| Clean debug build | `rm -rf macos/build && macos/scripts/build-app.sh --debug` | **EXECUTED/PASS** |
| Universal build | `macos/scripts/build-app.sh --arch universal` | **EXECUTED/PASS** — every executable `x86_64 arm64`; `codesign --verify --strict --deep` passes |

### 10.2 Bonjour with the agent run by the background service

The service type is read from the code, not assumed:
`desktop/core/src/lib.rs` — `SERVICE_TYPE = "_pliwee._tcp.local."`,
`LEGACY_SERVICE_TYPE = "_omnibridge._tcp.local."`; `runtime/src/mdns.rs`
advertises both, instance name = device id.

The agent measured was the one **the user** had turned on from `Pliwee.app`
before this round (its identity was created at 02:20:38; this round did not
start it). How it was identified as the service's process:

| Check | Observation |
| --- | --- |
| launchd | `launchctl print gui/501/io.github.yurisismotto.pliwee.daemon`: `submitted by smd`, `state = running`, `pid = 31205`, `parent bundle identifier = io.github.yurisismotto.pliwee`, `program identifier = Contents/MacOS/pliweed` |
| process | `ps`: pid 31205, ppid **1** (launchd), user `yuri.sismotto` |
| signing | `io.github.yurisismotto.pliwee.daemon`, ad hoc |
| listener | `lsof -iTCP:55432 -sTCP:LISTEN` → `pliweed 31205 … TCP *:55432 (LISTEN)` |
| identity | `state.json` device id `8d0cc84faf9c084c3ed410c3ddcc4b7b`, 0 peers |

Observed with the system's own Bonjour client, outside the agent's process:

| Step | Result |
| --- | --- |
| `dns-sd -B _pliwee._tcp local.` | instance `8d0cc84f…` on interfaces 1 (`lo0`), 11 (`en0`), 20 (`en5`) |
| `dns-sd -B _omnibridge._tcp local.` | the same instance on 1, 11, 20 |
| `dns-sd -L 8d0cc84f… _pliwee._tcp local.` | `8d0cc84f….local.:55432`, TXT `v=1 pv=1-1 id=8d0cc84f… dn=MacBook Pro de Yuri` |
| `dns-sd -G v4v6 8d0cc84f….local.` | 192.168.68.61 and 192.168.68.69 on `en0` and `en5` — `en0`'s address is 192.168.68.61 |
| endpoint ↔ daemon | port 55432 is held by pid 31205, the launchd agent |

An entry for `893dbf94…` also appears: that is an identity deleted in the
first round, still in `mDNSResponder`'s cache from a terminal run, and is not
counted. The instance counted is the one whose name is the running agent's
device id.

**Why this differs from §4.2.** The unified log shows System Settings open at
02:19:50 and, at 02:20:40, `UserEventAgent … LocalNetwork: found bundle id
io.github.yurisismotto.pliwee.daemon by PID`: local network privacy
attributed the agent by the signing identifier `build-app.sh` gives it since
§5.3, and the user's consent was in place. No code change was needed. The
terminal-run differential (B) was therefore not required.

**Result: EXECUTED/PASS** — observed by a Bonjour client outside the agent,
on the LAN interfaces, while `pliweed` ran as the service. Observation from a
*different host* is the Android discovery gate (§10.3, A), not executed.

### 10.3 Physical Android

No Android device was reachable from this session: no `adb`, no USB device,
and the running agent had no paired peer. Per the round's rule, `fake_phone`
was not substituted.

| Gate | Result |
| --- | --- |
| A Discovery · B Pairing · C TLS/pinning · D Reconnect · E Android→Mac file · F Rejection · G Mac→Android file · H Clipboard Android→Mac · I Clipboard Mac→Android · J Daemon restart · K App restart · L Mac reboot | **NOT EXECUTED** — no physical Android device in this session |

For G: Mac→Android sending exists in the control contract (`Request::Send`)
and in the app (Send File…), so when it is run it is a real gate, not
NOT IMPLEMENTED. For I: manual sending exists; automatic sending is
deliberately absent (PLAT-DEC-009) and is not a defect.

L was not run for a second reason: rebooting would end the working session.

### 10.4 Menu bar

| Check | Result |
| --- | --- |
| Interaction capability | `AXIsProcessTrusted() = false`, `CGPreflightScreenCaptureAccess() = false` — preflight calls, which show no prompt |
| Icon present (status item created) | **EXECUTED/PASS** in round 1, in-process (§4.2) |
| Icon visible on screen | **NOT EXECUTED** — the window server lists no status-layer window (layer 25 is empty system-wide on macOS 27) and window contents need Screen Recording |
| Click opens the menu | **NOT EXECUTED** — needs Accessibility or a person |
| Status, devices, Open Pliwee, Settings, Quit *from the menu* | **NOT EXECUTED** — the actions behind them were exercised through the debug hooks in round 1, which is not the menu |

### 10.5 Fedora / Linux regression

**NOT EXECUTED — requires a Fedora/Linux host.** The Linux-target type-check
and clippy in §4.1 are not a regression run and are not counted as one.

The commands, from the repository's own scripts and CI, for a Fedora 44 host
with this branch checked out:

```bash
# 0. prerequisites (README "Build from source")
sudo dnf install gcc pkgconf-pkg-config rust cargo gtk4-devel libadwaita-devel glib2-devel \
                 wl-clipboard upower dbus-daemon

# 1. the workspace — the whole of it, GUI and D-Bus suites included
cd desktop
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings   # as desktop-quality.yml
cargo test --locked --workspace                                                # pliwee-linux tray/D-Bus, daemon, CLI, GUI
cd ..

# 2. the harnesses' own self-tests and the static packaging checks
packaging/tests/harness-selftests.sh
packaging/tests/packaging-checks.sh

# 3. the systemd user unit, on this host (gates S1–S3)
cargo build --release -p pliwee-daemon --manifest-path desktop/Cargo.toml
packaging/tests/systemd-unit-gates.sh --binary desktop/target/release/pliweed

# 4. packages: bundle, RPM, install/remove/reinstall in a container
packaging/release/make-source-bundle.sh --output dist
packaging/fedora/build-rpm.sh dist --output out/fedora44
packaging/tests/packaging-checks.sh --bundle dist --rpm out/fedora44/pliwee-*.x86_64.rpm
packaging/tests/install-smoke.sh --image registry.fedoraproject.org/fedora:44 out/fedora44/*.rpm

# 5. lifecycle on a real installed desktop (libvirt guest), then with the phone
packaging/tests/lifecycle-gates.sh --domain <guest> --distro fedora44 --pkgdir out/fedora44 --evidence <dir>
packaging/tests/lifecycle-peer-gates.sh --domain <guest> --distro fedora44 --evidence <dir> \
    --phone-ip <android-ip> --adb-serial <serial>
```

The tray, clipboard, files, pairing and discovery gates are inside steps 1
and 5 (L1–L26; L12/L14–L16 need the physical phone). GNOME and KDE tray
behaviour on a real session is certified by the procedures in
`docs/certification/linux/gnome/` and `kde/`.

### 10.6 Bugs found in this round

None in the product. One operational side effect of this round's own
procedure, recorded because it affects the user's running setup:

* **The clean build replaced the binaries of the user's registered agent.**
  The prescribed `rm -rf macos/build` deleted the bundle the user's agent was
  registered from, and the rebuild put differently built binaries there:
  `codesign --verify` on the running agent and app now reports *"the code on
  disk does not match what is running"*, and rebuilding the same arm64
  release did not restore a match. The running processes are unaffected; at
  their next start launchd will refuse the new `pliweed` (`EX_CONFIG`, §5.5)
  and the keychain will ask about the new binary (§5.6). Recovery: Settings ›
  **Restart Service**, and *Always Allow* at the keychain prompt. A Developer
  ID signature would make rebuilds harmless; until then, a bundle that is
  registered as a service should not be rebuilt in place.

### 10.7 Architectural review of ADR-0021

Checked against the code on `2e41ea2`:

| Property | Evidence | Holds |
| --- | --- | --- |
| SwiftUI/AppKit only as front end | `macos/Sources` speaks `pliwee-control` JSON over the socket; no TLS, key, pairing or capability code in Swift | yes |
| Rust agent is the source of truth | every action is a `Request`; grants re-checked by the agent; the app renders reports | yes |
| Shared control protocol | `control-protocol.json` checked by both suites (mutation-tested in round 1) | yes |
| `platform-macos` isolated | every line and dependency `cfg(target_os = "macos")`; `unsafe` confined by `portable_boundary.rs` | yes |
| `platform-unix` shared | `pliwee-linux` re-exports it; the live-owner rule exists once | yes |
| No security duplicated in Swift | as above | yes |
| No Linux architectural regression | `platform/linux.rs` is the moved code; type-checks and lints for Linux | **not runtime-proved** (§10.5) |
| Coherent macOS lifecycle | register/unregister, restart policy, quit ≠ stop: measured in round 1; service-run discovery measured here | yes, except reboot (NOT EXECUTED) |
| Runtime paths | `~/Library/…`, 0600/0700 measured | yes |
| Security boundary preserved | keychain absence rule tested; socket owner-only measured; TLS/pinning exercised with `fake_phone` only | yes, Android not exercised |

**Recommendation: B — keep ADR-0021 *Proposed* until the Fedora regression
has run.** D2 is not a macOS-only decision: it moved the Linux control
transport into a new crate and recomposed the Linux daemon's `main`, and the
ADR's own consequences assert that Linux behaviour is unchanged. Accepting it
while that assertion has only been type-checked would accept a claim the
repository's evidence rule (AGENTS.md) says has not been measured. The
macOS-only decisions (D1, D3–D8) are supported by this round's evidence and
need no change; once the Fedora run is green, nothing else in the ADR blocks
acceptance. The status was not changed: that is the owner's decision.

### 10.8 Closure table

| Item | Result | Evidence |
| --- | --- | --- |
| Native app | EXECUTED/PASS | round 1 §4.2; the user's release app running (pid 30609) |
| Menu bar presence | EXECUTED/PASS (created) · NOT EXECUTED (visible) | §10.4 |
| Menu bar click/open | NOT EXECUTED | no Accessibility / Screen Recording |
| SwiftUI window | EXECUTED/PASS | round 1 snapshots of all pages |
| SMAppService | EXECUTED/PASS | round 1 register/unregister; this round: user-registered agent under `smd` |
| Daemon connectivity | EXECUTED/PASS | round 1; this round: launchd agent listening, resolvable |
| Control protocol | EXECUTED/PASS | fixture tests both sides (§10.1) |
| Keychain | EXECUTED/PASS | round 1 §4.1 real round trips; key item present for the running agent |
| IOKit | EXECUTED/PASS | round 1, real power-source read |
| NSPasteboard | EXECUTED/PASS | round 1 real round trip (plain + concealed) |
| TLS | EXECUTED/PASS with `fake_phone` · NOT EXECUTED with Android | round 1 §4.2 |
| Pinning | EXECUTED/PASS with `fake_phone` · NOT EXECUTED with Android | round 1 §4.2 |
| Pairing | EXECUTED/PASS with `fake_phone` · NOT EXECUTED with Android | round 1 §4.2 |
| File receive | EXECUTED/PASS with `fake_phone` · NOT EXECUTED with Android | round 1 §4.2 |
| File reject | NOT EXECUTED | not run in either round |
| File send (Mac → peer) | NOT EXECUTED | not run in either round |
| Clipboard Android → Mac | NOT EXECUTED | §10.3 |
| Clipboard Mac → Android | NOT EXECUTED | §10.3 |
| Bonjour via service | EXECUTED/PASS (same host) · NOT EXECUTED (from another host) | §10.2 |
| Reconnect | NOT EXECUTED | §10.3 |
| Daemon restart | NOT EXECUTED | §10.3 |
| App restart | NOT EXECUTED | §10.3 |
| Mac reboot | NOT EXECUTED | §10.3 |
| Rust tests | EXECUTED/PASS | §10.1 |
| Swift tests | EXECUTED/PASS | §10.1 |
| arm64 build | EXECUTED/PASS | §10.1 |
| universal build | EXECUTED/PASS (built, verified) · NOT EXECUTED (x86_64 run) | §10.1, round 1 |
| Fedora regression | NOT EXECUTED | §10.5 |
| ADR status | Proposed — recommendation B | §10.7 |

### 10.9 Verdict

**MACOS DESKTOP V1: CONDITIONALLY ACCEPTED.**

Every gate that could be executed here passed, nothing failed, and the
principal macOS gap of round 1 — discovery from the background service — is
closed with evidence. It is not CERTIFIED, because mandatory gates remain
NOT EXECUTED: the physical Android matrix (A–L), the menu-bar click, a Mac
reboot, and the Fedora regression. The conditions for certification are
exactly those runs; none of them is expected to need a design change, and
any that fails reopens this verdict.
