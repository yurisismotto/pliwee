# ADR-0023 — Pliwee Space: pairwise trust for the multi-device group

**Status:** Proposed · 2026-10-04

Proposed for [#43](https://github.com/yurisismotto/pliwee/issues/43), the
trust-model decision that [#6](https://github.com/yurisismotto/pliwee/issues/6)
requires. Not accepted. Nothing in this ADR may be implemented until the
project owner accepts it. The open points the owner is asked to settle are in
[§Questions for the owner](#questions-for-the-owner).

"Pliwee Space" is the working name #6 gives the group for architecture
documents. Whether the UI uses it is not decided here.

**Relationship to earlier ADRs.**

* **Keeps** [ADR-0006](ADR-0006-device-identity-and-pairing.md) whole as
  amended by ADR-0022: the P-256 identity key, the SPKI fingerprint, the random
  device id that is not a credential, the token, the proof, the confirmation
  MAC and mutual pinning.
* **Keeps** [ADR-0007](ADR-0007-tls-transport-and-pinning.md) whole: TLS 1.3,
  mandatory mutual authentication, SPKI pinning, and no CA, local or otherwise.
* **Keeps** [ADR-0022](ADR-0022-any-to-any-topology-and-symmetric-pairing.md)
  whole, and answers the question it leaves to #6. ADR-0022 states *"Trust
  stays pairwise; whether it ever becomes transitive is #6's decision"* and
  lists *"Pairing through an already-trusted device (introduction)"* as out of
  its scope. This ADR decides: trust stays pairwise, and no introduction
  mechanism exists in this iteration (§D1, §D8).
* **Constrains**, without deciding, the future revocation work of
  [#19](https://github.com/yurisismotto/pliwee/issues/19) (§D7).
* Leaves [ADR-0008](ADR-0008-capability-architecture.md) and
  [ADR-0017](ADR-0017-capability-roles.md) unchanged. Capability grants stay
  per peer.

No earlier ADR is superseded or amended, so none needs a note.

---

## Context

V1 trust is a relation between two devices. Each device keeps a trust store of
peer SPKI fingerprints. A record is created only by a human-confirmed pairing.
A revocation keeps the record with `revoked` set and its grants cleared,
instead of deleting it. Hiding a revoked record reduces it further to a
tombstone, which keeps only the fingerprint and the revoked flag. Both trust
stores already work this way: `desktop/core/src/store.rs` and
`android/app/src/main/java/io/github/yurisismotto/pliwee/store/TrustStore.kt`.
Capability grants are stored per peer, independently of what the peer
advertises (THREAT_MODEL T5).

ADR-0022 lets any device pair with and talk to any other. With three or more
devices, a person thinks "my devices", not a list of pairs. #6 asks for a model
of that group: how a device joins, how the set is shown, and what removing a
device means. It must do this without weakening the pairwise trust V1
established.

There are two basic shapes:

1. **Pairwise.** Every pair of devices that should trust each other pairs
   explicitly. The group is a view over those pairings.
2. **Transitive, by introduction.** A member can vouch for a new device, and
   other members accept it without pairing with it directly.

These invariants hold for every option considered, as set out in #43:

* a device name is not identity;
* a device id is not a credential;
* network proximity is not membership;
* discovery is not trust;
* cryptographic identity is the device key, named by its SPKI fingerprint;
* no cloud service and no account is introduced;
* no transitive trust is introduced implicitly.

---

## Decision

### D1 — Trust is pairwise

A device trusts exactly the peers whose SPKI fingerprints are in its own trust
store, unrevoked. Each of those records was created by a pairing between the
two devices themselves (ADR-0006, ADR-0022 §D5). That pairing ran in a window
opened locally on the issuer, carried a single-use token out of band, and
passed the human confirmations the negotiated protocol version requires.

Nothing else creates a trust record: not another member, not a message, not a
Space. In particular:

* **no introduction.** A member cannot cause another member to trust a third
  device, with or without a signature, a prompt or a confirmation (§D8);
* **no shared group secret, group key or group certificate.** No credential
  exists whose possession makes a device a member;
* **no device acts as an authority.** No member is the "owner", "primary" or
  "admin" of the group. Every member decides for itself.

### D2 — What a Pliwee Space is

**A Pliwee Space is a local view, not a shared object.** On a device X:

> **Space(X)** = X itself, together with every peer whose fingerprint has an
> unrevoked record in X's trust store.

From that definition:

* **The Space has no identifier, no key, no member list and no wire
  representation.** It cannot be sent, signed, joined, synchronised or
  impersonated, because it does not exist outside one device's trust store.
* **Every device's Space is authoritative for that device only.** Space(A) and
  Space(B) are two different sets and may differ (§D5).
* **"The user's Space"**, the group a person has in mind, is the union of their
  devices' views. Pliwee never computes it as an authority. A UI may describe
  it, but no decision is ever made against it.

Each device can list its Space with every member's **identity and trust
state**, which is #6's acceptance condition. The same list may also show the
device's revoked records, marked as such. They are not members, and they are
shown so that a revoked device stays attributable (T14):

| Field | Source | Role |
| --- | --- | --- |
| SPKI fingerprint (short form, full on request) | the trust store | **the identity** |
| device id | the trust store, learned at pairing | a lookup key that picks which pin to try (ADR-0022 §D3). Never a credential |
| device name | the trust store, sanitized (T17) | display only. Never identity |
| trust state | the trust store | *trusted* (a member) or *revoked* (not a member). A revoked record the user hid is a tombstone and is not listed, exactly as both stores keep them today |
| observed state | this device's own sessions | not trust: last seen, online or offline (#8), and *this peer rejects us* when the peer answers `HELLO_STATUS_REJECTED` (ADR-0022 §D3) |
| capability grants | the trust store | per peer, unchanged (T5, ADR-0017) |

### D3 — Membership is cryptographic trust, and nothing else

**D is in Space(X) if and only if X's trust store holds an unrevoked record for
D's SPKI fingerprint.** No membership state exists separately from trust:

* no membership without a pin, and no unrevoked pin without membership;
* being on the same network, appearing in DNS-SD, using a familiar name or a
  remembered device id, or being trusted by another member does **not** make a
  device a member of anyone's Space;
* **membership grants no capability.** What a member may do is still decided
  per peer and per message by its grants (T5, ADR-0017). A device that is a
  member of X's Space with no grants can open a session and nothing more.

**A trusted relation is mutual when it is created and can become one-sided
later.** Pairing pins both ways (ADR-0022 §D5), so a new pairing puts each
device in the other's Space. Revocation is local (§D6), so afterwards A may
have revoked B while B still holds A pinned. B then sees *this peer rejects
us*, stops dialing A (ADR-0022 §D3) and keeps A in its list until its own user
removes it. This is already true in V1. No message carries a revocation, so a
desktop-side revoke leaves the phone's record untouched, and the phone learns
of it only when the desktop answers `HELLO_STATUS_REJECTED` (outside an open
pairing window on the desktop, where a revoked peer is treated as unknown and
answered `PAIRING_REQUIRED`). The phone treats
that answer as terminal for that peer. The UI must say so instead of hiding
it.

### D4 — A device in more than one Space, and how a new device appears

**There is no named Space for a device to belong to.** "A device in two
Spaces" therefore has no meaning to define. What a person means by it, such as
a family TV shared by two people, is fully described by pairings:

* the TV pairs with Alice's phone and with Bob's phone. Space(TV) contains
  both phones;
* Space(Alice's phone) contains the TV, not Bob's phone, and the reverse;
* Alice's phone and Bob's phone trust each other only if they pair with each
  other.

So overlapping groups need no mechanism, and **this iteration introduces no
partitioning, labels or named Spaces.** This **replaces** #6's suggested
default, that a device belongs to one group until a use case justifies more.
It does not satisfy that default. A device has exactly one Space, its own
view. In the sense of §D2's "user's Space", the shared TV belongs to both
Alice's group and Bob's. That is allowed because overlap needs no rule: no
member's trust depends on the group (§Questions for the owner, 4). A future
proposal for named or labelled Spaces would make them a local display grouping
that cannot change trust. It would need its own ADR.

**A newly paired device appears only to the device it paired with.** When D
pairs with A:

1. D enters Space(A), and A enters Space(D).
2. For every other member B, nothing changes. D is unknown to B. B refuses D's
   sessions: D is not in B's store, and no window is open on B (ADR-0022
   §D2). D may discover B by DNS-SD, and that is not trust.
3. For D and B to trust each other, a person pairs them directly, starting
   from a window opened locally on one of them (ADR-0022 §D5).

A device that should reach N other devices needs N pairings. A fully
connected group of n devices needs n(n−1)/2: 3, 6 and 10 for the three, four
and five devices a person typically has once #6's "three or more" applies. That
is the cost of this decision (§Consequences). The UI may lower it by guiding
the person through the pairings one after another. A guided flow is UX only:
every step is still a full pairing with its own token and confirmations. It
never pre-fills a fingerprint learned from another member, and no network
message opens any of its windows.

### D5 — Different views of the group are normal and need no agreement

Members' views differ whenever pairings or revocations are not complete across
all devices. Pliwee does not try to make them agree: there is **no consensus,
no convergence protocol, and no authoritative member list.** Each device acts
on its own store only.

| Situation | What happens | Outcome |
| --- | --- | --- |
| A trusts D, B does not | D talks to A only | correct. The person has paired D with A only |
| A revoked C, B has not yet | C keeps its session with B until B revokes | correct, but needs revoking on B too. Showing where C is still trusted is #19's |
| A revoked B, B still trusts A | B is rejected by A and stops dialing it (ADR-0022 §D3) | one-sided. B shows *this peer rejects us* |
| B is offline while the person revokes C elsewhere | B still trusts C when it comes back | B is revoked when the person revokes on B. Nothing reaches B automatically in this iteration (§D7) |

Because no device acts on another's view, a divergence can never **widen**
trust. In the worst case a revocation has not happened yet on some member, and
that is visible on that member.

**Views reported by other members are claims, not state.** #6 wants each
device to list the group, and #19 wants to show where a device is still
trusted. Both may eventually want to show what *other* members say they trust.
This ADR does not define a mechanism for that (§Questions for the owner, 1).
If one is ever specified, it is bound by this decision:

* a report is a claim by the reporting member, received only over that
  member's authenticated session, and shown as that member's claim;
* it names devices by fingerprint. A name or a device id in a report is
  display only;
* it **never** creates, changes or removes a record in the receiving device's
  trust store, never opens a pairing window, and never raises a pairing prompt
  (ADR-0022 §D5);
* a member that lies in a report can mislead a display, nothing more.

### D6 — Removing a device

**Removal is revocation, and revocation is local.** "Remove D from the Space"
means revoking D on each device whose Space contains it. On each one, the
existing semantics apply: the record is kept with `revoked` set, its grants
are cleared, D's live session is torn down at once, and a later connection
from D is refused and attributable (T5, T14). Bringing D back takes a fresh
pairing. No member can revoke D in another member's store in this iteration.

Two current implementation facts must change with #6. They are not
decisions of this ADR. On Android, revoking stops the whole connection
service, which is correct with one peer but must become a per-peer teardown
when there are several. And "a later connection from D is refused" holds for
inbound connections only once a device listens: today that is the desktop,
and on Android it waits for ADR-0022 §D2's listener.

**A device leaving by its own choice** revokes all its peers locally. The
others still hold its pin until they revoke it. If the leaving device also
destroys its identity key (on Android: reinstall, data reset or factory reset;
on Linux: deleting the identity file, which a package reinstall leaves in
place), those
pins can no longer be satisfied by that device. On Android that is final,
because the Keystore key was never exportable. On a Linux desktop the key is a
software file (ADR-0006), so a copy or a backup of it outlives the
destruction. Only revocation on the members closes that. A new key is a new
device that must pair again (ADR-0006). Destroying the key is the right way to
hand a device on, and the UI should offer it there.

### D7 — What this means for revocation propagation (#19)

With pairwise trust, "revoke everywhere" means revoking on every member. A
member that is offline learns about it when the person revokes on it. #19 owns
the guided flow and the "where is it still trusted" view. Both are possible
under this ADR, and the view relies on §D5's claims, if those are ever
specified.

This ADR **does not** introduce propagated revocation, meaning one member
narrowing another member's trust by message, and it does not decide whether
#19 should. If #19 proposes it, its ADR must at least:

* **only narrow, never widen.** A propagated statement can revoke and can never
  trust, restore or un-revoke. Restoring trust still takes a fresh pairing;
* **be attributable and verifiable** against the sender's pinned identity,
  received over its authenticated session or signed by its key, and never be
  accepted from discovery or from an unpinned peer;
* **address the compromised revoker.** A hostile member that "revokes" every
  other member is a denial of service against the whole group. The ADR must
  say whether a propagated revocation needs local confirmation, applies only to
  the subject device, or is limited in some other way. A revocation applied
  without a local decision makes one member act on another's store. That
  departs from §D1's "every member decides for itself", and that ADR must amend
  §D1 explicitly to allow it;
* **address the revoked device revoking its accuser.** A stolen device that
  learns it is being revoked must not be able to race the revocation, for
  example by revoking the legitimate members first;
* **keep tombstones.** A revoked device stays recorded and refused, as today
  (T14).

The asymmetry is deliberate. Narrowing trust by message risks lost
availability. Widening it by message risks lost confidentiality and integrity
for the whole group, and that is what §D8 rejects.

### D8 — No introduction in this iteration, and what a future one would need

Transitive trust is rejected for the first V2 iteration (§Alternatives). It may
be proposed later only through its **own ADR**, and that ADR must:

* make introduction **explicit and opt-in**, on the device that would gain
  trust, and never implicit or automatic;
* require a **human confirmation on the receiving device** that does not rely
  only on information supplied by the introducer;
* say how it reconciles with ADR-0022 §D5, under which no network message may
  open a pairing window or raise a pairing prompt on another device. §D5's
  stated concern is unauthenticated LAN peers. **This ADR reads the rule as
  covering authenticated members too**, because a prompt pushed by a member is
  exactly how a compromised member would spread trust. On that reading, an
  introduction that prompts the receiving device over the network needs §D5
  amended or clarified by an accepted ADR first;
* bound what one compromised member can do through it (§Security
  implications);
* define how an introduced trust is revoked, and whether revoking the
  introducer affects it.

---

## Alternatives

### Transitive trust by signed introduction, accepted automatically

A trusted member A sends every other member B a statement signed by A's
identity key: "trust the device with fingerprint D". B adds D to its store.

* **For:** the Nth device needs one pairing, not N−1. It is the most convenient
  model.
* **Against, and decisive:** **one compromised member silently expands trust
  across the whole group.** A stolen unlocked phone (A5), or any member that
  turns hostile (A6), can pair an attacker's device with itself — it controls
  its own pairing window — and then introduce that device to every other
  member. Each member would trust a key that no human on that member ever saw,
  for the person's whole group, and possibly with data the stolen phone was
  never granted. Under pairwise trust, the same attacker gains a trust
  relation with the stolen phone only.
* It also breaks invariants V1 relies on: every trust record backed by an
  out-of-band channel and a local human decision (ADR-0006, T3), and no
  network message creating trust (ADR-0022 §Security implications).
* **Rejected.**

### Introduction with confirmation on the receiving device

As above, but B shows a prompt such as "A introduces device D (fingerprint …).
Trust it?" and adds D only if its user accepts.

* **For:** closer to pairwise, because a human on B decides.
* **Against:** the fingerprint the human sees came from A over the network. It
  did not come from an out-of-band channel the human controls. A compromised A
  chooses what B displays, so the prompt becomes social engineering ("your new
  tablet, accept?") instead of a comparison. ADR-0006 already calls the prompt
  the weakest assumption in the model. Here it would be the only defence, and
  it would be fed by the attacker. It also lets a network message raise a
  prompt on another device, which ADR-0022 §D5 forbids as this ADR reads it
  (§D8). A stolen TV or phone could push such prompts to every member.
* **Rejected for this iteration.** It is the natural starting point for a
  future introduction ADR, bound by §D8.

### A shared group secret or group key

The Space holds a symmetric secret or key pair. Holding it proves membership.

* **Against:** it is a credential that every member holds, so leaking it from
  one device admits anyone, which is transitive trust by construction. Removing
  a member needs re-keying on every other member, and with no cloud, offline
  members miss the re-key. It would also need a software key that can be
  exported or copied between devices, but Android identity keys are
  non-exportable Keystore keys (ADR-0006).
* **Rejected.**

### A Space CA, or an owner device that signs members

One device, the "owner", certifies the others. Members trust anything it
signs.

* **Against:** ADR-0007 already rejected a local CA. It adds CA key management,
  revocation lists and expiry to solve a problem pinning solves directly. It
  makes one device a single point of compromise, which is transitive trust with
  a single root, and a hub, which #4 excludes. Losing the owner device leaves
  the group unable to add members.
* **Rejected.**

### Threshold or web-of-trust introductions (k of n members vouch)

* **Against:** for the handful of devices one person typically owns,
  a threshold either equals "all" (pairwise with extra steps) or is small
  enough for one stolen device plus one more to meet. It adds the most
  machinery and the hardest UX for the least gain.
* **Rejected.**

### Pairwise trust with the Space as a view (chosen)

* **For:** it leaves V1's security model untouched. Every trust record is still
  backed by a local window, an out-of-band token and a human confirmation. A
  compromised member's reach is limited to its own pairings. There is no new
  wire message, no new key and no new trust anchor. Revocation keeps its
  current meaning. V1 peers take part without change.
* **Against:** N−1 pairings for the Nth device, and views that differ until the
  person completes them (§D4, §D5). Both are visible, and neither can widen
  trust.

---

## Compatibility

* **ADR-0006 / ADR-0007.** Unchanged. Identity, fingerprint, pinning,
  authorization (a trust-store hit, or a valid proof inside a locally opened
  window) and the trust store's format are what the Space is computed from. A
  Space is a query over that store. Nothing is added to it as trust.
* **ADR-0022.** Unchanged, and relied upon: symmetric pairing between any two
  device kinds is what makes pairwise trust practical for a phone, a tablet
  and a TV. Pairing windows open only locally (§D5 there), confirmations are
  mandatory as accepted, and there is at most one control session per pair
  (§D4 there). This ADR adds no exception to any of them.
* **V1 peers.** A V1 desktop or phone is a member of a V2 device's Space like
  any other peer, through its existing pairing. It needs to know nothing about
  Spaces, because no Space is ever on the wire.
* **Wire impact: none.** This ADR adds no message, field, ALPN or protocol
  version. A future member-view report (§D5) or propagated revocation (§D7)
  would each need their own SPEC, gated on the protocol version as ADR-0022
  does for its additions.
* **Stored state: none new as trust.** The existing records, tombstones and
  grants are enough. A UI may cache display state, but never in a way that
  feeds authorization.

---

## Consequences

* **Joining costs one pairing per device the new device should reach.** That
  is 3, 6 or 10 pairings for a whole group of 3, 4 or 5 devices. ADR-0022's QR
  and text-code modes keep each one short. A guided flow can sequence them
  (§D4). Friction measured here is the evidence a future introduction ADR
  would need.
* **"Remove everywhere" is a walk over members, not a single act.** #19 owns
  making that walk visible and complete.
* **The UI must show partial states honestly**: a device trusted here but not
  there, a peer that rejects us, a revocation not yet done on an offline
  member. Hiding them would recreate, as UX, the false belief that the group
  is one trust domain.
* **#7, #8, #18, #19 and #20 build on a per-device view.** Anything that wants
  a group-wide fact has to gather it from members as claims (§D5), or decide
  per device.
* **Trust does not imply data flow.** Being in a Space gives no peer a path to
  another peer's data. Today that holds because no capability relays:
  `clipboard.v1` enforces it (T23, `CLIP-SEC-09`). This ADR does not add a rule
  to ADR-0008. It recommends that every capability proposed for a multi-device
  group, and especially for a shared device, states whether it can re-emit one
  peer's data to another (§Questions for the owner, 5).
* `docs/security/THREAT_MODEL.md` changes **with the implementation of #6**,
  not with this ADR, following ADR-0022's practice. The entries are listed
  below.

## Security implications

* **Blast radius of one member is its own pairings.** A stolen, compromised or
  hostile member (A5, A6) can use the capabilities it was granted, on the
  members that trust it, until each one revokes it (T5, T13). It can pair new
  devices with itself, if it controls its own window. It **cannot** add a
  device to any other member, remove one, or change another member's grants.
  This is the property the decision exists to keep, and the reason pairwise is
  preferred over transitive trust.
* **Stolen member.** The thief holds the device, its identity key and its own
  trust store, and can reach every member that still trusts the device. What
  that means depends on how the key is held (ADR-0006):
  * **Android:** the key is non-exportable in the Keystore. The thief can
    impersonate the device only by using the device itself.
  * **Linux desktop:** the key is a software PKCS#8 file protected by file
    modes. A thief or malware running as the user can copy it and then
    impersonate the device **from any host, to every member that trusts it**,
    for as long as any of them does. Destroying the original changes nothing
    (§D6). TPM2 sealing, ADR-0006's top security debt, is what would move this
    line. The group makes the debt worse: one copied key is now valid on N
    members, not one.

  Recovery is revocation on each member (§D6). Until that is done, exposure is
  per member and visible per member. #19 shortens it, and §D7 bounds how.
* **Compromised member lying.** It can misreport its observed state, its name
  (sanitized, T17) and, if §D5 reports are ever specified, its view of the
  group. None of these is an input to trust.
* **Shared device (Android TV).** A TV is used by everyone in the room (A5),
  and any of them can open its pairing window with the remote. Pairing their
  own phone with the TV gives them a relation with the TV only. It gives them
  nothing on Alice's or Bob's phones, and nothing that Alice's or Bob's phones
  share with each other. The confirmations the negotiated protocol version
  requires (ADR-0022 §D5) still apply. On a shared TV, though, the person
  confirming may be the guest holding the remote, so the confirmation protects
  the guest's own pairing and nothing else. Containment comes from trust being
  pairwise and from per-peer grants. What the TV shows, such as notifications
  mirrored to a screen everyone can see, is a capability and grant question,
  not a trust one. Under transitive trust, a TV would amplify introductions.
  Under pairwise trust, it cannot.
* **Discovery and proximity.** Seeing a member's DNS-SD record, sharing its
  network, or using its name or device id confers nothing (T2, ADR-0005).
* **No new anchor, no new secret.** No group key, CA or Space credential exists
  to steal, rotate or leak. Every trust record is backed by a local window, an
  out-of-band token and a local human confirmation, as in V1.
* **No logging change.** Anything logged about a Space follows ADR-0022's rule
  (§Security implications there) and T11: short fingerprints, outcomes and
  sanitized names, never a token or user content.

**Threat-model entries to add with the implementation:**

* T13 and T14 generalised: a lost device is trusted by several members, and
  revocation is complete only when it is done on each one;
* T15 generalised: a copied desktop identity key impersonates that desktop to
  every member that trusts it, not to one phone;
* a member trusted by some members and not others, and the UI's duty to show
  it (§D5);
* a shared, physically exposed member such as a TV (A5 against a member, not
  only against a pairing);
* if specified later: lying member-view reports (§D5) and propagated
  revocation as a denial of service (§D7).

---

## Questions for the owner

These are the points this proposal leaves to the owner at acceptance. None of
them changes §D1: trust stays pairwise either way.

1. **Member-view reports.** Should a later SPEC let members exchange the claims
   described in §D5, so a device can show what the other members say they
   trust? #19's "where is it still trusted" view depends on this. Without it,
   the person checks each device. This ADR fixes the constraints, not the
   mechanism.
2. **Leaving and handing on a device.** Should "leave the Space" offer to
   destroy and regenerate the device's identity key (§D6), and should handing
   a device on require it?
3. **Shared-device defaults.** Should a device that knows locally that it is a
   TV start new pairings with fewer default grants? Its own kind is local
   knowledge. ADR-0022 forbids acting on a *peer's* claimed kind, and this
   question does not.
4. **One group per device.** #6 suggested that a device belongs to one group
   until a use case justifies more. This ADR replaces that default (§D4): with
   Spaces as per-device views, overlap needs no rule. Does the owner accept
   the replacement?
5. **Data flow across pairs.** Should "no capability re-emits one peer's data
   to another" become a rule of the capability architecture, through an
   amendment to ADR-0008? This ADR only recommends it (§Consequences).

## Notes

* Umbrella: #4. Part of #6, and required before #6 is implemented and before
  work that depends on the group or trust model: #7, #8, #18, #19 and #20.
* Implementing #6 under this ADR needs no protocol SPEC, because no wire
  change is made. It does need the UI and trust-store behaviour of §D2, §D3
  and §D6 specified for both platforms. Any member-view report (§D5) or
  propagated revocation (§D7) needs its own SPEC first, and propagated
  revocation needs its own ADR.
