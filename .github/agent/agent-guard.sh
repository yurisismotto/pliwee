#!/usr/bin/env bash
# agent-guard.sh — the mechanical checks an autonomous issue worker must pass
# before it may work on an issue, commit, or push.
#
#   agent-guard.sh identity          the effective git identity is the owner's
#   agent-guard.sh branch ISSUE      HEAD is feature/issue-ISSUE-<slug>, nothing else
#   agent-guard.sh commits BASE      every commit in BASE..HEAD is the owner's,
#                                    is not a merge, and carries no AI attribution
#   agent-guard.sh paths BASE        no file changed in BASE...HEAD is one the
#                                    worker may not touch (its guard rails, CI,
#                                    AGENTS.md, historical evidence)
#   agent-guard.sh issue ISSUE       the issue is approved and nothing open blocks it
#   agent-guard.sh message FILE      a commit message carries no AI attribution
#   agent-guard.sh push              pre-push: reads git's ref lines on stdin and
#                                    refuses main, deletion and non-fast-forward
#   agent-guard.sh --selftest        prove every check rejects what it must
#
# Exit status: 0 the check passed · 1 the check failed or could not measure ·
# 3 (issue only) the issue is not ready, with the reasons on stdout, one per
# line, prefixed "blocked: " — the worker turns those into its agent:blocked
# comment.
#
# WHAT THIS DOES NOT DO
# ---------------------
# It cannot tell whether an ADR an issue needs has been accepted, whether a
# SPEC exists, or whether an owner decision is still open: those live in prose,
# and judging them is the worker's job (.claude/skills/pliwee-issue-worker).
# A 0 from `issue` means "not mechanically blocked", never "ready to build".
#
# THE OWNER'S IDENTITY IS AN INPUT, NOT A DEFAULT
# -----------------------------------------------
# PLIWEE_OWNER_NAME, PLIWEE_OWNER_EMAIL and PLIWEE_OWNER_LOGIN are set by the
# owner on the host that runs the worker (docs/development/AGENT-WORKFLOW.md).
# None has a default here: a check that compared the identity against a value it
# made up would pass on nothing, which AGENTS.md calls invalid. When one is
# missing the check fails and says which.
#
# AGENTS.md § Git Authorship Policy: when the identity is wrong, the worker
# STOPS. Nothing here, and nothing that calls it, repairs it.

set -uo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SELF="$HERE/$(basename -- "${BASH_SOURCE[0]}")"
REPO="$(cd -- "$HERE/../.." && pwd)"
# shellcheck source=../../packaging/tests/lib/assert.sh
. "$REPO/packaging/tests/lib/assert.sh"

fail() { printf 'agent-guard: FAILED: %s\n' "$*" >&2; return 1; }

# need_env VAR... — every named variable is set and non-empty.
need_env() {
    local v missing=()
    for v in "$@"; do [ -n "${!v:-}" ] || missing+=("$v"); done
    [ "${#missing[@]}" -eq 0 ] \
        || fail "not configured on this host: ${missing[*]} — without it there is nothing to compare against"
}

# The patterns AGENTS.md forbids, matched line by line against a commit message.
# Any Co-authored-by is refused, not only an AI one: a worker commit has exactly
# one author, the owner, and a co-author line in it can only be noise or worse.
ATTRIBUTION_RE='^[[:space:]]*(co-authored-by|generated-by|assisted-by|ai-assisted-by|made-with)[[:space:]]*:|noreply@anthropic\.com|generated with \[?claude|🤖'

# ---------------------------------------------------------------------------
# identity — `git var` resolves what a commit made now would record, including
# GIT_AUTHOR_* / GIT_COMMITTER_* in the environment, which `git config` alone
# would not see.
# ---------------------------------------------------------------------------
check_identity() {
    need_env PLIWEE_OWNER_NAME PLIWEE_OWNER_EMAIL || return 1
    local want="$PLIWEE_OWNER_NAME <$PLIWEE_OWNER_EMAIL>" role ident who
    for role in AUTHOR COMMITTER; do
        ident="$(git var "GIT_${role}_IDENT" 2>/dev/null)" \
            || { fail "git has no $role identity configured — STOP, the worker must not set one"; return 1; }
        who="${ident% * *}"     # drop the trailing "<epoch> <tz>"
        [ "$who" = "$want" ] \
            || { fail "the $role identity is '$who', the owner's is '$want' — STOP, the worker must not repair it"; return 1; }
    done
    printf 'ok    identity: author and committer are %s\n' "$want"
}

# ---------------------------------------------------------------------------
# branch ISSUE — one issue, one branch, and never a long-lived one. The pattern
# excludes main, develop and release/* by construction.
# ---------------------------------------------------------------------------
check_branch() {
    local issue="${1:-}" b
    [[ "$issue" =~ ^[1-9][0-9]*$ ]] || { fail "branch: '$issue' is not an issue number"; return 1; }
    b="$(git symbolic-ref --short -q HEAD)" || { fail "branch: HEAD is detached"; return 1; }
    [[ "$b" =~ ^feature/issue-${issue}-[a-z0-9]+(-[a-z0-9]+)*$ ]] \
        || { fail "branch: on '$b'; issue #$issue is worked on feature/issue-$issue-<slug> only"; return 1; }
    printf 'ok    branch: %s\n' "$b"
}

# ---------------------------------------------------------------------------
# commits BASE — an empty range is a failure, not a pass: a check over zero
# commits has measured nothing (AGENTS.md, "the capture is non-empty").
# ---------------------------------------------------------------------------
check_commits() {
    local base="${1:-}" revs=() r an ae cn ce msg hit bad=0
    need_env PLIWEE_OWNER_NAME PLIWEE_OWNER_EMAIL || return 1
    [ -n "$base" ] && git rev-parse --verify -q "$base^{commit}" >/dev/null \
        || { fail "commits: base '$base' is not a commit"; return 1; }
    mapfile -t revs < <(git rev-list "$base..HEAD")
    [ "${#revs[@]}" -gt 0 ] || { fail "commits: $base..HEAD is empty — nothing was measured"; return 1; }
    if [ -n "$(git rev-list --merges "$base..HEAD")" ]; then
        fail "commits: $base..HEAD contains a merge commit; the worker rebases nothing and merges nothing"; bad=1
    fi
    for r in "${revs[@]}"; do
        an="$(git show -s --format=%an "$r")"; ae="$(git show -s --format=%ae "$r")"
        cn="$(git show -s --format=%cn "$r")"; ce="$(git show -s --format=%ce "$r")"
        if [ "$an" != "$PLIWEE_OWNER_NAME" ] || [ "$ae" != "$PLIWEE_OWNER_EMAIL" ]; then
            fail "commits: ${r:0:9} is authored by '$an <$ae>', not the owner"; bad=1
        fi
        if [ "$cn" != "$PLIWEE_OWNER_NAME" ] || [ "$ce" != "$PLIWEE_OWNER_EMAIL" ]; then
            fail "commits: ${r:0:9} is committed by '$cn <$ce>', not the owner"; bad=1
        fi
        msg="$(git show -s --format=%B "$r")"
        hit="$(grep -inE -- "$ATTRIBUTION_RE" <<<"$msg" || true)"
        if [ -n "$hit" ]; then
            fail "commits: ${r:0:9} carries attribution AGENTS.md forbids: $hit"; bad=1
        fi
    done
    [ "$bad" -eq 0 ] || return 1
    printf 'ok    commits: %d in %s..HEAD, all by the owner, no merges, no attribution\n' "${#revs[@]}" "$base"
}

# ---------------------------------------------------------------------------
# paths BASE — what the worker may not change, checked on what it committed
# rather than trusted to its permission rules (which are prefix matches, and
# advisory). Renames are split so a file moved out of a protected directory
# still counts as touching it. An empty change set is a failure: nothing was
# measured.
# ---------------------------------------------------------------------------
PROTECTED_RE='^(\.github/workflows/|\.github/agent/|\.github/actionlint\.yaml$|\.claude/skills/pliwee-issue-worker/|AGENTS\.md$|docs/(audits|certification|reports)/)'

check_paths() {
    local base="${1:-}" mb changed hit
    [ -n "$base" ] && git rev-parse --verify -q "$base^{commit}" >/dev/null \
        || { fail "paths: base '$base' is not a commit"; return 1; }
    mb="$(git merge-base "$base" HEAD)" || { fail "paths: HEAD shares no history with $base"; return 1; }
    changed="$(git diff --no-renames --name-only "$mb" HEAD)"
    need_nonempty "files changed in $base...HEAD" "$changed" || return 1
    hit="$(grep -E -- "$PROTECTED_RE" <<<"$changed" || true)"
    [ -z "$hit" ] || { fail "paths: the worker changed what it may not: $(tr '\n' ' ' <<<"$hit")"; return 1; }
    printf 'ok    paths: %d file(s) changed in %s...HEAD, none protected\n' "$(grep -c '' <<<"$changed")" "$base"
}

# ---------------------------------------------------------------------------
# message FILE — the commit-msg hook. Git's own "# ..." help lines are ignored.
# ---------------------------------------------------------------------------
check_message() {
    local file="${1:-}" msg hit
    [ -f "$file" ] || { fail "message: '$file' is not a file"; return 1; }
    msg="$(grep -v '^#' -- "$file")"
    need_nonempty "commit message" "$msg" || return 1
    hit="$(grep -inE -- "$ATTRIBUTION_RE" <<<"$msg" || true)"
    [ -z "$hit" ] || { fail "message carries attribution AGENTS.md forbids: $hit"; return 1; }
    printf 'ok    message: no attribution\n'
}

# ---------------------------------------------------------------------------
# push — the pre-push hook. Git writes one line per ref:
#   <local ref> <local sha> <remote ref> <remote sha>
# A worker pushes its own feature/issue-N-<slug> branch, under the same name,
# fast-forward only. Everything else — main, develop, a tag, a deletion, a
# rewrite of history already pushed — is refused. Zero lines is refused too: a
# hook that read nothing approved nothing.
# ---------------------------------------------------------------------------
check_push() {
    local lref lsha rref rsha n=0 bad=0 zero='^0+$'
    while read -r lref lsha rref rsha; do
        [ -n "${lref:-}" ] || continue
        n=$((n + 1))
        if [[ "$lsha" =~ $zero ]]; then
            fail "push: deleting $rref is not something a worker does"; bad=1; continue
        fi
        if ! [[ "$rref" =~ ^refs/heads/feature/issue-[1-9][0-9]*-[a-z0-9]+(-[a-z0-9]+)*$ ]]; then
            fail "push: $rref is not a feature/issue-N-<slug> branch"; bad=1; continue
        fi
        if [ "$lref" != "$rref" ]; then
            fail "push: $lref pushed as $rref; a worker pushes a branch under its own name"; bad=1; continue
        fi
        if ! [[ "$rsha" =~ $zero ]] && ! git merge-base --is-ancestor "$rsha" "$lsha" 2>/dev/null; then
            fail "push: $rref would not fast-forward; a worker never rewrites what it pushed"; bad=1
        fi
    done
    [ "$n" -gt 0 ] || { fail "push: no ref lines on stdin — nothing was checked"; return 1; }
    [ "$bad" -eq 0 ] || return 1
    printf 'ok    push: %d ref(s), all feature/issue branches, fast-forward\n' "$n"
}

# ---------------------------------------------------------------------------
# issue ISSUE — the mechanical half of the dependency gate.
#
# Data comes from GitHub, or from PLIWEE_AGENT_FIXTURE_DIR/<n>/{issue,editor,
# blocked_by}.json when the self-test sets it. A fetch that fails is exit 1, never
# "ready": no data is not the same as no blockers.
# ---------------------------------------------------------------------------
fetch() { # fetch ISSUE KIND
    local n="$1" kind="$2"
    if [ -n "${PLIWEE_AGENT_FIXTURE_DIR:-}" ]; then
        cat -- "$PLIWEE_AGENT_FIXTURE_DIR/$n/$kind.json"
        return
    fi
    local repo="${GITHUB_REPOSITORY:-${PLIWEE_REPO:-}}"
    case "$kind" in
        issue)      gh api "repos/$repo/issues/$n" ;;
        blocked_by) gh api --paginate "repos/$repo/issues/$n/dependencies/blocked_by" | jq -s 'add // []' ;;
        editor)     gh api graphql -F n="$n" -f o="${repo%/*}" -f r="${repo#*/}" -f query='
                      query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){
                        issue(number:$n){editor{login}}}}' \
                      | jq '{editor: .data.repository.issue.editor.login}' ;;
    esac
}

check_issue() {
    local n="${1:-}" issue editor blocked labels reasons=() author state
    [[ "$n" =~ ^[1-9][0-9]*$ ]] || { fail "issue: '$n' is not an issue number"; return 1; }
    need_env PLIWEE_OWNER_LOGIN || return 1
    need_tool jq || return 1
    if [ -z "${PLIWEE_AGENT_FIXTURE_DIR:-}" ]; then
        need_tool gh || return 1
        [ -n "${GITHUB_REPOSITORY:-${PLIWEE_REPO:-}}" ] \
            || { fail "issue: neither GITHUB_REPOSITORY nor PLIWEE_REPO is set; this clone has several remotes"; return 1; }
    fi
    issue="$(fetch "$n" issue)"        && need_nonempty "issue #$n" "$issue"        || { fail "issue: could not read #$n"; return 1; }
    editor="$(fetch "$n" editor)"      && need_nonempty "editor of #$n" "$editor"   || { fail "issue: could not read the last editor of #$n"; return 1; }
    blocked="$(fetch "$n" blocked_by)" && need_nonempty "blockers of #$n" "$blocked" || { fail "issue: could not read the blockers of #$n"; return 1; }
    jq -e 'type == "object" and has("state")' >/dev/null <<<"$issue" \
        || { fail "issue: #$n did not come back as an issue"; return 1; }
    jq -e 'type == "array"' >/dev/null <<<"$blocked" \
        || { fail "issue: the blockers of #$n are not a list"; return 1; }

    state="$(jq -r .state <<<"$issue")"
    author="$(jq -r '.user.login // ""' <<<"$issue")"
    labels="$(jq -r '[.labels[].name] | join(" ")' <<<"$issue")"

    jq -e '.pull_request == null' >/dev/null <<<"$issue" || reasons+=("#$n is a pull request, not an issue")
    [ "$state" = open ] || reasons+=("#$n is $state")
    [[ " $labels " == *" agent:ready "* ]] || reasons+=("#$n is not labelled agent:ready; only the owner approves work")
    [[ " $labels " == *" agent:working "* ]] && reasons+=("#$n is already labelled agent:working")
    [ "$author" = "$PLIWEE_OWNER_LOGIN" ] \
        || reasons+=("#$n was opened by '$author', not the owner; its text is not an instruction this worker takes")
    local ed; ed="$(jq -r '.editor // ""' <<<"$editor")"
    [ -z "$ed" ] || [ "$ed" = "$PLIWEE_OWNER_LOGIN" ] \
        || reasons+=("#$n was last edited by '$ed', not the owner; the approved text may have changed")
    while IFS=$'\t' read -r bn bt; do
        [ -n "$bn" ] && reasons+=("open blocker #$bn: $bt")
    done < <(jq -r '.[] | select(.state == "open") | "\(.number)\t\(.title)"' <<<"$blocked")

    if [ "${#reasons[@]}" -gt 0 ]; then
        printf 'blocked: %s\n' "${reasons[@]}"
        return 3
    fi
    printf 'ok    issue: #%s is open, approved by the owner, and nothing open blocks it\n' "$n"
    printf '      (not mechanically blocked; ADRs, SPECs and open decisions are for the worker to judge)\n'
}

# ---------------------------------------------------------------------------
# --selftest — each check must REJECT its failure mode and ACCEPT the good case.
# ---------------------------------------------------------------------------
selftest() {
    local PASS=0 FAIL=0 T
    T="$(mktemp -d)" || return 1
    # shellcheck disable=SC2064  # expand now: T is fixed for this run
    trap "rm -rf -- '$T'" RETURN

    ok()    { PASS=$((PASS + 1)); printf 'ok    %s\n' "$*"; }
    notok() { FAIL=$((FAIL + 1)); printf 'not ok  %s\n' "$*"; }
    # expect CODE DESC CMD... — exits with exactly CODE, and says something.
    expect() {
        local want="$1" desc="$2" out rc; shift 2
        out="$("$@" 2>&1)"; rc=$?
        if [ "$rc" -ne "$want" ]; then notok "$desc — exit $rc, wanted $want: ${out//$'\n'/ | }"
        elif [ -z "${out//[[:space:]]/}" ]; then notok "$desc — exit $rc but no diagnostic"
        else ok "$desc"; fi
    }

    export PLIWEE_OWNER_NAME="Owner Name" PLIWEE_OWNER_EMAIL="owner@example.org" PLIWEE_OWNER_LOGIN="owner"
    unset GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL
    export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1

    local r="$T/repo"
    git init -q -b main "$r" && cd "$r" || return 1
    git config user.name "$PLIWEE_OWNER_NAME"; git config user.email "$PLIWEE_OWNER_EMAIL"
    git commit -q --allow-empty -m base; git tag base

    printf '\n== identity ==\n'
    expect 0 "ACCEPTS the owner's identity"                         "$SELF" identity
    expect 1 "REJECTS a bot author email in the environment"        env GIT_AUTHOR_EMAIL="41898282+claude[bot]@users.noreply.github.com" "$SELF" identity
    git config user.name "claude[bot]"
    expect 1 "REJECTS a bot user.name in the repository config"     "$SELF" identity
    git config user.name "$PLIWEE_OWNER_NAME"
    expect 1 "REJECTS a committer overridden in the environment"    env GIT_COMMITTER_NAME="claude[bot]" "$SELF" identity
    expect 1 "REJECTS when the owner's identity is not configured"  env -u PLIWEE_OWNER_EMAIL "$SELF" identity

    printf '\n== branch ==\n'
    git switch -q -c feature/issue-42-small-fix
    expect 0 "ACCEPTS feature/issue-42-small-fix for #42"           "$SELF" branch 42
    expect 1 "REJECTS the same branch for another issue"            "$SELF" branch 4
    expect 1 "REJECTS a non-number"                                 "$SELF" branch 42abc
    git switch -q main
    expect 1 "REJECTS main"                                         "$SELF" branch 42
    git switch -q -c feature/issue-42-
    expect 1 "REJECTS a branch with no slug"                        "$SELF" branch 42
    git switch -q --detach main
    expect 1 "REJECTS a detached HEAD"                              "$SELF" branch 42

    printf '\n== commits ==\n'
    git switch -q feature/issue-42-small-fix
    expect 1 "REJECTS an empty range — nothing measured"            "$SELF" commits base
    git commit -q --allow-empty -m "fix: one small thing"
    expect 0 "ACCEPTS one clean commit by the owner"                "$SELF" commits base
    expect 1 "REJECTS a base that is not a commit"                  "$SELF" commits no-such-ref
    expect 1 "REJECTS when the owner's identity is not configured"  env -u PLIWEE_OWNER_NAME "$SELF" commits base
    local m
    for m in "Co-Authored-By: Claude <noreply@anthropic.com>" "Co-authored-by: Someone <s@example.org>" \
             "Generated-By: an agent" "🤖 Generated with [Claude Code](https://claude.com/claude-code)"; do
        git commit -q --allow-empty -m "fix: thing" -m "$m"
        expect 1 "REJECTS a message carrying '${m%%:*}'"            "$SELF" commits base
        git reset -q --hard HEAD~1
    done
    git -c user.name="claude[bot]" -c user.email="41898282+claude[bot]@users.noreply.github.com" \
        commit -q --allow-empty -m "fix: by a bot"
    expect 1 "REJECTS a commit authored by a bot"                   "$SELF" commits base
    git reset -q --hard HEAD~1
    GIT_COMMITTER_NAME="Someone Else" git commit -q --allow-empty -m "fix: committed by another"
    expect 1 "REJECTS a commit committed by someone else"           "$SELF" commits base
    git reset -q --hard HEAD~1
    git switch -q -c side base && git commit -q --allow-empty -m side && git switch -q feature/issue-42-small-fix
    git merge -q --no-ff --no-edit side
    expect 1 "REJECTS a merge commit"                               "$SELF" commits base

    printf '\n== paths ==\n'
    local keep; keep="$(git rev-parse HEAD)"     # restored below; push needs this state
    expect 1 "REJECTS a range that changed no file"                 "$SELF" paths base
    mkdir -p desktop
    printf 'x\n' > desktop/lib.rs; git add desktop/lib.rs; git commit -q -m "fix: code"
    expect 0 "ACCEPTS a change to product code"                     "$SELF" paths base
    expect 1 "REJECTS a base that is not a commit"                  "$SELF" paths no-such-ref
    local pp
    for pp in .github/agent/agent-guard.sh .github/workflows/ci.yml AGENTS.md docs/reports/x.md \
              .claude/skills/pliwee-issue-worker/SKILL.md .github/actionlint.yaml; do
        mkdir -p "$(dirname "$pp")"; printf 'x\n' > "$pp"; git add -- "$pp"; git commit -q -m "chore: $pp"
        expect 1 "REJECTS a change to $pp"                          "$SELF" paths base
        git reset -q --hard HEAD~1
    done
    mkdir -p docs/reports; git mv desktop/lib.rs docs/reports/lib.rs; git commit -q -m "chore: move"
    expect 1 "REJECTS a file renamed into docs/reports/"            "$SELF" paths base
    git reset -q --hard "$keep"

    printf '\n== message ==\n'
    printf 'fix: a thing\n\nWhy it was wrong.\n# Please enter the commit message\n' > "$T/msg-ok"
    printf 'fix: a thing\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n' > "$T/msg-bad"
    printf '# only git help text\n' > "$T/msg-empty"
    expect 0 "ACCEPTS a plain message"                              "$SELF" message "$T/msg-ok"
    expect 1 "REJECTS a co-author trailer"                          "$SELF" message "$T/msg-bad"
    expect 1 "REJECTS an empty message"                             "$SELF" message "$T/msg-empty"
    expect 1 "REJECTS a missing file"                               "$SELF" message "$T/nope"

    printf '\n== push ==\n'
    local Z=0000000000000000000000000000000000000000 h1 h0 hs
    git reset -q --hard HEAD~1                   # drop the merge
    h1="$(git rev-parse HEAD)"; h0="$(git rev-parse base)"; hs="$(git rev-parse side)"
    local fb=refs/heads/feature/issue-42-small-fix
    expect 0 "ACCEPTS a new feature/issue branch"                   sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "$fb $h1 $fb $Z"
    expect 0 "ACCEPTS a fast-forward"                               sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "$fb $h1 $fb $h0"
    expect 1 "REJECTS a push to main"                               sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "$fb $h1 refs/heads/main $h0"
    expect 1 "REJECTS a branch pushed under another name"           sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "refs/heads/side $hs $fb $Z"
    expect 1 "REJECTS a non-fast-forward"                           sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "$fb $h0 $fb $h1"
    expect 1 "REJECTS a deletion"                                   sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "(delete) $Z $fb $h1"
    expect 1 "REJECTS a tag"                                        sh -c 'printf "%s\n" "$1" | "$0" push' "$SELF" "refs/tags/v9 $h1 refs/tags/v9 $Z"
    expect 1 "REJECTS one bad ref among good ones"                  sh -c 'printf "%s\n%s\n" "$1" "$2" | "$0" push' "$SELF" "$fb $h1 $fb $Z" "$fb $h1 refs/heads/main $h0"
    expect 1 "REJECTS an empty stdin — nothing checked"             sh -c '"$0" push </dev/null' "$SELF"
    cd "$T" || return 1

    printf '\n== issue ==\n'
    local f="$T/fx"
    # mk N STATE AUTHOR EDITOR LABELS BLOCKED_JSON [PR]
    mk() {
        mkdir -p "$f/$1"
        jq -n --arg s "$2" --arg a "$3" --arg l "$5" --argjson pr "${7:-null}" \
            '{state:$s, user:{login:$a}, pull_request:$pr, labels:($l|split(" ")|map(select(.!=""))|map({name:.}))}' > "$f/$1/issue.json"
        jq -n --arg e "$4" '{editor: (if $e == "" then null else $e end)}' > "$f/$1/editor.json"
        printf '%s\n' "$6" > "$f/$1/blocked_by.json"
    }
    mk 1 open   owner ""      "agent:ready roadmap" '[]'
    mk 2 open   owner owner   "agent:ready"         '[{"number":5,"state":"closed","title":"done"}]'
    mk 3 open   owner ""      "agent:ready"         '[{"number":5,"state":"open","title":"Capability negotiation"}]'
    mk 4 open   owner ""      "roadmap"             '[]'
    mk 5 closed owner ""      "agent:ready"         '[]'
    mk 6 open   stranger ""   "agent:ready"         '[]'
    mk 7 open   owner stranger "agent:ready"        '[]'
    mk 8 open   owner ""      "agent:ready agent:working" '[]'
    mk 9 open   owner ""      "agent:ready"         '[]' '{"url":"x"}'
    mkdir -p "$f/10"; printf '{"state":"open","user":{"login":"owner"},"labels":[]}\n' > "$f/10/issue.json"
    printf '{"editor":null}\n' > "$f/10/editor.json"   # no blocked_by.json: the fetch fails
    export PLIWEE_AGENT_FIXTURE_DIR="$f"
    expect 0 "ACCEPTS an approved owner issue with no blockers"     "$SELF" issue 1
    expect 0 "ACCEPTS a blocker that is closed"                     "$SELF" issue 2
    expect 3 "REJECTS an open blocker"                              "$SELF" issue 3
    expect 3 "REJECTS an issue without agent:ready"                 "$SELF" issue 4
    expect 3 "REJECTS a closed issue"                               "$SELF" issue 5
    expect 3 "REJECTS an issue opened by someone else"              "$SELF" issue 6
    expect 3 "REJECTS an issue last edited by someone else"         "$SELF" issue 7
    expect 3 "REJECTS an issue already being worked"                "$SELF" issue 8
    expect 3 "REJECTS a pull request"                               "$SELF" issue 9
    expect 1 "REJECTS — as an error, not ready — a failed fetch"    "$SELF" issue 10
    expect 1 "REJECTS a missing issue"                              "$SELF" issue 404
    expect 1 "REJECTS when the owner's login is not configured"     env -u PLIWEE_OWNER_LOGIN "$SELF" issue 1
    local out; out="$("$SELF" issue 3 2>&1)"
    contains "$out" "blocked: open blocker #5: Capability negotiation" \
        && ok "a blocked issue names the blocker it is waiting for" \
        || notok "a blocked issue does not name its blocker: $out"

    printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
    [ "$PASS" -gt 0 ] && [ "$FAIL" -eq 0 ]
}

case "${1:-}" in
    identity)   check_identity ;;
    branch)     check_branch "${2:-}" ;;
    commits)    check_commits "${2:-}" ;;
    paths)      check_paths "${2:-}" ;;
    issue)      check_issue "${2:-}" ;;
    message)    check_message "${2:-}" ;;
    push)       check_push ;;
    --selftest) selftest ;;
    *)          sed -n '2,17p' "$SELF" | sed 's/^# \{0,1\}//' >&2; exit 1 ;;
esac
