# Sprint report — Wave 0, core platform abstraction

**Branch:** `feature/core-platform-abstraction-v1` · **Certification commit:** `cfd33f6`

> ## Certification: **WAVE 0 CERTIFIED** — 2026-09-01
>
> All twelve official gates ([28 §12](../../sprints/../research/platform-expansion/28-WAVE-0-IMPLEMENTATION-SPEC.md))
> pass, all four P0 PoCs pass, and every code deliverable is complete.
>
> The two items that were unexecuted when this report was first written have
> since been **executed, not inferred**:
>
> * **G6 / W0-ANDROID-REGRESSION** — the unmodified Android app pairs,
>   connects and transfers with the post-Wave-0 daemon on the SM-X620.
>   `clipboard.v1` and `files.v1` both directions, `battery.v1`, 21
>   instrumented tests. Closed 2026-08-31/09-01 (§18).
> * **POC-CORE-04 / G3** — `cargo check --no-default-features --target
>   x86_64-pc-windows-msvc` for the six portable crates, green on a
>   GitHub-hosted `windows-2025-vs2026` runner with `host:
>   x86_64-pc-windows-msvc`. [Run 33465365649](https://github.com/yurisismotto/anyflow/actions/runs/33465365649),
>   2026-09-01 (§17).
>
> **Certified means the Wave 0 refactor is behaviour-preserving, interoperable
> and protocol-identical, and that the portable boundary holds against an MSVC
> compiler.** It does **not** mean AnyFlow runs on Windows — that is runtime
> certification, and it belongs to Wave 5+.

---

## 1. Executive summary

| | |
| --- | --- |
| Rust tests | **375** passed, 0 failed (366 + the 9 `real_backend` tests, since run on an unlocked seat; baseline: 303 + 9) |
| New Rust tests | **+63** |
| Existing tests modified | **2**, both schema fixtures — see §8 |
| Android tests | **232** JVM + **21** instrumented on the SM-X620, 0 failed |
| Official gates ([28 §12](../../sprints/../research/platform-expansion/28-WAVE-0-IMPLEMENTATION-SPEC.md)) | **12 / 12 PASS** |
| Gates unexecuted | **0** |
| P0 PoCs | **4 PASS** (POC-CORE-01/02/03/04) |
| `.proto` files changed | **0** |
| `android/**` files changed | **0** |
| Clippy warnings | **0** |

Wave 0 turned an implicitly-Linux architecture into one with an explicit
platform boundary, and fixed the three defects that a refactor would otherwise
have carried into every platform inheriting the code.

The measure is a negative one, and it holds: **AnyFlow on Fedora behaves
identically to before.** A genuine pre-Wave-0 `state.json` — schema 1, written
by the old binary, four paired peers — was opened by the new code and upgraded
in place with the same device id, the same fingerprint, the same peers, the
same revocations and the same grants. Pairing, `battery.v1` and `files.v1` in
both directions were driven end to end over real TLS 1.3 with real pinning.

**The architectural claim is proved rather than asserted.** A complete,
mutually-authenticated, SPKI-pinned TLS 1.3 handshake now runs from an
`IdentityProvider` that holds no private-key bytes and exposes no method
returning any — in both roles, and with both ends non-exportable. That is the
question that decided whether AnyFlow can ever reach a TPM, a Secure Enclave or
an Android Keystore, and the answer is yes.

**All six portable crates build for `x86_64-pc-windows-gnu` from this Linux
host**, `ring` included — and, since 2026-09-01, they check, build and link
their test targets for **`x86_64-pc-windows-msvc` on a real Windows runner**
under CI-001 (§17). That is a boundary result and nothing more: it says
`std::os::unix` did not leak into a portable crate, and that the boundary
survives a different C toolchain and CRT. It does **not** say AnyFlow runs on
Windows, and this document does not claim it.

---

## 2. Wave 0 architecture as implemented

```
                      protocol/proto/**                    (untouched)
                             │
                      anyflow-proto                        (unchanged)
                             │
   ┌─────────────────────────┴────────────────────────────────────────┐
   │  anyflow-core                          PORTABLE (no default feat) │
   │    framing session tls pairing qr fingerprint capability          │
   │    discovery clipboard_policy                                     │
   │                                                                   │
   │    identity.rs   ──► trait IdentityProvider        (NEW)          │
   │                      trait IdentityBackend         (NEW)          │
   │                      KeyBacking, IdentityState     (NEW)          │
   │    secret_store.rs ► trait SecretStore             (NEW)          │
   │    store.rs      ──► portable state/trust policy + classification │
   │    platform/unix_fs.rs   ── feature "unix-fs", the ONLY exception │
   │    unsafe_code = "forbid"                                (KEPT)   │
   └────┬──────────────────────────────────────────────────────────────┘
        │
   ┌────┴─────────────────────────────────────────────┐
   │  capability crates — protocol halves              │
   │    battery   ── BatterySource      already existed│
   │    clipboard ── ClipboardBackend   already existed│
   │                 + sensitive_support()      (NEW)  │
   │                 backends behind "linux-backends"  │
   │    files     ── FileSink                   (NEW)  │
   │                 destination behind "unix-fs"      │
   │    unsafe_code = "forbid"                  (KEPT) │
   └────┬─────────────────────────────────────────────┘
        │
   ┌────┴───────────────────────────┐   ┌──────────────────────────────┐
   │  anyflow-control        (NEW)  │   │  anyflow-runtime      (NEW)  │
   │  request/response types        │◄──┤  listener, mdns, state,      │
   │  + trait ControlTransport      │   │  control server (generic)    │
   │  + trait ControlListener       │   │  unsafe_code = "forbid"      │
   │  + BindError::AlreadyOwned     │   └────────┬─────────────────────┘
   │  unsafe_code = "forbid"        │            │
   └────┬───────────────────────────┘   ┌────────┴─────────────────────┐
        │                               │  anyflow-linux         (NEW) │
        │                               │  UDS transport, XDG paths,   │
        │                               │  0600/0700, /etc/hostname,   │
        │                               │  store composition           │
        │                               │  unsafe_code = "deny"        │
        │                               └────────┬─────────────────────┘
        │                                        │
   ┌────┴──────┬──────────────────┐     ┌────────┴────┐
   │anyflow-cli│  anyflow-gui     │     │  anyflowd   │
   │  deny     │  deny            │     │  deny       │
   └───────────┴──────────────────┘     └─────────────┘
```

The completion test from the specification — *adding a platform means writing
an adapter crate; it never means editing `anyflow-core`, `tls.rs`,
`session.rs` or a capability crate's protocol half* — is now enforced by a
test rather than by intention (`core/tests/portable_boundary.rs`).

---

## 3. Crate and module changes

| Crate | Change |
| --- | --- |
| `anyflow-proto` | manifest only (lint scope) |
| `anyflow-core` | `IdentityProvider`, `IdentityBackend`, `KeyBacking`, `IdentityState`, `SecretStore`, identity classification, `platform/unix_fs`, schema 2, TLS resolvers, `unix-fs` feature |
| `anyflow-control` | **NEW (move)** — `daemon/src/control.rs` minus its Linux half, plus `transport.rs` |
| `anyflow-runtime` | **NEW (split)** — `listener`, `mdns`, `state`, `server` (generic over `ControlListener`) |
| `anyflow-linux` | **NEW** — UDS transport, `control_socket_path`, `open_store`, path/name resolution |
| `anyflow-capability-files` | `FileSink` + `Destination` handle; `destination.rs` → `UnixDownloadSink` behind `unix-fs`; filename hardening |
| `anyflow-capability-clipboard` | `sensitive_support()`; `--sensitive` probe; backends behind `linux-backends`; `x11rb` optional |
| `anyflow-capability-battery` | manifest only |
| `anyflow-daemon` | thin composition + `main`; re-export shims so no test moved |
| `anyflow-cli`, `anyflow-gui` | depend on `anyflow-control` + `anyflow-linux`, **not** `anyflow-daemon` |

Five crate names, three of them new code. `anyflow-core` keeps its name, its
path and its public API surface. No cosmetic rename was made.

---

## 4. Platform seams implemented

Six, each justified by a verified platform difference. The six the
specification rejected were **not** created.

| Seam | Status | The difference that requires it |
| --- | --- | --- |
| `IdentityProvider` | **NEW** | Enclave/TPM/Keystore keys cannot be exported as PKCS#8 |
| `IdentityBackend` | **NEW** | An Enclave key is *generated inside* the Enclave; only a handle leaves |
| `SecretStore` | **NEW** | A DACL is not `0o600`; dropping the check on Windows would be a silent regression |
| `ControlTransport` / `ControlListener` | **NEW** | `UnixStream` is `#[cfg(unix)]`; a named pipe's default DACL grants Everyone read |
| `FileSink` | **NEW** | `FOLDERID_Downloads` vs `$XDG_DOWNLOAD_DIR`; iOS has no Downloads at all |
| `ClipboardBackend` | reused | already existed; extended with `sensitive_support()` |
| `BatterySource` | reused | already existed; untouched |

**Not created:** `DiscoveryBackend`, `NotificationBackend`, `RuntimeLifecycle`,
`DestinationResolver`, `FileSystemBackend`, `LocalIpcBackend`.

`IdentityBackend` is the one seam beyond the specification's list. It is not a
seventh concern: it is the creation half of `IdentityProvider`, split out
because `create()` is where hardware differs most, and because folding it into
the provider would have forced an `export_secret()` method — reintroducing
exportability into the very trait that exists to remove it.

---

## 5. The portable core boundary

`anyflow-core` and the three capability crates build with
`--no-default-features` and contain no `std::os`, no environment assumption and
no filesystem assumption. Verified two ways:

1. **Grep gate** — `core/tests/portable_boundary.rs`, 7 tests, run by
   `cargo test`. It also checks that each declared exception still exists, that
   each is genuinely behind a `#[cfg(feature)]`, that the security-critical
   files carry no conditional arm *at all*, and that the `unsafe_code` policy
   is what §11 says it is.
2. **Cross-compile** — all six crates built for `x86_64-pc-windows-gnu`. See
   §17.

### The one deviation from the specification, and why

The specification puts the Unix filesystem implementation in `anyflow-linux`.
It is instead in `anyflow-core/src/platform/unix_fs.rs`, behind the default-on
`unix-fs` feature, and `anyflow-linux` re-exports it.

The reason is **CC-5**: `Store::open(dir)` is called by
`core/tests/identity_and_store.rs`, `daemon/tests/common/mod.rs` and
`daemon/examples/fake_phone.rs`, and no refactor may edit a test to pass.
Moving the constructor to an adapter crate would have required editing tests.
The specification's own compile gate is written `--no-default-features`, which
is precisely the shape this needs, so the deviation is small and the gate it
must satisfy is unchanged.

The allowance is bounded and checked: four files, each named in
`FEATURE_GATED_PLATFORM_MODULES`, each asserted to be behind a feature. Adding
a fifth is a decision a reviewer will see.

---

## 6. Identity abstraction

```rust
pub trait IdentityProvider: Send + Sync + Debug {
    fn device_id(&self) -> &str;
    fn device_name(&self) -> &str;
    fn platform(&self) -> Platform;          // fixes audit finding C2
    fn certificate_der(&self) -> &CertificateDer<'static>;
    fn fingerprint(&self) -> Fingerprint;
    fn signing_key(&self) -> Arc<dyn rustls::sign::SigningKey>;
    fn backing(&self) -> KeyBacking;
    fn verify_protection(&self) -> Result<()>;
    // provided: device_info(), certified_key()
}
```

There is no method returning key bytes, and there is no way to add one without
excluding every hardware keystore — which is the point.

`LocalIdentity` keeps its entire existing public surface (so no call site
moved) and implements the trait. A blanket `impl IdentityProvider for Arc<T>`
means `&Arc<LocalIdentity>`, which several call sites already held, still
reaches the TLS builders. `Identity` is an owning handle with inherent
delegating methods, so `store.identity().fingerprint()` compiles with no trait
import — that is what let this land without editing a single existing caller.

`platform()` on the provider is the fix for **C2**: before Wave 0,
`Platform::Linux` was hardcoded at two lines inside `store.rs`, which meant the
persistence layer decided what kind of machine this was.

---

## 7. rustls `SigningKey` integration

The entire portability change to the most security-critical file in the
repository, `core/src/tls.rs`:

```diff
-        .with_single_cert(
-            vec![identity.certificate_der().clone()],
-            identity.rustls_private_key(),
-        )
-        .map_err(Error::Tls)?;
+        .with_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(
+            identity.certified_key(),
+        )));
```

```diff
-        .with_client_auth_cert(
-            vec![identity.certificate_der().clone()],
-            identity.rustls_private_key(),
-        )
-        .map_err(Error::Tls)?;
+        .with_client_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(
+            identity.certified_key(),
+        )));
```

Two call sites, exactly as the audit predicted. `rustls::sign::SingleCertAndKey`
is rustls's own resolver, implementing both `ResolvesServerCert` and
`ResolvesClientCert`; no custom resolver was written. Both builder methods
return the config directly, so the `.map_err(Error::Tls)?` disappears on both
lines.

`TLS13_ONLY`, `PinnedServerCertVerifier`, `RecordingClientCertVerifier`,
`verify_tls13_signature`, the TLS 1.2 refusals, ALPN, `send_tls13_tickets = 0`
and `peer_fingerprint` are **byte-for-byte unchanged**. The pinning verifiers —
which `tls.rs` itself calls "the single most dangerous thing in this codebase"
— are not in the diff.

Three rustls contracts the seam documents for future implementers:
`Signer::sign` is synchronous (no prompt, no `await`); the message is **not**
pre-hashed; ECDSA output must be X9.62 DER, not IEEE-P1363.

---

## 8. Linux software identity adapter

`SoftwareBacking` in `anyflow-core` creates and reloads a PKCS#8 P-256 key;
`FileSecretStore` in `core/src/platform/unix_fs.rs` stores it; `anyflow-linux`
composes the two with `Platform::Linux` and `/etc/hostname`.

**CC-4 holds.** `identity.key` is still PKCS#8 at 0600 inside a 0700 directory.
Wave 0 changed how the key is *reached*, not what is on disk.

Two behaviour changes were made deliberately and are the point of the wave:

1. `LocalIdentity::from_parts` now **parses the key** and checks that its
   public key matches the stored certificate (`CertifiedKey::keys_match`). A
   key and a certificate from two different identities used to load happily and
   then fail every handshake with a signature error that named neither.
   `InconsistentKeys::Unknown` is accepted, because a hardware backing may
   legitimately be unable to answer and refusing would ban it.
2. `default_device_name()` no longer falls back to the literal `"Fedora"`. It
   reads `/etc/hostname`, then `/proc/sys/kernel/hostname`, then
   `"AnyFlow Desktop"`. On this machine `/etc/hostname` is empty, so the old
   code told a phone it was pairing with "Fedora" regardless of the
   distribution. Existing installs keep their stored name (CC-3).

### The two modified tests

**CC-5 says a refactor that needs a test edited has changed behaviour, and
that this is a review stop.** Two tests were edited. Both are recorded here
because hiding them would be worse than the edit:

| Test | Edit | Why it is not a behaviour change |
| --- | --- | --- |
| `a_newer_schema_version_is_refused_rather_than_misread` | the fixture line `"schema_version": 1` became `format!("… {SCHEMA_VERSION}")` | The assertion — *refuse a newer schema* — is untouched and still passes. Only the literal that constructs the fixture moved, and it is now version-agnostic so a future bump cannot silently turn the test into a no-op |
| `store_never_persists_message_or_clipboard_content` | `"key_backing"` added to the expected key list | The test's own failure message instructs exactly this: *"state.json gained a top-level field; **confirm it holds no user content**"*. `key_backing` holds `"software"` — an enum naming how the private key is protected. Confirmed and recorded in the test |

Both edits are consequences of §7 items 4–5 of the specification, which
*require* adding `key_backing` and bumping `SCHEMA_VERSION`. The schema change
is mandated; these two tests are the ones that notice it.

---

## 9. Identity failure semantics

Six states, in `anyflow_core::identity::IdentityState`, with the codes the
specification names:

| State | Reached when | Action |
| --- | --- | --- |
| `IDENTITY_NOT_CREATED` | no state document **and** no key material, both established positively | **The only state that may create a key** |
| `IDENTITY_AVAILABLE` | both present, both parse, key matches certificate, backing matches the record | Proceed |
| `IDENTITY_TEMPORARILY_UNAVAILABLE` | a transient I/O failure on either | Retryable. Refuse to start. Never regenerate |
| `IDENTITY_HARDWARE_UNAVAILABLE` | the recorded `key_backing` is not the one this build can open | Fatal. Never regenerate |
| `IDENTITY_CORRUPTED` | key unparseable, key ≠ certificate, state unparseable, or the key's protection is not met | Fatal |
| `IDENTITY_LOST` | state present and key absent or **unreadable**; or key present with no state | Fatal |

`Store::probe_identity()` / `probe_identity_at()` report the state without side
effects, so an operator, a test or `anyflow status` can ask without risking
anything.

The classification is driven by `SecretStore`, whose contract keeps the four
answers apart that the old code collapsed:

```
Ok(None)                 genuinely absent            → the only path to create
Err(NotPrivate)          present, protection not met → fatal, verbatim remedy
Err(PermissionDenied)    present, cannot read        → IDENTITY_LOST
Err(Io)                  we could not find out       → IDENTITY_TEMPORARILY_UNAVAILABLE
Err(Corrupted)           present, not what it claims → IDENTITY_CORRUPTED
```

---

## 10. No-silent-regeneration proof

`core/tests/identity_states.rs`, 20 tests. Each injects one fault, asserts the
store refuses to start, and then asserts the two things that matter:
`state.json` is **byte-for-byte** what it was, and once the fault is removed
the *same* identity comes back — same fingerprint, same device id.

Faults covered: key at mode 000 · data directory non-traversable · key deleted
· state deleted · state malformed · state truncated · key unparseable · key
empty · key from a different identity · key at mode 0644 · recorded backing
`tpm` this build cannot open · injected `Io` on either read · injected
`PermissionDenied`.

Two structural assertions back them up:

* `only_not_created_may_create` — every state except `NotCreated` answers
  `false` to `may_create()`.
* the injected-fault tests assert `secrets.writes()` is **empty**: nothing was
  written to storage at all while the identity was unreadable.

The load-bearing line, in `Store::open_with`, is that `Resolution::Create` is
reachable from exactly one match arm: `(None, None)` — both reads returning
`Ok(None)`.

---

## 11. `state.json` safety fix

The defect, verbatim: `Path::exists()` returns `false` for *any* metadata
error — `EACCES`, a broken symlink, a non-traversable parent. `Store::open`
asked it about the state file and the key, and on `false` called
`initialize()`, which wrote a fresh `state.json` over the trust store.

Fixed by `Path::try_exists()` plus the `SecretStore` contract above. `NOT_FOUND`,
`PERMISSION_DENIED`, `IO_ERROR`, `NOT_PRIVATE` and `CORRUPTED` are five distinct
answers and only the first may lead to a write.

Regression tests: the six required cases plus seven more, in
`identity_states.rs` (20) and `core/src/platform/unix_fs.rs` (8 unit tests
covering absence, round-trip, loose mode, unreadable secret, unreadable state,
`harden`, name traversal, atomic replace).

A second instance of the same mistake was found and fixed while moving the
control socket: `server::bind` removed a socket file *unconditionally* if
`path.exists()`. It now distinguishes a stale file from a live owner by
connecting to it, and returns `BindError::AlreadyOwned` for the latter (§13).

---

## 12. Filename hardening and Unicode `Cf`

`capabilities/files/src/filename.rs`, protocol-global, no `#[cfg]`
(PLAT-DEC-014, SI-10). Four additions:

| Rule | Behaviour |
| --- | --- |
| `:` and `< > " \| ? *` | **stripped**. `report.pdf:payload.exe` → `report.pdfpayload.exe`; no NTFS alternate data stream can be named |
| Unicode category `Cf` | **stripped**. `invoice\u{202E}cod.exe` → `invoicecod.exe` |
| `CONIN$`, `CONOUT$` | added to `RESERVED_STEMS`; `NUL:` and `con:` are now rejected too, because the colon is stripped before the stem is checked |
| — | everything already present kept: basename split, `.`/`..`, `Cc`, trailing dots/spaces/tabs, `CON`–`LPT9`, byte cap on a char boundary |

**Strip rather than reject** was chosen for the character classes, consistent
with how this module has always treated a hostile *character* as opposed to a
hostile *name*: a reduced name is still the peer's name, an invented one is
not. `None` is still returned when nothing usable survives.

The `Cf` table is written out as ranges rather than pulled from a crate: it is
one auditable table, and adding a Unicode database to the crate that decides
what lands on the user's disk is a larger thing to trust than a list. It covers
the bidi controls, the zero-width set, `U+FEFF`, and the **tag block**
(`U+E0000`–`U+E007F`), which is invisible everywhere and can carry a whole
second string inside a filename.

8 new adversarial tests, including a property check over the whole hostile
corpus and `ordinary_unicode_is_untouched` — because the failure mode of a rule
like this is becoming "anything non-ASCII".

---

## 13. Local IPC boundary and Windows preparation

`anyflow_control::transport` defines `ControlTransport`, `ControlListener`,
`ControlStream` and `BindError`. `anyflow-runtime`'s control server is generic
over them and names no socket. `anyflow-linux` provides the only
implementation.

`BindError::AlreadyOwned` exists in Wave 0 even though Linux barely needs it,
because it is the shape the Windows named-pipe mitigation requires and
retrofitting an error variant into a trait after adapters exist is exactly the
churn Wave 0 prevents. The contract is documented on the trait:

> Implementations **must** return `AlreadyOwned` when the name is held by a
> live process, and **must not** fall back to another name.

That preserves, without writing a line of Win32: an explicit pipe DACL,
per-user access, `FILE_FLAG_FIRST_PIPE_INSTANCE`, abort-on-squatting, and no
fallback to an attacker-controlled alternate name.

No Named Pipe was implemented. No XPC was implemented. No Win32 concept was
put in the core.

---

## 14. `wl-copy --sensitive` compatibility

`--sensitive` was added in wl-clipboard **2.3.0**. Debian 13 and every current
Ubuntu LTS ship 2.2.1, whose unknown-option path is
`print_usage(stderr); exit(1)` — so passing the flag there does not degrade, it
**fails**. Fedora's `2.2.1^git20251124` *has* the flag, which is why a version
dependency is the wrong instrument.

Implemented per **PLAT-DEC-013(a)**:

1. **Probe once**, at construction, beside `probe_data_control()`. It runs
   `wl-copy --help` and looks for `--sensitive` in the output. `--help` needs
   no compositor, sets no clipboard, and is safe behind a lock screen. No shell
   is involved; the argument vector is built directly.
2. **Ordinary writes are untouched.** The verdict is consulted only when
   `sensitive` is set.
3. **A sensitive clip on an old `wl-copy` is refused**, with a message naming
   the cause and the remedy, instead of an opaque `wl-copy exited with 1`.
4. **The capability does not lie.** `ClipboardBackend::sensitive_support()`
   is a new pure predicate; `anyflow clipboard status` reports it on its own
   line, separate from `auto-send`, because the two fail for different reasons.
5. Unprovable support is reported as unsupported: the probe fails closed, in
   the same direction as the write path.

### On the brief's constraint

The brief asks that a `sensitive_hint` not become "falha total" when the
backend lacks the flag, while also directing that the documented decision be
followed — and PLAT-DEC-013 rejects option (b), writing the clip unmarked with
a warning, as *"a silent privacy downgrade"*.

These are reconciled as follows, and the reading is recorded so it can be
overruled: what is removed is the *unexplained total failure*. Normal clipboard
traffic is unaffected; only the sensitive path refuses, and it refuses with a
named cause and a named remedy. Writing a password into Klipper's history
without telling anyone is the outcome the decision forbids, and it stays
forbidden. **If the intent was option (b), this is a one-line change and a
decision for the maintainer, not for the implementer.**

Six tests, covering: supports the flag · does not support it · unknown/absent
option output · an unrunnable `wl-copy` · the capability's own report · an
ordinary write unaffected by the verdict. The probe's decision is split into
`probe_sensitive_from_output()` so it is testable without a `wl-copy` of either
vintage — which matters, because the development machine is exactly the one
where the defect cannot be reproduced.

---

## 15. `unsafe_code` policy boundary

`forbid` cannot be relaxed by an inner `#[allow]`, so a workspace-wide `forbid`
made every future adapter impossible to write without first weakening the core.

The workspace `[lints]` table is **removed**, and the lint is declared per
crate:

| Level | Crates |
| --- | --- |
| `forbid` | `anyflow-proto`, `anyflow-core`, `anyflow-control`, `anyflow-runtime`, `anyflow-capability-{battery,clipboard,files}` |
| `deny` | `anyflow-linux`, `anyflow-daemon`, `anyflow-cli`, `anyflow-gui` |

Relaxing the *workspace default* to `deny` was considered and rejected: it
silently drops the guarantee from the crates where it matters most. With no
default at all, a new crate that declares nothing gets rustc's `allow` — which
shows up in review as a **missing block** rather than as an invisible
weakening. The reasoning is recorded in `desktop/Cargo.toml` where the table
used to be.

> **Superseded in part — 2026-10-03, branch `feature/macos-desktop-v1`.** The
> architectural event below happened: the macOS adapter (`pliwee-macos`) needs
> FFI for IOKit's power-source API and for one AppKit constant. The test was
> renamed `no_crate_uses_unsafe_outside_the_declared_ffi_modules`; it allows
> `unsafe` in exactly two declared files, each block justified by a
> `// SAFETY:` comment that a new test requires, and fails anywhere else. The
> portable crates still `forbid` it. See
> [ADR-0021](../../adr/ADR-0021-macos-desktop-integration.md) D2 and
> [`reports/macos/MACOS-DESKTOP-V1.md`](../macos/MACOS-DESKTOP-V1.md) §6. The
> original text stands below.

`no_crate_actually_uses_unsafe_today` asserts that Wave 0 added none. If it
starts failing, that is an architectural event worth a conversation, not a
silenced assertion.

---

## 16. Discovery boundary

`mdns-sd` claims Linux, macOS and Windows, so a `DiscoveryBackend` trait would
today abstract over one implementation. **No seam was created**, as the
specification directs — add it when a second implementation exists.

`core/src/discovery.rs` (the record model, no responder) stays portable and is
in the cross-compile gate. `runtime/src/mdns.rs` (the responder) moved to
`anyflow-runtime` unchanged. Linux discovery behaviour is byte-identical; the
daemon advertised `_anyflow._tcp.local.` on IPv4+IPv6 during the smoke test.

`anyflow-runtime` is deliberately **excluded** from the portable compile gate
because of `mdns-sd`, whose Windows behaviour is V-12 / POC-WIN-02 and outside
Wave 0.

---

## 17. P0 PoCs and results

The four P0 PoCs, from [21 §4](../../sprints/../research/platform-expansion/21-POC-MASTER-PLAN.md)
and [27 §7.3](../../sprints/../research/platform-expansion/27-ARCHITECTURE-DECISION-CLOSEOUT.md):

### POC-CORE-01 — cross-compile the portable crates · **PASS**

```bash
cargo build --target x86_64-pc-windows-gnu \
  -p anyflow-proto -p anyflow-core -p anyflow-control \
  -p anyflow-capability-clipboard -p anyflow-capability-files \
  -p anyflow-capability-battery --no-default-features
```

Six `.rlib`s produced, from a clean target directory, with `ring` compiled for
`x86_64-pc-windows-gnu` through `x86_64-w64-mingw32-gcc`. The Windows `std` and
the MinGW toolchain came from Fedora's `rust-std-static-x86_64-pc-windows-gnu`
and `mingw64-gcc`, extracted into a scratch sysroot without installing
anything system-wide. No security control was `#[cfg]`-ed away and no stub was
written: the crates compile because the platform code is behind a feature, not
because it was deleted.

The first attempt **failed**, usefully: `anyflow-capability-files` depended on
`anyflow-core` with default features, so Cargo's feature unification switched
`unix-fs` back on for the whole selection. Fixed by giving the capability
crates and `anyflow-runtime` `default-features = false` on `anyflow-core`. Had
the gate been grep-only, that would have passed for the wrong reason.

### POC-CORE-02 — `IdentitySigner` seam is behaviour-preserving · **PASS**

Every existing test passes unmodified, including
`core/tests/identity_and_store.rs` (which asserts a 0644 key is refused) and
`daemon/tests/e2e.rs`. `require_private_mode`'s hard failure survives verbatim
— the exact message, including the `chmod 600` remedy, is asserted.

Beyond the specification's requirement, `core/tests/identity_seam.rs` drives
**four complete pinned TLS 1.3 handshakes** from a non-exportable provider:
server-side, client-side, both-ends, and a pinning-rejection case that asserts
the failure comes from `PinnedServerCertVerifier` during the handshake and not
from an application-layer check afterwards. A fifth asserts a signer that
refuses fails the handshake rather than weakening it.

Interop with the shipping Android app is **not** covered — see G6.

### POC-CORE-03 — `ControlTransport` seam · **PASS**

`daemon/tests/control.rs` (4 tests) passes unmodified. The GUI builds without
the daemon crate:

```
$ cargo tree -p anyflow-gui | grep -c anyflow-daemon
0
$ cargo tree -p anyflow-cli | grep -c anyflow-daemon
0
```

The "local only, never reachable from the network" property is preserved and
restated on the new trait. Six tests in `anyflow-linux` cover bind, double-bind
(`AlreadyOwned`), stale-socket replacement, 0600 mode, release, and the path.

### POC-CORE-04 — Windows-target MSVC check on a Windows runner · **PASS**

Executed 2026-09-01 on a GitHub-hosted Windows runner, as CI-001. This is the
canonical evidence record; every other document links here rather than
restating it.

| | |
| --- | --- |
| Workflow | `.github/workflows/portable-windows-msvc.yml` — job `cargo check · x86_64-pc-windows-msvc` |
| Run | [**33465365649**](https://github.com/yurisismotto/anyflow/actions/runs/33465365649) · event `push` · **conclusion `success`** |
| Commit | `cfd33f6fa5bb525fa1a650574f6fa870fdac2708` on `feature/core-platform-abstraction-v1` |
| Runner image | `windows-2025-vs2026` 20260824.214.3 · Microsoft Windows Server 2025 10.0.26100 Datacenter · runner 2.337.0 |
| C toolchain | Visual Studio Enterprise 2026, 18.9.12112.369 |
| `rustc -Vv` | `rustc 1.98.0 (88d9e12ae 2026-08-18)` · LLVM 22.1.8 · **`host: x86_64-pc-windows-msvc`** |
| `cargo -V` | `cargo 1.98.0 (797e8a9bc 2026-08-05)` |
| Active toolchain | `stable-x86_64-pc-windows-msvc` (overridden by `desktop/rust-toolchain.toml`) |
| Target installed | `x86_64-pc-windows-msvc` |
| Steps | 12 of 12 `success`; 0 rustc warnings; no `continue-on-error` anywhere |

The gate itself, verbatim from [28 §10.3](../../sprints/../research/platform-expansion/28-WAVE-0-IMPLEMENTATION-SPEC.md),
plus `--locked`:

```
cargo check --locked --no-default-features --target x86_64-pc-windows-msvc \
  -p anyflow-proto -p anyflow-core -p anyflow-control \
  -p anyflow-capability-clipboard -p anyflow-capability-files -p anyflow-capability-battery
```

`Finished dev profile … in 1m 07s`. The same six crates then **built** (`cargo
build`, same flags — codegen and link, where `ring`'s MSVC objects have to
work): `Finished … in 24.09s`.

| Beyond the specified check | Result on the runner |
| --- | --- |
| Host-triple assertion — a `-gnu` host fails the job | `effective host triple: x86_64-pc-windows-msvc` |
| Portable package set still exists (`cargo metadata`) | all six present |
| `cargo test --no-run`, five crates + `anyflow-core --lib --test identity_seam --test pairing --test portable_boundary --test protocol` | 14 MSVC test executables linked under `target\x86_64-pc-windows-msvc\debug\deps\` |
| Dependency boundary on the **resolved graph** (`cargo tree`), not on source | `boundary clean: 87 packages, no platform crate, no platform feature` — the same 87 the Linux dry run predicted. `libc` appears only as a `tokio` *feature*; the packages are `windows-sys v0.61.2`, `mio`, `socket2` |
| `ring` compiled by MSVC | `ring_core_0_17_14_.lib` and `ring_core_0_17_14__test.lib` in `target/x86_64-pc-windows-msvc/debug/build/ring-ea9757050f2935d9/out/`; `rustls v0.23.43 [ring,std]` in the graph |
| `unsafe_code` policy (ARCH-010) | `unsafe policy intact: forbid in the portable/security crates, deny in the adapters` |

`anyflow-runtime` is **deliberately excluded**, per §10.3 — its `mdns-sd`
Windows behaviour is V-12 / POC-WIN-02, not Wave 0's problem.

`ring` was not replaced and no crypto backend was substituted. Two
`anyflow-core` test targets — `identity_and_store.rs` and `identity_states.rs`
— are excluded because they test the Unix filesystem adapter itself
(`Store::open`, `probe_identity_at`), which does not exist with `unix-fs` off;
a guard step fails the job if `core/tests/*.rs` changes, so a new core test
cannot slip past the gate unnoticed.

**What this does not prove:** that AnyFlow *runs* on Windows. A compile check
is a boundary regression test. Runtime certification is Wave 5+, and a green
CI-001 must never be read as Windows support.

No other PoC was executed. Nothing PoC-shaped was left in production code: the
cross-compile used a scratch sysroot outside the repository, and the only new
code is tests.

---

## 18. Regression evidence

### Fedora smoke — **PASS**

Run against a **copy** of the real, live, pre-Wave-0 store at
`~/.local/share/anyflow/` (the original was never written to and is still
schema 1).

| Check | Result |
| --- | --- |
| daemon starts | ✅ `device=795fec0868…` `name=Fedora` `fingerprint=DF65 D3E4 BA28 EDF9` `key_backing=software` |
| pairing state readable | ✅ 4 pre-existing peers, 2 revoked, grants intact |
| discovery | ✅ `advertising _anyflow._tcp.local. port=55432 families=IPv4+IPv6` |
| control endpoint | ✅ `/run/user/1000/anyflow/control.sock` |
| `anyflow status` / `devices` | ✅ stable output, plus the new `key  software-backed` line |
| pairing | ✅ new device paired over real TLS 1.3 with real pinning |
| `battery.v1` | ✅ `sent battery.v1: 87% charging`; PING/PONG 2 ms |
| `files.v1` phone → desktop | ✅ `offered → transferring → completed`, file at 0600, content exact |
| `files.v1` desktop → phone | ✅ `waiting_accept → transferring → verifying → completed` |
| `clipboard.v1` status | ✅ backend detected, watch source XFIXES, `sensitive marking: yes` |
| GUI starts | ✅ GTK4/libadwaita Wayland client, running |
| GUI ↔ agent | ✅ 40 requests in 20 s over the control endpoint — `status`, `devices`, `transfers`, `clipboard_status` — with the GUI no longer depending on `anyflow-daemon` |

**`clipboard.v1` real round-trip: since executed — PASS.** When this section
was first written the graphical session had no clipboard seat available to a
non-interactive shell (plain `wl-copy` timed out identically, exit 124 — GNOME
lock-screen behaviour, not attributable to Wave 0). It was re-run on an
unlocked seat: the `sensitive_hint` round trip passes in both directions, and
the 9 previously-ignored `real_backend` tests pass (9/9). That closes **G10**.
Evidence:
[`WAVE-0-LOCAL-CERTIFICATION-REPORT.md`](../../certification/foundation/WAVE-0-LOCAL-CERTIFICATION-REPORT.md)
§§10–14.

### G7 — pre-Wave-0 store upgrades in place — **PASS**

| | before (real, old binary) | after (Wave 0) |
| --- | --- | --- |
| `schema_version` | 1 | 2 |
| `key_backing` | absent | `software` |
| `device_id` | `795fec0868ebef8c3d7ad3e775dc6ed0` | **same** |
| fingerprint | `DF65 D3E4 BA28 EDF9` | **same** |
| peers | 4 (2 revoked) | **same 4**, revocations and per-capability grants intact |

No re-pairing. The user's live store was not modified.

### Android — build **PASS**, interop **PASS**

```
./gradlew clean                      BUILD SUCCESSFUL
./gradlew testDebugUnitTest          BUILD SUCCESSFUL — 232 tests, 0 failures
./gradlew :app:assembleDebug         BUILD SUCCESSFUL — app-debug.apk
./gradlew :app:assembleDebugAndroidTest  BUILD SUCCESSFUL — app-debug-androidTest.apk
```

`git diff --stat android/` is empty — not one Android file changed.

**Closed on hardware, 2026-08-31/09-01.** The interop and instrumented runs
that were pending when this section was first written have since been executed
on the SM-X620 against the post-Wave-0 daemon: `battery.v1`, `clipboard.v1`
both directions, `files.v1` both directions (real Sharesheet), and
`./gradlew :app:connectedDebugAndroidTest` → **21 tests, 0 failures, 0
skipped**. The Android Keystore identity survived instrumentation with no
re-pair. Full evidence:
[`WAVE-0-LOCAL-CERTIFICATION-REPORT.md`](../../certification/foundation/WAVE-0-LOCAL-CERTIFICATION-REPORT.md)
§§4–9, 21–22. That closes **G6**.

### Everything else

| Suite | Tests | Result |
| --- | :-: | --- |
| `core/tests/identity_and_store.rs` | 24 | ✅ (2 fixture lines edited, §8) |
| `core/tests/pairing.rs` | 21 | ✅ unmodified |
| `core/tests/protocol.rs` | 12 | ✅ unmodified |
| `daemon/tests/e2e.rs` | 18 | ✅ unmodified |
| `daemon/tests/files.rs` | 36 | ✅ unmodified |
| `daemon/tests/clipboard.rs` | 21 | ✅ unmodified |
| `daemon/tests/wire.rs` | 10 | ✅ unmodified |
| `daemon/tests/sessions.rs` | 6 | ✅ unmodified |
| `daemon/tests/listen.rs` | 5 | ✅ unmodified |
| `daemon/tests/control.rs` | 4 | ✅ unmodified |
| `capabilities/clipboard/tests/security.rs` | 31 | ✅ unmodified |
| `capabilities/clipboard/tests/{loops,logging}.rs` | 13 | ✅ unmodified |
| **New:** `core/tests/identity_states.rs` | 20 | ✅ |
| **New:** `core/tests/identity_seam.rs` | 8 | ✅ |
| **New:** `core/tests/portable_boundary.rs` | 7 | ✅ |
| **New:** `anyflow-linux` unit | 6 | ✅ |
| **New:** `unix_fs`, filename, sensitive units | +22 | ✅ |

---

## 19. Bugs found

| # | Where | Severity |
| --- | --- | --- |
| 1 | `store.rs` — `Path::exists()` conflated absence with any metadata error; `initialize()` overwrote the trust store | **Critical.** Fixed |
| 2 | `filename.rs` — `:` (alternate data streams) and Unicode `Cf` (`U+202E` bidi spoofing) passed through | **High.** Fixed |
| 3 | `backend/wayland.rs` — `--sensitive` passed unconditionally; fails outright on wl-clipboard < 2.3.0 | **High** on Debian/Ubuntu. Fixed |
| 4 | `Cargo.toml` — `unsafe_code = "forbid"` workspace-wide, unrelaxable | **Blocker** for adapters. Fixed |
| 5 | **NEW:** `server::bind` removed the control socket unconditionally when the path existed, on the reasoning that "a second live daemon would have failed its own port bind first" — but the port bind happens *after* it in `main` | Medium. Fixed with a live-owner probe and `BindError::AlreadyOwned` |
| 6 | **NEW:** `identity.key` and the stored certificate were never checked against each other; a mismatched pair loaded and then failed every handshake with a signature error naming neither | Medium. Fixed |
| 7 | **NEW:** `default_device_name()` returned the literal `"Fedora"` whenever `/etc/hostname` was empty — which it is on this very machine, so the daemon has been telling phones it is "Fedora" regardless of distribution | Low. Fixed |
| 8 | **NEW (pre-existing, not fixed):** `anyflow status \| head` panics with `Broken pipe` instead of exiting quietly | Cosmetic. Recorded, out of Wave 0 scope |
| 9 | **NEW (pre-existing, fixed):** two `clippy::err_expect` warnings in `core/tests/identity_and_store.rs`, present at `HEAD` | Trivial |

---

## 20. Security audit

Every invariant re-checked. **Wave 0 changes where keys live and how the local
endpoint is guarded. It changes nothing about who is trusted or why.**

| # | Invariant | Status |
| --- | --- | --- |
| SI-1 | TLS 1.3 only | ✅ `TLS13_ONLY` unchanged |
| SI-2 | SPKI pinning is the sole identity check | ✅ both verifiers untouched |
| SI-3 | Hostnames and IPs are never identity | ✅ no SAN validation reintroduced |
| SI-4 | Pairing explicit and human-confirmed | ✅ `pairing.rs` untouched; smoke-tested |
| SI-5 | Per-capability grants re-checked per message | ✅ `capability.rs` untouched |
| SI-6 | `files.v1` never auto-granted | ✅ `auto_grant` unchanged |
| SI-7 | No clipboard content in logs | ✅ `logging.rs` passes; the new refusal message asserts it carries no content |
| SI-8 | Private-key protection check never weakened | ✅ enforced *earlier* than before — inside `read_secret`, before a byte is returned |
| SI-9 | `initialize()` reachable only from `NOT_CREATED` | ✅ one match arm; 20 tests |
| SI-10 | Filename sanitisation protocol-global, no `#[cfg]` | ✅ asserted by `portable_boundary.rs` |
| SI-11 | `unsafe_code = "forbid"` on the portable crates | ✅ asserted by `portable_boundary.rs` |
| SI-12 | Session resumption disabled | ✅ `send_tls13_tickets = 0` unchanged |

Additional checks: no cleartext transport; no clipboard persistence, history or
relay; unknown peers still fail closed; no trust from discovery; no trust-all
verifier; ECDSA P-256 unchanged; revocation semantics unchanged and verified on
real records; no secret logging — `KeyBacking` is displayed locally and, by
test, never appears in `DeviceInfo` (PLAT-DEC-012).

**Net security position: improved.** Three defects fixed, two of them
present-tense on Linux; one new defect found and fixed; certificate/key
continuity now checked; the protection check moved earlier.

---

## 21. Performance

No busy polling introduced. No unbounded queue or cache added. No whole-file
buffering. No blocking crypto moved onto the async event loop — `Signer::sign`
is where it always was, inside rustls's handshake.

Two bounded additions at startup only: the `wl-copy --help` probe (one
short-lived process, once, beside the existing `wl-paste --watch` probe), and
one extra `connect()` on the control socket path when a socket file already
exists, which on a Unix socket resolves immediately with no network round trip.

`LocalIdentity` now builds its `SigningKey` once at construction instead of
cloning PKCS#8 bytes on every `server_config`/`client_config` call — marginally
cheaper, and it removes a key copy per TLS config.

---

## 22. Files changed

33 tracked files modified, 12 added, 5 moved. `git diff --stat` totals 1981
insertions and 310 deletions before the new files.

**Not touched:** `protocol/**` (0 changed), `android/**` (0 changed),
`.github/**`, `packaging/**`, `docs/adr/**`,
`docs/security/THREAT_MODEL.md`, `desktop/gui/src/views/**` except the import
line.

---

## 23. Technical debt

| # | Item |
| --- | --- |
| 1 | **`anyflow-runtime` is not in the portable gate.** It composes the capability crates with default features, so it is Unix-only today. Deliberate — `mdns-sd`'s Windows behaviour is V-12 — but a future wave should propagate `default-features = false` through it |
| 2 | **The Unix filesystem adapter lives in `anyflow-core` behind a feature**, not in `anyflow-linux`. Forced by CC-5 (§5). Revisit when the test suite may be edited |
| 3 | ~~**CI-001 not written.**~~ **Closed** — `.github/workflows/portable-windows-msvc.yml` landed with the Wave 0 commit and is green (§17). Remaining: the workflow has **no dependency cache** (a first workflow adding a third-party action is a supply-chain decision, deferred deliberately) |
| 4 | **The `Cf` table is hand-maintained.** Unicode 16.0. A character added to `Cf` later is a missed strip until the table is revised |
| 5 | **`anyflow status \| head` panics on `SIGPIPE`.** Pre-existing |
| 6 | **`Store::load` parses `state.json` twice** — once to classify, once to load. Negligible on a file of tens of records; noted so it is a choice, not an oversight |
| 7 | **PLAT-DEC-013's reading is recorded, not ratified** (§14). If the maintainer intended option (b), it is a one-line change |
| 8 | **B-1 — a failed Sharesheet offer leaves the dialog stuck on "Sending…"** (Android, real bug). Found during the G6 hardware run. Tracked as [issue #12](https://github.com/yurisismotto/anyflow/issues/12), P2. Deliberately **not** fixed in the certification commit — it is an Android change, and `android/**` must stay at 0 files changed for Wave 0 |
| 9 | **B-2 — `real_backend` calls `wl-copy --clear` unbounded**, so it hangs forever on a locked seat (test-only). Tracked as [issue #13](https://github.com/yurisismotto/anyflow/issues/13), P3 |

---

## 24. Acceptance gates

### Official Wave 0 gates ([28 §12](../../sprints/../research/platform-expansion/28-WAVE-0-IMPLEMENTATION-SPEC.md))

| # | Gate | Result |
| --- | --- | --- |
| G1 | existing tests pass unmodified | ⚠️ **PASS with two documented exceptions** (§8) — both schema fixtures, both mandated by the spec's own §7 |
| G2 | new tests from §10.2 pass | ✅ PASS — 63 added |
| G3 | Windows compile gate for the six portable crates | ✅ **PASS** — `-msvc` on a GitHub-hosted Windows runner, [run 33465365649](https://github.com/yurisismotto/anyflow/actions/runs/33465365649) (§17) |
| G4 | no `std::os::unix` in the portable crates | ✅ PASS |
| G5 | GUI and CLI build without `anyflow-daemon` | ✅ PASS — `cargo tree \| grep -c` → 0 for both |
| G6 | **the unmodified Android app pairs, connects, sends and receives** | ✅ **PASS** — on the SM-X620; pair, `battery.v1`, `clipboard.v1` both ways, `files.v1` both ways (§18) |
| G7 | a pre-Wave-0 `state.json` upgrades in place | ✅ PASS — on a real one |
| G8 | identity fault injection refuses and leaves `state.json` byte-identical | ✅ PASS — 20 tests |
| G9 | `anyflow status` reports `KeyBacking::Software` | ✅ PASS — `key  software-backed` |
| G10 | Fedora hardware smoke | ✅ **PASS** — including `sensitive_hint` both directions, re-run on an unlocked seat (§18) |
| G11 | clippy clean; `unsafe_code` lints as specified | ✅ PASS — 0 warnings |
| G12 | no `.proto` changed; `PROTOCOL_VERSION_MAX` unchanged | ✅ PASS — `git diff --stat protocol/` empty |

**G1, G6 and G12 cannot be waived.** All three pass. G12 passes with
`git diff --stat protocol/` empty. G1 passes with two edits that are
consequences of a schema change the specification mandates, and which are
declared here rather than hidden. **G6 ran, on real hardware, and passed.**

### Reported gates, mapped to the official ones

| Reported | Maps to | Result |
| --- | --- | --- |
| W0-IDENTITY-SEAM | G2, POC-CORE-02 | **PASS** |
| W0-NO-SILENT-REGENERATION | G8 | **PASS** |
| W0-STATE-SAFETY | G7, G8 | **PASS** |
| W0-FILENAME-HARDENING | G2 | **PASS** |
| W0-SENSITIVE-COMPAT | G2, G10 | **PASS** — real round-trip executed |
| W0-UNSAFE-BOUNDARY | G11 | **PASS** |
| W0-PORTABLE-CORE | G3, G4, G5 | **PASS** — grep + `-gnu` + `-msvc` on a Windows runner |
| W0-LINUX-REGRESSION | G10 | **PASS** |
| W0-TLS-REGRESSION | G1 | **PASS** |
| W0-FILES-REGRESSION | G1, G10 | **PASS** |
| W0-CLIPBOARD-REGRESSION | G1, G10 | **PASS** — logic and real round-trip |
| W0-ANDROID-REGRESSION | G6 | **PASS** — 232 JVM + 21 instrumented on the SM-X620 |
| W0-P0-POCS | POC-CORE-01/02/03/04 | **4 PASS** |

---

## 25. Git status

Branch `feature/core-platform-abstraction-v1`. Committed and pushed as
**`cfd33f6`** — the certification-candidate commit, and the commit CI-001 ran
against. `git diff --check` clean. Nothing matching `*.key`, `*.pem`, `*.p12`, `*.pfx`,
`*.jks`, `*.keystore`, `*.apk`, `*.aab`, `state.json`, a trust store, an
identity file, a log or a build output is tracked, staged or untracked in the
repository. `android/app/build` and `desktop/target` are git-ignored.

One untracked file, `": add cross-platform expansion research and roadmap\""`,
predates this sprint — it is the artefact of a mistyped `git commit` in an
earlier session, and was left alone.

---

## 26. Final status

> ## **WAVE 0 CERTIFIED** — 2026-09-01

Every deliverable is implemented, every official gate passes, all four P0 PoCs
pass, and the Fedora smoke is clean on a real pre-Wave-0 store.

The three steps this section previously listed as remaining have all been
executed:

1. **Done.** The SM-X620 ran the unmodified `app-debug.apk` against the
   post-Wave-0 daemon: pair → connect → clipboard both ways → file both ways,
   plus 21 instrumented tests. **G6 PASS** (§18).
2. **Done.** `cargo test -p anyflow-capability-clipboard --test real_backend
   -- --ignored --test-threads=1` on an unlocked seat: 9/9, `sensitive_hint`
   round trip both directions. **G10 PASS** (§18).
3. **Done.** CI-001 landed as `.github/workflows/portable-windows-msvc.yml`
   and is green on a GitHub-hosted Windows runner.
   **POC-CORE-04 / G3 PASS** (§17).

**What certification means here, precisely.** Wave 0's claim is a negative one
— *AnyFlow on Fedora behaves identically to before, and the Rust workspace now
has an explicit platform boundary* — and that claim is now evidenced end to
end: unchanged protocol (`git diff --stat protocol/` empty), unchanged Android
source (0 files), unchanged behaviour on real hardware with a real pre-Wave-0
store, and a portable core that compiles under MSVC.

**What it does not mean.** It does not mean AnyFlow runs on Windows, macOS or
iOS. No platform was added in Wave 0. Windows runtime work — CNG, named pipes,
`mdns-sd` coexistence (V-12 / POC-WIN-02) — is Wave 5+, and a green CI-001
must never be read as Windows support.

**Open, and deliberately outside this certification:** B-1 ([#12](https://github.com/yurisismotto/anyflow/issues/12))
and B-2 ([#13](https://github.com/yurisismotto/anyflow/issues/13)) — see §23.
Neither is a Wave 0 gate; both are tracked.
