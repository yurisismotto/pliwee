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
