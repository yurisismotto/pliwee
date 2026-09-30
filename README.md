<p align="center">
  <img src="docs/design/assets/pliwee-mark.svg" alt="Pliwee logo" width="120">
</p>

<h1 align="center">Pliwee</h1>

<p align="center"><strong>One flow. Any device.</strong></p>

<p align="center">
  Send files, share your clipboard, mirror notifications and see your phone's
  battery — between your Android phone and your Linux desktop, directly over
  your own network. No cloud, no account, no telemetry.
</p>

<p align="center">
  <a href="https://github.com/yurisismotto/pliwee/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/yurisismotto/pliwee?label=release"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
  <img alt="Platforms: Linux and Android" src="https://img.shields.io/badge/platforms-Linux%20%7C%20Android-4F6BFF">
</p>

<p align="center">
  <a href="#install-pliwee">Install</a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#what-pliwee-does">Features</a> ·
  <a href="#known-limitations">Known limitations</a> ·
  <a href="#verifying-a-release-download">Verify downloads</a> ·
  <a href="#documentation">Documentation</a>
</p>

> **Pliwee was called OmniBridge** until its v1.0.0 Linux release. An existing
> OmniBridge 1.0.0 install upgrades in place with your distribution's normal
> upgrade command, through the transitional `omnibridge` packages, and keeps its
> identity, pairings, grants, policies and the desktop app's device choice. See
> [Upgrading from OmniBridge](packaging/common/README.md#upgrading-from-omnibridge)
> and [ADR-0020](docs/adr/ADR-0020-rename-to-pliwee.md).

## Install Pliwee

Pliwee for Linux is distributed as packages attached to its
**[GitHub Releases](https://github.com/yurisismotto/pliwee/releases/latest)**.
There is no apt or dnf repository yet: you download the two packages for your
distribution and install them with your package manager.

Every release has two packages:

* **`pliwee`** — the background service (`pliweed`) and the
  `pliwee` command-line tool;
* **`pliwee-gui`** — the Pliwee desktop application. It requires the
  exact same version of `pliwee`, so install both together.

The commands below are for **Pliwee v1.1.0**, the current release, on
x86_64. Want to check the files before installing them? See
[Verifying a release download](#verifying-a-release-download).

**Upgrading from OmniBridge 1.0.0?** Also download the transitional packages
from the same release — `omnibridge-1.1.0-1.fc44.noarch.rpm` on Fedora;
`<distro>-omnibridge_1.1.0-1_all.deb` and `<distro>-omnibridge-gui_1.1.0-1_all.deb`
on Ubuntu and Debian — and install them in the same `dnf`/`apt` command as the
two packages below.

### Fedora

The RPMs are built on Fedora 44.

```bash
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/pliwee-1.1.0-1.fc44.x86_64.rpm
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/pliwee-gui-1.1.0-1.fc44.x86_64.rpm
sudo dnf install ./pliwee-1.1.0-1.fc44.x86_64.rpm ./pliwee-gui-1.1.0-1.fc44.x86_64.rpm
```

### Ubuntu 24.04 LTS

```bash
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/ubuntu2404-pliwee_1.1.0-1_amd64.deb
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/ubuntu2404-pliwee-gui_1.1.0-1_amd64.deb
sudo apt install ./ubuntu2404-pliwee_1.1.0-1_amd64.deb ./ubuntu2404-pliwee-gui_1.1.0-1_amd64.deb
```

### Ubuntu 26.04 LTS

```bash
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/ubuntu2604-pliwee_1.1.0-1_amd64.deb
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/ubuntu2604-pliwee-gui_1.1.0-1_amd64.deb
sudo apt install ./ubuntu2604-pliwee_1.1.0-1_amd64.deb ./ubuntu2604-pliwee-gui_1.1.0-1_amd64.deb
```

### Debian 13 (trixie)

```bash
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/debian13-pliwee_1.1.0-1_amd64.deb
curl -fLO https://github.com/yurisismotto/pliwee/releases/download/v1.1.0/debian13-pliwee-gui_1.1.0-1_amd64.deb
sudo apt install ./debian13-pliwee_1.1.0-1_amd64.deb ./debian13-pliwee-gui_1.1.0-1_amd64.deb
```

The Ubuntu and Debian packages share the same file names inside the
distribution, so the release prefixes each one with its target
(`ubuntu2404-`, `ubuntu2604-`, `debian13-`). Use the files for **your**
distribution — they are built separately for each one.

> **Firewall.** Fedora Workstation, Ubuntu and Debian need nothing by default.
> If you have turned on a firewall yourself, Pliwee needs TCP port 55432
> and mDNS: see [Fedora](packaging/fedora/README.md#firewall) or, with `ufw`,
> `sudo ufw allow 55432/tcp` and `sudo ufw allow mdns`.

### Android companion app

**You need the Pliwee Android app on your phone** — every feature works
between a paired phone and computer.

**The Android app is not on Google Play yet**, and the v1.1.0 release does not
include an Android APK. For now it has to be built from source; see
[Running on Android](#running-on-android). Publishing it through Google Play is
the next step, and this is where the link will appear.

## Getting started

1. **Install Pliwee on Linux** — see [Install Pliwee](#install-pliwee).
2. **Start the Pliwee service.** The packages install it switched off, so
   nothing listens on your network until you choose. Turn it on for your user
   (no root needed); it will then start on its own every time you log in:

   ```bash
   systemctl --user enable --now pliweed.service
   ```

3. **Install and open the Android app** on your phone — see
   [Android companion app](#android-companion-app).
4. **Pair the two devices.** Open **Pliwee** from your applications menu
   and choose **Pair device**, or run `pliwee pair` in a terminal. A QR
   code appears; in the Android app, tap **Pair device** and scan it. The
   computer then shows the phone's fingerprint — **check it matches the
   phone's screen**, and accept.
   See [Pairing](#pairing) for what happens underneath.
5. **Grant only what you want.** A newly paired device can do nothing yet.
   Allow files, clipboard or battery for that phone on the desktop app's
   **Devices** page (open the phone's **Details and controls**), or with
   `pliwee grant <device> <capability>`.
   Notification mirroring needs both ends: on the phone, give Pliwee
   Android's notification access, turn sharing on for this computer and pick
   the apps; on the desktop, switch on **Receive notifications from this
   device** for that phone on the **Notifications** page.
6. **Use it.** Share a file to Pliwee from any Android app, send files from
   the desktop, send your clipboard, and watch notifications and battery level
   arrive. `pliwee status` shows what is running and connected.

## What Pliwee does

| | What you get |
| --- | --- |
| **Files** | Send a file from your phone with Android's **Share** menu, or from your computer. Received files land in your Downloads folder, under `Pliwee`, and an existing file is never overwritten. |
| **Clipboard** | Copy on your computer, paste on your phone — automatically, if you turn that on for a device. Going the other way is a deliberate tap: **Send clipboard** in the app, the Quick Settings tile, or sharing text to Pliwee. |
| **Notifications** | Mirror your phone's notifications to your desktop and dismiss them from either side. Off until you turn it on, and then only for the apps you pick. |
| **Battery and device information** | See your phone's battery level on the desktop. |
| **Secure local pairing** | Pair by scanning a QR code and confirming a fingerprint. Devices talk only to each other, over your local network, encrypted end to end. |

**About the clipboard, precisely.** Desktop → Android can be automatic (opt-in,
per device); Android → desktop is always a manual action. That is not a design
shortcut: Android 10 and later refuse to let a background app read the
clipboard, and Pliwee uses none of the tricks that get around that. It is
**not** an unrestricted, automatic two-way clipboard, and it does not claim to
be. Details: [docs/architecture/CLIPBOARD.md](docs/architecture/CLIPBOARD.md).

**Nothing is shared until you say so.** Discovering a device on the network
grants it nothing. Pairing is confirmed by a human on both ends, and every
feature above is a separate permission you give one device at a time and can
take back at any moment.

**Not yet:** media control and browser integration are not implemented. The
architecture is built to receive them, and that is all.

For developers, each feature is a versioned protocol capability: `files.v1`,
`clipboard.v1`, `notifications.v1` and `battery.v1` — see
[Capabilities in detail](#capabilities-in-detail).

## Supported platforms

| Platform | Status | What that means |
| --- | --- | --- |
| **Fedora 41+** (developed and certified on 44) | **Runtime certified** | Every capability has been exercised on real hardware in a real GNOME Wayland session, through six certification waves |
| **Ubuntu 24.04 LTS** | **Runtime certified** | The packages CI publishes, installed on a real desktop with a real GNOME Wayland session, and paired with a physical Android phone over the LAN |
| **Ubuntu 26.04 LTS** | **Runtime certified** | same |
| **Debian 13 trixie** | **Runtime certified**, with one documented exception | same, except that manual clipboard **sending** cannot work on its compositor — see [Known limitations](#known-limitations) |
| **Android** | Companion app | Built from source for now; not yet on Google Play |

The release RPMs are built on Fedora 44; on an older Fedora, build from source.
**Ubuntu 22.04 LTS and Debian 12 bookworm cannot run the desktop application**
and are not targets: their libadwaita (1.2) and GTK (4.8) predate the APIs the
interface is built from. The service and command-line tool need no GTK at all.

What "runtime certified" covers, and what it does not, is in
[Certification](#certification).

## Security and privacy

* **Local-first.** Your devices talk directly to each other over your local
  network. There is no Pliwee server, no account and no telemetry.
* **Encrypted and authenticated.** Every connection uses TLS 1.3 with mutual
  authentication; each device is identified by its own public key, pinned at
  pairing time.
* **Paired on purpose.** Pairing needs a QR code shown on your computer and a
  fingerprint you confirm by eye.
* **One permission at a time.** Each feature is granted separately, per
  device, and can be revoked immediately.
* **Your content stays out of logs.** Clipboard content is never written to
  disk and never logged; notification and file contents are not logged either.
* **No special powers.** No root, no accessibility service, no ADB.

The full model is in [Security](#security) below and in
[docs/security/THREAT_MODEL.md](docs/security/THREAT_MODEL.md).

## Known limitations

* **Widening a capability grant takes effect on the next connection.** A
  session's effective capability set is fixed at handshake time, so after
  `pliwee grant … files.v1` the phone must reconnect. *Narrowing* is
  immediate, including against a transfer already running — the asymmetry
  fails in the safe direction, but it is a rough edge.
* **No resume.** A transfer interrupted by a disconnect fails and its partial
  file is deleted. The receiver already knows the expected size and digest, so
  resume is tractable, but it needs durable partial state that this version
  deliberately does not keep.
* **One file per share.** `ACTION_SEND_MULTIPLE` is registered so Pliwee
  appears for multi-select, but only the first item is sent.
* The trust store's persistence path is not covered by the local JVM unit
  tests: it needs a real `Context` and `filesDir`. Its pure logic is tested,
  and `ClipboardPersistenceTest` now covers the file I/O on a device.
* **Automatic Android → desktop clipboard is not implemented, and will not be.**
  Android 10+ refuses clipboard reads to an app without input focus, and every
  way around it is forbidden or user-hostile. Android → desktop is a deliberate
  action: the Send clipboard button, the Quick Settings tile, or sharing text
  to Pliwee. Verified on an SM-X620 (Android 16): background read REFUSED,
  focused read ALLOWED, background `setPrimaryClip` APPLIED.
* **Sending the desktop clipboard needs a session that can read it, and some
  cannot.** Reading a selection this process does not own requires either the
  `wlr`/`ext-data-control` protocol or a reachable Xwayland. GNOME implements
  neither protocol, so it depends on the Xwayland fallback — which is present
  on Ubuntu 24.04 and 26.04, where sending works, and **not usable on Debian 13
  trixie**, where it does not. Both automatic *and* manual sending are affected,
  because both read the selection the same way. **Receiving a clipboard from the
  phone is unaffected** on every distribution: writing a clip needs no such
  protocol. `pliwee clipboard status` reports `auto-send`, `manual send` and
  `receiving` separately, so the answer for your session is printed rather than
  guessed.
* **The desktop clipboard also needs an unlocked session.** On GNOME Wayland,
  `wl-copy` and `wl-paste` block behind the lock screen rather than failing.
  Every call is bounded by a timeout and reported as such, so nothing hangs —
  but clipboard sync does not work while the screen is locked. A lock is one
  cause of that timeout and the bullet above is another; the error names both
  rather than assuming.
* **Sensitive clips are refused where `wl-copy` cannot mark them.** A clip the
  phone marks as a password or other secret is written with `wl-copy
  --sensitive`, which tells clipboard managers to keep it out of their
  history. That option arrived in wl-clipboard 2.3.0, and Ubuntu 24.04,
  Ubuntu 26.04 and Debian 13 all ship 2.2.1 — so on those three, **Pliwee
  refuses such a clip rather than writing it unmarked**, because an unmarked
  password silently persisted in a history file is the worse outcome. Ordinary
  clipboard sharing is unaffected. `pliwee clipboard status` and the GUI's
  clipboard page both say so up front rather than at the moment a password
  fails to arrive. Pliwee decides this by asking `wl-copy --help` for the
  option, never by reading its version — Fedora's `2.2.1^git…` has the flag
  and Debian's `2.2.1` does not, with the same version string.
* **Clipboard auto-send needs a compositor that can report clipboard changes.**
  GNOME implements neither wlr- nor ext-data-control, so Pliwee watches via
  XFIXES on the Xwayland `CLIPBOARD` selection instead (ADR-0014). Without a
  reachable Xwayland there is no watcher — and, as the bullet above says,
  **no manual send either**, because both read the selection the same way.
  This bullet used to claim auto-send "degrades to manual sending"; it does
  not, and `pliwee clipboard status` now reports the two separately
  instead of inferring one from the other.
* **Sending a file or a clipboard from the phone to the desktop is not yet
  certified end to end on hardware**, on any distribution, because driving it
  needs a human at the phone's document picker. The code paths are exercised by
  the in-process suite; they have not been measured on hardware.
* The desktop private key is protected by filesystem permissions, not by
  hardware. TPM2 sealing is the top security debt
  ([ADR-0006](docs/adr/ADR-0006-device-identity-and-pairing.md)).
* mDNS advertises a stable device id and name, which is a modest tracking
  signal on untrusted networks (threat model, T18). `--no-mdns` is the blunt
  workaround; a per-network toggle is the proper fix.

## Verifying a release download

Every Pliwee release ships a `SHA256SUMS` covering all its artifacts,
and a detached OpenPGP signature over that manifest. Checking the signature
first and the digests second is the only order that helps: checking digests
first is checking a download against itself.

**The key to trust.** One fingerprint, and it does not change when the
maintainer's email does:

```
Pliwee Release Signing Key
primary  F545DC184E909192C3FB6F6E64963019E731BE07
```

The primary is **certify-only**; releases are signed by its `sign`-only subkey
`E8EDE4706F067739A8D3A8B74C48CB81694FD134`, which expires 2028-09-22. Verify
against the **primary** fingerprint above — that is the long-term anchor, and
the verifier resolves the subkey for you.

This is the same key that signed OmniBridge v1.0.0; it still carries its
`OmniBridge Release Signing Key` identity alongside the Pliwee one.

**Where the key is.** The armoured public key is published as a **release
asset**, `pliwee-release-pubkey.asc`, attached to every release. It is
deliberately **not** kept in this repository: a key checked into the tree it
signs adds nothing a release asset does not already give you, and it invites
the mistake of trusting a key because it sits next to the code.

```
https://github.com/yurisismotto/pliwee/releases/latest/download/pliwee-release-pubkey.asc
```

That URL always resolves to the newest release's copy; a specific release's
copy is on its own page. Downloading the key is **not** what makes it
trustworthy — a key fetched over HTTPS is still a key somebody could have
replaced. The fingerprint printed above is what makes the check mean
something, so compare it every time.

**What to verify.** The release directory is the bundle
`pliwee-<version>-linux-x86_64.tar.gz` from the release page, extracted: it
holds every artifact in the layout `SHA256SUMS` describes, with `SHA256SUMS`
and `SHA256SUMS.asc` beside them. `verify-release.sh` is in this repository and
in the source tarball.

```bash
# 1. import the published public key into a keyring of its own
curl -fsSLO https://github.com/yurisismotto/pliwee/releases/latest/download/pliwee-release-pubkey.asc
gpg --homedir ./ob-verify --import pliwee-release-pubkey.asc
gpg --homedir ./ob-verify --export > ob-release.gpg

# and check it is the identity above before trusting it
gpg --homedir ./ob-verify --fingerprint F545DC184E909192C3FB6F6E64963019E731BE07

# 2. check the signature, then the files
./packaging/release/verify-release.sh \
    --dir <the downloaded release directory> \
    --keyring ob-release.gpg \
    --fingerprint F545DC184E909192C3FB6F6E64963019E731BE07
```

A run that succeeds prints `VERIFIED` and the number of files checked. Anything
else is a failure — in particular a **missing** `SHA256SUMS.asc` is a failure,
not a skip, because anyone who can substitute an artifact can also delete the
signature. `--allow-unsigned` exists, says exactly what it is not checking, and
reports its result as `CHECKED (UNSIGNED)`.

The OmniBridge v1.0.0 downloads stay where they were published,
[`yurisismotto/OmniBridge` v1.0.0](https://github.com/yurisismotto/OmniBridge/releases/tag/v1.0.0),
and verify the same way against the same fingerprint.

The releases also carry SLSA build provenance, which is a different claim and
not a substitute:

```bash
gh attestation verify <artifact> -R yurisismotto/pliwee
```

Provenance answers *"was this built by Pliwee's CI, from which commit?"*.
The signature answers *"does the maintainer stand behind this release?"*. A
green provenance check is not a maintainer signature.

The evidence behind this identity — custody, the signed set, independent
verification and the negative tests — is in
[docs/certification/release/RELEASE-SIGNING-CLOSURE-V1.md](docs/certification/release/RELEASE-SIGNING-CLOSURE-V1.md).

---

*Everything below is for developers and for anyone who wants the detail.*

## Build from source

### Prerequisites

Three things, and only the third is distribution-specific in any interesting
way:

* **A C compiler.** `ring` compiles C and assembly for the TLS primitives.
  Nothing else in the tree needs one — the daemon and the CLI link no C
  library at all.
* **A Rust toolchain, 1.88 or newer.** See the table below; this is the one
  place where a distribution's own package may not be enough.
* **GTK 4.12+ and libadwaita 1.5+**, for the GUI only. Every supported
  distribution ships enough (Ubuntu 24.04 is exactly at the libadwaita floor).

It does **not** need `protobuf-compiler` — the schema is compiled by `protox`,
in pure Rust ([ADR-0004](docs/adr/ADR-0004-protocol-buffers.md)) — and it does
**not** need OpenSSL, Avahi, libdbus or libX11 development packages. Transport
security is `rustls`/`ring`, D-Bus is `zbus` (a pure-Rust implementation),
mDNS is `mdns-sd` (its own responder, not an Avahi client) and the Xwayland
clipboard watch is `x11rb` with its own connection backend.

**Fedora**

```bash
sudo dnf install gcc pkgconf-pkg-config rust cargo
sudo dnf install gtk4-devel libadwaita-devel glib2-devel   # GUI only
sudo dnf install wl-clipboard                              # clipboard, at runtime
sudo dnf install upower                                    # battery.v1, optional
```

**Ubuntu 24.04 / 26.04 and Debian 13**

```bash
sudo apt install gcc libc6-dev pkg-config
sudo apt install libgtk-4-dev libadwaita-1-dev   # GUI only; this also brings
                                                 # in glib-compile-resources
sudo apt install wl-clipboard                    # clipboard, at runtime
sudo apt install upower                          # battery.v1, optional
```

The package names differ; the runtime binary Pliwee actually looks for is
called `wl-copy` on all of them, and the package carrying it is called
`wl-clipboard` on all of them.

### The Rust toolchain, per distribution

**Pliwee requires Rust ≥ 1.88.** That number is not a preference: the
committed `Cargo.lock` contains crates (`time`, `rcgen`, `zbus`) that declare
it, so an older toolchain fails in Cargo's resolver before compiling a line of
Pliwee. The distribution's own `rustc` package is **not** required — it is
simply the most convenient source when it is new enough.

| Distribution | Its default `rustc` | Enough? | What to use |
| --- | --- | --- | --- |
| Fedora 44 | 1.98 | **yes** | `dnf install rust cargo` |
| Ubuntu 26.04 LTS | 1.93.1 | **yes** | `apt install rustc cargo` |
| Ubuntu 24.04 LTS | 1.75 | **no** | a newer versioned toolchain from Ubuntu's own archive — `apt install rustc-1.91 cargo-1.91` — or [rustup](https://rustup.rs). Note that `rustc-1.82` is also in the archive and is **not** enough |
| Debian 13 trixie | 1.85.1 | **no** | `trixie-backports` (`rustc` 1.94.1), or [rustup](https://rustup.rs) |

### Build and run

```bash
cd desktop
cargo build --release
cargo test --workspace          # the full workspace test suite

./target/release/pliweed       # foreground, or install the user unit
```

As a service. The unit is a **user** unit — the identity key lives 0600 in
your `$XDG_DATA_HOME` and the control socket in your `$XDG_RUNTIME_DIR`, so
nothing here wants root. It is distribution-neutral and lives in
`packaging/common/`, which is the one copy every package format installs:

```bash
install -Dm0644 packaging/common/pliweed.service \
    ~/.config/systemd/user/pliweed.service
systemctl --user enable --now pliweed.service
```

Building the packages themselves is described in
[packaging/fedora/README.md](packaging/fedora/README.md) and
[packaging/debian/README.md](packaging/debian/README.md).

### Running on Android

Needs JDK 21 and Android SDK platform 35. See
[android/README.md](android/README.md). Build with
`cd android && ./gradlew :app:assembleDebug`.

## Command-line reference

```bash
pliwee status              # identity, port, capabilities, live connections
pliwee pair                # opens a pairing window and prints a QR code
pliwee devices             # paired devices
pliwee ping <device>       # round-trip over the live session
pliwee unpair <device>     # revoke; takes effect immediately
```

File transfer is a separately granted capability and is **never** granted
automatically — writing a file to your disk is a side effect
([ADR-0008](docs/adr/ADR-0008-capability-architecture.md)):

```bash
pliwee grant <device> files.v1     # allow file transfer with this device
pliwee send <device> ~/photo.jpg   # offer a file; streams progress
pliwee transfers                   # everything since the daemon started
pliwee cancel <transfer-prefix>    # stop one mid-flight
pliwee revoke <device> files.v1    # withdraw; stops transfers already running
```

Received files land in `<XDG downloads>/Pliwee`. An existing name is never
overwritten — `photo.jpg` becomes `photo (1).jpg`. See
[docs/architecture/FILES.md](docs/architecture/FILES.md).

Clipboard sharing is likewise never granted automatically — a device that can
write your clipboard can also see what you paste next:

```bash
pliwee grant <device> clipboard.v1        # allow clipboard sharing
pliwee clipboard status                   # what works here, and per-device policy
pliwee clipboard send <device>            # send the current clipboard, now
pliwee clipboard send <device> --sensitive  # ask the phone to mark it sensitive
pliwee clipboard apply <device>           # apply a clip that is waiting
pliwee clipboard auto-send <device> on    # push every local copy to that device
pliwee clipboard auto-receive <device> on # apply its clips as they arrive
```

Granting is one decision; automation is another. A freshly granted device can
send and receive **by hand**, and both automatic directions start **off** —
`auto-send` means everything you copy leaves this machine, and `auto-receive`
means that device can replace what you are about to paste. Clipboard content
is never written to disk and never logged, at any level. See
[docs/architecture/CLIPBOARD.md](docs/architecture/CLIPBOARD.md).

The daemon has no terminal, so it cannot prompt: it **declines** incoming
files and logs why. `pliweed --accept-files-without-asking` is the documented
escape hatch for an unattended test rig.

`<device>` is a device id or a fingerprint prefix of at least 8 characters. An
ambiguous prefix is an error, never a guess.

The daemon never needs root.

## Pairing

1. On the computer: `pliwee pair`. A QR code appears; it is valid for 120
   seconds
   and works once.
2. On the phone: **Pair device**, then scan the code.
3. The phone pins the computer's key *from the QR*, before opening a socket —
   so the first connection is already authenticated and there is no
   man-in-the-middle window.
4. The phone proves it holds the pairing code, bound to both identities and to
   a fresh nonce.
5. The computer shows the phone's fingerprint. **Check it matches the phone's
   screen**, then accept.
6. Both sides store the other's public key. The token is destroyed.

Afterwards the phone reconnects on its own using the stored identities. A
network change does not require re-pairing.

## Capabilities in detail

The certified foundation — identity, discovery, pairing, authenticated
transport, ping/pong — carries four capabilities: `battery.v1`, `files.v1`,
`clipboard.v1` and `notifications.v1`. **Media control and browser
integration are not implemented**; the architecture is built to receive them,
and that is all.

**`notifications.v1` is implemented and certified.** It is an optional,
off-by-default Android notification mirror, requiring both the Android OS
notification-access grant *and* a separate per-peer grant, with no history,
no cloud and no telemetry. The decision is
[ADR-0015](docs/adr/ADR-0015-notification-access.md), with
[ADR-0016](docs/adr/ADR-0016-notification-identity.md) and
[ADR-0017](docs/adr/ADR-0017-capability-roles.md); it is specified in
[docs/research/notifications-v1/](docs/research/notifications-v1/) and
certified in
[NOTIFICATIONS-V1-N6-FINAL-CERTIFICATION.md](docs/certification/notifications/NOTIFICATIONS-V1-N6-FINAL-CERTIFICATION.md).

Clipboard sharing is, precisely: **automatic desktop → Android sync**
(opt-in, per device) and **manual Android → desktop send**. It is not
"automatic bidirectional clipboard", and saying so would be wrong: Android
10+ refuses clipboard reads to an app without input focus, and Pliwee uses
none of the techniques that defeat that. See
[docs/architecture/CLIPBOARD.md](docs/architecture/CLIPBOARD.md).

## Principles

1. Local-first. The LAN is the only transport.
2. No cloud service, no account, no telemetry.
3. Nothing in plaintext. TLS 1.3 only.
4. Identity is a public key — never an IP, hostname or MAC address.
5. Discovery is not trust. Reachability grants nothing.
6. Explicit pairing, confirmed by a human, with key pinning afterwards.
7. Every feature is a separately granted capability.
8. No root, no accessibility service, no ADB, no hidden permissions. A
   privileged Android capability is acquired only for a named, user-visible
   feature, through the platform's own API for it, with separately revocable
   consent — and never to defeat a restriction that protects the user
   ([ADR-0015](docs/adr/ADR-0015-notification-access.md)).
9. Logs never contain user content.

## Security

Read [docs/security/THREAT_MODEL.md](docs/security/THREAT_MODEL.md).

Short version: identity is a hardware-backed P-256 key (Android Keystore /
StrongBox on the phone; a 0600 file the daemon refuses to start without on the
desktop). Transport is TLS 1.3 with mutual authentication and SPKI pinning.
Pairing uses a 160-bit single-use token proved via HMAC bound to both
identities and a server nonce. Replay is stopped by strictly increasing
sequence numbers and message-id de-duplication — never by timestamps, because
clocks disagree.

There is no build flag, debug variant or test helper anywhere in this
repository that disables certificate validation.

## Certification

The three Debian-family targets moved from "build-supported" to "runtime
certified" in
[lifecycle closure](docs/certification/linux/RELEASE-LIFECYCLE-CLOSURE-V1.md)
and [peer-gate closure](docs/certification/linux/RELEASE-PEER-GATES-CLOSURE-V1.md).
**23 of 26 lifecycle gates are certified on all three** — clean install, the
installed file manifest, autostart, the launcher, D-Bus activation into a live
session, the tray, mDNS, the listening port, restart, logout/login, reboot,
remove, reinstall, purge and the trust store surviving all of it — plus
Android discovery, file transfer and notification mirroring against a physical
SM-X620 on the same LAN. Two gates are N/A with a stated reason (there is one
published build per target, so there is nothing to upgrade *from*), and one is
partial on Debian 13 alone.

The first gate that ran found a defect that made the packages **unusable** on
both Ubuntu releases — `omnibridged.service` could not start at all — and it
was fixed before certification continued. That is the argument for doing this
rather than shipping on a green build matrix.

What is still **not** claimed, on any distribution: sending a file or a
clipboard **from** the phone to the desktop is untested end to end, because
driving it needs a human at the phone's document picker. The code paths are
exercised by the in-process suite; they have not been measured on hardware.

Every certification, audit and report is indexed under
[docs/](docs/README.md).

## Testing

```bash
cd desktop && cargo test --workspace              # the full workspace test suite
cd android && ./gradlew :app:testDebugUnitTest    # 771 tests

# Touches the real system clipboard, so it is opt-in:
cd desktop && cargo test -p pliwee-capability-clipboard --test real_backend \
    -- --ignored --test-threads=1                 # 9 tests

# On a connected Android device:
cd android && ./gradlew :app:connectedDebugAndroidTest   # 102 tests
```

The Rust suite includes end-to-end pairing over real TLS on loopback and a
hostile-client suite that replays envelopes, duplicates message ids, rewinds
sequence numbers, claims another device's fingerprint and skips the handshake.

The Android suite covers the same wire rules on the Kotlin side, and checks
pinning against real certificates emitted by the desktop implementation
(`protocol/testdata/`).

Three known-answer vectors are asserted by **both** suites, so the
implementations cannot drift apart silently:

* the pairing proof and confirmation HMACs,
* the SPKI fingerprints of the shared certificate fixtures, and
* the `clipboard.v1` content hash — SHA-256 over the UTF-8 bytes — along with
  the text rules around it, since a clip one platform sends and the other
  refuses is a bug rather than a policy.

## Repository layout

```
pliwee/
├── protocol/proto/            Wire format — compiled by BOTH implementations
├── desktop/                   Rust workspace
│   ├── proto/                 Generated protobuf types
│   ├── core/                  Identity, pairing, TLS, framing, session
│   ├── capabilities/battery/  battery.v1
│   ├── capabilities/files/    files.v1 — transfers, filename safety, stream auth
│   ├── capabilities/clipboard/ clipboard.v1 — text rules, policy, loop suppression
│   ├── capabilities/notifications/ notifications.v1 — mirror, roles, redaction
│   ├── daemon/                pliweed
│   ├── cli/                   pliwee
│   └── gui/                   pliwee-gui — GTK4 / libadwaita
├── android/                   Kotlin + Compose app
├── browser-extension/         (placeholder)
├── packaging/common/          the systemd user unit and the cargo vendor config
├── packaging/fedora/          RPM spec, firewalld service
├── packaging/debian/          debhelper packaging for Debian and Ubuntu
└── docs/                      see docs/README.md for the full taxonomy
    ├── adr/                   ADR-0001 … ADR-0020
    ├── architecture/          OVERVIEW.md, PROTOCOL.md, FILES.md, CLIPBOARD.md, NOTIFICATIONS.md
    ├── design/                BRAND.md, UI-GUIDELINES.md, tokens.json, assets/
    ├── security/              THREAT_MODEL.md
    ├── research/              cross-platform expansion, notifications.v1
    ├── audits/                readiness and gap analyses, by area
    ├── certification/         PASS/FAIL gates and their evidence, by area
    ├── reports/               sprint and hardening reports, by area
    └── migrations/            AnyFlow → OmniBridge (and, when published, OmniBridge → Pliwee)
```

Root Markdown is limited to this file and
[AGENTS.md](AGENTS.md); every other document lives under
[docs/](docs/README.md), which explains where a new one belongs.

## Documentation

* [docs/README.md](docs/README.md) — the map of all project documentation
* [docs/architecture/OVERVIEW.md](docs/architecture/OVERVIEW.md) — how the
  system fits together, with
  [PROTOCOL](docs/architecture/PROTOCOL.md),
  [FILES](docs/architecture/FILES.md),
  [CLIPBOARD](docs/architecture/CLIPBOARD.md) and
  [NOTIFICATIONS](docs/architecture/NOTIFICATIONS.md)
* [docs/security/THREAT_MODEL.md](docs/security/THREAT_MODEL.md) — the threat
  model
* [docs/adr/](docs/adr/) — architecture decision records
* [docs/design/BRAND.md](docs/design/BRAND.md) — the Pliwee visual identity
* [android/README.md](android/README.md) — the Android app

## Project history

This project was called **AnyFlow** (*One flow. Any device.*) until it was
renamed to **OmniBridge** before the public v1.0.0 release. The rename went all
the way down — binaries, application ids, ALPN, mDNS, the QR prefix, the
protobuf namespace and the pairing-proof domain — with no compatibility
aliases, because there was no released version to stay compatible with. **An
existing AnyFlow build cannot talk to an OmniBridge build and a development
pairing must be redone once.** See
[ADR-0018](docs/adr/ADR-0018-rename-to-omnibridge.md) for the decision and
[the migration note](docs/migrations/MIGRATION-ANYFLOW-TO-OMNIBRIDGE.md) for
what to do about an existing checkout or test device.

The repository was then renamed too, and lived at
`github.com/yurisismotto/OmniBridge`. Its history was recreated on 2026-09-24
from a byte-identical tree; the earlier history, and the commits, pull
requests and CI runs that older documents cite, are in
[`yurisismotto/omnibridge-history`](https://github.com/yurisismotto/omnibridge-history).

The certification reports in this repository were written under the old
name and keep their original wording; they are evidence, not documentation.
Their AnyFlow naming — and the old repository URLs in the CI run and issue
links they cite — is preserved deliberately.

On 2026-09-30 the project continued as **Pliwee**, in the new repository
[`yurisismotto/pliwee`](https://github.com/yurisismotto/pliwee)
([ADR-0020](docs/adr/ADR-0020-rename-to-pliwee.md)).
[`yurisismotto/OmniBridge`](https://github.com/yurisismotto/OmniBridge) is kept
read-only as history — the v1.0.0 release, its assets and its signature stay
there — and `yurisismotto/omnibridge-history` is left untouched.

## Contributing

Issues and pull requests are welcome on
[GitHub](https://github.com/yurisismotto/pliwee). Before adding a document,
read [docs/README.md](docs/README.md) for where it belongs;
[AGENTS.md](AGENTS.md) holds the repository's working agreements, including the
rules for test gates and historical evidence.

## License

Apache-2.0. See [LICENSE](LICENSE).
