# Agent execution identity split V2

Status: **PENDING CERTIFICATION**

## Measured reason

The initial single-App execution identity held both:

- Contents: write
- Pull requests: write

The identity certification demonstrated that this combination could approve
and merge an owner-authored PR into the disposable `certify/main` branch.

Production `main` did not move.

## V2

### pliwee-worker

- Metadata: read
- Contents: write
- Issues: write
- Checks: read
- Actions: read
- Pull requests: **no access**

### pliwee-pr-broker

- Metadata: read
- Pull requests: write
- Contents: **no access**

### Owner

The repository owner remains the only administrator and the only actor that
may authorize an update of the protected integration ref.

### Model

Claude receives no GitHub credential.

## Ready state

The PR broker necessarily has enough Pull Request permission to change
Draft/Ready state. The trusted ready-guard returns non-owner Ready transitions
to Draft using a short-lived broker token.

This is compensating governance, not the merge boundary.

## Negative gates

DNS, routing, TLS, rate-limit and GitHub service failures are BLOCKED.
They can never satisfy a negative security test.

## Certification refinement — 2026-10-07

The first V2 run produced 53 PASS, 2 FAIL and 1 BLOCKED.

The Draft/Ready failures were caused by test ordering: the owner had already
transitioned the certification PR to Ready for earlier merge probes.
Ready->Ready therefore was not a valid permission test and produced no
broker-originated ready_for_review transition.

The refined certification now:

- resets the probe to a real Draft before either execution identity is tested;
- requires the worker Draft->Ready mutation to be denied;
- proves the broker reaches isDraft=false;
- requires a successful Agent · ready guard workflow run to restore
  isDraft=true;
- treats repository-disabled auto-merge as a policy denial;
- returns the PR to Ready explicitly as the owner before final merge probes;
- applies the owner approval only after that Ready transition;
- requires the PR broker still to be unable to merge;
- requires the worker specifically to be stopped by the owner-only protected
  certify/main update rule even after owner approval;
- allows only the owner to perform the disposable certification merge.

Production main remains outside destructive certification probes.

## Certification refinement — ephemeral protected destination

The persistent `certify/main` design is superseded.

Measurement showed that its protection correctly refused direct
synchronization. Reusing that branch also allowed previous certification
history to contaminate authorization probes with non-fast-forward or
merge-conflict failures.

Every Identity V2 certification run now creates a unique destination:

`certify/run-<tag>`

The owner creates that branch exactly at the current production `main` SHA.

`certify/run-*` is not excluded from the repository's generic owner-only
branch ruleset. Creation, update and deletion of the certification destination
therefore require the owner/admin bypass.

The worker probe is created directly on top of that fresh destination.
Consequently:

- the worker direct-push test is a genuine fast-forward candidate;
- the certification PR is conflict-free by construction;
- merge denials test authorization rather than stale Git ancestry;
- the PR broker must remain unable to merge;
- the worker must remain unable to update the protected destination even after
  owner approval;
- only the owner may perform the disposable certification merge;
- the owner deletes the ephemeral destination afterward;
- production `main` is never rewritten by certification.

Draft/Ready remains advisory UX and is not a merge-authorization boundary.

The legacy `certify/main` ref and its two dedicated rulesets are retained
temporarily only as historical infrastructure. They are not used by the V2
certifier and can be retired after a completely green certification and
canary.
