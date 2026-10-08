//! Per-peer address hints (Mesh V2 SPEC §5 and §11, issue #91).
//!
//! Every test here comes back to one sentence: **a hint is a route, never an
//! identity.** It is bounded, it is disposable, it goes when the trust goes,
//! and nothing about it can make a key trusted, untrusted or somebody else.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use pliwee_core::clipboard_policy::ClipboardPolicy;
use pliwee_core::identity::LocalIdentity;
use pliwee_core::notification_policy::NotificationPolicy;
use pliwee_core::store::{AddressHint, Store, TrustedPeer, MAX_ADDRESS_HINTS, SCHEMA_VERSION};
use pliwee_core::Fingerprint;
use pliwee_proto::v1::Platform;

fn fingerprint(seed: &str) -> Fingerprint {
    LocalIdentity::generate(seed, Platform::Android)
        .expect("identity")
        .fingerprint()
}

fn peer(fingerprint: Fingerprint, name: &str) -> TrustedPeer {
    TrustedPeer {
        device_id: "0123456789abcdef0123456789abcdef".into(),
        device_name: name.into(),
        platform: Platform::Android as i32,
        fingerprint,
        paired_at_unix: 1_700_000_000,
        granted_capabilities: [("battery.v1".to_string(), true)].into_iter().collect(),
        last_protocol_version: 1,
        revoked: false,
        hidden: false,
        clipboard_policy: ClipboardPolicy::default(),
        notification_policy: NotificationPolicy::default(),
        address_hints: Vec::new(),
    }
}

fn v4(last: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, last)), port)
}

fn routes(store: &Store, fp: &Fingerprint) -> Vec<SocketAddr> {
    store
        .address_hints(fp)
        .iter()
        .map(AddressHint::socket_addr)
        .collect()
}

fn state_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("state.json")
}

fn read_doc(dir: &tempfile::TempDir) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(state_path(dir)).expect("read")).expect("parse")
}

fn write_doc(dir: &tempfile::TempDir, doc: &serde_json::Value) {
    std::fs::write(
        state_path(dir),
        serde_json::to_vec_pretty(doc).expect("encode"),
    )
    .expect("write");
}

// ---------------------------------------------------------------------------
// Migration: existing state loads without re-pairing and with no hints
// ---------------------------------------------------------------------------

#[test]
fn a_store_written_before_hints_existed_loads_unchanged_and_without_hints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let trusted = fingerprint("Fedora");
    let revoked = fingerprint("SM-X620");
    let identity_before;
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(trusted, "Fedora")).expect("add");
        store.add_peer(peer(revoked, "SM-X620")).expect("add");
        store.revoke_peer(&revoked).expect("revoke");
        identity_before = (
            store.identity().fingerprint(),
            store.identity().device_id().to_string(),
        );
    }

    // Exactly what a schema-2 build wrote: no `address_hints` key anywhere.
    let mut doc = read_doc(&dir);
    for p in doc["peers"].as_array_mut().expect("peers") {
        assert!(
            p.as_object_mut()
                .expect("record")
                .remove("address_hints")
                .is_some(),
            "precondition: this build writes the key, so removing it is the migration"
        );
    }
    assert!(!doc.to_string().contains("address_hints"));
    assert_eq!(doc["schema_version"], 2);
    write_doc(&dir, &doc);

    let store = Store::open(dir.path()).expect("an older store must open");
    assert_eq!(
        (
            store.identity().fingerprint(),
            store.identity().device_id().to_string()
        ),
        identity_before,
        "same identity: nothing re-pairs"
    );
    let t = store.trusted_peer(&trusted).expect("still trusted");
    assert!(t.allows("battery.v1"), "with its grants");
    assert!(t.address_hints.is_empty());
    assert!(store.address_hints(&trusted).is_empty());
    assert!(store.peer_record(&revoked).expect("kept").revoked);
    assert_eq!(store.peers().count(), 2);
}

#[test]
fn the_schema_version_is_unchanged_because_hints_are_additive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");
    store
        .record_address_success(&fp, v4(10, 47_800), 1_790_000_000)
        .expect("record");

    let doc = read_doc(&dir);
    assert_eq!(
        doc["schema_version"], SCHEMA_VERSION,
        "an additive field does not bump the version older builds check"
    );
    assert_eq!(
        doc["peers"][0]["address_hints"][0]["address"],
        "192.168.1.10"
    );
    assert_eq!(doc["peers"][0]["address_hints"][0]["port"], 47_800);
}

#[test]
fn malformed_hints_are_discarded_and_never_make_the_store_unreadable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fp = fingerprint("Fedora");
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(fp, "Fedora")).expect("add");
    }

    let mut doc = read_doc(&dir);
    doc["peers"][0]["address_hints"] = serde_json::json!([
        {"address": "not an address", "port": 1, "last_success_unix": 1},
        {"address": "192.168.1.20", "port": 70000, "last_success_unix": 1},
        {"address": "0.0.0.0", "port": 47800, "last_success_unix": 1},
        {"address": "192.168.1.21", "port": 0, "last_success_unix": 1},
        "garbage",
        {"address": "192.168.1.22", "port": 47800, "last_success_unix": 5},
    ]);
    write_doc(&dir, &doc);

    let store = Store::open(dir.path()).expect("a bad hint is not a bad store");
    assert!(store.trusted_peer(&fp).is_some());
    assert_eq!(routes(&store, &fp), vec![v4(22, 47_800)]);

    // A value that is not a list at all reads as no hints.
    for not_a_list in [serde_json::json!({"oops": true}), serde_json::Value::Null] {
        let mut doc = read_doc(&dir);
        doc["peers"][0]["address_hints"] = not_a_list;
        write_doc(&dir, &doc);
        let store = Store::open(dir.path()).expect("still readable");
        assert!(store.trusted_peer(&fp).is_some());
        assert!(store.address_hints(&fp).is_empty());
    }
}

#[test]
fn an_oversized_or_duplicated_list_on_disk_is_bounded_on_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fp = fingerprint("Fedora");
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(fp, "Fedora")).expect("add");
    }

    let mut hints: Vec<serde_json::Value> = (1..=20)
        .map(|i| serde_json::json!({"address": format!("192.168.1.{i}"), "port": 47800, "last_success_unix": i}))
        .collect();
    hints.insert(
        1,
        serde_json::json!({"address": "::ffff:192.168.1.1", "port": 47800, "last_success_unix": 0}),
    );
    let mut doc = read_doc(&dir);
    doc["peers"][0]["address_hints"] = serde_json::Value::Array(hints);
    write_doc(&dir, &doc);

    let store = Store::open(dir.path()).expect("open");
    let expected: Vec<SocketAddr> = (1..=8).map(|i| v4(i, 47_800)).collect();
    assert_eq!(routes(&store, &fp), expected);
}

// ---------------------------------------------------------------------------
// Bound: more than 8 successes leaves exactly the 8 most recent
// ---------------------------------------------------------------------------

#[test]
fn more_than_eight_successes_keep_exactly_the_eight_most_recent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    for i in 1..=12u8 {
        assert!(store
            .record_address_success(&fp, v4(i, 47_800), 1_790_000_000 + i64::from(i))
            .expect("record"));
    }

    assert_eq!(MAX_ADDRESS_HINTS, 8);
    let expected: Vec<SocketAddr> = (5..=12).rev().map(|i| v4(i, 47_800)).collect();
    assert_eq!(
        routes(&store, &fp),
        expected,
        "most recent first, oldest gone"
    );

    let reopened = Store::open(dir.path()).expect("reopen");
    assert_eq!(
        routes(&reopened, &fp),
        expected,
        "and the same after a restart"
    );
    let times: Vec<i64> = reopened
        .address_hints(&fp)
        .iter()
        .map(|h| h.last_success_unix)
        .collect();
    assert_eq!(
        times,
        (5..=12)
            .rev()
            .map(|i| 1_790_000_000 + i)
            .collect::<Vec<_>>()
    );
}

#[test]
fn eviction_follows_the_order_of_success_not_the_wall_clock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    for i in 1..=8u8 {
        store
            .record_address_success(&fp, v4(i, 47_800), 2_000_000_000)
            .expect("record");
    }
    // The clock has stepped back by years; this address still just worked.
    store
        .record_address_success(&fp, v4(99, 47_800), 1)
        .expect("record");

    let r = routes(&store, &fp);
    assert_eq!(r.len(), 8);
    assert_eq!(r[0], v4(99, 47_800), "the newest success is kept");
    assert!(!r.contains(&v4(1, 47_800)), "the oldest success is dropped");
}

// ---------------------------------------------------------------------------
// De-duplication: equivalent hints do not duplicate and refresh recency
// ---------------------------------------------------------------------------

#[test]
fn an_equivalent_hint_replaces_the_old_one_and_moves_to_the_front() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    store
        .record_address_success(&fp, v4(1, 47_800), 100)
        .expect("record");
    store
        .record_address_success(&fp, v4(2, 47_800), 200)
        .expect("record");
    store
        .record_address_success(&fp, v4(1, 47_800), 300)
        .expect("record");

    let hints = store.address_hints(&fp);
    assert_eq!(hints.len(), 2, "one route, one hint");
    assert_eq!(hints[0].socket_addr(), v4(1, 47_800));
    assert_eq!(hints[0].last_success_unix, 300, "recency refreshed");
    assert_eq!(hints[1].socket_addr(), v4(2, 47_800));
}

#[test]
fn an_ipv4_mapped_address_is_the_same_route_as_the_ipv4_address() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    let mapped = SocketAddr::new(
        IpAddr::V6(Ipv4Addr::new(192, 168, 1, 7).to_ipv6_mapped()),
        47_800,
    );
    store
        .record_address_success(&fp, mapped, 100)
        .expect("record");
    store
        .record_address_success(&fp, v4(7, 47_800), 200)
        .expect("record");

    assert_eq!(routes(&store, &fp), vec![v4(7, 47_800)]);
}

#[test]
fn the_same_address_on_another_port_is_another_route() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    store
        .record_address_success(&fp, v4(7, 47_800), 100)
        .expect("record");
    store
        .record_address_success(&fp, v4(7, 47_801), 200)
        .expect("record");

    assert_eq!(routes(&store, &fp), vec![v4(7, 47_801), v4(7, 47_800)]);
}

#[test]
fn an_ipv6_address_is_a_route_of_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    let global = SocketAddr::new(
        IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 7)),
        47_800,
    );
    store
        .record_address_success(&fp, global, 100)
        .expect("record");
    store
        .record_address_success(&fp, v4(7, 47_800), 200)
        .expect("record");

    assert_eq!(routes(&store, &fp), vec![v4(7, 47_800), global]);
    let reopened = Store::open(dir.path()).expect("reopen");
    assert_eq!(routes(&reopened, &fp), vec![v4(7, 47_800), global]);
}

#[test]
fn pairing_again_starts_with_no_hints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");
    store
        .record_address_success(&fp, v4(7, 47_800), 100)
        .expect("record");

    // What a fresh pairing writes: a whole new record, routes included. A
    // pairing proves a key, not where to find it.
    store.add_peer(peer(fp, "Fedora")).expect("re-pair");

    assert!(store.trusted_peer(&fp).is_some());
    assert!(store.address_hints(&fp).is_empty());
}

#[test]
fn an_address_nothing_could_dial_is_not_recorded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");
    let before = std::fs::read(state_path(&dir)).expect("read");

    for bad in [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 47_800),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 47_800),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), 47_800),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353),
        SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            47_800,
        ),
        v4(7, 0),
    ] {
        assert!(
            !store.record_address_success(&fp, bad, 100).expect("record"),
            "{bad} is not a route"
        );
    }
    assert!(store.address_hints(&fp).is_empty());
    assert_eq!(
        std::fs::read(state_path(&dir)).expect("read"),
        before,
        "a refused hint writes nothing"
    );
}

// ---------------------------------------------------------------------------
// Revocation and tombstones retain no usable routing hints
// ---------------------------------------------------------------------------

#[test]
fn revoking_drops_the_hints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("SM-X620");
    store.add_peer(peer(fp, "SM-X620")).expect("add");
    store
        .record_address_success(&fp, v4(7, 47_800), 100)
        .expect("record");
    assert_eq!(store.address_hints(&fp).len(), 1, "precondition");

    store.revoke_peer(&fp).expect("revoke");

    assert!(store
        .peer_record(&fp)
        .expect("kept")
        .address_hints
        .is_empty());
    assert!(store.address_hints(&fp).is_empty());
    let reopened = Store::open(dir.path()).expect("reopen");
    assert!(reopened
        .peer_record(&fp)
        .expect("kept")
        .address_hints
        .is_empty());
    assert!(!read_doc(&dir).to_string().contains("192.168.1.7"));
}

#[test]
fn a_tombstone_keeps_no_hints_and_cannot_gain_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("SM-X620");
    store.add_peer(peer(fp, "SM-X620")).expect("add");
    store
        .record_address_success(&fp, v4(7, 47_800), 100)
        .expect("record");
    store.revoke_peer(&fp).expect("revoke");
    store.hide_revoked_peer(&fp).expect("hide");

    let t = store.peer_record(&fp).expect("tombstone");
    assert!(t.revoked && t.hidden);
    assert!(t.address_hints.is_empty());

    assert!(
        !store
            .record_address_success(&fp, v4(8, 47_800), 200)
            .expect("record"),
        "a revoked key gets no new route"
    );
    assert!(store
        .peer_record(&fp)
        .expect("tombstone")
        .address_hints
        .is_empty());
    assert!(
        store.trusted_peer(&fp).is_none(),
        "and is still not trusted"
    );
}

#[test]
fn hints_on_a_revoked_record_in_the_file_are_dropped_on_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let revoked = fingerprint("SM-X620");
    let hidden_only = fingerprint("SM-X621");
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(revoked, "SM-X620")).expect("add");
        store.add_peer(peer(hidden_only, "SM-X621")).expect("add");
        store.revoke_peer(&revoked).expect("revoke");
    }

    // Hand-edited: a revoked record carrying hints, and a "hidden but not
    // revoked" one — which load reads as revoked — carrying them too.
    let mut doc = read_doc(&dir);
    for p in doc["peers"].as_array_mut().expect("peers") {
        p["address_hints"] = serde_json::json!([
            {"address": "192.168.1.7", "port": 47800, "last_success_unix": 1}
        ]);
        if p["revoked"] == false {
            p["hidden"] = serde_json::Value::Bool(true);
        }
    }
    write_doc(&dir, &doc);

    let store = Store::open(dir.path()).expect("reopen");
    for fp in [revoked, hidden_only] {
        let r = store.peer_record(&fp).expect("record");
        assert!(r.revoked);
        assert!(r.address_hints.is_empty(), "no route survives a revocation");
    }
}

// ---------------------------------------------------------------------------
// Hints cannot select or authorize an identity
// ---------------------------------------------------------------------------

#[test]
fn a_hint_never_creates_a_trust_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let stranger = fingerprint("stranger");
    let before = std::fs::read(state_path(&dir)).expect("read");

    assert!(!store
        .record_address_success(&stranger, v4(7, 47_800), 100)
        .expect("record"));

    assert!(store.peer_record(&stranger).is_none(), "still unknown");
    assert!(store.trusted_peer(&stranger).is_none());
    assert_eq!(store.peers().count(), 0);
    assert_eq!(std::fs::read(state_path(&dir)).expect("read"), before);
}

#[test]
fn a_shared_address_does_not_make_another_key_that_device() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let phone = fingerprint("phone");
    let tablet = fingerprint("tablet");
    let impostor = fingerprint("impostor");
    store.add_peer(peer(phone, "Pixel")).expect("add");
    store.add_peer(peer(tablet, "SM-X620")).expect("add");
    store.revoke_peer(&tablet).expect("revoke");

    // The phone has been reached at this address. Another key now answers
    // there: DHCP handed the lease on, or somebody is pretending.
    store
        .record_address_success(&phone, v4(7, 47_800), 100)
        .expect("record");
    let phone_before = store.peer_record(&phone).expect("phone").clone();

    // The admission questions are asked by fingerprint and give the same
    // answers they gave before any hint existed.
    assert!(
        store.peer_record(&impostor).is_none(),
        "the impostor is unknown"
    );
    assert!(store.trusted_peer(&impostor).is_none());
    assert!(store.address_hints(&impostor).is_empty());
    assert!(
        store.trusted_peer(&tablet).is_none(),
        "a revoked key is not rescued by anything about routes"
    );

    // Recording the same route for the revoked key is refused, and leaves
    // the phone's record and its grants exactly as they were.
    assert!(!store
        .record_address_success(&tablet, v4(7, 47_800), 200)
        .expect("record"));
    let phone_after = store.trusted_peer(&phone).expect("phone");
    assert_eq!(
        phone_after.granted_capabilities,
        phone_before.granted_capabilities
    );
    assert_eq!(phone_after.device_id, phone_before.device_id);
    assert_eq!(phone_after.address_hints, phone_before.address_hints);
}

#[test]
fn recording_a_hint_changes_nothing_but_the_hints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");
    let before = store.peer_record(&fp).expect("record").clone();

    store
        .record_address_success(&fp, v4(7, 47_800), 100)
        .expect("record");

    let after = store.peer_record(&fp).expect("record");
    assert_eq!(after.fingerprint, before.fingerprint);
    assert_eq!(after.device_id, before.device_id);
    assert_eq!(after.device_name, before.device_name);
    assert_eq!(after.revoked, before.revoked);
    assert_eq!(after.hidden, before.hidden);
    assert_eq!(after.granted_capabilities, before.granted_capabilities);
    assert_eq!(after.clipboard_policy, before.clipboard_policy);
    assert_eq!(after.notification_policy, before.notification_policy);
    assert_eq!(after.last_protocol_version, before.last_protocol_version);
    assert_ne!(after.address_hints, before.address_hints);
}

#[test]
fn hints_supplied_with_a_new_record_are_bounded_and_dropped_if_revoked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let trusted = fingerprint("Fedora");
    let revoked = fingerprint("SM-X620");

    let mut p = peer(trusted, "Fedora");
    p.address_hints = (1..=12)
        .filter_map(|i| AddressHint::new(v4(i, 47_800), i64::from(i)))
        .collect();
    p.address_hints.push(p.address_hints[0].clone());
    store.add_peer(p).expect("add");
    assert_eq!(store.address_hints(&trusted).len(), MAX_ADDRESS_HINTS);

    let mut r = peer(revoked, "SM-X620");
    r.revoked = true;
    r.address_hints = vec![AddressHint::new(v4(9, 47_800), 1).expect("hint")];
    store.add_peer(r).expect("add");
    assert!(store
        .peer_record(&revoked)
        .expect("kept")
        .address_hints
        .is_empty());
}
