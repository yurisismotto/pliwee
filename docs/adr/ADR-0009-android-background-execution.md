# ADR-0009 — Android background execution

**Status:** Accepted · 2026-08-29

> **Amending note — 2026-10-04, owner acceptance of [ADR-0022](ADR-0022-any-to-any-topology-and-symmetric-pairing.md) ([#41](https://github.com/yurisismotto/pliwee/issues/41), branch `feature/issue-41-worker`), §D2–§D3.** The same `connectedDevice` foreground service also hosts a TLS listener that accepts trusted peers, and the phone may connect to several trusted peers rather than one computer; backoff applies per peer, with jitter, and the service stops when no trusted peer is left to dial and no pairing window is open. The service type, its qualifying permission, no boot receiver, `START_NOT_STICKY`, no wakelocks and the capped backoff are unchanged. Accepted is not implemented: until ADR-0022 lands, the tree still behaves as described below. The original text below is left as it was decided.

## Context

The phone must maintain a connection to the computer while the user wants one,
under Android's modern background rules: Doze, App Standby Buckets, background
service restrictions, and the foreground-service type requirements introduced
in Android 14.

The brief is explicit: do not use `dataSync` as a permanent daemon, do not
require root, do not use an accessibility service, and do not build hacks
around platform policy.

## Decision

**A `connectedDevice` foreground service, running only while a connection is
wanted and a network exists.**

* Type `connectedDevice`, declared in the manifest and passed to
  `startForeground` on API 34+.
* The type requires a qualifying permission. We hold
  `CHANGE_WIFI_MULTICAST_STATE` because we genuinely take a `MulticastLock`
  for mDNS discovery — the permission is not a formality to pass the check.
* `START_NOT_STICKY`. No boot receiver. If the system stops the service, it
  stays stopped until the user or a network event brings it back.
* Reconnection is driven by `ConnectivityManager.NetworkCallback` plus capped
  exponential backoff (2 s base, 5 min cap). `onAvailable` resets the backoff,
  because a new network is the one moment a retry is likely to succeed.
* No wakelocks, no `AlarmManager`, no spin loops.
* The service stops itself when there is no paired computer, and when the
  computer reports that it no longer trusts this device — retrying cannot fix
  either.

## Alternatives

**`dataSync` foreground service.** Rejected as instructed, and correctly:
Android time-boxes `dataSync` and it is meant for finite transfers. Using it
as a permanent daemon is the pattern the platform has been tightening down on,
and it would be killed.

**`specialUse`.** Technically permitted with a justification string. Rejected:
`connectedDevice` describes what this actually is, and `specialUse` invites
Play Store review friction for no benefit.

**WorkManager / periodic jobs.** Correct for deferrable work. Rejected: a live
connection is not deferrable work, and a 15-minute minimum interval is
unusable for a "connection status" feature.

**Firebase Cloud Messaging to wake the phone.** The conventional answer for
push. **Rejected on principle** — it requires a cloud service and Google Play
Services, both of which this project exists to avoid.

**Accessibility service.** Rejected as instructed, and it would be an abuse of
the API regardless.

**A persistent socket with no foreground service.** Rejected: it would be
killed within minutes of the screen turning off, and working around that is
exactly the kind of hack the brief forbids.

## Consequences

* A persistent notification while connected. This is honest — the app *is*
  holding a connection — and Android requires it.
* The connection does not survive the system stopping the service. Recovery is
  a network event or the user opening the app.
* Battery cost is bounded: one idle TCP connection plus capped backoff.
* The app is well-behaved enough to be distributable on F-Droid and, if
  wanted, the Play Store.

## Security implications

* Positive: no elevated permissions, no root, no ADB, no accessibility
  service, no notification listener. The manifest lists what is deliberately
  absent so an addition is a visible diff.
* Positive: the persistent notification means the user can always tell the
  connection is live. A background connection with no indicator would be
  worse for the user even though it would be more convenient.
* Positive: no cloud dependency means no third party learns when the user's
  devices are near each other.
* Caveat: the foreground service keeps the process alive, which keeps the
  Keystore handles and the trust store in memory longer. The private key
  itself never enters process memory, so this is a small exposure.
