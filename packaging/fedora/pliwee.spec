# No -debuginfo / -debugsource subpackage for v1. Recorded as a deliberate
# choice, not an oversight:
# docs/audits/packaging/PACKAGING-V1-READINESS-AUDIT.md §19 Q2.
%global debug_package %{nil}

Name:           pliwee
# AUTHORITATIVE SOURCE: desktop/Cargo.toml [workspace.package] version.
#
# rpm cannot read a TOML file at spec-parse time, and an SRPM does not carry
# Cargo.toml at the point Version: is needed, so this is a literal copy. It is
# not an unguarded one: packaging/release/make-source-bundle.sh refuses to
# build a bundle when the two disagree, and packaging/tests/packaging-checks.sh
# asserts it without building anything. Audit §13.1.
Version:        1.1.0
Release:        1%{?dist}
Summary:        Local-first device continuity between Android and Fedora

License:        Apache-2.0
URL:            https://github.com/yurisismotto/pliwee

# Both tarballs come from packaging/release/make-source-bundle.sh. Source1 is
# every crate in desktop/Cargo.lock, vendored, because mock and koji build
# with networking switched off and `cargo build --locked` still downloads.
# That was defect B3 (audit §5.1) and it made this package unbuildable.
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.xz

# The Rust floor is measured from the committed lockfile, not chosen: `time`
# 0.3.55 demands 1.88 and cargo's resolver refuses before compiling a line.
# desktop/Cargo.toml carries the full derivation. This line used to say 1.82,
# which the workspace's own comment records as having been false (audit P1).
BuildRequires:  rust >= 1.88
BuildRequires:  cargo

# --- native toolchain ------------------------------------------------------
BuildRequires:  gcc

# --- GUI build dependencies (audit B1) -------------------------------------
#
# %%build compiles pliwee-gui, and it used to do so with none of this
# declared, so the package could not build at all. Each line below is here
# because something in the build asks for it by name:
#
#   pkgconf-pkg-config  gtk4-sys, libadwaita-sys and the six other -sys crates
#                       resolve their C libraries through `system-deps`, which
#                       shells out to pkg-config. gtk4-devel also Requires
#                       /usr/bin/pkg-config directly.
#   gtk4-devel          pkg-config module `gtk4` >= 4.12 (the v4_12 feature in
#                       desktop/gui/Cargo.toml gates CssProvider::load_from_string).
#                       Pulls pango-devel, graphene-devel and gdk-pixbuf2-devel,
#                       which gdk4-sys, gsk4-sys, pango-sys, graphene-sys and
#                       gdk-pixbuf-sys need, so those are not repeated here.
#   libadwaita-devel    pkg-config module `libadwaita-1` >= 1.5 (adw::Dialog and
#                       adw::AlertDialog do not exist at 1.4).
#   glib2-devel         two separate reasons: the `glib-2.0`, `gio-2.0` and
#                       `gobject-2.0` pkg-config modules, and the
#                       glib-compile-resources binary that desktop/gui/build.rs
#                       runs through glib-build-tools to compile the gresource
#                       bundle into the binary.
#
# Nothing here was added speculatively. The set was derived from the
# [package.metadata.system-deps] tables of the locked -sys crates, mapped to
# owning packages with rpm -qf, and then proved minimal by building in a
# container that had only these installed.
BuildRequires:  pkgconf-pkg-config
BuildRequires:  gtk4-devel >= 4.12
BuildRequires:  libadwaita-devel >= 1.5
BuildRequires:  glib2-devel

# --- systemd macros (audit B2) ---------------------------------------------
#
# %%{_userunitdir} comes from this package and from nowhere else. Without it
# the macro does not expand, %%install creates a directory whose name is the
# literal string '%%{_userunitdir}', and %%files fails on the path that is
# missing. Declaring it is the fix; hardcoding /usr/lib/systemd/user would
# bypass the symptom and lose the distro-correct path.
BuildRequires:  systemd-rpm-macros

# --- %%check dependencies ---------------------------------------------------
#
# platform-linux/tests/{tray_dbus,tray_gnome}.rs do not mock the bus. Each one
# raises its own private dbus-daemon with no service directories, publishes a
# real StatusNotifierItem on it and drives a fake watcher with KDE's own
# signatures — which is the only way those tests can say anything true about
# what Plasma and GNOME will do. The fixture shells out to the binary by name
# and says so itself: `.expect("dbus-daemon should be installed")`,
# platform-linux/tests/common/mod.rs:68.
#
# Nothing else in the suite needs a binary the buildroot lacks: the full set
# it shells out to is /bin/sh, id, loginctl and dbus-daemon, and the first
# three are already there. The tests that want a *session* bus, a display, a
# real UPower or a real clipboard are #[ignore]d and stay skipped here.
BuildRequires:  dbus-daemon

# --- desktop metadata validators -------------------------------------------
#
# %%install runs desktop/gui/tools/install-desktop-metadata.sh, which validates
# both metadata files before installing them and skips the check when the
# validator is absent. Declaring these turns that from a courtesy into a build
# gate: a malformed .desktop entry is ignored by the session silently, and a
# malformed metainfo file is dropped by the AppStream cache builder without a
# word. Both failures look exactly like the file never having been installed.
#
#   desktop-file-utils   desktop-file-validate
#   libappstream-glib    appstream-util validate-relax
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

# --- firewalld directory layout --------------------------------------------
#
# Owns /usr/lib/firewalld/services so the service definition lands somewhere a
# package is responsible for. It is the directory layout only: it does not pull
# firewalld itself, and it enables nothing.
BuildRequires:  firewalld-filesystem

# No protobuf-compiler: the build uses protox, a pure-Rust protobuf compiler.
# See docs/adr/ADR-0004-protocol-buffers.md.

# UPower is a weak dependency: without it the daemon simply does not report
# this machine's own battery, which is the normal case on a desktop tower.
Recommends:     upower

# Wayland clipboard. Suggests, and deliberately not a versioned Requires: the
# daemon reports honestly what its backend can do and works without it.
#
# No version constraint, and that is measured rather than lazy. `wl-copy
# --sensitive` is absent from wl-clipboard 2.2.1 on Debian and Ubuntu but
# present in Fedora's 2.2.1^git package — the same version string, different
# code. A >= here would be false on one distribution or the other whichever
# number was chosen. Audit R5.
Suggests:       wl-clipboard

# Directory ownership only, for the firewalld service definition in %%files.
Requires:       firewalld-filesystem

# --- the OmniBridge -> Pliwee package transition (ADR-0020) -----------------
#
# The first Pliwee version is 1.1.0 (rebrand plan, decision B4: adopted in
# Wave 7, to be confirmed by the owner before anything is published). The bound
# below is written against it as a literal: it describes the one upgrade path
# from OmniBridge and must not move with later versions.
#
# The core package is replaced by a TRANSITIONAL `omnibridge` 1.1.0 that
# requires `pliwee`, and deliberately NOT by `Obsoletes: omnibridge`. MEASURED
# with dnf5 on Fedora 44 (rebrand Wave 7 report, R1-R3): an obsoleted
# omnibridge-1.0.0 is erased, so its %%preun runs with $1 = 0, and on this
# systemd that is `systemd-update-helper remove-user-units omnibridged.service`,
# which disables and stops the unit for every logged-in user. The person
# running `dnf upgrade` would lose their enablement and their running daemon.
# Upgraded instead, the same %%preun sees $1 = 1 and does nothing. With both a
# transitional package and an Obsoletes:, dnf still took the Obsoletes: path
# ($1 = 0), so there is no Obsoletes: on the core at all.
#
# omnibridge-gui 1.0.0 has no systemd scriptlet, so for it the ordinary
# Obsoletes: + Provides: is both correct and what the plan asks for.

%description
Pliwee connects an Android phone to a Fedora workstation over the
local network. It is local-first: there is no cloud service, no account and
no telemetry. Devices authenticate each other with pinned public keys over
TLS 1.3 after an explicit, human-confirmed pairing.

This package provides the user-session daemon and the pliwee command-line
tool. The daemon runs unprivileged under systemd --user and never requires
root. The desktop application ships separately, as pliwee-gui.

Pliwee was called OmniBridge until 1.0.0. An OmniBridge install upgrades to
this package in place, keeping its device identity and every pairing.

%package -n omnibridge
Summary:        Transitional package: OmniBridge is now Pliwee
BuildArch:      noarch
Requires:       %{name} = %{version}-%{release}

%description -n omnibridge
OmniBridge was renamed Pliwee. This empty package upgrades an OmniBridge
1.0.0 install to Pliwee without removing the old package first, which would
disable the daemon for every logged-in user. It can be removed once Pliwee is
installed.

%package gui
Summary:        Desktop application for Pliwee
# The exact build. The GUI speaks the daemon's control socket, so a version
# skew between the two is a protocol skew.
Requires:       %{name} = %{version}-%{release}
Obsoletes:      omnibridge-gui < 1.1.0
Provides:       omnibridge-gui = %{version}-%{release}

%description gui
The Pliwee desktop application: pairing, the device list, transfers, the
Quick Panel and per-capability grants.

It is not a resident process. It is started when a window is wanted — from the
application menu, from the tray item the daemon publishes, or by the session
bus through D-Bus activation — and it exits when the window is closed.
The daemon remains the only long-lived Pliwee process.

%prep
%autosetup -n %{name}-%{version}

# Unpack the vendored crates next to the workspace and point cargo at them.
# From here on the build resolves every dependency from this tree, which is
# what makes it survive a network-isolated buildroot.
tar -xf %{SOURCE1} -C desktop
install -Dpm0644 packaging/common/cargo-vendor-config.toml desktop/.cargo/config.toml

%build
# CARGO_HOME is redirected into the build tree so no ~/.cargo/config.toml
# belonging to whoever runs rpmbuild can quietly reintroduce a registry. With
# that plus --offline, a network fetch is not merely unnecessary here, it is
# impossible — which is the property B3 was missing.
export CARGO_HOME=%{_builddir}/%{name}-cargo-home

cd desktop
# Explicit targets rather than --workspace: the package ships exactly these
# three binaries, and naming them is what keeps the build from drifting into
# compiling members the package does not install. They already pull in every
# workspace library between them, so nothing is lost.
cargo build --release --locked --offline \
    -p pliwee-daemon \
    -p pliwee-cli \
    -p pliwee-gui

%install
install -Dpm0755 desktop/target/release/pliweed    %{buildroot}%{_bindir}/pliweed
install -Dpm0755 desktop/target/release/pliwee     %{buildroot}%{_bindir}/pliwee
install -Dpm0755 desktop/target/release/pliwee-gui %{buildroot}%{_bindir}/pliwee-gui

# One unit, two formats. packaging/common/ is the canonical location: the
# Debian packaging installs this same file, so a hardening change cannot land
# on one distribution and miss the other.
install -Dpm0644 packaging/common/pliweed.service \
    %{buildroot}%{_userunitdir}/pliweed.service
# The OmniBridge 1.0.0 unit name, as a symlink in the same directory: systemd
# loads it as an alias of pliweed.service, so an account whose
# ~/.config/systemd/user still enables omnibridged.service starts Pliwee at its
# next login, and only once. No scriptlet writes into a home directory; this
# file is the whole mechanism. Shipped through Pliwee v1.x (ADR-0020;
# packaging/common/README.md, "Upgrading from OmniBridge").
ln -s pliweed.service %{buildroot}%{_userunitdir}/omnibridged.service

# firewalld service definitions: TCP 55432, installed and never enabled. No
# scriptlet in this spec runs firewall-cmd, on install or on removal. mDNS is
# deliberately not redeclared — firewalld ships its own, correctly scoped.
# Audit §9.2.
#
# Two files, one port. `pliwee` is the service to use. `omnibridge` is the
# OmniBridge 1.0.0 file, unchanged, and it stays through Pliwee v1.x so that a
# zone which names the `omnibridge` service still loads and still reloads
# after the upgrade (ADR-0020). Neither is added to any zone.
install -Dpm0644 packaging/fedora/pliwee-firewalld.xml \
    %{buildroot}%{_prefix}/lib/firewalld/services/pliwee.xml
install -Dpm0644 packaging/fedora/omnibridge-firewalld.xml \
    %{buildroot}%{_prefix}/lib/firewalld/services/omnibridge.xml

# The desktop entry, the hicolor icon, the D-Bus activation entry and the
# AppStream metadata, all from the one script a development install uses. That
# is why this calls it instead of repeating four install lines: the .desktop
# file, the icon and the metainfo are installed *verbatim*, so the
# application's identity cannot differ between a development machine and a
# package. Only the D-Bus service file is generated, and only its Exec= line,
# from --prefix. Audit §10, R9.
#
# --destdir keeps every write inside the buildroot, and the script's own
# refresh_caches() returns early when it is set, so no build machine's desktop
# database or icon cache is touched. On the installed machine the
# distribution's own rpm file triggers do that, which is measured in audit
# §4.4 and is why this package ships no scriptlet for either.
desktop/gui/tools/install-desktop-metadata.sh \
    --prefix %{_prefix} --destdir %{buildroot}


%check
export CARGO_HOME=%{_builddir}/%{name}-cargo-home
cd desktop
cargo test --release --locked --offline

%files
%license LICENSE
# README.md and nothing else. 0.1.0-2 carried `%%doc README.md docs/`, which put
# 178 files and 19 MB of engineering evidence — audits, certifications,
# research, sprint reports — into every install, presented as user
# documentation. It also shipped mock buildroot paths inside a packaged file,
# which rpmlint reports as an error and is right to. Audit §12.1 and R10.
%doc README.md
%{_bindir}/pliweed
%{_bindir}/pliwee
%{_userunitdir}/pliweed.service
%{_userunitdir}/omnibridged.service
# The icon belongs to the CORE package, not the GUI. pliweed owns the
# StatusNotifierItem and its icon name is the application id, which a shell
# resolves out of hicolor — not out of the GUI's compiled-in GResource. If the
# icon shipped only with pliwee-gui, a core-only install would draw a grey
# square on KDE. Audit §10.
%{_datadir}/icons/hicolor/scalable/apps/io.github.yurisismotto.pliwee.svg
%{_prefix}/lib/firewalld/services/pliwee.xml
%{_prefix}/lib/firewalld/services/omnibridge.xml

# Transitional: no files. Its whole job is to be the upgrade of omnibridge.
%files -n omnibridge

%files gui
%license LICENSE
%{_bindir}/pliwee-gui
%{_datadir}/applications/io.github.yurisismotto.pliwee.desktop
%{_datadir}/dbus-1/services/io.github.yurisismotto.pliwee.service
%{_datadir}/metainfo/io.github.yurisismotto.pliwee.metainfo.xml

# --- systemd user lifecycle -------------------------------------------------
#
# The unit is a *user* unit, so these are the --user variants, never the system
# ones. What they do here, from Fedora 44's own preset files rather than from
# memory:
#
#   %%systemd_user_post    runs `systemctl --global preset`, which consults
#                         /usr/lib/systemd/user-preset/. Fedora 44 ships
#                         90-default-user.preset and 99-default-disable.preset
#                         and neither names pliweed.service, so the preset
#                         leaves it DISABLED. That is the intended outcome, not
#                         an accident: a global enable would raise a LAN
#                         listener for every account on the machine, including
#                         service accounts that will never pair anything, and
#                         the daemon does nothing useful before a device is
#                         paired. Calling the macro is still right — it is what
#                         makes %%preun disable cleanly. Audit §4.3.
#
#   %%systemd_user_preun   disables the unit for users who enabled it, on real
#                         removal only; a no-op on upgrade.
#
#   %%systemd_user_postun  records that unit files changed.
#
# None of them can restart a running daemon: a root scriptlet has no route to a
# user's service manager. That is inherent to user units and is documented in
# packaging/common/README.md rather than worked around.
#
# There is deliberately NO scriptlet for the desktop database, the icon cache
# or the session bus. The first two are handled by the distribution's own rpm
# file triggers (measured, audit §4.4). The third cannot be done from root at
# all, which is why pliweed repairs its own activation from inside the
# user's session instead (audit §8.2).
#
# And no firewall-cmd, on any path.
#
# The scriptlets name pliweed.service only. The omnibridged.service alias is a
# file, not a unit of its own: disabling pliweed.service removes an account's
# legacy omnibridged.service link as well (measured, systemd 259).
%post
%systemd_user_post pliweed.service
cat <<'EOF'

Pliwee installed. To start it for your user:

    systemctl --user enable --now pliweed.service
    pliwee status

Then pair your phone:

    pliwee pair

Upgrading from OmniBridge 1.0.0: your device identity and pairings carry over,
and if you had enabled omnibridged.service, Pliwee starts at your next login.

The daemon runs as your user and never needs root. If your firewall zone is not
the Fedora Workstation default, see the Firewall section of
/usr/share/doc/pliwee/README.md.
EOF

%preun
%systemd_user_preun pliweed.service

# On %%postun, and why rpmlint's `empty-%%postun` is correct and ignored:
#
# MEASURED on Fedora 44 by evaluating the macro — %%systemd_user_postun expands
# to nothing at all, so the scriptlet below really is empty.
#
# The alternative is %%systemd_user_postun_with_restart, which is NOT empty: it
# marks user units for restart, and that would restart a user's running daemon
# on every upgrade. Audit §4.9 settles the opposite as deliberate behaviour and
# gate L17 asserts it — "record that the running daemon was NOT restarted".
# Taking the non-empty macro to quiet a linter would reverse a documented
# decision and break a gate.
#
# Calling the documented triple also keeps this correct if a future
# systemd-rpm-macros gives the macro a body. The explanation lives out here
# rather than inside the scriptlet because text inside it is scanned by
# rpmlint, and naming the package manager in a comment is reported as a
# dangerous command.
%postun
%systemd_user_postun pliweed.service


%changelog
* Fri Sep 25 2026 Yuri Converso Sismotto <yuri.sismotto@gmail.com> - 1.1.0-1
- OmniBridge is now Pliwee (ADR-0020). Binaries pliwee, pliweed and
  pliwee-gui; user unit pliweed.service; application id
  io.github.yurisismotto.pliwee; firewalld service pliwee.
- Upgrade path from omnibridge 1.0.0: a transitional omnibridge package that
  requires pliwee, and Obsoletes/Provides for omnibridge-gui.
- omnibridged.service ships as an alias of pliweed.service, and
  omnibridge.xml stays installed unchanged, through Pliwee 1.x.
* Wed Sep 23 2026 Yuri Converso Sismotto <yuri.sismotto@gmail.com> - 1.0.0-1
- First general-availability release. No product, protocol or packaging change
  from 0.1.0-3: this is the release-version bump alone, and the payload the
  release candidate certification measured is unchanged.
- Release reset to 1, because Version: moved.
* Tue Sep 22 2026 Yuri Converso Sismotto <yuri.sismotto@gmail.com> - 0.1.0-3
- Split the desktop application into an omnibridge-gui subpackage.
- Install the desktop entry, the hicolor icon, the D-Bus activation entry and
  AppStream metadata, all from desktop/gui/tools/install-desktop-metadata.sh so
  a package and a development install produce identical files.
- Add the systemd --user lifecycle macros. The unit still ships disabled.
- Ship a firewalld service definition for TCP 55432, installed and never
  enabled; mDNS is deliberately not redeclared.
- Trim %%doc from the whole docs/ tree to README.md. 0.1.0-2 shipped 178 files
  of engineering evidence as user documentation, one of which carried mock
  buildroot paths into the package.
- Suggests: wl-clipboard, with no version constraint.
* Tue Sep 22 2026 Yuri Converso Sismotto <yuri.sismotto@gmail.com> - 0.1.0-2
- Make the package buildable. 0.1.0-1 never produced an artifact: it compiled
  omnibridge-gui with no GTK or libadwaita build dependency declared (B1),
  used %%{_userunitdir} without BuildRequires: systemd-rpm-macros (B2), and
  ran cargo against crates.io inside a network-isolated buildroot (B3).
- Build offline from a vendored source bundle; install the GUI binary rather
  than compiling and discarding it.
* Sat Aug 29 2026 Yuri Converso Sismotto <yuri.sismotto@gmail.com> - 0.1.0-1
- Initial package: protocol foundation, pairing, authenticated transport,
  battery.v1
