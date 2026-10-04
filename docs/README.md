# Pliwee documentation

Everything that is not a project entry point or a governance file lives here.
The repository root carries [`README.md`](../README.md) and
[`AGENTS.md`](../AGENTS.md) and no other Markdown — see
[AGENTS.md § Documentation placement](../AGENTS.md#documentation-placement) for
the rule and why it exists.

## Where does my document go?

Classify by **what the document is**, not by which sprint produced it or what
its filename looks like.

| If the document is… | it goes in | example |
| --- | --- | --- |
| a technical decision meant to stay authoritative | [`adr/`](adr/) | `ADR-0017-capability-roles.md` |
| a description of how the system works *now* | [`architecture/`](architecture/) | `architecture/PROTOCOL.md` |
| a design, brand or UI standard | [`design/`](design/) | `design/UI-GUIDELINES.md` |
| the threat model or a security standard | [`security/`](security/) | `security/THREAT_MODEL.md` |
| feasibility work, investigation, a spec under study | [`research/<topic>/`](research/) | `research/platform-expansion/` |
| a gap analysis, readiness analysis or occurrence audit | [`audits/<area>/`](audits/) | `audits/packaging/` |
| a final PASS/FAIL gate with its evidence | [`certification/<area>/`](certification/) | `certification/clipboard/` |
| a sprint result, implementation report or hardening report | [`reports/<area>/`](reports/) | `reports/notifications/` |
| a record of a rename, port or data migration | [`migrations/`](migrations/) | `migrations/MIGRATION-ANYFLOW-TO-OMNIBRIDGE.md` |
| a public-facing policy the app or a store links to | [`policy/`](policy/) | `policy/PRIVACY-POLICY.md` |
| how the project is developed: process, CI map, agent workflow, kept current | [`development/`](development/) | `development/TEST-TIERS.md` |
| the product roadmap: where the project is heading, kept current | [`roadmap/`](roadmap/) | `roadmap/ROADMAP.md` |

The three that are easiest to confuse:

* **audit** asks *is this ready, and what is missing?* It is written before the
  work and may conclude that the work cannot start.
* **certification** asks *did it pass?* It is the gate, it carries the
  evidence, and its verdict is the thing later documents cite.
* **report** answers *what did this sprint do?* One per sprint or wave, whether
  or not a gate was involved.

`<area>` is an existing directory wherever one fits. Add a new one only when a
document genuinely has no home; do not create a directory for a document that
belongs beside its siblings.

## What is here

```
docs/
├── adr/                        21 decisions on main; ADR-0021 reserved for macOS preview; latest ADR-0022 — see adr/README.md
├── architecture/               how the system works now: OVERVIEW, PROTOCOL, CLIPBOARD, FILES, NOTIFICATIONS
├── design/                     BRAND, UI-GUIDELINES, PLAY-STORE-LISTING, tokens.json, assets/ (the build reads the app icon here),
│                               references/ (owner-supplied boards; never read by a build)
├── security/                   THREAT_MODEL
├── research/
│   ├── platform-expansion/     cross-platform feasibility and the Wave 0 spec — 29 docs + README
│   ├── notifications-v1/       notifications.v1 specification and its PoCs
│   └── pliwee-rebrand/         the wave-by-wave implementation plan for ADR-0020
├── audits/
│   ├── android/                Android / Google Play v1 readiness (PLAY0 → PLAY4) and Play Console declarations
│   ├── linux-compat/           Ubuntu / Debian compatibility, U0 → U2 — see its README
│   ├── packaging/              Linux packaging readiness, build foundation, systemd unit — see its README
│   ├── release/                Release Readiness v1 — baseline, signing foundation, harness hardening
│   └── rebrand/                remaining AnyFlow occurrences after the rename
├── certification/
│   ├── foundation/             Wave 0 / POC-CORE-04 gates — see its README
│   ├── clipboard/              clipboard.v1 hardware certification
│   ├── notifications/          notifications.v1 N6 final certification
│   ├── security/               Security Certification v1 — the SEC gates
│   ├── linux/                  real-host desktop certification, gnome/ and kde/ — see its README
│   └── release/                Release Readiness v1 final + signing closure — the RC verdict
├── reports/
│   ├── android/                Android sprint reports
│   ├── branding/               visual identity, Quick Panel, rebrand closure
│   ├── files/                  files.v1 capability sprint report
│   ├── foundation/             Wave 0 platform abstraction and its follow-up debts
│   ├── linux/                  KDE StatusNotifier, U2 post-certification hardening — see its README
│   ├── notifications/          notifications.v1 N0 → N5 waves — see its README
│   ├── security/               trust-store and device-revocation work
│   └── ux/                     UX hardening and debt cleanup
├── development/                the development process, kept current: AGENT-WORKFLOW, TEST-TIERS
├── migrations/                 AnyFlow → OmniBridge; the OmniBridge 1.0.0 Fedora firewall erratum
├── policy/                     public policies the app links to: PRIVACY-POLICY (its URL is compiled into the app)
└── roadmap/                    ROADMAP — the directional product roadmap; GitHub milestones and issues carry the detail
```

A directory holding several related historical documents carries its own
`README.md` index. The rest are listed by the table above; there is no
hand-maintained catalogue of every file.

## Historical documents are not rewritten

Most of `audits/`, `certification/` and `reports/` is evidence: it records what
was measured, on which commit, on which hardware, on a particular day. Those
documents keep their verdicts, dates, commands, transcripts, test counts,
AnyFlow-era naming, old GitHub URLs and old paths, including paths that no
longer exist. Moving a file is not rewriting it.

When a later sprint invalidates something a historical document asserts, the
document gets a dated, clearly marked superseding note and the original claim is
left standing beneath it. It does not get edited into agreement with the
present.

The one exception is a Markdown link whose target moved: that is repaired, and
only the link target changes — never the link text, which is part of the record.
Three links still point at AnyFlow-era paths (`packaging/fedora/anyflow.spec`
and two `io.github.yurisismotto.anyflow` Kotlin sources). They are deliberately
left broken, because those paths are the evidence.

**Repository recreation, 2026-09-24.** Everything written before that date was
measured on the repository now named `yurisismotto/omnibridge-history`. This
repository was recreated with a new history starting from a byte-identical
tree, so the commit and tag SHAs, PR numbers, issue numbers and Actions run
links those documents quote resolve **there**, not here — including links that
still say `yurisismotto/omnibridge` or `yurisismotto/anyflow`. They are left
as written.
