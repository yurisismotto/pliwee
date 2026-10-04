# Test tiers

Every test and gate in the repository, sorted by **what it needs to mean
anything**. The tier decides where a gate may run — and therefore what the
issue worker may claim. A gate run somewhere it cannot measure its subject is
not a pass; it is `NOT EXECUTED — <reason>`
([AGENTS.md § A PASS with no observed evidence is invalid](../../AGENTS.md#a-pass-with-no-observed-evidence-is-invalid)).

Mapped on 2026-10-04 against `main` at `b17e525`. The macOS rows describe
`feature/macos-desktop-v1`, which is not on `main` yet. Updated the same day
for the two CI jobs the owner approved (`rust-workspace.yml`,
`harness-selftests.yml`); the run times below were measured on the owner's
Fedora host with the suites running side by side, so they are upper bounds.

## The tiers

| Tier | Needs | Runs on | The worker… |
| --- | --- | --- | --- |
| **1** — portable, unit, protocol, static | a compiler, a JDK, a shell | GitHub-hosted runners and the worker host | runs every applicable one, and CI runs them again |
| **2** — Linux runtime, integration, packaging, lifecycle | a container engine, a libvirt guest, a real desktop session, or a signing key | the owner's Fedora host, with VMs; a self-hosted runner later | does **not** run them; lists the ones the change needs under "Not executed" |
| **3** — real platform and hardware | a physical Android phone over ADB, a Mac, Windows, an Android TV, a human at a screen | the owner, with the device | never runs them, never claims them |

## Tier 1

| Gate | Command | In CI | Notes |
| --- | --- | --- | --- |
| Rust format | `cd desktop && cargo fmt --all --check` | `desktop-quality.yml` | |
| Rust lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | `desktop-quality.yml` | stable toolchain, per `rust-toolchain.toml` |
| Rust tests — portable and Linux-generic | `cargo test --locked -p pliwee-capability-{clipboard,files,battery} -p pliwee-core -p pliwee-control -p pliwee-proto` | `linux-distro-compat.yml`, on Ubuntu 24.04 / 26.04 / Debian 13 containers | four tests that need an unprivileged user are skipped there, by name, with a guard that fails if a fifth appears |
| Rust tests — whole workspace | `cd desktop && cargo test --locked --workspace --all-targets` | `rust-workspace.yml` | includes the hostile-client suite and end-to-end pairing over loopback TLS. 1147 passed, 0 failed, 25 ignored, 215 s with the build, on the owner's host, and the same counts in a headless Ubuntu 24.04 container as a non-root user; the 25 are the real-session tests below, `#[ignore]` in the source. The workspace has no doctests (`--doc`: 0 in 11 crates) |
| MSRV | `cargo check` on the declared `rust-version` | `linux-distro-compat.yml` | |
| Distribution build | `cargo check -p pliwee-daemon/-cli/-gui` against each distro's own GTK | `linux-distro-compat.yml` | build compatibility only, not runtime |
| Windows portable boundary | `cargo check`/`build`, `cargo test --no-run`, MSVC | `portable-windows-msvc.yml` | compiles; does not run Pliwee on Windows |
| Android build | `./gradlew :app:assembleDebug :fixture:assembleDebug :app:assembleDebugAndroidTest` | `android-ci.yml` | instrumented tests compile; they do not run |
| Android unit tests | `./gradlew :app:testDebugUnitTest` | `android-ci.yml` | |
| Android release guard | `bundleRelease` without signing must fail | `android-ci.yml` | |
| Signing provisioning | `android/signing/tests/provision-selftest.sh` | `android-ci.yml` | throwaway keys |
| Packaging static checks | `./packaging/tests/packaging-checks.sh` | `packaging-checks.yml` | 148 checks on `docs/roadmap-v2` |
| Harness self-tests | `./packaging/tests/harness-selftests.sh` | `packaging-checks.yml` | |
| Release-signing negatives | `./packaging/tests/release-signing-tests.sh` | `packaging-checks.yml` | ephemeral keys |
| systemd unit | `systemd-analyze verify packaging/common/pliweed.service` | `packaging-checks.yml` | |
| Dependency advisories | `cargo audit --deny warnings` | `security-audit.yml` | also scheduled |
| Release artifact build | bundle, RPM, DEB, SBOM | `release-artifacts.yml` | builds; signing only with the key |
| Agent guard rails | `./.github/agent/agent-guard.sh --selftest` | `agent-guard.yml`, `harness-selftests.yml` | 58 passed, 0 failed, 6 s |
| Coordinator self-tests | `./packaging/tests/pre-g8-manual-gates-selftests.sh` | inside `harness-selftests.sh`, so `packaging-checks.yml` and `harness-selftests.yml` | 229 passed, 0 failed, 219 s; no guest needed. Listed as "not in CI" in the first version of this table — `harness-selftests.sh` already ran it |
| Autopilot self-tests | `./packaging/tests/pre-g8-autopilot-selftests.sh` | **no** | 273 passed, 0 failed, 1580 s (26 min) on 2026-10-04; no guest needed. Too slow for a per-PR gate, and it runs the G7-UP suite again inside itself: run it by hand, or on a schedule if the owner wants one |
| G7-UP migration self-tests | `./packaging/tests/g7up-gui-migration-selftests.sh` | `harness-selftests.yml` | 120 passed, 0 failed, 477 s; no VM |
| Evidence whitespace | `./packaging/tests/evidence-whitespace-check.sh [PATH…]` | `--selftest` in `harness-selftests.yml` | `--selftest`: 8 passed, 0 failed. Passes a CR at the end of a line, which `git diff --check` rejects — canary [#34](https://github.com/yurisismotto/pliwee/issues/34) |
| Whitespace in a diff | `git diff --check` | — | every change |

## Tier 2

| Gate | Command | Needs |
| --- | --- | --- |
| Install smoke | `./packaging/tests/install-smoke.sh` | a container engine, built packages |
| systemd unit gates S1–S3 | `./packaging/tests/systemd-unit-gates.sh` | a real systemd user session |
| Lifecycle gates L1–L26 | `./packaging/tests/lifecycle-gates.sh` | a libvirt guest per distribution |
| Upgrade gates G7-UP | `./packaging/tests/upgrade-gates.sh`, `u2-gui-fixture.sh`, `u2-state-check.sh` | a libvirt guest with OmniBridge 1.0.0 |
| Pre-G8 autopilot | `./packaging/tests/pre-g8-autopilot.sh` | guests; stops where a person is needed |
| Real clipboard backend | `cargo test -p pliwee-capability-clipboard --test real_backend -- --ignored --test-threads=1` | a real Wayland session |
| Real D-Bus / logind | the notifications crate's `real_dbus` and `real_lock` targets | a real session bus and logind session |
| Release-signing, production | `./packaging/tests/release-signing-production-tests.sh` | the real signed release set |

## Tier 3

| Gate | Command | Needs |
| --- | --- | --- |
| Android instrumented tests | `cd android && ./gradlew :app:connectedDebugAndroidTest` | a connected device or emulator |
| Peer lifecycle gates L12, L14–L16 | `./packaging/tests/lifecycle-peer-gates.sh` | a physical Android phone on the same LAN, ADB |
| Security log evidence | `./packaging/tests/security-log-evidence.sh` | real hardware |
| Pre-G8 manual gates | `./packaging/tests/pre-g8-manual-gates.sh` | an operator, a phone, a guest |
| GNOME / KDE desktop certification | `docs/certification/linux/` | a real graphical session |
| macOS (`feature/macos-desktop-v1`) | `macos/scripts/test.sh`, `macos/scripts/build-app.sh`, `cargo test -p pliwee-macos` | a Mac; the Keychain test needs a logged-in user |
| Windows runtime | — | Windows hardware; not started (platform-expansion Waves 5–6) |
| Android TV | — | Google TV hardware; roadmap #10 |

## Gaps found while mapping

1. **No CI job runs the whole Rust workspace's tests.** *Closed by
   `rust-workspace.yml` (2026-10-04), once it has run: a check that has never
   reported is not a closed gap.* `pliwee-runtime`,
   `pliwee-daemon`, `pliwee-cli`, `pliwee-gui`, `pliwee-linux` and
   `pliwee-capability-notifications` are tested only by a local
   `cargo test --workspace`. The worker runs that locally; nothing re-runs it in
   a clean environment. Closing it needs a measured list of which of those
   tests need a session bus, and a CI job for the rest.
2. **Three guest-free self-test suites are not in CI** (the coordinator, the
   autopilot and the G7-UP migration self-tests). They need no VM. The
   autopilot suite's run time has to be measured before it joins a PR gate.
   *2026-10-04: the coordinator suite was in CI all along, inside
   `harness-selftests.sh`. G7-UP joins `harness-selftests.yml`. The autopilot
   suite: measured at 26 minutes, so it stays out of the PR gate.*
3. **`main` has no branch protection and no ruleset.** No check is required
   before merge, and nothing on the server stops a push to `main`.
   *2026-10-04: ruleset `24443525` applied — PR required, no force-push, no
   deletion, no bypass. Required checks wait for their first observed run.* See
   [AGENT-WORKFLOW.md § Owner decisions](AGENT-WORKFLOW.md#owner-decisions).
4. **No self-hosted runner is registered**, so no Tier 2 gate runs on a pull
   request.

None of these was fixed when this was mapped; each was a decision about CI time
or repository settings that belongs to the owner. The owner took them on
2026-10-04 ([AGENT-WORKFLOW.md § Owner decisions](AGENT-WORKFLOW.md#owner-decisions)).
