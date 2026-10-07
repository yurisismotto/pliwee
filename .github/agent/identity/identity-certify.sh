#!/usr/bin/env bash
# identity-certify.sh — measure, on the server, what the worker App can and
# cannot do. Run by the owner after the App exists and the rulesets in
# rulesets/ are applied (docs/development/AGENT-WORKFLOW.md § Execution
# identity, steps 1–7). Gates G4–G8 of the certification.
#
#   identity-certify.sh run       every check below; evidence into $CERT_OUT
#
# Needs: the owner's own gh login (setup, the owner's half, cleanup); the App's
# installation token in $CERT_APP_TOKEN_FILE (app.sh token); $PLIWEE_WORKER_APP_BOT.
#
# Nothing here can reach main: every merge attempt targets `certify/main`,
# which the certify-mirror ruleset holds to main's review rules. Required
# checks are not mirrored — no workflow runs on a PR to certify/main — and are
# proved instead against main itself by the provenance probe.
#
# Each check states what it expects. A denial that succeeds is FAIL; a check
# whose precondition is missing is BLOCKED, never PASS. Exit 0 only when every
# check is PASS.

set -uo pipefail
REPO_SLUG="${PLIWEE_REPO:-yurisismotto/pliwee}"
OWNER="${PLIWEE_OWNER_LOGIN:-yurisismotto}"
BOT="${PLIWEE_WORKER_APP_BOT:-}"
OUT="${CERT_OUT:-$PWD/identity-cert-$(date -u +%Y%m%dT%H%M%SZ)}"
URL="https://github.com/$REPO_SLUG.git"
TAG="cert$(date +%s)"
mkdir -p "$OUT"; : > "$OUT/results.tsv"
PASS=0; FAIL=0; BLOCK=0

say() { printf '%s\n' "$*" | tee -a "$OUT/log.txt"; }
rec() { # ID VERDICT DESC EVIDENCE
    printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "${4//$'\n'/ }" >> "$OUT/results.tsv"
    case "$2" in PASS) PASS=$((PASS+1)) ;; FAIL) FAIL=$((FAIL+1)) ;; *) BLOCK=$((BLOCK+1)) ;; esac
    say "$(printf '%-5s %-7s %s — %s' "$1" "$2" "$3" "${4:0:220}")"
}
APP_TOKEN=""
app_api() { GH_TOKEN="$APP_TOKEN" gh api "$@" 2>&1; }
owner_api() { env -u GH_TOKEN gh api "$@" 2>&1; }
app_git() { # git with the App token as the only credential
    env GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0= \
        GIT_CONFIG_KEY_1=http.extraHeader \
        GIT_CONFIG_VALUE_1="AUTHORIZATION: basic $(printf 'x-access-token:%s' "$APP_TOKEN" | base64 -w0)" \
        GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/bin/false git "$@" 2>&1
}
# deny ID DESC CMD... — CMD must FAIL; its output is the evidence.
deny() { local id="$1" d="$2" out rc; shift 2; out="$("$@")"; rc=$?
    if [ "$rc" -ne 0 ]; then rec "$id" PASS "$d (refused)" "$out"; else rec "$id" FAIL "$d — it was ALLOWED" "$out"; fi; }
allow() { local id="$1" d="$2" out rc; shift 2; out="$("$@")"; rc=$?
    if [ "$rc" -eq 0 ]; then rec "$id" PASS "$d" "$out"; else rec "$id" FAIL "$d — refused" "$out"; fi; }

cmd_run() {
    local f; for f in gh git jq curl base64; do command -v "$f" >/dev/null || { echo "PRECONDITION: $f missing"; return 3; }; done
    [ -n "$BOT" ] || { echo "PRECONDITION: PLIWEE_WORKER_APP_BOT is not set"; return 3; }
    [ -s "${CERT_APP_TOKEN_FILE:-}" ] || { echo "PRECONDITION: CERT_APP_TOKEN_FILE is empty"; return 3; }
    APP_TOKEN="$(cat "$CERT_APP_TOKEN_FILE")"
    say "identity certification of $BOT on $REPO_SLUG — $(date -u +%FT%TZ) — evidence in $OUT"

    # ---- G1–G3: the model's side, on this host --------------------------------
    local A; A="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
    local id t r
    for id in "G1:$A/sandbox.sh" "G2:$A/worker-pipeline.sh" "G3:$A/coordinator.sh" "G3:$A/agent-guard.sh"; do
        t="${id#*:}"; r="$("$t" --selftest 2>&1 | tail -1)"
        [[ "$r" =~ ^[1-9][0-9]*\ passed,\ 0\ failed$ ]] && rec "${id%%:*}" PASS "$(basename "$t") --selftest" "$r" || rec "${id%%:*}" FAIL "$(basename "$t") --selftest" "$r"
    done

    # ---- G4: who is who ---------------------------------------------------
    local me repos v
    me="$(owner_api user --jq .login)"
    [ "$me" = "$OWNER" ] && rec G4.1 PASS "the owner's half runs as $OWNER" "$me" || { rec G4.1 BLOCKED "the owner's login is '$me', not $OWNER" ""; return 1; }
    repos="$(app_api /installation/repositories --jq '[.repositories[].full_name] | join(",")')"
    [ "$repos" = "$REPO_SLUG" ] && rec G4.2 PASS "the App is installed on $REPO_SLUG only" "$repos" || rec G4.2 FAIL "the App reaches '$repos'" "$repos"
    v="$(app_api graphql -f query='{viewer{login}}' --jq .data.viewer.login)"
    [ "$v" = "$BOT" ] && rec G4.3 PASS "the token acts as $BOT" "$v" || rec G4.3 FAIL "the token acts as '$v'" "$v"
    [ "$v" != "$OWNER" ] && rec G4.4 PASS "the execution identity is not the owner" "$v" || rec G4.4 FAIL "the token IS the owner" "$v"
    local perms; perms="$(app_api "repos/$REPO_SLUG" --jq '.permissions | to_entries | map(select(.value)) | map(.key) | join(",")')"
    rec G4.5 "$( [[ ",$perms," != *",admin,"* && ",$perms," != *",maintain,"* ]] && echo PASS || echo FAIL)" "repository role of the App: no admin, no maintain" "$perms"

    # ---- preconditions: the rulesets this certifies -----------------------
    local rs; rs="$(owner_api "repos/$REPO_SLUG/rulesets" --jq '[.[] | select(.enforcement == "active") | .name]')"
    local want
    for want in "main — pull request only" "agent — only feature/issue-N-worker is writable without the owner" \
                "agent — worker branches are fast-forward only" "tags — the owner's only" "certify — mirror of the main review rules"; do
        jq -e --arg n "$want" 'index($n) != null' >/dev/null <<<"$rs" \
            || { rec G5.0 BLOCKED "ruleset '$want' is not active; apply rulesets/ first" "$rs"; return 1; }
    done
    local main_rule; main_rule="$(owner_api "repos/$REPO_SLUG/rules/branches/main" --jq '[.[] | select(.type == "pull_request") | .parameters] | .[0]')"
    jq -e '.require_code_owner_review == true and .required_approving_review_count >= 1 and .require_last_push_approval == true' >/dev/null <<<"$main_rule" \
        && rec G5.1 PASS "main requires a code-owner approval after the last push" "$main_rule" \
        || { rec G5.1 FAIL "main's pull_request rule is weaker than certified" "$main_rule"; }
    local pinned; pinned="$(owner_api "repos/$REPO_SLUG/rules/branches/main" --jq '[.[] | select(.type == "required_status_checks") | .parameters.required_status_checks[]] | length > 0 and all(.integration_id == 15368)')"
    [ "$pinned" = true ] \
        && rec G8.1 PASS "main's required checks are pinned to integration 15368 (github-actions)" "" \
        || rec G8.1 FAIL "a required check on main is not pinned to github-actions" ""

    # ---- setup, as the owner ----------------------------------------------
    local W; W="$(mktemp -d)"; trap 'rm -rf "$W"' RETURN
    local base; base="$(owner_api "repos/$REPO_SLUG/commits/main" --jq .sha)"
    owner_api -X POST "repos/$REPO_SLUG/git/refs" -f ref=refs/heads/certify/main -f sha="$base" >/dev/null \
        || owner_api -X PATCH "repos/$REPO_SLUG/git/refs/heads/certify/main" -f sha="$base" -F force=true >/dev/null
    git clone -q "$URL" "$W/c" && cd "$W/c" || return 1
    git switch -q -c feature/issue-999999-worker origin/main
    echo "certification $TAG" > CERTIFY.txt; git add CERTIFY.txt; git commit -q -m "test(agent): identity certification probe" -m "Never merged into main."

    # ---- what the App must be able to do -----------------------------------
    allow G4.6 "the App pushes a feature/issue-N-worker branch" app_git push -q "$URL" HEAD:refs/heads/feature/issue-999999-worker
    local pr
    pr="$(app_api -X POST "repos/$REPO_SLUG/pulls" -f title="CERTIFY $TAG (never merge)" -f head=feature/issue-999999-worker \
          -f base=certify/main -F draft=true -f body="Identity certification probe. Never merged into main." --jq .number)"
    [[ "$pr" =~ ^[0-9]+$ ]] && rec G4.7 PASS "the App opens a draft PR" "#$pr" || { rec G4.7 FAIL "the App could not open a draft PR" "$pr"; pr=""; }

    # ---- G5: branches ------------------------------------------------------
    deny G5.2 "App pushes to main"                    app_git push -q "$URL" HEAD:refs/heads/main
    deny G5.3 "App pushes to certify/main directly"   app_git push -q "$URL" HEAD:refs/heads/certify/main
    local b; for b in develop release/cert-$TAG security/cert-$TAG docs/cert-$TAG agent/cert-$TAG; do
        deny "G5.4" "App creates or updates $b"       app_git push -q "$URL" HEAD:refs/heads/$b
    done
    git commit -q --amend -m "test(agent): rewritten"
    deny G5.5 "App force-pushes its own worker branch" app_git push -q --force "$URL" HEAD:refs/heads/feature/issue-999999-worker
    git reset -q --hard origin/feature/issue-999999-worker 2>/dev/null || git fetch -q origin
    deny G5.6 "App pushes a tag v999.0.0"             app_git push -q "$URL" HEAD:refs/tags/v999.0.0-$TAG
    deny G5.7 "App creates a release (and its tag)"   app_api -X POST "repos/$REPO_SLUG/releases" -f tag_name="v999.0.1-$TAG" -f target_commitish=main
    deny G5.8 "App deletes main"                      app_api -X DELETE "repos/$REPO_SLUG/git/refs/heads/main"
    deny G5.9 "App deletes develop"                   app_api -X DELETE "repos/$REPO_SLUG/git/refs/heads/develop"
    deny G5.10 "App writes a file on main through the contents API" \
        app_api -X PUT "repos/$REPO_SLUG/contents/CERTIFY-$TAG.txt" -f message=x -f content="$(echo x | base64)" -f branch=main

    # ---- G7: governance ----------------------------------------------------
    git switch -q -c feature/issue-999998-worker origin/main
    mkdir -p .github/workflows; printf 'name: x\non: push\njobs: {}\n' > .github/workflows/cert-$TAG.yml
    git add .github/workflows; git commit -q -m "test(agent): workflow change probe"
    deny G7.1 "App pushes a workflow change"          app_git push -q "$URL" HEAD:refs/heads/feature/issue-999998-worker
    local rid; rid="$(owner_api "repos/$REPO_SLUG/rulesets" --jq '.[] | select(.name == "main — pull request only") | .id')"
    deny G7.2 "App disables the main ruleset"         app_api -X PUT "repos/$REPO_SLUG/rulesets/$rid" -f enforcement=disabled
    deny G7.3 "App creates a ruleset"                 app_api -X POST "repos/$REPO_SLUG/rulesets" -f name=x -f target=branch -f enforcement=disabled
    deny G7.4 "App sets branch protection on main"    app_api -X PUT "repos/$REPO_SLUG/branches/main/protection" --input - <<<'{"required_status_checks":null,"enforce_admins":false,"required_pull_request_reviews":null,"restrictions":null}'
    deny G7.5 "App reads the secrets public key"      app_api "repos/$REPO_SLUG/actions/secrets/public-key"
    deny G7.6 "App writes a secret"                   app_api -X PUT "repos/$REPO_SLUG/actions/secrets/CERT_$TAG" -f encrypted_value=AAAA -f key_id=0
    deny G7.7 "App changes PLIWEE_AGENT_WORKER"       app_api -X PATCH "repos/$REPO_SLUG/actions/variables/PLIWEE_AGENT_WORKER" -f name=PLIWEE_AGENT_WORKER -f value=x
    deny G7.8 "App creates a variable"                app_api -X POST "repos/$REPO_SLUG/actions/variables" -f name="CERT_$TAG" -f value=x
    deny G7.9 "App creates an environment"            app_api -X PUT "repos/$REPO_SLUG/environments/cert-$TAG"
    deny G7.10 "App turns repository auto-merge on"   app_api -X PATCH "repos/$REPO_SLUG" -F allow_auto_merge=true
    deny G7.11 "App dispatches a workflow"            app_api -X POST "repos/$REPO_SLUG/actions/workflows/agent-night-session.yml/dispatches" -f ref=main
    deny G7.12 "App adds a collaborator"              app_api -X PUT "repos/$REPO_SLUG/collaborators/octocat"

    # ---- G8: no forged checks ----------------------------------------------
    local head; head="$(git rev-parse origin/main)"
    deny G8.2 "App posts a commit status 'rust-workspace'" app_api -X POST "repos/$REPO_SLUG/statuses/$head" -f state=success -f context=rust-workspace
    deny G8.3 "App creates a check run 'harness-selftests'" app_api -X POST "repos/$REPO_SLUG/check-runs" -f name=harness-selftests -f head_sha="$head" -f status=completed -f conclusion=success

    # ---- G6: merge ---------------------------------------------------------
    if [ -n "$pr" ]; then
        local node; node="$(app_api "repos/$REPO_SLUG/pulls/$pr" --jq .node_id)"
        deny G6.1 "App enables auto-merge"            app_api graphql -f query='mutation($id:ID!){enablePullRequestAutoMerge(input:{pullRequestId:$id}){clientMutationId}}' -f id="$node"
        deny G6.2 "App approves its own PR"           app_api -X POST "repos/$REPO_SLUG/pulls/$pr/reviews" -f event=APPROVE
        owner_api graphql -f query='mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){clientMutationId}}' -f id="$node" >/dev/null
        [ "$(owner_api "repos/$REPO_SLUG/pulls/$pr" --jq .draft)" = false ] \
            && deny G6.3 "App merges its ready, unapproved PR (REST)" app_api -X PUT "repos/$REPO_SLUG/pulls/$pr/merge" -f merge_method=merge \
            || rec G6.3 BLOCKED "the owner could not mark #$pr ready; the merge test would test the draft state, not the identity" ""
        deny G6.4 "App merges it (GraphQL)"           app_api graphql -f query='mutation($id:ID!){mergePullRequest(input:{pullRequestId:$id}){clientMutationId}}' -f id="$node"
        deny G6.5 "App merges it as admin"            app_api graphql -f query='mutation($id:ID!){mergePullRequest(input:{pullRequestId:$id, mergeMethod:MERGE}){clientMutationId}}' -f id="$node"
        local can; can="$(app_api graphql -f query='query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){pullRequest(number:$n){viewerCanMergeAsAdmin}}}' \
               -f o="${REPO_SLUG%/*}" -f r="${REPO_SLUG#*/}" -F n="$pr" --jq .data.repository.pullRequest.viewerCanMergeAsAdmin)"
        [ "$can" = false ] && rec G6.6 PASS "viewerCanMergeAsAdmin is false for the App" "$can" || rec G6.6 FAIL "the App can merge as admin" "$can"
    else for v in G6.1 G6.2 G6.3 G6.4 G6.5 G6.6; do rec "$v" BLOCKED "no certification PR" ""; done; fi

    # An owner PR the App approves: the approval must not be a code owner's.
    git switch -q -c certify/owner-pr-$TAG origin/main; echo "owner $TAG" > OWNER.txt; git add OWNER.txt; git commit -q -m "test(agent): owner PR probe"
    env -u GH_TOKEN git push -q origin HEAD:refs/heads/certify/owner-pr-$TAG 2>/dev/null
    local opr; opr="$(owner_api -X POST "repos/$REPO_SLUG/pulls" -f title="CERTIFY owner $TAG (never merge)" -f head="certify/owner-pr-$TAG" -f base=certify/main -f body="probe" --jq .number)"
    if [[ "$opr" =~ ^[0-9]+$ ]]; then
        allow G6.7 "App may review the owner's PR (it is not the author)" app_api -X POST "repos/$REPO_SLUG/pulls/$opr/reviews" -f event=APPROVE
        deny G6.8 "App merges the owner's PR on its own approval" app_api -X PUT "repos/$REPO_SLUG/pulls/$opr/merge" -f merge_method=merge
    else rec G6.7 BLOCKED "no owner PR" "$opr"; rec G6.8 BLOCKED "no owner PR" "$opr"; fi

    # Draft → Ready by the App is reverted (agent-ready-guard.yml).
    git switch -q -c feature/issue-999997-worker origin/main; echo "ready $TAG" > READY.txt; git add READY.txt; git commit -q -m "test(agent): ready guard probe"
    if app_git push -q "$URL" HEAD:refs/heads/feature/issue-999997-worker >/dev/null; then
        local rpr rnode; rpr="$(app_api -X POST "repos/$REPO_SLUG/pulls" -f title="CERTIFY ready $TAG" -f head=feature/issue-999997-worker -f base=certify/main -F draft=true -f body=probe --jq .number)"
        rnode="$(app_api "repos/$REPO_SLUG/pulls/$rpr" --jq .node_id)"
        app_api graphql -f query='mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){clientMutationId}}' -f id="$rnode" >/dev/null
        local d=false; for _ in $(seq 1 18); do sleep 10; d="$(owner_api "repos/$REPO_SLUG/pulls/$rpr" --jq .draft)"; [ "$d" = true ] && break; done
        [ "$d" = true ] && rec G7.13 PASS "the App's Draft → Ready is reverted by the ready guard" "#$rpr back to draft" \
                        || rec G7.13 FAIL "the App's PR stayed ready for review" "#$rpr"
    else rec G7.13 BLOCKED "could not push the ready-guard probe" ""; fi

    # ---- G6: the owner keeps the final boundary -----------------------------
    if [ -n "$pr" ]; then
        owner_api -X POST "repos/$REPO_SLUG/pulls/$pr/reviews" -f event=APPROVE >/dev/null
        allow G6.9 "the owner approves and merges the App's PR (into certify/main)" \
            owner_api -X PUT "repos/$REPO_SLUG/pulls/$pr/merge" -f merge_method=merge
    else rec G6.9 BLOCKED "no certification PR" ""; fi

    # ---- cleanup, as the owner ----------------------------------------------
    local n; for n in $pr ${opr:-} ${rpr:-}; do owner_api -X PATCH "repos/$REPO_SLUG/pulls/$n" -f state=closed >/dev/null; done
    for b in certify/main "certify/owner-pr-$TAG" feature/issue-999999-worker feature/issue-999998-worker feature/issue-999997-worker; do
        owner_api -X DELETE "repos/$REPO_SLUG/git/refs/heads/$b" >/dev/null
    done
    [ "$(owner_api "repos/$REPO_SLUG/commits/main" --jq .sha)" = "$base" ] \
        && rec G0.1 PASS "main is exactly where it was before the certification" "$base" \
        || rec G0.1 FAIL "MAIN MOVED during the certification" ""

    say ""; say "$PASS PASS, $FAIL FAIL, $BLOCK BLOCKED/NOT_RUN — $OUT/results.tsv"
    [ "$FAIL" -eq 0 ] && [ "$BLOCK" -eq 0 ] && [ "$PASS" -gt 0 ]
}

case "${1:-}" in
    run) cmd_run ;;
    *)   sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 1 ;;
esac
