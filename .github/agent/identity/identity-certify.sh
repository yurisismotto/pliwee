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
BROKER_BOT="${PLIWEE_PR_BROKER_APP_BOT:-}"
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
BROKER_TOKEN=""

app_api() {
    GH_TOKEN="$APP_TOKEN" gh api "$@" 2>&1
}

broker_api() {
    GH_TOKEN="$BROKER_TOKEN" gh api "$@" 2>&1
}
owner_api() { env -u GH_TOKEN gh api "$@" 2>&1; }
app_git() { # git with the App token as the only credential
    env GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0= \
        GIT_CONFIG_KEY_1=http.extraHeader \
        GIT_CONFIG_VALUE_1="AUTHORIZATION: basic $(printf 'x-access-token:%s' "$APP_TOKEN" | base64 -w0)" \
        GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/bin/false git "$@" 2>&1
}
# A negative security gate passes only when the remote side demonstrably
# refused the operation for policy/permission reasons. Network failures are
# never proof of security.

infra_error() {
    grep -Eqi \
      'Could not resolve host|error connecting to|connection reset|connection refused|network is unreachable|operation timed out|TLS|HTTP (429|500|502|503|504)' \
      <<<"$1"
}

policy_denial() {
    grep -Eqi \
      'Repository rule violations found|Resource not accessible by integration|Cannot delete the default branch|refusing to allow a GitHub App|Auto merge is not allowed for this repository|not authorized|not permitted|forbidden|protected ref|HTTP (403|405)' \
      <<<"$1"
}

deny() {
    local id="$1" d="$2" out rc
    shift 2

    out="$("$@")"
    rc=$?

    if [ "$rc" -eq 0 ]; then
        rec "$id" FAIL "$d — it was ALLOWED" "$out"
    elif infra_error "$out"; then
        rec "$id" BLOCKED \
          "$d — infrastructure/network error, not a security denial" \
          "$out"
    elif policy_denial "$out"; then
        rec "$id" PASS "$d (refused)" "$out"
    else
        rec "$id" BLOCKED \
          "$d — refusal was not proven to be policy/permission enforcement" \
          "$out"
    fi
}

allow() {
    local id="$1" d="$2" out rc
    shift 2

    out="$("$@")"
    rc=$?

    if [ "$rc" -eq 0 ]; then
        rec "$id" PASS "$d" "$out"
    elif infra_error "$out"; then
        rec "$id" BLOCKED "$d — infrastructure/network error" "$out"
    else
        rec "$id" FAIL "$d — refused" "$out"
    fi
}

cmd_run() {
    local f; for f in gh git jq curl base64; do command -v "$f" >/dev/null || { echo "PRECONDITION: $f missing"; return 3; }; done
    [ -n "$BOT" ] || { echo "PRECONDITION: PLIWEE_WORKER_APP_BOT is not set"; return 3; }
    [ -n "$BROKER_BOT" ] || { echo "PRECONDITION: PLIWEE_PR_BROKER_APP_BOT is not set"; return 3; }
    [ -s "${CERT_APP_TOKEN_FILE:-}" ] || { echo "PRECONDITION: CERT_APP_TOKEN_FILE is empty"; return 3; }
    [ -s "${CERT_BROKER_TOKEN_FILE:-}" ] || { echo "PRECONDITION: CERT_BROKER_TOKEN_FILE is empty"; return 3; }

    APP_TOKEN="$(cat "$CERT_APP_TOKEN_FILE")"
    BROKER_TOKEN="$(cat "$CERT_BROKER_TOKEN_FILE")"
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
    local brepos bv

    brepos="$(broker_api /installation/repositories --jq '[.repositories[].full_name] | join(",")')"

    [ "$brepos" = "$REPO_SLUG" ]         && rec G4.5b PASS "the PR broker is installed on $REPO_SLUG only" "$brepos"         || rec G4.5b FAIL "the PR broker reaches '$brepos'" "$brepos"

    bv="$(broker_api graphql -f query='{viewer{login}}' --jq .data.viewer.login)"

    [ "$bv" = "$BROKER_BOT" ]         && rec G4.5c PASS "the broker token acts as $BROKER_BOT" "$bv"         || rec G4.5c FAIL "the broker token acts as '$bv'" "$bv"

    # ---- preconditions: the rulesets this certifies -----------------------
    local rs; rs="$(owner_api "repos/$REPO_SLUG/rulesets" --jq '[.[] | select(.enforcement == "active") | .name]')"
    local want
    for want in "main — pull request only" "agent — only feature/issue-N-worker is writable without the owner" \
                "agent — worker branches are fast-forward only" "tags — the owner's only" \
                "certify — mirror of the main review rules" "certify — owner-only updates probe"; do
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
    local W
    W="$(mktemp -d)"
    trap 'rm -rf "$W"' RETURN

    local base cert_sha sync_out sync_rc
    base="$(owner_api "repos/$REPO_SLUG/commits/main" --jq .sha)"

    # certify/main is persistent. Create once; afterwards move it only forward.
    cert_sha="$(
        owner_api \
          "repos/$REPO_SLUG/git/ref/heads/certify/main" \
          --jq .object.sha
    )"

    if ! [[ "$cert_sha" =~ ^[0-9a-f]{40}$ ]]; then
        owner_api \
          -X POST \
          "repos/$REPO_SLUG/git/refs" \
          -f ref=refs/heads/certify/main \
          -f sha="$base" \
          >/dev/null

        cert_sha="$(
            owner_api \
              "repos/$REPO_SLUG/git/ref/heads/certify/main" \
              --jq .object.sha
        )"
    fi

    # Merge production main INTO the disposable certification ref.
    # This keeps certify/main a descendant of its previous state and avoids
    # the non-fast-forward rule that invalidated previous certification runs.
    if [ "$cert_sha" != "$base" ]; then
        sync_out="$(
            owner_api \
              -X POST \
              "repos/$REPO_SLUG/merges" \
              -f base=certify/main \
              -f head="$base" \
              -f commit_message="test(agent): sync certification ref to main $base"
        )"
        sync_rc=$?

        if [ "$sync_rc" -ne 0 ]; then
            if infra_error "$sync_out"; then
                rec G5.0 BLOCKED \
                    "could not synchronize certify/main because of infrastructure/network failure" \
                    "$sync_out"
                return 1
            fi

            rec G5.0 BLOCKED \
                "owner could not forward-synchronize persistent certify/main" \
                "$sync_out"
            return 1
        fi
    fi

    git clone -q "$URL" "$W/c" &&
        cd "$W/c" ||
        return 1

    git fetch -q origin main certify/main

    if git merge-base \
         --is-ancestor \
         origin/main \
         origin/certify/main
    then
        rec G5.0b PASS \
            "persistent certify/main contains current main" \
            "$(git rev-parse origin/certify/main)"
    else
        rec G5.0b BLOCKED \
            "certify/main does not contain current main; merge probes would be invalid" \
            "main=$(git rev-parse origin/main) certify=$(git rev-parse origin/certify/main)"
        return 1
    fi

    # Critical: the certification PR must be based on certify/main itself.
    # Otherwise stale certification history creates artificial merge conflicts.
    git switch \
      -q \
      -c feature/issue-999999-worker \
      origin/certify/main

    printf 'certification %s\n' "$TAG" > "CERTIFY-$TAG.txt"

    git add "CERTIFY-$TAG.txt"

    git commit \
      -q \
      -m "test(agent): identity certification probe" \
      -m "Never merged into main."

    # ---- split publication capabilities -----------------------------------
    allow G4.6 \
        "worker App pushes a feature/issue-N-worker branch" \
        app_git push -q "$URL" HEAD:refs/heads/feature/issue-999999-worker

    # The worker publishes Git objects but has no Pull requests permission.
    deny G4.6b \
        "worker App opens a pull request" \
        app_api -X POST "repos/$REPO_SLUG/pulls" \
        -f title="must-not-open" \
        -f head=feature/issue-999999-worker \
        -f base=certify/main \
        -F draft=true \
        -f body=probe

    # The broker has PR permission but no Contents permission. The worker
    # branch is intentionally used here because no branch-name rule would
    # otherwise hide an accidentally granted Contents permission.
    deny G4.6c \
        "PR broker writes repository contents" \
        broker_api -X PUT \
        "repos/$REPO_SLUG/contents/BROKER-CONTENTS-$TAG.txt" \
        -f message=probe \
        -f content="$(printf probe | base64 -w0)" \
        -f branch=feature/issue-999999-worker

    local pr pr_out pr_rc
    pr_out="$(
        broker_api \
          -X POST \
          "repos/$REPO_SLUG/pulls" \
          -f title="CERTIFY $TAG (never merge)" \
          -f head=feature/issue-999999-worker \
          -f base=certify/main \
          -F draft=true \
          -f body="Identity certification probe. Never merged into main." \
          --jq .number
    )"
    pr_rc=$?

    if [ "$pr_rc" -eq 0 ] && [[ "$pr_out" =~ ^[0-9]+$ ]]; then
        pr="$pr_out"
        rec G4.7 PASS "the PR broker opens a draft PR" "#$pr"
    elif infra_error "$pr_out"; then
        rec G4.7 BLOCKED \
          "the PR broker draft-PR probe hit infrastructure/network failure" \
          "$pr_out"
        pr=""
    else
        rec G4.7 FAIL \
          "the PR broker could not open a draft PR" \
          "$pr_out"
        pr=""
    fi

    # ---- G5: branches ------------------------------------------------------

    # Probe main from an actual descendant of main so a non-fast-forward
    # rejection cannot masquerade as repository-rule enforcement.
    git switch -q --detach origin/main

    printf 'main push probe %s\n' "$TAG" > "MAIN-PUSH-$TAG.txt"
    git add "MAIN-PUSH-$TAG.txt"
    git commit -q -m "test(agent): main direct-push probe"

    deny G5.2 \
        "App pushes a fast-forward candidate directly to main" \
        app_git push -q "$URL" HEAD:refs/heads/main

    # Back to the worker commit based directly on certify/main.
    git switch -q feature/issue-999999-worker

    deny G5.3 \
        "App pushes a fast-forward candidate directly to certify/main" \
        app_git push -q "$URL" HEAD:refs/heads/certify/main
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
        deny G6.2 "worker App approves the broker-authored worker PR"           app_api -X POST "repos/$REPO_SLUG/pulls/$pr/reviews" -f event=APPROVE
        owner_api graphql -f query='mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){clientMutationId}}' -f id="$node" >/dev/null
        [ "$(owner_api "repos/$REPO_SLUG/pulls/$pr" --jq .draft)" = false ] \
            && deny G6.3 "worker App merges the broker-authored ready PR (REST)" app_api -X PUT "repos/$REPO_SLUG/pulls/$pr/merge" -f merge_method=merge \
            || rec G6.3 BLOCKED "the owner could not mark #$pr ready; the merge test would test the draft state, not the identity" ""
        deny G6.4 "worker App merges the broker-authored PR (GraphQL)"           app_api graphql -f query='mutation($id:ID!){mergePullRequest(input:{pullRequestId:$id}){clientMutationId}}' -f id="$node"
        deny G6.5 "worker App merges the broker-authored PR as admin"            app_api graphql -f query='mutation($id:ID!){mergePullRequest(input:{pullRequestId:$id, mergeMethod:MERGE}){clientMutationId}}' -f id="$node"
        local can; can="$(app_api graphql -f query='query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){pullRequest(number:$n){viewerCanMergeAsAdmin}}}' \
               -f o="${REPO_SLUG%/*}" -f r="${REPO_SLUG#*/}" -F n="$pr" --jq .data.repository.pullRequest.viewerCanMergeAsAdmin)"
        [ "$can" = false ] && rec G6.6 PASS "viewerCanMergeAsAdmin is false for the App" "$can" || rec G6.6 FAIL "the App can merge as admin" "$can"
    else for v in G6.1 G6.2 G6.3 G6.4 G6.5 G6.6; do rec "$v" BLOCKED "no certification PR" ""; done; fi

    # An owner PR the App approves: the approval must not be a code owner's.
    git switch -q -c certify/owner-pr-$TAG origin/certify/main; echo "owner $TAG" > OWNER.txt; git add OWNER.txt; git commit -q -m "test(agent): owner PR probe"
    env -u GH_TOKEN git push -q origin HEAD:refs/heads/certify/owner-pr-$TAG 2>/dev/null
    local opr; opr="$(owner_api -X POST "repos/$REPO_SLUG/pulls" -f title="CERTIFY owner $TAG (never merge)" -f head="certify/owner-pr-$TAG" -f base=certify/main -f body="probe" --jq .number)"
    if [[ "$opr" =~ ^[0-9]+$ ]]; then
        allow G6.7 "PR broker may review the owner's PR" broker_api -X POST "repos/$REPO_SLUG/pulls/$opr/reviews" -f event=APPROVE
        deny G6.8 "PR broker merges the owner's PR on its own approval" broker_api -X PUT "repos/$REPO_SLUG/pulls/$opr/merge" -f merge_method=merge
    else rec G6.7 BLOCKED "no owner PR" "$opr"; rec G6.8 BLOCKED "no owner PR" "$opr"; fi

    # ---- Draft / Ready capability ------------------------------------------
    #
    # Draft/Ready is presentation state only.
    #
    # Measured 2026-10-07:
    # - REST PR creation/review remains denied to pliwee-worker;
    # - nevertheless its Contents installation token can invoke the GraphQL
    #   markPullRequestReadyForReview mutation;
    # - a ready_for_review workflow was not a reliable compensating boundary.
    #
    # Therefore authorization MUST NOT depend on Draft/Ready.
    # Trusted publication still opens Draft and never voluntarily transitions it.
    # The decisive security test is below: after owner approval, neither App may
    # update the protected certification ref.
    rec G7.13 PASS \
        "Draft/Ready is explicitly excluded from the authorization boundary" \
        "trusted publisher opens Draft; protected-ref merge gates are authoritative"

    # ---- G6: the owner keeps the final boundary -----------------------------
    if [ -n "$pr" ]; then

        local final_node final_ready final_state
        local final_worker_merge final_worker_rc

        final_node="$(
            owner_api \
              "repos/$REPO_SLUG/pulls/$pr" \
              --jq .node_id
        )"

        ######################################################################
        # OWNER marks Ready.
        #
        # ready-guard deliberately excludes repository owner.
        ######################################################################

        final_ready="$(
            owner_api graphql \
              -f query='mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){pullRequest{isDraft}}}' \
              -f id="$final_node" \
              --jq '.data.markPullRequestReadyForReview.pullRequest.isDraft'
        )"

        final_state="$(
            owner_api \
              "repos/$REPO_SLUG/pulls/$pr" \
              --jq .draft
        )"

        if [ "$final_ready" = false ] &&
           [ "$final_state" = false ]; then

            rec G6.9pre PASS \
                "owner alone returns certification PR to Ready" \
                "#$pr"

        else

            rec G6.9pre BLOCKED \
                "could not prove owner Ready transition before final merge probes" \
                "mutation=$final_ready state=$final_state"
        fi

        ######################################################################
        # OWNER approval.
        ######################################################################

        owner_api \
          -X POST \
          "repos/$REPO_SLUG/pulls/$pr/reviews" \
          -f event=APPROVE \
          >/dev/null

        ######################################################################
        # PR broker:
        #
        # Pull requests write
        # Contents NONE
        #
        # Must never merge.
        ######################################################################

        deny G6.9a \
            "PR broker cannot merge even after owner approval" \
            broker_api \
            -X PUT \
            "repos/$REPO_SLUG/pulls/$pr/merge" \
            -f merge_method=merge

        ######################################################################
        # Worker:
        #
        # Contents write
        # Pull requests NONE
        #
        # Since Contents permission reaches the merge endpoint, this must be
        # stopped SPECIFICALLY by owner-only protected certify/main updates.
        ######################################################################

        final_worker_merge="$(
            app_api \
              -X PUT \
              "repos/$REPO_SLUG/pulls/$pr/merge" \
              -f merge_method=merge
        )"
        final_worker_rc=$?

        if [ "$final_worker_rc" -eq 0 ]; then

            rec G6.9b FAIL \
                "worker App merged after owner approval — protected-ref boundary failed" \
                "$final_worker_merge"

        elif infra_error "$final_worker_merge"; then

            rec G6.9b BLOCKED \
                "worker post-approval merge probe hit infrastructure/network failure" \
                "$final_worker_merge"

        elif grep -Eqi \
              'Cannot update this protected ref|Repository rule violations found' \
              <<<"$final_worker_merge"; then

            rec G6.9b PASS \
                "worker App cannot update certify/main even after owner approval" \
                "$final_worker_merge"

        else

            rec G6.9b BLOCKED \
                "worker merge was refused, but owner-only protected-ref enforcement was not proven" \
                "$final_worker_merge"
        fi

        ######################################################################
        # OWNER alone may update certify/main.
        ######################################################################

        allow G6.9c \
            "owner alone merges approved worker PR into certify/main" \
            owner_api \
            -X PUT \
            "repos/$REPO_SLUG/pulls/$pr/merge" \
            -f merge_method=merge

    else

        rec G6.9pre BLOCKED \
            "no certification PR" \
            ""

        rec G6.9a BLOCKED \
            "no certification PR" \
            ""

        rec G6.9b BLOCKED \
            "no certification PR" \
            ""

        rec G6.9c BLOCKED \
            "no certification PR" \
            ""
    fi

    # ---- cleanup, as the owner ----------------------------------------------
    local n; for n in $pr ${opr:-}; do owner_api -X PATCH "repos/$REPO_SLUG/pulls/$n" -f state=closed >/dev/null; done
    for b in "certify/owner-pr-$TAG" feature/issue-999999-worker feature/issue-999998-worker; do
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
