# Distribution-neutral packaging

One directory, one file today: `pliweed.service`. The packages also install
`omnibridged.service`, the OmniBridge 1.0.0 name, as a symlink to it; that is
not a second unit, see [Upgrading from OmniBridge](#upgrading-from-omnibridge).

## Why it is not under `packaging/fedora/`

It used to be, and that was a real problem rather than an untidy one. The unit
is a security artefact — it is where `ProtectSystem=strict`, the syscall filter
and the address-family restriction live. A second copy under
`packaging/debian/` would have been the obvious way to package Debian, and the
day someone tightened one of those directives it would have landed on one
distribution and quietly missed the other.

So there is one file. The RPM's `%install` copies it into `%{_userunitdir}`.
The Debian packaging installs the same path. Neither owns it.

## What the unit does

It runs `pliweed` as a **user** service. Not a system service, and the
distinction is load-bearing:

* the identity key is `0600` inside a `0700` directory in the user's
  `$XDG_DATA_HOME`, and the trust store beside it says which phones this
  *person* has paired;
* the control socket is `0600` in the user's `$XDG_RUNTIME_DIR`;
* the daemon binds TCP 55432 — above 1024, so no capability is needed.

Running it as root would gain nothing and would put a network-facing parser in
a place where a bug is worth far more to an attacker. Nothing in this
repository installs a system unit, and nothing should.

## The three directives that carry the most weight

```ini
RuntimeDirectory=pliwee
RuntimeDirectoryMode=0700
ReadWritePaths=%h/.local/share
```

Each of them is there because of something that was measured, not assumed.
The measurements are in
[`../../docs/audits/packaging/PACKAGING-V1-SYSTEMD-UNIT.md`](../../docs/audits/packaging/PACKAGING-V1-SYSTEMD-UNIT.md)
and, before it, §4.2 of the readiness audit.

**`RuntimeDirectory=`** — `ProtectSystem=strict` *is* effective in a systemd
user unit on Fedora 44 / systemd 259; the general caveat in `systemd.exec(5)`
about namespacing and `PrivateUsers=` does not get the daemon off the hook.
Without this directive the daemon cannot `mkdir` in `$XDG_RUNTIME_DIR`, the
control socket has nowhere to live, and `pliwee status`, the GUI and the
tray all have nothing to connect to. `0700` is the same mode the daemon
applies itself when it is run outside systemd, so the two paths agree.

**`ReadWritePaths=%h/.local/share`** — the *parent*, deliberately, not
`~/.local/share/pliwee`. Two separate measured reasons:

1. `ReadWritePaths=` without a `-` prefix refuses to start a unit whose path
   does not exist — which is every fresh install; and
2. adding the `-` prefix fixes that and then does **not** punch the hole
   either, so the daemon's first run cannot create its own data directory
   inside a read-only `$HOME`.

Granting the parent is one directory wider than ideal. It stays entirely
inside `ProtectHome=read-only`, it is bounded and reviewable, and it is
**not** a weakening of `ProtectSystem=strict`, which is untouched.

**No `StateDirectory=`** — in a *user* unit `StateDirectory=` maps to
`$XDG_STATE_HOME`, i.e. `~/.local/state`, which the daemon never opens. The
old unit had it. It created an empty directory nobody read and did nothing for
the one that mattered.

## What the unit must never do

The data directory is **not package-owned and not unit-owned**. No
`StateDirectory=`, no `ExecStartPre=` that creates it, nothing in any
maintainer script that touches it. It holds the user's pairings; install,
upgrade, remove and purge all leave it exactly as they found it.
`packaging/tests/packaging-checks.sh` asserts both halves of that.

## Re-measuring it

```bash
./packaging/tests/packaging-checks.sh          # static: the directives are still there
./packaging/tests/systemd-unit-gates.sh        # live: gates S1, S2, S3 on this machine
```

The second one starts the real unit against a throwaway `XDG_DATA_HOME`,
measures the runtime directory and socket modes, reads the journal for sandbox
denials, and fails if the real trust store changed by so much as a mode bit.
It needs no root and refuses to run as root. Audit §14.3 runs it again on
Ubuntu 24.04, Ubuntu 26.04 and Debian 13.

## Enabling it

Packages ship it **disabled**, on every format. A global enable would turn on
a LAN listener for every account on the machine, and the daemon does nothing
until an identity exists and a device is paired, so autostart-before-pairing
buys the user nothing.

```bash
systemctl --user enable --now pliweed.service
loginctl enable-linger $USER    # optional: keep it running while logged out
```

Lingering is deliberately opt-in. Leaving a network service running after
logout should be something the user chooses.

**An upgrade does not restart a running daemon.** A root scriptlet has no
route to a user's service manager. This is inherent to user units, not a
packaging defect; the new binary is in place and takes effect at the next
`systemctl --user restart pliweed` or the next login.

## Where received files go

Files a paired device sends land in `<XDG downloads>/Pliwee`. The unit grants
`~/Downloads` (`ReadWritePaths=-%h/Downloads`) and nothing else in `$HOME`
beyond `~/.local/share`. If your download directory is elsewhere — a
localised name such as `~/Téléchargements`, `XDG_DOWNLOAD_DIR`, or
`pliweed --download-dir` — grant its parent once, and the daemon's start-up
warning prints the exact line for your directory:

```bash
systemctl --user edit pliweed.service
#   [Service]
#   ReadWritePaths=-/home/you/Téléchargements
systemctl --user restart pliweed.service
```

Until 2026-09-30 the unit did not grant `~/Downloads` either, and every
incoming file failed with *"could not store the file"* — OmniBridge 1.0.0
included.

## Upgrading from OmniBridge

Pliwee 1.1.0 is the first release under the new name (ADR-0020). An
OmniBridge 1.0.0 install upgrades with the distribution's normal command, and
three things have to be true afterwards. The rebrand plan's Wave 7 decision
B5 chose how, by measurement rather than by assumption. The measurements are
in [`docs/reports/branding/PLIWEE-WAVE-7-LINUX-INTEGRATION-PACKAGING.md`](../../docs/reports/branding/PLIWEE-WAVE-7-LINUX-INTEGRATION-PACKAGING.md).

**1. An account that enabled `omnibridged.service` runs Pliwee at its next
login, and exactly one daemon runs.** That enablement is a symlink in
`~/.config/systemd/user/default.target.wants/`, which no package may write.
So the packages ship `/usr/lib/systemd/user/omnibridged.service` as a
**symlink to `pliweed.service`** in the same directory. systemd loads that as
an *alias*: the old link starts `pliweed.service`, one unit under two names,
so a second daemon is impossible by construction rather than prevented by
`Conflicts=`. Measured on systemd 259: the legacy link pulls the unit in once;
a running OmniBridge daemon is merged into `pliweed.service` on the next
`daemon-reload` (same PID) and `systemctl --user start pliweed` starts nothing
new; `systemctl --user disable pliweed` removes the legacy link too; an
account that never enabled it stays disabled.

**2. Enablement is visible under the new name.** It is not, by itself:
`systemctl --user is-enabled pliweed.service` answers `disabled` while only
the old link exists. `pliweed` notices that at start-up (it reads, never
writes, the user's unit configuration) and logs the one command that records
it under the new name:

```bash
systemctl --user reenable pliweed.service
```

It removes the legacy link, creates the canonical one, and running it again
changes nothing.

**3. The old package's scriptlets must not undo it.** The OmniBridge 1.0.0
RPM's `%preun` runs `systemd-update-helper remove-user-units
omnibridged.service` when the package is *erased*, and that disables and stops
the unit for every logged-in user. `Obsoletes:` erases it. So the upgrade path
is a **transitional `omnibridge` package**, version 1.1.0, that depends on
`pliwee`: `omnibridge` is *upgraded*, its `%preun` sees `$1 = 1`, and nothing
is disabled. Measured with dnf5 on Fedora 44 (`Obsoletes:` alone: `$1 = 0`;
transitional: `$1 = 1`). On Debian and Ubuntu the transitional package is
required anyway — `apt upgrade` does not install a package nothing depends
on, and with `Replaces:`/`Breaks:`/`Provides:` alone it upgraded nothing.
`omnibridge-gui` has no systemd scriptlet, so the RPM replaces it with
`Obsoletes:` + `Provides:`; the Debian packaging keeps a transitional
`omnibridge-gui` for the same `apt upgrade` reason.

`apt-get upgrade` keeps both transitional packages back, because they add a
new dependency; use `apt upgrade`, `apt full-upgrade` or `apt-get
dist-upgrade`.

**What resets.** The desktop keys some per-user settings by the application
id, which changed from `io.github.yurisismotto.omnibridge` to
`io.github.yurisismotto.pliwee`: a dock or panel favourite, the per-app
notification settings, and whether the tray item was pinned or hidden. They
are not carried over, and nothing edits a user's desktop settings to carry
them. Pin Pliwee again, and set its notification preferences again, once.

**What does not reset.** The device identity and every pairing (the daemon
copies `~/.local/share/omnibridge` into `~/.local/share/pliwee` on its first
start and leaves the original untouched, ADR-0020 D9), the GUI's device
choice (`gui.json`), and the firewall: `omnibridge.xml` is still installed
beside `pliwee.xml`, so a zone that names the `omnibridge` service still
reloads.

**Development installs.** Files installed by an OmniBridge checkout's
`desktop/gui/tools/install-desktop-metadata.sh` stay under the old id. Run
`desktop/gui/tools/install-desktop-metadata.sh --uninstall` from this
checkout: it removes the Pliwee files and, by exact name, the OmniBridge ones
(`io.github.yurisismotto.omnibridge.desktop`, `.svg`, `.service`,
`.metainfo.xml`, and a `bin/omnibridge-gui` symlink it made).

**Going back.** Remove `pliwee`, `pliwee-gui` and the transitional
`omnibridge`, then install the 1.0.0 packages. OmniBridge 1.0.0 starts on its
own, untouched `~/.local/share/omnibridge`; pairings made under Pliwee are not
in it. Removing `pliwee` disables its unit for logged-in users (that is what
removal means), so re-enable `omnibridged.service` afterwards.
