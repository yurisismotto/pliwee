# Working agreements for automated contributors

This file applies to any automated coding agent working in this repository,
whatever vendor or tool it comes from. It is the place to look before generating
a document, and it takes precedence over an agent's own defaults.

Human contributors are welcome to follow it too; nothing here is agent-specific
except the audience.

## Documentation placement

**Agents MUST NOT create sprint reports, audits, certification reports, research
reports, compatibility reports, or implementation closeouts at the repository
root.**

Root Markdown is reserved for project entry points and governance. Today that
means exactly two files:

| File | Why it is at root |
| --- | --- |
| `README.md` | the project's entry point — what Pliwee is, how to build and run it |
| `AGENTS.md` | this file — the conventions an agent needs before it writes anything |

`CONTRIBUTING.md`, `SECURITY.md` and `CODE_OF_CONDUCT.md` would also belong at
root if the project adopted them. Nothing else does.

**All generated project documentation must be placed under the appropriate
`docs/` category.** [`docs/README.md`](docs/README.md) carries the taxonomy and
a table that maps a document's kind to its directory. Classify by what the
document *is* — audit, certification, report, research, architecture, design,
decision, migration — not by the sprint that produced it and not by its
filename.

If no existing category fits, say so and propose one rather than defaulting to
the root.

## Historical documents are evidence

`docs/audits/`, `docs/certification/` and `docs/reports/` hold records of what
was measured, on which commit, on which hardware, on a given day. Do not edit
them into agreement with the present. Preserve verdicts, dates, commands,
command output, test counts, hardware details, conclusions, AnyFlow-era naming,
old GitHub URLs, and old paths quoted inside transcripts — including paths that
no longer resolve.

When a later change invalidates a claim in one of these documents, add a dated
superseding note that names the branch or date responsible, and leave the
original claim standing beneath it.

Moving a file is not rewriting it. Use `git mv` so the rename is recorded, and
verify the content is byte-identical afterwards. The only content change a move
may carry is the repair of a Markdown link whose target moved — and then only
the link target changes, never the link text.

## A PASS with no observed evidence is invalid

**A test, certification gate or harness must fail loudly when its measurement
precondition is absent.** Reporting success on a measurement that did not
happen is worse than reporting failure: a red gate gets investigated, a green
one closes the question.

The rule is symmetric, and the second half is the one that is easy to miss.
**A FAIL measured against a precondition the harness itself destroyed is
equally invalid.** A harness that force-stops the app it is about to measure,
or greps a window that closed before the operation started, produces failures
that say nothing about the product — and sends someone hunting a defect that
is in the test.

Twenty-nine of these are on record: seven in Packaging v1, nine in
[lifecycle closure](docs/certification/linux/RELEASE-LIFECYCLE-CLOSURE-V1.md) §5,
ten in [peer-gate closure](docs/certification/linux/RELEASE-PEER-GATES-CLOSURE-V1.md) §5,
and three found while hardening the harnesses themselves. Each is encoded as a
refusal in [`packaging/tests/lib/assert.sh`](packaging/tests/lib/assert.sh),
named against the wave that suffered it, and each is proved to reject its own
failure mode by
[`packaging/tests/harness-selftests.sh`](packaging/tests/harness-selftests.sh).

When you write a gate, the checks that matter are:

* **the tool exists** — an absent binary makes a gate pass having run nothing;
* **the capture is non-empty** — an empty log answers every question "absent",
  including the ones whose true answer is "present";
* **the window covers the operation** — anchor it to something the *product*
  wrote about *this* run: a transfer id, an invocation id, a filename it
  logged. The anchor must be found before any claim that something else is
  missing can mean anything;
* **the count is exact** — "more than zero" is not a count; six packages were
  built and one was shipped;
* **the operation ran** — two observations around it, with the change
  asserted, not one observation afterwards;
* **the search cannot lose a match** — never pipe into `grep -q`. It exits on
  the first match, the producer dies of SIGPIPE, and `pipefail` turns that into
  "no match": measured at **225 misses in 300 runs**. Use `grep -q P <<<"$var"`
  or grep the file. CI fails on the pipe form.

Prefer the primitive to a fresh `if`: it carries the occurrence that justifies
it, and the self-tests keep it honest. If a new false green appears, add it
there with the measurement, rather than fixing only the call site that showed
it.

**Do not weaken a gate to make it pass.** If a gate cannot be measured, record
it as not executed with the reason. `n/a` with an explanation is evidence;
a green tick over nothing is not.

## Autonomous issue work

An agent working unattended on an issue does so only under these rules,
whatever tool runs it. The process and its reasons are in
[`docs/development/AGENT-WORKFLOW.md`](docs/development/AGENT-WORKFLOW.md).

* **Only the owner starts it.** An agent takes an issue only when the
  repository owner has labelled it `agent:ready`. It never picks up an open
  issue on its own initiative, and it works on one issue per branch.
* **The issue is the scope contract.** An ADR that is not Accepted, a SPEC that
  does not exist, an open blocker or an open owner decision means the agent
  stops, labels the issue `agent:blocked` and says exactly what is missing. It
  never decides an architecture question in order to continue, and never
  modifies an Accepted ADR to fit an implementation.
* **It proposes; the owner disposes.** An agent pushes only its own
  `feature/issue-<n>-<slug>` branch and opens a **draft** pull request that
  says "Implements #n". It never merges, marks a PR ready, enables auto-merge,
  pushes to `main`, force-pushes, or closes the issue.
* **Its tests do not replace CI**, and a gate is never weakened, skipped or
  removed to let an agent pass. What it could not run is reported as
  `NOT EXECUTED — <reason>`.

## Git Authorship Policy

AI agents are development tools, not repository contributors.

Agents MUST NOT modify Git author or committer identity.

All commits must use the repository owner's configured Git identity.

Never add AI attribution trailers, including:

- Co-Authored-By: Claude
- Co-Authored-By: Anthropic
- Generated-By
- AI-generated attribution
- agent authorship or co-authorship

Before every commit, verify:

    git config user.name
    git config user.email

If the configured identity is not the repository owner's configured identity,
STOP. The agent must not repair, replace, or override it automatically.
