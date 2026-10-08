//! The trust store half of "Remove from list": tombstones.
//!
//! The invariant every test here circles is one sentence: **REVOKED is not
//! UNKNOWN.** Taking a revoked device off the screen is a presentation
//! change, and the moment it becomes a deletion the same key stops being a
//! device this machine threw out and becomes a device this machine has never
//! met — which is a different greeting at the door (`REJECTED` against
//! `PAIRING_REQUIRED`, see `session::accept`) and a different story to tell
//! the owner.
//!
//! The admission half — what actually happens on the wire to a tombstoned
//! key — is in `daemon/tests/revoked_cleanup.rs`, because it needs a real
//! handshake and this crate has no daemon.

use std::collections::BTreeMap;

use pliwee_core::clipboard_policy::ClipboardPolicy;
use pliwee_core::identity::LocalIdentity;
use pliwee_core::notification_policy::{LockPolicy, NotificationPolicy};
use pliwee_core::store::{HideOutcome, Store, TrustedPeer, SCHEMA_VERSION};
use pliwee_core::Fingerprint;
use pliwee_proto::v1::Platform;

/// A fingerprint that is stable within a test and different between them.
fn fingerprint(seed: &str) -> Fingerprint {
    let id = LocalIdentity::generate(seed, Platform::Android).expect("identity");
    id.fingerprint()
}

fn peer(fingerprint: Fingerprint, name: &str) -> TrustedPeer {
    TrustedPeer {
        device_id: "0123456789abcdef0123456789abcdef".into(),
        device_name: name.into(),
        platform: Platform::Android as i32,
        fingerprint,
        paired_at_unix: 1_700_000_000,
        granted_capabilities: [
            ("battery.v1".to_string(), true),
            ("files.v1".to_string(), true),
            ("clipboard.v1".to_string(), true),
            ("notifications.v1".to_string(), true),
        ]
        .into_iter()
        .collect(),
        last_protocol_version: 1,
        revoked: false,
        hidden: false,
        clipboard_policy: ClipboardPolicy {
            allow_send: true,
            allow_receive: true,
            auto_send: true,
            auto_receive: true,
        },
        notification_policy: NotificationPolicy {
            allow_mirror: true,
            when_sink_locked: LockPolicy::Full,
            allow_dismiss_sync: true,
        },
        address_hints: Vec::new(),
    }
}

/// A store in a temp dir, with `peer` added and revoked.
fn revoked_store(name: &str) -> (tempfile::TempDir, Store, Fingerprint) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint(name);
    store.add_peer(peer(fp, name)).expect("add");
    store.revoke_peer(&fp).expect("revoke");
    (dir, store, fp)
}

// ---------------------------------------------------------------------------
// D3 / D4 — visible revoked becomes a hidden tombstone
// ---------------------------------------------------------------------------

#[test]
fn a_revoked_device_is_listed_until_it_is_removed_and_never_after() {
    let (_dir, mut store, fp) = revoked_store("SM-X620");

    assert!(
        store.listed_peers().any(|p| p.fingerprint == fp),
        "a revoked device is visible until the owner says otherwise"
    );
    assert_eq!(store.revoked_listed_peers().count(), 1);

    assert_eq!(
        store.hide_revoked_peer(&fp).expect("hide"),
        HideOutcome::Hidden
    );

    assert!(
        !store.listed_peers().any(|p| p.fingerprint == fp),
        "a tombstone is not on any list a person reads"
    );
    assert_eq!(store.revoked_listed_peers().count(), 0);
}

// ---------------------------------------------------------------------------
// D5 (store half) — the tombstone still answers "revoked", not "unknown"
// ---------------------------------------------------------------------------

#[test]
fn a_tombstone_is_still_a_record_and_still_revoked() {
    let (_dir, mut store, fp) = revoked_store("SM-X620");
    store.hide_revoked_peer(&fp).expect("hide");

    let record = store
        .peer_record(&fp)
        .expect("the record that keeps the revocation true must still be there");
    assert!(record.revoked, "hiding is not un-revoking");
    assert!(record.hidden);
    assert_eq!(
        record.fingerprint, fp,
        "the pinned identity is what survives"
    );

    assert!(
        store.trusted_peer(&fp).is_none(),
        "a tombstone is never a trusted peer"
    );
}

// ---------------------------------------------------------------------------
// D2 — a trusted device has no such action
// ---------------------------------------------------------------------------

#[test]
fn a_trusted_device_cannot_be_removed_from_the_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("still trusted");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    assert_eq!(
        store.hide_revoked_peer(&fp).expect("hide"),
        HideOutcome::NotRevoked,
        "removing from the list must not be a back door to revoking"
    );
    assert!(
        store.trusted_peer(&fp).is_some(),
        "and it must change nothing"
    );
    assert!(store.listed_peers().any(|p| p.fingerprint == fp));
}

#[test]
fn removing_a_device_that_is_not_there_is_an_error_not_a_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    assert_eq!(
        store
            .hide_revoked_peer(&fingerprint("stranger"))
            .expect("hide"),
        HideOutcome::NotFound
    );
    assert_eq!(store.peers().count(), 0);
}

#[test]
fn removing_a_tombstone_again_is_a_no_op() {
    let (_dir, mut store, fp) = revoked_store("SM-X620");
    store.hide_revoked_peer(&fp).expect("hide");
    assert_eq!(
        store.hide_revoked_peer(&fp).expect("hide again"),
        HideOutcome::AlreadyHidden
    );
}

// ---------------------------------------------------------------------------
// D6 — identity is the fingerprint, never the name
// ---------------------------------------------------------------------------

#[test]
fn removing_one_device_leaves_a_same_named_device_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");

    // Two tablets, the same model, the same name on screen, the same device
    // id — everything a list could sort or match on is identical. Only the
    // keys differ, and the key is the only thing that decides.
    let first = fingerprint("tablet one");
    let second = fingerprint("tablet two");
    store.add_peer(peer(first, "SM-X620")).expect("add");
    store.add_peer(peer(second, "SM-X620")).expect("add");
    store.revoke_peer(&first).expect("revoke");
    store.revoke_peer(&second).expect("revoke");

    store.hide_revoked_peer(&first).expect("hide");

    assert!(!store.peer_record(&first).expect("kept").is_listed());
    assert!(
        store.peer_record(&second).expect("kept").is_listed(),
        "the other SM-X620 was not touched"
    );
    assert_eq!(store.revoked_listed_peers().count(), 1);
}

// ---------------------------------------------------------------------------
// D9 — what is kept, and what is not
// ---------------------------------------------------------------------------

#[test]
fn a_tombstone_keeps_the_key_and_nothing_else() {
    let (_dir, mut store, fp) = revoked_store("SM-X620");
    store.hide_revoked_peer(&fp).expect("hide");

    let t = store.peer_record(&fp).expect("record");

    // Kept, because admission reads them.
    assert_eq!(t.fingerprint, fp);
    assert!(t.revoked);
    assert!(t.hidden);

    // Purged, because nothing about refusing a key needs them.
    assert_eq!(t.device_id, "", "a device id is history, not identity");
    assert_eq!(t.device_name, "", "and a display name even more so");
    assert_eq!(t.platform, 0);
    assert_eq!(t.paired_at_unix, 0);
    assert_eq!(t.last_protocol_version, 0);
    assert_eq!(t.granted_capabilities, BTreeMap::new());
    // Denied rather than merely default: a tombstone's line in `state.json`
    // must not read `allow_mirror: true`, which is what a policy's *default*
    // serializes to.
    assert_eq!(t.clipboard_policy, ClipboardPolicy::DENIED);
    assert_eq!(t.notification_policy, NotificationPolicy::DENIED);
    assert!(!t.clipboard_policy.allow_send && !t.clipboard_policy.allow_receive);
    assert!(!t.notification_policy.allow_mirror);

    // And the same, read off the disk rather than out of memory: a purge that
    // only happened in RAM would come back on the next start.
    let reopened = Store::open(_dir.path()).expect("reopen");
    let t = reopened.peer_record(&fp).expect("record");
    assert_eq!(t.device_name, "");
    assert!(t.granted_capabilities.is_empty());
    assert_eq!(t.notification_policy, NotificationPolicy::DENIED);
}

// ---------------------------------------------------------------------------
// D10 — a sensitive grant cannot ride the cleanup back in
// ---------------------------------------------------------------------------

#[test]
fn revoking_clears_the_grants_and_the_policies_that_go_with_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("SM-X620");
    store.add_peer(peer(fp, "SM-X620")).expect("add");

    store.revoke_peer(&fp).expect("revoke");

    let r = store.peer_record(&fp).expect("record");
    assert!(
        r.granted_capabilities.is_empty(),
        "grants go with the trust"
    );
    assert_eq!(
        r.notification_policy,
        NotificationPolicy::DENIED,
        "`allow_mirror` must not survive on a record that can be paired again"
    );
    assert_eq!(r.clipboard_policy, ClipboardPolicy::DENIED);
    assert!(!r.allows("notifications.v1"));
    assert!(!r.allows("clipboard.v1"));
}

// ---------------------------------------------------------------------------
// D11 / D12 — a fresh pairing brings the device back, exactly once
// ---------------------------------------------------------------------------

#[test]
fn pairing_again_over_a_tombstone_restores_one_visible_trusted_record() {
    let (_dir, mut store, fp) = revoked_store("SM-X620");
    store.hide_revoked_peer(&fp).expect("hide");
    assert_eq!(store.peers().count(), 1, "the tombstone is the only record");

    // What `DaemonState::store_peer` writes after a proof succeeds: a fresh
    // record, built from the handshake and from nothing that was on disk.
    let mut fresh = peer(fp, "SM-X620");
    fresh.granted_capabilities = [("battery.v1".to_string(), true)].into_iter().collect();
    fresh.clipboard_policy = ClipboardPolicy::default();
    fresh.notification_policy = NotificationPolicy::default();
    store.add_peer(fresh).expect("re-pair");

    assert_eq!(
        store.peers().count(),
        1,
        "one cryptographic identity is one row, before and after"
    );
    let r = store.trusted_peer(&fp).expect("trusted again");
    assert!(!r.revoked);
    assert!(r.is_listed(), "and visible again");
    assert!(
        !r.allows("notifications.v1") && !r.allows("clipboard.v1"),
        "nothing sensitive came back with it"
    );
    assert_eq!(store.listed_peers().count(), 1);
}

// ---------------------------------------------------------------------------
// D13 / D14 — a different key is a different device
// ---------------------------------------------------------------------------

#[test]
fn a_tombstone_grants_nothing_to_a_different_fingerprint() {
    let (_dir, mut store, old) = revoked_store("SM-X620");
    store.hide_revoked_peer(&old).expect("hide");

    // Same name, same device id, same everything a list could key on.
    let new = fingerprint("a different key");
    let mut fresh = peer(new, "SM-X620");
    fresh.granted_capabilities = [("battery.v1".to_string(), true)].into_iter().collect();
    fresh.clipboard_policy = ClipboardPolicy::default();
    fresh.notification_policy = NotificationPolicy::default();
    store.add_peer(fresh).expect("pair the new key");

    assert!(
        store.trusted_peer(&new).is_some(),
        "B is trusted on its own"
    );
    assert!(
        store.trusted_peer(&old).is_none(),
        "A's tombstone is untouched and still not trusted"
    );
    assert!(
        !store.peer_record(&new).expect("B").hidden,
        "B is a new peer, not an inheritor of A's state"
    );
    assert!(
        store.peer_record(&old).expect("A").hidden,
        "and A did not become visible because something with its name appeared"
    );
    assert_eq!(store.peers().count(), 2, "two keys, two records");
}

// ---------------------------------------------------------------------------
// D15 — persistence
// ---------------------------------------------------------------------------

#[test]
fn a_tombstone_survives_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fp = fingerprint("SM-X620");
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(fp, "SM-X620")).expect("add");
        store.revoke_peer(&fp).expect("revoke");
        store.hide_revoked_peer(&fp).expect("hide");
    }

    let store = Store::open(dir.path()).expect("reopen");
    let r = store
        .peer_record(&fp)
        .expect("a tombstone that did not survive a restart is a deleted device");
    assert!(r.revoked);
    assert!(r.hidden);
    assert!(store.trusted_peer(&fp).is_none());
    assert_eq!(store.listed_peers().count(), 0);
}

// ---------------------------------------------------------------------------
// D16 — migration from a store written before this field existed
// ---------------------------------------------------------------------------

#[test]
fn a_store_without_the_field_reads_every_revoked_device_as_visible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let trusted = fingerprint("Fedora");
    let revoked = fingerprint("SM-X620");
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(trusted, "Fedora")).expect("add");
        store.add_peer(peer(revoked, "SM-X620")).expect("add");
        store.revoke_peer(&revoked).expect("revoke");
    }

    // Rewrite the document exactly as a pre-cleanup build would have: no
    // `hidden` key on any record.
    let path = dir.path().join("state.json");
    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
    for p in doc["peers"].as_array_mut().expect("peers") {
        p.as_object_mut().expect("record").remove("hidden");
    }
    assert!(!doc.to_string().contains("hidden"));
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).expect("encode")).expect("write");

    let store = Store::open(dir.path()).expect("reopen an older store");
    assert_eq!(
        store.listed_peers().count(),
        2,
        "migration must not hide anybody's revoked devices for them"
    );
    assert_eq!(store.revoked_listed_peers().count(), 1);
    assert!(
        store.trusted_peer(&trusted).is_some(),
        "and the trusted device is still trusted, with its grants"
    );
    assert!(store.trusted_peer(&trusted).expect("t").allows("files.v1"));
    assert!(
        store.peer_record(&revoked).expect("r").revoked,
        "and the pin behind the revocation is still there"
    );
}

// ---------------------------------------------------------------------------
// D17 — a malformed tombstone fails closed
// ---------------------------------------------------------------------------

#[test]
fn a_record_that_says_hidden_but_not_revoked_is_read_as_revoked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fp = fingerprint("SM-X620");
    {
        let mut store = Store::open(dir.path()).expect("open");
        store.add_peer(peer(fp, "SM-X620")).expect("add");
    }

    // The dangerous combination: out of sight, and still trusted. A file that
    // says this is either hand-edited or damaged, and believing it would mean
    // a device nobody can see keeps its grants.
    let path = dir.path().join("state.json");
    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
    for p in doc["peers"].as_array_mut().expect("peers") {
        p["hidden"] = serde_json::Value::Bool(true);
        p["revoked"] = serde_json::Value::Bool(false);
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).expect("encode")).expect("write");

    let store = Store::open(dir.path()).expect("reopen");
    let r = store.peer_record(&fp).expect("record");
    assert!(
        r.revoked,
        "hidden implies revoked, decided here rather than believed from the file"
    );
    assert!(store.trusted_peer(&fp).is_none());
    assert!(!r.allows("files.v1"));
}

#[test]
fn the_schema_version_is_unchanged_because_the_field_is_additive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = Store::open(dir.path()).expect("open");
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("state.json")).expect("read"))
            .expect("parse");
    assert_eq!(doc["schema_version"], SCHEMA_VERSION);
    assert_eq!(
        SCHEMA_VERSION, 2,
        "adding an optional flag with a safe default is not a schema break; \
         bumping this would make older builds refuse the file for nothing"
    );
}

// ---------------------------------------------------------------------------
// D18 / D19 / D20 — bulk removal
// ---------------------------------------------------------------------------

#[test]
fn bulk_removal_touches_revoked_visible_devices_and_only_those() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");

    let trusted = fingerprint("Fedora");
    let revoked_a = fingerprint("old tablet a");
    let revoked_b = fingerprint("old tablet b");
    let already_hidden = fingerprint("old tablet c");

    store.add_peer(peer(trusted, "Fedora")).expect("add");
    for (fp, name) in [
        (revoked_a, "SM-X620"),
        (revoked_b, "SM-X620"),
        (already_hidden, "SM-X620"),
    ] {
        store.add_peer(peer(fp, name)).expect("add");
        store.revoke_peer(&fp).expect("revoke");
    }
    store.hide_revoked_peer(&already_hidden).expect("hide");

    let before = store.peer_record(&trusted).expect("trusted").clone();
    let hidden = store.hide_all_revoked_peers().expect("bulk");

    assert_eq!(hidden.len(), 2, "the two visible revoked ones, and no more");
    assert!(hidden.contains(&revoked_a) && hidden.contains(&revoked_b));
    assert!(
        !hidden.contains(&already_hidden),
        "an existing tombstone is not removed twice"
    );

    // D19: the trusted device is unchanged, field by field.
    let after = store.peer_record(&trusted).expect("trusted");
    assert_eq!(after.device_id, before.device_id);
    assert_eq!(after.device_name, before.device_name);
    assert_eq!(after.paired_at_unix, before.paired_at_unix);
    assert_eq!(after.granted_capabilities, before.granted_capabilities);
    assert_eq!(after.clipboard_policy, before.clipboard_policy);
    assert_eq!(after.notification_policy, before.notification_policy);
    assert!(!after.revoked && !after.hidden);

    // Every revoked key is still refused, and still present.
    for fp in [revoked_a, revoked_b, already_hidden] {
        let r = store.peer_record(&fp).expect("tombstone kept");
        assert!(r.revoked && r.hidden);
    }
    assert_eq!(store.listed_peers().count(), 1);
}

#[test]
fn bulk_removal_with_nothing_revoked_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    let fp = fingerprint("Fedora");
    store.add_peer(peer(fp, "Fedora")).expect("add");

    // D20: the caller decides whether to offer the action from this same
    // iterator, so an empty result is the same fact as a hidden button.
    assert_eq!(store.revoked_listed_peers().count(), 0);

    let before = std::fs::read(dir.path().join("state.json")).expect("read");
    assert!(store.hide_all_revoked_peers().expect("bulk").is_empty());
    let after = std::fs::read(dir.path().join("state.json")).expect("read");
    assert_eq!(before, after, "a no-op must not rewrite the trust store");
}
