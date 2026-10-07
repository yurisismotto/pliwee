#!/usr/bin/env bash
# sandbox.sh — run the headless worker with no credential it could merge with.
#
#   sandbox.sh run --work DIR --outbox DIR -- CMD...
#                         run CMD in a bubblewrap sandbox; CMD runs only if the
#                         sandbox's own probe passes inside it first
#   sandbox.sh probe      (inside the sandbox) prove the isolation holds
#   sandbox.sh --selftest prove `run` isolates and `probe` rejects what it must
#
# WHY
# ---
# The runner account holds the publisher's GitHub token: the credential that
# pushes the worker's branch and opens its draft PR, and that — like any token
# with Contents: write — could also merge. Claude runs `cargo`, `gradle` and
# build scripts, i.e. arbitrary code, as that same OS user. Permission rules
# cannot stop arbitrary code reading a file its user can read, and on this host
# kernel.yama.ptrace_scope is 0, so the same user can also read the runner's
# memory. So the model runs where none of that exists:
#
#   * a new PID namespace: the runner, its worker process and every other
#     process of the account are invisible (no /proc/<pid>/environ, no mem);
#   * the environment is cleared and rebuilt from an allow list: no GH_TOKEN,
#     GITHUB_TOKEN, ACTIONS_* or RUNNER_*;
#   * $HOME is read-only, so ~/.gitconfig, ~/.bashrc and the like cannot be
#     turned into code the trusted steps run later; only named caches and
#     Claude's own state are writable;
#   * the gh login, keyrings, ssh and gpg material, git credential files and
#     the whole runner directory are replaced by empty tmpfs mounts;
#   * /run/user/<uid> (the session bus, the keyring socket) and /tmp are empty;
#   * only the work copy and the outbox are writable outside $HOME's caches.
#
# The network stays: Claude needs its API, cargo needs crates.io. Nothing
# reachable over it can merge without a credential, and none is left inside.
#
# FAIL CLOSED: no bwrap, a probe that finds a credential, a runner process, a
# writable $HOME or an unwritable work copy — the command never starts.
#
# Configuration (environment, all optional):
#   PLIWEE_RUNNER_ROOT     the runner's directory (hidden whole); default: two
#                          levels above $RUNNER_WORKSPACE when that is set
#   PLIWEE_SANDBOX_HIDE    extra paths to hide, colon-separated
#   PLIWEE_SANDBOX_RW      extra writable paths under $HOME, colon-separated
#   PLIWEE_SANDBOX_ENV     extra variable NAMES to pass in, space-separated
#                          (refused if one looks like a GitHub/Actions token)

set -uo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SELF="$HERE/$(basename -- "${BASH_SOURCE[0]}")"
REPO="$(cd -- "$HERE/../.." && pwd)"
# shellcheck source=../../packaging/tests/lib/assert.sh
. "$REPO/packaging/tests/lib/assert.sh"

fail() { printf 'sandbox: FAILED: %s\n' "$*" >&2; return 1; }

# Names that must never cross into the sandbox, whatever the allow list says.
TOKEN_RE='^(GH_TOKEN|GITHUB_TOKEN|GH_ENTERPRISE_TOKEN|GITHUB_ENTERPRISE_TOKEN|ACTIONS_[A-Z_]*|RUNNER_[A-Z_]*|GITHUB_[A-Z_]*)$'

# Paths under $HOME that hold credentials. Hidden when they exist.
HOME_SECRETS=(.config/gh .local/share/keyrings .git-credentials .config/git/credentials
              .netrc .ssh .gnupg .docker .config/containers .android/adbkey .android/adbkey.pub
              .gradle/gradle.properties .cargo/credentials .cargo/credentials.toml)
# Writable inside $HOME: Claude's own state and build caches, nothing that a
# trusted step reads back as configuration.
HOME_RW=(.claude .claude.json .cargo/registry .cargo/git .gradle/caches .gradle/wrapper
         .gradle/native .cache)
# Passed through when set.
ENV_ALLOW=(HOME USER LOGNAME PATH LANG LC_ALL TERM TZ SHELL
           PLIWEE_OWNER_NAME PLIWEE_OWNER_EMAIL PLIWEE_OWNER_LOGIN PLIWEE_REPO
           PLIWEE_AGENT_BASE PLIWEE_AGENT_BRIEF PLIWEE_AGENT_OUTBOX
           ANTHROPIC_API_KEY CLAUDE_CODE_OAUTH_TOKEN
           CARGO_HOME RUSTUP_HOME JAVA_HOME ANDROID_HOME ANDROID_SDK_ROOT GRADLE_USER_HOME)

runner_root() {
    if [ -n "${PLIWEE_RUNNER_ROOT:-}" ]; then printf '%s\n' "$PLIWEE_RUNNER_ROOT"
    elif [ -n "${RUNNER_WORKSPACE:-}" ]; then dirname -- "$(dirname -- "$RUNNER_WORKSPACE")"
    fi
}

# The list the probe checks is the list run hid: one source for both.
hidden_paths() {
    local p rr
    for p in "${HOME_SECRETS[@]}"; do printf '%s\n' "$HOME/$p"; done
    rr="$(runner_root)"; [ -n "$rr" ] && printf '%s\n' "$rr"
    printf '%s\n' "/run/user/$(id -u)"
    if [ -n "${PLIWEE_SANDBOX_HIDE:-}" ]; then tr ':' '\n' <<<"$PLIWEE_SANDBOX_HIDE"; fi
}

cmd_run() {
    local work="" outbox=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --work)   work="${2:-}"; shift 2 ;;
            --outbox) outbox="${2:-}"; shift 2 ;;
            --)       shift; break ;;
            *)        fail "run: unknown argument '$1'"; return 1 ;;
        esac
    done
    [ "$#" -gt 0 ] || { fail "run: no command"; return 1; }
    need_tool bwrap || { fail "bwrap is not installed; the worker does not run unsandboxed"; return 1; }
    [ -d "$work" ] && [ -d "$outbox" ] || { fail "run: --work and --outbox must be existing directories"; return 1; }
    work="$(cd -- "$work" && pwd)"; outbox="$(cd -- "$outbox" && pwd)"
    [ -n "${HOME:-}" ] && [ -d "$HOME" ] || { fail "run: HOME is not a directory"; return 1; }

    local args=(--ro-bind / / --dev /dev --proc /proc --tmpfs /tmp
                --unshare-pid --unshare-ipc --unshare-uts --die-with-parent --new-session
                --ro-bind "$HOME" "$HOME")
    local p
    for p in "${HOME_RW[@]}"; do
        [ -e "$HOME/$p" ] && args+=(--bind "$HOME/$p" "$HOME/$p")
    done
    if [ -n "${PLIWEE_SANDBOX_RW:-}" ]; then
        while IFS= read -r p; do
            [ -n "$p" ] || continue
            case "$p" in "$HOME"/*) ;; *) fail "run: PLIWEE_SANDBOX_RW path '$p' is not under \$HOME"; return 1 ;; esac
            [ -e "$p" ] && args+=(--bind "$p" "$p")
        done < <(tr ':' '\n' <<<"$PLIWEE_SANDBOX_RW")
    fi
    # Hide after the rw binds, so a secret inside a writable tree is still hidden.
    while IFS= read -r p; do
        [ -n "$p" ] || continue
        if [ -d "$p" ]; then args+=(--tmpfs "$p")
        elif [ -e "$p" ]; then args+=(--ro-bind /dev/null "$p")
        fi
    done < <(hidden_paths)
    # The work copy, the outbox and this script come back last: they may live
    # inside the runner directory that was just hidden.
    args+=(--bind "$work" "$work" --bind "$outbox" "$outbox"
           --ro-bind "$REPO/.github/agent" /tmp/.pliwee-sandbox/.github/agent
           --ro-bind "$REPO/packaging/tests/lib" /tmp/.pliwee-sandbox/packaging/tests/lib
           --chdir "$work" --clearenv)
    local names=("${ENV_ALLOW[@]}") n
    if [ -n "${PLIWEE_SANDBOX_ENV:-}" ]; then
        for n in $PLIWEE_SANDBOX_ENV; do names+=("$n"); done
    fi
    for n in "${names[@]}"; do
        [[ "$n" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || { fail "run: '$n' is not a variable name"; return 1; }
        if [[ "$n" =~ $TOKEN_RE ]]; then fail "run: refusing to pass $n into the sandbox"; return 1; fi
        [ -n "${!n+x}" ] && args+=(--setenv "$n" "${!n}")
    done
    args+=(--setenv PLIWEE_SANDBOX_WORK "$work" --setenv PLIWEE_SANDBOX_OUTBOX "$outbox"
           --setenv PLIWEE_SANDBOX_HIDDEN "$(hidden_paths | tr '\n' ':')")

    # The probe runs first, in the same sandbox, and the command only if it passes.
    bwrap "${args[@]}" -- bash -c '
        /tmp/.pliwee-sandbox/.github/agent/sandbox.sh probe >&2 || exit 97
        exec "$@"' sandbox-run "$@"
    local rc=$?
    [ "$rc" -eq 97 ] && fail "the sandbox probe failed; the command did not run"
    return "$rc"
}

# probe — run INSIDE the sandbox. Each check fails on the condition it exists
# for; the self-test runs it outside a sandbox and requires it to fail there.
cmd_probe() {
    local bad=0 v p pid comm hits
    for v in $(compgen -e); do
        if [[ "$v" =~ $TOKEN_RE ]]; then echo "probe: $v is set"; bad=1; fi
    done
    [ -n "${PLIWEE_SANDBOX_HIDDEN:-}" ] || { echo "probe: PLIWEE_SANDBOX_HIDDEN is empty — nothing to check against"; return 1; }
    while IFS= read -r p; do
        [ -n "$p" ] || continue
        if [ -d "$p" ]; then
            # Empty, except for the work copy and outbox mounted back inside it.
            hits="$(find "$p" \( -path "${PLIWEE_SANDBOX_WORK:-/nonexistent}" -o -path "${PLIWEE_SANDBOX_OUTBOX:-/nonexistent}" \) -prune \
                         -o -type f -print 2>/dev/null | head -5)"
            [ -z "$hits" ] || { echo "probe: $p is visible and holds files: ${hits//$'\n'/ }"; bad=1; }
        elif [ -s "$p" ] && [ -r "$p" ]; then
            echo "probe: $p is readable"; bad=1
        fi
    done < <(tr ':' '\n' <<<"$PLIWEE_SANDBOX_HIDDEN")
    hits=""
    for pid in /proc/[0-9]*; do
        comm="$(cat "$pid/comm" 2>/dev/null)" || continue
        case "$comm" in Runner.*|runsvc.sh|Runner) hits+="${pid#/proc/}:$comm " ;; esac
    done
    [ -z "$hits" ] || { echo "probe: runner processes are visible: $hits"; bad=1; }
    [ "$(cat /proc/1/comm 2>/dev/null)" != systemd ] || { echo "probe: PID 1 is systemd — no PID namespace"; bad=1; }
    if ( : > "$HOME/.pliwee-sandbox-probe" ) 2>/dev/null; then
        rm -f -- "$HOME/.pliwee-sandbox-probe"; echo "probe: \$HOME is writable"; bad=1
    fi
    if command -v gh >/dev/null 2>&1 && gh auth token >/dev/null 2>&1; then
        echo "probe: gh still has a token"; bad=1
    fi
    for v in PLIWEE_SANDBOX_WORK PLIWEE_SANDBOX_OUTBOX; do
        p="${!v:-}"
        if [ -z "$p" ] || ! ( : > "$p/.pliwee-sandbox-probe" ) 2>/dev/null; then
            echo "probe: $v '${p}' is not writable"; bad=1
        else rm -f -- "$p/.pliwee-sandbox-probe"; fi
    done
    [ "$bad" -eq 0 ] || return 1
    echo "ok    sandbox: no token in the environment, credentials hidden, no runner process, \$HOME read-only"
}

selftest() {
    local PASS=0 FAIL=0 T out rc
    ok()    { PASS=$((PASS + 1)); printf 'ok    %s\n' "$*"; }
    notok() { FAIL=$((FAIL + 1)); printf 'not ok  %s\n' "$*"; }
    need_tool bwrap || { echo "PRECONDITION FAILED: bwrap is needed by these self-tests"; return 3; }
    T="$(mktemp -d "${TMPDIR:-/tmp}/sandbox-selftest.XXXXXXXX")" || return 1
    # shellcheck disable=SC2064
    trap "rm -rf -- '$T'" RETURN

    # A fake account: a gh login, an ssh key, Claude state, a cargo cache, and a
    # runner directory holding its credentials and the work tree.
    local H="$T/home" RR="$T/home/actions-runner"
    mkdir -p "$H/.config/gh" "$H/.ssh" "$H/.claude" "$H/.cargo/registry" "$RR/_work/_temp/work" "$RR/_work/_temp/outbox"
    echo 'oauth_token: gho_SELFTEST_SECRET' > "$H/.config/gh/hosts.yml"
    echo 'PRIVATE KEY' > "$H/.ssh/id_ed25519"
    echo '{"token":"runner-secret"}' > "$RR/.credentials"
    echo '[user]' > "$H/.gitconfig"
    local W="$RR/_work/_temp/work" O="$RR/_work/_temp/outbox"
    # A process with a recognisable name, outside, that must not be visible inside.
    cp "$(command -v sleep)" "$T/Runner.Worker"; "$T/Runner.Worker" 60 & local spid=$!
    [ "$(cat "/proc/$spid/comm" 2>/dev/null)" = Runner.Worker ] \
        && ok "precondition: a process named Runner.Worker is visible outside" \
        || notok "precondition: the decoy runner process is not visible even outside"

    sb() { env HOME="$H" PLIWEE_RUNNER_ROOT="$RR" GH_TOKEN=gho_LEAK GITHUB_TOKEN=ghs_LEAK \
               ACTIONS_RUNTIME_TOKEN=leak "$SELF" run --work "$W" --outbox "$O" -- "$@"; }

    out="$(sb bash -c 'echo inside' 2>&1)"; rc=$?
    { [ "$rc" -eq 0 ] && contains "$out" "ok    sandbox:" && contains "$out" "inside"; } \
        && ok "ACCEPTS: the probe passes and the command runs" || notok "the sandboxed command did not run: rc=$rc $out"
    # Every negative below also requires the probe's ok line and the command's
    # marker: a sandbox that never ran would otherwise "hide" everything.
    ran() { contains "$1" "ok    sandbox:" && contains "$1" "RAN-INSIDE"; }
    out="$(sb bash -c 'echo RAN-INSIDE; cat ~/.config/gh/hosts.yml; cat ~/.ssh/id_ed25519; cat '"$RR"'/.credentials' 2>&1)"
    ! ran "$out" || contains "$out" "SELFTEST_SECRET" || contains "$out" "PRIVATE KEY" || contains "$out" "runner-secret" \
        && notok "a credential was readable inside: $out" || ok "REJECTS reading the gh login, an ssh key and the runner's credentials"
    out="$(sb bash -c 'echo RAN-INSIDE; env' 2>&1)"
    ! ran "$out" || contains "$out" "LEAK" || contains "$out" "ACTIONS_RUNTIME_TOKEN" \
        && notok "a token crossed into the sandbox" || ok "REJECTS GH_TOKEN, GITHUB_TOKEN and ACTIONS_* in the environment"
    out="$(sb bash -c 'echo RAN-INSIDE; cat /proc/[0-9]*/comm' 2>&1)"
    ! ran "$out" || contains "$out" "Runner.Worker" && notok "a runner process is visible inside" || ok "REJECTS seeing the runner's processes"
    out="$(sb bash -c 'echo RAN-INSIDE; echo "[core] fsmonitor = evil" >> ~/.gitconfig' 2>&1)"
    ! ran "$out" || contains "$(cat "$H/.gitconfig")" "fsmonitor" && notok "\$HOME/.gitconfig was writable" \
        || ok "REJECTS writing \$HOME configuration that trusted steps would read"
    sb bash -c 'echo built > ~/.cargo/registry/x; echo done > "$PLIWEE_SANDBOX_WORK/f"; echo out > "$PLIWEE_SANDBOX_OUTBOX/o"' >/dev/null 2>&1
    [ -s "$H/.cargo/registry/x" ] && [ -s "$W/f" ] && [ -s "$O/o" ] \
        && ok "ACCEPTS writing the work copy, the outbox and the build caches" || notok "work, outbox or cache not writable"
    out="$(env HOME="$H" PLIWEE_RUNNER_ROOT="$RR" PLIWEE_SANDBOX_ENV="GH_TOKEN" GH_TOKEN=x "$SELF" run --work "$W" --outbox "$O" -- true 2>&1)"; rc=$?
    [ "$rc" -ne 0 ] && contains "$out" "refusing to pass GH_TOKEN" \
        && ok "REJECTS an allow list that names a token" || notok "a token name was accepted into the allow list: $out"
    out="$(env HOME="$H" PLIWEE_RUNNER_ROOT="$RR" PATH=/nonexistent /usr/bin/bash "$SELF" run --work "$W" --outbox "$O" -- true 2>&1)"; rc=$?
    [ "$rc" -ne 0 ] && ok "REJECTS running without bwrap — never unsandboxed" || notok "ran without bwrap: $out"
    out="$(env HOME="$H" PLIWEE_RUNNER_ROOT="$RR" "$SELF" run --work "$T/missing" --outbox "$O" -- true 2>&1)"; rc=$?
    [ "$rc" -ne 0 ] && ok "REJECTS a work directory that does not exist" || notok "accepted a missing work dir"

    # The probe must not be vacuous: outside a sandbox it has to fail.
    out="$(env HOME="$H" GH_TOKEN=x PLIWEE_SANDBOX_HIDDEN="$H/.config/gh:" PLIWEE_SANDBOX_WORK="$W" \
               PLIWEE_SANDBOX_OUTBOX="$O" "$SELF" probe 2>&1)"; rc=$?
    { [ "$rc" -ne 0 ] && contains "$out" "GH_TOKEN is set" && contains "$out" "is visible and holds files" \
        && contains "$out" "is writable"; } \
        && ok "probe REJECTS an unsandboxed environment: token, visible gh login, writable \$HOME" \
        || notok "the probe passed outside a sandbox: $out"
    out="$(env -u PLIWEE_SANDBOX_HIDDEN "$SELF" probe 2>&1)"; rc=$?
    [ "$rc" -ne 0 ] && ok "probe REJECTS an empty list of paths to check" || notok "probe passed with nothing to check"

    kill "$spid" 2>/dev/null; wait "$spid" 2>/dev/null
    printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
    [ "$PASS" -gt 0 ] && [ "$FAIL" -eq 0 ]
}

case "${1:-}" in
    run)        shift; cmd_run "$@" ;;
    probe)      cmd_probe ;;
    --selftest) selftest ;;
    *)          sed -n '2,8p' "$SELF" | sed 's/^# \{0,1\}//' >&2; exit 1 ;;
esac
