# 10 — macOS feasibility

| Field | Value |
| --- | --- |
| **Title** | OmniBridge on macOS |
| **Status** | Research / Draft |
| **Last reviewed** | 2026-08-31 |
| **Scope** | Rust on macOS, discovery, identity, clipboard, background agent, UI, packaging. |
| **Decision status** | PROPOSED. **PLAT-DEC-004** (Secure Enclave strategy) and **PLAT-DEC-009** (polling clipboard watch) are OPEN. |
| **Evidence** | OFFICIAL DOC VERIFIED from `developer.apple.com` where retrievable; several Apple pages could not be fetched in this session and are marked **EXTERNAL VERIFICATION REQUIRED**. REPO VERIFIED for OmniBridge's own behaviour. |
| **Related documents** | [11](11-IOS-IPADOS-FEASIBILITY.md), [12](12-APPLE-SECURITY-AND-INTEGRATION.md), [15](15-CROSS-PLATFORM-CLIPBOARD.md), [17](17-BACKGROUND-EXECUTION-MODEL.md), [19](19-PACKAGING-AND-DISTRIBUTION.md) |

---

## 1. Verdict

**macOS is feasible with one design compromise and one significant cost.**

- The compromise: **clipboard watching must poll**, which conflicts with a rule the
  `ClipboardBackend` trait currently states as an absolute. It is a contract amendment, not a
  technical obstacle.
- The cost: **Apple Developer Program membership, code signing and notarization are
  mandatory** for any distribution outside the App Store. That is a recurring fee and a
  release-pipeline requirement, not an engineering problem — but it is a real gate.

Everything else — Rust, TLS, discovery, identity, background execution, UI — has a clean
answer. macOS ranks **after Windows** in this research's priority order, mainly on
effort-per-user, not on difficulty.

---

## 2. Toolchain

| Item | Finding |
| --- | --- |
| Rust targets | `aarch64-apple-darwin` and `x86_64-apple-darwin` are **Tier 2** — "guaranteed to build", rustup-distributed (OFFICIAL DOC VERIFIED, *The rustc book*) |
| Apple Silicon | Primary target |
| Intel | Secondary. Whether to keep it is a product decision; see [19](19-PACKAGING-AND-DISTRIBUTION.md) |
| Universal binary | `lipo` two builds, or `cargo build --target` twice + `lipo` |
| Build host | **Requires a Mac** (Xcode, `codesign`, `notarytool`). This is a hardware/CI constraint, not a code one → [22](22-IMPLEMENTATION-ROADMAP.md) |
| C toolchain | Xcode CLT provides it; the `ring` question from [04 §4](04-LINUX-PORTABILITY.md) is moot here |
| `protoc` | Not needed (protox) |

Tier 2 rather than Tier 1 is worth a note: it means Rust builds the standard library and
checks that the target compiles, but does not run the full test suite there. In practice
`*-apple-darwin` is heavily used and stable; the practical risk is low. It does mean OmniBridge's
own test suite must actually run on macOS in CI rather than being assumed to pass.

---

## 3. TLS

Nothing to do, for the same reason as Windows: `rustls` + `ring` with custom pinning verifiers
consults no platform TLS stack (`core/src/tls.rs`, REPO VERIFIED). **Do not use
Network.framework's TLS** — see §4.2.

---

## 4. Discovery

### 4.1 The choice

| Option | Assessment |
| --- | --- |
| **`mdns-sd`** (reuse `daemon/src/mdns.rs`) | The crate README states macOS support and mentions verification against `dns-sd`. Reuses certified code. **Risk: port 5353.** macOS runs `mDNSResponder` as a system service, and it is far more territorial about 5353 than Avahi is. Whether a second in-process responder can bind and receive multicast alongside it is **the** open question. |
| **`NWBrowser` / `NWListener`** (Network.framework) | Apple's current Bonjour API. Cooperates with `mDNSResponder` by design — no port contention, because the system responder does the work. Requires Swift/Obj-C glue and an FFI hop, or the `dnssd` C API (`DNSServiceRegister`) which is callable from Rust. |
| `NSNetService` | Deprecated in favour of Network.framework. Do not use. |

**Recommendation: try `mdns-sd` (POC-MAC-02) but expect to fall back to the platform API,
and budget for the fallback.** macOS is the platform where "run your own responder" is most
likely to fail, and the platform API here is genuinely good — `mDNSResponder` is the reference
implementation of the protocol OmniBridge speaks.

If the fallback is needed, the cleanest route is the `dnssd` C API (`DNSServiceRegister`,
`DNSServiceBrowse`) rather than Network.framework, because it is C-callable from Rust with no
Swift bridge and no `NWListener` lifecycle to model. That keeps the adapter in Rust and out of
the Swift app.

Either way the **wire format is unchanged**: `_omnibridge._tcp.local.` with the `v`/`pv`/`id`/`dn`
TXT keys, which is what preserves interoperability with today's Android client.

### 4.2 Why not Network.framework for the connection itself

`NWConnection` with TLS would mean re-implementing SPKI pinning in
`sec_protocol_options_set_verify_block`, in Swift, alongside the Rust implementation — a
second copy of the single most security-critical function in the product
([02 §1](02-CROSS-PLATFORM-TARGET-ARCHITECTURE.md)). The whole point of the shared core is to
not do that. Use `mdns-sd`/`dnssd` for *discovery* and rustls over a plain
`TcpStream`/`TcpListener` for the *connection*.

---

## 5. Identity: Keychain and Secure Enclave

### 5.1 The good news

OmniBridge's identity key is **ECDSA P-256**, chosen back in
[ADR-0006](../../adr/ADR-0006-device-identity-and-pairing.md) for Android Keystore compatibility.

The Apple Secure Enclave supports **only** 256-bit elliptic-curve keys — P-256/secp256r1 — for
both ECDSA signing and ECDH (OFFICIAL DOC VERIFIED, `kSecAttrTokenIDSecureEnclave` and
`SecureEnclave.P256` in CryptoKit).

So OmniBridge's key algorithm is not merely *compatible* with the Secure Enclave; it is the only
algorithm the Secure Enclave would have accepted. A project that had picked Ed25519 — the
better primitive on paper — would be unable to use hardware-backed identity on **either**
mobile platform. This is the most consequential piece of good luck (or good judgement) in the
audit.

### 5.2 The hard part

The Secure Enclave key is non-exportable, which collides with `LocalIdentity`'s
`key_pkcs8_der: Vec<u8>` in exactly the way described in
[01 §3.1](01-CURRENT-ARCHITECTURE-AUDIT.md). The `IdentitySigner` seam solves it in principle,
and rustls's `sign::SigningKey`/`Signer` is the documented mechanism.

There is **no `rustls-secure-enclave` crate** equivalent to `rustls-cng`. This must be written:
a `SigningKey` whose `sign()` calls `SecKeyCreateSignature` with
`.ecdsaSignatureMessageX962SHA256` (or CryptoKit's `SecureEnclave.P256.Signing.PrivateKey`)
and returns the DER-encoded signature rustls expects for
`SignatureScheme::ECDSA_NISTP256_SHA256`.

Two constraints that must be designed for, not discovered:

1. **`Signer::sign` is synchronous and blocking.** Therefore the Secure Enclave key **must be
   created without a user-presence access-control flag** (`.userPresence`,
   `.biometryCurrentSet`). A Touch ID prompt inside a TLS handshake is not viable — handshakes
   happen on reconnect, on wake, on network change, unattended. Use
   `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` with `.privateKeyUsage` only.
   → [12](12-APPLE-SECURITY-AND-INTEGRATION.md).
2. **Signature encoding.** `SecKeyCreateSignature` returns X9.62 DER `(r, s)`, which is what
   TLS 1.3 wants for ECDSA — but this must be verified byte-for-byte against a real handshake,
   not assumed. Android hit an analogous trap: `DeviceIdentity.kt` records that v1 keys were
   generated without `KeyProperties.DIGEST_NONE` and were therefore *unusable for TLS client
   authentication*, and because keystore authorisations are immutable the only remedy was a new
   alias. **The same class of mistake is available on Apple platforms and would be equally
   unrecoverable for already-paired users.**

**POC-MAC-03 + POC-MAC-04 must be run together and must complete a real handshake against the
existing Linux `omnibridged`.** Proving "we can sign" and "rustls accepts a custom signer"
separately would repeat exactly the Android v1 mistake.

### 5.3 Fallback

If the Secure Enclave is unavailable (older Intel Macs without a T2, or a VM), fall back to a
Keychain-stored key with `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`.

The Keychain raises the same objection Linux already answered: `store.rs` explicitly avoids
the Secret Service because *"a `systemd --user` daemon can start before any keyring is
unlocked"*. On macOS the equivalent is the login keychain, which **is** unlocked at login — so
a login-item agent starting after login finds it available. That is a meaningful difference
and it means the Keychain is a reasonable macOS fallback where gnome-keyring was not a
reasonable Linux one.

---

## 6. Clipboard — the one compromise

| Operation | API | Assessment |
| --- | --- | --- |
| **read** | `NSPasteboard.general.string(forType: .string)` | Straightforward |
| **write** | `clearContents()` then `setString(_:forType: .string)` | Straightforward |
| **watch** | `NSPasteboard.general.changeCount`, compared against the last value | **Polling.** `changeCount` "increments every time the contents of the pasteboard changes"; comparing it is the documented way to detect change (OFFICIAL DOC VERIFIED). No public change *notification* exists in AppKit. |
| **sensitive hint** | `NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")` alongside the text | A **community convention** (nspasteboard.org), **not an Apple API**. V-08 is **STILL OPEN** and [26 §V-08](26-EXTERNAL-VERIFICATION-CLOSEOUT.md) recommends retiring the question: adoption breadth is not establishable from any primary source, the cost is one line, and it is a *hint* exactly like `wl-copy --sensitive`. Set it, document it as best-effort, gate nothing on it. |

### 6.1 The contract conflict

`capabilities/clipboard/src/backend/mod.rs` states, as a rule on implementors:

> **No implementation may satisfy this by polling.**

On macOS that rule cannot be satisfied. Three ways forward:

| Option | Consequence |
| --- | --- |
| **(a)** Amend the contract: polling allowed *only* where the platform offers nothing else, and the backend must declare it | Honest; costs nothing on Linux/Windows, where the rule still holds; `describe()` already exists to surface "polling every N ms" to `omnibridge clipboard status`. **Recommended.** |
| (b) No automatic clipboard send on macOS | Throws away the capability the product is most known for, on a platform where it is achievable |
| (c) Private/undocumented API | Never. Notarization risk and it would break. |

**Recommendation: (a).** The rule's *purpose* — "do not burn battery and wake the CPU on a
timer when an event source exists" — is preserved, because on macOS no event source exists.
State the interval explicitly and pick it deliberately: a `changeCount` read is cheap
(no pasteboard content is touched), and something in the 250–500 ms range is the usual
practice. Suspend polling when no peer has `auto_send` enabled — the manager already drops the
`ClipboardWatch` in that case, so this falls out of the existing design.

**PLAT-DEC-009.** Note this is a *documentation and contract* change in
`capabilities/clipboard/src/backend/mod.rs`, not a protocol change.


> **⚠ Updated by the verification sprint (2026-08-31).** **Three macOS conclusions changed.** (1) Pasteboard access is **user-gated from macOS 15.4** — `NSPasteboard.AccessBehavior` defaults to *ask* for programmatic access to the General pasteboard. (2) **Local network privacy applies to macOS from macOS 15**, and `launchd` **agents** do not get the daemon exemption. (3) **Developer ID signing is a runtime requirement**, not only a distribution one, because local network privacy tracks identity by code signature. See
> [26](26-EXTERNAL-VERIFICATION-CLOSEOUT.md).

### 6.2 macOS pasteboard privacy

**RESOLVED (V-06).** `NSPasteboard.AccessBehavior` was introduced in **macOS 15.4** with four
cases — `.default`, `.ask`, `.alwaysAllow`, `.alwaysDeny`. **There is no Info.plist key**:
`accessBehavior` is read-only, and the value is set by the user in System Settings, per app, only
after the app has triggered an alert.

Verbatim, on `.default`: *"The default behavior for the General pasteboard is to ask upon
programmatic access… Once programmatic pasteboard access triggers the first pasteboard access
alert, the state automatically changes to [ask]."* And on `.ask`: *"access that is both **user
originated and paste related** will always be allowed, and will not result in a notification."*

**So OmniBridge's automatic clipboard send — programmatic, not user-originated — is user-gated on
macOS 15.4+.** It does not make polling impossible, but it makes silent auto-send conditional on
the user choosing `.alwaysAllow`. That is a product statement and
[03](03-PLATFORM-CAPABILITY-MATRIX.md) is updated accordingly.

What remains unmeasured is narrower and sharper: reading `changeCount` is *metadata*, not content.
Whether polling it alone trips the alert, or only the subsequent content read, is now the precise
subject of **POC-MAC-05**.

**This should be the first thing POC-MAC-05 measures**, before any implementation effort:
*does an unsandboxed, notarized, Developer-ID-signed background agent reading
`NSPasteboard.general` on the current macOS trigger a user prompt, once, repeatedly, or never?*

---

## 7. Background execution

macOS is the closest of the three desktops to Linux's model.

| Mechanism | Assessment |
| --- | --- |
| **`SMAppService`** (macOS 13+) | The current API. Registers a login item, a `LaunchAgent` or a `LaunchDaemon` bundled inside the app, and registered items appear in **System Settings → General → Login Items** where the user can disable them. Replaces `SMLoginItemSetEnabled`. **Recommended.** |
| `LaunchAgent` plist in `~/Library/LaunchAgents` | The classic route; still works; less discoverable for the user and messier to uninstall |
| `LaunchDaemon` | Runs as root outside a user session — **wrong**, for the same reason a Windows Service is wrong ([08 §5](08-WINDOWS-FEASIBILITY.md)): no user pasteboard, no user Downloads, no human |

**Architecture: an `OmniBridge.app` whose bundled helper is registered as a login-item agent via
`SMAppService`.** The agent hosts the portable Rust runtime, holds the TLS listener, the
discovery registration and the pasteboard poller, and exposes a control channel to the UI.
The main app is a menu-bar item plus windows — it can be quit without stopping the agent, and
this mirrors the Linux `omnibridged` + `omnibridge-gui` split exactly.

Two macOS-specific behaviours to design for:

- **App Nap / timer coalescing.** A background agent's timers can be throttled. The pasteboard
  poller must tolerate irregular intervals, and if precision matters, an activity assertion
  (`NSProcessInfo.beginActivity`) is the supported way to ask for less throttling — used
  sparingly, because it costs battery.
- **Sleep/wake.** Network interfaces change; `mdns-sd`'s `enable_addr_auto()` handles this on
  Linux and must be verified on macOS, or the platform API's own re-registration used instead.

**SMAppService reliability is worth a note:** Apple's developer forums carry a number of reports
of `registerAndReturnError` succeeding without actually registering, and of code-signing-related
registration failures. This is normal for a relatively new API but it means **POC-MAC-06 should
test registration on a clean system, on an upgraded system, and after an app update** — not
just once on the developer's machine.

---

## 8. Filesystem

| Concern | macOS |
| --- | --- |
| Downloads | `FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)`. Replaces the XDG chain. |
| Data directory | `~/Library/Application Support/OmniBridge` |
| File modes | POSIX modes work. `store.rs`'s `require_private_mode`/`harden_dir` and `destination.rs`'s `O_EXCL` + 0600 **compile and behave correctly as written** — macOS is the one non-Linux platform where the existing unix code is right rather than merely portable |
| Sandbox | An unsandboxed Developer-ID app has full access with user consent (TCC prompts for Desktop/Documents/Downloads). A sandboxed (App Store) build would need `com.apple.security.files.downloads.read-write` and a security-scoped bookmark → §10 |
| Filename rules | macOS is POSIX-ish, but HFS+/APFS normalise Unicode (NFD) and `:` is historically a separator in the Finder. The Windows rule set from [09 §6](09-WINDOWS-SECURITY-AND-INTEGRATION.md) applied everywhere covers the `:` case. |
| Quarantine | Files written by an app are not quarantined unless we set `com.apple.quarantine`. A received executable therefore runs without a Gatekeeper prompt. **Consider setting the quarantine attribute on received files** — this is a genuine security improvement over the Linux behaviour and is cheap. **SEC-006.** |

---

## 9. UI

**Recommendation: SwiftUI, with AppKit where SwiftUI is thin.**

| Element | Framework |
| --- | --- |
| Menu-bar item | `MenuBarExtra` (SwiftUI, macOS 13+) or `NSStatusItem` |
| Main window (devices, transfers, settings) | SwiftUI |
| Pairing QR display / scan | SwiftUI |
| File picker | `NSOpenPanel` / SwiftUI `.fileImporter` |
| Notifications | `UNUserNotificationCenter` |

The UI talks to the agent over the control channel, not through FFI — same reasoning as
[08 §9](08-WINDOWS-FEASIBILITY.md). On macOS the channel can simply be a **Unix domain socket**,
which means `daemon/src/server.rs`'s existing implementation works with only a path change
(`~/Library/Application Support/OmniBridge/control.sock` or a sandbox-appropriate location).
macOS is the platform where the existing IPC transfers wholesale.

If the app is ever sandboxed, XPC becomes necessary instead; that is one more reason to prefer
Developer ID distribution over the App Store (§10).

---

## 10. Packaging and distribution

| Option | Assessment |
| --- | --- |
| **`.app` in a signed, notarized DMG** | The standard for a utility like this. Drag to Applications. **Recommended primary.** |
| **Homebrew Cask** | A `brew install --cask omnibridge` pointing at the DMG. High-value for the target audience at near-zero cost. **Recommended secondary.** |
| `.pkg` installer | Needed only if something must be installed outside the app bundle. `SMAppService` removes that need. **Not recommended.** |
| Mac App Store | Requires the App Sandbox. Local networking, a background agent, pasteboard polling and arbitrary Downloads writes are all sandbox-hostile, and `SMAppService` login items in a sandboxed app are constrained. **Deferred**, possibly permanently. |

### 10.1 Signing and notarization are mandatory

OFFICIAL DOC VERIFIED: since macOS 10.14.5 software signed with a new Developer ID certificate
must be notarized to run, and since macOS 10.15 all Developer-ID software built after
2019-06-01 must be notarized. Gatekeeper checks for a Developer ID certificate on anything
distributed outside the App Store.

Practical consequences for the roadmap:

- **Apple Developer Program membership is required** (annual fee).
- The release pipeline needs `codesign` (hardened runtime, timestamp), `notarytool submit`,
  and `stapler staple` — all of which require **a Mac in CI**, or a self-hosted runner.
- Notarization is per-build and takes minutes; it must be in the release job, not a manual step.
- The Rust binary inside the bundle must itself be signed, and the hardened runtime interacts
  with anything that allocates executable memory (nothing in OmniBridge does).

This is the single largest *non-engineering* cost in the whole expansion and it should be
surfaced in planning rather than discovered at release time.

---

## 11. PoCs

> **Results — 2026-10-03, branch `feature/macos-desktop-v1`.** The first macOS
> integration measured several of these on an Apple Silicon Mac; the evidence
> is [`reports/macos/MACOS-DESKTOP-V1.md`](../../reports/macos/MACOS-DESKTOP-V1.md)
> and the decisions [ADR-0021](../../adr/ADR-0021-macos-desktop-integration.md).
> POC-MAC-01: the workspace builds and its portable and agent tests pass on
> `aarch64-apple-darwin`. POC-MAC-02: `mdns-sd` interoperates with
> `mDNSResponder` from a terminal; from the launchd agent the advertisement was
> not observed, pending local-network consent. POC-MAC-06: `SMAppService`
> agent registration and removal work, including ad hoc signed, with the
> caveat that a rebuilt ad-hoc agent must be registered again. POC-MAC-07: the
> SwiftUI app drives the agent over the Unix socket end to end. POC-MAC-03/04
> (Secure Enclave) and POC-MAC-05's prompting question were not run.

| ID | Question |
| --- | --- |
| **POC-MAC-01** | Does the workspace compile and pass its portable tests on `aarch64-apple-darwin`? |
| **POC-MAC-02** | Can `mdns-sd` advertise `_omnibridge._tcp.local.` alongside `mDNSResponder`, such that the Android app finds it? If not, does `DNSServiceRegister` work from Rust? |
| **POC-MAC-03** | Secure Enclave P-256 key + certificate built around its public key + SPKI fingerprint. |
| **POC-MAC-04** | A `rustls::sign::SigningKey` backed by `SecKeyCreateSignature`, completing a mutual TLS 1.3 handshake with SPKI pinning **against the existing Linux daemon**. |
| **POC-MAC-05** | `NSPasteboard` read/write/`changeCount` polling. **First measure whether reading prompts the user** on current macOS for a signed, notarized, unsandboxed background agent. |
| **POC-MAC-06** | `SMAppService` login-item agent: registration on clean install, on upgrade, after an app update; survival across logout/login, sleep/wake and network change. |
| **POC-MAC-07** | SwiftUI app ↔ agent over a Unix domain socket, reusing `server.rs`. |

---

## 12. Recommended direction

1. **Login-item agent via `SMAppService`** + a separate SwiftUI app. Mirrors the Linux split.
2. **Portable Rust core**; no Swift reimplementation of TLS, pinning or pairing.
3. **`mdns-sd` first, `dnssd` C API as the budgeted fallback.** macOS is where the fallback is
   most likely to be needed.
4. **Secure Enclave P-256 via a custom `SigningKey`**, created **without** user-presence.
   Keychain fallback. Prove it end-to-end in one POC, not in parts.
5. **Amend the `ClipboardBackend` contract to permit declared polling**; implement
   `changeCount` polling, suspended when no peer wants auto-send.
6. **Measure pasteboard prompting before committing to auto-send on macOS.**
7. **DMG + Homebrew Cask, Developer ID, notarized.** Defer the App Store.
8. **Budget for a Mac in CI** — it is required for signing and notarization, not optional.
