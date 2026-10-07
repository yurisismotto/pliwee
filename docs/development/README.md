# Development process

How work on Pliwee is done — as opposed to what Pliwee is
([`architecture/`](../architecture/)) or where it is heading (the product
roadmap). Kept current, not historical: when the process
changes, these documents change with it.

| Document | What it covers |
| --- | --- |
| [AGENT-WORKFLOW.md](AGENT-WORKFLOW.md) | issue-driven autonomous development: labels, the dependency gate, the worker, Git authorship, PR policy, the security model, the self-hosted runner, phases, canary, the night autopilot, activation and rollback |
| [AGENT-EXECUTION-SPEC.md](AGENT-EXECUTION-SPEC.md) | v1 of what an autonomous worker is given and hands back: precedence, the brief, the outbox, gates G0–G8, derived issues, result classes, night-session limits |
| [TEST-TIERS.md](TEST-TIERS.md) | every test and gate, by what it needs to mean anything, where it can run, and the CI gaps found while mapping them |

The rules any contributor or agent must follow are in
[`AGENTS.md`](../../AGENTS.md), which these documents do not override.
