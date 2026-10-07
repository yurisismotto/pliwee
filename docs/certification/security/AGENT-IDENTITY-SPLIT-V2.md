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
