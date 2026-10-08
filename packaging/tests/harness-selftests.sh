#!/usr/bin/env bash
# harness-selftests.sh — does the harness fail when it should?
#
# Every certification gate in this repository rests on an assumption nobody had
# tested: that when the thing being measured is absent, the harness says so.
# Twenty-nine times across four waves, it did not. This file tests that
# assumption directly.
#
# Each case does two things, and BOTH matter:
#
#   REJECTS  the primitive returns non-zero on its failure mode
#   ACCEPTS  the primitive returns zero on the good case
#
# Without the second half a primitive hard-coded to `return 1` would pass every
# rejection test here — which is the same vacuity in a new place. Without the
# first half the whole file is decoration.
#
# The failure modes are the ones actually observed, named in
# packaging/tests/lib/assert.sh against the wave that suffered them.

set -uo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/assert.sh
. "$HERE/lib/assert.sh"

PASS=0; FAIL=0; declare -a FAILED=()
ok()      { PASS=$(( PASS + 1 )); printf 'ok    %s\n' "$*"; }
notok()   { FAIL=$(( FAIL + 1 )); FAILED+=("$*"); printf 'not ok  %s\n' "$*"; }
section() { printf '\n== %s ==\n' "$*"; }

# rejects DESC CMD... — the primitive must return non-zero, and must say why.
rejects() {
    local desc="$1"; shift
    local out rc
    out="$("$@" 2>&1)"; rc=$?
    if [ "$rc" -eq 0 ]; then
        notok "REJECTS $desc — returned 0; the harness would have passed on nothing"
    elif [ -z "${out//[[:space:]]/}" ]; then
        notok "REJECTS $desc — returned $rc but printed no diagnostic; a silent failure is hard to act on"
    elif grep -qiE 'syntax error|command not found|no such file' <<<"$out"; then
        # The rejection must come from the PRIMITIVE, not from a shell that
        # could not load it. Measured: on an Ubuntu runner `/bin/sh` is dash,
        # which cannot parse assert.sh's arrays and here-strings, so a case
        # spawned with `sh -c` returned non-zero for a reason that had nothing
        # to do with the thing under test -- and this file recorded it as a
        # pass. A false green inside the suite whose subject is false greens.
        notok "REJECTS $desc — returned $rc because the SHELL failed, not the primitive: $(tr '\n' ' ' <<<"$out" | head -c 110)"
    else
        ok "REJECTS $desc — $(printf '%s' "$out" | sed 's/^assert: FAILED: //' | tr '\n' ' ' | head -c 105)"
    fi
}
# The two kinds are checked differently, on purpose.
#
#   need_*            ASSERTIONS. They must return non-zero AND print why, because
#                     whoever reads the run needs to know what was missing.
#   contains/absent   PREDICATES. They must return the right answer and print
#                     NOTHING, because the caller composes them into its own
#                     message: `contains "$cap" "$s" || notok "the window missed it"`.
#                     Requiring a diagnostic from these would push duplicate text
#                     into every call site.
#
# is_false/is_true check a predicate's answer alone.
is_false() {
    local desc="$1"; shift
    if "$@" >/dev/null 2>&1; then
        notok "PREDICATE $desc — answered true, and the truth is false"
    else
        ok "PREDICATE $desc — answers false, as it must"
    fi
}
is_true() {
    local desc="$1"; shift
    if "$@" >/dev/null 2>&1; then
        ok "PREDICATE $desc — answers true, as it must"
    else
        notok "PREDICATE $desc — answered false, and the truth is true"
    fi
}

# accepts DESC CMD... — the primitive must return zero on the good case.
accepts() {
    local desc="$1"; shift
    if "$@" >/dev/null 2>&1; then
        ok "ACCEPTS $desc"
    else
        notok "ACCEPTS $desc — returned non-zero on a case that is fine; a primitive that rejects everything proves nothing"
    fi
}

WORK="$(mktemp -d "${TMPDIR:-/tmp}/pliwee-selftest.XXXXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# ---------------------------------------------------------------------------
section "A missing executable  (Packaging v1: an absent runuser)"
# ---------------------------------------------------------------------------
rejects "a tool that is not installed" \
    need_tool omnibridge-definitely-not-a-real-tool-4f9a
rejects "one absent tool among several present ones" \
    need_tool sh grep omnibridge-definitely-not-a-real-tool-4f9a
accepts "tools that are installed" need_tool sh grep find

# ---------------------------------------------------------------------------
section "An empty capture  (Packaging v1: an empty tracing capture; L16: one line)"
# ---------------------------------------------------------------------------
rejects "an empty capture"            need_nonempty "the journal" ""
rejects "a whitespace-only capture"   need_nonempty "the journal" $'  \n\t\n  '
rejects "a capture below the minimum" need_nonempty "the journal" $'one line' 20
accepts "a capture with real content" need_nonempty "the journal" $'line one\nline two'
accepts "a capture meeting an explicit minimum" \
    need_nonempty "the journal" "$(seq 1 30)" 20

# ---------------------------------------------------------------------------
section "A search that loses a match  (Evidence closure: 225 misses in 300 runs)"
# ---------------------------------------------------------------------------
# The capture is deliberately large and the match deliberately early: that is
# the exact shape that made `printf | grep -q` fail three times in four.
BIG="$(seq 1 4000 | sed 's/^/filler line /')"
BIG="MATCHME-EARLY-SENTINEL
$BIG"
is_true  "contains(), on a sentinel near the start of a 4000-line capture" \
    contains "$BIG" "MATCHME-EARLY-SENTINEL"
is_false "contains(), on a string the capture does not hold" \
    contains "$BIG" "THIS-STRING-IS-NOT-THERE-91af"
is_true  "absent(), on a string that really is absent" \
    absent "$BIG" "THIS-STRING-IS-NOT-THERE-91af"
is_false "absent(), on a string that is present — the privacy-gate direction" \
    absent "$BIG" "MATCHME-EARLY-SENTINEL"

# The regression itself: run the match many times and require it never to be
# lost. Under the old `printf | grep -q` form this failed ~75% of iterations.
miss=0
for _ in $(seq 1 200); do contains "$BIG" "MATCHME-EARLY-SENTINEL" || miss=$(( miss + 1 )); done
[ "$miss" -eq 0 ] \
    && ok "REGRESSION: 200/200 searches of a large capture found a present sentinel (the pipe form missed 75%)" \
    || notok "REGRESSION: $miss of 200 searches LOST a present sentinel; the pipefail/SIGPIPE defect is back"

# ---------------------------------------------------------------------------
section "A window that does not cover the operation  (defects 2, 9, 19)"
# ---------------------------------------------------------------------------
rejects "a window with no anchor for this operation" \
    need_window_covers "the journal" $'unrelated line\nanother unrelated line' "transfer=abc12345"
rejects "an empty window, before any anchor is even looked for" \
    need_window_covers "the journal" "" "transfer=abc12345"
accepts "a window carrying this operation's own anchor" \
    need_window_covers "the journal" $'noise\noffering a file transfer=abc12345 size=48\nnoise' "transfer=abc12345"

# ---------------------------------------------------------------------------
section "A wrong artifact count  (Packaging v1: six built, one shipped)"
# ---------------------------------------------------------------------------
rejects "a count that is wrong"                need_exact_count "installed packages" "1" "2"
rejects "a count that is zero"                 need_exact_count "installed packages" "0" "2"
rejects "a count that could not be read"       need_exact_count "installed packages" "" "2"
accepts "a count that is exactly right"        need_exact_count "installed packages" "2" "2"
accepts "a count with the whitespace guests add" need_exact_count "installed packages" $' 2 \n' "2"

# ---------------------------------------------------------------------------
section "A glob that matched nothing  (Packaging v1: L17 skipped its group)"
# ---------------------------------------------------------------------------
mkdir -p "$WORK/pkgs"
: > "$WORK/pkgs/pliwee_1.0_amd64.deb"
: > "$WORK/pkgs/pliwee-gui_1.0_amd64.deb"
rejects "a glob that matches nothing"        need_glob "$WORK/pkgs" '*.rpm' 2
rejects "a glob that matches the wrong number" need_glob "$WORK/pkgs" '*.deb' 3
rejects "a directory that does not exist"    need_glob "$WORK/nope" '*.deb' 2
accepts "a glob that matches exactly the expected count" need_glob "$WORK/pkgs" '*.deb' 2

# ---------------------------------------------------------------------------
section "A relative mount path  (Packaging v1: read as a named volume)"
# ---------------------------------------------------------------------------
rejects "a relative path used as a mount source"  need_abs_path "--pkgdir" "artifacts/fedora44"
rejects "a bare name used as a mount source"      need_abs_path "--pkgdir" "artifacts"
accepts "an absolute mount source"                need_abs_path "--pkgdir" "/tmp/artifacts"

# ---------------------------------------------------------------------------
section "A directory nothing can write to  (Packaging v1: the subuid case)"
# ---------------------------------------------------------------------------
mkdir -p "$WORK/ro" && chmod 500 "$WORK/ro"
if [ "$(id -u)" -eq 0 ]; then
    printf 'n/a   running as root, which can write to a 0500 directory; the unwritable case cannot be staged\n'
else
    rejects "a directory the harness cannot write to" need_writable "$WORK/ro"
fi
accepts "a writable directory" need_writable "$WORK"

# ---------------------------------------------------------------------------
section "An operation that never ran  (defects 10, 11: the session was torn down)"
# ---------------------------------------------------------------------------
rejects "a counter that did not move across the operation" need_ran "mirrored" "3" "3"
rejects "a before/after observation that is missing"       need_ran "mirrored" "3" ""
accepts "a counter that moved"                             need_ran "mirrored" "3" "4"

# ---------------------------------------------------------------------------
section "A delta that is not the one claimed  (defect 14: 'mirrored now' is not a total)"
# ---------------------------------------------------------------------------
rejects "no change where exactly one was expected"   need_delta "mirrored" "3" "3" 1
rejects "two arrivals counted as the one under test" need_delta "mirrored" "3" "5" 1
rejects "a non-numeric observation"                  need_delta "mirrored" "three" "4" 1
accepts "exactly the expected delta"                 need_delta "mirrored" "3" "4" 1
accepts "a delta of one from a cleared baseline"     need_delta "mirrored" "0" "1" 1

# ---------------------------------------------------------------------------
section "A prompt with no stdin  (Packaging v1 bash -s; defect 4: silent decline)"
# ---------------------------------------------------------------------------
# The mechanical half. `pliwee pair` read EOF from its [y/N] prompt and
# answered "no" while exiting 0, and two operator scans were lost before one
# journal line explained it. What a harness can check before starting such a
# command is that its stdin is not already closed.
# `bash -c`, not `sh -c`: lib/assert.sh declares `#!/usr/bin/env bash` and uses
# arrays and here-strings. On an Ubuntu runner /bin/sh is dash, which cannot
# parse it -- see the note in rejects() for what that cost.
accepts "a command given a real answer on stdin" \
    bash -c '. '"$HERE"'/lib/assert.sh; need_stdin_answer "pliwee pair" <<<"y"'
rejects "a command whose stdin is closed" \
    bash -c '. '"$HERE"'/lib/assert.sh; exec 0<&-; need_stdin_answer "pliwee pair"'

# ---------------------------------------------------------------------------
section "U10 before U6  (pre-G8 gate hardening: U6 recorded n/a, then U10 ran)"
# ---------------------------------------------------------------------------
# upgrade-gates.sh used to record U6 as n/a and downgrade (U10) in the same
# stage, so U6 could never be measured against the upgraded guest in the
# order §3 defines. The stages are now separate, and U10 stands behind
# g7up_verify_u6. It must refuse every U6 that is not a real PASS on the same
# guest and run — and accept the one that is, or it proves nothing.
# shellcheck source=lib/g7up-evidence.sh
. "$HERE/lib/g7up-evidence.sh"
# shellcheck source=lib/g7up-fixture.sh
. "$HERE/lib/g7up-fixture.sh"
G7="$WORK/g7"; mkdir -p "$G7"
g7up_fixture "$G7/base" fedora44 g7-f44
u6case() { # NAME SHELL-EDIT — a copy of the good record with one thing broken ($E is the copy)
    rm -rf "${G7:?}/$1"; cp -a "$G7/base" "$G7/$1"
    E="$G7/$1" bash -c ". '$HERE/lib/g7up-fixture.sh'; $2"
}
accepts "a real U6 PASS on the same distro, domain, run and guest" \
    g7up_verify_u6 "$G7/base" fedora44 g7-f44
u6case no-u6 'rm -rf "$E/U6" "$E/U6-RESULT"'
rejects "U10 with no U6 at all (the old order: upgrade, then straight to the downgrade)" \
    g7up_verify_u6 "$G7/no-u6" fedora44 g7-f44
u6case empty-result ': > "$E/U6-RESULT"'
rejects "an empty U6-RESULT file" g7up_verify_u6 "$G7/empty-result" fedora44 g7-f44
u6case bare-pass 'printf "verdict=PASS\n" > "$E/U6-RESULT"'
rejects "a hand-written 'verdict=PASS' and nothing else" g7up_verify_u6 "$G7/bare-pass" fedora44 g7-f44
u6case no-checkpoint 'rm -f "$E/UPGRADE-CHECKPOINT"'
rejects "a U6 record with no upgrade checkpoint behind it" g7up_verify_u6 "$G7/no-checkpoint" fedora44 g7-f44
u6case altered-log 'echo "ok    L15: something added later" >> "$E/U6/lifecycle-peer-gates.log"'
rejects "a U6 log altered after it was recorded" g7up_verify_u6 "$G7/altered-log" fedora44 g7-f44
u6case empty-log ': > "$E/U6/lifecycle-peer-gates.log"; g7up_fixture_rehash "$E"'
rejects "an empty U6 log, even with a matching digest" g7up_verify_u6 "$G7/empty-log" fedora44 g7-f44
u6case peer-failed 'sed -i "s/: 34 passed, 0 failed/: 33 passed, 1 failed/" "$E/U6/lifecycle-peer-gates.log"; echo "not ok  L15: no incoming-file prompt" >> "$E/U6/lifecycle-peer-gates.log"; g7up_fixture_rehash "$E"'
rejects "a U6 run in which lifecycle-peer-gates.sh failed a check" g7up_verify_u6 "$G7/peer-failed" fedora44 g7-f44
u6case aborted 'echo "PRECONDITION FAILED: pliweed is not running in the guest" >> "$E/U6/lifecycle-peer-gates.log"; g7up_fixture_rehash "$E"'
rejects "a U6 run that stopped on a precondition" g7up_verify_u6 "$G7/aborted" fedora44 g7-f44
u6case repaired 'sed -i "/already paired with this guest/d" "$E/U6/lifecycle-peer-gates.log"; echo "ok    the phone paired with g7-host after 20s" >> "$E/U6/lifecycle-peer-gates.log"; g7up_fixture_rehash "$E"'
rejects "a U6 that re-paired the phone (U6 is 'reconnects WITHOUT re-pairing')" g7up_verify_u6 "$G7/repaired" fedora44 g7-f44
u6case one-way 'sed -i "s/^ok    L14: phone -> guest arrived.*/n\/a   L14: phone -> guest NOT EXERCISED. The Android clipboard is empty/" "$E/U6/lifecycle-peer-gates.log"; g7up_fixture_rehash "$E"'
rejects "a clipboard that went one way only (n/a is not a round-trip)" g7up_verify_u6 "$G7/one-way" fedora44 g7-f44
u6case no-xfer-journal ': > "$E/U6/43b-L15-journal.txt"'
rejects "a file transfer with no journal line naming its id" g7up_verify_u6 "$G7/no-xfer-journal" fedora44 g7-f44
u6case wrong-domain 'sed -i "s/^domain=.*/domain=g7-u2404/" "$E/U6-RESULT"'
rejects "a U6 recorded for another domain" g7up_verify_u6 "$G7/wrong-domain" fedora44 g7-f44
u6case wrong-distro-id 'sed -i "s/^distro .*/distro        ubuntu2404/" "$E/U6/29-peer-identity.txt"; g7up_fixture_rehash "$E"'
rejects "a U6 whose peer gates measured another distribution" g7up_verify_u6 "$G7/wrong-distro-id" fedora44 g7-f44
u6case wrong-domain-tally 'sed -i "s|^fedora44 / g7-f44: |fedora44 / g7-f44-clone: |" "$E/U6/lifecycle-peer-gates.log"; g7up_fixture_rehash "$E"'
rejects "a U6 log whose tally names another guest" g7up_verify_u6 "$G7/wrong-domain-tally" fedora44 g7-f44
rejects "the right U6 asked about as another distro" g7up_verify_u6 "$G7/base" ubuntu2404 g7-f44
rejects "the right U6 asked about as another domain" g7up_verify_u6 "$G7/base" fedora44 g7-u2404
u6case old-run 'sed -i "s/^run_id=.*/run_id=g7up-fedora44-20260101T000000Z-00000001/" "$E/U6-RESULT"'
rejects "a U6 from an earlier upgrade run" g7up_verify_u6 "$G7/old-run" fedora44 g7-f44
u6case other-guest 'sed -i "s/^guest_machine_id=.*/guest_machine_id=ffffffffffffffffffffffffffffffff/" "$E/U6-RESULT"'
rejects "a U6 measured on another machine" g7up_verify_u6 "$G7/other-guest" fedora44 g7-f44
u6case other-fpr 'sed -i "s/^fingerprint .*/fingerprint   9999 9999 9999 9999/" "$E/U6/29-peer-identity.txt"; g7up_fixture_rehash "$E"'
rejects "a U6 whose peer saw a different local fingerprint than the upgraded guest's" g7up_verify_u6 "$G7/other-fpr" fedora44 g7-f44
u6case before-upgrade 'sed -i "s/^started_epoch=.*/started_epoch=1/" "$E/U6-RESULT"'
rejects "a U6 that started before the upgrade stage completed" g7up_verify_u6 "$G7/before-upgrade" fedora44 g7-f44
u6case upgrade-failed 'sed -i "s/^upgrade_not_ok=.*/upgrade_not_ok=2/" "$E/UPGRADE-CHECKPOINT"'
rejects "a U6 on a run whose upgrade stage failed checks" g7up_verify_u6 "$G7/upgrade-failed" fedora44 g7-f44
u6case nonzero-exit 'sed -i "s/^exit=.*/exit=1/" "$E/U6-RESULT"'
rejects "a U6 whose lifecycle-peer-gates.sh exited non-zero" g7up_verify_u6 "$G7/nonzero-exit" fedora44 g7-f44

# The stages themselves. A fake `virsh` records every call: a refusal that
# happens before the guest is contacted leaves it empty, and the acceptance
# case must reach the guest (and then stop, since there is none).
mkdir -p "$WORK/bin"
printf '#!/bin/sh\necho "$*" >> "%s/virsh.calls"\nexit 1\n' "$WORK" > "$WORK/bin/virsh"; chmod +x "$WORK/bin/virsh"
command -v jq >/dev/null 2>&1 || { printf '#!/bin/sh\nexit 0\n' > "$WORK/bin/jq"; chmod +x "$WORK/bin/jq"; }
mkdir -p "$WORK/old"; printf '%s  omnibridge_1.0.0-1_amd64.deb\n' "$(printf 'a%.0s' $(seq 1 64))" > "$WORK/old/SHA256SUMS"
g7up_fixture "$G7/stage-ok" fedora44 g7-f44 "$WORK/old/SHA256SUMS"
# Stage cases start from stage-ok, whose 1.0.0 set matches --old-pkgdir, so
# each is refused for the ONE thing it breaks and not for the set's digest.
stcase() { # NAME SHELL-EDIT
    rm -rf "${G7:?}/$1"; cp -a "$G7/stage-ok" "$G7/$1"
    E="$G7/$1" bash -c ". '$HERE/lib/g7up-fixture.sh'; $2"
}
stcase st-no-u6 'rm -rf "$E/U6" "$E/U6-RESULT"'
stcase st-empty ': > "$E/U6-RESULT"'
stcase st-bare 'printf "verdict=PASS\n" > "$E/U6-RESULT"'
stcase st-wrong-domain 'sed -i "s/^domain=.*/domain=g7-u2404/" "$E/U6-RESULT"'
stcase st-wrong-distro 'sed -i "s/^distro .*/distro        ubuntu2404/" "$E/U6/29-peer-identity.txt"; g7up_fixture_rehash "$E"'
stcase st-upgrade-failed 'sed -i "s/^upgrade_not_ok=.*/upgrade_not_ok=2/" "$E/UPGRADE-CHECKPOINT"'
stage() { # EXPECT(refuse|reach) DESC MESSAGE-FRAGMENT ARGS...
    local expect="$1" desc="$2" frag="$3" out rc calls; shift 3
    rm -f "$WORK/virsh.calls"
    out="$(PATH="$WORK/bin:$PATH" GA_EXEC_TIMEOUT=5 bash "$HERE/upgrade-gates.sh" "$@" 2>&1)"; rc=$?
    calls="$(cat "$WORK/virsh.calls" 2>/dev/null || true)"
    if [ "$expect" = refuse ]; then
        if [ "$rc" -ne 0 ] && [ -z "$calls" ] && contains "$out" "$frag"; then
            ok "STAGE refuses $desc — exit $rc before contacting the guest: $(grep -m1 -F "$frag" <<<"$out" | cut -c1-90)"
        else
            notok "STAGE refuses $desc — exit $rc, virsh calls: ${calls:-none}, output: $(tr '\n' ' ' <<<"$out" | cut -c1-160)"
        fi
    else
        if [ -n "$calls" ] && ! contains "$out" "U10 refused" && ! contains "$out" "U6 refused"; then
            ok "STAGE accepts $desc — it passed the evidence gate and went on to contact the guest"
        else
            notok "STAGE accepts $desc — it never reached the guest (exit $rc): $(tr '\n' ' ' <<<"$out" | cut -c1-160)"
        fi
    fi
}
stage refuse "--stage downgrade with no evidence at all" "no verified U6 PASS" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/none" --old-pkgdir "$WORK/old"
stage refuse "--stage downgrade after the upgrade but before U6" "no verified U6 PASS" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/st-no-u6" --old-pkgdir "$WORK/old"
stage refuse "--stage downgrade over an empty U6-RESULT" "no verified U6 PASS" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/st-empty" --old-pkgdir "$WORK/old"
stage refuse "--stage downgrade over a forged 'verdict=PASS'" "no verified U6 PASS" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/st-bare" --old-pkgdir "$WORK/old"
stage refuse "--stage downgrade over a U6 recorded for another domain" "no verified U6 PASS" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/st-wrong-domain" --old-pkgdir "$WORK/old"
stage refuse "--stage downgrade over a U6 that measured another distribution" "no verified U6 PASS" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/st-wrong-distro" --old-pkgdir "$WORK/old"
stage refuse "--stage downgrade on another domain than the one U6 measured" "no verified U6 PASS" \
    --stage downgrade --domain g7-other --distro fedora44 --evidence "$G7/stage-ok" --old-pkgdir "$WORK/old"
mkdir -p "$WORK/old2"; printf 'different\n' > "$WORK/old2/SHA256SUMS"
stage refuse "--stage downgrade with a different 1.0.0 set than the upgrade recorded" "not the 1.0.0 set" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/stage-ok" --old-pkgdir "$WORK/old2"
stage reach "--stage downgrade over a real U6 PASS" "" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/stage-ok" --old-pkgdir "$WORK/old"
cp -a "$G7/stage-ok" "$G7/stage-interrupted"; echo "run_id=x" > "$G7/stage-interrupted/U10-STARTED"
stage refuse "a second --stage downgrade after one was interrupted" "already started" \
    --stage downgrade --domain g7-f44 --distro fedora44 --evidence "$G7/stage-interrupted" --old-pkgdir "$WORK/old"
stage refuse "--stage peer-u6 before the upgrade stage completed" "no completed upgrade stage" \
    --stage peer-u6 --domain g7-f44 --distro fedora44 --evidence "$G7/none" --phone-ip 192.0.2.20
stage refuse "--stage peer-u6 once U10 has started" "U10 has already started" \
    --stage peer-u6 --domain g7-f44 --distro fedora44 --evidence "$G7/stage-interrupted" --phone-ip 192.0.2.20
stage refuse "--stage peer-u6 after an upgrade stage that failed checks" "failed check(s)" \
    --stage peer-u6 --domain g7-f44 --distro fedora44 --evidence "$G7/st-upgrade-failed" --phone-ip 192.0.2.20
stage reach "--stage peer-u6 after a completed upgrade stage" "" \
    --stage peer-u6 --domain g7-f44 --distro fedora44 --evidence "$G7/st-no-u6" --phone-ip 192.0.2.20
stage refuse "a second --stage upgrade into an evidence directory that already has one" "already holds a completed upgrade" \
    --stage upgrade --domain g7-f44 --distro fedora44 --evidence "$G7/stage-ok" --old-pkgdir "$WORK/old" --new-pkgdir "$WORK/old"
up_block="$(awk '/^if \[ "\$STAGE" = upgrade \]; then/ {f=1; next} f && /^if \[ "\$STAGE" = / {exit} f' "$HERE/upgrade-gates.sh")"
if [ "$(grep -c . <<<"$up_block")" -ge 100 ] && contains "$up_block" 'section "U9' \
        && ! contains "$up_block" 'section "U10' && contains "$up_block" 'G7UP_CHECKPOINT'; then
    ok "STATIC the upgrade stage ($(grep -c . <<<"$up_block") lines, O1 through U9) contains no U10: it stops on Pliwee and writes the checkpoint"
else
    notok "STATIC the upgrade stage still contains a U10 section, lacks the checkpoint, or could not be read"
fi

# ---------------------------------------------------------------------------
section "The published signed layout  (pre-G8 autopilot: U1 and deliver() wanted different shapes)"
# ---------------------------------------------------------------------------
# verify-release.sh (U1) needs the release as signed: one SHA256SUMS naming
# `<distro>/<file>`. deliver() copied only top-level packages and checked them
# with --ignore-missing, which verified NOTHING in that layout and made
# coreutils 9 exit 1. g7up_pkg_subdir now tells deliver() where the packages
# are; a set whose manifest names none of them must be refused, not delivered.
L="$WORK/layout"; mkdir -p "$L/signed/debian13" "$L/signed/fedora44" "$L/flat" "$L/unnamed" "$L/nosums"
echo deb > "$L/signed/debian13/omnibridge_1.0.0-1_amd64.deb"; echo rpm > "$L/signed/fedora44/omnibridge-1.0.0-1.fc44.x86_64.rpm"
( cd "$L/signed" && sha256sum debian13/omnibridge_1.0.0-1_amd64.deb fedora44/omnibridge-1.0.0-1.fc44.x86_64.rpm > SHA256SUMS )
echo deb > "$L/flat/pliwee_1.1.0-1_amd64.deb"; ( cd "$L/flat" && sha256sum pliwee_1.1.0-1_amd64.deb > SHA256SUMS )
echo deb > "$L/unnamed/pliwee_1.1.0-1_amd64.deb"; printf '%s  something-else.deb\n' "$(printf 'b%.0s' $(seq 1 64))" > "$L/unnamed/SHA256SUMS"
echo deb > "$L/nosums/pliwee_1.1.0-1_amd64.deb"
subdir_is() { [ "$(g7up_pkg_subdir "$1" "$2" "$3" 2>/dev/null)" = "$4" ]; }
accepts "the published layout: debian13's packages are under debian13/" subdir_is "$L/signed" debian13 deb debian13/
accepts "the published layout: fedora44's packages are under fedora44/" subdir_is "$L/signed" fedora44 rpm fedora44/
accepts "a flat set whose SHA256SUMS names its packages bare" subdir_is "$L/flat" debian13 deb ""
rejects "a set whose SHA256SUMS names none of its packages" g7up_pkg_subdir "$L/unnamed" debian13 deb
rejects "a set with no SHA256SUMS" g7up_pkg_subdir "$L/nosums" debian13 deb
rejects "the published layout asked for a distribution it does not carry" g7up_pkg_subdir "$L/signed" ubuntu2404 deb

# ---------------------------------------------------------------------------
section "A process matched by its old name  (LIFECYCLE-fedora44, 2026-09-26: '[o]mnibridged' after the rebrand)"
# ---------------------------------------------------------------------------
# lifecycle-gates.sh saw `pgrep -x pliweed` succeed, then looked for the
# daemon with `ps -eo user,pid,cmd | grep "[o]mnibridged"`, found nothing and
# aborted "no pliweed process found after start" (gate log
# LIFECYCLE-fedora44.20260926T043103Z.log). Its root count read the same stale
# name, so it would have said 0 whatever ran as root. Both are run here, as the
# harness defines them, against a REAL process called pliweed.
LG="$HERE/lifecycle-gates.sh"
l4_ps="$(sed -n "s/^L4_PS='\(.*\)'\$/\1/p" "$LG")"
l4_root="$(sed -n "s/^L4_ROOT_AWK='\(.*\)'\$/\1/p" "$LG")"
if [ -n "$l4_ps" ] && [ -n "$l4_root" ]; then
    ok "STATIC lifecycle-gates.sh defines the L4 capture (L4_PS) and its root count (L4_ROOT_AWK)"
else
    notok "STATIC lifecycle-gates.sh no longer defines L4_PS / L4_ROOT_AWK; the checks below would test nothing"
fi
# Whatever login runs this file, the guest's may be longer than 8 characters.
l4_width="$(sed -n 's/.*[ ,]user:\([0-9][0-9]*\)=.*/\1/p' <<<"$l4_ps")"
[ "${l4_width:-0}" -ge 32 ] && ok "STATIC L4_PS gives the user column a width of $l4_width, so a login longer than 8 characters is not truncated (#97)" \
                           || notok "STATIC L4_PS gives the user column no width of 32 or more: '$l4_ps'"
mkdir -p "$WORK/l4"; cp "$(command -v sleep)" "$WORK/l4/pliweed"
"$WORK/l4/pliweed" 60 & l4_pid=$!
for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -x pliweed >/dev/null 2>&1 && break; sleep 0.2; done
if [ -n "$l4_ps" ] && pgrep -x pliweed >/dev/null 2>&1; then
    cap="$(sh -c "$l4_ps" 2>/dev/null)"
    if contains_re "$cap" "^ *$(id -un) +$l4_pid +$WORK/l4/pliweed 60\$"; then
        ok "REGRESSION: L4_PS captures a running pliweed as 'user pid args' ($(id -un) $l4_pid)"
    else
        notok "REGRESSION: L4_PS did not capture the running pliweed (pid $l4_pid): '$cap'"
    fi
    # L4 reads that capture twice: a line's first field is the daemon's user,
    # and L4_ROOT_AWK counts the lines whose user is root. Both are run here on
    # this process's line of the capture, so a column format they cannot parse
    # goes red here and not in a guest.
    own="$(awk -v p="$l4_pid" '$2 == p { print $1 }' <<<"$cap")"
    [ "$own" = "$(id -un)" ] && ok "REGRESSION: L4 reads the daemon's user from the capture as $(id -un)" \
                             || notok "REGRESSION: L4 reads the daemon's user from the capture as '$own', not $(id -un)"
    [ "$(id -u)" -eq 0 ] && want_root=1 || want_root=0
    line="$(awk -v p="$l4_pid" '$2 == p' <<<"$cap")"
    if [ -z "$line" ]; then
        notok "L4 root count: the capture has no line for pid $l4_pid; the count would read 0 over nothing"
    elif [ -n "$l4_root" ] && [ "$(awk "$l4_root" <<<"$line")" = "$want_root" ]; then
        ok "L4 root count: the captured pliweed (uid $(id -u)) counts $want_root"
    else
        notok "L4 root count: the captured pliweed (uid $(id -u)) does not count $want_root"
    fi
    # The false red of #97: procps truncates a width-less `user=` column longer
    # than 8 characters to 7 and a `+`, so a runner logged in as pliwee-agent
    # read back 'pliwee-+' and the line above failed over its own capture.
    # Measured on that same process when this login is long enough to show it.
    l4_user="$(id -un)"
    # Not the last column: procps lets the last one run past its width.
    trunc="$(ps -p "$l4_pid" -o user=,pid= 2>/dev/null | awk '{ print $1 }')"
    if [ "${#l4_user}" -le 8 ]; then
        printf 'n/a   login %s is 8 characters or fewer; the long-name capture is checked only statically here\n' "$l4_user"
    elif [ "$trunc" = "$l4_user" ]; then
        printf 'n/a   this ps prints the %d-character login %s whole without a width; the truncation cannot be shown here\n' "${#l4_user}" "$l4_user"
    else
        ok "MEASURED: a width-less user column reads the ${#l4_user}-character login $l4_user as '$trunc'; the capture above read it whole (#97)"
    fi
    # Scoped to the process this test started: a workstation that runs its own
    # OmniBridge 1.0.0 daemon made the host-wide match non-empty and this line
    # red for a process the claim is not about (2026-09-28, pid 3634).
    old="$(sh -c 'ps -eo user,pid,cmd | grep "[o]mnibridged"' 2>/dev/null | awk -v p="$l4_pid" '$2 == p')"
    [ -z "$old" ] && ok "MEASURED: the old '[o]mnibridged' match finds nothing for that same process (the observed abort)" \
                  || notok "MEASURED: the old match unexpectedly found: $old"
else
    notok "REGRESSION: could not run a process called pliweed, or L4_PS is missing; L4 was not tested"
fi
kill "$l4_pid" 2>/dev/null; wait "$l4_pid" 2>/dev/null
root_is() { [ "$(awk "$l4_root" <<<"$1")" = "$2" ]; }
if [ -n "$l4_root" ]; then
    root_is $'anyflow 1234 /usr/bin/pliweed' 0 && ok "L4 root count: the user's daemon alone is 0" || notok "L4 root count: the user's daemon alone is not 0"
    root_is $'anyflow 1234 /usr/bin/pliweed\nroot 99 /usr/bin/pliweed' 1 \
        && ok "REGRESSION: L4 root count: a pliweed owned by root IS counted (the old one read 0)" \
        || notok "REGRESSION: L4 root count missed a pliweed owned by root"
fi
# No command in the harness may inspect the old binary name, however it is
# spelled: brackets are removed before searching, comments are ignored.
stale="$(grep -nv '^[[:space:]]*#' "$LG" | tr -d '[]' | grep -i 'omnibridged' || true)"
[ -z "$stale" ] && ok "STATIC lifecycle-gates.sh inspects no 'omnibridged' process, bracketed or not" \
                || notok "STATIC lifecycle-gates.sh still inspects the old daemon name: $stale"

# ---------------------------------------------------------------------------
section "A guest's stderr thrown away  (G7UP-fedora44-INSTALL, 2026-09-26: an empty U2-firewall.txt)"
# ---------------------------------------------------------------------------
# ga_exec printed the guest's stderr with `base64 -d 2>/dev/null >&2`, which
# sends it to /dev/null. firewall-cmd's "Error: INVALID_SERVICE" (exit 101)
# never reached the harness, whose capture of it was 0 bytes. Here ga_exec
# reads the agent reply that firewall-cmd produced (stdout empty, the error
# on stderr, exit 101) from a stand-in virsh, and must pass all of it on.
GA="$WORK/ga"; mkdir -p "$GA"
fw_err="Error: INVALID_SERVICE: Zone 'work': 'omnibridge' not among existing services"
cat > "$GA/virsh" <<VIRSH
#!/bin/sh
while [ "\$1" = -c ]; do shift 2; done
case "\$1" in
  dominfo) echo "Name: \$2" ;;
  domstate) echo running ;;
  dumpxml) echo "<target name='org.qemu.guest_agent.0'/>" ;;
  qemu-agent-command) case "\$3" in
      *'"guest-exec"'*) echo '{"return":{"pid":2553}}' ;;
      *) echo '{"return":{"exitcode":101,"err-data":"$(printf '%s\n' "$fw_err" | base64 -w0)","out-truncated":false,"err-truncated":false,"exited":true}}' ;;
    esac ;;
esac
VIRSH
chmod +x "$GA/virsh"
ga_run() { ( PATH="$GA:$PATH"; . "$HERE/lib/guest-agent.sh"; ga_exec g7-test "$@" ); }
ga_run 'firewall-cmd --permanent --zone=work --add-service=omnibridge' >"$GA/out" 2>"$GA/err"; ga_rc=$?
[ "$ga_rc" = 101 ] && ok "ga_exec returns the guest's exit code (101)" || notok "ga_exec returned $ga_rc, not the guest's 101"
if contains "$(cat "$GA/err")" "$fw_err"; then
    ok "REGRESSION: ga_exec passes the guest's stderr on (firewall-cmd's INVALID_SERVICE error)"
else
    notok "REGRESSION: ga_exec dropped the guest's stderr: '$(cat "$GA/err")'"
fi
ga_run 'firewall-cmd --permanent --zone=work --add-service=omnibridge' > "$GA/capture" 2>&1
[ -s "$GA/capture" ] && contains "$(cat "$GA/capture")" INVALID_SERVICE \
    && ok "REGRESSION: the harness's own capture form (> file 2>&1) keeps the error: $(wc -c < "$GA/capture") bytes, not 0" \
    || notok "REGRESSION: the '> file 2>&1' capture is $(wc -c < "$GA/capture") bytes"
[ ! -s "$GA/out" ] && ok "…and nothing was invented on stdout" || notok "ga_exec wrote '$(cat "$GA/out")' on stdout"

# ---------------------------------------------------------------------------
section "A service used before firewalld has loaded it  (G7UP-fedora44-INSTALL, 2026-09-26: INVALID_SERVICE)"
# ---------------------------------------------------------------------------
# U2 followed the OmniBridge 1.0.0 README: --permanent --add-service, then
# --reload. The package had installed omnibridge.xml while firewalld was
# running, and a running firewalld knows only the service files it loaded at
# its last start or reload: "Error: INVALID_SERVICE: Zone 'work': 'omnibridge'
# not among existing services", exit 101. Measured on a write-protected copy
# of that guest, 2026-09-26: the old order fails so, and reload / add / reload
# puts it in the permanent and the running zone. FW models exactly that
# (state in files), with a knob to fail each step, and g7up_fw_add_service,
# the code U2 runs, is driven through it.
FW="$WORK/fw"; mkdir -p "$FW/bin"
cat > "$FW/bin/firewall-cmd" <<'FWC'
#!/usr/bin/env bash
# TEST ONLY: firewalld's service-definition loading, as measured.
S="$FW_STATE"; perm=0; zone=""; add=""; list=0
for a in "$@"; do case "$a" in
  --permanent) perm=1 ;; --zone=*) zone="${a#--zone=}" ;; --add-service=*) add="${a#--add-service=}" ;;
  --list-services) list=1 ;; --reload) reload=1 ;; esac; done
if [ "${reload:-0}" = 1 ]; then
    n=$(( $(cat "$S/reloads" 2>/dev/null || echo 0) + 1 )); echo "$n" > "$S/reloads"
    [ "${FW_FAIL_RELOAD:-}" = "$n" ] && { echo "Error: COMMAND_FAILED: reload $n" >&2; exit 13; }
    cp "$S/disk" "$S/loaded"; cp "$S/perm" "$S/runtime"; echo success; exit 0
fi
if [ -n "$add" ]; then
    [ "${FW_FAIL_ADD:-}" = 1 ] && { echo "Error: COMMAND_FAILED: add" >&2; exit 12; }
    grep -qx -- "$add" "$S/loaded" || { echo "Error: INVALID_SERVICE: Zone '$zone': '$add' not among existing services" >&2; exit 101; }
    grep -qx -- "$add" "$S/perm" || echo "$add" >> "$S/perm"; echo success; exit 0
fi
if [ "$list" = 1 ]; then
    f="$S/runtime"; [ "$perm" = 1 ] && f="$S/perm"
    [ "${FW_HIDE_PERM:-}" = 1 ] && [ "$perm" = 1 ] && { echo "ssh"; exit 0; }
    tr '\n' ' ' < "$f" | sed 's/ $//'; echo; exit 0
fi
exit 2
FWC
chmod +x "$FW/bin/firewall-cmd"
fw_boot() { # a guest whose firewalld started before the package installed omnibridge.xml
    rm -rf "$FW/state"; mkdir -p "$FW/state"
    printf 'ssh\nmdns\n' > "$FW/state/disk"; cp "$FW/state/disk" "$FW/state/loaded"
    printf 'ssh\n' > "$FW/state/perm"; cp "$FW/state/perm" "$FW/state/runtime"
    echo omnibridge >> "$FW/state/disk"          # the package install, after firewalld started
}
fw_env() { env PATH="$FW/bin:$PATH" FW_STATE="$FW/state" "$@"; }
FWRUN() { fw_env bash -c '. "$0/lib/g7up-evidence.sh"; gx() { sh -c "$1"; }
          g7up_fw_add_service work omnibridge "$1/U2-firewall.txt"
          printf "rc=%s\nperm=%s\nrt=%s\n" "$FW_RC" "$FW_PERMANENT" "$FW_RUNTIME"
          g7up_has_service "$FW_PERMANENT" omnibridge && echo perm_has=yes
          g7up_has_service "$FW_RUNTIME" omnibridge && echo rt_has=yes' "$HERE" "$FW"; }

fw_boot
old_out="$(fw_env sh -c 'firewall-cmd --permanent --zone=work --add-service=omnibridge && firewall-cmd --reload' 2>&1)"; old_rc=$?
[ "$old_rc" = 101 ] && contains "$old_out" "INVALID_SERVICE: Zone 'work': 'omnibridge' not among existing services" \
    && ok "MEASURED: the old order (add, then reload) fails with INVALID_SERVICE, exit 101" \
    || notok "the model does not reproduce the measured failure (rc $old_rc: $old_out)"
fw_boot; r="$(FWRUN)"
contains "$r" "rc=0" && contains "$r" "perm_has=yes" && contains "$r" "rt_has=yes" \
    && ok "REGRESSION: reload / add / reload: exit 0, omnibridge in the permanent AND the running zone 'work'" \
    || notok "the U2 sequence did not succeed: $r"
cap="$(cat "$FW/U2-firewall.txt")"
contains "$cap" '$ firewall-cmd --reload' && contains "$cap" '$ firewall-cmd --permanent --zone=work --add-service=omnibridge' \
    && contains "$cap" "# sequence exit status: 0" && contains "$cap" "ssh omnibridge" \
    && ok "U2-firewall.txt records each command, its output, the exit status and both service lists" \
    || notok "U2-firewall.txt is missing something: $cap"
fw_boot; r="$(FW_FAIL_RELOAD=1 FWRUN)"
contains "$r" "rc=13" && ! contains "$r" "perm_has=yes" && contains "$(cat "$FW/U2-firewall.txt")" "COMMAND_FAILED: reload 1" \
    && ok "REJECTS a failing first reload: exit 13, nothing added, the error kept in the evidence" \
    || notok "a failing first reload was not caught: $r"
fw_boot; r="$(FW_FAIL_ADD=1 FWRUN)"
contains "$r" "rc=12" && ! contains "$r" "perm_has=yes" && contains "$(cat "$FW/U2-firewall.txt")" "COMMAND_FAILED: add" \
    && ok "REJECTS a failing --add-service: exit 12, the error kept in the evidence" \
    || notok "a failing --add-service was not caught: $r"
fw_boot; r="$(FW_FAIL_RELOAD=2 FWRUN)"
contains "$r" "rc=13" && contains "$r" "perm_has=yes" && ! contains "$r" "rt_has=yes" \
    && ok "REJECTS a failing final reload: exit 13, and the running zone does not have it" \
    || notok "a failing final reload was not caught: $r"
fw_boot; r="$(FW_HIDE_PERM=1 FWRUN)"
contains "$r" "rc=0" && ! contains "$r" "perm_has=yes" \
    && ok "REJECTS a permanent zone that does not list omnibridge, even when every command said success" \
    || notok "a permanent list without omnibridge was accepted: $r"
g7up_has_service "ssh omnibridge-extra mdns" omnibridge >/dev/null 2>&1 \
    && notok "g7up_has_service matched 'omnibridge' inside 'omnibridge-extra'" \
    || ok "the service match is exact: 'omnibridge-extra' is not omnibridge"
# U2 itself: the fixed sequence, and all three assertions on it.
u2="$(sed -n '/^section "U2 — enable, start, firewall/,/^cat <<NEXT$/p' "$HERE/upgrade-gates.sh")"
contains "$u2" 'g7up_fw_add_service work omnibridge "$EVIDENCE/U2-firewall.txt"' && contains "$u2" '[ "$FW_RC" = 0 ]' \
    && contains "$u2" 'g7up_has_service "$FW_PERMANENT" omnibridge' && contains "$u2" 'g7up_has_service "$FW_RUNTIME" omnibridge' \
    && ok "STATIC U2 runs g7up_fw_add_service and asserts its exit status, the permanent and the running zone" \
    || notok "STATIC U2 no longer runs the reload/add/reload sequence with all three assertions"
# (This file reproduces the old order on purpose, above, so it is not scanned.)
old_form="$(grep -nE -- "--permanent[^'\"]*--add-service=[a-z]+ *&& *firewall-cmd --reload" "$HERE"/*.sh "$HERE"/lib/*.sh \
    | grep -v "^$HERE/harness-selftests.sh:" | grep -v '^[^:]*:[0-9]*: *#' || true)"
[ -z "$old_form" ] && ok "STATIC no harness adds a service before reloading (the 1.0.0 README order)" \
                   || notok "STATIC the add-then-reload order is back: $old_form"

# ---------------------------------------------------------------------------
section "The pre-G8 coordinator  (pre-g8-manual-gates-selftests.sh)"
# ---------------------------------------------------------------------------
# Non-vacuous: a suite that ran nothing would also "pass".
if bash "$HERE/pre-g8-manual-gates-selftests.sh" > "$WORK/coord.txt" 2>&1 \
        && [ "$(grep -c '^ok ' "$WORK/coord.txt")" -ge 50 ]; then
    ok "the coordinator's self-tests pass ($(grep -c '^ok ' "$WORK/coord.txt") checks)"
else
    notok "the coordinator's self-tests FAILED:"; grep '^not ok' "$WORK/coord.txt" | sed 's/^/        /'
fi

printf '\n-----------------------------------------------\n'
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
if [ "$FAIL" -gt 0 ]; then printf '\nFailed:\n'; for g in "${FAILED[@]}"; do printf '  %s\n' "$g"; done; fi
[ "$FAIL" -eq 0 ]
