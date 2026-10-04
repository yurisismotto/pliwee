# Issue-driven autonomous development

How an AI coding agent may implement a Pliwee issue on its own, and every
limit around it. Written 2026-10-04.

**Status: Phase A — prepared, nothing enabled.** The labels exist; the guard
rails, the skill and the workflow are in the repository; no runner is
registered and the workflow's switch is off. Nothing here has implemented an
issue yet.

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
identity check ── not the owner's ──▶ STOP, agent:failed
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
    TIER 1 GATES → COMMIT → PUSH → DRAFT PR "Implements #N"
    ↓
workflow verifies what reached GitHub: one branch, owner's commits,
no attribution, one open draft PR
    ↓
agent:review                     CI runs independently on the PR
    ↓
owner reviews, runs Tier 2/3 if needed, merges — or does not
```

The agent never merges, never marks a PR ready, never enables auto-merge,
never pushes to `main`, never closes an issue.

## Git authorship — the deciding constraint

[AGENTS.md § Git Authorship Policy](../../AGENTS.md#git-authorship-policy):
commits use the repository owner's configured identity, the agent never
modifies it, and no AI attribution trailer is ever added.

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
that in testing. Which spelling is canonical is an owner decision (below).

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
| The agent merges, pushes to `main` or rewrites history | permission deny rules (`gh pr merge`, `--force`, `--no-verify`, `git config`, `git rebase` …); the `pre-push` hook refuses any ref but `feature/issue-N-<slug>`, deletions and non-fast-forwards; the token has no admin; a ruleset on `main` (owner decision) makes it server-side |
| Wrong author or an AI trailer | `commit-msg` and `pre-push` hooks; `agent-guard.sh commits`; the workflow re-checks the pushed branch and fails into `agent:failed` |
| The agent weakens a gate or edits CI | deny rules on `.github/workflows/**`, `.github/agent/**`, `AGENTS.md`; the token has no `workflows` permission, so GitHub refuses a push that changes a workflow; CI runs again on the PR |
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
   `git config --global user.name "<canonical name>"` and `user.email`. This is
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
   PLIWEE_OWNER_NAME=<canonical name>
   PLIWEE_OWNER_EMAIL=<canonical email>
   PLIWEE_OWNER_LOGIN=yurisismotto
   ```
6. In *Settings → Actions → General*: require approval for workflows from
   **all outside collaborators**, so a fork PR never runs without the owner.

Phase E later adds separate, labelled runners for Tier 2 (libvirt guests) and
Tier 3 (an ADB-connected phone, a Mac) — never the same account as the worker.

## CI stays independent

The worker running tests does not replace CI. Every PR the worker opens targets
`main`, so every existing workflow runs on it on GitHub-hosted runners, plus
`agent-guard.yml`. No workflow is weakened or removed for the worker's sake.
The gaps found while mapping — no CI job for the whole Rust workspace, three
guest-free self-test suites outside CI, no branch protection — are listed in
[TEST-TIERS.md § Gaps](TEST-TIERS.md#gaps-found-while-mapping).

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
proves the pipe end to end. Proposed: **"Add a self-test case to
`evidence-whitespace-check.sh` for a CRLF line ending"** — or any change of the
same size that touches one script, has an obvious test, and cannot affect a
shipped binary. The owner opens it with the contract above.

The canary passes when all of these are observed, not reported:

| Check | How |
| --- | --- |
| git author and committer are the owner's configured identity | `git log --format='%an <%ae> / %cn <%ce>' origin/main..origin/feature/issue-N-*` |
| no attribution trailer | `agent-guard.sh commits origin/main` on the pushed branch |
| branch is `feature/issue-N-<slug>`, from `origin/main` | `git merge-base --is-ancestor origin/main <branch>` |
| CI ran on the PR, on GitHub-hosted runners | `gh pr checks <pr>` lists the workflows |
| the PR is a draft, says "Implements #N", has all sections | `gh pr view <pr> --json isDraft,body` |
| the PR author is the owner's account (the token's user) | `gh pr view <pr> --json author` |
| test evidence in the PR matches a re-run | re-run the listed commands |
| labels moved `agent:ready → agent:working → agent:review` | the issue's timeline |
| nothing merged, auto-merge off | `gh pr view <pr> --json state,autoMergeRequest` |
| the token could not have done more | `gh api user` and the token's settings page: no Workflows/Administration |

## Activation

Phase A (this branch): nothing to activate. Labels already exist.

Phase B, on the owner's host, inside a clean clone:

```bash
git config core.hooksPath .github/agent/hooks
export PLIWEE_OWNER_NAME="<canonical name>" PLIWEE_OWNER_EMAIL="<canonical email>" \
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

## Rollback

Fastest first; each is enough on its own to stop new work.

```bash
gh variable delete PLIWEE_AGENT_WORKER -R yurisismotto/pliwee   # the job skips
gh workflow disable "Agent · issue worker" -R yurisismotto/pliwee
sudo systemctl stop 'actions.runner.yurisismotto-pliwee.*'     # no runner, nothing runs
```

To stop a run in progress: `gh run cancel <run-id>`. To revoke everything:
delete the runner in *Settings → Actions → Runners* and revoke the
fine-grained token. A pushed branch or draft PR is inert until the owner acts
on it; close the PR and delete the branch if unwanted. Removing the whole
mechanism is reverting the commit that added it; the labels can stay or be
deleted with `gh label delete`.

## Owner decisions

These are open, and Phase B or C waits on them:

1. **The canonical git identity** — `Yuri C. Sismotto` (repository config, recent
   history) or `Yuri Converso Sismotto` (global config), and the email. The
   runner account is configured with it, and `PLIWEE_OWNER_*` repeats it.
2. **A ruleset on `main`** — require a pull request and these status checks,
   block force-push and deletion, no bypass for the worker's token. Without it
   the hooks and token scopes are the only stop between the agent and `main`.
   It also binds the owner, so it is the owner's call.
3. **Fork-PR approval** set to "all outside collaborators".
4. **CI additions** from [TEST-TIERS.md § Gaps](TEST-TIERS.md#gaps-found-while-mapping):
   a whole-workspace Rust test job and the guest-free self-test suites, at the
   CI time they cost.
5. **Whether `agent:review` PRs ever leave draft** automatically. Today: never;
   the owner marks them ready.
6. **The canary issue** — the one proposed above, or another.
