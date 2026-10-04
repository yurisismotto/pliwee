# Issue-driven autonomous development

How an AI coding agent may implement a Pliwee issue on its own, and every
limit around it. Written 2026-10-04.

**Status: Phase A — prepared, nothing enabled.** The labels exist; the guard
rails, the skill and the workflow are in the repository; no runner is
registered and the workflow's switch is off. Nothing here has implemented an
issue yet.

**2026-10-04, owner decisions applied** ([§ Owner decisions](#owner-decisions)):
the `main` ruleset and fork-PR approval are live on the server; the two new CI
jobs are written and await their first observed run; the worker itself is still
off, and the canary could not run through it — see
[§ The canary](#the-canary).

[`AGENTS.md`](../../AGENTS.md) remains the vendor-neutral source of truth and
overrides everything below. This document explains the process and its
reasons; the procedure an agent follows is
[`.claude/skills/pliwee-issue-worker/SKILL.md`](../../.claude/skills/pliwee-issue-worker/SKILL.md);
the test tiers are in [TEST-TIERS.md](TEST-TIERS.md).

## The flow

```
owner opens an issue (the scope contract)
    ↓
owner applies  agent:ready            ← the only way work starts
    ↓
self-hosted runner, owner's host
    ↓
identity check ── not the owner's ──▶ BLOCKED: agent:blocked, no commit
    ↓
mechanical gate ── blocked ──▶ agent:blocked + comment listing what is missing
    ↓
agent:working
    ↓
Claude Code, headless, pliwee-issue-worker skill
    READ → judge ADRs / SPECs / open decisions ── unresolved ──▶ agent:blocked
    PLAN → branch feature/issue-N-<slug> from origin/main
    IMPLEMENT → TEST → FIX → RETEST (until a real PASS or a real blocker)
    SELF-REVIEW (separate subagent for protocol / security changes)
    TIER 1 GATES → STAGE → COMMIT THROUGH FIXED WRAPPER
    ↓
trusted workflow verifies local result → PUSH → DRAFT PR "Implements #N"
    ↓
workflow verifies what reached GitHub: one branch, owner's commits,
no attribution, one open draft PR
    ↓
agent:review                     CI runs independently on the PR
    ↓
owner reviews, runs Tier 2/3 if needed
    ↓
owner comments exactly  @pliwee merge  on the PR
    ↓
trusted owner-merge workflow locks the approved head SHA, requires green CI,
marks ready, merges, deletes the feature branch and closes the issue
```

The agent never merges, never marks a PR ready, never enables auto-merge,
never pushes to `main`, never closes an issue. Finalization starts only from
the repository owner's exact `@pliwee merge` PR comment. The workflow pins
both the approved PR HEAD and the then-current `main` SHA; if either changes
after authorization, finalization is refused. After GitHub merges, the
workflow verifies that the server-generated integration commit has exactly
those two SHAs as its parents.

## Git authorship — the deciding constraint

[AGENTS.md § Git Authorship Policy](../../AGENTS.md#git-authorship-policy):
every contributor-created commit uses the repository owner's configured
identity, the agent never modifies it, and no AI attribution trailer is ever
added. A server-generated GitHub merge commit is a narrow integration-record
exception: it may carry GitHub's server/automation identity only after explicit
owner approval, and the owner-merge workflow proves that its two parents are
the approved `main` SHA and the approved PR HEAD. It contains no independent
agent-authored change.

**`anthropics/claude-code-action@v1` does not meet this, on any runner.**
Measured in its source at the `v1` tag on 2026-10-04:

* `src/github/operations/git-config.ts` runs
  `git config user.name "<bot_name>"` and
  `git config user.email "<bot_id>+<bot_name>@users.noreply.github.com"`,
  defaulting to `claude[bot]` / `41898282`. It is called from **both** the tag
  mode and the agent mode, so commits are authored by the bot whichever is
  used, and also on a self-hosted runner, where it overwrites the checkout's
  identity.
* `src/create-prompt/index.ts` instructs the model to add a
  `Co-authored-by: <trigger user>` trailer to every commit.
* `bot_name`/`bot_id` can be pointed at the owner's account, but the email
  would then be the owner's `noreply` address, not the configured one — and
  the action would still be the thing setting the identity, which the policy
  forbids the agent from doing.

So the worker does not use that action. It runs the Claude Code CLI directly
(`claude -p`) on a **self-hosted runner on the owner's Fedora host**, in an OS
account whose git identity **the owner** configured. Nothing in the worker sets
an identity; [`agent-guard.sh identity`](../../.github/agent/agent-guard.sh)
verifies it before work starts, the `commit-msg` and `pre-push` hooks verify it
on every commit and push, and the workflow verifies the pushed commits again
from GitHub's side.

**Found while measuring:** on the owner's host the global identity is
`Yuri Converso Sismotto`, while this repository's local config — and recent
history — says `Yuri C. Sismotto`. A fresh clone, which is what a runner
checkout is, would commit under the global name. The guard rejected exactly
that in testing.

### Decided by the owner, 2026-10-04

The expected owner identity is **`Yuri C. Sismotto <yuri.sismotto@hotmail.com>`**.
Automated development preserves it:

* an agent never replaces or overrides `user.name` / `user.email`;
* the identity is verified before every commit (`agent-guard.sh identity`, the
  `commit-msg` hook) and again on every push and on what reached GitHub;
* no AI co-author or attribution trailer, no `Generated-By` / `Co-Authored-By`
  agent metadata;
* **if the execution environment cannot produce commits consistent with this,
  the agent does not commit: it reports `BLOCKED`** — the issue is labelled
  `agent:blocked` with a comment saying so;
* the identity GitHub shows for pull-request and issue activity may be the
  automation or integration actor; contributor-created **commit authorship**
  follows AGENTS.md regardless;
* the merge commit GitHub creates after the owner's explicit approval is an
  integration record, not a contributor-created commit. Its server identity is
  permitted only under AGENTS.md's narrow exception, and the workflow verifies
  its exact two-parent provenance before closing the issue.

## Labels — the work queue

| Label | Set by | Meaning |
| --- | --- | --- |
| `agent:ready` | **owner only** | approved for autonomous implementation |
| `agent:working` | workflow | claimed; a worker is on it |
| `agent:blocked` | workflow or worker | stopped before code: a dependency, ADR, SPEC or decision is missing; the comment says which |
| `agent:review` | workflow | finished; a draft PR and its CI await the owner |
| `agent:failed` | workflow | gave up, or a verification failed; the comment links the evidence |

The worker never picks up an open issue on its own initiative. Re-applying
`agent:ready` after a blocker is resolved restarts it.

## The issue contract

An issue is fit for `agent:ready` only when it is a contract an agent can be
held to:

* **Scope** and **Out of scope** that leave no design decision open;
* **Acceptance** criteria that are observable, ideally as tests;
* every ADR it relies on is **Accepted**, and every SPEC it needs **exists**;
* its "blocked by" issues are **closed**;
* it says which test tiers matter, if not obvious from the area;
* it was opened, and last edited, by the owner.

Most roadmap issues today (#4–#33) are **not** fit: they record intent and open
decisions by design ("this issue records intent, constraints and open decisions,
not a commitment or a specification"). The worker would correctly block on
them. Implementation issues should be cut from them once their ADRs and SPECs
exist. [`.github/ISSUE_TEMPLATE/agent-task.md`](../../.github/ISSUE_TEMPLATE/agent-task.md)
gives the shape.

## The dependency gate

Two halves, both mandatory:

1. **Mechanical** — `agent-guard.sh issue N`: open, an issue not a PR, labelled
   `agent:ready`, not already `agent:working`, opened and last edited by the
   owner, and no open "blocked by" issue. A fetch that fails is an error, never
   "ready".
2. **Judgement** — the skill, §2: reads the issue, its linked issues, ADRs and
   SPECs, and stops on an unaccepted ADR, a missing SPEC, an open owner
   decision or an open P0 architecture or security question. It comments
   exactly what is missing. It never decides an architecture question in order
   to continue.

## Pull-request policy

| The worker may | The worker may not |
| --- | --- |
| create `feature/issue-N-<slug>` from `origin/main` | push to `main`, `develop`, `release/*` or a tag |
| commit, as the owner's configured identity | set or change an identity; add any attribution trailer |
| push its branch, fast-forward only | force-push, amend pushed commits, rebase, merge |
| open a **draft** PR, and update it | mark it ready, approve it, merge it, enable auto-merge |
| comment on and label its issue | close the issue; "Closes #N" — the PR says "Implements #N" |
| write code, tests, and current documentation (`docs/architecture/`, `docs/development/`, READMEs) | write anywhere under `docs/audits/`, `docs/certification/` or `docs/reports/` — the owner writes evidence — or edit an Accepted ADR, `AGENTS.md`, CI workflows or its own guard rails |

Every PR body has the sections in
[`pr-body.md`](../../.claude/skills/pliwee-issue-worker/pr-body.md): Issue,
Summary, Implementation, Tests, Security, Platform validation, Not executed,
Remaining risks. A hardware gate that was not run is listed as not executed.

## Security model

### What is being protected

The `main` branch and its history; the owner's identity and credentials; the
owner's host; the honesty of the gates; the scope of what was approved.

### Threats, and what stops each

| Threat | Controls |
| --- | --- |
| Someone other than the owner starts the worker | trigger is `issues: labeled` only; the job requires the label `agent:ready` **and** sender = repository owner; the guard requires the issue's author and last editor = owner |
| A fork PR runs code on the self-hosted runner (public repository) | no `pull_request`, `pull_request_target`, `issue_comment` or `workflow_run` workflow targets the `pliwee-agent` runner; every other workflow uses GitHub-hosted runners; fork PR workflows require approval (owner setting, below) |
| Prompt injection through issue text or comments | only owner-authored, owner-edited issues pass the gate; the skill treats anyone else's comment as data; the token cannot touch workflows or settings, so an injected instruction cannot widen its own power |
| The agent merges, pushes to `main` or rewrites history | permission deny rules (`gh pr merge`, `gh pr ready`, `--force`, `--no-verify`, `git config`, `git rebase` …); the `pre-push` hook refuses any ref but `feature/issue-N-<slug>`, deletions and non-fast-forwards; the token has no admin; the `main` ruleset (applied 2026-10-04) refuses a direct push, a force-push and a deletion server-side, for everyone, with no bypass. **Not** server-side: a merge of a green PR — see [§ The merge boundary](#the-merge-boundary) |
| Wrong author or an AI trailer | `commit-msg` and `pre-push` hooks; `agent-guard.sh commits`; the workflow re-checks the pushed branch and fails into `agent:failed` |
| The agent weakens a gate or edits CI | deny rules on `.github/workflows/**`, `.github/agent/**`, `AGENTS.md`; the token has no `workflows` permission, so GitHub refuses a push that changes a workflow; `agent-guard.sh paths` refuses a pushed branch that touches CI, the guard rails, the skill, `AGENTS.md` or historical evidence; CI runs again on the PR |
| The agent edits its own guard so the checks pass | the workflow copies `.github/agent/` and `lib/assert.sh` from `main` into `$RUNNER_TEMP` **before** Claude starts; the git hooks and the post-run verification run that copy, never the one in the working tree or on the pushed branch |
| A false green from the agent | CI re-runs everything Tier 1 in a clean environment; the workflow measures the branch and PR itself; "Not executed" is mandatory |
| Runaway cost or a stuck run | `--max-turns` (default 200), `--max-budget-usd` (default 25), `timeout-minutes: 180`, one issue per run, concurrency per issue |
| Compromise of the runner account | a dedicated OS user with no sudo, no access to the owner's home, the Android signing keys or the release signing key; a fine-grained token scoped to this one repository |

The permission rules are prefix matches and could be sidestepped by a
determined model (`bash -c …`); path rules are `Edit(...)`, which Claude Code
applies to every file-editing tool, `Write` included (it warns that
`Write(...)` path rules are not matched). They are one layer, never the boundary; the
boundary is the token's scopes, the hooks, the account's OS permissions and the
owner's review.

### Credentials on the runner

* **GitHub** — a fine-grained personal access token, created by the owner,
  repository `yurisismotto/pliwee` only: Contents read/write, Pull requests
  read/write, Issues read/write, Metadata read. **No** Workflows, Administration,
  Secrets or Actions permission. Stored by `gh auth login` in the runner
  account. Pushes made with it trigger CI; pushes made with `GITHUB_TOKEN` would
  not.
* **Claude** — the runner account's own Claude Code login (or an API key in its
  environment). Not a repository secret.
* **Nothing else.** No signing key, no ADB-authorised phone, no libvirt access
  in Phases B–D.

## Self-hosted runner plan

1. Create an OS account on the Fedora host: `sudo useradd -m pliwee-agent`, no
   sudo group, no membership in `libvirt` or `plugdev`.
2. As that account, configure git **as the owner decides**:
   `git config --global user.name "Yuri C. Sismotto"` and
   `git config --global user.email yuri.sismotto@hotmail.com`. This is
   the owner configuring the owner's identity, not the agent.
3. Install the build toolchains the Tier 1 gates need: Rust stable (rustup),
   JDK 17 and the Android SDK command-line tools, `jq`, `gh`, `shellcheck`, and
   Claude Code. Log in to `gh` with the token above and to Claude Code.
4. Register the runner for this repository only, with the extra label
   `pliwee-agent`:
   `./config.sh --url https://github.com/yurisismotto/pliwee --labels pliwee-agent --name fedora-agent --unattended`
   (token from *Settings → Actions → Runners → New self-hosted runner*). Run it
   as a systemd service under `pliwee-agent`.
5. Write the runner's `.env` (next to `config.sh`):
   ```
   PLIWEE_OWNER_NAME=Yuri C. Sismotto
   PLIWEE_OWNER_EMAIL=yuri.sismotto@hotmail.com
   PLIWEE_OWNER_LOGIN=yurisismotto
   ```
6. In *Settings → Actions → General*: require approval for workflows from
   **all outside collaborators**, so a fork PR never runs without the owner.
   **Applied 2026-10-04** through the API (`approval_policy:
   all_external_contributors`, read back from
   `repos/yurisismotto/pliwee/actions/permissions/fork-pr-contributor-approval`).

Phase E later adds separate, labelled runners for Tier 2 (libvirt guests) and
Tier 3 (an ADB-connected phone, a Mac) — never the same account as the worker.

## CI stays independent

The worker running tests does not replace CI. Every PR the worker opens targets
`main`, so every existing workflow runs on it on GitHub-hosted runners, plus
`agent-guard.yml`. No workflow is weakened or removed for the worker's sake.
The gaps found while mapping are listed in
[TEST-TIERS.md § Gaps](TEST-TIERS.md#gaps-found-while-mapping). Two of them
are closed by workflows added on 2026-10-04, both on GitHub-hosted runners,
both without a path filter so that they can become required checks:

| Workflow | Check name | Runs | Kind |
| --- | --- | --- | --- |
| [`rust-workspace.yml`](../../.github/workflows/rust-workspace.yml) | `rust-workspace` | `cargo test --locked --workspace --all-targets` | product tests |
| [`harness-selftests.yml`](../../.github/workflows/harness-selftests.yml) | `harness-selftests` | every guest-free harness self-test suite | harness self-tests |

They are kept apart on purpose: a red product test and a red harness self-test
are different findings, and the second means a gate can no longer be believed.

## Phase D — CI self-repair (designed, not built)

```
CI fails on a worker PR
    ↓
workflow_run (completed, failure), on the self-hosted runner, only when
  head_repository == this repository  AND  head branch matches feature/issue-N-*
  AND  the PR is a draft labelled agent:review  AND  attempts < MAX_REPAIR_ATTEMPTS
    ↓
the worker reads the failing job's log, and classifies:
    product defect      → fix the code, add a regression test
    test defect         → fix the test only if the evidence shows the test is
                          wrong, and say so in the PR; never loosen an assertion
    harness/environment → do not touch product code; report it
    ↓
Tier 1 locally → push (fast-forward) → CI runs again
    ↓
after MAX_REPAIR_ATTEMPTS = 3 failed rounds:
    agent:failed, and a comment with the failing gate, the evidence, the
    attempts, the suspected root cause and the remaining blocker
```

The attempt count lives on the PR as a hidden marker comment the workflow
writes, not in the agent's memory. A harness failure is never reported as a
product pass, and a gate is never weakened to get green.

## Phases

| Phase | What | Main risk | Gate to advance |
| --- | --- | --- | --- |
| **A** Repository readiness | labels, guard rails and their CI, skill, issue contract, test tiers, this document | none at runtime; documents can drift from practice | owner reviews and merges this branch; the identity decision is taken |
| **B** Worker, controlled | the owner runs `claude -p "/pliwee-issue-worker N" --settings .github/agent/claude-settings.json` by hand, on the host, on the canary | the agent misjudges scope or claims an unmeasured pass | the canary passes every check below, and one real small issue after it |
| **C** `agent:ready` trigger | register the runner; set `PLIWEE_AGENT_WORKER=enabled` | an unattended run on the owner's machine | a ruleset on `main`; fork-PR approval on; two canary runs through the workflow verified |
| **D** CI self-repair | the design above | a loop that "fixes" a gate by weakening it | several Phase C PRs merged without CI repair being needed by hand; attempt counter tested |
| **E** Platform runners | Tier 2 / Tier 3 runners | hardware access from automation | separate accounts, separate labels, per-gate evidence that matches the certification documents |

## The canary

Before any P0/P1 roadmap issue, one small, isolated, non-destructive issue
proves the pipe end to end. Approved by the owner on 2026-10-04 and opened as
**[#34 — CANARY: reject CRLF in evidence-whitespace-check.sh](https://github.com/yurisismotto/pliwee/issues/34)**.

It is a real defect, measured before the issue was opened: the guard exists to
catch what `git diff --check` cannot see, `git diff --check` rejects a CR at the
end of a line, and the guard passes it — including a trailing space hidden
behind the CR. The existing `--selftest` has no CR fixture. It touches one
script, has an obvious red-before-green test, and cannot affect a shipped
binary.

The canary passes when all of these are observed, not reported:

| Check | How |
| --- | --- |
| git author and committer are `Yuri C. Sismotto <yuri.sismotto@hotmail.com>` | `git log --format='%an <%ae> / %cn <%ce>' origin/main..origin/feature/issue-N-*` |
| no attribution trailer | `agent-guard.sh commits origin/main` on the pushed branch |
| branch is `feature/issue-N-<slug>`, from `origin/main` | `git merge-base --is-ancestor origin/main <branch>` |
| CI ran on the PR, on GitHub-hosted runners | `gh pr checks <pr>` lists the workflows |
| the PR is a draft, says "Implements #N", has all sections | `gh pr view <pr> --json isDraft,body` |
| the PR author is the token's user — the owner's account or an integration actor; either is acceptable, commit authorship is what must be the owner's | `gh pr view <pr> --json author` |
| test evidence in the PR matches a re-run | re-run the listed commands |
| labels moved `agent:ready → agent:working → agent:review` | the issue's timeline |
| the worker ran from the workflow, not by hand | a run of *Agent · issue worker* with event `issues` for the label |
| the regression test was red before the fix and green after | the PR's Tests section, re-run |
| `rust-workspace` and `harness-selftests` ran on the PR and passed | `gh pr checks <pr>` |
| the PR is still a draft after CI is green, and nobody but the owner promoted it | `gh pr view <pr> --json isDraft`; the PR timeline |
| nothing merged, auto-merge off | `gh pr view <pr> --json state,autoMergeRequest` |
| nothing reached `main` but through a merged PR | `git log origin/main` unchanged by the run |
| the token could not have done more | `gh api user` and the token's settings page: no Workflows/Administration |

Anything short of every row is **PARTIAL** or **BLOCKED**, never PASS.

### Preconditions the canary cannot run without

Measured on 2026-10-04; each one alone stops the workflow from ever starting:

1. **The worker is not on `main`.** GitHub runs a workflow for an `issues`
   event only from the default branch, and `agent-issue-worker.yml` lives on
   `feature/agent-issue-worker-infra`. Until the owner merges that branch, a
   label applied to #34 starts nothing. The same is true of the guard rails:
   the worker branches from `origin/main`, so before the merge its checkout
   would have no `agent-guard.sh`, no hooks and no skill — and git runs no
   hook from a missing `core.hooksPath`, silently. A hand run of Phase B before
   the merge would therefore not be the worker either.
2. **No runner labelled `pliwee-agent` is registered**
   (`repos/yurisismotto/pliwee/actions/runners`: `total_count: 0`). Creating
   its OS account needs `sudo`; registering it needs a token only the owner
   can mint.
3. **`PLIWEE_AGENT_WORKER` is not set** (`actions/variables`: empty), so the
   job is skipped by design.
4. **The runner account's credentials** — its git identity, its fine-grained
   token and its Claude login — are the owner's to create
   ([§ Self-hosted runner plan](#self-hosted-runner-plan)).

None of these is something an agent may do for itself, and none is worked
around.

### Run 1 — 2026-10-04: BLOCKED

`agent:ready` applied to #34 by the owner's account at 05:11:28Z, with this
branch at `527a5c7` and `main` at `b17e525`. Observed for three minutes: **no
workflow run** (`gh run list --event issues`: 0), and *Agent · issue worker*
absent from `actions/workflows` — precondition 1. The mechanical gate, run by
hand against GitHub, passed (`agent-guard.sh issue 34`: ok). No branch, commit
or PR exists for #34. The issue was moved to `agent:blocked` by hand, with the
evidence and the next steps in
[its comment](https://github.com/yurisismotto/pliwee/issues/34#issuecomment-5976853015).
Verdict: **BLOCKED**, not a failure of the worker — the worker never ran.

## Activation

### Order

Each step is observed before the next one starts.

1. **Server-side protection** — the `main` ruleset and fork-PR approval.
   *Applied 2026-10-04.*
2. **The owner opens a PR from `feature/agent-issue-worker-infra`.** Its CI is
   the first observed run of `rust-workspace` and `harness-selftests`.
3. **Required checks** — once both have reported on that PR, and passed, add
   them to the ruleset ([§ Required checks](#required-checks)).
4. **The owner merges the branch.** The worker, its guard rails and its skill
   are now on `main`.
5. **The runner** — account, identity, token, Claude login, registration,
   `.env` ([§ Self-hosted runner plan](#self-hosted-runner-plan)).
6. **The switch** — `PLIWEE_AGENT_WORKER=enabled` (below).
7. **The canary** — the owner removes and re-applies `agent:ready` on #34
   (the trigger is the `labeled` event, so a label already there starts
   nothing).

Phase A (this branch): nothing to activate. Labels already exist.

Phase B, on the owner's host, inside a clean clone:

```bash
git config core.hooksPath .github/agent/hooks
export PLIWEE_OWNER_NAME="Yuri C. Sismotto" PLIWEE_OWNER_EMAIL="yuri.sismotto@hotmail.com" \
       PLIWEE_OWNER_LOGIN=yurisismotto PLIWEE_REPO=yurisismotto/pliwee
./.github/agent/agent-guard.sh identity
gh issue edit <canary> --add-label agent:ready
./.github/agent/agent-guard.sh issue <canary>
claude -p "/pliwee-issue-worker <canary>" --settings .github/agent/claude-settings.json \
       --max-turns 200 --max-budget-usd 25 --output-format json > canary-result.json
```

Phase C, after the runner is registered and the owner decisions are taken:

```bash
gh variable set PLIWEE_AGENT_WORKER --body enabled -R yurisismotto/pliwee
# optional limits
gh variable set PLIWEE_AGENT_MAX_TURNS --body 200 -R yurisismotto/pliwee
gh variable set PLIWEE_AGENT_MAX_BUDGET_USD --body 25 -R yurisismotto/pliwee
```

## Required checks

A check enters the ruleset only after GitHub has published it under its real
name on a pull request and it passed there. A required check that never
reports blocks every PR, and every existing workflow is path-filtered — so
none of them is required, and the two new ones run on every PR for exactly
this reason.

| Check | Workflow | State |
| --- | --- | --- |
| `rust-workspace` | `rust-workspace.yml` | **PENDING ACTIVATION AFTER FIRST OBSERVED CHECK** |
| `harness-selftests` | `harness-selftests.yml` | **PENDING ACTIVATION AFTER FIRST OBSERVED CHECK** |

Once both have passed on a PR, confirm the names, then add them. The ruleset
id is `24443525`; `15368` is the GitHub Actions app, so a status of the same
name from anywhere else does not satisfy the rule.

```bash
gh pr checks <pr> -R yurisismotto/pliwee        # both names listed, both pass
gh api repos/yurisismotto/pliwee/rulesets/24443525 > ruleset.json
jq '{name, target, enforcement, conditions, bypass_actors,
     rules: (.rules + [{type: "required_status_checks", parameters: {
       strict_required_status_checks_policy: false,
       do_not_enforce_on_create: false,
       required_status_checks: [
         {context: "rust-workspace",    integration_id: 15368},
         {context: "harness-selftests", integration_id: 15368}]}}])}' \
   ruleset.json > ruleset-new.json
gh api -X PUT repos/yurisismotto/pliwee/rulesets/24443525 --input ruleset-new.json
gh api repos/yurisismotto/pliwee/rules/branches/main --jq '.[].type'   # now lists required_status_checks
```

A check is removed from the list only by the owner, with the reason written
here. If a required check is renamed, add the new name, watch it pass, and only
then remove the old one.

## The merge boundary

The ruleset stops a direct push, a force-push and a deletion of `main` for
every actor. It does **not** stop a merge: it requires a pull request with
**zero** approvals, because the owner is the only maintainer and GitHub does
not let an author approve their own PR. The worker's fine-grained token
belongs to the owner's account and carries *Contents: write*, which is enough
to merge a green PR through the API. What stops the worker merging today is
the deny rules, the absence of `gh api`/`gh pr merge` from its allow list, and
the post-run check — not the server.

Making it server-side means a separate actor for the worker: a machine
account or GitHub App with *Write* role, its own token, then one required
approval and a bypass for the repository-admin role in `pull_request` mode only
(so the owner still merges their own PRs, and the worker never can). Commit
authorship is unaffected — it comes from git, not from the token. That is an
owner decision, not taken; until it is, this boundary is advisory.

## Rollback

Fastest first; each is enough on its own to stop new work.

```bash
gh variable delete PLIWEE_AGENT_WORKER -R yurisismotto/pliwee   # the job skips
gh workflow disable "Agent · issue worker" -R yurisismotto/pliwee
sudo systemctl stop 'actions.runner.yurisismotto-pliwee.*'     # no runner, nothing runs
```

Server-side settings, if one of them has to come off:

```bash
gh api -X PUT repos/yurisismotto/pliwee/rulesets/24443525 --input ruleset.json   # the copy saved before adding checks
gh api -X PUT repos/yurisismotto/pliwee/rulesets/24443525 -f enforcement=disabled # ruleset off, kept for re-enabling
gh api -X PUT repos/yurisismotto/pliwee/actions/permissions/fork-pr-contributor-approval \
   -f approval_policy=first_time_contributors                                    # the value before 2026-10-04
```

Disabling the ruleset removes the only server-side stop on a push to `main`;
take the worker's switch off first.

To stop a run in progress: `gh run cancel <run-id>`. To revoke everything:
delete the runner in *Settings → Actions → Runners* and revoke the
fine-grained token. A pushed branch or draft PR is inert until the owner acts
on it; close the PR and delete the branch if unwanted. Removing the whole
mechanism is reverting the commit that added it; the labels can stay or be
deleted with `gh label delete`.

## Owner decisions

### Decided

| # | Decision | Date | Status |
| --- | --- | --- | --- |
| 0 | Git identity: `Yuri C. Sismotto <yuri.sismotto@hotmail.com>` ([§ Git authorship](#decided-by-the-owner-2026-10-04)) | 2026-10-04 | **DECIDED** |
| 1 | Main ruleset | 2026-10-04 | **APPROVED** — applied |
| 2 | Fork workflow approval | 2026-10-04 | **APPROVED** — applied; all outside collaborators; fork code is untrusted |
| 3a | Full Rust workspace CI | 2026-10-04 | **APPROVED** — `rust-workspace.yml` |
| 3b | Non-VM harness self-tests CI | 2026-10-04 | **APPROVED** — `harness-selftests.yml` |
| 4 | Agent Draft → Ready | 2026-10-04 | **DEFERRED** — owner-only |
| 5 | CRLF canary | 2026-10-04 | **APPROVED** — [#34](https://github.com/yurisismotto/pliwee/issues/34) |

**1. Main ruleset — APPROVED.** `main` takes changes through a pull request
only; force-push and deletion are blocked; no actor has a bypass — not the
worker, not Claude, not an integration. Auto-merge is not part of the flow and
the agent never merges: `agent → branch → draft PR → CI → owner → merge`,
never `agent → main`. Applied as ruleset `24443525`, read back from
`repos/yurisismotto/pliwee/rules/branches/main`. Before: no ruleset, no branch
protection. Required status checks follow [§ Required checks](#required-checks);
what the ruleset does not stop is in [§ The merge boundary](#the-merge-boundary).

**2. Fork workflow approval — APPROVED.** Workflows from every outside
collaborator wait for the owner's approval (`all_external_contributors`;
before: `first_time_contributors`). Approval is not trust: a fork PR is
untrusted input whoever approved it to run. No workflow uses
`pull_request_target`; every workflow a PR can start runs on a GitHub-hosted
runner with a read-only token and no secret; the self-hosted runner is
reachable only from `issues: labeled` by the owner. The review behind this is
in [§ Fork pull requests](#fork-pull-requests).

**3. New CI jobs — APPROVED.** `rust-workspace` (product tests) and
`harness-selftests` (harness self-tests), on every PR, kept separate on
purpose ([§ CI stays independent](#ci-stays-independent)).

**4. Agent Draft → Ready — DEFERRED.** Every PR the worker opens stays a
**draft**. The worker does not run `gh pr ready`, does not convert the draft,
does not request a review in place of the owner's decision, does not merge and
does not enable auto-merge — even when CI is green.

```
agent:ready → worker → draft PR → CI → agent:review → OWNER marks ready → OWNER decides the merge
```

The skill says so, the permission file denies `gh pr ready`, `gh pr merge`,
`gh pr review` and `gh pr edit --add-reviewer`, and the workflow fails the run
into `agent:failed` if the PR is not an open draft or has auto-merge on.
Revisiting this is a new owner decision.

**5. CRLF canary — APPROVED.** [#34](https://github.com/yurisismotto/pliwee/issues/34),
[§ The canary](#the-canary).

### Still open

* **A separate actor for the worker**, so that the merge boundary is
  server-side ([§ The merge boundary](#the-merge-boundary)).

## Fork pull requests

Reviewed 2026-10-04 over every workflow on this branch:

| Question | Answer |
| --- | --- |
| Can a fork make a workflow run with a write token? | No. Every `pull_request` workflow declares `contents: read`; GitHub also downgrades a fork PR's token to read. The one job asking for more (`release-artifacts.yml` → `collect`: `id-token`, `attestations`) gets neither on a fork PR, so its attestation step fails closed there. |
| Can a fork obtain a secret? | No. Secrets are not passed to fork PRs, and the only secret referenced anywhere (`RELEASE_SIGNING_KEY`) is read in `release-artifacts.yml`, whose signing step runs only when the key is present. |
| Can a fork reach the self-hosted runner? | No. Only `agent-issue-worker.yml` targets it, on `issues: labeled`, and only when the owner applied `agent:ready`. No `pull_request`, `pull_request_target`, `issue_comment` or `workflow_run` workflow names it. |
| Can a fork change a script run in a privileged context? | No privileged context exists for fork code: fork PRs run their own copy of every script, with nothing to take. The worker runs scripts from `main`, and now runs its guard from a copy taken before Claude starts. |
| Is there a dangerous `pull_request_target`? | There is none. |
| Artifact or cache poisoning? | No artifact crosses workflows (artifacts are uploaded and downloaded within one `release-artifacts.yml` run). The only cache is `setup-java`'s Gradle cache in `android-ci.yml`; GitHub scopes a PR's cache writes to the PR's ref, where `main` never reads them. The new jobs use no cache. |
| Untrusted checkout before privileged code? | Not for forks. For the worker's own output it was the case — the verification step switched to the pushed branch and ran *that branch's* `agent-guard.sh`. Fixed: it runs the copy taken from `main`. |

Residual risk: a fork PR still runs arbitrary code on a GitHub-hosted runner
once approved. That is the runner's sandbox, and it holds nothing of ours.
