#!/usr/bin/env bash
#
# Lightweight checks over the packaging tree and, when one is given, over a
# generated source bundle and a built RPM.
#
# These are the packaging equivalent of the MSRV guard in
# .github/workflows/linux-distro-compat.yml: cheap assertions that catch the
# specific ways this tree has already been observed to drift. Each one names
# the defect it is standing guard over.
#
#   ./packaging-checks.sh                       # static checks only
#   ./packaging-checks.sh --bundle dist         # ...plus the bundle in dist/
#   ./packaging-checks.sh --rpm path/to.rpm     # ...plus a built package
#
# No absolute path is baked in: every path is derived from the repository root
# or passed as an argument, so this runs the same in a checkout, a container
# and CI.

set -euo pipefail

ROOT="$(git -C "$(dirname -- "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
SPEC="$ROOT/packaging/fedora/pliwee.spec"
VENDOR_CONFIG="$ROOT/packaging/common/cargo-vendor-config.toml"

UNIT="$ROOT/packaging/common/pliweed.service"
FIREWALLD="$ROOT/packaging/fedora/pliwee-firewalld.xml"
# The OmniBridge 1.0.0 firewalld file. Kept, byte for byte, through Pliwee
# v1.x (ADR-0020): a zone that names the `omnibridge` service must still load.
LEGACY_FIREWALLD="$ROOT/packaging/fedora/omnibridge-firewalld.xml"
LEGACY_FIREWALLD_SHA256="af8fa0c6865e35fad996cd7540a7e7c30cb93b90403839a7333ede01b449ab2d"
# The first Pliwee version (rebrand plan B4; approved by the owner on
# 2026-09-25, ADR-0020 amendment A1). Every transition bound is written
# against this literal; see the "OmniBridge -> Pliwee" group below.
FIRST_PLIWEE_VERSION="1.1.0"
DEBIAN="$ROOT/packaging/debian"
GUI_DATA="$ROOT/desktop/gui/data"
APP_ID="io.github.yurisismotto.pliwee"

BUNDLE_DIR=""
RPM_FILES=()
while [ $# -gt 0 ]; do
    case "$1" in
        --bundle) BUNDLE_DIR="${2:?--bundle needs a directory}"; shift 2 ;;
        --rpm) RPM_FILES+=("${2:?--rpm needs a file}"); shift 2 ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done

# Listings go to files rather than through a pipe. `grep -q` exits on its
# first match, which hands the writing end of a pipe a SIGPIPE, which
# `set -o pipefail` then reports as a failed check — a false negative that
# only appears once the listing outgrows the 64 KiB pipe buffer, which is
# exactly the kind of test that lies quietly for months.
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/pliwee-checks.XXXXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

PASS=0
FAIL=0
pass() { printf '  ok    %s\n' "$*"; PASS=$((PASS + 1)); }
fail() { printf '  FAIL  %s\n' "$*"; FAIL=$((FAIL + 1)); }
group() { printf '\n%s\n' "$*"; }

# --------------------------------------------------------------------------
group "Version synchronisation (audit §13.1)"
# --------------------------------------------------------------------------
workspace_version="$(
    awk '
        /^\[workspace\.package\]/ { in_section = 1; next }
        /^\[/                     { in_section = 0 }
        in_section && /^[[:space:]]*version[[:space:]]*=/ {
            gsub(/.*=[[:space:]]*"|".*/, ""); print; exit
        }
    ' "$ROOT/desktop/Cargo.toml"
)"
spec_version="$(awk '/^Version:/ { print $2; exit }' "$SPEC")"

if [ -z "$workspace_version" ]; then
    fail "desktop/Cargo.toml [workspace.package] version is unreadable"
elif [ "$workspace_version" = "$spec_version" ]; then
    pass "spec Version: $spec_version matches the workspace version"
else
    fail "spec says $spec_version, desktop/Cargo.toml says $workspace_version"
fi

# --------------------------------------------------------------------------
group "MSRV synchronisation (audit P1)"
# --------------------------------------------------------------------------
msrv="$(awk -F'"' '/^rust-version/ { print $2; exit }' "$ROOT/desktop/Cargo.toml")"
spec_rust="$(awk '/^BuildRequires:[[:space:]]*rust[[:space:]]*>=/ { print $NF; exit }' "$SPEC")"
if [ "$msrv" = "$spec_rust" ]; then
    pass "spec BuildRequires: rust >= $spec_rust matches rust-version = $msrv"
else
    fail "spec requires rust >= ${spec_rust:-<none>}, workspace MSRV is $msrv"
fi

# --------------------------------------------------------------------------
group "Build-fatal defects B1/B2/B3 stay fixed"
# --------------------------------------------------------------------------
for br in pkgconf-pkg-config gtk4-devel libadwaita-devel glib2-devel; do
    if grep -qE "^BuildRequires:[[:space:]]+$br( |$)" "$SPEC"; then
        pass "B1: BuildRequires $br"
    else
        fail "B1: the spec builds pliwee-gui without BuildRequires: $br"
    fi
done

# The tray tests raise their own dbus-daemon rather than mocking the bus, so
# %check needs the binary even though nothing in %build does.
if grep -qE '^BuildRequires:[[:space:]]+dbus-daemon( |$)' "$SPEC"; then
    pass "B1: BuildRequires dbus-daemon (the tray suites need a real bus)"
else
    fail "B1: %check raises a dbus-daemon that the buildroot will not have"
fi

if grep -q '^BuildRequires:[[:space:]]*systemd-rpm-macros' "$SPEC"; then
    pass "B2: BuildRequires systemd-rpm-macros"
else
    fail "B2: %{_userunitdir} is used without BuildRequires: systemd-rpm-macros"
fi

if grep -q '%{_userunitdir}' "$SPEC"; then
    pass "B2: the user unit path stays a macro"
else
    fail "B2: %{_userunitdir} was replaced by a hardcoded path"
fi

if grep -qE 'cargo (build|test).*--offline' "$SPEC"; then
    pass "B3: cargo runs --offline"
else
    fail "B3: cargo may reach crates.io during the build"
fi
if grep -qE 'cargo (build|test).*--locked' "$SPEC"; then
    pass "B3: cargo runs --locked"
else
    fail "B3: the build may update Cargo.lock"
fi
if grep -q '^Source1:' "$SPEC" && grep -q 'cargo-vendor-config.toml' "$SPEC"; then
    pass "B3: a vendor tarball and its cargo config are wired into %prep"
else
    fail "B3: no vendored source is unpacked, so an isolated build cannot resolve"
fi

# --------------------------------------------------------------------------
group "Vendor config describes an offline-only source (audit B3)"
# --------------------------------------------------------------------------
if grep -q 'replace-with = "vendored-sources"' "$VENDOR_CONFIG"; then
    pass "crates-io is replaced by vendored-sources"
else
    fail "crates-io is not replaced in $VENDOR_CONFIG"
fi
if grep -qE '^directory = "vendor"' "$VENDOR_CONFIG"; then
    pass "the vendor directory is relative, so the config is location-independent"
else
    fail "the vendor directory is absent or absolute in $VENDOR_CONFIG"
fi
if grep -qE '^\[source\."(git|sparse)\+' "$VENDOR_CONFIG"; then
    fail "a network source is configured in $VENDOR_CONFIG"
else
    pass "no git or sparse registry source is configured"
fi

# --------------------------------------------------------------------------
group "The package installs everything it builds (audit P7)"
# --------------------------------------------------------------------------
for bin in pliweed pliwee pliwee-gui; do
    if grep -qE "^%\{_bindir\}/$bin$" "$SPEC"; then
        pass "%files lists $bin"
    else
        fail "$bin is built but never appears in %files"
    fi
done
for pkg in pliwee-daemon pliwee-cli pliwee-gui; do
    if grep -q -- "-p $pkg" "$SPEC"; then
        pass "%build names $pkg explicitly"
    else
        fail "%build does not name $pkg"
    fi
done

# --------------------------------------------------------------------------
group "The canonical systemd user unit (audit §7.1; gates S1-S3)"
# --------------------------------------------------------------------------
# One unit, every format. The file used to live under packaging/fedora/, which
# is where a Debian package would have grown a second copy and where a
# hardening change would have landed on one distribution and missed the other.
if [ -f "$UNIT" ]; then
    pass "the unit is at packaging/common/pliweed.service"
else
    fail "packaging/common/pliweed.service is missing"
fi
if [ -e "$ROOT/packaging/fedora/pliweed.service" ]; then
    fail "a second copy of the unit survives under packaging/fedora/"
else
    pass "no duplicate unit under packaging/fedora/"
fi
if grep -qE '^install .*packaging/common/pliweed\.service' "$SPEC"; then
    pass "the spec installs the common unit"
else
    fail "the spec does not install packaging/common/pliweed.service"
fi

if [ -f "$UNIT" ]; then
    # Directives, not comments: every check below reads the file with comment
    # and blank lines stripped, so prose describing a directive can never be
    # mistaken for the directive itself.
    grep -vE '^[[:space:]]*(#|$)' "$UNIT" > "$SCRATCH/unit.directives"

    # P2, first defect: without this the daemon cannot create
    # $XDG_RUNTIME_DIR/pliwee under ProtectSystem=strict and has nowhere
    # to bind control.sock.
    if grep -qx 'RuntimeDirectory=pliwee' "$SCRATCH/unit.directives"; then
        pass "S2: RuntimeDirectory=pliwee"
    else
        fail "S2: RuntimeDirectory=pliwee is absent; the control socket has no directory"
    fi
    if grep -qx 'RuntimeDirectoryMode=0700' "$SCRATCH/unit.directives"; then
        pass "S2: RuntimeDirectoryMode=0700"
    else
        fail "S2: RuntimeDirectoryMode is not 0700"
    fi

    # P2, second and third defects. The grant is the PARENT: measured, the
    # leaf does not exist on a fresh install, and the '-' prefix that would
    # let the unit start does not then let the daemon create it.
    if grep -qx 'ReadWritePaths=%h/.local/share' "$SCRATCH/unit.directives"; then
        pass "S1: ReadWritePaths=%h/.local/share (the parent, so a fresh install can create its data directory)"
    else
        fail "S1: ReadWritePaths= is not the settled %h/.local/share"
    fi
    # Received files. Without this grant every incoming transfer failed with
    # "Read-only file system" under the unit (measured end to end, 2026-09-30).
    if grep -qx 'ReadWritePaths=-%h/Downloads' "$SCRATCH/unit.directives"; then
        pass "S1: ReadWritePaths=-%h/Downloads (files.v1 can store what it receives)"
    else
        fail "S1: the unit does not grant -%h/Downloads; every received file would fail to store"
    fi
    if grep -q '^StateDirectory=' "$SCRATCH/unit.directives"; then
        fail "StateDirectory= is back; in a user unit it creates ~/.local/state, which the daemon never opens"
    else
        pass "no StateDirectory= (it would point at ~/.local/state)"
    fi

    # The sandbox. These are the lines a well-meaning fix for a start-up
    # failure reaches for first, so each one is named rather than counted.
    for directive in \
        'NoNewPrivileges=true' \
        'PrivateTmp=true' \
        'ProtectSystem=strict' \
        'ProtectHome=read-only' \
        'ProtectKernelTunables=true' \
        'ProtectControlGroups=true' \
        'RestrictNamespaces=true' \
        'RestrictRealtime=true' \
        'RestrictSUIDSGID=true' \
        'LockPersonality=true' \
        'MemoryDenyWriteExecute=true' \
        'SystemCallArchitectures=native' \
        'SystemCallFilter=@system-service' \
        'SystemCallFilter=~@privileged @resources @obsolete' \
        'RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX AF_NETLINK'
    do
        if grep -qxF "$directive" "$SCRATCH/unit.directives"; then
            pass "hardening kept: $directive"
        else
            fail "hardening weakened or removed: $directive"
        fi
    done

    # ProtectKernelModules= is the one hardening directive that must NOT come
    # back, and it is asserted negatively for that reason.
    #
    # It shipped in 0.1.0-1 and made the unit unstartable on Ubuntu 24.04 and
    # Ubuntu 26.04: it implies CapabilityBoundingSet=~CAP_SYS_MODULE, and a
    # *user* manager can only apply a bounding-set change from inside an
    # unprivileged user namespace. Where that is not permitted, systemd falls
    # back to PR_CAPBSET_DROP, which needs CAP_SETPCAP that no user process
    # has, and the unit dies with status=218/CAPABILITIES before ExecStart.
    #
    # The protection it nominally adds is already present twice: the module
    # syscalls are absent from the SystemCallFilter=@system-service allow-list,
    # and NoNewPrivileges=true stops the bounding set being re-gained across an
    # execve. See packaging/common/pliweed.service for the measurements.
    #
    # Same rule for the two directives that name the capability machinery
    # outright: neither belongs in a unit the user's own manager starts.
    for directive in 'ProtectKernelModules' 'CapabilityBoundingSet' 'AmbientCapabilities'; do
        if grep -qE "^${directive}=" "$SCRATCH/unit.directives"; then
            fail "$directive= is back in a user unit; it makes the unit unstartable wherever unprivileged user namespaces are restricted (218/CAPABILITIES)"
        else
            pass "no $directive= — a user manager cannot apply it, and it is redundant here"
        fi
    done

    # A user unit, and it must stay one. User=/Group= in a user unit is not
    # even legal, but naming root anywhere is the mistake worth catching.
    if grep -qE '^(User|Group)=' "$SCRATCH/unit.directives"; then
        fail "the unit sets User=/Group=; it is a --user unit and must not"
    else
        pass "no User=/Group= — it runs as whoever owns the session"
    fi
    if grep -qx 'WantedBy=default.target' "$SCRATCH/unit.directives"; then
        pass "[Install] WantedBy=default.target (a user unit target)"
    else
        fail "the unit is not installed into default.target"
    fi
    if grep -qx 'ExecStart=/usr/bin/pliweed' "$SCRATCH/unit.directives"; then
        pass "ExecStart is the absolute installed path"
    else
        fail "ExecStart is not /usr/bin/pliweed"
    fi
    # The trust store is not the unit's to own. ReadWritePaths grants the
    # parent and stops there; anything that named the leaf as a directory to
    # create or clean would be creating package-owned user state.
    if grep -qE '^(RuntimeDirectory|StateDirectory|CacheDirectory|LogsDirectory|ConfigurationDirectory)=.*\.local' "$SCRATCH/unit.directives"; then
        fail "a *Directory= directive points into the user's data tree"
    else
        pass "no *Directory= directive creates or owns user state"
    fi
fi

# --------------------------------------------------------------------------
group "Desktop integration (audit §10; P4, P5, P7)"
# --------------------------------------------------------------------------
# One script installs the desktop entry, the icon, the D-Bus activation entry
# and the AppStream metadata, so a package and a development install cannot
# disagree about the application's identity (R9). The spec must call it rather
# than repeat its four install lines.
if grep -q 'install-desktop-metadata.sh' "$SPEC"; then
    pass "the spec installs desktop metadata with the shared script"
else
    fail "the spec does not call install-desktop-metadata.sh (R9: identity can drift)"
fi
if grep -q -- '--destdir' <<<"$(grep -A2 'install-desktop-metadata\.sh' "$SPEC")"; then
    pass "the metadata installer is given a --destdir"
else
    fail "the metadata installer may write outside the buildroot"
fi

# P5. `Exec` in a D-Bus service file must be absolute; the bus does not search
# PATH. The template's placeholder is what makes that true after substitution,
# and a template that lost it would install a relative Exec.
dbus_template="$GUI_DATA/$APP_ID.service.in"
if grep -qE '^Exec=@BINDIR@/pliwee-gui' "$dbus_template"; then
    pass "P5: the D-Bus template's Exec is built from @BINDIR@"
else
    fail "P5: $dbus_template does not derive Exec from @BINDIR@"
fi
if grep -qE '^Name='"$APP_ID"'$' "$dbus_template"; then
    pass "the D-Bus template declares the application id verbatim"
else
    fail "the D-Bus template's Name= is not $APP_ID"
fi

# Q3. AppStream metadata exists and agrees with the workspace version. Without
# it Pliwee is invisible in GNOME Software and KDE Discover.
metainfo="$GUI_DATA/$APP_ID.metainfo.xml"
if [ -f "$metainfo" ]; then
    pass "Q3: AppStream metadata is present"
    meta_version="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$metainfo" | head -1)"
    if [ "$meta_version" = "$workspace_version" ]; then
        pass "the metainfo release $meta_version matches the workspace version"
    else
        fail "the metainfo says ${meta_version:-<none>}, the workspace says $workspace_version"
    fi
else
    fail "Q3: no AppStream metadata at $metainfo"
fi

# The distribution owns these two caches through its own file triggers
# (MEASURED, audit §4.4). A scriptlet here would be a second, worse copy.
for forbidden_scriptlet in update-desktop-database gtk-update-icon-cache; do
    if grep -qE "^[^#]*$forbidden_scriptlet" "$SPEC"; then
        fail "the spec runs $forbidden_scriptlet; the distro's file triggers already do"
    else
        pass "no $forbidden_scriptlet scriptlet (the distro's file trigger owns it)"
    fi
done

# --------------------------------------------------------------------------
group "Every shipped XML parses"
# --------------------------------------------------------------------------
# Cheap, and not theoretical: the first draft of both files below used '--' as
# a comment underline, which is illegal inside an XML comment and made them
# unparseable. A malformed metainfo file is dropped by the AppStream cache
# builder in silence, and a malformed firewalld service is rejected at load.
for xml in "$FIREWALLD" "$LEGACY_FIREWALLD" "$metainfo" ; do
    [ -f "$xml" ] || continue
    if python3 -c 'import sys,xml.dom.minidom; xml.dom.minidom.parse(sys.argv[1])' "$xml" 2>/dev/null; then
        pass "parses: ${xml#"$ROOT"/}"
    else
        fail "does not parse as XML: ${xml#"$ROOT"/}"
    fi
done

# --------------------------------------------------------------------------
group "Firewall: shipped, never enabled (audit §9)"
# --------------------------------------------------------------------------
for fw in pliwee omnibridge; do
    if grep -q "firewalld/services/$fw.xml" "$SPEC"; then
        pass "the core package installs the firewalld service definition $fw.xml"
    else
        fail "the spec does not install firewalld/services/$fw.xml"
    fi
done
# The whole design in one assertion: a package that runs firewall-cmd is
# opening or closing a port the user did not ask it to.
if grep -nE '^[^#]*firewall-cmd' "$SPEC" > "$SCRATCH/fw" 2>/dev/null && [ -s "$SCRATCH/fw" ]; then
    fail "the spec runs firewall-cmd outside a comment"
    sed 's/^/        /' "$SCRATCH/fw"
else
    pass "no scriptlet runs firewall-cmd, on any path"
fi
for fw_file in "$FIREWALLD" "$LEGACY_FIREWALLD"; do
[ -f "$fw_file" ] || { fail "firewalld file missing: ${fw_file#"$ROOT"/}"; continue; }
if [ -f "$fw_file" ]; then
    # Parsed, not grepped. The file's own comment explains why UDP 5353 is
    # absent, and a grep over the raw text reads that explanation as a
    # declaration — which is exactly the false positive this replaced.
    python3 - "$fw_file" > "$SCRATCH/fw-ports" <<'PYEOF'
import sys, xml.dom.minidom
doc = xml.dom.minidom.parse(sys.argv[1])
for el in doc.getElementsByTagName("port"):
    print(f'{el.getAttribute("protocol")}/{el.getAttribute("port")}')
PYEOF
    declared="$(tr '\n' ' ' < "$SCRATCH/fw-ports" | sed 's/ $//')"
    if [ "$declared" = "tcp/55432" ]; then
        pass "$(basename "$fw_file") declares exactly tcp/55432 and nothing else"
    else
        fail "$(basename "$fw_file") declares '$declared'; Pliwee needs exactly tcp/55432"
    fi
    if grep -q '^udp/5353$' "$SCRATCH/fw-ports"; then
        fail "$(basename "$fw_file") redeclares mDNS; firewalld ships its own, correctly scoped"
    else
        pass "$(basename "$fw_file"): mDNS is not redeclared (firewalld's own mdns service covers it)"
    fi
fi
done
# "Keep installing omnibridge.xml, identical" (rebrand plan, Wave 7): the
# legacy file is the one OmniBridge 1.0.0 shipped, not a re-authored copy.
legacy_fw_sha="$(sha256sum "$LEGACY_FIREWALLD" 2>/dev/null | cut -d' ' -f1)"
if [ "$legacy_fw_sha" = "$LEGACY_FIREWALLD_SHA256" ]; then
    pass "omnibridge-firewalld.xml is byte-identical to the file OmniBridge 1.0.0 shipped"
else
    fail "omnibridge-firewalld.xml changed (sha256 ${legacy_fw_sha:-<unreadable>}); it must stay as shipped"
fi

# --------------------------------------------------------------------------
group "systemd user lifecycle (audit §7.3, §4.3)"
# --------------------------------------------------------------------------
for macro in systemd_user_post systemd_user_preun systemd_user_postun; do
    if grep -qE "^%$macro pliweed\.service" "$SPEC"; then
        pass "%$macro is called"
    else
        fail "%$macro is missing; the unit will not be handled on install or removal"
    fi
done
# R7. `systemctl --global enable` would raise a LAN listener for every account
# on the machine. The preset leaves it disabled and that is the decision.
if grep -qE '^[^#]*systemctl --global enable' "$SPEC"; then
    fail "the spec globally enables the unit (R7)"
else
    pass "the unit is not globally enabled"
fi

# --------------------------------------------------------------------------
group "%doc is documentation, not the evidence tree (audit R10)"
# --------------------------------------------------------------------------
doc_line="$(grep -E '^%doc ' "$SPEC" || true)"
if [ -z "$doc_line" ]; then
    fail "the spec ships no %doc at all"
elif grep -qE '(^|[[:space:]])docs/?($|[[:space:]])' <<<"$doc_line"; then
    fail "%doc ships the docs/ tree: $doc_line"
else
    pass "%doc is $doc_line"
fi

# --------------------------------------------------------------------------
group "Subpackage split (audit §12)"
# --------------------------------------------------------------------------
if grep -q '^%package gui' "$SPEC"; then
    pass "pliwee-gui is its own subpackage"
else
    fail "the GUI is not split out"
fi
if grep -qE '^Requires:[[:space:]]*%\{name\} = %\{version\}-%\{release\}' "$SPEC"; then
    pass "pliwee-gui requires the exact core build"
else
    fail "pliwee-gui does not pin the core package's exact version-release"
fi
if grep -qE '^Suggests:[[:space:]]*wl-clipboard$' "$SPEC"; then
    pass "R5: Suggests wl-clipboard, with no version constraint"
elif grep -qE '^Suggests:[[:space:]]*wl-clipboard' "$SPEC"; then
    fail "R5: wl-clipboard carries a version constraint, which is false on one distro or the other"
else
    fail "wl-clipboard is not suggested"
fi

# --------------------------------------------------------------------------
group "Debian / Ubuntu packaging (audit §6)"
# --------------------------------------------------------------------------
for f in control rules changelog copyright source/format README.source \
         pliwee.install pliwee-gui.install; do
    if [ -e "$DEBIAN/$f" ]; then
        pass "debian/$f"
    else
        fail "debian/$f is missing"
    fi
done
if [ -x "$DEBIAN/rules" ]; then
    pass "debian/rules is executable"
else
    fail "debian/rules is not executable; dpkg-buildpackage will refuse it"
fi

# One unit, every format. The Debian packaging must install the SAME file the
# RPM does, not a copy that can drift.
if grep -q 'packaging/common/pliweed.service' "$DEBIAN/rules"; then
    pass "debian/rules installs the canonical unit from packaging/common/"
else
    fail "debian/rules does not install packaging/common/pliweed.service"
fi
if [ -e "$DEBIAN/pliweed.service" ] || [ -e "$DEBIAN/omnibridge.user.service" ]; then
    fail "a second copy of the unit exists under packaging/debian/"
else
    pass "no duplicate unit under packaging/debian/"
fi
# And the same metadata installer, so the .desktop entry, the icon, the D-Bus
# activation entry and the AppStream file cannot differ between formats (R9).
if grep -q 'install-desktop-metadata.sh' "$DEBIAN/rules"; then
    pass "debian/rules installs desktop metadata with the shared script"
else
    fail "debian/rules does not call install-desktop-metadata.sh (R9)"
fi

# The offline build is what makes a buildd build possible.
for flag in --locked --offline; do
    if grep -qE "cargo (build|test).*$flag" "$DEBIAN/rules"; then
        pass "debian/rules runs cargo $flag"
    else
        fail "debian/rules does not run cargo $flag"
    fi
done
if grep -q 'CARGO_HOME' "$DEBIAN/rules"; then
    pass "debian/rules redirects CARGO_HOME into the build tree"
else
    fail "debian/rules lets the builder's ~/.cargo/config.toml reach the build"
fi

# The unit ships disabled on every format (R7).
if grep -q 'dh_installsystemduser --no-enable' "$DEBIAN/rules"; then
    pass "R7: dh_installsystemduser --no-enable"
else
    fail "R7: the Debian package may enable the unit for every user"
fi

# Runtime must not require Rust: rustc/cargo are build dependencies only.
depends_block="$(awk '/^Package: /{p=1} p' "$DEBIAN/control" | grep -E '^(Depends|Recommends|Suggests):' || true)"
if grep -qE '\b(rustc|cargo)\b' <<<"$depends_block"; then
    fail "a runtime relation names rustc or cargo"
else
    pass "no runtime relation names rustc or cargo"
fi
# The MSRV floor must be stated, and must match the workspace.
if grep -qE "^ *rustc \(>= $msrv\)" "$DEBIAN/control"; then
    pass "Build-Depends states rustc (>= $msrv), matching the workspace MSRV"
else
    fail "Build-Depends does not state rustc (>= $msrv)"
fi

# Audit §9.3: a .deb must not carry firewalld metadata.
if grep -rq 'firewalld' "$DEBIAN"/control "$DEBIAN"/rules "$DEBIAN"/*.install 2>/dev/null; then
    fail "the Debian packaging references firewalld (audit §9.3 says it must not)"
else
    pass "no firewalld metadata in the Debian packaging (audit §9.3)"
fi
# And must not drag in a GNOME Shell extension.
if grep -rqiE 'gnome-shell-extension|appindicator' "$DEBIAN"/control 2>/dev/null; then
    fail "the Debian packaging depends on a GNOME Shell extension"
else
    pass "no GNOME Shell extension dependency"
fi

# --------------------------------------------------------------------------
group "No maintainer script may touch the trust store, on any path (R6)"
# --------------------------------------------------------------------------
# The one that matters most. ~/.local/share/pliwee holds the identity key
# and every pairing. A postrm that removed it on purge would look like
# tidiness in review and would destroy the user's trust store silently.
scripts_found=0
for f in "$DEBIAN"/*.postinst "$DEBIAN"/*.postrm "$DEBIAN"/*.preinst \
         "$DEBIAN"/*.prerm "$DEBIAN"/postinst "$DEBIAN"/postrm \
         "$DEBIAN"/preinst "$DEBIAN"/prerm; do
    [ -e "$f" ] || continue
    scripts_found=$((scripts_found + 1))
    if grep -nE '(\.local/share|\$HOME|~/)' "$f" | grep -vE '^[0-9]+:[[:space:]]*#' > "$SCRATCH/deb-state"; then
        fail "$(basename "$f") references a user-state path"
        sed 's/^/        /' "$SCRATCH/deb-state"
    else
        pass "$(basename "$f") does not reference user state"
    fi
done
if [ "$scripts_found" -eq 0 ]; then
    pass "no hand-written maintainer scripts at all — debhelper generates them"
fi
# `purge` is the transaction most likely to grow a destructive postrm.
# `grep -n` over one file emits `<line>:<text>` with no filename, so the
# comment filter anchors on the line number alone.
grep -n 'purge' "$DEBIAN/rules" 2>/dev/null \
    | grep -vE '^[0-9]+:[[:space:]]*#' > "$SCRATCH/deb-purge" || true
if [ -s "$SCRATCH/deb-purge" ]; then
    fail "debian/rules mentions purge outside a comment"
    sed 's/^/        /' "$SCRATCH/deb-purge"
else
    pass "debian/rules adds no purge behaviour"
fi

# --------------------------------------------------------------------------
group "No maintainer script touches user state (audit R6)"
# --------------------------------------------------------------------------
# state.json and identity.key are the trust store. A scriptlet that removed
# them would destroy a user's pairings silently, and it would look like
# tidiness in review. The grep is the guard.
grep -nE '(\.local/share|\$HOME|%\{_sharedstatedir\}|~/)' "$SPEC" \
    | grep -vE '^[0-9]+:#' > "$SCRATCH/state-refs" || true
if [ -s "$SCRATCH/state-refs" ]; then
    fail "the spec references a user-state path outside a comment"
else
    pass "no user-state path is referenced outside comments"
fi

# --------------------------------------------------------------------------
if [ -n "$BUNDLE_DIR" ]; then
group "Generated source bundle"
# --------------------------------------------------------------------------
src_tarball="$(find "$BUNDLE_DIR" -maxdepth 1 -name 'pliwee-*.tar.gz' | head -1)"
vendor_tarball="$(find "$BUNDLE_DIR" -maxdepth 1 -name 'pliwee-*-vendor.tar.xz' | head -1)"

if [ -n "$src_tarball" ]; then pass "source tarball: $(basename "$src_tarball")"
else fail "no source tarball in $BUNDLE_DIR"; fi
if [ -n "$vendor_tarball" ]; then pass "vendor tarball: $(basename "$vendor_tarball")"
else fail "no vendor tarball in $BUNDLE_DIR"; fi

if [ -n "$src_tarball" ]; then
    tar -tzf "$src_tarball" > "$SCRATCH/src.list"

    # The first four are build inputs and the icon is read by
    # `desktop/gui/build.rs`. U2 is neither: it is here because
    # `make-source-bundle.sh` used to carry a special-case exclusion for it,
    # from when it was an untracked file at the repository root. It is now a
    # tracked document under `docs/`, and asserting that the bundle ships it
    # is what would catch that exclusion being reintroduced.
    for required in \
        desktop/Cargo.lock \
        desktop/Cargo.toml \
        packaging/fedora/pliwee.spec \
        packaging/common/cargo-vendor-config.toml \
        packaging/common/pliweed.service \
        packaging/fedora/pliwee-firewalld.xml \
        packaging/fedora/omnibridge-firewalld.xml \
        desktop/gui/tools/install-desktop-metadata.sh \
        desktop/gui/data/io.github.yurisismotto.pliwee.desktop \
        desktop/gui/data/io.github.yurisismotto.pliwee.service.in \
        desktop/gui/data/io.github.yurisismotto.pliwee.metainfo.xml \
        docs/design/assets/pliwee-app-icon.svg \
        docs/audits/linux-compat/LINUX-UBUNTU-DEBIAN-COMPAT-U2.md
    do
        if grep -qE "^pliwee-[^/]+/$required$" "$SCRATCH/src.list"; then
            pass "bundle carries $required"
        else
            fail "bundle is missing $required"
        fi
    done

    # docs/design/assets is not documentation as far as the build is
    # concerned: desktop/gui/build.rs reads the app icon out of it.
    for forbidden in \
        '\.git/' 'desktop/target/' 'android/build/' 'android/[^/]*/build/' \
        '\.apk$' '\.aab$' '\.key$' '\.pem$' '\.jks$' '\.keystore$' \
        'state\.json$' 'trust-store\.json$' 'local\.properties$'
    do
        if grep -qE "$forbidden" "$SCRATCH/src.list"; then
            fail "bundle contains a forbidden path matching /$forbidden/"
        else
            pass "bundle has no $forbidden"
        fi
    done
fi

if [ -n "$vendor_tarball" ]; then
    tar -tJf "$vendor_tarball" > "$SCRATCH/vendor.list"
    if grep -qE '^vendor/' "$SCRATCH/vendor.list"; then
        pass "vendor tarball unpacks to vendor/"
    else
        fail "vendor tarball does not have a vendor/ root"
    fi
    checksums="$(grep -c '\.cargo-checksum\.json$' "$SCRATCH/vendor.list" || true)"
    if [ "$checksums" -gt 0 ]; then
        pass "all $checksums vendored crates carry a cargo checksum"
    else
        fail "no .cargo-checksum.json in the vendor tarball"
    fi
fi
fi

# --------------------------------------------------------------------------
if [ "${#RPM_FILES[@]}" -gt 0 ]; then
group "Built packages"
# --------------------------------------------------------------------------
if ! command -v rpm >/dev/null; then
    fail "rpm is not installed; cannot inspect the built packages"
else
    # One listing per package, plus a combined one. R8: the exact APP_ID paths
    # are asserted, because packaging drifting from the application id installs
    # a differently-named icon and every desktop draws a grey square.
    : > "$SCRATCH/all.list"
    core_list=""
    gui_list=""
    transitional_list=""
    for rpm_file in "${RPM_FILES[@]}"; do
        name="$(rpm -qp --qf '%{NAME}' "$rpm_file" 2>/dev/null || basename "$rpm_file")"
        listing="$SCRATCH/$name.list"
        # LC_ALL=C: rpm localises "(contains no files)", and a translated
        # placeholder line would be read as a file (measured on a pt_BR host).
        LC_ALL=C rpm -qpl "$rpm_file" 2>/dev/null > "$listing"
        cat "$listing" >> "$SCRATCH/all.list"
        pass "read $name ($(grep -vcx '(contains no files)' "$listing" || true) files)"
        # shellcheck disable=SC2034  # transitional_list is read by no check yet (as at 7824444); kept as it was
        case "$name" in
            pliwee-gui) gui_list="$listing" ;;
            pliwee)     core_list="$listing" ;;
            omnibridge) transitional_list="$listing" ;;
        esac
    done

    has() { grep -qxF "$2" "$1"; }

    if [ -n "$core_list" ]; then
        for path in \
            /usr/bin/pliweed \
            /usr/bin/pliwee \
            /usr/lib/systemd/user/pliweed.service \
            "/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg" \
            /usr/lib/firewalld/services/pliwee.xml \
            /usr/lib/firewalld/services/omnibridge.xml \
            /usr/lib/systemd/user/omnibridged.service
        do
            if has "$core_list" "$path"; then
                pass "core: $path"
            else
                fail "core package is missing $path"
            fi
        done
        # The GUI binary moved out. If it is still here, the split did not
        # happen and the two packages both own it.
        if has "$core_list" /usr/bin/pliwee-gui; then
            fail "core package still contains /usr/bin/pliwee-gui"
        else
            pass "core: the GUI binary is not here (it is in pliwee-gui)"
        fi
        # R10. The evidence tree must not be in the package.
        if grep -q '^/usr/share/doc/pliwee/docs' "$core_list"; then
            fail "R10: the docs/ evidence tree is in the package ($(grep -c '^/usr/share/doc/pliwee/docs' "$core_list") files)"
        else
            pass "R10: no docs/ tree in the package"
        fi
        if has "$core_list" /usr/share/doc/pliwee/README.md; then
            pass "core: README.md is shipped"
        else
            fail "core package ships no README.md"
        fi
    fi

    if [ -n "$gui_list" ]; then
        for path in \
            /usr/bin/pliwee-gui \
            "/usr/share/applications/$APP_ID.desktop" \
            "/usr/share/dbus-1/services/$APP_ID.service" \
            "/usr/share/metainfo/$APP_ID.metainfo.xml"
        do
            if has "$gui_list" "$path"; then
                pass "gui: $path"
            else
                fail "pliwee-gui is missing $path"
            fi
        done
    fi

    # P5, against the built package rather than the template: the bus does not
    # search PATH, so a relative Exec here is a launcher that never starts.
    for rpm_file in "${RPM_FILES[@]}"; do
        if grep -q "dbus-1/services/$APP_ID.service" <<<"$(rpm -qpl "$rpm_file" 2>/dev/null)"; then
            exec_line="$(rpm2cpio "$rpm_file" 2>/dev/null \
                | cpio -i --to-stdout "./usr/share/dbus-1/services/$APP_ID.service" 2>/dev/null \
                | grep '^Exec=' || true)"
            case "$exec_line" in
                "Exec=/usr/bin/pliwee-gui --gapplication-service")
                    pass "P5: the packaged D-Bus Exec is the absolute installed path" ;;
                Exec=/*)
                    fail "P5: unexpected absolute Exec: $exec_line" ;;
                *)
                    fail "P5: the packaged D-Bus Exec is not absolute: ${exec_line:-<none>}" ;;
            esac
        fi
    done

    # The OmniBridge name for the unit is a symlink to pliweed.service in the
    # same directory, which is what makes systemd load it as an alias. A
    # regular file there would be a second unit and a second daemon.
    for rpm_file in "${RPM_FILES[@]}"; do
        [ "$(rpm -qp --qf '%{NAME}' "$rpm_file" 2>/dev/null)" = pliwee ] || continue
        link="$(rpm -qp --qf '[%{FILENAMES} %{FILELINKTOS}\n]' "$rpm_file" 2>/dev/null \
            | awk '$1 == "/usr/lib/systemd/user/omnibridged.service" { print $2 }')"
        if [ "$link" = "pliweed.service" ]; then
            pass "core: omnibridged.service is a symlink to pliweed.service (an alias)"
        else
            fail "core: omnibridged.service is not a symlink to pliweed.service (got '${link:-<not a symlink>}')"
        fi
    done
    # The transitional package, when it is given: no files at all, and it
    # requires the exact core build. Counted from the header's FILENAMES
    # array, which is empty for a package with no files and is never
    # translated, rather than from `rpm -qpl`'s human-readable placeholder.
    for rpm_file in "${RPM_FILES[@]}"; do
        [ "$(rpm -qp --qf '%{NAME}' "$rpm_file" 2>/dev/null)" = omnibridge ] || continue
        n_files="$(rpm -qp --qf '[%{FILENAMES}\n]' "$rpm_file" 2>/dev/null | grep -c . || true)"
        if [ "${n_files:-x}" = 0 ]; then
            pass "transitional omnibridge: no files (FILENAMES is empty)"
        else
            fail "the transitional omnibridge package carries ${n_files:-?} file(s)"
        fi
        req="$(rpm -qp --requires "$rpm_file" 2>/dev/null | grep -E '^pliwee ' || true)"
        ver="$(rpm -qp --qf '%{VERSION}-%{RELEASE}' "$rpm_file" 2>/dev/null)"
        if [ "$req" = "pliwee = $ver" ]; then
            pass "transitional omnibridge requires exactly pliwee = $ver"
        else
            fail "the transitional omnibridge requires '${req:-<no pliwee>}', not pliwee = $ver"
        fi
    done

    # Audit §12.3: no package may own a path under a user's home.
    if grep -qE '^(/home|/root|/var/home)' "$SCRATCH/all.list"; then
        fail "a package owns a path under a home directory"
        grep -E '^(/home|/root|/var/home)' "$SCRATCH/all.list" | sed 's/^/        /'
    else
        pass "no package owns anything under a home directory"
    fi
    if grep -q '%{_' "$SCRATCH/all.list"; then
        fail "an unexpanded rpm macro is in a package (B2 regression)"
    else
        pass "no unexpanded rpm macro in any file list"
    fi
fi
fi

# --------------------------------------------------------------------------
group "OmniBridge -> Pliwee transition (ADR-0020; rebrand Wave 7)"
# --------------------------------------------------------------------------
# The bounds are exact, and the reasons they have the shape they do are
# measurements, recorded in docs/reports/branding/PLIWEE-WAVE-7-*.md:
#
#   * RPM core: a TRANSITIONAL `omnibridge` package, NOT `Obsoletes:`. An
#     obsoleted omnibridge-1.0.0 is erased, its %preun runs with $1 = 0, and
#     that is `systemd-update-helper remove-user-units omnibridged.service`:
#     disable --now for every logged-in user. Measured, dnf5, Fedora 44.
#   * RPM gui: Obsoletes: + Provides: (omnibridge-gui has no systemd scriptlet).
#   * Debian: Replaces: + Breaks: on both, and transitional packages, because
#     `apt upgrade` installs nothing that no package depends on. Measured.

version_gt() { [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$1" ]; }
if version_gt "$FIRST_PLIWEE_VERSION" "1.0.0"; then
    pass "the first Pliwee version $FIRST_PLIWEE_VERSION is above OmniBridge 1.0.0"
else
    fail "the first Pliwee version $FIRST_PLIWEE_VERSION is not above 1.0.0"
fi
if [ "$workspace_version" = "$FIRST_PLIWEE_VERSION" ] || version_gt "$workspace_version" "$FIRST_PLIWEE_VERSION"; then
    pass "the workspace version $workspace_version is at or above the first Pliwee version"
else
    fail "the workspace version $workspace_version is below the first Pliwee version $FIRST_PLIWEE_VERSION"
fi

# The package set, from rpmspec when it is here. Without it the check is
# recorded as not measured rather than passed.
if command -v rpmspec >/dev/null 2>&1; then
    rpmspec -q --qf '%{NAME}\n' --define 'dist %{nil}' "$SPEC" 2>/dev/null | sort > "$SCRATCH/spec-names"
    if [ "$(tr '\n' ' ' < "$SCRATCH/spec-names")" = "omnibridge pliwee pliwee-gui " ]; then
        pass "the spec builds exactly pliwee, pliwee-gui and the transitional omnibridge (rpmspec)"
    else
        fail "the spec builds '$(tr '\n' ' ' < "$SCRATCH/spec-names")'; expected omnibridge pliwee pliwee-gui"
    fi
else
    printf '  n/a   rpmspec is not installed: the package set was not parsed (the line checks below still run)\n'
fi
spec_directives="$(grep -vE '^[[:space:]]*#' "$SPEC")"
# The section of the spec that belongs to one (sub)package's preamble.
preamble() {
    awk -v want="$1" '
        /^%package / { cur = ($2 == "-n") ? $3 : "pliwee-" $2; next }
        /^%(description|prep|build|install|check|files|post|preun|postun|changelog)/ { cur = "" }
        cur == want && !/^[[:space:]]*#/ { print }
    ' "$SPEC"
}
{ awk '/^%package |^%description/ { exit } !/^[[:space:]]*#/ { print }' "$SPEC"; } > "$SCRATCH/pre-core"
preamble omnibridge > "$SCRATCH/pre-trans"
preamble pliwee-gui > "$SCRATCH/pre-gui"

if grep -qE '^Obsoletes:' "$SCRATCH/pre-core"; then
    fail "the core package declares Obsoletes:; an obsoleted omnibridge 1.0.0 runs remove-user-units (\$1 = 0)"
else
    pass "the core package declares no Obsoletes: (the transitional package upgrades omnibridge instead)"
fi
if grep -qE '^Provides:[[:space:]]*omnibridge([[:space:]]|$)' "$SCRATCH/pre-core"; then
    fail "the core package Provides: omnibridge; the name belongs to the transitional package"
else
    pass "the core package does not Provide: omnibridge"
fi
if grep -q '^%package -n omnibridge$' <<<"$spec_directives" \
    && grep -qxE 'Requires:[[:space:]]+%\{name\} = %\{version\}-%\{release\}' "$SCRATCH/pre-trans" \
    && grep -qxE 'BuildArch:[[:space:]]+noarch' "$SCRATCH/pre-trans"; then
    pass "transitional %package -n omnibridge: noarch, Requires: pliwee = %{version}-%{release}"
else
    fail "the transitional omnibridge package is missing or does not require the exact pliwee build"
fi
if grep -q '^%files -n omnibridge$' <<<"$spec_directives"; then
    pass "the transitional omnibridge package has a %files section (and it is empty)"
else
    fail "no %files -n omnibridge: rpmbuild would not produce the transitional package"
fi
if grep -qxE "Obsoletes:[[:space:]]+omnibridge-gui < $FIRST_PLIWEE_VERSION" "$SCRATCH/pre-gui"; then
    pass "pliwee-gui: Obsoletes: omnibridge-gui < $FIRST_PLIWEE_VERSION (exact bound)"
else
    fail "pliwee-gui does not declare exactly 'Obsoletes: omnibridge-gui < $FIRST_PLIWEE_VERSION'"
fi
if grep -qxE 'Provides:[[:space:]]+omnibridge-gui = %\{version\}-%\{release\}' "$SCRATCH/pre-gui"; then
    pass "pliwee-gui: Provides: omnibridge-gui = %{version}-%{release}"
else
    fail "pliwee-gui does not Provide: omnibridge-gui = %{version}-%{release}"
fi

# The unit alias, in both formats.
if grep -qxF 'ln -s pliweed.service %{buildroot}%{_userunitdir}/omnibridged.service' <<<"$spec_directives" \
    && grep -qxF '%{_userunitdir}/omnibridged.service' <<<"$spec_directives"; then
    pass "the spec ships omnibridged.service as a symlink to pliweed.service, and lists it"
else
    fail "the spec does not ship the omnibridged.service alias symlink (and list it in %files)"
fi
if [ "$(cat "$DEBIAN/pliwee.links" 2>/dev/null)" = "usr/lib/systemd/user/pliweed.service usr/lib/systemd/user/omnibridged.service" ]; then
    pass "debian/pliwee.links ships omnibridged.service as a symlink to pliweed.service"
else
    fail "debian/pliwee.links does not ship exactly the omnibridged.service alias"
fi
# And the alias is a symlink, not an Alias= a user would have to re-enable to
# get: the accounts that need it enabled the old name before this existed.
if grep -qE '^Alias=' "$SCRATCH/unit.directives" 2>/dev/null; then
    fail "the unit declares Alias=; the alias is shipped as a symlink, and an Alias= would only act on enable"
else
    pass "no Alias= in the unit (the alias is the shipped symlink)"
fi

# Debian relations, read per stanza.
stanza() { awk -v want="$1" '/^Package: /{ p = ($2 == want) } p && !/^#/' "$DEBIAN/control"; }
for pair in "pliwee omnibridge" "pliwee-gui omnibridge-gui"; do
    set -- $pair
    st="$(stanza "$1")"
    for rel in Replaces Breaks; do
        if grep -qxF "$rel: $2 (<< $FIRST_PLIWEE_VERSION~)" <<<"$st"; then
            pass "debian $1: $rel: $2 (<< $FIRST_PLIWEE_VERSION~)"
        else
            fail "debian $1 does not declare exactly '$rel: $2 (<< $FIRST_PLIWEE_VERSION~)'"
        fi
    done
    if grep -qE "^Provides:.*\b$2\b" <<<"$st"; then
        fail "debian $1 Provides: $2; the name belongs to the transitional package"
    else
        pass "debian $1 does not Provide: $2"
    fi
    tr_st="$(stanza "$2")"
    if grep -qx 'Architecture: all' <<<"$tr_st" \
        && grep -qE "^Depends: $1 \(>= \\\$\{binary:Version\}\)" <<<"$tr_st"; then
        pass "debian transitional $2: Architecture: all, Depends: $1 (>= \${binary:Version})"
    else
        fail "debian transitional $2 is missing, or does not depend on $1"
    fi
done
if [ "$(awk 'NR == 1 { print $1, $2 }' "$DEBIAN/changelog")" = "pliwee ($FIRST_PLIWEE_VERSION-1)" ]; then
    pass "debian/changelog's top entry is the pliwee source: $(head -1 "$DEBIAN/changelog")"
else
    fail "debian/changelog's top entry is not the pliwee source"
fi

# The desktop catalogue knows the old component was renamed.
if grep -qF '<id>io.github.yurisismotto.omnibridge</id>' <<<"$(sed -n '/<replaces>/,/<\/replaces>/p' "$metainfo")"; then
    pass "the metainfo <replaces> the OmniBridge component id"
else
    fail "the metainfo does not <replace> io.github.yurisismotto.omnibridge"
fi

# ---------------------------------------------------------------------------
# H1 — no harness may pipe into `grep -q`
# ---------------------------------------------------------------------------
# `grep -q` exits the moment it matches. Under `set -o pipefail` -- which every
# harness here sets -- the producer on the left then dies of SIGPIPE, exit 141,
# and the PIPELINE reports failure although the pattern was found.
#
# Measured, not theorised: piping a 33 KB journal capture into `grep -qE mdns`
# missed a match that was present in **225 of 300 runs**. The same three
# hundred runs missed none at all with `grep -q PATTERN <<<"$var"` or with the
# pattern applied to a file.
#
# It is worse than flaky. A privacy gate is written the unsafe way round --
#
#     if grep -qF "$SENTINEL" <<<"$capture"; then notok "leaked"; else ok "absent"; fi
#
# -- so a lost match is a PASS on a real leak, which is the exact failure this
# whole wave exists to stop, arriving through the matcher rather than through
# the measurement.
#
# Host-side pipelines only. A `| grep -q` inside a quoted guest command runs
# under the guest's `/bin/sh -c`, which does not set pipefail, so the pipeline
# status is grep's own and the hazard does not arise.
printf '\n== H1: no host-side `| grep -q` in the harnesses ==\n'
h1_bad=0
for h in "$ROOT"/packaging/tests/*.sh; do
    while IFS= read -r hit; do
        # Strip the line number, then ignore comments and this check's own
        # description of the pattern it is looking for.
        body="${hit#*:}"
        case "$body" in
            [[:space:]]*'#'*|'#'*) continue ;;
        esac
        case "$hit" in
            *ga_wait_for*|*'gx "'*|*'gu "'*|*runuser*|*'H1'*) continue ;;
        esac
        fail "H1: host-side pipe into grep -q in $(basename "$h"): $(printf '%s' "$hit" | sed 's/^[[:space:]]*//' | cut -c1-90)"
        h1_bad=$((h1_bad + 1))
    done < <(grep -nE '\| *grep -q' "$h" || true)
done
[ "$h1_bad" -eq 0 ] && pass "H1: no harness pipes into grep -q on the host side"

# ---------------------------------------------------------------------------
# H2 — every guest-driving harness loads the fail-loud primitives
# ---------------------------------------------------------------------------
# lib/assert.sh is where the twenty-nine recorded false results are encoded as
# refusals. A harness that drives a guest and does not load it is one that has
# to remember all of them in prose.
printf '\n== H2: the guest harnesses load lib/assert.sh ==\n'
h2_bad=0
for h in lifecycle-gates.sh lifecycle-peer-gates.sh security-log-evidence.sh upgrade-gates.sh pre-g8-manual-gates.sh \
         u2-state-check.sh pre-g8-autopilot.sh; do
    f="$ROOT/packaging/tests/$h"
    [ -f "$f" ] || { fail "H2: $h is missing"; h2_bad=$((h2_bad + 1)); continue; }
    if grep -q 'lib/assert.sh' "$f"; then
        pass "H2: $h loads lib/assert.sh"
    else
        fail "H2: $h does not load lib/assert.sh"
        h2_bad=$((h2_bad + 1))
    fi
done
[ "$h2_bad" -eq 0 ] || true

# ---------------------------------------------------------------------------
# F1 — no document has a user add a just-installed firewalld service unreloaded
# ---------------------------------------------------------------------------
# OmniBridge 1.0.0's Fedora README said `--permanent --add-service=omnibridge`,
# then `--reload`. The package installs omnibridge.xml (pliwee.xml) while
# firewalld runs, and a running firewalld refuses a service it has not loaded:
# INVALID_SERVICE, exit 101 (G7UP-fedora44-INSTALL, 2026-09-26). In a code
# block, a `firewall-cmd --permanent --add-service=pliwee|omnibridge` must come
# after a `firewall-cmd --reload` in the same block. Historical evidence
# (docs/audits, certification, reports) is left as it was measured, and the
# erratum quotes the 1.0.0 block on purpose.
fw_order_hits() { # FILE... — "file:line" of each add-service with no reload before it in its block
    awk '/^[[:space:]]*```/ { inblk = !inblk; reloaded = 0; next }
         inblk && /firewall-cmd/ && /--reload/ && !/--permanent/ { reloaded = 1 }
         inblk && /firewall-cmd/ && /--permanent/ && /--add-service=(pliwee|omnibridge)/ && !reloaded { print FILENAME ":" FNR }' "$@"
}
printf '\n== F1: firewalld services are reloaded before they are added ==\n'
f1_old="$(mktemp)"; f1_new="$(mktemp)"
printf '```bash\nsudo firewall-cmd --permanent --add-service=omnibridge\nsudo firewall-cmd --reload\n```\n' > "$f1_old"
printf '```bash\nsudo firewall-cmd --reload\nsudo firewall-cmd --permanent --add-service=pliwee\nsudo firewall-cmd --reload\n```\n' > "$f1_new"
if [ -n "$(fw_order_hits "$f1_old")" ] && [ -z "$(fw_order_hits "$f1_new")" ]; then
    pass "F1: the check flags the 1.0.0 order and accepts reload / add / reload"
else
    fail "F1: the check itself does not tell the 1.0.0 order from the corrected one"
fi
rm -f "$f1_old" "$f1_new"
f1_docs=()
while IFS= read -r f; do f1_docs+=("$ROOT/$f"); done < <(git -C "$ROOT" ls-files '*.md' \
    | grep -vE '^docs/(audits|certification|reports)/|^docs/migrations/OMNIBRIDGE-1\.0\.0-FEDORA-FIREWALL-ERRATUM\.md$')
if [ "${#f1_docs[@]}" -lt 20 ]; then
    fail "F1: only ${#f1_docs[@]} Markdown files found; the scan would prove nothing"
else
    f1_hits="$(fw_order_hits "${f1_docs[@]}")"
    [ -z "$f1_hits" ] \
        && pass "F1: none of ${#f1_docs[@]} current documents adds a Pliwee/OmniBridge service before reloading" \
        || fail "F1: add-service before reload in: $(sed "s|^$ROOT/||" <<<"$f1_hits" | tr '\n' ' ')"
fi

# ---------------------------------------------------------------------------
# H3 — the self-tests exist, are executable, and actually assert both ways
# ---------------------------------------------------------------------------
# A self-test file that only ever checked the good case would pass whatever the
# primitives did. Both halves must be present in it.
printf '\n== H3: the harness self-tests assert in both directions ==\n'
st="$ROOT/packaging/tests/harness-selftests.sh"
if [ ! -x "$st" ]; then
    fail "H3: packaging/tests/harness-selftests.sh is missing or not executable"
else
    n_rej="$(grep -c '^rejects \|^is_false ' "$st" || true)"
    n_acc="$(grep -c '^accepts \|^is_true ' "$st" || true)"
    [ "${n_rej:-0}" -ge 10 ] 2>/dev/null \
        && pass "H3: $n_rej cases require a primitive to REJECT its failure mode" \
        || fail "H3: only ${n_rej:-0} rejection cases; the self-tests are not covering the recorded classes"
    [ "${n_acc:-0}" -ge 5 ] 2>/dev/null \
        && pass "H3: $n_acc cases require a primitive to ACCEPT the good case, so one hard-coded to fail cannot pass" \
        || fail "H3: only ${n_acc:-0} acceptance cases; a primitive that rejected everything would pass the suite"
fi

# ---------------------------------------------------------------------------
# H4 — G7-UP runs in the order §3 defines, and the coordinator drives it
# ---------------------------------------------------------------------------
# Pre-G8 gate hardening: the upgrade stage recorded U6 n/a and downgraded
# (U10) in the same run, so U6 could never be measured against the upgraded
# guest. The self-tests prove the refusals at run time; this pins the shape,
# and the exact assertions G7-UP had, so a later edit cannot quietly undo it.
printf '\n== H4: G7-UP stage order and the pre-G8 coordinator ==\n'
ug="$ROOT/packaging/tests/upgrade-gates.sh"
# From the upgrade stage's opening line to the next stage's; a `^fi$` range
# would stop at the first inner `fi` and read a dozen lines.
up_block="$(awk '/^if \[ "\$STAGE" = upgrade \]; then/ {f=1; next} f && /^if \[ "\$STAGE" = / {exit} f' "$ug")"
n_up="$(grep -c . <<<"$up_block" || true)"
if [ "${n_up:-0}" -ge 100 ] && grep -qF 'section "U9' <<<"$up_block" && ! grep -qF 'section "U10' <<<"$up_block" \
        && grep -qF 'G7UP_CHECKPOINT' <<<"$up_block"; then
    pass "H4: the upgrade stage ($n_up lines, through U9) stops on Pliwee with a checkpoint and contains no U10"
else
    fail "H4: the upgrade stage still downgrades, or writes no checkpoint"
fi
l_ref="$(grep -n 'g7up_verify_u6 "\$EVIDENCE" "\$DISTRO" "\$DOMAIN"' "$ug" | head -1 | cut -d: -f1)"
l_u0="$(grep -n '^section "U0' "$ug" | head -1 | cut -d: -f1)"
if [ -n "$l_ref" ] && [ -n "$l_u0" ] && [ "$l_ref" -lt "$l_u0" ]; then
    pass "H4: the downgrade stage checks the U6 evidence (line $l_ref) before U0 contacts the guest (line $l_u0)"
else
    fail "H4: the U6 evidence check is missing or comes after U0 (lines ${l_ref:-none} / ${l_u0:-none})"
fi
h4_bad=0
for a in 'need_exact_count "U1: packages at exactly 1.0.0-1" "$n_ok" 2' \
         'need_exact_count "O1: OmniBridge packages at exactly 1.0.0-1" "$n0" 2' \
         'need_exact_count "O1: legacy files digested"' \
         'need_exact_count "O1: daemon processes" "$pids1" 1' \
         'need_exact_count "O2: pliweed processes" "$pids2" 1' \
         'need_exact_count "O2: omnibridged processes" "$old2" 0' \
         'need_window_covers "U4 journal after the upgrade" "$jnl" "migrated from $LEGACY"' \
         'need_window_covers "U8 journal" "$jnl" "refusing to start: $LEGACY"' \
         'ok "U10: OmniBridge 1.0.0 starts on its pre-migration identity"'; do
    grep -qF -- "$a" "$ug" || { fail "H4: G7-UP lost the assertion: $a"; h4_bad=$((h4_bad + 1)); }
done
[ "$h4_bad" -eq 0 ] && pass "H4: G7-UP keeps its exact-count, anchored-window and U10 identity assertions"
co="$ROOT/packaging/tests/pre-g8-manual-gates.sh"
if [ -x "$co" ]; then
    h4_bad=0
    for s in upgrade-gates.sh security-log-evidence.sh lifecycle-gates.sh provision-signing-keys.sh connectedDebugAndroidTest; do
        grep -qF "$s" "$co" || { fail "H4: the coordinator does not drive $s"; h4_bad=$((h4_bad + 1)); }
    done
    [ "$h4_bad" -eq 0 ] && pass "H4: the coordinator drives the repository's own gate scripts"
    if grep -nE '^[^#]*(gh (repo|release|pr)|git (merge|push|commit)|--quick-add-uid|adduid|play(-| )console|androidpublisher)' "$co" >/dev/null; then
        fail "H4: the coordinator contains a repository, release, merge, OpenPGP-UID or Play action"
    else
        pass "H4: the coordinator creates no repository or release, merges nothing, adds no UID and does not touch Play"
    fi
    grep -qxF '        "$prov" --media-a "$MEDIA_A" --media-b "$MEDIA_B"' "$co" \
        && pass "H4: signing provisioning runs with the terminal's own stdio (no pipe, tee or redirect)" \
        || fail "H4: the signing provisioning line is not the bare, uncaptured invocation"
else
    fail "H4: packaging/tests/pre-g8-manual-gates.sh is missing or not executable"
fi

# ---------------------------------------------------------------------------
# H5 — G7-UP certifies Pliwee migrating FROM 1.0.0, not the retired 1.0.0 GUI
# ---------------------------------------------------------------------------
# U2 used to open omnibridge-gui in the guest, open the guest's screen here
# with virt-viewer, and wait for an operator to click the peer, recording the
# result as a GUI selection. That certified nothing about Pliwee. The legacy
# gui.json is now migration input built in the published 1.0.0 format
# (u2-gui-fixture.sh, method=fixture), and O2 measures pliwee-gui's own
# migration of it. This pins both halves so neither can quietly come back.
printf '\n== H5: the legacy GUI is migration input, and the migration is measured ==\n'
# The harnesses and their libraries; not this file, and not the self-tests,
# which name these patterns precisely to assert that they are absent.
h5=()
for f in "$ROOT"/packaging/tests/*.sh "$ROOT"/packaging/tests/lib/*.sh; do
    case "$f" in */packaging-checks.sh|*-selftests.sh) continue ;; esac
    h5+=("$f")
done
[ "${#h5[@]}" -ge 15 ] || fail "H5: only ${#h5[@]} harness files to scan; the scan would prove nothing"
h5_code() { # PATTERN — matching lines that are not comments
    grep -nE -- "$1" "${h5[@]}" 2>/dev/null | grep -vE '^[^:]+:[0-9]+:[[:space:]]*#' || true
}
hits="$(h5_code 'setsid[^|]*omnibridge-gui|omnibridge-gui[^|]*--page|virt-viewer|exec omnibridge-gui')"
[ -z "$hits" ] && pass "H5: no harness launches omnibridge-gui or opens a guest's screen with virt-viewer" \
    || fail "H5: the retired GUI is still driven: $(sed "s|^$ROOT/||" <<<"$hits" | head -3 | tr '\n' ' ')"
hits="$(h5_code "(open|opened|start|launch)[^\"]* omnibridge-gui|select(ed)? (the (peer|tablet|device)|\\\$name) in|[Cc]lick \\\$name|OmniBridge's window")"
[ -z "$hits" ] && pass "H5: no operator instruction asks to open omnibridge-gui or to select/click a peer in it" \
    || fail "H5: an instruction still sends the operator to the old GUI: $(sed "s|^$ROOT/||" <<<"$hits" | head -3 | tr '\n' ' ')"
hits="$(h5_code 'omnibridge-gui was opened|GUI selection|GUI PASS|old GUI (tested|certified)|selected in omnibridge-gui')"
[ -z "$hits" ] && pass "H5: no G7-UP code or record text claims the old GUI was tested" \
    || fail "H5: G7-UP text still claims a GUI result: $(sed "s|^$ROOT/||" <<<"$hits" | head -3 | tr '\n' ' ')"
fx="$ROOT/packaging/tests/u2-gui-fixture.sh"; lg="$ROOT/packaging/tests/lib/legacy-gui-state.sh"
if [ -x "$fx" ] && grep -qF 'echo "method=fixture"' "$fx" && grep -qF 'echo "omnibridge_gui_exercised=no"' "$fx" \
        && grep -qF 'set -C' "$fx" && grep -qF 'runuser -u $GUEST_USER -- sh -c' "$fx" && grep -qF 'lgs_one_trusted "$TMP/state.json"' "$fx"; then
    pass "H5: the fixture records method=fixture, reads the peer from state.json, writes as the user and never over a file"
else
    fail "H5: u2-gui-fixture.sh lost method=fixture, the state.json source, the user write or noclobber"
fi
grep -qF '"legacy GUI selected-peer state exists in the published OmniBridge 1.0.0 format and selects the real paired peer (deterministic migration fixture)"' "$co" \
    && pass "H5: the coordinator's U2 item says what is measured: the 1.0.0-format migration fixture" \
    || fail "H5: the coordinator's fifth U2 item is not the migration-fixture wording"
h5_bad=0
for a in 'O1_GUI="$EVIDENCE/O1-gui.txt"; O1_GUI_BYTES="$EVIDENCE/O1-legacy-gui.json"' \
         'need_exact_count "O2: migration lines from the first pliwee-gui start" "$n_mig" 1' \
         'need_exact_count "O2: migration lines from the second pliwee-gui start" "$n_mig" 0' \
         'cmp -s "$EVIDENCE/O2-pliwee-gui.json" "$O1_GUI_BYTES"' \
         '[ "$c_sha" = "$o1sha" ]' \
         'lgs_check_gui "$EVIDENCE/O2-pliwee-gui.json" "$o1fpr"' \
         '[ "$c_mode" = "$LGS_PLIWEE_FILE_MODE" ] && [ "$c_dmode" = "$LGS_PLIWEE_DIR_MODE" ]' \
         '[ "$leg1" = "$o1sha"$'"'"'\t'"'"'"$legacy_o1" ]' \
         '[ "$canon_after" != absent ] && [ "$canon_after" = "$canon_before" ]' \
         'exec pliwee-gui'; do
    grep -qF -- "$a" "$ug" || { fail "H5: the upgrade stage lost the GUI-migration assertion: $a"; h5_bad=$((h5_bad + 1)); }
done
[ "$h5_bad" -eq 0 ] && pass "H5: O2 starts pliwee-gui and compares its gui.json with the O1 bytes, digest, fingerprint and modes, twice"
lm="$ROOT/desktop/platform-linux/src/legacy_migration.rs"; sel="$ROOT/desktop/gui/src/selection.rs"
if grep -qF 'pliwee_linux::migrate_config_file(&files)?' "$sel" && grep -qF 'const DIR_MODE: u32 = 0o700;' "$lm" \
        && grep -qF 'const FILE_MODE: u32 = 0o600;' "$lm" && grep -qF 'LGS_PLIWEE_DIR_MODE=700' "$lg" && grep -qF 'LGS_PLIWEE_FILE_MODE=600' "$lg" \
        && grep -qF 'fn a_published_1_0_0_choice_migrates_byte_for_byte' "$sel"; then
    pass "H5: Selection::load still owns the migration (0700/0600), the harness expects the same modes, and the Rust test pins the 1.0.0 bytes"
else
    fail "H5: the gui.json migration contract and what the harness measures have drifted apart"
fi

printf '\n%s\n' "-----------------------------------------------"
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
