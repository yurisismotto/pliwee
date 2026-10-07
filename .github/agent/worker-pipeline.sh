#!/usr/bin/env bash
# worker-pipeline.sh — the issue worker's trusted publication path, as one
# script the workflow and its end-to-end self-test both run.
#
#   worker-pipeline.sh prepare ISSUE   fresh clone into $WORK, branch from main;
#                                      prints the base SHA
#   worker-pipeline.sh bundle ISSUE    (in the sandbox) the work as a git bundle
#   worker-pipeline.sh verify ISSUE BASE
#                                      a clean clone in $PUB; fetch the bundle;
#                                      the trusted guard on refs only
#   worker-pipeline.sh push ISSUE      push from $PUB; the remote must equal it
#   worker-pipeline.sh --selftest      the whole path against a local remote,
#                                      with a well-behaved worker and hostile ones
#
# Environment: WORK OUTBOX PUB (directories), TRUSTED (the trusted copy's root),
# PLIWEE_REPO, PLIWEE_REMOTE_URL (default https://github.com/$PLIWEE_REPO.git),
# PLIWEE_OWNER_NAME / _EMAIL.
#
# Why it is shaped like this: the worker's work copy is written by arbitrary
# code (the model, and the builds it runs). Git executes configuration — a
# core.fsmonitor or core.hooksPath in .git/config, a filter driver — so no
# trusted step ever runs git inside the work copy. The work leaves as a bundle
# (objects and one ref, no configuration), and everything after that runs in a
# clone this script made, with hooks from the trusted copy, without a checkout.

set -uo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SELF="$HERE/$(basename -- "${BASH_SOURCE[0]}")"
REPO="$(cd -- "$HERE/../.." && pwd)"
# shellcheck source=../../packaging/tests/lib/assert.sh
. "$REPO/packaging/tests/lib/assert.sh"

fail() { printf 'worker-pipeline: FAILED: %s\n' "$*" >&2; return 1; }
remote() { printf '%s\n' "${PLIWEE_REMOTE_URL:-https://github.com/${PLIWEE_REPO:?PLIWEE_REPO is not set}.git}"; }
num() { [[ "${1:-}" =~ ^[1-9][0-9]*$ ]] || { fail "'${1:-}' is not an issue number"; return 1; }; }
dirs() { local v; for v in "$@"; do [ -n "${!v:-}" ] || { fail "$v is not set"; return 1; }; done; }

cmd_prepare() {
    num "$1" || return 1; dirs WORK OUTBOX || return 1
    local b="feature/issue-$1-worker" r; r="$(remote)"
    if git ls-remote --exit-code --heads "$r" "refs/heads/$b" >/dev/null 2>&1; then
        fail "$b already exists on the remote; refusing to reuse or rewrite it"; return 1
    fi
    rm -rf -- "$WORK" "$OUTBOX"; mkdir -p "$OUTBOX/derived" "$OUTBOX/decision" || return 1
    git clone -q "$r" "$WORK" || { fail "clone failed"; return 1; }
    git -C "$WORK" switch -q -c "$b" origin/main || return 1
    git -C "$WORK" config core.hooksPath /tmp/.pliwee-sandbox/.github/agent/hooks
    git -C "$WORK" rev-parse origin/main
}

cmd_bundle() { # runs git only inside the sandbox
    num "$1" || return 1; dirs WORK OUTBOX TRUSTED || return 1
    "$TRUSTED/.github/agent/sandbox.sh" run --work "$WORK" --outbox "$OUTBOX" -- \
        git bundle create -q "$OUTBOX/work.bundle" "origin/main..refs/heads/feature/issue-$1-worker"
}

cmd_verify() {
    num "$1" || return 1; dirs OUTBOX PUB TRUSTED || return 1
    local b="feature/issue-$1-worker" base="${2:-}" g="$TRUSTED/.github/agent/agent-guard.sh"
    [ -n "$base" ] || { fail "verify: no base SHA"; return 1; }
    [ -s "$OUTBOX/work.bundle" ] || { fail "verify: no bundle — the worker committed nothing"; return 1; }
    rm -rf -- "$PUB"
    git clone -q --no-checkout "$(remote)" "$PUB" || return 1
    git -C "$PUB" config core.hooksPath "$TRUSTED/.github/agent/hooks"
    git -C "$PUB" bundle verify -q "$OUTBOX/work.bundle" 2>/dev/null || { fail "verify: the bundle does not verify against main"; return 1; }
    git -C "$PUB" bundle list-heads "$OUTBOX/work.bundle" | awk '{print $2}' | sort -u > "$PUB.heads" || return 1
    [ "$(cat "$PUB.heads")" = "refs/heads/$b" ] || { fail "verify: the bundle carries $(tr '\n' ' ' < "$PUB.heads"), not exactly refs/heads/$b"; return 1; }
    git -C "$PUB" fetch -q "$OUTBOX/work.bundle" "refs/heads/$b:refs/heads/$b" || return 1
    git -C "$PUB" symbolic-ref HEAD "refs/heads/$b"
    [ "$(git -C "$PUB" merge-base origin/main "$b")" = "$base" ] || { fail "verify: the work does not start from main at $base"; return 1; }
    ( cd "$PUB" && "$g" branch "$1" && "$g" commits origin/main && "$g" paths origin/main ) || return 1
    git -C "$PUB" diff --check "origin/main...$b" || { fail "verify: whitespace errors"; return 1; }
    printf 'ok    %s verified from the bundle, in a clean clone\n' "$b"
}

cmd_push() {
    num "$1" || return 1; dirs PUB || return 1
    local b="feature/issue-$1-worker" l r
    PLIWEE_AGENT_BASE=origin/main git -C "$PUB" push -q -u origin "refs/heads/$b:refs/heads/$b" || { fail "push refused"; return 1; }
    l="$(git -C "$PUB" rev-parse "refs/heads/$b")"
    r="$(git ls-remote --heads "$(remote)" "refs/heads/$b" | awk '{print $1}')"
    if [ -z "$r" ] || [ "$l" != "$r" ]; then fail "the remote branch is not the verified HEAD"; return 1; fi
    printf 'ok    published %s at %s\n' "$b" "$l"
}

# ---------------------------------------------------------------------------
# --selftest — the end-to-end canary of this path, offline.
# ---------------------------------------------------------------------------
selftest() {
    local PASS=0 FAIL=0 T
    ok()    { PASS=$((PASS + 1)); printf 'ok    %s\n' "$*"; }
    notok() { FAIL=$((FAIL + 1)); printf 'not ok  %s\n' "$*"; }
    need_tool bwrap git jq || { echo "PRECONDITION FAILED: bwrap, git and jq are needed"; return 3; }
    T="$(mktemp -d "${TMPDIR:-/tmp}/pipeline-selftest.XXXXXXXX")" || return 1
    # shellcheck disable=SC2064
    trap "rm -rf -- '$T'" RETURN

    export PLIWEE_OWNER_NAME="Owner Name" PLIWEE_OWNER_EMAIL="owner@example.org" PLIWEE_OWNER_LOGIN=owner
    export GIT_CONFIG_NOSYSTEM=1 HOME="$T/home"; mkdir -p "$HOME/.config/gh"
    printf '[user]\n\tname = %s\n\temail = %s\n[init]\n\tdefaultBranch = main\n' "$PLIWEE_OWNER_NAME" "$PLIWEE_OWNER_EMAIL" > "$HOME/.gitconfig"
    echo 'oauth_token: gho_PIPELINE_SECRET' > "$HOME/.config/gh/hosts.yml"
    unset GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

    # "GitHub": a bare repository whose main holds the trusted tree.
    local seed="$T/seed"; git init -q "$seed"
    mkdir -p "$seed/.github" "$seed/packaging/tests/lib" "$seed/src"
    cp -a "$REPO/.github/agent" "$seed/.github/agent"; cp "$REPO/packaging/tests/lib/assert.sh" "$seed/packaging/tests/lib/"
    echo 'one' > "$seed/src/file.txt"
    git -C "$seed" add -A && git -C "$seed" commit -q -m "base"
    git clone -q --bare "$seed" "$T/remote.git"
    export PLIWEE_REMOTE_URL="$T/remote.git"
    export TRUSTED="$T/trusted"; mkdir -p "$TRUSTED/.github" "$TRUSTED/packaging/tests/lib"
    cp -a "$REPO/.github/agent" "$TRUSTED/.github/agent"; cp "$REPO/packaging/tests/lib/assert.sh" "$TRUSTED/packaging/tests/lib/"
    export WORK="$T/work" OUTBOX="$T/outbox" PUB="$T/pub"
    local SB="$TRUSTED/.github/agent/sandbox.sh" base n=40

    # run_case N DESC EXPECT(published|refused) WORKER-SCRIPT [WHY]
    # A refusal counts only when its output names WHY: refused for some other
    # reason (no bundle, a broken fixture) is not the refusal being tested.
    run_case() {
        local issue="$1" desc="$2" want="$3" script="$4" why="${5:-}" out rc got
        rm -rf -- "$T/pwned"
        local before; before="$(git ls-remote --heads "$PLIWEE_REMOTE_URL" "refs/heads/feature/issue-$issue-worker" | awk '{print $1}')"
        if base="$("$SELF" prepare "$issue" 2>&1 | tail -1)"; then
            # Inline: /tmp is empty inside the sandbox, so a script file there would not exist.
            "$SB" run --work "$WORK" --outbox "$OUTBOX" -- bash -c "set -e; $script" >"$T/worker.out" 2>&1
        [ -z "${PIPELINE_DEBUG:-}" ] || { echo "--- worker output ($desc)"; cat "$T/worker.out"; }
            out="$( { "$SELF" bundle "$issue" && "$SELF" verify "$issue" "$base" && "$SELF" push "$issue"; } 2>&1 )"; rc=$?
        else out="prepare refused: $base"; rc=1; fi
        if [ -n "$before" ]; then
            # The branch existed: the only acceptable outcome is that it did not move.
            [ "$(git ls-remote --heads "$PLIWEE_REMOTE_URL" "refs/heads/feature/issue-$issue-worker" | awk '{print $1}')" = "$before" ] \
                && got=refused || got=published
        elif git ls-remote --exit-code --heads "$PLIWEE_REMOTE_URL" "refs/heads/feature/issue-$issue-worker" >/dev/null 2>&1; then got=published; else got=refused; fi
        if [ -e "$T/pwned" ]; then notok "$desc — code the worker planted ran OUTSIDE the sandbox: $(cat "$T/pwned")"
        elif [ "$got" != "$want" ]; then notok "$desc — $got, wanted $want (rc $rc): ${out//$'\n'/ | }"
        elif [ -n "$why" ] && ! contains "$out" "$why"; then notok "$desc — refused, but not because '$why': ${out//$'\n'/ | }"
        else ok "$desc"; fi
    }
    commit='git add -A && ./.github/agent/worker-commit.sh "fix: change the file" "Because the issue says so."'

    printf '\n== the good case ==\n'
    run_case $((n+=1)) "ACCEPTS: a worker that commits as the owner is verified and published" published \
        "echo two >> src/file.txt; $commit"
    git --git-dir="$T/remote.git" log -1 --format='%an <%ae>|%s' "refs/heads/feature/issue-$n-worker" | grep -q '^Owner Name <owner@example.org>|fix: change the file$' \
        && ok "the published commit is the owner's, with the worker's message" || notok "the published commit is not what the worker made"
    run_case "$n" "REJECTS: a second run for the same issue never rewrites a published branch" refused "echo three >> src/file.txt; $commit" "already exists on the remote"

    printf '\n== hostile workers ==\n'
    run_case $((n+=1)) "REJECTS: a worker that committed nothing" refused "echo 'did nothing'" "Refusing to create empty bundle"
    run_case $((n+=1)) "REJECTS: a commit carrying an AI attribution trailer" refused \
        "echo x >> src/file.txt; git add -A; git -c core.hooksPath=/dev/null commit -q -m 'fix: x' -m 'Co-Authored-By: Claude <noreply@anthropic.com>'" "carries attribution AGENTS.md forbids"
    run_case $((n+=1)) "REJECTS: a commit under another identity" refused \
        "echo x >> src/file.txt; git add -A; git -c core.hooksPath=/dev/null -c user.name=bot -c user.email=bot@example.org commit -q -m 'fix: x'" "is authored by 'bot <bot@example.org>'"
    run_case $((n+=1)) "REJECTS: a change to a workflow" refused \
        "mkdir -p .github/workflows; echo 'on: push' > .github/workflows/x.yml; $commit" "the worker changed what it may not: .github/workflows/x.yml"
    run_case $((n+=1)) "REJECTS: a change to the agent guard (the trusted copy judges it)" refused \
        "printf '#!/bin/sh\nexit 0\n' > .github/agent/agent-guard.sh; $commit" "the worker changed what it may not: .github/agent/agent-guard.sh"
    run_case $((n+=1)) "REJECTS: a merge commit" refused \
        "git switch -q -c side origin/main; echo s > s.txt; git add s.txt; git -c core.hooksPath=/dev/null commit -q -m side; git switch -q feature/issue-$n-worker; echo y >> src/file.txt; git add -A; git -c core.hooksPath=/dev/null commit -q -m 'fix: y'; git -c core.hooksPath=/dev/null merge -q --no-ff --no-edit side" "contains a merge commit"
    # The bundle step names its ref, so to smuggle one the worker must replace
    # the bundle after it is made — which it cannot: it has exited. Prove the
    # verifier refuses one anyway.
    n=$((n + 1)); base="$("$SELF" prepare "$n" 2>&1 | tail -1)"
    "$SB" run --work "$WORK" --outbox "$OUTBOX" -- bash -c "echo q >> src/file.txt; $commit" >/dev/null 2>&1
    git -C "$WORK" branch -f main HEAD    # main now carries the worker's commit too
    git -C "$WORK" bundle create -q "$OUTBOX/work.bundle" "origin/main..refs/heads/feature/issue-$n-worker" "origin/main..refs/heads/main"
    [ "$(git -C "$WORK" bundle list-heads "$OUTBOX/work.bundle" | wc -l)" -ge 2 ] \
        && ok "precondition: the hostile bundle really carries two refs" || notok "precondition: the hostile bundle carries one ref"
    out="$("$SELF" verify "$n" "$base" 2>&1)"
    contains "$out" "not exactly refs/heads/feature/issue-$n-worker" && ok "REJECTS: a bundle carrying more than the issue branch" || notok "a two-ref bundle verified: $out"

    printf '\n== code the worker plants for the trusted steps ==\n'
    run_case $((n+=1)) "a core.fsmonitor planted in the work copy never runs outside the sandbox" published \
        "git config core.fsmonitor 'touch $T/pwned; echo'; echo w >> src/file.txt; $commit"
    # Non-vacuous: the same planted config DOES run when git is invoked in the
    # work copy outside the sandbox — which is exactly what no step does.
    git -C "$WORK" status >/dev/null 2>&1
    [ -e "$T/pwned" ] && ok "precondition: that fsmonitor would run if a trusted step ran git in the work copy" \
        || notok "precondition: the planted fsmonitor does not fire even outside; the case above proves nothing"
    rm -f -- "$T/pwned"
    run_case $((n+=1)) "a core.hooksPath and filter driver planted in the work copy never run outside the sandbox" published \
        "mkdir -p h; printf '#!/bin/sh\ntouch $T/pwned\n' > h/pre-push; chmod +x h/pre-push; git config core.hooksPath \"\$PWD/h\"; git config filter.x.smudge 'touch $T/pwned'; echo '* filter=x' > .gitattributes; echo v >> src/file.txt; $commit; git config core.hooksPath h"
    out="$("$SB" run --work "$WORK" --outbox "$OUTBOX" -- bash -c 'cat ~/.config/gh/hosts.yml 2>&1; echo RAN' 2>&1)"
    if contains "$out" RAN && ! contains "$out" PIPELINE_SECRET; then ok "the worker cannot read the publisher's gh login"
    else notok "the gh login was readable to the worker: $out"; fi

    printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
    [ "$PASS" -gt 0 ] && [ "$FAIL" -eq 0 ]
}

case "${1:-}" in
    prepare)    cmd_prepare "${2:-}" ;;
    bundle)     cmd_bundle "${2:-}" ;;
    verify)     cmd_verify "${2:-}" "${3:-}" ;;
    push)       cmd_push "${2:-}" ;;
    --selftest) selftest ;;
    *)          sed -n '2,15p' "$SELF" | sed 's/^# \{0,1\}//' >&2; exit 1 ;;
esac
