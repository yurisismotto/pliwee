# ADR-0013 — The `files.v1` authenticated data stream

**Status:** Accepted · 2026-08-30

> **Superseding note — 2026-09-24, Pliwee rebrand Wave 5 (branch `feature/pliwee-rebrand-wave5`), [ADR-0020](ADR-0020-rename-to-pliwee.md) §D4.** The canonical data stream is `pliwee-data/1` with the domain `pliwee/files.v1/data-stream/v1`. `omnibridge-data/1` and `omnibridge/files.v1/data-stream/v1` below are the legacy profile's values, and a data stream must negotiate the same profile as the control session that issued its challenge. The original text below is left as it was decided.

> **Amending note — 2026-10-04, owner acceptance of [ADR-0022](ADR-0022-any-to-any-topology-and-symmetric-pairing.md) ([#41](https://github.com/yurisismotto/pliwee/issues/41), branch `feature/issue-41-worker`), §D3.** [§The phone always dials](#the-phone-always-dials) is amended to *the control session's dialer dials*: the device that dialled the control session dials that session's data streams, and its listener accepts them. Every other rule below is unchanged — the challenge is issued on the control session, the data stream presents the same pinned identity and negotiates the same profile, and the MAC binds acceptor, dialer and transfer. Accepted is not implemented: until ADR-0022 lands, the tree still behaves as described below. The original text below is left as it was decided.

## Context

[ADR-0012](ADR-0012-bulk-transfer-and-frame-limit.md) decided that bulk
payloads never travel inside an `Envelope`, and that `MAX_FRAME_LEN` stays at
64 KiB. It described the shape a transfer would take — negotiate over the
control stream, move bytes over a separate stream — and deliberately left one
question open:

> Whether the data stream is a second TLS connection or a multiplexed stream
> over the existing one is left open […] That decision belongs to the
> file-sharing ADR.

This is that ADR. It also has to answer a question ADR-0012 did not raise:
TLS tells the receiver *which device* is on a socket, but says nothing about
*which transfer* the socket is for. A device may legitimately have several in
flight.

## Decision

**A data stream is a second TLS 1.3 connection to the same port, selected by
ALPN, and authenticated by a MAC over a single-use challenge.**

### A second connection, not a multiplexer

`omnibridge/1` is the control session. `omnibridge-data/1` is a data stream. Both
arrive at the same listener, on the same port, with the same mutual
authentication and the same pinned identities; the listener reads
`alpn_protocol()` after the handshake and routes accordingly.

Sharing the port matters more than it might appear. File transfer inherits the
listener, the mDNS record, the dual-stack binding and the connection limit
that are already certified. There is no second port to open, nothing new to
discover, and no new firewall rule — and the sprint's "do not create a second
discovery mechanism" falls out for free rather than needing discipline.

An ALPN mismatch fails during the handshake, before any application byte, so a
client that reaches the wrong thing gets a clean error rather than a confusing
parse failure. A connection that negotiates *no* known ALPN is dropped:
treating an absent ALPN as "probably a control session" would hand the
handshake path to any client that omitted it.

### The phone always dials

The direction of the *bytes* is independent of who opens the *socket*. The
desktop always accepts data streams; the phone always dials one, in both
directions of transfer.

This is not a preference, it is forced: an Android app is not a stable
listener, and making it one would mean a second foreground service, a second
port, and an inbound attack surface on the device that holds the user's
photos. The existing `PinnedTrustManager.checkClientTrusted` throws on
purpose, and this decision keeps it that way.

### The stream is authenticated explicitly

The receiver's acceptance carries a fresh, single-use, 32-byte
`stream_challenge`. The dialer proves it:

```text
mac = HMAC-SHA256(
    key = stream_challenge,
    msg = "omnibridge/files.v1/data-stream/v1"
          || len_prefixed(acceptor_identity_fingerprint)
          || len_prefixed(dialer_identity_fingerprint)
          || len_prefixed(transfer_id))
```

Deliberately the same construction as the pairing proof in
`omnibridge_core::pairing`: a standard MAC, a versioned domain separator, and
every field length-prefixed so two different field splits cannot produce the
same message. No new cryptography was invented; a reader who has understood
the pairing proof has already understood this.

The acceptor *also* requires that the TLS peer certificate on the data
connection is the same pinned identity that negotiated the transfer. The MAC
is defence in depth on top of that check, never a replacement for it.

### The challenge never precedes agreement

`FileOffer` has no `stream_challenge` field, and that absence is load-bearing.
An offer exists before anyone has agreed to anything; a challenge present at
that point would let a peer open the data stream and start moving bytes before
the receiving human had accepted.

So the challenge is issued by the peer that accepts data streams, in whichever
control message it sends *at the moment the transfer becomes active*:

* `FILE_ACCEPT` when the acceptor is the receiver (Android → Fedora);
* `FILE_READY` when the acceptor is the sender (Fedora → Android).

`FILE_READY` exists only for this. The alternative — putting the challenge in
the offer — was rejected for the reason above, and a variant that always sent
a separate `FILE_READY` was rejected as a wasted round trip in the common
case.

A second consequence is worth stating: by the time a dialer holds a challenge,
the acceptor has already moved the transfer into its transferring state. There
is no race in which a legitimate stream arrives too early and is refused.

## Alternatives

**Multiplex over the existing TLS connection.** Genuinely attractive: one
socket, no second handshake, and the transfer is bound to the session by
construction rather than by a MAC. Rejected because it means writing a
multiplexer — framing, per-stream flow control, head-of-line avoidance — and
getting flow control wrong reintroduces exactly the blocking that ADR-0012
exists to prevent. A second TCP connection gets the kernel's flow control for
free, and its independence is what keeps a 4 GiB copy from delaying a `PING`
or an unpair.

The cost is real and accepted: one extra TLS handshake per transfer (single
digit milliseconds on a LAN), and the control session's death does not
automatically break the data stream — so it has to be noticed and acted on.
See "Consequences".

**Use the transfer id as a bearer token.** Simplest, and what a lot of similar
software does. Rejected outright: an id that authorizes by being known must be
kept secret everywhere it appears, and ids appear in logs, in progress UI, in
crash reports. The MAC lets the id be freely loggable — truncated, by
convention — because knowing it grants nothing.

**Derive the stream key from a TLS exporter (RFC 5705).** The elegant answer,
and the first one attempted: both ends of the control session can derive a
shared secret nobody else has, with no new message. Rejected because it is not
implementable on Android — the JDK exposes no keying-material exporter, and
Conscrypt's is not public API. A design the phone cannot implement is not a
design. Sending a random challenge inside the control session's TLS is exactly
as strong for this purpose.

**A pre-shared per-peer stream key, derived once at pairing.** Fewer moving
parts. Rejected: a long-lived key would have to be stored, rotated and
revoked, and a single-use challenge that lives only in memory for the duration
of one transfer has none of those problems.

**Raise `MAX_FRAME_LEN` and chunk into capability messages.** Answered in
full by ADR-0012.

## Consequences

* File transfer costs one extra TLS handshake per transfer. On a LAN this is
  noise next to the transfer itself.
* The control session stays responsive throughout a copy, which is what makes
  cancellation and revocation work *during* a transfer rather than after it.
* **A data stream does not die when its control session does.** It is a
  separate TCP connection, so the daemon reaps a transfer whose session
  channel has closed rather than waiting for a timeout that would never come.
  This is the single most important consequence of choosing two connections,
  and it is why `TransferManager` has a reaper at all.
* The listener now has two kinds of connection to route between. The routing
  is one `match` on the negotiated ALPN, immediately after the handshake, and
  it fails closed.
* A future capability that needs bulk transfer reuses `omnibridge-data/1` and the
  same challenge construction rather than inventing a third path.
* The phone can never receive a transfer while it has no control session,
  because it would have nowhere to learn a challenge from. That is correct and
  intended: a device that is not connected is not transferring.

## Security implications

* Positive: the data stream inherits TLS 1.3, mutual authentication and SPKI
  pinning unchanged. It is not a new trust decision, and `tls.rs` gained no
  new verifier — only a second ALPN string and a config that selects it.
* Positive: the challenge is single-use and consumed on success, so a
  replayed authentication frame finds nothing to check against.
* Positive: binding the MAC to both fingerprints means a captured proof is
  worthless against a different machine and cannot be replayed by a different
  device — including a *second paired device*, which TLS identity alone
  would already stop, but which is worth being belt-and-braces about.
* Positive: the grant is re-checked when the stream authenticates, not only
  when the offer arrived, so a revocation lands mid-transfer.
* Caveat: an attacker who can read the control session's plaintext — i.e. has
  already compromised one of the two devices — can read a challenge and open a
  data stream. This is not a weakening: on a compromised device they could
  simply use the legitimate code path. The threat model has never claimed to
  defend a device against itself.
* Caveat: the data-stream authentication frame is the least-trusted input in
  the capability, arriving from a peer that has completed TLS but proved
  nothing else. It is length-capped at 4 KiB and checked before allocation,
  and it is refused with a generic reason so a dialer learns nothing about
  which check failed.
