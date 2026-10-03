//! The control protocol, pinned to a committed fixture.
//!
//! `fixtures/control-protocol.json` holds the exact JSON every message kind
//! serialises to. This test checks the Rust types against it in both
//! directions; `macos/Tests/PliweeKitTests` decodes and encodes the same file
//! from Swift. The two front ends written in Rust share these types and
//! cannot drift; `Pliwee.app` cannot share them, so it shares this file
//! instead, and a change to the contract fails one suite or the other until
//! both agree.
//!
//! When the contract changes on purpose, regenerate the fixture and review
//! the diff:
//!
//! ```text
//! PLIWEE_BLESS_CONTROL_FIXTURES=1 cargo test -p pliwee-control --test control_fixtures
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use pliwee_control::*;
use serde_json::{json, Value};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("control-protocol.json")
}

fn battery() -> BatteryReport {
    BatteryReport {
        percentage: 81,
        charging_state: "charging".into(),
        age_secs: 12,
        stale: false,
    }
}

const FP: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const FP_SHORT: &str = "A1B2 C3D4 E5F6 0718";

fn phone() -> DeviceReport {
    DeviceReport {
        device_id: "0123456789abcdef0123456789abcdef".into(),
        device_name: "Galaxy S25".into(),
        platform: "android".into(),
        fingerprint: FP.into(),
        fingerprint_short: FP_SHORT.into(),
        paired_at_unix: 1_790_000_000,
        granted_capabilities: vec!["battery.v1".into(), "files.v1".into()],
        revoked: false,
        paired: true,
        connected: true,
        state: DeviceState::Connected,
        silent_secs: Some(3),
        last_seen_secs_ago: None,
        battery: Some(battery()),
    }
}

fn revoked_tablet() -> DeviceReport {
    DeviceReport {
        device_id: String::new(),
        device_name: String::new(),
        platform: "unknown".into(),
        fingerprint: "ff".repeat(32),
        fingerprint_short: "FFFF FFFF FFFF FFFF".into(),
        paired_at_unix: 1_780_000_000,
        granted_capabilities: vec![],
        revoked: true,
        paired: false,
        connected: false,
        state: DeviceState::Revoked,
        silent_secs: None,
        last_seen_secs_ago: Some(600),
        battery: None,
    }
}

fn transfer() -> TransferReport {
    TransferReport {
        transfer_id: "00112233445566778899aabbccddeeff".into(),
        seq: 4,
        device_name: "Galaxy S25".into(),
        fingerprint_short: FP_SHORT.into(),
        direction: transfer_direction::RECEIVING.into(),
        filename: "holiday photo.jpg".into(),
        mime_type: "image/jpeg".into(),
        size_bytes: 300_000,
        bytes_transferred: 300_000,
        percentage: Some(100),
        state: transfer_state::COMPLETED.into(),
        failure: None,
        failure_code: None,
        stored_at: Some("/Users/ana/Downloads/Pliwee/holiday photo.jpg".into()),
    }
}

fn failed_transfer() -> TransferReport {
    TransferReport {
        transfer_id: "ffeeddccbbaa99887766554433221100".into(),
        seq: 5,
        device_name: "Galaxy S25".into(),
        fingerprint_short: FP_SHORT.into(),
        direction: transfer_direction::SENDING.into(),
        filename: "notes.txt".into(),
        mime_type: "text/plain".into(),
        size_bytes: 0,
        bytes_transferred: 0,
        percentage: None,
        state: transfer_state::FAILED.into(),
        failure: Some("the receiver declined the file".into()),
        failure_code: Some(transfer_failure::DECLINED_BY_USER.into()),
        stored_at: None,
    }
}

fn status() -> StatusReport {
    StatusReport {
        device_name: "Ana's MacBook Air".into(),
        device_id: "fedcba9876543210fedcba9876543210".into(),
        fingerprint: "0f".repeat(32),
        fingerprint_short: "0F0F 0F0F 0F0F 0F0F".into(),
        key_backing: "software".into(),
        listen_port: 55432,
        listen_families: "IPv4+IPv6".into(),
        protocol_version_min: 1,
        protocol_version_max: 1,
        capabilities: vec![
            "battery.v1".into(),
            "files.v1".into(),
            "clipboard.v1".into(),
            "notifications.v1".into(),
        ],
        paired_devices: 1,
        connections: vec![ConnectionReport {
            device_id: "0123456789abcdef0123456789abcdef".into(),
            device_name: "Galaxy S25".into(),
            fingerprint_short: FP_SHORT.into(),
            negotiated_capabilities: vec!["battery.v1".into(), "files.v1".into()],
            battery: Some(battery()),
            session_id: 7,
            state: DeviceState::Connected,
            silent_secs: 3,
        }],
        devices: vec![phone(), revoked_tablet()],
        pairing_active: false,
        migrated_from: None,
        legacy_partial_files: vec![],
    }
}

fn clipboard() -> ClipboardStatusReport {
    ClipboardStatusReport {
        enabled: true,
        backend: "nspasteboard".into(),
        backend_detail: "NSPasteboard (general pasteboard)".into(),
        backend_available: true,
        watch_available: false,
        sensitive_available: true,
        sensitive_detail: String::new(),
        event_cache_entries: 2,
        suppression_cache_entries: 1,
        peers: vec![ClipboardPeerReport {
            device_id: "0123456789abcdef0123456789abcdef".into(),
            device_name: "Galaxy S25".into(),
            fingerprint_short: FP_SHORT.into(),
            granted: true,
            revoked: false,
            connected: true,
            allow_send: true,
            allow_receive: true,
            auto_send: false,
            auto_receive: false,
            last_outcome: Some("applied".into()),
        }],
        pending: vec![PendingClipReport {
            device_name: "Galaxy S25".into(),
            fingerprint_short: FP_SHORT.into(),
            bytes: 42,
            hash_prefix: "deadbeef".into(),
            sensitive: true,
            origin_device_id: "0123456789abcdef0123456789abcdef".into(),
            age_secs: 9,
        }],
    }
}

fn notifications() -> NotificationsStatusReport {
    NotificationsStatusReport {
        enabled: true,
        backend: "none".into(),
        backend_detail: "no notification sink".into(),
        available: false,
        body_markup: false,
        persistence: false,
        lock_source: "unknown".into(),
        lock_detail: "lock state unknown; treated as locked".into(),
        locked: true,
        mirrors: 0,
        peers: vec![NotificationPeerReport {
            device_id: "0123456789abcdef0123456789abcdef".into(),
            device_name: "Galaxy S25".into(),
            fingerprint_short: FP_SHORT.into(),
            granted: false,
            revoked: false,
            connected: true,
            allow_mirror: false,
            when_locked: "app-only".into(),
            allow_dismiss_sync: false,
            mirrors: 0,
            displayed: 0,
            evicted: 0,
            local_roles: 0,
            local_epoch: 1,
            peer_is_source: true,
            peer_is_dismiss_target: false,
            peer_epoch: 2,
            local_reports_dismissals: false,
            dismissals_sent: 0,
            dismissals_refused: 0,
            snapshot_open: false,
            queued: 0,
            coalesced: 0,
            dropped: 0,
        }],
    }
}

fn requests() -> BTreeMap<&'static str, Request> {
    BTreeMap::from([
        ("status", Request::Status),
        ("devices", Request::Devices),
        (
            "pair",
            Request::Pair {
                ttl_secs: Some(120),
            },
        ),
        ("pair_default_ttl", Request::Pair { ttl_secs: None }),
        ("confirm", Request::Confirm { accept: true }),
        ("unpair", Request::Unpair { device: FP.into() }),
        (
            "hide_revoked_device",
            Request::HideRevokedDevice {
                fingerprint: "ff".repeat(32),
            },
        ),
        ("hide_all_revoked_devices", Request::HideAllRevokedDevices),
        ("ping", Request::Ping { device: FP.into() }),
        (
            "grant",
            Request::Grant {
                device: FP.into(),
                capability: "files.v1".into(),
                granted: true,
            },
        ),
        (
            "send",
            Request::Send {
                device: FP.into(),
                path: "/Users/ana/Desktop/report.pdf".into(),
            },
        ),
        ("transfers", Request::Transfers),
        ("watch_file_offers", Request::WatchFileOffers),
        (
            "file_decision",
            Request::FileDecision {
                transfer: "00112233445566778899aabbccddeeff".into(),
                accept: false,
            },
        ),
        (
            "cancel_transfer",
            Request::CancelTransfer {
                transfer: "00112233".into(),
            },
        ),
        ("clipboard_status", Request::ClipboardStatus),
        (
            "clipboard_send",
            Request::ClipboardSend {
                device: FP.into(),
                sensitive: false,
            },
        ),
        (
            "clipboard_apply",
            Request::ClipboardApply { device: FP.into() },
        ),
        (
            "clipboard_policy",
            Request::ClipboardPolicy {
                device: FP.into(),
                flag: ClipboardFlag::AutoReceive,
                enabled: true,
            },
        ),
        ("notifications_status", Request::NotificationsStatus),
        (
            "notifications_policy",
            Request::NotificationsPolicy {
                device: FP.into(),
                setting: NotificationSetting::Mirror { enabled: true },
            },
        ),
        (
            "notifications_policy_when_locked",
            Request::NotificationsPolicy {
                device: FP.into(),
                setting: NotificationSetting::WhenLocked {
                    policy: "app-only".into(),
                },
            },
        ),
    ])
}

fn responses() -> BTreeMap<&'static str, Response> {
    BTreeMap::from([
        ("status", Response::Status(status())),
        (
            "devices",
            Response::Devices(vec![phone(), revoked_tablet()]),
        ),
        (
            "transfers",
            Response::Transfers(vec![transfer(), failed_transfer()]),
        ),
        ("clipboard", Response::Clipboard(clipboard())),
        ("notifications", Response::Notifications(notifications())),
        ("pong", Response::Pong { rtt_ms: 18 }),
        (
            "ok",
            Response::Ok {
                message: "granted files.v1".into(),
            },
        ),
        (
            "error",
            Response::Error {
                message: "no such device".into(),
            },
        ),
    ])
}

fn events() -> BTreeMap<&'static str, Event> {
    BTreeMap::from([
        (
            "pairing_ready",
            Event::PairingReady {
                payload: "pliwee://pair?v=1&fp=0f0f".into(),
                qr_ascii: "██".into(),
                expires_in_secs: 120,
            },
        ),
        (
            "confirm_request",
            Event::ConfirmRequest {
                device_name: "Galaxy S25".into(),
                device_id: "0123456789abcdef0123456789abcdef".into(),
                fingerprint: FP.into(),
                fingerprint_short: FP_SHORT.into(),
            },
        ),
        (
            "finished",
            Event::Finished {
                status: "paired".into(),
                detail: FP_SHORT.into(),
            },
        ),
        ("transfer_progress", Event::TransferProgress(transfer())),
        (
            "file_approval_ready",
            Event::FileApprovalReady { unattended: false },
        ),
        (
            "file_offer_request",
            Event::FileOfferRequest(FileOfferRequest {
                transfer_id: "00112233445566778899aabbccddeeff".into(),
                device_name: "Galaxy S25".into(),
                device_id: "0123456789abcdef0123456789abcdef".into(),
                fingerprint: FP.into(),
                fingerprint_short: FP_SHORT.into(),
                filename: "holiday photo.jpg".into(),
                size_bytes: 300_000,
                mime_type: "image/jpeg".into(),
            }),
        ),
        (
            "file_offer_withdrawn",
            Event::FileOfferWithdrawn {
                transfer_id: "00112233445566778899aabbccddeeff".into(),
                reason: "the device disconnected".into(),
            },
        ),
    ])
}

fn to_values<T: serde::Serialize>(m: BTreeMap<&'static str, T>) -> serde_json::Map<String, Value> {
    m.into_iter()
        .map(|(k, v)| (k.to_string(), serde_json::to_value(v).expect("serialise")))
        .collect()
}

fn expected() -> Value {
    json!({
        "$comment": [
            "The local control protocol, one example of every message.",
            "Generated by desktop/control/tests/control_fixtures.rs; read by that",
            "test and by macos/Tests/PliweeKitTests. Do not edit by hand:",
            "PLIWEE_BLESS_CONTROL_FIXTURES=1 cargo test -p pliwee-control --test control_fixtures"
        ],
        "requests": to_values(requests()),
        "responses": to_values(responses()),
        "events": to_values(events()),
    })
}

#[test]
fn every_message_serialises_to_the_committed_fixture() {
    let expected = expected();
    let path = fixture_path();
    if std::env::var_os("PLIWEE_BLESS_CONTROL_FIXTURES").is_some() {
        let mut text = serde_json::to_string_pretty(&expected).expect("pretty");
        text.push('\n');
        std::fs::write(&path, text).expect("write fixture");
    }
    let committed: Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display())),
    )
    .expect("the fixture is JSON");
    assert_eq!(
        committed,
        expected,
        "the control protocol no longer matches {}; if the change is \
         deliberate, regenerate it (see this file's header) and update \
         Pliwee.app's models to match",
        path.display()
    );
}

#[test]
fn every_fixture_message_parses_back_into_the_same_message() {
    // The other direction: what the fixture says, Rust reads. This is the
    // half that catches a Swift-shaped message the agent would refuse.
    let committed: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path()).expect("read"))
            .expect("JSON");
    for (name, value) in committed["requests"].as_object().expect("requests") {
        let parsed: Request =
            serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("request {name}: {e}"));
        assert_eq!(&serde_json::to_value(parsed).expect("re-serialise"), value);
    }
    for (name, value) in committed["responses"].as_object().expect("responses") {
        let parsed: Response = serde_json::from_value(value.clone())
            .unwrap_or_else(|e| panic!("response {name}: {e}"));
        assert_eq!(&serde_json::to_value(parsed).expect("re-serialise"), value);
    }
    for (name, value) in committed["events"].as_object().expect("events") {
        let parsed: Event =
            serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("event {name}: {e}"));
        assert_eq!(&serde_json::to_value(parsed).expect("re-serialise"), value);
    }
}

#[test]
fn every_request_kind_is_in_the_fixture() {
    // A new `Request` variant must appear in the fixture, or Pliwee.app has
    // no example of it to be tested against. The match is exhaustive, so a
    // new variant fails to compile here until it is given a fixture name.
    let names: Vec<&str> = requests().keys().copied().collect();
    for request in requests().values() {
        let tag = match request {
            Request::Status => "status",
            Request::Devices => "devices",
            Request::Pair { .. } => "pair",
            Request::Confirm { .. } => "confirm",
            Request::Unpair { .. } => "unpair",
            Request::HideRevokedDevice { .. } => "hide_revoked_device",
            Request::HideAllRevokedDevices => "hide_all_revoked_devices",
            Request::Ping { .. } => "ping",
            Request::Grant { .. } => "grant",
            Request::Send { .. } => "send",
            Request::Transfers => "transfers",
            Request::WatchFileOffers => "watch_file_offers",
            Request::FileDecision { .. } => "file_decision",
            Request::CancelTransfer { .. } => "cancel_transfer",
            Request::ClipboardStatus => "clipboard_status",
            Request::ClipboardSend { .. } => "clipboard_send",
            Request::ClipboardApply { .. } => "clipboard_apply",
            Request::ClipboardPolicy { .. } => "clipboard_policy",
            Request::NotificationsStatus => "notifications_status",
            Request::NotificationsPolicy { .. } => "notifications_policy",
        };
        assert!(names.contains(&tag), "{tag}");
    }
    assert!(names.len() >= 20);
}
