//! Duplicate control-session resolution (ADR-0022 §D4, GitHub #102).
//!
//! The policy is pure, so every case here is exact: instants are built from
//! durations, and the boundaries are tested on, just before and just after
//! the line. The convergence section runs both devices' resolvers against
//! each other over every combination of fingerprint order, dialers, each
//! side's view of which session is older, each side's probe outcome and
//! whether a close reaches the other side in time, and checks the one
//! property §D4 exists for: two sessions never both survive.
//!
//! That enumeration covers inputs, not interleavings: answers come at 1 s,
//! closes at 2 s and the deadline at 3 s. Other orderings — a close before
//! this side's answer, a close after the deadline — are covered one device
//! at a time in the resolver section.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pliwee_core::duplicate_session::{
    is_lower, Admission, ChurnLimiter, ChurnVerdict, Dialer, DuplicateError, DuplicateResolver,
    MonotonicTime, Session, Step, CHURN_LIMIT, CHURN_REFUSAL, CHURN_WINDOW, PROBE_TIMEOUT,
};
use pliwee_core::Fingerprint;

fn fp(bytes: &[(usize, u8)]) -> Fingerprint {
    let mut raw = [0u8; 32];
    for &(i, b) in bytes {
        raw[i] = b;
    }
    Fingerprint::from_hex(&data_encoding::HEXLOWER.encode(&raw)).expect("32 bytes of hex")
}

fn at(ms: u64) -> MonotonicTime {
    MonotonicTime::from_since_origin(Duration::from_millis(ms))
}

// ---------------------------------------------------------------------------
// Constants: the owner-accepted defaults (ADR-0022, MESH-V2 §4)
// ---------------------------------------------------------------------------

#[test]
fn constants_are_the_accepted_defaults() {
    assert_eq!(PROBE_TIMEOUT, Duration::from_secs(3));
    assert_eq!(CHURN_LIMIT, 5);
    assert_eq!(CHURN_WINDOW, Duration::from_secs(60));
    assert_eq!(CHURN_REFUSAL, Duration::from_secs(60));
}

// ---------------------------------------------------------------------------
// Fingerprint order: raw unsigned bytes, lexicographic
// ---------------------------------------------------------------------------

#[test]
fn fingerprints_compare_as_unsigned_bytes() {
    // 0x80 is negative as an i8. A signed comparison would put it first.
    let low = fp(&[(0, 0x7f)]);
    let high = fp(&[(0, 0x80)]);
    assert!(is_lower(&low, &high));
    assert!(!is_lower(&high, &low));
    let ff = fp(&[(0, 0xff)]);
    assert!(is_lower(&high, &ff));
}

#[test]
fn fingerprints_compare_lexicographically_not_by_magnitude_or_sum() {
    // The first differing byte decides, however large the later ones are.
    let a = fp(&[(0, 0x01)]);
    let b = fp(&[(0, 0x00), (1, 0xff), (31, 0xff)]);
    assert!(is_lower(&b, &a));
    // Equal up to the last byte.
    let c = fp(&[(0, 0xaa), (31, 0x01)]);
    let d = fp(&[(0, 0xaa), (31, 0x02)]);
    assert!(is_lower(&c, &d));
    assert!(!is_lower(&d, &c));
    // Irreflexive.
    assert!(!is_lower(&c, &c));
}

// ---------------------------------------------------------------------------
// One device's resolver
// ---------------------------------------------------------------------------

fn resolver(
    local: Fingerprint,
    peer: Fingerprint,
    older: Dialer,
    newer: Dialer,
) -> DuplicateResolver {
    DuplicateResolver::new(local, peer, older, newer, at(10_000)).expect("distinct identities")
}

const ALL_DIALERS: [Dialer; 2] = [Dialer::Local, Dialer::Peer];

#[test]
fn a_session_with_our_own_fingerprint_is_not_a_duplicate() {
    let me = fp(&[(0, 0x42)]);
    assert_eq!(
        DuplicateResolver::new(me, me, Dialer::Local, Dialer::Peer, at(0)).err(),
        Some(DuplicateError::SelfConnection)
    );
}

#[test]
fn nothing_is_decided_before_the_probe_answers_or_times_out() {
    let mut r = resolver(fp(&[(0, 1)]), fp(&[(0, 2)]), Dialer::Local, Dialer::Peer);
    assert_eq!(r.deadline(), at(13_000));
    let wait = Step::Wait { until: at(13_000) };
    assert_eq!(r.step(), wait);
    assert_eq!(r.poll(at(10_000)), wait);
    assert_eq!(r.poll(at(12_999)), wait);
    assert_eq!(wait.survivor(), None);
}

#[test]
fn a_stale_older_session_always_yields_the_newer() {
    for (local, peer) in [
        (fp(&[(0, 1)]), fp(&[(0, 2)])),
        (fp(&[(0, 2)]), fp(&[(0, 1)])),
    ] {
        for older in ALL_DIALERS {
            for newer in ALL_DIALERS {
                // Timed out exactly at the deadline.
                let mut r = resolver(local, peer, older, newer);
                assert_eq!(
                    r.poll(at(13_000)),
                    Step::Close {
                        close: Session::Older
                    }
                );
                assert_eq!(r.step().survivor(), Some(Session::Newer));

                // Polled late.
                let mut r = resolver(local, peer, older, newer);
                assert_eq!(
                    r.poll(at(99_000)),
                    Step::Close {
                        close: Session::Older
                    }
                );

                // An answer at the deadline is too late, polled or not.
                let mut r = resolver(local, peer, older, newer);
                assert_eq!(
                    r.probe_answered(at(13_000)),
                    Step::Close {
                        close: Session::Older
                    }
                );

                // And a late answer after the timeout fired changes nothing.
                let mut r = resolver(local, peer, older, newer);
                r.poll(at(13_000));
                assert_eq!(
                    r.probe_answered(at(13_001)),
                    Step::Close {
                        close: Session::Older
                    }
                );
            }
        }
    }
}

#[test]
fn an_answer_one_millisecond_before_the_deadline_counts() {
    // We are lower and dialled the older session, so a counted answer keeps
    // the older; a stale verdict would have kept the newer. The two
    // outcomes differ, so the assertion tells which one happened.
    let local = fp(&[(0, 1)]);
    let peer = fp(&[(0, 2)]);
    let mut r = resolver(local, peer, Dialer::Local, Dialer::Peer);
    let step = r.probe_answered(at(12_999));
    assert_eq!(
        step,
        Step::Close {
            close: Session::Newer
        }
    );
}

#[test]
fn simultaneous_open_keeps_the_lower_fingerprint_dialers_session() {
    let low = fp(&[(0, 0x7f)]);
    let high = fp(&[(0, 0x80)]);

    // We are lower: our dial wins, wherever it falls in our order.
    let mut r = resolver(low, high, Dialer::Local, Dialer::Peer);
    assert_eq!(
        r.probe_answered(at(10_500)),
        Step::Close {
            close: Session::Newer
        }
    );
    let mut r = resolver(low, high, Dialer::Peer, Dialer::Local);
    assert_eq!(
        r.probe_answered(at(10_500)),
        Step::Close {
            close: Session::Older
        }
    );

    // We are higher: the peer's dial wins.
    let mut r = resolver(high, low, Dialer::Local, Dialer::Peer);
    assert_eq!(
        r.probe_answered(at(10_500)),
        Step::Close {
            close: Session::Older
        }
    );
    let mut r = resolver(high, low, Dialer::Peer, Dialer::Local);
    assert_eq!(
        r.probe_answered(at(10_500)),
        Step::Close {
            close: Session::Newer
        }
    );
}

#[test]
fn same_dialer_the_dialer_closes_the_session_it_abandoned() {
    for (local, peer) in [
        (fp(&[(0, 1)]), fp(&[(0, 2)])),
        (fp(&[(0, 2)]), fp(&[(0, 1)])),
    ] {
        let mut r = resolver(local, peer, Dialer::Local, Dialer::Local);
        assert_eq!(
            r.probe_answered(at(10_001)),
            Step::Close {
                close: Session::Older
            }
        );
    }
}

#[test]
fn same_dialer_the_listener_closes_neither_until_the_deadline() {
    for (local, peer) in [
        (fp(&[(0, 1)]), fp(&[(0, 2)])),
        (fp(&[(0, 2)]), fp(&[(0, 1)])),
    ] {
        let mut r = resolver(local, peer, Dialer::Peer, Dialer::Peer);
        let wait = Step::Wait { until: at(13_000) };
        assert_eq!(r.probe_answered(at(10_001)), wait);
        assert_eq!(r.poll(at(12_999)), wait);
        // The dialer did not close one in time: keep the one accepted last.
        assert_eq!(
            r.poll(at(13_000)),
            Step::Close {
                close: Session::Older
            }
        );
    }
}

#[test]
fn same_dialer_the_listener_follows_the_dialers_close() {
    for closed in [Session::Older, Session::Newer] {
        let mut r = resolver(fp(&[(0, 1)]), fp(&[(0, 2)]), Dialer::Peer, Dialer::Peer);
        r.probe_answered(at(10_001));
        assert_eq!(
            r.session_ended(closed, at(12_999)),
            Step::Survivor(closed.other())
        );
        // Sticky: the deadline passing does not close the survivor.
        assert_eq!(r.poll(at(13_000)), Step::Survivor(closed.other()));
    }
}

#[test]
fn a_session_closing_during_the_probe_leaves_the_other() {
    for older in ALL_DIALERS {
        for newer in ALL_DIALERS {
            for closed in [Session::Older, Session::Newer] {
                let mut r = resolver(fp(&[(0, 1)]), fp(&[(0, 2)]), older, newer);
                assert_eq!(
                    r.session_ended(closed, at(11_000)),
                    Step::Survivor(closed.other())
                );
                assert_eq!(r.probe_answered(at(11_500)), Step::Survivor(closed.other()));
                assert_eq!(r.poll(at(20_000)), Step::Survivor(closed.other()));
            }
        }
    }
}

#[test]
fn a_close_at_or_after_the_deadline_cannot_rescue_a_stale_older_session() {
    // Regression: `session_ended` used to skip the deadline, so a missed
    // `poll` followed by the newer session ending kept the older one, which
    // never answered the probe.
    for when in [13_000, 13_500] {
        let mut r = resolver(fp(&[(0, 1)]), fp(&[(0, 2)]), Dialer::Peer, Dialer::Peer);
        assert_eq!(
            r.session_ended(Session::Newer, at(when)),
            Step::Close {
                close: Session::Older
            }
        );
    }
    // One millisecond earlier, the close is still in time.
    let mut r = resolver(fp(&[(0, 1)]), fp(&[(0, 2)]), Dialer::Peer, Dialer::Peer);
    assert_eq!(
        r.session_ended(Session::Newer, at(12_999)),
        Step::Survivor(Session::Older)
    );
}

#[test]
fn a_decision_is_sticky() {
    let mut r = resolver(fp(&[(0, 1)]), fp(&[(0, 2)]), Dialer::Local, Dialer::Peer);
    let decided = r.probe_answered(at(10_500));
    assert_eq!(
        decided,
        Step::Close {
            close: Session::Newer
        }
    );
    // The session we closed closing is what we asked for; the survivor
    // closing later is an ordinary disconnect, not this resolution's.
    assert_eq!(r.session_ended(Session::Newer, at(10_600)), decided);
    assert_eq!(r.session_ended(Session::Older, at(10_700)), decided);
    assert_eq!(r.probe_answered(at(10_800)), decided);
    assert_eq!(r.poll(at(99_000)), decided);
}

#[test]
fn a_deadline_beyond_the_clock_saturates_instead_of_panicking() {
    let end = MonotonicTime::from_since_origin(Duration::MAX);
    let mut r = DuplicateResolver::new(
        fp(&[(0, 1)]),
        fp(&[(0, 2)]),
        Dialer::Local,
        Dialer::Peer,
        end,
    )
    .expect("distinct identities");
    assert_eq!(r.deadline(), end);
    assert_eq!(
        r.poll(end),
        Step::Close {
            close: Session::Older
        }
    );
}

// ---------------------------------------------------------------------------
// Both perspectives converge
// ---------------------------------------------------------------------------

/// Device indices.
const A: usize = 0;
const B: usize = 1;

#[derive(Debug, Clone, Copy)]
struct Scenario {
    fps: [Fingerprint; 2],
    /// Physical dialer (A or B) of physical session 0 and 1.
    dialer_of: [usize; 2],
    /// Per device: which physical session it saw established first.
    older_seen: [usize; 2],
    /// Per device: did its probe on its older session get an answer in time?
    answered: [bool; 2],
    /// Does a close by one device reach the other before its deadline?
    close_delivered: bool,
}

#[derive(Debug)]
struct Outcome {
    /// Physical sessions closed by each device's own decision.
    closed_by: [Option<usize>; 2],
    /// Each device's view of the survivor, as a physical session.
    believed: [Option<usize>; 2],
}

impl Outcome {
    fn survivors(&self) -> Vec<usize> {
        (0..2)
            .filter(|s| !self.closed_by.contains(&Some(*s)))
            .collect()
    }
}

fn run(s: &Scenario) -> Outcome {
    let view = |dev: usize, session: Session| match session {
        Session::Older => s.older_seen[dev],
        Session::Newer => 1 - s.older_seen[dev],
    };
    let local_view = |dev: usize, physical: usize| {
        if s.older_seen[dev] == physical {
            Session::Older
        } else {
            Session::Newer
        }
    };
    let dialer = |dev: usize, physical: usize| {
        if s.dialer_of[physical] == dev {
            Dialer::Local
        } else {
            Dialer::Peer
        }
    };

    let mut rs: Vec<DuplicateResolver> = (0..2)
        .map(|dev| {
            DuplicateResolver::new(
                s.fps[dev],
                s.fps[1 - dev],
                dialer(dev, view(dev, Session::Older)),
                dialer(dev, view(dev, Session::Newer)),
                at(0),
            )
            .expect("distinct identities")
        })
        .collect();

    // t = 1 s: the probes that are answered are answered.
    let mut steps = [rs[A].step(), rs[B].step()];
    for dev in [A, B] {
        if s.answered[dev] {
            steps[dev] = rs[dev].probe_answered(at(1_000));
        }
    }
    // t = 2 s: each close reaches the other side, if it does in time.
    let mut closed_by = [None, None];
    for dev in [A, B] {
        if let Step::Close { close } = steps[dev] {
            closed_by[dev] = Some(view(dev, close));
        }
    }
    if s.close_delivered {
        for dev in [A, B] {
            if let Some(physical) = closed_by[1 - dev] {
                steps[dev] = rs[dev].session_ended(local_view(dev, physical), at(2_000));
            }
        }
    }
    // t = 3 s: the deadline.
    for dev in [A, B] {
        steps[dev] = rs[dev].poll(at(3_000));
        if let Step::Close { close } = steps[dev] {
            closed_by[dev] = Some(view(dev, close));
        }
    }
    // Late deliveries change nothing that is decided.
    for dev in [A, B] {
        if let Some(physical) = closed_by[1 - dev] {
            assert_eq!(
                rs[dev].session_ended(local_view(dev, physical), at(4_000)),
                steps[dev]
            );
        }
        assert!(
            !matches!(steps[dev], Step::Wait { .. }),
            "{s:?}: undecided after the deadline"
        );
    }
    Outcome {
        closed_by,
        believed: [
            steps[A].survivor().map(|x| view(A, x)),
            steps[B].survivor().map(|x| view(B, x)),
        ],
    }
}

fn every_scenario() -> Vec<Scenario> {
    let pairs = [
        [fp(&[(0, 0x00)]), fp(&[(0, 0xff)])],
        [fp(&[(0, 0x80)]), fp(&[(0, 0x7f)])],
        [fp(&[(0, 0xaa), (31, 0x01)]), fp(&[(0, 0xaa), (31, 0x02)])],
        [fp(&[(0, 0x00), (1, 0xff)]), fp(&[(0, 0x01)])],
    ];
    let mut out = Vec::new();
    for fps in pairs {
        for fps in [fps, [fps[1], fps[0]]] {
            for dialer_of in [[A, A], [A, B], [B, A], [B, B]] {
                for older_a in [0, 1] {
                    for older_b in [0, 1] {
                        for answered in [[true, true], [true, false], [false, true], [false, false]]
                        {
                            for close_delivered in [true, false] {
                                out.push(Scenario {
                                    fps,
                                    dialer_of,
                                    older_seen: [older_a, older_b],
                                    answered,
                                    close_delivered,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn two_sessions_never_both_survive() {
    let all = every_scenario();
    assert_eq!(
        all.len(),
        8 * 4 * 4 * 4 * 2,
        "the enumeration is exhaustive"
    );
    for s in &all {
        let o = run(s);
        let survivors = o.survivors();
        assert!(survivors.len() <= 1, "{s:?} -> {o:?}");
        // Whoever believes in a survivor believes in the one that survived.
        for dev in [A, B] {
            if let Some(b) = o.believed[dev] {
                if !survivors.is_empty() {
                    assert_eq!(survivors, vec![b], "{s:?} -> {o:?}");
                }
            }
        }
    }
}

#[test]
fn simultaneous_opens_converge_to_the_lower_fingerprint_dialers_session() {
    let mut checked = 0;
    for s in every_scenario() {
        if s.dialer_of[0] == s.dialer_of[1] || s.answered != [true, true] {
            continue;
        }
        let lower = if is_lower(&s.fps[A], &s.fps[B]) { A } else { B };
        let expected = s
            .dialer_of
            .iter()
            .position(|&d| d == lower)
            .expect("one each");
        let o = run(&s);
        assert_eq!(o.survivors(), vec![expected], "{s:?} -> {o:?}");
        assert_eq!(o.believed, [Some(expected), Some(expected)], "{s:?}");
        checked += 1;
    }
    assert_eq!(checked, 8 * 2 * 4 * 2);
}

#[test]
fn same_dialer_converges_to_one_session_when_the_close_arrives() {
    let mut checked = 0;
    for s in every_scenario() {
        if s.dialer_of[0] != s.dialer_of[1] || s.answered != [true, true] || !s.close_delivered {
            continue;
        }
        let dialer = s.dialer_of[0];
        let o = run(&s);
        // The dialer abandoned the session it saw first.
        let abandoned = s.older_seen[dialer];
        assert_eq!(o.closed_by[dialer], Some(abandoned), "{s:?}");
        assert_eq!(o.closed_by[1 - dialer], None, "listener closed one: {s:?}");
        assert_eq!(o.survivors(), vec![1 - abandoned], "{s:?} -> {o:?}");
        assert_eq!(o.believed, [Some(1 - abandoned); 2], "{s:?}");
        checked += 1;
    }
    assert_eq!(checked, 8 * 2 * 4);
}

#[test]
fn both_stale_with_agreeing_order_keeps_the_newer() {
    let mut checked = 0;
    for s in every_scenario() {
        if s.answered != [false, false] || s.older_seen[A] != s.older_seen[B] {
            continue;
        }
        let o = run(&s);
        assert_eq!(o.survivors(), vec![1 - s.older_seen[A]], "{s:?} -> {o:?}");
        checked += 1;
    }
    assert_eq!(checked, 8 * 4 * 2 * 2);
}

// ---------------------------------------------------------------------------
// Churn bound
// ---------------------------------------------------------------------------

fn s(secs: u64) -> MonotonicTime {
    at(secs * 1_000)
}

#[test]
fn five_supersessions_in_a_window_are_tolerated() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for t in 0..5 {
        assert_eq!(c.record_supersession(peer, s(t)), ChurnVerdict::Counted);
    }
    assert_eq!(c.admission(&peer, s(5)), Admission::Admit);
}

#[test]
fn the_sixth_within_sixty_seconds_refuses_for_sixty_seconds() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for t in [0, 10, 20, 30, 40] {
        c.record_supersession(peer, s(t));
    }
    // 59.999 s after the first: still inside the window.
    let sixth = at(59_999);
    let until = at(119_999);
    assert_eq!(
        c.record_supersession(peer, sixth),
        ChurnVerdict::Refusing { until }
    );
    assert_eq!(c.admission(&peer, sixth), Admission::Refuse { until });
    assert_eq!(c.admission(&peer, at(119_998)), Admission::Refuse { until });
    // The refusal ends exactly 60 s later.
    assert_eq!(c.admission(&peer, until), Admission::Admit);
}

#[test]
fn a_supersession_exactly_sixty_seconds_old_no_longer_counts() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for t in [0, 10, 20, 30, 40] {
        c.record_supersession(peer, s(t));
    }
    // The first has left the window: five count, not six.
    assert_eq!(c.record_supersession(peer, s(60)), ChurnVerdict::Counted);
    assert_eq!(c.admission(&peer, s(60)), Admission::Admit);
    // The next one, at 61 s, counts 10..61 = six.
    assert_eq!(
        c.record_supersession(peer, s(61)),
        ChurnVerdict::Refusing { until: s(121) }
    );
}

#[test]
fn six_at_the_same_instant_trip_the_bound() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for _ in 0..5 {
        assert_eq!(c.record_supersession(peer, s(7)), ChurnVerdict::Counted);
    }
    assert_eq!(
        c.record_supersession(peer, s(7)),
        ChurnVerdict::Refusing { until: s(67) }
    );
}

#[test]
fn the_count_restarts_after_a_refusal() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for _ in 0..6 {
        c.record_supersession(peer, s(0));
    }
    assert_eq!(c.admission(&peer, s(60)), Admission::Admit);
    for t in 60..65 {
        assert_eq!(c.record_supersession(peer, s(t)), ChurnVerdict::Counted);
    }
    assert_eq!(
        c.record_supersession(peer, s(65)),
        ChurnVerdict::Refusing { until: s(125) }
    );
}

#[test]
fn churn_is_counted_per_peer() {
    let noisy = fp(&[(0, 1)]);
    let quiet = fp(&[(0, 2)]);
    let mut c = ChurnLimiter::new();
    for _ in 0..6 {
        c.record_supersession(noisy, s(0));
    }
    c.record_supersession(quiet, s(0));
    assert!(matches!(
        c.admission(&noisy, s(1)),
        Admission::Refuse { .. }
    ));
    assert_eq!(c.admission(&quiet, s(1)), Admission::Admit);
    assert_eq!(c.admission(&fp(&[(0, 3)]), s(1)), Admission::Admit);
}

#[test]
fn an_instant_from_the_past_does_not_reopen_the_window() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for t in [50, 51, 52, 53, 54] {
        c.record_supersession(peer, s(t));
    }
    // Out of order: treated as happening at 54 s, so it is the sixth.
    assert_eq!(
        c.record_supersession(peer, s(0)),
        ChurnVerdict::Refusing { until: s(114) }
    );
}

#[test]
fn an_instant_from_the_past_is_clamped_after_a_refusal_too() {
    // Regression: the clamp used to read the last stored instant, which a
    // refusal clears, so the first instant after it went unclamped.
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for _ in 0..6 {
        c.record_supersession(peer, s(100));
    }
    // Treated as 100 s: counted inside the refusal, not as a fresh 0 s.
    for _ in 0..5 {
        assert_eq!(c.record_supersession(peer, s(0)), ChurnVerdict::Counted);
    }
    // The sixth trips it again, from 100 s.
    assert_eq!(
        c.record_supersession(peer, s(0)),
        ChurnVerdict::Refusing { until: s(160) }
    );
}

#[test]
fn tripping_again_during_a_refusal_extends_it() {
    let peer = fp(&[(0, 9)]);
    let mut c = ChurnLimiter::new();
    for _ in 0..6 {
        c.record_supersession(peer, s(0));
    }
    for t in 10..15 {
        assert_eq!(c.record_supersession(peer, s(t)), ChurnVerdict::Counted);
    }
    assert_eq!(
        c.record_supersession(peer, s(15)),
        ChurnVerdict::Refusing { until: s(75) }
    );
    assert_eq!(
        c.admission(&peer, s(60)),
        Admission::Refuse { until: s(75) }
    );
}

#[test]
fn prune_and_forget_bound_the_state() {
    let a = fp(&[(0, 1)]);
    let b = fp(&[(0, 2)]);
    let mut c = ChurnLimiter::new();
    c.record_supersession(a, s(0));
    for _ in 0..6 {
        c.record_supersession(b, s(0));
    }
    assert_eq!(c.tracked_peers(), 2);
    // At 59 s both still hold state.
    c.prune(s(59));
    assert_eq!(c.tracked_peers(), 2);
    // At 60 s a's supersession has expired and b's refusal has ended.
    c.prune(s(60));
    assert_eq!(c.tracked_peers(), 0);

    for _ in 0..6 {
        c.record_supersession(b, s(100));
    }
    c.forget(&b);
    assert_eq!(c.tracked_peers(), 0);
    assert_eq!(c.admission(&b, s(101)), Admission::Admit);
}

// ---------------------------------------------------------------------------
// Dormancy: no production path calls the policy, and the protocol max is 1
// ---------------------------------------------------------------------------

#[test]
fn this_build_still_speaks_only_protocol_version_1() {
    assert_eq!(pliwee_core::session::PROTOCOL_VERSION_MAX, 1);
}

fn rust_sources_under_src(dir: &Path, in_src: bool, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(name, "target" | "tests" | "benches" | "examples") {
                continue;
            }
            rust_sources_under_src(&path, in_src || name == "src", out);
        } else if in_src && path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_production_source_uses_the_policy_yet() {
    let desktop = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("desktop/ should exist");
    let module = desktop.join("core/src/duplicate_session.rs");
    let lib = desktop.join("core/src/lib.rs");
    let session = desktop.join("core/src/session.rs");

    let mut files = Vec::new();
    rust_sources_under_src(&desktop, false, &mut files);
    // Positive controls: the scan reads the session layer and the module
    // declaration, and finds the declaration. Without them an empty result
    // below could mean the scan read nothing.
    assert!(
        files.contains(&session),
        "scan missed {}",
        session.display()
    );
    assert!(files.contains(&module), "scan missed {}", module.display());
    let lib_text = std::fs::read_to_string(&lib).expect("read lib.rs");
    assert_eq!(lib_text.matches("duplicate_session").count(), 1);
    assert!(lib_text.contains("pub mod duplicate_session;"));

    let needles = [
        "duplicate_session",
        "DuplicateResolver",
        "ChurnLimiter",
        "CHURN_REFUSAL",
    ];
    let hits: Vec<_> = files
        .iter()
        .filter(|p| **p != module && **p != lib)
        .filter(|p| {
            let text =
                std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            needles.iter().any(|n| text.contains(n))
        })
        .collect();
    assert!(
        hits.is_empty(),
        "production Rust uses the dormant policy: {hits:?}"
    );
}
