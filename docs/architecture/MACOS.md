# macOS

How Pliwee runs on a Mac: what each piece is, where it lives, and how the
pieces talk. The decisions behind it, and the alternatives rejected, are
[ADR-0021](../adr/ADR-0021-macos-desktop-integration.md); building and running
it is [`macos/README.md`](../../macos/README.md).

Status: **preview**, built from source, Apple Silicon first. Not yet signed
with a Developer ID or notarized.

## The shape

```text
 Pliwee.app  (SwiftUI + AppKit, macos/)                       the user's session
 ┌────────────────────────────────────────────┐
 │ MenuBarExtra ─┐                             │
 │ main window  ─┼─ AppModel ── PliweeKit ─────┼──┐  newline-delimited JSON
 │ offer prompt ─┘   (polls,     ControlClient │  │  (pliwee-control's contract)
 │                    streams)                 │  │
 └────────────────────────────────────────────┘  │
        │ SMAppService.agent(…)                    ▼
        ▼                              ~/Library/Application Support/Pliwee/run/control.sock
 launchd  (per-user agent, Aqua session)          ▲
        │ starts, restarts on failure              │ pliwee-unix (bind, live-owner rule, 0600/0700)
        ▼                                          │
 pliweed  (Rust, desktop/daemon) ─────────────────┘
   ├── pliwee-runtime   listener (TLS 1.3, SPKI pinning), mDNS, state, control server
   ├── pliwee-core      identity, pairing, session, trust store       ── unchanged, shared
   ├── capabilities     battery · clipboard · files · notifications   ── unchanged, shared
   └── platform::macos ── pliwee-macos
          paths        ~/Library/Application Support/Pliwee, ~/Library/Logs/Pliwee
          keychain     SecretStore: private key in the login keychain
          battery      LocalBatterySource: IOKit power sources
          clipboard    ClipboardBackend: NSPasteboard
          host         computer name (scutil)

 pliwee  (CLI, Contents/Helpers/pliwee) ── same socket, same contract
```

The Linux picture is the same with `pliwee-linux` in place of `pliwee-macos`,
`systemd --user` in place of launchd, and the GTK application in place of
`Pliwee.app`. Nothing above the `platform` line differs.

## Components

| Piece | Where | What it is |
| --- | --- | --- |
| `Pliwee.app` | `macos/Sources/Pliwee` | The menu-bar item and the window. A client of the agent; holds no keys and makes no network connection. |
| `PliweeKit` | `macos/Sources/PliweeKit` | The app's testable half: the control-protocol models, the Unix-socket client, runtime paths, and the device/action/service-health rules. No UI framework. |
| `pliweed` | `desktop/daemon` | The agent. On macOS composed by `src/platform/macos.rs`. |
| `pliwee-macos` | `desktop/platform-macos` | The macOS adapter. Empty on every other OS. |
| `pliwee-unix` | `desktop/platform-unix` | The Unix-socket control transport, shared with Linux. |
| `pliwee` | `desktop/cli` | The command-line tool, bundled at `Contents/Helpers/pliwee`. |

## Inter-process communication

The app and the CLI reach the agent the same way the GTK app and the CLI do
on Linux: connect to the control socket, write one JSON request and a newline,
read newline-delimited JSON back. One request per connection, except the three
streams — `pair`, `send` and `watch_file_offers` — which keep the connection
open for the events and for the one answer each asks for.

The socket is `~/Library/Application Support/Pliwee/run/control.sock`:

* the directory is made 0700 and the socket 0600 by `pliwee-unix`'s `bind`,
  which also refuses to take over a socket a live agent is serving and
  replaces a stale one left by a crash;
* the path is derived from `$HOME` alone, identically in Rust
  (`platform-macos/src/paths.rs`) and Swift (`PliweeKit/RuntimePaths.swift`),
  and both test suites assert the same literals;
* `sun_path` holds 103 bytes on macOS, which leaves room for a 51-byte home
  directory. A longer one is refused by name.

The contract is `desktop/control/src/lib.rs`. Swift cannot share its types, so
`desktop/control/tests/fixtures/control-protocol.json` — one example of every
request, response and event, generated from the Rust types — is checked by
the Rust suite in both directions and decoded and encoded by the Swift suite.

The socket never carries clipboard content, notification text, file bytes or
key material, on macOS as on Linux: the Rust types have no field that could.

## Lifecycle

| Event | What happens |
| --- | --- |
| First launch of `Pliwee.app` | The window opens. The service is **off**: nothing listens. |
| *Run Pliwee in the background* on | `SMAppService.agent(…).register()`. launchd starts `pliweed` now and at every login (`RunAtLoad`), restarts it after a failure (`KeepAlive {SuccessfulExit: false}`, 5 s throttle). It appears in System Settings › General › Login Items. |
| Window closed | The app stays, as a menu-bar item with no Dock icon. The agent is unaffected. |
| Quit Pliwee | The app ends, closing its polling and its two streams. The agent keeps running. |
| *Run Pliwee in the background* off | `unregister()`: launchd stops `pliweed` and forgets the job. |
| *Open Pliwee at login* | `SMAppService.mainApp`: the menu-bar app at login. Independent of the service. |
| Restart Service | `unregister()` then `register()`; for an agent that is enabled and not answering. |

While the agent runs, the app attaches as its incoming-file **approval
provider** (`watch_file_offers`). With nobody attached the agent declines every
offer, which is the headless default. The prompt is a floating panel; Decline
is its default and cancel action; closing it declines; losing the agent
withdraws every open prompt.

The app polls `status` (and, when the agent answers, `transfers`,
`clipboard_status` and `notifications_status`) every 2 seconds while its
window is open and every 5 seconds otherwise — metadata the agent already
holds in memory, as on Linux.

### What the service status means

The menu bar's one status comes from two observations — did the socket answer,
and what does `SMAppService` say — resolved by `PliweeKit/ServiceHealth.swift`:

| Status | When |
| --- | --- |
| Connected | the agent answered and at least one paired device has a live session |
| Disconnected | the agent answered and no device is connected |
| Starting… | registered, not answering yet, within 20 s of being turned on |
| Service off | not registered and not answering |
| Needs approval | registered, waiting for the user in Login Items |
| Not responding | registered and still silent after the grace |
| Not running | not answering, and not run from a bundle that can register it |

## Where things are

| What | Where | Protection |
| --- | --- | --- |
| Identity state, pairings, grants | `~/Library/Application Support/Pliwee/state.json` | 0600 in a 0700 directory |
| The identity's private key | login keychain, generic password `io.github.yurisismotto.pliwee` / `identity@<data dir>` | the keychain's encryption and per-application access control |
| Control socket | `~/Library/Application Support/Pliwee/run/control.sock` | 0600 in a 0700 directory |
| Agent log | `~/Library/Logs/Pliwee/pliweed.log`, rotated past 8 MiB | 0600 |
| Received files | `~/Downloads/Pliwee/` | written by the shared `files.v1` sink: never overwrites, 0600 |

The agent logs to that file only when launchd started it (it recognises its
own job label in `XPC_SERVICE_NAME`); run from a terminal it logs to the
terminal.

## The adapters

| Seam (Wave 0) | macOS implementation | Notes |
| --- | --- | --- |
| `SecretStore` | `KeychainSecretStore` | Only `errSecItemNotFound` is absence. A `HOME` with no keychain is refused by name. |
| `IdentityBackend` | `SoftwareBacking` (shared) | A Secure Enclave backing is PLAT-DEC-004, open. |
| `ControlTransport` | `pliwee-unix` | Shared with Linux. |
| `LocalBatterySource` | `IopsBattery` | IOKit `IOPS*`. Nothing, never 0%, when there is nothing to say. |
| `ClipboardBackend` | `PasteboardBackend` | `NSPasteboard`. Sensitive clips carry `org.nspasteboard.ConcealedType`. No watch: see below. |
| `NotificationSink` / `LockSource` | `NoSink` / `UnknownLock` (shared) | No `SINK` role is announced. |
| `FileSink` | `UnixDownloadSink` (shared) | `~/Downloads/Pliwee`; POSIX modes behave as on Linux. |
| discovery | `mdns-sd` (shared) | Advertises `_pliwee._tcp` and `_omnibridge._tcp`, as on Linux. |
| platform reported to peers | `PLATFORM_UNSPECIFIED` | `core.proto` has no macOS value; ADR-0021 D6. |

`unsafe` exists in two files of `pliwee-macos` — the IOKit calls and one
AppKit constant — each block justified; `core/tests/portable_boundary.rs`
holds that list and fails on `unsafe` anywhere else.

## Security model

Unchanged from Linux, because it is the same code: TLS 1.3 with mutual
authentication and SPKI pinning, pairing by one-time token with proof of
possession and a human fingerprint check, grants per device and per
capability, nothing granted by pairing alone, every grant re-read from the
trust store on every request. What macOS adds:

* the private key leaves the filesystem for the login keychain;
* the control socket is in the user's home, so no other user can create it
  first, and it is 0600 in a 0700 directory;
* the agent runs as the user, never as root;
* the app is a client and holds nothing the CLI does not.

## Permissions and entitlements

The app is not sandboxed and declares **no entitlements**. It asks for no
Accessibility, Full Disk Access, Screen Recording or Automation permission —
none is needed. What macOS may ask the user, each once, attributed to Pliwee:

| Prompt | Why | Without it |
| --- | --- | --- |
| Local network | the agent advertises over Bonjour and talks to phones on the LAN (`NSLocalNetworkUsageDescription`, `NSBonjourServices` for `_pliwee._tcp`, `_omnibridge._tcp`) | discovery does not reach the network; pairing by QR code still carries addresses |
| Downloads folder | received files are written to `~/Downloads/Pliwee` | received files cannot be stored |
| Paste from other apps (macOS 15.4+) | sending the clipboard reads the general pasteboard | sending the clipboard fails; receiving is unaffected |
| Keychain access | a differently signed `pliweed` (an ad-hoc rebuild) reading the key | the agent waits; see `macos/README.md` |
| Background item added | registering the agent | — (a notification, not a question) |

## Limitations

* **No automatic clipboard sending.** `NSPasteboard` has no change
  notification and the clipboard contract forbids polling; PLAT-DEC-009.
* **No notification mirroring.** No sink on macOS yet.
* **Software key.** No Secure Enclave yet; PLAT-DEC-004.
* **Discovery from the agent unconfirmed.** Measured to work beside
  `mDNSResponder` from a terminal; not observed from the launchd agent pending
  the user's local-network consent (ADR-0021, consequences).
* **Ad-hoc signing in development:** keychain prompts after a rebuild, launchd
  refusing a rebuilt agent until it is registered again, and the agent listed
  as `pliweed` in Login Items. A Developer ID signature removes all three.
* **Not distributable yet:** no Developer ID, no notarization.
* **Socket path length:** home directories longer than 51 bytes are refused.
* **x86_64:** the universal build is produced and verified; running its Intel
  slice has not been tested.
