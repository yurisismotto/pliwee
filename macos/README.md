# Pliwee for macOS

A native macOS application — SwiftUI, a menu-bar item and one window — for
the same Pliwee agent (`pliweed`) that runs on Linux. **Preview**: it builds
and runs from source on Apple Silicon; there is no signed, notarized download
yet (see [Signing and distribution](#signing-and-distribution)).

How it fits together, and why it was built this way:
[`docs/architecture/MACOS.md`](../docs/architecture/MACOS.md) and
[ADR-0021](../docs/adr/ADR-0021-macos-desktop-integration.md).

```text
Pliwee.app
├── Contents/MacOS/Pliwee      SwiftUI app: menu bar + window. A client, nothing more.
├── Contents/MacOS/pliweed     the Rust agent, run by launchd as a per-user agent
├── Contents/Helpers/pliwee    the command-line tool
└── Contents/Library/LaunchAgents/io.github.yurisismotto.pliwee.daemon.plist
```

The app talks to the agent over the agent's Unix control socket, with the
same newline-delimited JSON the `pliwee` CLI and the GTK app use. Protocol,
TLS, pairing, pinning, trust and every capability stay in Rust.

## Requirements

| To | You need |
| --- | --- |
| **run** a built `Pliwee.app` | macOS 14 or later. Nothing else: no Rust, no Homebrew. |
| **build** it | macOS, the Xcode Command Line Tools (`xcode-select --install`), and Rust via [rustup](https://rustup.rs). Xcode itself is optional. |

`desktop/rust-toolchain.toml` selects the toolchain; the workspace floor is
Rust 1.88.

## Build

```bash
macos/scripts/build-app.sh                 # release, this Mac's architecture
macos/scripts/build-app.sh --debug         # debug build, with validation hooks
macos/scripts/build-app.sh --arch universal  # arm64 + x86_64 (needs: rustup target add x86_64-apple-darwin)
```

The result is `macos/build/Pliwee.app`, signed ad hoc. The script checks every
tool it needs before starting, refuses a bundle whose launchd label differs
between the plist, the Rust adapter and the Swift app, and verifies that the
bundle holds exactly its three executables with every requested architecture.

To work on the Swift code alone: `cd macos && swift build`, or open the
`macos` directory in Xcode (File › Open…): it is a Swift package, so there is
no `.xcodeproj` to keep in sync.

## Run

```bash
open macos/build/Pliwee.app
```

The window opens; the menu-bar item stays when you close it. The background
service is **off** until you turn it on — in Settings, *Run Pliwee in the
background* — exactly as on Linux nothing listens on the network until
`systemctl --user enable --now pliweed.service`. Turning it on registers
`pliweed` with launchd through `SMAppService`: it starts now, starts at every
login, restarts if it fails, and appears in System Settings › General › Login
Items. Turning it off stops it and removes the registration.

Quit Pliwee (menu bar › Quit) ends the app. It does **not** stop the service,
which goes on moving files and answering your phone, as `pliweed` outlives the
GTK app on Linux.

The command-line tool works against the same agent:

```bash
macos/build/Pliwee.app/Contents/Helpers/pliwee status
```

To run the agent by hand instead of as a service (development):

```bash
cd desktop && cargo run -p pliwee-daemon        # the app finds it on the same socket
```

### Where things are

| What | Where |
| --- | --- |
| Identity state and pairings | `~/Library/Application Support/Pliwee/state.json` (0600, directory 0700) |
| The identity's private key | login keychain, item `io.github.yurisismotto.pliwee` |
| Control socket | `~/Library/Application Support/Pliwee/run/control.sock` (0600, directory 0700) |
| Agent log (when run by launchd) | `~/Library/Logs/Pliwee/pliweed.log` — also in Console.app |
| Received files | `~/Downloads/Pliwee/` |

## Test

```bash
macos/scripts/test.sh            # the Swift tests (PliweeKitTests)
```

With only the Command Line Tools, SwiftPM's default build system does not load
the Swift Testing macro plugin those tools ship, and every `@Test` fails to
expand; `test.sh` loads it explicitly. With Xcode, `swift test` works as is.

The Rust side on a Mac:

```bash
cd desktop
cargo fmt --all --check
cargo clippy --workspace --exclude pliwee-gui --all-targets --all-features -- -D warnings
cargo test --workspace --exclude pliwee-gui --exclude pliwee-linux
cargo test -p pliwee-linux --lib
cargo test -p pliwee-macos -- --ignored     # touches your real keychain and clipboard; see below
```

What is excluded, and why — none of it is skipped silently:

* `pliwee-gui` is GTK 4 and libadwaita; it needs their Linux development
  packages through `pkg-config`.
* `pliwee-linux`'s integration tests start a private `dbus-daemon` and fail,
  by design, when there is none. Its unit tests (`--lib`) run.
* `daemon/tests/legacy_state_migration.rs` is compiled out on macOS: it
  measures the Linux OmniBridge-to-Pliwee migration, which macOS never needed.
* The `--ignored` tests in `pliwee-macos` write to the login keychain (under a
  throwaway service name, deleted afterwards) and replace the clipboard
  contents. They are real round trips and run on purpose, not by default.

## Signing and distribution

A local build is signed **ad hoc**. That is enough to run on the Mac that built
it, and it is not a distribution signature: Gatekeeper rejects an ad-hoc app
copied to another Mac. Nothing here notarizes, and nothing pretends to.

To distribute, the project needs an Apple Developer ID:

```bash
macos/scripts/build-app.sh --sign "Developer ID Application: <name> (<TEAMID>)"
ditto -c -k --keepParent macos/build/Pliwee.app Pliwee.zip
xcrun notarytool submit Pliwee.zip --keychain-profile <profile> --wait
xcrun stapler staple macos/build/Pliwee.app
```

`--sign` signs every executable with the hardened runtime and a secure
timestamp. The notarization commands are listed for completeness and have
**not** been run: there is no Developer ID in this project yet.

A Developer ID is also what makes three development-only behaviours below
disappear, because they all come from an ad-hoc signature changing with every
build.

## Troubleshooting

**The menu says "Not responding" right after turning the service on, or after
rebuilding.** Look at `~/Library/Logs/Pliwee/pliweed.log`.

* If it stops at *"reading this Mac's identity key from the login keychain"*,
  macOS is asking whether this `pliweed` may use the key. A rebuilt, ad-hoc
  signed `pliweed` is a different program as far as the keychain is concerned.
  Answer the prompt (*Always Allow*).
* If the log has nothing new at all, launchd may be refusing to start a
  rebuilt `pliweed`: it recorded the code signature of the binary that was
  registered, and an ad-hoc rebuild no longer matches (`launchctl print
  gui/$(id -u)/io.github.yurisismotto.pliwee.daemon` shows `last exit code =
  78: EX_CONFIG`). Use **Restart Service** in Settings, which registers it
  again.

**"Turn On" does nothing / the switch is disabled.** The app is not running
from a bundle that carries the agent (for example `swift run`). Build the app
with `build-app.sh`, or run `pliweed` yourself.

**Your phone does not find the Mac.** macOS asks once whether Pliwee may find
and connect to devices on your local network; if that was declined, allow it in
System Settings › Privacy & Security › Local Network. Pairing by QR code works
regardless, because the code carries the Mac's addresses. If the application
firewall is on, allow incoming connections for Pliwee (TCP port 55432).

**Received files do not appear.** `~/Downloads` is privacy-protected; if macOS
asked whether Pliwee may access it and the answer was no, allow it in System
Settings › Privacy & Security › Files and Folders.

**Sending the clipboard asks for permission.** From macOS 15.4, reading the
clipboard from a program can raise a system prompt, per application. That is
the system's decision; Pliwee reads the clipboard only when you send it.

**"Automatic sending" is off and cannot be turned on.** macOS has no
clipboard-change notification and Pliwee does not poll the clipboard; see
[ADR-0021](../docs/adr/ADR-0021-macos-desktop-integration.md) D7.

## Uninstall

1. Settings › turn off *Run Pliwee in the background* (this stops the agent and
   removes it from Login Items).
2. Quit Pliwee and delete `Pliwee.app`.
3. To also forget this Mac's identity and pairings: delete
   `~/Library/Application Support/Pliwee`, `~/Library/Logs/Pliwee`, and the
   `io.github.yurisismotto.pliwee` item in Keychain Access. Received files in
   `~/Downloads/Pliwee` are yours and are left alone.
