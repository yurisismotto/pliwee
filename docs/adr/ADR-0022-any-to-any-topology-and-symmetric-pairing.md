# ADR-0022 — Any-to-any topology and symmetric pairing

**Status:** Accepted · 2026-10-04

Proposed 2026-10-04 ([#39](https://github.com/yurisismotto/pliwee/issues/39)).
Accepted by the project owner, Yuri C. Sismotto, on 2026-10-04
([#41](https://github.com/yurisismotto/pliwee/issues/41)). The owner's answers
to the four open questions are recorded in
[§Owner decisions at acceptance](#owner-decisions-at-acceptance) and applied in
the sections they concern.

Accepted is not implemented. The tree still implements the V1 topology of
[ADR-0005](ADR-0005-lan-discovery-mdns.md),
[ADR-0006](ADR-0006-device-identity-and-pairing.md) and
[ADR-0013](ADR-0013-file-transfer-data-stream.md) until the implementation
lands, and that implementation needs a SPEC that does not exist yet
([§Notes](#notes)).

**Relationship to earlier ADRs.**

* **Supersedes** [ADR-0005](ADR-0005-lan-discovery-mdns.md)'s fixed direction
  — *"The desktop advertises; the phone browses and always initiates the
  connection"* — and the outcome, not the reasoning, of its rejected
  alternative *"Phone advertises, desktop connects"* (§D1, §Compatibility).
* **Supersedes** [ADR-0006](ADR-0006-device-identity-and-pairing.md)'s pairing
  roles — *desktop issues the token and shows the QR, phone scans* — and its
  consequence *"Pairing needs a camera"* (§D5). Every property of the pairing
  construction is kept.
* **Amends** [ADR-0013](ADR-0013-file-transfer-data-stream.md) §"The phone
  always dials" to *"the control session's dialer dials"* (§D3).
* **Amends** [ADR-0009](ADR-0009-android-background-execution.md): the same
  `connectedDevice` foreground service now also hosts a listener (§D2). Its
  rules — no boot receiver, `START_NOT_STICKY`, no wakelocks, capped backoff —
  are unchanged.
* **Keeps** [ADR-0007](ADR-0007-tls-transport-and-pinning.md) whole: TLS 1.3
  only, mandatory mutual authentication, SPKI pinning, no resumption, no SANs.
  It now applies to every device in both connection roles. One pairing mode
  verifies the issuer's SPKI after the handshake instead of before it, and
  never trusts it until then. That mode is called out where it is defined
  (§D5) because it is the one place this ADR touches ADR-0007's reasoning.
* Leaves [ADR-0008](ADR-0008-capability-architecture.md),
  [ADR-0010](ADR-0010-protocol-envelope-and-framing.md),
  [ADR-0017](ADR-0017-capability-roles.md) and
  [ADR-0020](ADR-0020-rename-to-pliwee.md) unchanged. Trust stays pairwise;
  whether it ever becomes transitive is #6's decision, not this one.

The text of every earlier ADR is left as it was decided. Each superseded or
amended ADR carries a superseding or amending note dated 2026-10-04, the same
way ADR-0020 was recorded.

---

## Context

V1 has one shape: one Android device paired with one desktop. Features are
already negotiated capabilities, but the topology around them is fixed in
three places:

| Where | What is fixed | Stated reason |
| --- | --- | --- |
| ADR-0005 | desktop advertises and accepts; phone browses and always dials | a phone's address changes, it sleeps, and one handshake path is easier to reason about |
| ADR-0006 | desktop opens the pairing window and shows the QR; phone scans | the desktop has a screen, the phone has a camera |
| ADR-0013 | desktop accepts `files.v1` data streams; phone always dials them | follows from ADR-0005 |

V2 (#4) wants any trusted device to talk directly to any other trusted device
it can reach: phone ↔ tablet, phone ↔ desktop, tablet ↔ desktop, and later
Android TV, Windows and macOS. No desktop may be a mandatory hub. Without
changing the three rows above, every new pair of platforms is a special case,
and a phone and a tablet can only meet through a computer.

Four facts constrain the answer.

1. **Android can accept connections only while a process is alive.** Under
   ADR-0009 that means while the `connectedDevice` foreground service runs, or
   while an activity is in the foreground. There is no wake-on-network and
   there will be no FCM (ADR-0009, principle 2). A sleeping phone is offline.
2. **Some devices have no camera, and some have no convenient display.** An
   Android TV has a screen but no camera. A desktop usually has no camera. A
   headless desktop has neither a screen nor a camera, only a terminal
   reachable over SSH.
3. **Reachability is not symmetric.** A desktop firewall, a Wi-Fi network with
   partial client isolation, or a phone's network stack can let A reach B
   while B cannot reach A.
4. **The wire already carries nothing that names a device kind.** `DeviceInfo`
   has `device_id`, `device_name`, `platform` (`ANDROID` or `LINUX`) and the
   fingerprint. Phone, tablet and TV are all `ANDROID`. A decision that needs
   the device kind would need new, peer-supplied, unauthenticated metadata.

---

## Decision

### D1 — Every device is a peer; the connection has roles, the device does not

For any one TCP connection, **dialer** and **listener** are the TLS client and
TLS server. They are properties of the connection. They are not properties of
the device kind, and they are not stored in the trust store.

* **Every device kind can dial.** A device whose Pliwee process is running
  dials any trusted peer it has no session with, whenever it has a reason to
  think that peer is reachable (§D3).
* **Every device kind can listen, while its platform keeps it alive.** What
  that means per platform is in §D2.
* **A pair can connect when at least one side is listening and the other can
  reach it.** No device kind is required to listen at all times, and none is
  required to be present for two others to connect.
* **Nothing above the session layer may depend on who dialled.** A capability
  behaves the same whichever side was the TLS client. The one place where V1
  depended on it, the `files.v1` data stream, is generalized in §D3.

### D2 — Who listens and advertises

A device advertises its DNS-SD record **only while its listener is accepting**.
Advertising is the statement "you can dial me now". A record for a device that
does not accept connections would only cost the dialer a failed connection
attempt.

| Platform | Listens | Advertises | Dials |
| --- | --- | --- | --- |
| Linux desktop (daemon) | always while the daemon runs (as today) | while listening, unless `--no-mdns` or the per-network control says no | yes, new |
| macOS / Windows (later) | as the Linux desktop | as the Linux desktop | yes |
| Android phone / tablet | while the `connectedDevice` service runs, or while the pairing screen is in the foreground (§D5) | while listening, under the per-network control | while the service runs (as today) |
| Android TV | as phone / tablet: while its service runs | as phone / tablet | as phone / tablet |

The Android listener lives inside the existing `connectedDevice` foreground
service. It is the same connection to the same peers, now accepted as well as
dialled, so the service type and its qualifying permission do not change. No
boot receiver, no wakelock, no alarm, no `dataSync` and no `specialUse` are
introduced. When the system stops the service, the listener stops and the NSD
registration is withdrawn. A device that has stopped answering is offline, and
presence (#8) reports it as offline. ADR-0009 justifies the `MulticastLock` by
browsing. Whether NSD registration also needs it held for as long as the
device advertises, and what that costs in battery, is measured during
implementation and recorded in the SPEC. It does not change the service type
or the qualifying permission.

**Every listener enforces exactly what the desktop listener enforces today**
(ADR-0007). On Android this is a server-side verifier that does not exist yet:

* client authentication is **mandatory** (`setNeedClientAuth(true)`). A peer
  without a certificate never reaches the session layer;
* an unknown but structurally valid client certificate is accepted at the TLS
  layer only so that its SPKI can be extracted. **Authorization** is the
  session layer's decision: a trust-store hit, or a valid pairing proof while
  a local pairing window is open. Nothing else counts;
* TLS 1.3 only, and **a resumed handshake is never accepted**: every
  connection runs the full handshake in which the pin and the client
  signature are checked, as ADR-0007 requires. The platform `SSLContext` has
  no public API to stop issuing TLS 1.3 tickets, so the implementation must
  prove with a test that a resumption attempt gets a full handshake or is
  refused. If the platform stack cannot guarantee that, the listener is
  blocked until it can, for example by bundling Conscrypt. Disabling tickets
  "where possible" does not satisfy this requirement;
* ALPN is selected from the same list, in the same order, as the desktop's
  listener. No ALPN, or an ALPN that does not match, drops the connection;
* the same resource bounds as T16: a cap on concurrent connections (a platform
  may choose a lower cap than 32), a handshake timeout, a protocol handshake
  timeout, and frame limits checked before allocation.

This verifier is security-critical code in the sense of ADR-0007 §Security
implications. It needs the same tests the desktop has: a client without a
certificate is rejected during the handshake, and an unpaired device cannot
use the protocol. Those tests are a precondition for enabling the listener.

**Self-connection is refused.** Every device that browses sees its own record.
A device ignores records carrying its own `device_id`. It also closes any
session whose peer SPKI equals its own, whichever side detects it, because the
same identity on both ends of a connection is a bug or a reflection, never a
peer.

**Advertising on Android is opt-in per network.** T18 records the privacy cost
of one desktop advertising a stable `id` and `dn`. This ADR multiplies that
cost by every phone and tablet, and a phone visits far more networks than a
desktop. The per-network advertising control that ADR-0005 and T18 put on the
roadmap is therefore a **prerequisite** for advertising from Android, not a
follow-up:

* Android advertises only on networks where it is allowed. Decided at
  acceptance (owner question 1):
  * advertising may be allowed automatically on the network where this device
    **successfully completed an explicit pairing**;
  * on every other network, advertising is **off until the user explicitly
    enables it**;
  * merely establishing a trusted session on a network **must not**
    automatically enable advertising there;
  * discovery remains non-authoritative and never grants trust (ADR-0005).
* While it is not advertising, the device still listens and dials. Trusted
  peers can still reach it at a remembered address (§D3). Not advertising
  reduces how easily the device can be found. It never removes access for
  peers that are already trusted.
* The one exception is a pairing window the user opened on this device
  (§D5). While it is open, the issuer advertises even on a network where
  advertising is otherwise off. The window is opened locally, lasts at most
  120 s, and is the moment the user wants the device to be found.

### D3 — Who dials, and when

**Triggers.** A device dials a trusted peer it has no live control session
with when:

1. it sees a DNS-SD record whose `id` matches that peer's stored device id;
2. a network becomes available (`onAvailable`), using the peer's remembered
   addresses;
3. the user opens the app or asks for the peer explicitly.

The dialer always pins the SPKI stored for that peer. A record's `id` only
picks **which** pin to try. It is never evidence that the peer is who it
claims to be (ADR-0005).

**Addresses are hints, kept per peer.** A device remembers, for each trusted
peer, the addresses at which a pinned handshake last succeeded, whichever side
dialled. A listener records the source address of an authenticated inbound
session together with the peer's advertised port, or the default port. Like
the QR address hints, these are not trust and not identity.

**Backoff is per peer, with jitter.** ADR-0009's capped exponential backoff
(2 s base, 5 min cap, reset on a new network) applies per trusted peer, with
±20 % jitter so that two peers that lost each other at the same moment do not
retry in lockstep. A pin failure at one address counts against that address
only. A spoofed record that leads to a failed handshake must not suppress
dialing the peer's real address (T2).

**Preferred dialer.** Simultaneous dialing is made rare, not impossible. For
any pair, compare the two 32-byte raw SPKI fingerprints as unsigned bytes,
lexicographically:

* the device with the **lower** fingerprint dials as soon as a trigger fires;
* the device with the **higher** fingerprint, if it can see that the peer is
  listening (the peer advertises), waits a random 1.5–3 s before dialing, and
  does not dial if a session from that peer arrives in the meantime;
* if the peer is not advertising, the higher device dials at once. The peer
  may not be listening at all, and waiting would only delay the one dial that
  can work.

The 1.5–3 s delay is an initial SPEC default accepted by the owner (owner
question 2): an operational constant, not a security invariant, which a future
SPEC may tune from testing without changing this decision.

Both devices know both fingerprints after pairing, so they compute the same
order without exchanging anything. The order carries no authority: it
schedules dials and settles duplicates (§D4). Nothing else may depend on it.

**Stop conditions.** A device stops dialing a peer that answers
`HELLO_STATUS_REJECTED` (the peer no longer trusts it), as ADR-0009 already
does for the single desktop. That applies to one peer, not all of them. The
Android service stops itself when no trusted peer is left to dial and no
pairing window is open. In V1 the condition was "no paired computer".

**Keepalive.** A live session is kept alive and checked with the existing
`PING`/`PONG`. The interval and timeout are fixed in the SPEC, not here. A
session that fails its keepalive is closed, and dialing resumes under backoff.

**`files.v1` data streams follow the control session.** ADR-0013 fixed *the
phone dials, the desktop accepts* because the phone was the control-session
dialer. Generalized: **the device that dialled the control session dials that
session's data streams, and the listener accepts them**. The listener is the
side that is known to be reachable. All other ADR-0013 rules are unchanged:
the challenge is issued on the control session, the data stream must present
the same pinned identity and negotiate the same profile, and the MAC binds
acceptor, dialer and transfer.

### D4 — At most one control session per pair

Two control sessions between the same two identities are never kept. Two
sessions would make it ambiguous which one carries a capability message. They
would double clipboard and notification delivery, and give each capability two
states to keep consistent.

A device detects a duplicate when a **second authenticated control session**
(both sides have exchanged `HELLO`/`HELLO_ACK` with `TRUSTED`) exists for a
peer fingerprint that already has one. Data streams (ALPN `pliwee-data/1`) are
not control sessions and are never counted.

The rule:

1. **Probe the older session.** Send a `PING` on the session that was
   established first, as this device saw it, and wait up to a bounded timeout
   (3 s, the initial SPEC default accepted by the owner).
2. **If it does not answer, it is stale.** Close it and keep the new one.
   This handles the common case in which the peer lost the old connection
   (sleep, network change, reboot) and dialled again, while this side still
   holds a half-open socket. A correct peer never dials a peer it holds a live
   session with, so a new session from it is a sign that it has abandoned the
   old one.
3. **If it answers, both are live.**
   * **Dialled by different devices: a simultaneous open.** Keep the session
     **whose dialer has the lower fingerprint** (§D3 ordering), and close the
     other.
   * **Dialled by the same device.** A correct dialer opens a second session
     only after it has given up on the first, so the dialer decides: it closes
     the session it abandoned. The listener closes neither on its own account.
     If the dialer has not closed one by the end of the probe timeout, the
     listener keeps the session it accepted last.

In step 3 both devices compute the same answer independently. The inputs are
the two fingerprints and which device dialled each session, and both sides
know all of them. No wire message is needed for them to agree. The SPEC may
add a close reason so the losing side can tell "superseded" from "network
failure". It is gated on protocol version 2 (§Wire impact), because a V1 peer
closes a session on any message type it does not know (ADR-0010).

**While a duplicate is being resolved** (at most the probe timeout), a device
sends no new capability message to that peer on either session. It goes on
receiving on both. Every capability message is sent on exactly one session.
A message sent on a session that is then closed is never re-sent on the
survivor; it fails as it would on a disconnect. That is what stops a
clipboard update or a notification from being delivered twice: per-connection
`message_id` de-duplication cannot see across two sessions.

**After a supersession:**

* the side whose session was closed does **not** count it as a failure **when,
  and only when, a surviving session to that peer is live**. Backoff does not
  grow, and it does not redial while the survivor lives. If both sessions
  closed, for example because a probe timed out on a lossy path, that is an
  ordinary failure and normal backoff applies. Otherwise two honest peers on a
  bad network could redial in a tight loop until the churn bound below shut
  them out;
* operations in flight on the closed session fail exactly as they would on any
  disconnect: a `files.v1` transfer whose challenge was issued on that session
  fails, and is not silently moved to the survivor. Duplicates arise at connect
  time, before most capability traffic, so this is rare;
* per-connection capability state (ADR-0017 role announcements and epochs, the
  replay window) stays with the session it belongs to. The survivor keeps its
  own. Nothing is merged from the loser, and the loser's state is dropped with
  it.

**Disagreement converges.** If the two sides reach different conclusions, for
example because the older session dies during the probe, the worst outcome is
that both sessions close. Both sides then redial under jittered backoff, and
the preferred-dialer delay makes a second collision unlikely. Losing a session
is a delay. Keeping two would be a correctness problem.

**A hostile trusted peer cannot exploit this beyond churn.** Only a holder of
a pinned identity can open an authenticated session at all. A trusted peer
that keeps opening sessions to make this side drop its existing one is A6,
and it can only disrupt its own pairing. Bound it anyway: more than 5
supersessions for one peer within 60 s refuses that peer's new sessions for
60 s and logs the event with the short fingerprint only (T11). These limits,
and the 3 s probe above, are initial SPEC defaults accepted by the owner (owner
question 2): operational constants, not security invariants, which a future
SPEC may tune from testing without changing this decision.

### D5 — Symmetric pairing: issuer and joiner

ADR-0006's flow already has two roles. It names them after device kinds. This
ADR names them after what they do, and lets any device play either:

* **Issuer** — opens a pairing window locally, generates the token, shows the
  pairing code, and **listens** for the pairing connection. It is always the
  TLS server of the pairing connection and the **responder** in the proof.
* **Joiner** — receives the code out of band, **dials** the issuer, and proves
  possession of the token. It is always the TLS client and the **initiator** in
  the proof.

Fixing *issuer = TLS server = responder* keeps the order of the fingerprints in
the proof (`responder_fp ‖ initiator_fp`) tied to one role. A proof computed
for one direction can never verify in the other.

**What is kept from ADR-0006, unchanged:**

* the token: 160 bits from the OS CSPRNG, single use, 120-second TTL judged by
  the issuer's monotonic clock, zeroed on drop, never sent on the wire;
* the issuer's fresh 32-byte nonce in `HELLO_ACK{PAIRING_REQUIRED}`;
* `proof = HMAC-SHA256(token, "pliwee/pairing-proof/v1" ‖ len32(responder_fp)
  ‖ responder_fp ‖ len32(initiator_fp) ‖ initiator_fp ‖ len32(nonce) ‖
  nonce)`, the confirmation under the domain-separated
  `pliwee/pairing-confirm/v1`, and the legacy-profile domains of ADR-0020;
* constant-time comparison on both sides;
* abort after 3 failed proofs;
* ADR-0006's order on the issuer: verify the proof, **consume the token**,
  then ask the human, then answer `PAIR_STATUS_ACCEPTED` or
  `PAIR_STATUS_DECLINED_BY_USER`. Consuming before the prompt means a second
  proof can never race the first while the prompt is open. A declined pairing
  costs the user a new window;
* **mutual pinning**: on success both store the other's SPKI fingerprint and
  device id, and the token is destroyed. Because both sides of every pairing
  can now dial (§D1), mutual pinning is what lets either side reconnect later.

**Pairing windows are opened locally, never over the network.** Either device
may initiate: the human chooses *Show a pairing code* on one device and *Scan
or enter a code* on the other, starting from whichever device they are holding.
No network message can open a pairing window or show a pairing prompt on
another device. Letting an unauthenticated LAN peer raise a dialog on a TV or
a phone would turn discovery into a way to push UI, and that is a short step
from treating discovery as trust.

A device is in at most one pairing role at a time. Starting to join closes any
window it has open as an issuer.

On Android the issuer listens while the pairing screen is in the foreground.
An activity in the foreground may accept connections, so no foreground service
is needed for the window. Leaving the screen closes the window and destroys
the token.

**Two ways to carry the code.**

| Mode | Code carries | Joiner pins before connecting? | Used when |
| --- | --- | --- | --- |
| **QR** | issuer SPKI fingerprint, token, issuer device id, address hints. The existing `pliwee1:` format, unchanged | **yes**, exactly as ADR-0006/0007 | the joiner has a camera and the issuer has a screen. The preferred mode |
| **Text code** | token only (160 bits as grouped base32, plus a check digit), in a new scheme fixed by the SPEC | **no**: the issuer SPKI is verified by the confirmation MAC before it is stored | no camera on the joiner, or no screen able to show a QR on the issuer, but a way to read text on one side and enter it on the other: keyboard, paste over SSH, TV remote as a last resort |

The QR format already names no device kind. It is
`<responder-fingerprint>:<token>:<device-id>:<addrs>`, so a phone, tablet or TV
can issue it unchanged, and every existing V1 parser reads it.

**The text-code mode is the one place this ADR changes when pinning happens**,
and it is bounded:

* The joiner learns the issuer's address by choosing it from discovered
  records, or from an address typed alongside the code. That choice is a
  routing hint only.
* During the pairing connection, and only then, the joiner accepts any
  structurally valid server certificate, with the handshake signature still
  verified by the platform, and records the server's SPKI.
* It requires `HELLO_ACK{PAIRING_REQUIRED}`. Any other status from a server
  it has not pinned, `TRUSTED` included, aborts the pairing.
* It sends `HELLO` and `PAIR_REQUEST` only. It sends no capability traffic,
  stores nothing, and accepts nothing else until `PAIR_RESPONSE{ACCEPTED}`
  arrives with a confirmation MAC that verifies under the token against
  **the SPKI it recorded**. Only then is that SPKI pinned.
* Text-code pairing runs on the canonical profile only (ALPN `pliwee/1`). A
  record seen only under `_omnibridge._tcp` is not a text-code issuer, so a
  spoofed legacy record cannot steer the pairing onto the legacy profile.
* A man in the middle presents its own key. The joiner's proof is then bound
  to the attacker's fingerprint and cannot be relayed to the real issuer. The
  attacker cannot produce a valid confirmation without the token, and cannot
  recover the 160-bit token offline from a proof. Trust is created only by
  proof of knowledge of the token in both directions, so this is not trust on
  first use, the thing ADR-0007 rejects.
* What the joiner does expose to an impostor before the MAC check is its
  `HELLO`: its device name, platform, fingerprint and capability list. That
  residual is recorded in §Security implications.
* Because the joiner did not pin the issuer out of band, **the joiner's human
  confirmation is mandatory in text-code mode, and it happens before
  `PAIR_REQUEST` is sent**. The joiner knows both fingerprints as soon as the
  TLS handshake ends, so it can show the comparison code then. A decline sends
  nothing, which leaves the issuer with nothing to retract. In QR mode the
  code is shown at the same point and the joiner's confirmation is **equally
  mandatory** (owner question 3, below).
* **The text code is a secret on screen and must be handled as one.** This is
  T7 in a new form: a code that can be typed can also be read over a
  shoulder. Rules:
  * the desktop CLI reads the code from the terminal or stdin, never from
    `argv`, so it reaches neither shell history nor `/proc/<pid>/cmdline`;
  * Pliwee offers no "copy code" action;
  * an Android field that accepts a code is excluded from `clipboard.v1` and
    marked sensitive;
  * the code is never logged, as the token is today (T11);
  * a window shows its code only while it is open, and closing the window
    destroys the token.

**Human confirmation compares one string on two screens.** ADR-0006 asks the
issuer's human to confirm the joiner's fingerprint, read against nothing. In
V2 both sides show the same **comparison code**, derived from both
fingerprints in a fixed order:
`SHA-256("pliwee/pairing-compare/v1" ‖ len32(responder_fp) ‖ responder_fp ‖
len32(initiator_fp) ‖ initiator_fp)`. The full fingerprint stays available on
request.

**Human confirmation is mandatory in both pairing modes**, QR and text code
(owner question 3). A pairing must not become trusted until the required local
human confirmation succeeds: on the issuer, the prompt that sits between
consuming the token and answering `PAIR_STATUS_ACCEPTED`, as in ADR-0006; on
the joiner, the comparison before `PAIR_REQUEST` is sent. A QR code pins the
issuer before connecting, but it does not replace either confirmation.

**The comparison code carries at least 64 bits**, rendered as six words from a
fixed 2048-word list (66 bits), as accepted by the owner (owner question 4).
The exact word list and its rendering belong to the SPEC. It is the last
defence against an attacker **who already holds the token** (ADR-0006, T7), and a text code makes that
attacker more likely. Such an attacker sits in the middle, fixes the key it
shows the joiner, learns the joiner's fingerprint from TLS, and then generates
keys for its own connection to the issuer until the two screens agree. Against
a 6-digit code that takes about 2^20 key generations, a matter of seconds and
well inside the 120 s window. Against 64 bits it is out of reach. That is why
this ADR does not use the short numeric code ADR-0006 rejected: a short code
would need a commitment round to be safe, and a long one does not. With the
long code, the human comparison keeps the strength of ADR-0006's
full-fingerprint confirmation while being easier to read.

A V1 peer cannot show the code. A V2 issuer knows its joiner is V1 from the
`HELLO` (maximum protocol version 1, §Wire impact), and shows the joiner's
full fingerprint instead, as in V1.

**Device-kind matrix.**

| Pair | Issuer → joiner (preferred) | Fallback |
| --- | --- | --- |
| phone ↔ tablet | either shows QR, the other scans | text code |
| phone / tablet ↔ desktop | desktop shows QR, phone scans (the V1 flow) | phone shows text code, entered at `pliwee pair` on the desktop (read from the terminal); or the reverse |
| phone ↔ Android TV (no camera) | TV shows QR, phone scans | phone shows text code, typed on the TV with the remote |
| desktop ↔ desktop | either prints the text code, the other enters or pastes it | QR rendered in the terminal, if the other side has a camera |
| headless desktop ↔ anything | headless prints QR or text code to its terminal | headless joins with a pasted text code |
| two devices with no camera and no way to show or enter text | **not supported by this ADR** | needs a different out-of-band channel (§Alternatives) |

---

## Alternatives

### Topology

**Keep a desktop hub.** Every phone and tablet keeps dialling one desktop, and
the desktop relays. Rejected: #4 makes a mandatory hub out of scope. A phone
and a tablet could not meet without a computer switched on, and relaying puts
a third device inside a pairwise trust relationship.

**Fixed roles by device kind** (desktop listens, TV listens, tablet dials
desktops but listens for phones, …). Deterministic, and avoids simultaneous
dialing between different kinds. Rejected:

* the wire carries no device kind, and adding one would make a peer-supplied
  string an input to a connection decision;
* equal kinds (phone ↔ phone, desktop ↔ desktop) need a tie-break anyway;
* it fails whenever the designated listener is asleep and the other side is
  awake.

**Exactly one dialer per pair, the lower fingerprint, always.** This removes
simultaneous dialing entirely, which is attractive. Rejected because of fact 3:
when the lower device cannot reach the higher one (for example a desktop
firewall blocks inbound connections), the pair never connects, even though the
other direction would have worked. Kept as a **preference** (§D3), with
duplicate resolution behind it.

**Two sessions per pair, one per direction.** Rejected: every capability would
need a rule for which session to send on. Clipboard loops (T23) and duplicate
notifications would come back, and the "one connection, one identity profile"
rule (ADR-0020) would have two connections to reconcile.

**An in-band simultaneous-open negotiation** (each side sends a random value in
`HELLO`, the higher value wins, like libp2p). Rejected: it needs a new wire
field and a round trip. Ordering by fingerprint gives the same determinism from
values both sides already hold.

**Newest session always wins.** Simple. Rejected on its own: in a true
simultaneous open "newest" is measured on two clocks, so both sides may close
the session the other kept. It survives inside §D4 only for the stale case,
gated by a liveness probe.

**Wake a sleeping device** (Wake-on-LAN, FCM, BLE wake). Rejected for V2. FCM is
excluded on principle (ADR-0009). Wake-on-LAN does not apply to phones and is a
desktop power-management decision that belongs to a capability of its own. BLE
is the nearby-transport question of #26. A sleeping device is offline, and
presence (#8) says so.

### Pairing

**Keep ADR-0006's asymmetric flow.** Rejected: there is no flow for phone ↔
tablet, and none for a TV.

**Numeric comparison / SAS with a commitment** (Bluetooth Secure Simple
Pairing). It needs no secret, no camera and no text entry, only two displays.
Rejected for V2, for three reasons: the commitment rounds are new protocol
machinery; the whole guarantee would rest on the human comparison, which
ADR-0006 already calls the weakest assumption in the model; and it would drop
the single-use short-lived secret that #39 requires the design to keep. It is
the natural candidate if a pair with no camera and no text entry ever needs
supporting.

**A PAKE (CPace, SPAKE2) over a short typed code.** It would make the text
code 6–8 digits instead of 32 characters, which matters for a TV remote.
Rejected for now, for ADR-0006's reason: our secret is high-entropy, so there
is no low-entropy exchange to protect. It would also add a cryptographic
dependency on both platforms, plus known-answer vectors to keep in step. To be
revisited, through its own ADR, if typing the text code turns out to be the
reason TV pairing fails in practice.

**Put a truncated issuer fingerprint in the text code** (for example 80 bits),
so the joiner pins a prefix before connecting. It would remove the
pre-confirmation `HELLO` exposure. Rejected: it makes the code about 50 % longer
for every user, to protect information (name, platform, capability list) that
the device already advertises over mDNS when advertising is on.

**Network-triggered pairing** (the phone asks the TV, over the LAN, to open a
window and show its code). Convenient. Rejected: it lets any unauthenticated
device on the network raise a prompt on another (§D5).

**Pairing through an already-trusted device** (introduction). That is
transitive trust, decided in #6. Out of scope here.

**NFC or BLE as an out-of-band channel.** Hardware-dependent, and not present
on desktops or most TVs. Future work, alongside #26.

---

## Compatibility

### What ADR-0005 keeps and loses

| ADR-0005 | After this ADR |
| --- | --- |
| DNS-SD, `_pliwee._tcp` (and `_omnibridge._tcp` legacy, ADR-0020) | kept |
| TXT `v`, `pv`, `id`, `dn`; fingerprint **not** published | kept. No new TXT key is needed: advertising itself means "listening" |
| "Discovery is not trust"; sanitizing names and ids | kept, for every device that browses |
| **desktop advertises, phone browses and always dials; direction fixed** | **superseded**: any device advertises while listening and dials when it has a reason to (§D1–D3) |
| rejected "phone advertises, desktop connects" | its reasons are kept as constraints: a phone's address changes (handled by remembered addresses and mDNS), it sleeps (it is offline, §D2), and "one handshake path" now means one TLS and session state machine, used in both roles |
| per-network advertising toggle "on the roadmap" | becomes a prerequisite for Android advertising (§D2) |

### What ADR-0006 keeps and loses

Kept: everything under *Identity* (P-256 in the Android Keystore, StrongBox
where available; the desktop key file and its mode checks; the SPKI
fingerprint; the random device id) and every property of the token, the proof
and the confirmation. Android TV uses the same Keystore path as phones.

Superseded: steps 1–3 as written (desktop issues, phone scans), now *issuer*
and *joiner* of any kind (§D5); and the consequence "Pairing needs a camera",
now "QR or text code".

### What ADR-0007 keeps

All of it. "The desktop requires client authentication" becomes *every
listener requires client authentication*. "The client pins the server's SPKI,
taken from the QR code at pairing time and from the trust store afterwards"
holds for QR pairing and for every connection after pairing. Text-code pairing
verifies the SPKI by the confirmation MAC before pinning it, and grants nothing
until then (§D5).

### Wire impact

* **The topology itself needs no new message.** The session state machine
  already has a client role (`HELLO`, `PAIR_REQUEST`) and a server role
  (`HELLO_ACK`, `PAIR_RESPONSE`). A V2 device runs each role on more
  platforms. Duplicate resolution uses `PING`/`PONG`, which exist. A V2 device
  that negotiated version 1 with a V1 peer runs D1–D4 unchanged.
* **Protocol version 2 is how a V2 device recognises a V2 peer.** A device
  that implements this ADR offers `max_protocol_version = 2` in `HELLO`. Under
  the existing negotiation rule (ADR-0010, PROTOCOL.md) both sides then know
  whether the other is V2, before any pairing or capability message. Two
  behaviours depend on it, and only those two: the comparison code (§D5) and
  the optional "superseded" close reason (§D4). Against a peer that
  negotiated version 1, a V2 device shows the full fingerprint instead of the
  comparison code and closes a superseded session without a reason. The SPEC
  defines what version 2 adds. Nothing else in this ADR depends on the
  version.
* **New, to be specified:** the text-code scheme, canonical profile only
  (§D5).
* **Not needed:** a device-kind field. If #8 adds one for presence, it is
  descriptive metadata and must not feed into any decision in this ADR.

### Migration from V1 (Android ↔ desktop)

* **Existing pairings stay valid.** Trust is the SPKI pin, which this ADR does
  not touch. Nobody re-pairs.
* **New phone, old desktop.** The phone dials as before. The old desktop never
  dials, so the phone's listener is unused. Nothing breaks.
* **Old phone, new desktop.** The old phone never listens or advertises. The
  desktop has no record to react to, and dials to remembered addresses fail
  quietly under backoff. The phone keeps dialling as before. Nothing breaks.
* **Both new.** Either side reconnects. The desktop learns the phone's
  addresses from mDNS, where the phone advertises, and from the source address
  of the phone's previous inbound sessions.
* **OmniBridge 1.0.0 desktops (legacy profile).** They never dial and never
  join. A Pliwee device reaches them as it does today, under the legacy profile
  (ADR-0020). Nothing in this ADR is offered on the legacy profile.
* **Pairing a V1 device.** A V2 desktop issuing a QR to a V1 phone is the V1
  flow, unchanged. A V1 app can neither issue nor accept a text code: it has
  no path for entering one. A V1 device therefore pairs only by QR, and only
  as the joiner (phone) or issuer (desktop) it already is.
  **Open, for the owner (recorded 2026-10-04, #41):** a V1 joiner has no
  joiner-side confirmation, and a V1 issuer cannot show a comparison code. How
  mandatory human confirmation (owner question 3) applies when one side of a
  pairing is a V1 device is not settled by the acceptance decision, and this
  bullet is not to be read as settling it.
* **Stored state.** The trust store gains per-peer address hints and dial
  bookkeeping. Neither is trust, and its migration is an implementation detail
  for the SPEC.

---

## Consequences

* A phone and a tablet pair and talk with no computer present. A TV pairs with
  a phone without a camera.
* Every platform now carries both connection roles. The Android server-side TLS
  verifier is new security-critical code, and the desktop gains a dialer.
* Connections exist only while at least one side's process is alive and
  listening. Two phones whose services are both stopped cannot meet. That is
  the honest outcome of ADR-0009, not a defect to engineer around.
* Duplicate sessions are resolved deterministically, and in rare races they
  cost a reconnect. Capability code must already survive a session closing
  under it, and still must.
* The privacy cost of mDNS grows with every advertising device. It is contained
  by advertising only while listening, and on Android only on allowed networks.
* Text-code pairing is slower than scanning a QR. It is the fallback, not the
  default.
* `docs/security/THREAT_MODEL.md` must be updated **with the implementation**,
  not before, for: an inbound listener on Android (T16 per platform); session
  churn by a hostile trusted peer (§D4); the text-code mode's
  pre-confirmation exposure (T3); a token that can now be read and typed
  (T7, §D5); and T18's wider advertisement.
* `docs/architecture/PROTOCOL.md`, `OVERVIEW.md` and `FILES.md` describe the
  phone-dials model today. They change with the implementation.

## Security implications

* **Discovery is still not trust.** A record picks which pin to try. A spoofed
  record leads to a handshake that fails the pin, and that failure counts
  against the spoofed address only, never against the peer (§D3).
* **Authorization is unchanged and applies on every listener:** trust-store hit
  or pairing proof inside a locally opened window. `an_unpaired_device_cannot_use_the_protocol`
  must hold for every device kind in both connection roles. That is an
  acceptance condition for the implementation, not a hope.
* **No new path to trust.** Pairing windows open only locally. Text-code
  pairing creates trust only after the token is proved in both directions.
  Ordering by fingerprint and duplicate resolution decide which of two already
  authenticated sessions survives, and nothing more.
* **Replay.** Unchanged layers: TLS records, `sequence` and `message_id` per
  connection, a single-use token, the issuer's nonce. Fixing *issuer = TLS
  server = responder* prevents a proof being valid in the reverse direction.
  The domain separators keep a proof from being replayed as a confirmation,
  and a third domain keeps the comparison code distinct from both.
* **Downgrade.** The identity profile is still fixed once per connection by
  ALPN, with no fallback (ADR-0020). The pairing mode is fixed by the code the
  human used: a QR always pins, and a network attacker cannot turn QR pairing
  into text-code pairing. A V2 peer offered a V1 flow runs the V1 flow, which
  is the same construction. The per-peer protocol-version floor (T12) is still
  recorded, still not enforced, and becomes more relevant with more peers.
* **Residual: text-code exposure.** In text-code mode the joiner sends `HELLO`
  to an unauthenticated server before the confirmation MAC verifies. An active
  attacker on the LAN who wins the address choice learns the joiner's name,
  platform, fingerprint and capability list, then fails. It learns no secret
  and gains no trust. The issuer side is exactly as strong as in V1.
* **Residual: a token holder.** As in V1 (T7), an attacker who reads the code
  and is faster than the user reaches the human prompt. The comparison code's
  ≥ 64 bits keep that prompt meaningful against an attacker who can generate
  keys (§D5). The prompt depends on the user actually comparing, which
  ADR-0006 calls the weakest assumption in the model, and that is still true.
* **Residual: window burning.** An attacker can submit 3 bad proofs and abort
  an issuer's window (as in V1). This is visible to the user, who opens a new
  one.
* **Residual: churn.** A hostile trusted peer can force reconnects, within the
  supersession bound. It can disrupt only its own pairing.
* **Logging.** Duplicate resolution, dial attempts and pairing log short
  fingerprints, the outcome and addresses (public values, T11). The text code,
  the token, and `HELLO` contents beyond the sanitized name are never logged.

## Owner decisions at acceptance

Decided by the project owner, Yuri C. Sismotto, on 2026-10-04
([#41](https://github.com/yurisismotto/pliwee/issues/41)). These were the
values and defaults the proposal left for the owner to set at acceptance. Each
answer is applied in the section it concerns.

1. **Android advertising per network** (§D2). Android may automatically allow
   advertising on the network where this device successfully completed an
   explicit pairing. On every other network, advertising is off until the
   user explicitly enables it. Merely establishing a trusted session on a
   network must not automatically enable advertising there. Discovery remains
   non-authoritative and never grants trust.
2. **Connection constants** (§D3, §D4). The proposed values are accepted as
   initial SPEC defaults: preferred-dialer delay 1.5–3 s; stale-session probe
   3 s; churn protection — more than 5 supersessions for one peer within 60 s
   refuses new sessions from that peer for 60 s. They are operational
   constants, not security invariants, and a future SPEC may tune them from
   testing without changing this decision.
3. **Human confirmation** (§D5). Mandatory in both pairing modes, QR and text
   code. A pairing must not become trusted until the required local human
   confirmation succeeds.
4. **Comparison code** (§D5). At least 64 bits of comparison entropy, rendered
   as six words from a fixed 2048-word list (66 bits). The exact word list and
   rendering belong to the SPEC.

## Notes

* Umbrella: #4. Prerequisite of #5, #6, #7, #8 and of the V2.1 transports #26
  and #27. Neither of those is decided here: the dialer/listener rule is stated
  for TCP on a LAN, and the "one session per pair" rule is stated per pair of
  identities, so another transport can be added under it.
* This ADR was accepted on 2026-10-04 (#41). Implementation still needs a SPEC
  (text-code scheme, close reason, constants, comparison-code word list and
  rendering, trust-store fields), which does not exist yet.
