//! Duplicate control-session resolution: which of two sessions survives.
//!
//! ADR-0022 §D4 keeps at most one authenticated control session per pair of
//! identities; `docs/architecture/MULTI-DEVICE-MESH-V2.md` §4 fixes the
//! constants. This module is the decision and nothing else:
//!
//! ```text
//! probe the older session (PING), wait up to PROBE_TIMEOUT
//!   no answer                    -> stale: close the older, keep the newer
//!   answer, different dialers    -> keep the session whose dialer has the
//!                                   lower raw SPKI fingerprint
//!   answer, same dialer          -> the dialer closes the older (the one it
//!                                   abandoned); the listener closes neither,
//!                                   and at the deadline keeps the newer
//! ```
//!
//! plus the churn bound: more than [`CHURN_LIMIT`] supersessions for one peer
//! within [`CHURN_WINDOW`] refuses that peer's new sessions for
//! [`CHURN_REFUSAL`].
//!
//! # Pure and dormant
//!
//! Nothing here does I/O. It sends no `PING`, closes no socket, sends no
//! close reason and reads no clock: every instant is a [`MonotonicTime`]
//! the caller supplies, so every outcome is a function of its inputs and the
//! tests can sit exactly on a boundary. It logs nothing either; a caller that
//! acts on a [`ChurnVerdict::Refusing`] logs the short fingerprint only
//! (ADR-0022 §D4, threat T11).
//!
//! No production session path calls this module yet (GitHub #102). Activating
//! it, together with protocol version 2, is a later and separately reviewed
//! slice.
//!
//! # What the order decides
//!
//! The fingerprint order schedules dials and settles duplicates (ADR-0022
//! §D3). It carries no authority: both sessions reaching this module are
//! already authenticated, and the policy only chooses between them.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::fingerprint::Fingerprint;

/// How long the older session has to answer the probe (ADR-0022 §D4 step 1).
/// The same bound ends the listener's wait in the same-dialer case.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Supersessions for one peer tolerated within [`CHURN_WINDOW`]. One more
/// trips the refusal.
pub const CHURN_LIMIT: usize = 5;

/// The sliding window the churn bound counts supersessions in.
pub const CHURN_WINDOW: Duration = Duration::from_secs(60);

/// How long a peer that tripped the churn bound has its new sessions refused.
pub const CHURN_REFUSAL: Duration = Duration::from_secs(60);

/// A reading of a monotonic clock, as a distance from an origin the caller
/// chooses and keeps for the lifetime of the values it compares.
///
/// The policy never reads a clock. Only the order of two readings and the
/// distance between them mean anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicTime(Duration);

impl MonotonicTime {
    pub const fn from_since_origin(since_origin: Duration) -> Self {
        Self(since_origin)
    }

    pub const fn since_origin(self) -> Duration {
        self.0
    }

    /// `self + d`, saturating rather than overflowing: a deadline beyond the
    /// representable range is one that never arrives.
    pub fn saturating_add(self, d: Duration) -> Self {
        Self(self.0.saturating_add(d))
    }

    /// `self - earlier`, or zero if `earlier` is not earlier.
    pub fn saturating_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

/// Which device dialled a control session, as this device sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialer {
    /// This device was the TLS client.
    Local,
    /// The peer was the TLS client.
    Peer,
}

/// One of the two sessions of a duplicate, named by the order in which this
/// device saw them established. The peer may have seen them the other way
/// round; nothing in the rule needs the two orders to agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Session {
    Older,
    Newer,
}

impl Session {
    pub fn other(self) -> Self {
        match self {
            Self::Older => Self::Newer,
            Self::Newer => Self::Older,
        }
    }
}

/// Raw SPKI fingerprints compared as unsigned bytes, lexicographically
/// (ADR-0022 §D3, MULTI-DEVICE-MESH-V2.md §4). True when `a` sorts first.
///
/// Spelled out over the bytes rather than left to `Fingerprint`'s derived
/// `Ord`: this order is a wire-level agreement with every other
/// implementation, not an incidental property of a Rust type.
pub fn is_lower(a: &Fingerprint, b: &Fingerprint) -> bool {
    a.as_bytes()[..] < b.as_bytes()[..]
}

/// Why a duplicate cannot be resolved at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DuplicateError {
    /// The peer presents this device's own SPKI. That is a bug or a
    /// reflection, never a peer: ADR-0022 §D2 closes it whichever side
    /// detects it, so there is no duplicate to resolve.
    #[error("peer fingerprint equals the local fingerprint")]
    SelfConnection,
}

/// What the caller does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing is decided. Send no capability message to this peer on either
    /// session (ADR-0022 §D4), keep receiving on both, and call
    /// [`DuplicateResolver::poll`] at `until` at the latest.
    Wait { until: MonotonicTime },
    /// Close `close`. The other session survives.
    Close { close: Session },
    /// One session closed without this device closing it. The named one is
    /// the survivor and there is nothing left to close.
    Survivor(Session),
}

impl Step {
    /// The session that survives, once one is decided.
    pub fn survivor(self) -> Option<Session> {
        match self {
            Self::Wait { .. } => None,
            Self::Close { close } => Some(close.other()),
            Self::Survivor(s) => Some(s),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The probe is outstanding on the older session.
    Probing,
    /// Same dialer, and the peer dialled both: waiting for it to close the
    /// session it abandoned.
    AwaitingDialer,
    /// Decided. Sticky: later calls return the same step.
    Done(Step),
}

/// One duplicate, from the moment its probe was sent until a survivor is
/// decided.
///
/// The caller detects the duplicate (a second authenticated control session
/// for a peer fingerprint that already has one; data streams never count),
/// sends the probe on the older session, and starts a resolver with the
/// instant it sent it. It then feeds the resolver what happens: the probe's
/// answer, a session closing under it, and the passing of time. Every call
/// returns the [`Step`] to take.
///
/// Once a step other than [`Step::Wait`] is returned, the resolver is done
/// and returns that step again whatever it is told. A survivor closing later
/// is an ordinary disconnect, not part of this resolution.
#[derive(Debug, Clone)]
pub struct DuplicateResolver {
    local: Fingerprint,
    peer: Fingerprint,
    older_dialer: Dialer,
    newer_dialer: Dialer,
    deadline: MonotonicTime,
    phase: Phase,
}

impl DuplicateResolver {
    /// `older_dialer` and `newer_dialer` say who dialled each session, in the
    /// order this device saw them established. `probe_sent_at` is when the
    /// probe went out on the older session.
    pub fn new(
        local: Fingerprint,
        peer: Fingerprint,
        older_dialer: Dialer,
        newer_dialer: Dialer,
        probe_sent_at: MonotonicTime,
    ) -> Result<Self, DuplicateError> {
        if local == peer {
            return Err(DuplicateError::SelfConnection);
        }
        Ok(Self {
            local,
            peer,
            older_dialer,
            newer_dialer,
            deadline: probe_sent_at.saturating_add(PROBE_TIMEOUT),
            phase: Phase::Probing,
        })
    }

    /// When the probe times out and, in the same-dialer case, the listener
    /// stops waiting for the dialer.
    pub fn deadline(&self) -> MonotonicTime {
        self.deadline
    }

    /// The step currently in force, without feeding anything in.
    pub fn step(&self) -> Step {
        match self.phase {
            Phase::Probing | Phase::AwaitingDialer => Step::Wait {
                until: self.deadline,
            },
            Phase::Done(step) => step,
        }
    }

    /// The older session answered the probe at `now`.
    ///
    /// An answer at or after the deadline is too late: the probe has already
    /// timed out and the older session is stale, whether or not
    /// [`poll`](Self::poll) was called in between.
    pub fn probe_answered(&mut self, now: MonotonicTime) -> Step {
        if self.phase != Phase::Probing {
            return self.poll(now);
        }
        if now >= self.deadline {
            return self.poll(now);
        }
        self.phase = match (self.older_dialer, self.newer_dialer) {
            // A simultaneous open: the lower fingerprint's dial wins. Both
            // devices know both fingerprints and both dialers, so both reach
            // the same session without exchanging anything.
            (older, newer) if older != newer => {
                let keep = if self.dialer_is_lower(older) {
                    Session::Older
                } else {
                    Session::Newer
                };
                Phase::Done(Step::Close {
                    close: keep.other(),
                })
            }
            // We dialled both. A correct dialer opens a second session only
            // after giving up on the first, so the one we abandoned is the
            // older, and we close it. "Older" stands for "abandoned" here by
            // that argument: the policy is not told which one was given up.
            (Dialer::Local, _) => Phase::Done(Step::Close {
                close: Session::Older,
            }),
            // The peer dialled both. We close neither on our own account
            // until the deadline.
            (Dialer::Peer, _) => Phase::AwaitingDialer,
        };
        self.poll(now)
    }

    /// `which` closed at `now` without this device closing it: the peer
    /// closed it, or the network did.
    ///
    /// A close at or after the deadline comes after the decision the
    /// deadline forced, whether or not [`poll`](Self::poll) was called in
    /// between: it cannot rescue an older session that never answered.
    pub fn session_ended(&mut self, which: Session, now: MonotonicTime) -> Step {
        self.poll(now);
        if let Phase::Probing | Phase::AwaitingDialer = self.phase {
            self.phase = Phase::Done(Step::Survivor(which.other()));
        }
        self.step()
    }

    /// Time has reached `now`. Call it at [`Step::Wait`]'s `until`.
    pub fn poll(&mut self, now: MonotonicTime) -> Step {
        if now >= self.deadline {
            match self.phase {
                // No answer within the bound: the older session is stale,
                // most likely a half-open socket the peer has abandoned.
                Phase::Probing => {
                    self.phase = Phase::Done(Step::Close {
                        close: Session::Older,
                    });
                }
                // The dialer has not closed one in time: keep the session
                // accepted last. The peer dialled both, so both were accepted
                // here, and the one accepted last is the newer.
                Phase::AwaitingDialer => {
                    self.phase = Phase::Done(Step::Close {
                        close: Session::Older,
                    });
                }
                Phase::Done(_) => {}
            }
        }
        self.step()
    }

    fn dialer_is_lower(&self, dialer: Dialer) -> bool {
        match dialer {
            Dialer::Local => is_lower(&self.local, &self.peer),
            Dialer::Peer => is_lower(&self.peer, &self.local),
        }
    }
}

/// Whether a new session from a peer may proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admit,
    /// Refused because the peer tripped the churn bound. Admitted again from
    /// `until` on.
    Refuse {
        until: MonotonicTime,
    },
}

/// What recording a supersession did to the peer's churn state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChurnVerdict {
    /// Counted; still within the bound.
    Counted,
    /// This supersession tripped the bound. New sessions from the peer are
    /// refused until `until`. The caller logs the event with the short
    /// fingerprint only.
    Refusing { until: MonotonicTime },
}

#[derive(Debug, Default, Clone)]
struct PeerChurn {
    /// Instants of the supersessions still inside the window, oldest first.
    /// Never more than `CHURN_LIMIT` long: one more trips the refusal, which
    /// clears it.
    recent: VecDeque<MonotonicTime>,
    refused_until: Option<MonotonicTime>,
    /// The latest instant recorded, kept across a refusal clearing `recent`
    /// so an instant from the past is still clamped forward.
    latest: Option<MonotonicTime>,
}

impl PeerChurn {
    fn forget_expired(&mut self, now: MonotonicTime) {
        while let Some(&oldest) = self.recent.front() {
            if now.saturating_since(oldest) >= CHURN_WINDOW {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        if self.refused_until.is_some_and(|until| now >= until) {
            self.refused_until = None;
        }
    }

    fn is_idle(&self) -> bool {
        self.recent.is_empty() && self.refused_until.is_none()
    }
}

/// The per-peer churn bound of ADR-0022 §D4.
///
/// A supersession at `t` counts while `now - t < CHURN_WINDOW`. Recording the
/// `CHURN_LIMIT + 1`-th one that counts refuses the peer's new sessions for
/// `now < t + CHURN_REFUSAL`, and starts the count afresh. A supersession
/// recorded while a refusal is in force counts too, and tripping the bound
/// again moves the end of the refusal later. Instants must be fed in
/// non-decreasing order per peer; an earlier one is treated as happening at
/// the latest instant seen while the peer holds state.
///
/// Per peer, at most `CHURN_LIMIT` instants are held. [`prune`](Self::prune)
/// drops every peer whose supersessions have expired and whose refusal has
/// ended, so the map is bounded by the peers that churned recently.
#[derive(Debug, Default, Clone)]
pub struct ChurnLimiter {
    peers: HashMap<Fingerprint, PeerChurn>,
}

impl ChurnLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// May a new session from `peer` proceed at `now`?
    pub fn admission(&self, peer: &Fingerprint, now: MonotonicTime) -> Admission {
        match self.peers.get(peer).and_then(|p| p.refused_until) {
            Some(until) if now < until => Admission::Refuse { until },
            _ => Admission::Admit,
        }
    }

    /// One of `peer`'s sessions was superseded at `now`.
    pub fn record_supersession(&mut self, peer: Fingerprint, now: MonotonicTime) -> ChurnVerdict {
        let entry = self.peers.entry(peer).or_default();
        let now = entry.latest.map_or(now, |latest| now.max(latest));
        entry.latest = Some(now);
        entry.forget_expired(now);
        entry.recent.push_back(now);
        if entry.recent.len() > CHURN_LIMIT {
            let until = now.saturating_add(CHURN_REFUSAL);
            entry.recent.clear();
            entry.refused_until = Some(until);
            ChurnVerdict::Refusing { until }
        } else {
            ChurnVerdict::Counted
        }
    }

    /// Drops every peer whose state no longer affects any answer at `now`.
    pub fn prune(&mut self, now: MonotonicTime) {
        self.peers.retain(|_, p| {
            p.forget_expired(now);
            !p.is_idle()
        });
    }

    /// Forgets `peer` entirely, for example when it is revoked.
    pub fn forget(&mut self, peer: &Fingerprint) {
        self.peers.remove(peer);
    }

    /// Peers currently holding state.
    pub fn tracked_peers(&self) -> usize {
        self.peers.len()
    }
}
