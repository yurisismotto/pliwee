# Pliwee rebrand — closure check

| Field | Value |
| --- | --- |
| Date | 2026-09-30 |
| Branch | `feat/pre-g8-autopilot` (every rebrand branch, W0 … accessibility fix, is its ancestor) |
| Code under test | `bebf9b1` (the Rust and Android suites ran on `ffc108c`; `bebf9b1` changes only `packaging/tests/`) |
| Host | Fedora 44, kernel 7.2.5-200.fc44.x86_64 |
| Verdict | **CLOSURE: PASS**, with the backlog in §4 |

## 0. What this replaces

The exhaustive G8 — 37 pre-G8 gates across four distributions, then a
certification wave on top — is replaced for the rebrand by the objective check
below. Nothing measured under the old plan is discarded: every pre-G8 record,
log and preserved attempt stays in the operator's evidence directory
(`~/.local/state/pliwee-pre-g8`), and the gates not run are listed in §4 as
not executed, with the reason, never as PASS.

## 1. The checks

| # | Check | Result | Evidence |
| --- | --- | --- | --- |
| C1 | Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --locked --workspace` | **PASS** — 1 147 tests, 0 failed | §2.1 |
| C2 | Android: `:app:assembleDebug :fixture:assembleDebug :app:assembleDebugAndroidTest :app:testDebugUnitTest`, signing-provision self-test, unsigned `bundleRelease` refused | **PASS** — 867 unit tests, 0 failed; provision 39/0; guard refused | §2.2 |
| C3 | Packaging and harness: `packaging-checks.sh`, `harness-selftests.sh`, `pre-g8-autopilot-selftests.sh`, `pre-g8-manual-gates-selftests.sh`, `g7up-gui-migration-selftests.sh`, `shellcheck -S warning` | **PASS** — 147/0, 106/0, 273/0, 229/0, 120/0, clean | §2.3 |
| C4 | Packages build | **PASS** for the Fedora 44 set used in C5 (built by the autopilot from `bbefb97`, product code identical to `bebf9b1`); all distributions are built again by `release-artifacts.yml` for the release | §2.4 |
| C5 | Real migration, Fedora 44: OmniBridge 1.0.0 with the physical tablet paired → Pliwee 1.1.0 | **PASS** — `G7UP-fedora44-UPGRADE`, 44 ok, 0 not ok | §3 |
| C6 | No residual OmniBridge identifier outside compatibility, history or migration documentation | **PASS**, with the public entry points listed in §2.5 moving with the repository migration | §2.5 |
| C7 | Working tree clean | **PASS** at the commit that adds this document | `git status` |

## 2. Detail

### 2.1 Rust

`desktop/`, toolchain from `rust-toolchain.toml`: format clean, clippy clean
with warnings denied, 76 test binaries, 1 147 tests passed, 0 failed.

### 2.2 Android

JDK 21 (Temurin), Gradle wrapper 8.11.1: both debug APKs and the instrumented
test APK assemble; `testDebugUnitTest` 867 tests, 0 failures, 0 errors;
`signing/tests/provision-selftest.sh` 39 passed, 0 failed; `bundleRelease`
without signing fails with *"Pliwee release signing is not configured:
PLIWEE_UPLOAD_KEYSTORE is not set"*, as it must.

### 2.3 Packaging and harness

ShellCheck was run from `docker.io/koalaman/shellcheck:stable` over
`packaging/tests/*.sh packaging/release/*.sh`, the CI invocation. Two harness
defects found on the way were fixed rather than worked around, each with a
self-test:

* `pre-g8-autopilot-selftests.sh` counted CLI steps with `grep -c` over a glob
  that can match one file, where grep prints no file name and the `:5$`
  filter rejected a correct count (`ffc108c`).
* `upgrade-gates.sh` U4 simulated the next login with `systemctl start
  user@UID` after `loginctl terminate-user`. With no login session, systemd
  259 (Fedora 44) times that job out, so U4 failed over a login that never
  happened (§3.1). U4 now logs in through the display manager, as O2 and
  lifecycle-gates L19 already did (`bebf9b1`).

### 2.4 Packages

`pliwee-1.1.0-1.fc44.x86_64`, `pliwee-gui-1.1.0-1.fc44.x86_64` and the
transitional `omnibridge-1.1.0-1.fc44.noarch` installed and upgraded cleanly
in C5. Between the build commit `bbefb97` and `bebf9b1` the only non-test,
non-documentation change is the `#[cfg(test)]` module of
`desktop/gui/src/selection.rs`.

### 2.5 Residual `omnibridge`

`git grep -il omnibridge` outside `docs/{audits,certification,reports,research,migrations,adr}`
falls into:

* **compatibility, by design (ADR-0020 D3, D4, D9):** the legacy protocol
  profile and its pinned wire-identity tests, the `omnibridged.service`
  alias, the transitional `omnibridge` packages and `omnibridge.xml`
  firewalld service, the data and `gui.json` migration, legacy download and
  partial-file handling, the retired Android signing certificates;
* **history:** the retired OmniBridge artwork in `docs/design/assets/` and its
  description in `BRAND.md`; `docs/security/THREAT_MODEL.md`'s naming note;
* **public entry points that move with the repository:** `README.md` (still
  the published OmniBridge 1.0.0 page by the note of 2026-09-25),
  `desktop/Cargo.toml` `repository`, `pliweed.service` `Documentation=`;
* **the Android store wave, not this one:** `docs/design/PLAY-STORE-LISTING.md`
  and `docs/policy/PRIVACY-POLICY.md`, whose current URL is compiled into
  sources and must keep resolving (ADR-0020 D5, public-URL rule).

No product identifier outside those groups.

## 3. Real migration — Fedora 44

Guest `pliwee-g8-f44-chain`, Fedora Linux 44 (Cloud Edition) with GNOME;
OmniBridge 1.0.0 installed from the published release (`G7UP-fedora44-INSTALL`,
2026-09-29T03:23:15Z); the physical Samsung SM-X620 paired through the
product, `clipboard.v1` and `files.v1` granted, clipboard policy
`send=on receive=on`, notification lock policy `when locked full`, and the
legacy `~/.config/omnibridge/gui.json` as a deterministic fixture in the
published 1.0.0 format naming that tablet's full fingerprint
(`G7UP-fedora44-U2`, measured, 2026-09-29T10:59:54Z).

`G7UP-fedora44-UPGRADE`, attempt 2026-09-30T01:49:12Z → 01:55:20Z, log
`logs/G7UP-fedora44-UPGRADE.20260930T014912Z.log` (sha256
`55d829d8720690c8ed223dc03d939b71cf7b9639b24bec179901e45ce904fc53`):

| Required | Measured |
| --- | --- |
| identity, device_id | O2 = O1: `g8-f44-chain (b25e74ab92b1aa618dce0ef66137ee0d)`, fingerprint `E80A 3534 DE50 F1B7`; `MIGRATED_FROM` records the O1 digest of `identity.key` |
| peer / fingerprint | O2 = O1: one peer, `D7F8 0D48 4347 2B11`, paired |
| grants | O2 = O1: `battery.v1, clipboard.v1, files.v1` |
| clipboard policy | `pliwee clipboard status` on the upgraded guest: `send=on receive=on auto-send=off auto-receive=off`, as set in U2; `state.json` copied byte for byte (digest in `MIGRATED_FROM`) |
| notification policy | `pliwee notifications status`: `mirror on  when locked full`, as set in U2 |
| gui.json / selected peer | pliwee-gui's first start migrated it once: same SHA-256 (`1e12dcba…c98e`), same 105 bytes, full fingerprint, 0600 in a 0700 directory; the second start changed nothing |
| legacy state intact | `~/.local/share/omnibridge/{identity.key,state.json}` and `~/.config/omnibridge/gui.json` byte-identical to O1, gui.json's owner, mode, size and mtime unchanged |
| Pliwee starts | U4: `pliweed` started at the next login through the legacy `omnibridged.service` enablement, logged the migration and the one-line re-enable instruction; U7: unchanged across a restart |
| Pliwee works | the tablet reconnected: `connected yes` for SM-X620 on the upgraded daemon |

The transitional package upgraded rather than erased `omnibridge`; no
scriptlet error; `omnibridged.service` resolves to `pliweed.service`; the
firewalld `omnibridge` service still resolves to 55432/tcp (U5); a second,
never-enabled account got no enablement link (U9).

### 3.1 The first attempt

`G7UP-fedora44-UPGRADE`, 2026-09-30T00:52:17Z, **FAIL** (exit 3): U4 *"pliweed
did not start at the next login"*. The record, its log and its evidence are
kept: the coordinator's history, and
`autopilot/preserved/G7UP-fedora44-UPGRADE.20260930T014900Z` (verified
identical). Diagnosed on the same upgraded guest the same night: a reboot's
autologin started `pliweed`, which migrated and kept identity, peer and
grants; `systemctl start user@1000.service` with no session failed *"because a
timeout was exceeded"*. A harness defect (§2.3), not a product one.

## 4. Backlog — carried, not blocking this closure

> **SUPERSEDED 2026-09-30, by `release/pliwee-1.1.0`**, for the first row
> only. The download-directory defect is fixed before 1.1.0: `pliweed.service`
> now grants `ReadWritePaths=-%h/Downloads`, and the daemon names the exact
> drop-in when a download directory elsewhere cannot be prepared. Measured end
> to end on this host, with `pliweed` under the unit file's own sandbox and
> `fake_phone` sending a file: with the previous unit, transfer `dfa7b7cb`
> *"could not open a temp file … Read-only file system (os error 30)"*,
> `state=failed`; with the fixed unit, transfer `bfb7b4f5` *"received,
> verified and stored"*, 0600. The row below is left as it was recorded.

| Item | State | Why it does not block |
| --- | --- | --- |
| **Files received by the packaged daemon cannot be written.** `pliweed.service` has `ProtectHome=read-only` and makes only `~/.local/share` writable; received files go to `<XDG downloads>/Pliwee`. Under the unit's sandbox a write into `~/Downloads/OmniBridge` is **denied** (probed on this host with `systemd-run -p ProtectSystem=strict -p ProtectHome=read-only -p ReadWritePaths=~/.local/share`), and the upgraded guest logged *"could not prepare the download directory: Read-only file system"*. | **open product defect, pre-existing** | Present unchanged in the published OmniBridge 1.0.0 unit; phone → desktop receive under the unit was never measured (lifecycle-peer-gates L15 records it NOT EXECUTED). It is not introduced by the rebrand and does not affect the upgrade. It is a release decision for 1.1.0, not a migration one. |
| W2-GNOME | FAIL (Orca: the Files switch announcement lacks *"Files for {name}"*) | accessibility, backlog by decision of 2026-09-30 |
| W2-KDE | not executed | accessibility, backlog |
| G7UP-fedora44 U6, SECLOG, U10 | not executed | the closure needs the migration and a working daemon, both measured in §3 |
| G7UP ubuntu2404 / ubuntu2604 / debian13 U2 → U10 | not executed | install and lifecycle are PASS on all three (U8, LIFECYCLE, INSTALL); the tablet migration was not repeated there |
