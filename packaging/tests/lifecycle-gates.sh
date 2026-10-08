#!/usr/bin/env bash
# lifecycle-gates.sh — run the Packaging v1 Linux lifecycle gates (L1…L26)
# against a real installed desktop inside a libvirt guest.
#
# Packaging v1 could run 15 of the 26 gates. The rest needed root on an
# installed desktop, a session to cycle, a machine to reboot, a LAN the phone
# could see, and somebody looking at a screen. All of that exists inside a
# guest: `qemu:///system` is reachable without sudo, the guest agent gives
# root without credentials, macvtap puts the guest on the physical LAN with its
# own MAC, and `virsh screenshot` captures the guest's real framebuffer.
#
# The gate definitions are PACKAGING-V1-READINESS-AUDIT.md §14.2, verbatim.
# They are not restated in easier terms anywhere in this file.
#
# THE RULE THIS FILE IS BUILT AROUND
# ----------------------------------
# A test, certification gate or harness must fail loudly when its measurement
# precondition is absent. A PASS with zero observed evidence is INVALID.
#
# Packaging v1 hit that failure mode seven times: an absent `runuser`, an empty
# tracing capture, a glob that skipped a gate, `bash -s` with no stdin, a subuid
# that could not write, a relative path read as a named volume, and six packages
# built with one shipped. Every assertion below that *could* pass vacuously is
# paired with a precondition that fails first.

set -uo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/guest-agent.sh
. "$HERE/lib/guest-agent.sh"
# shellcheck source=lib/assert.sh
. "$HERE/lib/assert.sh"

DOMAIN=""; DISTRO=""; PKGDIR=""; PHONE_IP=""; EVIDENCE=""
GUEST_USER="${GUEST_USER:-anyflow}"; GUEST_UID="${GUEST_UID:-1000}"

usage() {
    cat >&2 <<USAGE
usage: $0 --domain DOM --distro NAME --pkgdir DIR --evidence DIR [--phone IP]

  --domain    libvirt domain, already running, with a guest agent
  --distro    ubuntu2404 | ubuntu2604 | debian13 | fedora44
  --pkgdir    host directory holding that distro's packages + SHA256SUMS
  --evidence  host directory for raw evidence (created)
  --phone     Android peer's LAN address; enables L10/L12 evidence
USAGE
    exit 2
}
while [ $# -gt 0 ]; do
    case "$1" in
        --domain) DOMAIN="$2"; shift 2 ;;
        --distro) DISTRO="$2"; shift 2 ;;
        --pkgdir) PKGDIR="$2"; shift 2 ;;
        --evidence) EVIDENCE="$2"; shift 2 ;;
        --phone) PHONE_IP="$2"; shift 2 ;;
        *) usage ;;
    esac
done
[ -n "$DOMAIN" ] && [ -n "$DISTRO" ] && [ -n "$PKGDIR" ] && [ -n "$EVIDENCE" ] || usage

PASS=0; FAIL=0; NA=0
declare -a FAILED_GATES=()
mkdir -p "$EVIDENCE"

ok()      { PASS=$(( PASS + 1 )); printf 'ok    %s\n' "$*"; }
notok()   { FAIL=$(( FAIL + 1 )); FAILED_GATES+=("$*"); printf 'not ok  %s\n' "$*"; }
na()      { NA=$(( NA + 1 ));   printf 'n/a   %s\n' "$*"; }
section() { printf '\n== %s ==\n' "$*"; }

# abort — a precondition failed. The run stops: continuing would produce gate
# results measured against an environment that is not the one under test.
abort() { printf '\nPRECONDITION FAILED: %s\n' "$*" >&2; printf 'Aborting: %d ok, %d not ok so far.\n' "$PASS" "$FAIL" >&2; exit 3; }

# gx  — run as root in the guest
# gu  — run as the desktop user, inside their session bus
gx() { ga_exec "$DOMAIN" "$@"; }

# L4 — the daemon's processes, by exact executable name (as `pgrep -x pliweed`
# sees them), one "user pid args" line each; and the count of those owned by
# root, read from that same capture. Until 2026-09-26 L4 matched
# `grep "[o]mnibridged"`, the pre-rebrand binary name, which the bracket also
# hid from a search for it: the first real LIFECYCLE-fedora44 run found
# pliweed running and then aborted "no pliweed process found after start", and
# the root count beside it would have read 0 whatever ran as root.
# harness-selftests.sh runs both against a real process called pliweed.
# The user column has an explicit width: procps truncates a width-less `user=`
# longer than 8 characters to 7 and a `+`, so a login like `pliwee-agent` read
# back as `pliwee-+` and the self-test went red over its own capture (#97).
# 32 is utmp's user-name field (UT_NAMESIZE), the conventional login limit.
L4_PS='ps -C pliweed -o user:32=,pid=,args='
L4_ROOT_AWK='$1 == "root" { n++ } END { print n + 0 }'
gu() {
    ga_exec "$DOMAIN" "runuser -u $GUEST_USER -- env XDG_RUNTIME_DIR=/run/user/$GUEST_UID \
        DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$GUEST_UID/bus \
        XDG_DATA_HOME=/home/$GUEST_USER/.local/share sh -c $(printf '%q' "$*")"
}
save() { cat > "$EVIDENCE/$1"; }

# ---------------------------------------------------------------------------
# Preconditions. Every one of these has to hold before a single gate runs.
# ---------------------------------------------------------------------------
section "Preconditions"

ga_ping "$DOMAIN" 300 || abort "guest agent in '$DOMAIN' does not answer"
ok "the guest agent answers guest-ping"

guest_os="$(gx 'sed -n "s/^PRETTY_NAME=//p" /etc/os-release | tr -d \"')"
[ -n "$guest_os" ] || abort "could not read /etc/os-release from the guest"
ok "guest identifies as: $guest_os"

# The distro named on the command line must be the distro in the guest. Six
# packages built and one shipped was exactly this class of mistake.
case "$DISTRO" in
    ubuntu2404) expect="Ubuntu 24.04" ;;
    ubuntu2604) expect="Ubuntu 26.04" ;;
    debian13)   expect="Debian GNU/Linux 13" ;;
    fedora44)   expect="Fedora Linux 44" ;;
    *) abort "unknown --distro '$DISTRO'" ;;
esac
case "$guest_os" in
    *"$expect"*) ok "guest matches --distro $DISTRO (expected '$expect')" ;;
    *) abort "--distro says $DISTRO (expects '$expect') but the guest is '$guest_os'" ;;
esac

[ "$(gx 'id -u' | tr -d '[:space:]')" = "0" ] || abort "the guest agent is not running as root"
ok "the guest agent runs as root"

gx "id -u $GUEST_USER" >/dev/null 2>&1 || abort "guest has no user '$GUEST_USER'"
ok "guest desktop user '$GUEST_USER' exists"

# The host tools this harness cannot run without. Packaging v1's install-smoke
# called an absent `runuser`, the call failed, nobody looked, and the gate
# passed having run nothing.
need_tool virsh jq sha256sum find || abort "a host tool this harness depends on is missing"
ok "every host tool this harness needs is installed"

# Packages: present on the host, digest-verified, and the right number of them.
#
# Absolute, because a relative path is read as a NAMED VOLUME by podman and
# docker: Packaging v1 mounted an empty volume over the source tree that way
# and every file assertion inside it measured nothing.
need_abs_path "--pkgdir" "$PKGDIR" || abort "--pkgdir must be absolute"
[ -d "$PKGDIR" ] || abort "--pkgdir '$PKGDIR' is not a directory"
[ -f "$PKGDIR/SHA256SUMS" ] || abort "no SHA256SUMS in '$PKGDIR' — the artifact under test would be unverified"
( cd "$PKGDIR" && sha256sum -c SHA256SUMS >/dev/null 2>&1 ) \
    || abort "SHA256SUMS does not verify in '$PKGDIR'"
ok "every package in $PKGDIR verifies against SHA256SUMS"

case "$DISTRO" in
    # The release set per distribution: pliwee, pliwee-gui and the transitional
    # omnibridge package(s) that upgrade an OmniBridge 1.0.0 install (plus the
    # SRPM on Fedora). The transitional ones are delivered and verified with
    # the rest, and deliberately NOT installed: these gates measure a clean
    # Pliwee install. The upgrade from OmniBridge is upgrade-gates.sh (G7-UP).
    fedora44) PKGEXT="rpm"; want_pkgs=4 ;;
    *)        PKGEXT="deb"; want_pkgs=4 ;;
esac
mapfile -t PKGS < <(find "$PKGDIR" -maxdepth 1 -name "*.$PKGEXT" -printf '%f\n' | sort)
# A glob that matched nothing is how L17 silently skipped its whole group, and
# a glob that matched the WRONG number is how six packages were built and one
# was shipped. Both are the same assertion: an exact count.
need_glob "$PKGDIR" "*.$PKGEXT" "$want_pkgs" \
    || abort "the package set in '$PKGDIR' is not the one under test (found: ${PKGS[*]:-<none>})"
[ "${#PKGS[@]}" -eq "$want_pkgs" ] \
    || abort "expected $want_pkgs *.$PKGEXT in '$PKGDIR', found ${#PKGS[@]}: ${PKGS[*]:-<none>}"
ok "found exactly $want_pkgs *.$PKGEXT package(s): ${PKGS[*]}"

# Nothing of ours may be installed already, or "clean install" means nothing.
if [ "$PKGEXT" = "deb" ]; then
    pre="$(gx 'dpkg-query -W -f "\${Package} \${Status}\n" pliwee pliwee-gui 2>/dev/null | grep -c " install ok installed" || true')"
else
    pre="$(gx 'rpm -q --qf "%{NAME}\n" pliwee pliwee-gui 2>/dev/null | grep -c "^pliwee" || true')"
fi
[ "${pre//[[:space:]]/}" = "0" ] \
    || abort "Pliwee is already installed in the guest ($pre package(s)) — L1 cannot measure a clean install"
ok "no Pliwee package is installed in the guest (clean-install precondition)"

# A graphical session must exist, or L3/L6/L9/L19 measure nothing.
sessions="$(gx "loginctl list-sessions --no-legend 2>/dev/null | grep -c ' $GUEST_USER ' || true")"
[ "${sessions//[[:space:]]/}" -ge 1 ] 2>/dev/null \
    || abort "no logind session for '$GUEST_USER' — every session gate would measure nothing"
ok "a logind session exists for '$GUEST_USER'"

seat="$(gx "loginctl show-user $GUEST_USER -p Display --value 2>/dev/null")"
sess_type="$(gx "loginctl show-session $(echo "$seat" | tr -d '[:space:]') -p Type --value 2>/dev/null" | tr -d '[:space:]')"
[ -n "$sess_type" ] && [ "$sess_type" != "tty" ] \
    && ok "the session is graphical (Type=$sess_type)" \
    || abort "session for '$GUEST_USER' is not graphical (Type=${sess_type:-unknown})"

# LAN placement. The whole point of macvtap; assert it rather than assume it.
guest_ip="$(gx "ip -4 -br addr show scope global | awk '{print \$3}' | cut -d/ -f1 | head -1" | tr -d '[:space:]')"
[ -n "$guest_ip" ] || abort "the guest has no global IPv4 address — it is not on the LAN"
ok "guest is on the LAN at $guest_ip"

if [ -n "$PHONE_IP" ]; then
    gx "ping -c 3 -W 3 $PHONE_IP >/dev/null 2>&1" \
        || abort "the guest cannot reach the phone at $PHONE_IP — LAN gates would measure nothing"
    ok "guest reaches the Android peer at $PHONE_IP"
fi

{
  echo "domain    $DOMAIN"
  echo "distro    $DISTRO"
  echo "guest_os  $guest_os"
  echo "guest_ip  $guest_ip"
  echo "phone     ${PHONE_IP:-<not supplied>}"
  echo "kernel    $(gx 'uname -r')"
  echo "packages"
  ( cd "$PKGDIR" && sha256sum "${PKGS[@]}" )
} | save "00-preconditions.txt"

# ---------------------------------------------------------------------------
section "Delivering the packages over virtio-serial (no IP path)"
# ---------------------------------------------------------------------------
gx 'rm -rf /root/pliwee-pkgs && mkdir -p /root/pliwee-pkgs' >/dev/null
for p in "${PKGS[@]}"; do
    ga_put "$DOMAIN" "$PKGDIR/$p" "/root/pliwee-pkgs/$p" \
        || abort "could not deliver $p into the guest"
done
ga_put "$DOMAIN" "$PKGDIR/SHA256SUMS" "/root/pliwee-pkgs/SHA256SUMS" \
    || abort "could not deliver SHA256SUMS into the guest"
gx 'cd /root/pliwee-pkgs && sha256sum -c SHA256SUMS' >/dev/null 2>&1 \
    || abort "the delivered packages do not verify inside the guest"
ok "all ${#PKGS[@]} package(s) delivered and digest-verified inside the guest"

# ---------------------------------------------------------------------------
section "L1 — clean install"
# ---------------------------------------------------------------------------
if [ "$PKGEXT" = "deb" ]; then
    install_cmd='cd /root/pliwee-pkgs && DEBIAN_FRONTEND=noninteractive apt-get install -y ./pliwee_*_amd64.deb ./pliwee-gui_*_amd64.deb 2>&1'
else
    install_cmd='cd /root/pliwee-pkgs && dnf install -y --disablerepo="*" ./pliwee-[0-9]*.x86_64.rpm ./pliwee-gui-[0-9]*.x86_64.rpm 2>&1'
fi
install_out="$(gx "$install_cmd")"; install_rc=$?
printf '%s\n' "$install_out" | save "01-L1-install.txt"

[ "$install_rc" -eq 0 ] \
    && ok "L1: install exited 0" \
    || notok "L1: install exited $install_rc"

# "no scriptlet error" is part of the gate. Grep for the strings both package
# managers use, and assert the output was non-empty first — an empty capture
# would make this grep pass while measuring nothing.
[ -n "${install_out//[[:space:]]/}" ] \
    || abort "the install produced no output at all; nothing can be asserted about scriptlets"
if grep -qiE 'scriptlet failed|error in (PREIN|POSTIN|PREUN|POSTUN)|dpkg: error|subprocess installed .* returned error' <<<"$install_out"; then
    notok "L1: a scriptlet/maintainer-script error appears in the install output"
else
    ok "L1: no scriptlet or maintainer-script error in ${#install_out} bytes of install output"
fi

if [ "$PKGEXT" = "deb" ]; then
    # NOTE: ${...} must stay escaped -- the command crosses a `sh -c` inside the
    # guest, where an unescaped ${Status} expands to the empty string and the
    # count silently becomes 0.
    n_inst="$(gx 'dpkg-query -W -f "\${Status}\n" pliwee pliwee-gui 2>/dev/null | grep -c "install ok installed" || true')"
else
    n_inst="$(gx 'rpm -q pliwee pliwee-gui >/dev/null 2>&1 && echo 2 || echo 0')"
fi
# NOTE: the count crosses a `sh -c` in the guest. Packaging v1's version lost
# `${Status}` to that shell and the count silently became 0 -- which is why
# this is an exact-count assertion and not a >0 one.
need_exact_count "L1: installed package count" "$n_inst" "2" \
    && ok "L1: both packages report installed" \
    || notok "L1: expected 2 installed packages, dpkg/rpm reports ${n_inst//[[:space:]]/}"

ver="$(gx 'pliweed --version 2>&1 | head -1')"
[ -n "${ver//[[:space:]]/}" ] \
    && ok "L1: the installed daemon runs: $ver" \
    || notok "L1: /usr/bin/pliweed produced no --version output"

# ---------------------------------------------------------------------------
section "L2 — installed files manifest"
# ---------------------------------------------------------------------------
if [ "$PKGEXT" = "deb" ]; then
    core_files="$(gx 'dpkg -L pliwee | while read -r p; do [ -f "$p" ] && echo "$p"; done | sort')"
    gui_files="$(gx 'dpkg -L pliwee-gui | while read -r p; do [ -f "$p" ] && echo "$p"; done | sort')"
else
    core_files="$(gx 'rpm -ql pliwee | while read -r p; do [ -f "$p" ] && echo "$p"; done | sort')"
    gui_files="$(gx 'rpm -ql pliwee-gui | while read -r p; do [ -f "$p" ] && echo "$p"; done | sort')"
fi
printf 'core:\n%s\n\ngui:\n%s\n' "$core_files" "$gui_files" | save "02-L2-manifest.txt"

n_core="$(printf '%s\n' "$core_files" | grep -c . || true)"
n_gui="$(printf '%s\n' "$gui_files" | grep -c . || true)"
[ "$n_core" -gt 0 ] || abort "dpkg/rpm listed no files for the core package; the manifest gate would pass vacuously"
[ "$n_gui" -gt 0 ]  || abort "dpkg/rpm listed no files for the GUI package"
ok "L2: core owns $n_core file(s), gui owns $n_gui file(s)"

# §12.1 / §12.2, asserted by name rather than by count alone.
for f in /usr/bin/pliweed /usr/bin/pliwee \
         /usr/lib/systemd/user/pliweed.service \
         /usr/share/icons/hicolor/scalable/apps/io.github.yurisismotto.pliwee.svg; do
    grep -qx "$f" <<<"$core_files" \
        && ok "L2: core owns $f" \
        || notok "L2: core does NOT own $f"
done
for f in /usr/bin/pliwee-gui \
         /usr/share/applications/io.github.yurisismotto.pliwee.desktop \
         /usr/share/dbus-1/services/io.github.yurisismotto.pliwee.service \
         /usr/share/metainfo/io.github.yurisismotto.pliwee.metainfo.xml; do
    grep -qx "$f" <<<"$gui_files" \
        && ok "L2: gui owns $f" \
        || notok "L2: gui does NOT own $f"
done

# §12.4: no package owns a path under a home directory.
home_owned="$(printf '%s\n%s\n' "$core_files" "$gui_files" | grep -E '^(/home/|/root/)' || true)"
[ -z "$home_owned" ] \
    && ok "L2: no package owns a path under a home directory" \
    || notok "L2: a package owns $(printf '%s' "$home_owned" | wc -l) path(s) under a home directory"

# Every file in both manifests belongs to exactly one package.
dupes="$(comm -12 <(printf '%s\n' "$core_files") <(printf '%s\n' "$gui_files") | grep -c . || true)"
[ "$dupes" = "0" ] \
    && ok "L2: no file is owned by both packages" \
    || notok "L2: $dupes file(s) owned by both packages"

# The unit ships disabled (R7).
unit_state="$(gu 'systemctl --user is-enabled pliweed.service 2>&1' | tr -d '[:space:]')"
[ "$unit_state" = "disabled" ] \
    && ok "L2/R7: the unit is installed disabled (is-enabled=$unit_state)" \
    || notok "L2/R7: the unit is not shipped disabled (is-enabled=$unit_state)"

# The unit that is actually ON DISK after the install, not the one in the repo.
# A package built before fix/systemd-user-unit-capabilities-v1 carries a unit
# that cannot start on Ubuntu at all, and every runtime gate below would fail
# for that reason rather than for anything they are testing.
inst_unit="$(gx 'cat /usr/lib/systemd/user/pliweed.service 2>/dev/null')"
[ -n "${inst_unit//[[:space:]]/}" ] \
    || abort "the installed unit file is empty or missing; no runtime gate below could mean anything"
n_harden="$(printf '%s\n' "$inst_unit" | grep -cE '^(NoNewPrivileges|PrivateTmp|ProtectSystem|ProtectHome|ProtectKernelTunables|ProtectControlGroups|RestrictNamespaces|RestrictRealtime|RestrictSUIDSGID|LockPersonality|MemoryDenyWriteExecute|SystemCallArchitectures|SystemCallFilter|RestrictAddressFamilies)=' || true)"
[ "$n_harden" -ge 15 ] \
    && ok "L2: the installed unit carries $n_harden hardening directives" \
    || notok "L2: the installed unit carries only $n_harden hardening directives, expected at least 15"
grep -qx 'ProtectSystem=strict' <<<"$inst_unit" \
    && ok "L2: ProtectSystem=strict survived packaging" \
    || notok "L2: the installed unit does not carry ProtectSystem=strict"
if grep -qE '^(ProtectKernelModules|CapabilityBoundingSet|AmbientCapabilities)=' <<<"$inst_unit"; then
    notok "L2: the installed unit carries a capability-set directive; it cannot start where unprivileged user namespaces are restricted"
else
    ok "L2: the installed unit carries no capability-set directive (the Ubuntu 218/CAPABILITIES defect is not in this artifact)"
fi
printf '%s\n' "$inst_unit" | save "02b-installed-unit.service"

running_after_install="$(gx 'pgrep -c -x pliweed || true' | tr -d '[:space:]')"
[ "$running_after_install" = "0" ] \
    && ok "L1: installing the package started no daemon" \
    || notok "L1: installing the package left $running_after_install pliweed process(es) running"

# ---------------------------------------------------------------------------
section "L8 — D-Bus activation immediately after install, in the SAME live session"
# ---------------------------------------------------------------------------
# This gate is order-sensitive and is therefore taken here, before anything in
# this run logs out, reboots or restarts a session. A package installs the
# service file as root into a session bus that has already read its directory.
sess_start="$(gx "loginctl show-user $GUEST_USER -p Display --value" | tr -d '[:space:]')"
sess_since_boot="$(gx "loginctl show-session $sess_start -p Timestamp --value 2>/dev/null")"
ok "L8: session $sess_start has not been cycled since install (started $sess_since_boot)"

activatable="$(gu 'busctl --user list --activatable --no-pager 2>/dev/null' || true)"
[ -n "${activatable//[[:space:]]/}" ] \
    || abort "busctl --user listed nothing; the session bus is not reachable and L8 would measure nothing"
n_act="$(printf '%s\n' "$activatable" | grep -c . || true)"
ok "L8: the session bus reports $n_act activatable name(s)"

if grep -q 'io.github.yurisismotto.pliwee' <<<"$activatable"; then
    ok "L8: io.github.yurisismotto.pliwee is activatable with NO logout"
else
    notok "L8: the name is NOT activatable in the live session after install"
fi
printf '%s\n' "$activatable" | save "03-L8-activatable.txt"

# ---------------------------------------------------------------------------
section "L4, L5, L18 — the daemon, its runtime directory, and restart"
# ---------------------------------------------------------------------------
gu 'systemctl --user start pliweed.service' >/dev/null 2>&1
ga_wait_for "$DOMAIN" 60 "pgrep -x pliweed >/dev/null" \
    || abort "pliweed did not start in the guest; L4/L5/L18 would measure nothing"

ps_out="$(gx "$L4_PS")"
[ -n "${ps_out//[[:space:]]/}" ] || abort "no pliweed process found after start"
printf '%s\n' "$ps_out" | save "04-L4-process.txt"
ok "L4: pliweed is running: $(printf '%s' "$ps_out" | head -1 | awk '{print $1, $2}')"

daemon_user="$(printf '%s' "$ps_out" | awk 'NR==1{print $1}')"
[ "$daemon_user" = "$GUEST_USER" ] \
    && ok "L4: the daemon runs as $GUEST_USER" \
    || notok "L4: the daemon runs as '$daemon_user', not $GUEST_USER"

root_daemons="$(awk "$L4_ROOT_AWK" <<<"$ps_out")"
[ "${root_daemons//[[:space:]]/}" = "0" ] \
    && ok "L4: no pliweed process runs as root" \
    || notok "L4: ${root_daemons//[[:space:]]/} pliweed process(es) run as root"

rt_dir="/run/user/$GUEST_UID/pliwee"
stat_dir="$(gx "stat -c '%a %U %n' $rt_dir 2>&1")"
stat_sock="$(gx "stat -c '%a %U %n' $rt_dir/control.sock 2>&1")"
printf '%s\n%s\n' "$stat_dir" "$stat_sock" | save "05-L5-runtime-dir.txt"
case "$stat_dir" in
    "700 $GUEST_USER "*) ok "L5: $rt_dir is 0700 and owned by $GUEST_USER" ;;
    *) notok "L5: runtime directory is '$stat_dir', expected '700 $GUEST_USER $rt_dir'" ;;
esac
case "$stat_sock" in
    "600 $GUEST_USER "*) ok "L5: control.sock is 0600 and owned by $GUEST_USER" ;;
    *) notok "L5: control.sock is '$stat_sock', expected '600 $GUEST_USER …'" ;;
esac

# S3 — no sandbox denial.
#
# The capture is bound to THIS invocation of the unit, not to "the last 200
# lines". The journal is persistent across reboots, so an unbounded tail pulls
# in every earlier start on this machine -- including, on a guest that has been
# used to characterise a defect, the failures that defect produced. A gate that
# greps that window is reading somebody else's evidence.
invocation="$(gu 'systemctl --user show pliweed.service -p InvocationID --value 2>/dev/null' | tr -d '[:space:]')"
[ -n "$invocation" ] \
    || abort "the unit reports no InvocationID; the S3 capture could not be bound to this run"
ok "S3: journal capture bound to invocation $invocation"
jnl="$(gu "journalctl --user _SYSTEMD_INVOCATION_ID=$invocation --no-pager 2>/dev/null" || true)"
[ -n "${jnl//[[:space:]]/}" ] \
    || abort "the journal for invocation $invocation is empty; the S3 grep would pass on an empty stream"
printf '%s\n' "$jnl" | save "06-S3-journal.txt"
if grep -qiE 'Operation not permitted|ProtectSystem|ReadWritePaths|seccomp|Permission denied' <<<"$jnl"; then
    notok "S3: a sandbox/namespace/seccomp denial appears in the daemon journal"
else
    ok "S3: no sandbox, namespace or seccomp denial in $(printf '%s\n' "$jnl" | grep -c .) journal line(s)"
fi

# L18 — restart. The socket must be recreated at 0600.
gu 'systemctl --user restart pliweed.service' >/dev/null 2>&1
ga_wait_for "$DOMAIN" 60 "test -S $rt_dir/control.sock" \
    || notok "L18: control.sock was not recreated within 60s of restart"
restart_active="$(gu 'systemctl --user is-active pliweed.service' | tr -d '[:space:]')"
[ "$restart_active" = "active" ] \
    && ok "L18: the unit is active again after restart" \
    || notok "L18: after restart the unit is '$restart_active'"
stat_sock2="$(gx "stat -c '%a %U' $rt_dir/control.sock 2>&1")"
[ "$stat_sock2" = "600 $GUEST_USER" ] \
    && ok "L18: the socket is recreated at 0600, owned by $GUEST_USER" \
    || notok "L18: after restart the socket is '$stat_sock2'"

n_daemons="$(gx 'pgrep -c -x pliweed || true' | tr -d '[:space:]')"
[ "$n_daemons" = "1" ] \
    && ok "L18: exactly one pliweed process after restart" \
    || notok "L18: $n_daemons pliweed process(es) after restart, expected 1"

# ---------------------------------------------------------------------------
section "L11 — TCP 55432 and the firewall"
# ---------------------------------------------------------------------------
listen="$(gu 'ss -tlnp 2>/dev/null | grep 55432' || true)"
[ -n "${listen//[[:space:]]/}" ] \
    && ok "L11: the daemon listens on TCP 55432: $(printf '%s' "$listen" | awk '{print $4}')" \
    || notok "L11: nothing is listening on TCP 55432 in the guest"
printf '%s\n' "$listen" | save "07-L11-listen.txt"

fw_state="$(gx 'command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | head -1 || echo "ufw: not installed"')"
fwd_state="$(gx 'command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state 2>/dev/null || echo "firewalld: not installed/running"')"
printf 'ufw:       %s\nfirewalld: %s\n' "$fw_state" "$fwd_state" | save "08-L11-firewall.txt"
ok "L11: firewall state recorded — ufw: ${fw_state}; firewalld: ${fwd_state}"

# ---------------------------------------------------------------------------
section "L7 — D-Bus cold activation"
# ---------------------------------------------------------------------------
gu 'pkill -x pliwee-gui' >/dev/null 2>&1
sleep 2
gui_before="$(gx 'pgrep -c -x pliwee-gui || true' | tr -d '[:space:]')"
[ "$gui_before" = "0" ] \
    || abort "pliwee-gui is still running; L7 requires the cold case and would measure nothing"
ok "L7: no pliwee-gui process before activation (the cold case)"

act_out="$(gu 'gdbus call --session --dest io.github.yurisismotto.pliwee --object-path /io/github/yurisismotto/pliwee --method org.freedesktop.DBus.Peer.Ping 2>&1' || true)"
printf '%s\n' "$act_out" | save "09-L7-activation.txt"
if grep -q 'ServiceUnknown\|NameHasNoOwner' <<<"$act_out"; then
    notok "L7: activation failed — $act_out"
elif grep -q '()' <<<"$act_out"; then
    ok "L7: the bus activated the name from cold; Peer.Ping returned ()"
else
    notok "L7: unexpected activation result: ${act_out:-<empty>}"
fi
gui_after="$(gx 'pgrep -c -x pliwee-gui || true' | tr -d '[:space:]')"
[ "${gui_after:-0}" -ge 1 ] 2>/dev/null \
    && ok "L7: the bus started pliwee-gui on demand ($gui_after process)" \
    || notok "L7: no pliwee-gui process appeared after activation"

# ---------------------------------------------------------------------------
section "L6 — the GUI launches from the application menu"
# ---------------------------------------------------------------------------
desktop_file="/usr/share/applications/io.github.yurisismotto.pliwee.desktop"
gx "test -f $desktop_file" \
    || abort "$desktop_file is missing; L6 cannot be measured"

dv="$(gx "command -v desktop-file-validate >/dev/null 2>&1 && desktop-file-validate $desktop_file 2>&1 || echo '__NOTOOL__'")"
case "$dv" in
    __NOTOOL__*) na "L6: desktop-file-validate is not installed in the guest" ;;
    "")          ok "L6: desktop-file-validate reports the entry clean" ;;
    *)           notok "L6: desktop-file-validate: $dv" ;;
esac

icon_name="$(gx "sed -n 's/^Icon=//p' $desktop_file | head -1" | tr -d '[:space:]')"
[ -n "$icon_name" ] || abort "the .desktop entry declares no Icon=; the grey-square case cannot be distinguished"
icon_path="/usr/share/icons/hicolor/scalable/apps/${icon_name}.svg"
if gx "test -s $icon_path"; then
    icon_bytes="$(gx "stat -c %s $icon_path" | tr -d '[:space:]')"
    ok "L6: Icon=$icon_name resolves to $icon_path ($icon_bytes bytes, non-empty)"
    gx "head -c 200 $icon_path | grep -q '<svg'" \
        && ok "L6: the icon is a real SVG, not a placeholder" \
        || notok "L6: $icon_path does not begin with an <svg element"
else
    notok "L6: Icon=$icon_name does not resolve to a non-empty file at $icon_path"
fi

# Launching it. `gtk-launch` takes the *same* desktop entry the menu uses, so
# this exercises the entry rather than the binary path.
gu 'pkill -x pliwee-gui' >/dev/null 2>&1; sleep 2
virsh -c "$GA_CONNECT" screenshot "$DOMAIN" "$EVIDENCE/10-L6-before.ppm" >/dev/null 2>&1 \
    || abort "virsh screenshot failed; the visual half of L6/L9 cannot be measured"
gu "gtk-launch io.github.yurisismotto.pliwee" >/dev/null 2>&1 &
ga_wait_for "$DOMAIN" 45 "pgrep -x pliwee-gui >/dev/null" \
    && ok "L6: the .desktop entry started pliwee-gui" \
    || notok "L6: gtk-launch of the .desktop entry started no process"
sleep 6
virsh -c "$GA_CONNECT" screenshot "$DOMAIN" "$EVIDENCE/11-L6-after.ppm" >/dev/null 2>&1

# The screenshot must differ from the pre-launch one, or nothing appeared on
# screen and a "window opened" claim would rest on a process existing.
if [ -s "$EVIDENCE/10-L6-before.ppm" ] && [ -s "$EVIDENCE/11-L6-after.ppm" ]; then
    b="$(sha256sum "$EVIDENCE/10-L6-before.ppm" | cut -d' ' -f1)"
    a="$(sha256sum "$EVIDENCE/11-L6-after.ppm" | cut -d' ' -f1)"
    [ "$b" != "$a" ] \
        && ok "L6: the guest framebuffer changed after launching the entry (a window was drawn)" \
        || notok "L6: the framebuffer is byte-identical before and after launch — nothing was drawn"
else
    abort "one of the L6 screenshots is empty; the visual assertion would be vacuous"
fi

# ---------------------------------------------------------------------------
section "L9 — tray integration"
# ---------------------------------------------------------------------------
# The gate distinguishes two correct outcomes and one wrong one: with a tray
# host there must be exactly one item; without one there must be none AND
# everything else must still work. A package that pulled in a shell extension
# would be the failure.
watcher="$(gu 'busctl --user list --no-pager 2>/dev/null | grep -c StatusNotifierWatcher || true' | tr -d '[:space:]')"
sni="$(gu 'busctl --user list --no-pager 2>/dev/null | grep -c "org.kde.StatusNotifierItem" || true' | tr -d '[:space:]')"
gu 'busctl --user list --no-pager 2>/dev/null' | save "12-L9-bus-names.txt"

if [ "${watcher:-0}" -ge 1 ] 2>/dev/null; then
    ok "L9: a StatusNotifierWatcher is present on the session bus (a tray host exists)"
    [ "${sni:-0}" -ge 1 ] 2>/dev/null \
        && ok "L9: ${sni} StatusNotifierItem registered" \
        || notok "L9: a tray host is present but Pliwee registered no StatusNotifierItem"
else
    ok "L9: no StatusNotifierWatcher — this desktop has no tray host, which is the documented GNOME case"
    daemon_ok="$(gx 'pgrep -c -x pliweed || true' | tr -d '[:space:]')"
    [ "${daemon_ok:-0}" -ge 1 ] 2>/dev/null \
        && ok "L9: with no tray, the daemon still runs — 'everything else works'" \
        || notok "L9: no tray AND no daemon"
fi

# The packages must not have installed or enabled a shell extension.
ext="$(gx 'ls /usr/share/gnome-shell/extensions 2>/dev/null | grep -ci appindicator || true' | tr -d '[:space:]')"
if [ "$PKGEXT" = "deb" ]; then
    owns_ext="$(gx 'dpkg -S /usr/share/gnome-shell/extensions 2>/dev/null | grep -c pliwee || true' | tr -d '[:space:]')"
else
    owns_ext="$(gx 'rpm -qf /usr/share/gnome-shell/extensions 2>/dev/null | grep -c pliwee || true' | tr -d '[:space:]')"
fi
[ "${owns_ext:-0}" = "0" ] \
    && ok "L9: no Pliwee package owns a GNOME Shell extension (appindicator present on system: ${ext:-0})" \
    || notok "L9: a Pliwee package owns a GNOME Shell extension path"

# ---------------------------------------------------------------------------
section "L10 — mDNS advertisement"
# ---------------------------------------------------------------------------
# What is asserted here is that the service record is on the wire from this
# guest. Whether the PHONE sees it is L12 and is taken from the phone.
if gx 'command -v avahi-browse >/dev/null 2>&1'; then
    browse="$(gx 'timeout 12 avahi-browse -rpt _pliwee._tcp 2>/dev/null' || true)"
    printf '%s\n' "$browse" | save "13-L10-avahi.txt"
    n_rec="$(printf '%s\n' "$browse" | grep -c '^=' || true)"
    [ "${n_rec:-0}" -ge 1 ] 2>/dev/null \
        && ok "L10: _pliwee._tcp.local. is resolvable on the LAN ($n_rec record(s))" \
        || notok "L10: avahi-browse resolved 0 _pliwee._tcp records"
else
    na "L10: avahi-browse is not installed in the guest; using the daemon journal instead"
fi
mdns_jnl="$(gu 'journalctl --user -u pliweed --no-pager -n 400 2>/dev/null | grep -iE "mdns|advertis|_pliwee|_omnibridge" | tail -20' || true)"
[ -n "${mdns_jnl//[[:space:]]/}" ] \
    && ok "L10: the daemon journal records mDNS advertisement ($(printf '%s\n' "$mdns_jnl" | grep -c .) line(s))" \
    || notok "L10: no mDNS line in the daemon journal"
printf '%s\n' "$mdns_jnl" | save "14-L10-journal.txt"
mdns_sock="$(gu 'ss -ulnp 2>/dev/null | grep -c ":5353" || true' | tr -d '[:space:]')"
[ "${mdns_sock:-0}" -ge 1 ] 2>/dev/null \
    && ok "L10: UDP 5353 is bound in the guest" \
    || notok "L10: nothing is bound to UDP 5353"

# ---------------------------------------------------------------------------
section "L3, L19 — enable, then a full logout/login cycle"
# ---------------------------------------------------------------------------
# Lingering would keep the user manager alive without a session and make the
# whole gate meaningless: the daemon would never actually have to come back.
# Lingering keeps a user manager alive with no session, so `terminate-user`
# would not stop the daemon and both gates would pass without measuring a
# logout at all. Where it is on, it is turned off for the duration and put
# back afterwards -- and the fact is recorded, not hidden.
LINGER_WAS="$(gx "loginctl show-user $GUEST_USER -p Linger --value 2>/dev/null" | tr -d '[:space:]')"
if [ "$LINGER_WAS" = "yes" ]; then
    gx "loginctl disable-linger $GUEST_USER" >/dev/null 2>&1
    linger="$(gx "loginctl show-user $GUEST_USER -p Linger --value 2>/dev/null" | tr -d '[:space:]')"
    [ "$linger" = "no" ] \
        || abort "linger could not be turned off for $GUEST_USER; L3/L19 would measure nothing"
    ok "L3: linger was ON and has been turned off for this run (restored at the end)"
else
    linger="$LINGER_WAS"
    [ "$linger" = "no" ] \
        || abort "linger is '$linger' for $GUEST_USER; a logout would not stop the user manager"
    ok "L3: linger is off, so a logout really does tear the user manager down"
fi

gu 'systemctl --user enable pliweed.service' >/dev/null 2>&1
en="$(gu 'systemctl --user is-enabled pliweed.service' | tr -d '[:space:]')"
[ "$en" = "enabled" ] \
    && ok "L3: the unit is now enabled for $GUEST_USER" \
    || abort "could not enable the unit (is-enabled=$en); L3 cannot proceed"
gx "test -L /home/$GUEST_USER/.config/systemd/user/default.target.wants/pliweed.service" \
    && ok "L3: the enable created the user's own default.target.wants symlink" \
    || notok "L3: no default.target.wants/pliweed.service symlink"

pid_before="$(gx 'pgrep -x pliweed | head -1' | tr -d '[:space:]')"
[ -n "$pid_before" ] || abort "no pliweed pid before the session cycle; nothing to compare against"
ok "L3/L19: pliweed pid before the cycle is $pid_before"

gx "loginctl terminate-user $GUEST_USER" >/dev/null 2>&1
if ga_wait_for "$DOMAIN" 90 "! pgrep -u $GUEST_UID -f 'systemd --user' >/dev/null"; then
    ok "L19: the session and its user manager were torn down"
else
    notok "L19: the user manager survived terminate-user; the logout half did not happen"
fi

# The login half. After terminate-user, GDM presents its GREETER rather than
# re-running autologin -- MEASURED: `loginctl list-sessions` shows
# `c1 120 gdm seat0 tty1` and no user session. There is no account password
# available to type into that greeter, so the login is driven by restarting
# the display manager, which runs autologin again.
#
# The cycle is still real and the gate still measures what it claims: the old
# session and its user manager are gone (asserted above), and what comes back
# is a DIFFERENT session id with a DIFFERENT user manager and a different
# daemon pid (asserted below). How the greeter was dismissed does not change
# any of that.
sess_id_before="$(gx "loginctl show-user $GUEST_USER -p Display --value 2>/dev/null" | tr -d '[:space:]')"
if ! ga_wait_for "$DOMAIN" 45 "loginctl list-sessions --no-legend 2>/dev/null | grep -q ' $GUEST_USER '"; then
    greeter="$(gx "loginctl list-sessions --no-legend 2>/dev/null" || true)"
    printf '%s\n' "$greeter" | save "17-L19-greeter.txt"
    ok "L19: after the logout the seat holds only the display manager's greeter: $(printf '%s' "$greeter" | tr '\n' ';')"
    gx 'systemctl restart gdm3 2>/dev/null || systemctl restart gdm 2>/dev/null || systemctl restart sddm 2>/dev/null' >/dev/null 2>&1
    ok "L19: the display manager was restarted to run autologin (no account password is available for the greeter)"
fi
if ga_wait_for "$DOMAIN" 240 "loginctl list-sessions --no-legend 2>/dev/null | grep -q ' $GUEST_USER '"; then
    sess_id_after="$(gx "loginctl show-user $GUEST_USER -p Display --value 2>/dev/null" | tr -d '[:space:]')"
    ok "L19: a session for $GUEST_USER came back (session $sess_id_before -> $sess_id_after)"
    [ -n "$sess_id_after" ] && [ "$sess_id_after" != "$sess_id_before" ] \
        && ok "L19: it is a NEW logind session, not the old one resumed" \
        || notok "L19: the session id did not change ($sess_id_before -> $sess_id_after)"
else
    abort "no session came back within 240s; L3/L19 cannot be completed"
fi
ga_wait_for "$DOMAIN" 120 "pgrep -u $GUEST_UID -f 'systemd --user' >/dev/null" \
    || abort "the user manager did not come back"

if ga_wait_for "$DOMAIN" 120 "pgrep -x pliweed >/dev/null"; then
    pid_after="$(gx 'pgrep -x pliweed | head -1' | tr -d '[:space:]')"
    ok "L3: the daemon autostarted after login without being asked (pid $pid_after)"
    [ "$pid_after" != "$pid_before" ] \
        && ok "L3: it is a NEW process ($pid_before -> $pid_after), so the restart is real" \
        || notok "L3: the pid is unchanged — the daemon never actually stopped"
else
    notok "L3: pliweed did not autostart after login"
fi
active_after="$(gu 'systemctl --user is-active pliweed.service' | tr -d '[:space:]')"
[ "$active_after" = "active" ] \
    && ok "L3: systemctl --user is-active reports active after login" \
    || notok "L3: after login the unit is '$active_after'"

# L19's second half: cold activation must still work in the NEW session.
gu 'pkill -x pliwee-gui' >/dev/null 2>&1; sleep 2
act2="$(gu 'gdbus call --session --dest io.github.yurisismotto.pliwee --object-path /io/github/yurisismotto/pliwee --method org.freedesktop.DBus.Peer.Ping 2>&1' || true)"
printf '%s\n' "$act2" | save "15-L19-activation.txt"
grep -q '()' <<<"$act2" \
    && ok "L19: D-Bus cold activation still works in the new session" \
    || notok "L19: cold activation after relogin returned: ${act2:-<empty>}"

# ---------------------------------------------------------------------------
section "L20 — reboot"
# ---------------------------------------------------------------------------
boot_before="$(gx 'cat /proc/sys/kernel/random/boot_id' | tr -d '[:space:]')"
[ -n "$boot_before" ] || abort "could not read boot_id; a reboot could not be proved to have happened"
gx 'systemd-run --on-active=1 --timer-property=AccuracySec=100ms systemctl reboot' >/dev/null 2>&1 \
    || gx 'nohup sh -c "sleep 1; systemctl reboot" >/dev/null 2>&1 &' >/dev/null 2>&1
sleep 15
ga_ping "$DOMAIN" 420 || abort "the guest did not come back from the reboot within 420s"
boot_after="$(gx 'cat /proc/sys/kernel/random/boot_id' | tr -d '[:space:]')"
[ -n "$boot_after" ] && [ "$boot_after" != "$boot_before" ] \
    && ok "L20: the guest really rebooted (boot_id $boot_before -> $boot_after)" \
    || abort "boot_id did not change; no reboot happened and every L20 assertion below would be false evidence"

ga_wait_for "$DOMAIN" 300 "loginctl list-sessions --no-legend 2>/dev/null | grep -q ' $GUEST_USER '" \
    || abort "no session after reboot"
ok "L20: a graphical session for $GUEST_USER exists after reboot"

# L20 is defined as "L3, L5, L7, L10 all still pass".
if ga_wait_for "$DOMAIN" 180 "pgrep -x pliweed >/dev/null"; then
    ok "L20/L3: the daemon autostarted after the reboot"
else
    notok "L20/L3: pliweed did not autostart after the reboot"
fi
st_dir="$(gx "stat -c '%a %U' $rt_dir 2>&1")"
st_sock="$(gx "stat -c '%a %U' $rt_dir/control.sock 2>&1")"
[ "$st_dir" = "700 $GUEST_USER" ] && ok "L20/L5: runtime directory is 0700 $GUEST_USER after reboot" \
    || notok "L20/L5: runtime directory after reboot is '$st_dir'"
[ "$st_sock" = "600 $GUEST_USER" ] && ok "L20/L5: control.sock is 0600 $GUEST_USER after reboot" \
    || notok "L20/L5: control.sock after reboot is '$st_sock'"

gu 'pkill -x pliwee-gui' >/dev/null 2>&1; sleep 2
act3="$(gu 'gdbus call --session --dest io.github.yurisismotto.pliwee --object-path /io/github/yurisismotto/pliwee --method org.freedesktop.DBus.Peer.Ping 2>&1' || true)"
printf '%s\n' "$act3" | save "16-L20-activation.txt"
grep -q '()' <<<"$act3" \
    && ok "L20/L7: D-Bus cold activation works after the reboot" \
    || notok "L20/L7: cold activation after reboot returned: ${act3:-<empty>}"

mdns_after="$(gu 'ss -ulnp 2>/dev/null | grep -c ":5353" || true' | tr -d '[:space:]')"
[ "${mdns_after:-0}" -ge 1 ] 2>/dev/null \
    && ok "L20/L10: UDP 5353 is bound again after the reboot" \
    || notok "L20/L10: UDP 5353 is not bound after the reboot"
tcp_after="$(gu 'ss -tlnp 2>/dev/null | grep -c ":55432" || true' | tr -d '[:space:]')"
[ "${tcp_after:-0}" -ge 1 ] 2>/dev/null \
    && ok "L20/L11: TCP 55432 is listening again after the reboot" \
    || notok "L20/L11: TCP 55432 is not listening after the reboot"

# ---------------------------------------------------------------------------
section "L13, L17 — upgrade"
# ---------------------------------------------------------------------------
# One published build exists per Debian target, so there is nothing to upgrade
# FROM. That is recorded as not-applicable with its reason rather than skipped
# silently -- a group that vanishes without saying so is how L17 passed
# vacuously in Packaging v1.
if [ "${#PKGS[@]}" -gt 0 ] && [ "$PKGEXT" = "deb" ]; then
    na "L13/L17: only one published .deb build ($(printf '%s' "${PKGS[0]}" | sed 's/.*_\([0-9].*\)_amd64.deb/\1/')) exists for $DISTRO — there is no earlier version to upgrade from"
else
    na "L13/L17: no second build staged for $DISTRO"
fi

# ---------------------------------------------------------------------------
section "L21–L26 — remove, reinstall, purge, and the user's trust store"
# ---------------------------------------------------------------------------
STATE_DIR="/home/$GUEST_USER/.local/share/pliwee"
gx "test -f $STATE_DIR/identity.key" \
    || abort "no $STATE_DIR/identity.key in the guest; every trust-store assertion below would compare two absent files"
before_state="$(gx "cd $STATE_DIR && sha256sum * 2>/dev/null | sort; stat -c '%a %U %n' . * 2>/dev/null | sort")"
[ -n "${before_state//[[:space:]]/}" ] \
    || abort "the trust-store fingerprint is empty; L23 would be vacuous"
n_state="$(printf '%s\n' "$before_state" | grep -c . || true)"
printf '%s\n' "$before_state" | save "20-L23-state-before.txt"
ok "L23: trust store fingerprinted before removal ($n_state line(s), digest+mode+owner)"

gu 'systemctl --user stop pliweed.service' >/dev/null 2>&1

if [ "$PKGEXT" = "deb" ]; then
    rm_out="$(gx 'DEBIAN_FRONTEND=noninteractive apt-get remove -y pliwee-gui pliwee 2>&1')"
else
    rm_out="$(gx 'dnf remove -y pliwee-gui pliwee 2>&1')"
fi
rm_rc=$?
printf '%s\n' "$rm_out" | save "21-L21-remove.txt"
[ "$rm_rc" -eq 0 ] && ok "L21: remove exited 0" || notok "L21: remove exited $rm_rc"

for f in /usr/bin/pliweed /usr/bin/pliwee /usr/bin/pliwee-gui \
         /usr/lib/systemd/user/pliweed.service \
         /usr/share/applications/io.github.yurisismotto.pliwee.desktop \
         /usr/share/dbus-1/services/io.github.yurisismotto.pliwee.service; do
    gx "test -e $f" \
        && notok "L25: $f survived removal" \
        || ok "L25: $f is gone"
done

after_remove="$(gx "cd $STATE_DIR && sha256sum * 2>/dev/null | sort; stat -c '%a %U %n' . * 2>/dev/null | sort")"
[ "$after_remove" = "$before_state" ] \
    && ok "L21/L23: the trust store is byte-, mode- and owner-identical after remove" \
    || notok "L21/L23: the trust store CHANGED across remove"

# L22 reinstall
if [ "$PKGEXT" = "deb" ]; then
    ri_out="$(gx 'cd /root/pliwee-pkgs && DEBIAN_FRONTEND=noninteractive apt-get install -y ./pliwee_*_amd64.deb ./pliwee-gui_*_amd64.deb 2>&1')"
else
    ri_out="$(gx 'cd /root/pliwee-pkgs && dnf install -y --disablerepo="*" ./pliwee-[0-9]*.x86_64.rpm ./pliwee-gui-[0-9]*.x86_64.rpm 2>&1')"
fi
ri_rc=$?
printf '%s\n' "$ri_out" | save "22-L22-reinstall.txt"
[ "$ri_rc" -eq 0 ] && ok "L22: reinstall exited 0" || notok "L22: reinstall exited $ri_rc"
gx 'test -x /usr/bin/pliweed' \
    && ok "L22: the daemon binary is back" \
    || notok "L22: /usr/bin/pliweed is not present after reinstall"

after_reinstall="$(gx "cd $STATE_DIR && sha256sum * 2>/dev/null | sort; stat -c '%a %U %n' . * 2>/dev/null | sort")"
[ "$after_reinstall" = "$before_state" ] \
    && ok "L22/L23: the trust store is unchanged after reinstall" \
    || notok "L22/L23: the trust store CHANGED across reinstall"

# The daemon must find the SAME identity it had before the package left.
gu 'systemctl --user start pliweed.service' >/dev/null 2>&1
if ga_wait_for "$DOMAIN" 60 "pgrep -x pliweed >/dev/null"; then
    ok "L22: the daemon starts again after reinstall"
    id_now="$(gx "sha256sum $STATE_DIR/identity.key | cut -d' ' -f1" | tr -d '[:space:]')"
    id_was="$(printf '%s\n' "$before_state" | grep 'identity.key' | head -1 | cut -d' ' -f1)"
    [ -n "$id_now" ] && [ "$id_now" = "$id_was" ] \
        && ok "L22: it found the same identity key it had before removal" \
        || notok "L22: identity.key digest differs after reinstall ($id_was -> $id_now)"
else
    notok "L22: the daemon did not start after reinstall"
fi

# L24 purge — DEB only, and the gate that matters most.
if [ "$PKGEXT" = "deb" ]; then
    gu 'systemctl --user stop pliweed.service' >/dev/null 2>&1
    pg_out="$(gx 'DEBIAN_FRONTEND=noninteractive apt-get purge -y pliwee-gui pliwee 2>&1')"
    pg_rc=$?
    printf '%s\n' "$pg_out" | save "23-L24-purge.txt"
    [ "$pg_rc" -eq 0 ] && ok "L24: purge exited 0" || notok "L24: purge exited $pg_rc"
    gx "test -d $STATE_DIR" \
        && ok "L24: $STATE_DIR still exists after PURGE" \
        || notok "L24: PURGE removed the user's trust store directory"
    after_purge="$(gx "cd $STATE_DIR && sha256sum * 2>/dev/null | sort; stat -c '%a %U %n' . * 2>/dev/null | sort")"
    [ "$after_purge" = "$before_state" ] \
        && ok "L24/L23: the trust store is byte-, mode- and owner-identical after PURGE" \
        || notok "L24/L23: the trust store CHANGED across purge"
    printf '%s\n' "$after_purge" | save "24-L23-state-after-purge.txt"
    conff="$(gx 'dpkg-query -W -f "\${Conffiles}\n" pliwee pliwee-gui 2>/dev/null | grep -c . || true' | tr -d '[:space:]')"
    [ "${conff:-0}" = "0" ] \
        && ok "L24: no conffile left registered for either package" \
        || notok "L24: ${conff} conffile entries remain"
else
    na "L24: purge is a dpkg concept; RPM has no equivalent transaction"
fi

# L26 — nothing in the user's state may be root-owned, on any path taken above.
foreign="$(gx "find $STATE_DIR /home/$GUEST_USER/Downloads/Pliwee /run/user/$GUEST_UID/pliwee ! -user $GUEST_USER 2>/dev/null | head -20" || true)"
[ -z "${foreign//[[:space:]]/}" ] \
    && ok "L26: no file in the user's Pliwee state is owned by anyone but $GUEST_USER" \
    || notok "L26: $(printf '%s\n' "$foreign" | grep -c .) path(s) in the user's state are not owned by $GUEST_USER"
printf '%s\n' "${foreign:-<none>}" | save "25-L26-foreign-owned.txt"

# ---------------------------------------------------------------------------
section "Restoring guest state this run changed"
# ---------------------------------------------------------------------------
if [ "${LINGER_WAS:-no}" = "yes" ]; then
    gx "loginctl enable-linger $GUEST_USER" >/dev/null 2>&1
    back="$(gx "loginctl show-user $GUEST_USER -p Linger --value 2>/dev/null" | tr -d '[:space:]')"
    [ "$back" = "yes" ] \
        && ok "linger restored to its pre-run value (yes)" \
        || notok "linger was 'yes' before the run and is now '$back'"
else
    ok "linger was off before the run and was not changed"
fi

# ---------------------------------------------------------------------------
printf '\n-----------------------------------------------\n'
printf '%s / %s: %d passed, %d failed, %d n/a\n' "$DISTRO" "$guest_os" "$PASS" "$FAIL" "$NA"
if [ "$FAIL" -gt 0 ]; then
    printf '\nFailed:\n'
    for g in "${FAILED_GATES[@]}"; do printf '  %s\n' "$g"; done
fi
printf 'evidence: %s\n' "$EVIDENCE"
[ "$FAIL" -eq 0 ]
