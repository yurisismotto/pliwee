#!/usr/bin/env bash
# evidence-whitespace-check.sh — no committed evidence text ends a line in a
# space or a tab.
#
# `git diff --check` sees only tracked changes, so a new evidence directory can
# carry trailing whitespace while a transcript of that command says "clean"
# (the pre-G8 final-repair finding). This scans the files themselves, tracked or
# not, and fails on any line ending in horizontal whitespace.
#
# A CR at the end of a line (a CRLF line ending) fails too: `git diff --check`
# counts it as trailing whitespace, and it would otherwise hide a space or a tab
# before it, since the CR, not the space, is then the last character.
#
#   evidence-whitespace-check.sh [PATH...]   files or directories (recursive);
#                                            by default the pre-G8 hardening
#                                            report and its evidence directory
#   evidence-whitespace-check.sh --selftest  prove it rejects what it must
#
# It fails, too, when it scanned nothing: a missing path, or a directory with no
# text file in it, is not "no trailing whitespace". Binary files (a screenshot)
# are counted and skipped.

set -uo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd -- "$HERE/../.." && pwd)"
# shellcheck source=lib/assert.sh
. "$HERE/lib/assert.sh"

DEFAULTS=(
    "$REPO/docs/reports/branding/PLIWEE-PRE-G8-GATE-HARDENING.md"
    "$REPO/docs/reports/branding/pliwee-pre-g8-gate-hardening"
)

# scan PATH... — prints one line per offending line and a tally; fails on any
# offence and on an empty scan.
scan() {
    local p f n_text=0 n_bin=0 n_bad=0 hits files=()
    need_tool grep find || return 2
    [ "$#" -gt 0 ] || { echo "FAIL: no path to scan"; return 1; }
    for p in "$@"; do
        [ -e "$p" ] || { echo "FAIL: $p does not exist; nothing there was scanned"; return 1; }
        while IFS= read -r -d '' f; do files+=("$f"); done < <(find "$p" -type f -print0 | sort -z)
    done
    for f in "${files[@]}"; do
        # -I: a binary file matches nothing. An empty file is text with no lines.
        if [ -s "$f" ] && ! grep -Iq '' "$f"; then n_bin=$((n_bin + 1)); continue; fi
        n_text=$((n_text + 1))
        hits="$(grep -nH $'[ \t\r]$' "$f")"
        [ -n "$hits" ] || continue
        n_bad=$((n_bad + $(grep -c '' <<<"$hits")))
        sed 's/^/trailing whitespace: /' <<<"$hits"
    done
    printf 'scanned %d text file(s), skipped %d binary; %d line(s) with trailing horizontal whitespace\n' "$n_text" "$n_bin" "$n_bad"
    [ "$n_text" -gt 0 ] || { echo "FAIL: no text file was scanned"; return 1; }
    [ "$n_bad" -eq 0 ] || { echo "FAIL"; return 1; }
    echo "PASS"
}

selftest() {
    local W pass=0 fail=0 out
    W="$(mktemp -d "${TMPDIR:-/tmp}/ws-check.XXXXXXXX")"; trap 'rm -rf "$W"' RETURN
    t() { # DESCRIPTION EXPECTED_RC PATH...
        local d="$1" want="$2" rc; shift 2
        out="$(scan "$@" 2>&1)"; rc=$?
        if [ "$rc" = "$want" ]; then pass=$((pass + 1)); printf 'ok    %s\n' "$d"
        else fail=$((fail + 1)); printf 'not ok  %s (exit %s, wanted %s): %s\n' "$d" "$rc" "$want" "$(tail -1 <<<"$out")"; fi
    }
    mkdir -p "$W/clean/sub" "$W/space" "$W/tab" "$W/empty" "$W/binonly" "$W/crlf" "$W/spacecr"
    printf 'a\nb\n' > "$W/clean/a.txt"; printf 'ok    x\n\n' > "$W/clean/sub/b.txt"
    printf '\x89PNG\x00 \n' > "$W/clean/shot.png"
    printf 'a\nCAUGHT   M1 \nc\n' > "$W/space/m.txt"
    printf 'a\nb\t\n' > "$W/tab/h.txt"
    printf '\x89PNG\x00 \n' > "$W/binonly/shot.png"
    printf 'line one\r\nline two\r\n' > "$W/crlf/w.txt"
    printf 'a\ntrailing space \r\nc\n' > "$W/spacecr/s.txt"
    t "ACCEPTS clean text, recursively, with a binary file beside it" 0 "$W/clean"
    t "REJECTS a line ending in a space" 1 "$W/space"
    t "REJECTS a line ending in a tab" 1 "$W/tab"
    t "REJECTS a clean directory scanned together with a dirty one" 1 "$W/clean" "$W/space"
    t "REJECTS an empty directory: nothing scanned is not a pass" 1 "$W/empty"
    t "REJECTS a directory holding only binary files" 1 "$W/binonly"
    t "REJECTS a path that does not exist" 1 "$W/missing"
    out="$(scan "$W/space" "$W/tab" 2>&1)"
    if contains "$out" "2 line(s) with trailing" && contains "$out" "m.txt:2:"; then
        pass=$((pass + 1)); echo "ok    counts every offending line exactly, and names file and line"
    else fail=$((fail + 1)); echo "not ok  counts every offending line exactly: $out"; fi
    t "REJECTS a line ending in CR (a CRLF line ending)" 1 "$W/crlf"
    t "REJECTS a space hidden behind the CR" 1 "$W/spacecr"
    out="$(scan "$W/crlf" "$W/spacecr" 2>&1)"
    if contains "$out" "binary; 3 line(s) with trailing" && contains "$out" "w.txt:1:" \
        && contains "$out" "w.txt:2:" && contains "$out" "s.txt:2:"; then
        pass=$((pass + 1)); echo "ok    counts every CR-ended line exactly, and names file and line"
    else fail=$((fail + 1)); echo "not ok  counts every CR-ended line exactly: $out"; fi
    printf '\n%d passed, %d failed\n' "$pass" "$fail"
    [ "$fail" -eq 0 ]
}

case "${1:-}" in
    --selftest) selftest ;;
    -h|--help) sed -n '2,/^set -uo/p' "$0" | sed '$d' ;;
    "") scan "${DEFAULTS[@]}" ;;
    *) scan "$@" ;;
esac
