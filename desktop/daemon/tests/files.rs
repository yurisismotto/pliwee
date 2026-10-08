//! `files.v1` end to end, and the attacks it must refuse.
//!
//! Every test here runs the real capability over a real TLS data stream with
//! real pinning and the real MAC. Nothing is stubbed to make a rejection
//! happen: a test that expects a refusal gets it from the code that ships.
//!
//! The `F` numbers refer to the sprint's security matrix.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use pliwee_capability_files::auth::{compute_stream_mac, StreamChallenge};
use pliwee_capability_files::transfer::{FailureReason, TransferId, TransferState};
use pliwee_capability_files::{limits, stream};

const GRACE: Duration = Duration::from_secs(20);

/// A paired phone whose desktop has granted it `files.v1`.
///
/// Note the reconnection. A session's effective capability set is fixed at
/// handshake time, so *widening* a grant takes effect on the next connection —
/// which is the real flow too: `pliwee pair`, then `pliwee grant`, then the
/// phone reconnects. *Narrowing* is immediate, and that asymmetry is the safe
/// direction: see `f15_*` and `f1_a_withdrawn_grant_*`.
async fn paired(server: &TestServer, phone: &TestClient) -> ConnectedSession {
    let token = server.open_pairing(Duration::from_secs(30)).await;
    let first = phone
        .connect(server.addr, server.fingerprint, Some(&token))
        .await
        .expect("pairing");
    server.set_grant(phone.fingerprint, "files.v1", true).await;
    first.close().await;

    phone
        .connect(server.addr, server.fingerprint, None)
        .await
        .expect("reconnect with the grant in place")
}

/// A paired phone that was deliberately *not* granted `files.v1`.
async fn paired_ungranted(server: &TestServer, phone: &TestClient) -> ConnectedSession {
    let token = server.open_pairing(Duration::from_secs(30)).await;
    phone
        .connect(server.addr, server.fingerprint, Some(&token))
        .await
        .expect("pairing")
}

// ---------------------------------------------------------------------------
// The happy paths
// ---------------------------------------------------------------------------

/// FILE-02, in-process: Android -> Fedora.
#[tokio::test]
async fn a_phone_sends_a_file_and_the_desktop_verifies_and_stores_it() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("holiday photo.jpg");
    let payload = write_sample_file(&source, 300_000);

    let id = phone
        .transfers
        .offer_file(server.fingerprint, source)
        .await
        .expect("offer");

    let snapshot = wait_for_terminal(&phone.transfers, id, GRACE).await;
    assert_eq!(
        snapshot.state,
        TransferState::Completed,
        "sender should see the receiver's confirmation: {:?}",
        snapshot.failure
    );

    // The file is on disk, under its own name, byte for byte.
    let stored = server.downloads.join("holiday photo.jpg");
    assert!(
        stored.exists(),
        "stored files: {:?}",
        server.received_files()
    );
    assert_eq!(std::fs::read(&stored).expect("read"), payload);
    assert_eq!(
        sha256_of(&std::fs::read(&stored).expect("read")),
        sha256_of(&payload)
    );

    // And the receiver's own record agrees.
    let received = server
        .transfers
        .snapshot_one(id)
        .await
        .expect("the desktop knows this transfer");
    assert_eq!(received.state, TransferState::Completed);
    assert_eq!(received.bytes_transferred, payload.len() as u64);
    assert_eq!(received.percentage(), Some(100));

    assert!(
        server.partial_files().is_empty(),
        "no .part file may survive"
    );
    session.close().await;
}

/// FILE-03, in-process: Fedora -> Android.
///
/// Note which side dials: the phone does, in both directions. The desktop is
/// the sender here and still accepts the data stream, because a phone is not
/// a listener.
#[tokio::test]
async fn the_desktop_sends_a_file_and_the_phone_verifies_and_stores_it() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("report.pdf");
    let payload = write_sample_file(&source, 250_000);

    let id = server
        .transfers
        .offer_file(phone.fingerprint, source)
        .await
        .expect("offer");

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(
        snapshot.state,
        TransferState::Completed,
        "{:?}",
        snapshot.failure
    );

    let stored = phone.downloads.join("report.pdf");
    assert!(stored.exists(), "stored: {:?}", phone.received_files());
    assert_eq!(std::fs::read(&stored).expect("read"), payload);
    session.close().await;
}

/// FILE-13: a file far larger than the copy buffer moves fine, which is what
/// streaming means. The buffer is a compile-time constant, so the memory a
/// transfer costs does not scale with the file.
#[tokio::test]
async fn a_file_much_larger_than_the_copy_buffer_transfers_intact() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("big.bin");
    // 128 buffers' worth. If anything buffered the whole file this would
    // still pass, but the constant below is what actually bounds it.
    let size = limits::COPY_BUFFER_BYTES * 128;
    let payload = write_sample_file(&source, size);
    // The bound that makes this a streaming transfer rather than a buffered
    // one, asserted at compile time so it cannot drift.
    const _: () = assert!(limits::COPY_BUFFER_BYTES <= 64 * 1024);

    let id = phone
        .transfers
        .offer_file(server.fingerprint, source)
        .await
        .expect("offer");

    let snapshot = wait_for_terminal(&phone.transfers, id, Duration::from_secs(60)).await;
    assert_eq!(
        snapshot.state,
        TransferState::Completed,
        "{:?}",
        snapshot.failure
    );
    assert_eq!(
        sha256_of(&std::fs::read(server.downloads.join("big.bin")).expect("read")),
        sha256_of(&payload)
    );
    session.close().await;
}

/// FILE-12: the data stream works over IPv6 as well as IPv4, because it uses
/// the listener that was already certified for both.
#[tokio::test]
async fn a_transfer_works_over_both_address_families() {
    let server = TestServer::start_dual_stack().await;
    if !server.families.ipv6 {
        eprintln!("host has no IPv6 listener; skipping the v6 half");
    }

    for addr in [
        std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, server.addr.port())),
        std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, server.addr.port())),
    ] {
        if addr.is_ipv6() && !server.families.ipv6 {
            continue;
        }

        let phone = TestClient::new("phone");
        let token = server.open_pairing(Duration::from_secs(30)).await;
        let first = phone
            .connect(addr, server.fingerprint, Some(&token))
            .await
            .unwrap_or_else(|e| panic!("pairing over {addr}: {e}"));
        server.set_grant(phone.fingerprint, "files.v1", true).await;
        first.close().await;
        let session = phone
            .connect(addr, server.fingerprint, None)
            .await
            .unwrap_or_else(|e| panic!("reconnecting over {addr}: {e}"));

        let dir = tempfile::tempdir().expect("tempdir");
        let name = if addr.is_ipv4() {
            "over-v4.bin"
        } else {
            "over-v6.bin"
        };
        let source = dir.path().join(name);
        let payload = write_sample_file(&source, 40_000);

        let id = phone
            .transfers
            .offer_file(server.fingerprint, source)
            .await
            .expect("offer");
        let snapshot = wait_for_terminal(&phone.transfers, id, GRACE).await;
        assert_eq!(
            snapshot.state,
            TransferState::Completed,
            "transfer over {addr} failed: {:?}",
            snapshot.failure
        );
        assert_eq!(
            std::fs::read(server.downloads.join(name)).expect("read"),
            payload
        );
        session.close().await;
    }
}

/// FILE-10: a second file with the same name never overwrites the first.
#[tokio::test]
async fn a_second_file_with_the_same_name_is_numbered_not_overwritten() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut digests = Vec::new();

    for round in 0..3 {
        let source = dir.path().join(format!("copy{round}"));
        // Different content each time, so an overwrite would be detectable.
        let payload = write_sample_file(&source, 1000 + round * 7);
        digests.push(sha256_of(&payload));

        let staged = dir.path().join("photo.jpg");
        std::fs::rename(&source, &staged).expect("rename");

        let id = phone
            .transfers
            .offer_file(server.fingerprint, staged)
            .await
            .expect("offer");
        let snapshot = wait_for_terminal(&phone.transfers, id, GRACE).await;
        assert_eq!(
            snapshot.state,
            TransferState::Completed,
            "{:?}",
            snapshot.failure
        );
    }

    let names = server.received_files();
    assert!(names.contains(&"photo.jpg".to_string()), "{names:?}");
    assert!(names.contains(&"photo (1).jpg".to_string()), "{names:?}");
    assert!(names.contains(&"photo (2).jpg".to_string()), "{names:?}");

    // Each file kept its own content: nothing clobbered anything.
    let mut found: Vec<[u8; 32]> = ["photo.jpg", "photo (1).jpg", "photo (2).jpg"]
        .iter()
        .map(|n| sha256_of(&std::fs::read(server.downloads.join(n)).expect("read")))
        .collect();
    found.sort();
    digests.sort();
    assert_eq!(found, digests);

    session.close().await;
}

/// FILE-01: capability negotiation. `files.v1` is advertised but not granted
/// by default, and the difference is load-bearing.
#[tokio::test]
async fn files_is_advertised_but_never_granted_automatically() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired_ungranted(&server, &phone).await;

    // Both sides implement it, so it is mutually supported...
    assert!(session
        .negotiated_capabilities
        .contains(&"files.v1".to_string()));

    // ...and the desktop still has not granted it.
    let store = server.state.store.lock().await;
    let record = store
        .peer_record(&phone.fingerprint)
        .expect("paired")
        .clone();
    drop(store);
    assert!(
        !record.allows("files.v1"),
        "files.v1 must never be auto-granted: {:?}",
        record.granted_capabilities
    );
    assert!(record.allows("battery.v1"), "battery.v1 is auto-granted");

    session.close().await;
}

// ---------------------------------------------------------------------------
// F1 — a peer with no files.v1 grant
// ---------------------------------------------------------------------------

/// The transport-layer half: an ungranted peer's `files.v1` message never
/// even reaches the capability.
#[tokio::test]
async fn f1_an_offer_from_a_peer_without_a_grant_never_reaches_the_capability() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired_ungranted(&server, &phone).await;

    let id = TransferId::from_bytes(&[0x11; 16]).expect("id");
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "sneaky.txt".into(),
            size_bytes: 10,
            mime_type: String::new(),
            sha256: vec![0; 32],
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    // The session refuses it as an un-negotiated capability and replies with
    // a non-fatal ERROR envelope, which is not a `files.v1` message — so the
    // capture stays silent.
    assert!(
        captured
            .next_control_or_silence(Duration::from_millis(500))
            .await
            .is_none(),
        "an ungranted peer must get no files.v1 traffic at all"
    );

    // No record was created, so an ungranted peer cannot even consume a slot.
    assert!(server.transfers.snapshot().await.is_empty());
    // Nobody was asked to approve anything: the check ran long before a human
    // would have been bothered.
    assert_eq!(server.approvals.asked(), 0);
    assert!(server.received_files().is_empty());

    // And the refusal is non-fatal: one ungranted message must not cost the
    // user their connection.
    assert!(
        session.handle.ping(Duration::from_secs(5)).await.is_some(),
        "the session must survive an un-negotiated capability message"
    );

    session.close().await;
}

/// The capability-layer half: the receiver re-checks the grant itself, so a
/// permission withdrawn *after* the handshake is still enforced.
///
/// This is the check that cannot be skipped by trusting the handshake
/// snapshot, and it is what makes "never trust only what the sender declared"
/// true rather than aspirational.
#[tokio::test]
async fn f1_a_withdrawn_grant_is_enforced_by_the_receiver_within_the_session() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    // The grant is withdrawn while the session stays open. The session's own
    // capability list still contains files.v1 — it was fixed at handshake —
    // so the message *does* reach the capability, and the capability must
    // refuse it on its own.
    server.set_grant(phone.fingerprint, "files.v1", false).await;

    let id = TransferId::from_bytes(&[0x12; 16]).expect("id");
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "after-withdrawal.txt".into(),
            size_bytes: 10,
            mime_type: String::new(),
            sha256: vec![0; 32],
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Reject(r)) => {
            assert_eq!(r.reason(), pb::TransferFailureReason::NotAuthorized);
        }
        other => panic!("expected a rejection from the capability, got {other:?}"),
    }

    assert!(server.transfers.snapshot().await.is_empty());
    assert_eq!(server.approvals.asked(), 0);
    assert!(server.received_files().is_empty());
    session.close().await;
}

#[tokio::test]
async fn f1_a_desktop_cannot_offer_to_a_peer_it_has_not_granted() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired_ungranted(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("nope.txt");
    write_sample_file(&source, 100);

    let result = server.transfers.offer_file(phone.fingerprint, source).await;
    assert!(
        result.is_err(),
        "an ungranted peer must not be offered a file"
    );
    session.close().await;
}

// ---------------------------------------------------------------------------
// F2 — an unknown peer opens a data stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f2_a_stranger_cannot_open_a_data_stream() {
    let server = TestServer::start().await;

    // A perfectly valid identity that has simply never paired. TLS will
    // succeed — the server cannot pin an unknown client — and everything
    // after it must refuse.
    let stranger = TestClient::new("stranger");
    let mut io = open_data_stream(server.addr, &stranger.identity, server.fingerprint)
        .await
        .expect("TLS to a listener that accepts unknown clients");

    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: vec![0x22; 16],
            mac: vec![0x33; 32],
        },
    )
    .await
    .expect("write");

    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
    assert_eq!(ready.status(), pb::DataStreamStatus::Rejected);
    assert_eq!(ready.reason(), pb::TransferFailureReason::NotAuthorized);
    assert!(server.completed_files().is_empty());
}

// ---------------------------------------------------------------------------
// F3 — a different paired device tries to take over a transfer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f3_another_paired_device_cannot_attach_to_someone_elses_transfer() {
    let server = TestServer::start().await;

    // Two phones, both paired, both granted files.v1.
    let (alice, alice_captured) = TestClient::new_raw("alice");
    let alice_session = paired(&server, &alice).await;

    let mallory = TestClient::new("mallory");
    let mallory_session = paired(&server, &mallory).await;
    assert_ne!(alice.fingerprint, mallory.fingerprint);

    // Alice negotiates a transfer and learns its challenge.
    let id = TransferId::from_bytes(&[0x44; 16]).expect("id");
    let payload = b"alice's bytes".to_vec();
    send_files_control(
        &alice_session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "alice.txt".into(),
            size_bytes: payload.len() as u64,
            mime_type: String::new(),
            sha256: sha256_of(&payload).to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    let accept = match alice_captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Accept(a)) => a,
        other => panic!("expected an acceptance, got {other:?}"),
    };
    let challenge =
        StreamChallenge::from_bytes(&accept.stream_challenge).expect("a 32-byte challenge");

    // Now the worst case: Mallory has somehow learned both the transfer id
    // and the challenge, and forges the MAC exactly as Alice would — naming
    // Alice as the dialer. The only thing Mallory cannot forge is the TLS
    // identity on the socket, and that is what must stop her.
    let forged = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &alice.fingerprint,
        &id,
    );

    let mut io = open_data_stream(server.addr, &mallory.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: forged.to_vec(),
        },
    )
    .await
    .expect("write");

    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
    assert_eq!(
        ready.status(),
        pb::DataStreamStatus::Rejected,
        "a transfer belongs to the identity that negotiated it"
    );

    // A MAC computed honestly for Mallory's own identity fails too: the
    // challenge is keyed to a transfer that is not hers.
    let own = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &mallory.fingerprint,
        &id,
    );
    let mut io2 = open_data_stream(server.addr, &mallory.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io2,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: own.to_vec(),
        },
    )
    .await
    .expect("write");
    let ready2: pb::DataStreamReady = stream::read_frame(&mut io2).await.expect("reply");
    assert_eq!(ready2.status(), pb::DataStreamStatus::Rejected);

    // Alice's transfer is untouched and still waiting for her.
    let snapshot = server
        .transfers
        .snapshot_one(id)
        .await
        .expect("still there");
    assert_eq!(snapshot.state, TransferState::Transferring);
    // Alice's transfer has a `.part` file open, as it should; what must not
    // exist is a promoted file, because no bytes were ever accepted.
    assert!(server.completed_files().is_empty());

    alice_session.close().await;
    mallory_session.close().await;
}

// ---------------------------------------------------------------------------
// F4 — a guessed transfer id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f4_a_guessed_transfer_id_is_not_a_bearer_token() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0x55; 16]).expect("id");
    let payload = b"secret".to_vec();
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "x.txt".into(),
            size_bytes: payload.len() as u64,
            mime_type: String::new(),
            sha256: sha256_of(&payload).to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;
    let _ = captured.next_control(GRACE).await;

    // The right transfer id, from the right peer, over the right TLS
    // connection — and no proof of the challenge. This is the case that
    // decides whether the id is a bearer token. It must not be.
    for mac in [vec![0u8; 32], vec![0xff; 32], Vec::new(), vec![1; 64]] {
        let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
            .await
            .expect("TLS");
        stream::write_frame(
            &mut io,
            &pb::DataStreamAuth {
                protocol_version: 1,
                transfer_id: id.to_vec(),
                mac,
            },
        )
        .await
        .expect("write");

        let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
        assert_eq!(
            ready.status(),
            pb::DataStreamStatus::Rejected,
            "knowing a transfer id must not be enough"
        );
    }

    // Every attempt failed, and the legitimate dialer's one chance survives.
    let snapshot = server.transfers.snapshot_one(id).await.expect("alive");
    assert_eq!(snapshot.state, TransferState::Transferring);
    assert!(server.completed_files().is_empty());
    session.close().await;
}

// ---------------------------------------------------------------------------
// F5 — a reused transfer id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f5_a_transfer_id_cannot_be_reused_for_a_second_offer() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0x66; 16]).expect("id");
    let offer = pb::FileOffer {
        transfer_id: id.to_vec(),
        filename: "first.txt".into(),
        size_bytes: 4,
        mime_type: String::new(),
        sha256: sha256_of(b"abcd").to_vec(),
        timestamp_unix_ms: 0,
    };

    send_files_control(&session, pb::file_control::Body::Offer(offer.clone())).await;
    let first = captured.next_control(GRACE).await;
    assert!(
        matches!(first.body, Some(pb::file_control::Body::Accept(_))),
        "the first offer is fine: {:?}",
        first.body
    );

    // The same id again, with different metadata. Allowing it would let a
    // peer redefine a live transfer.
    let mut second = offer.clone();
    second.filename = "second.txt".into();
    second.size_bytes = 999;
    send_files_control(&session, pb::file_control::Body::Offer(second)).await;

    match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Reject(r)) => {
            assert_eq!(r.reason(), pb::TransferFailureReason::BadMetadata);
        }
        other => panic!("a reused transfer id must be refused, got {other:?}"),
    }

    // The original transfer kept its own metadata.
    let snapshot = server.transfers.snapshot_one(id).await.expect("alive");
    assert_eq!(snapshot.filename, "first.txt");
    assert_eq!(snapshot.size_bytes, 4);

    session.close().await;
}

#[tokio::test]
async fn f5_a_challenge_is_single_use_so_a_second_stream_is_refused() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0x77; 16]).expect("id");
    let payload = b"replayable?".to_vec();
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "once.txt".into(),
            size_bytes: payload.len() as u64,
            mime_type: String::new(),
            sha256: sha256_of(&payload).to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    let accept = match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Accept(a)) => a,
        other => panic!("expected acceptance, got {other:?}"),
    };
    let challenge = StreamChallenge::from_bytes(&accept.stream_challenge).expect("challenge");
    let mac = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &phone.fingerprint,
        &id,
    );

    // First stream: accepted, and it completes the transfer.
    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: mac.to_vec(),
        },
    )
    .await
    .expect("write");
    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
    assert_eq!(ready.status(), pb::DataStreamStatus::Ready);

    use tokio::io::AsyncWriteExt;
    io.write_all(&payload).await.expect("bytes");
    io.shutdown().await.expect("shutdown");

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(snapshot.state, TransferState::Completed);

    // Second stream, replaying the very same authentication frame verbatim.
    let mut replay = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut replay,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: mac.to_vec(),
        },
    )
    .await
    .expect("write");
    let ready2: pb::DataStreamReady = stream::read_frame(&mut replay).await.expect("reply");
    assert_eq!(
        ready2.status(),
        pb::DataStreamStatus::Rejected,
        "a replayed data-stream authentication must not work"
    );

    // The stored file was not touched a second time.
    assert_eq!(server.received_files(), vec!["once.txt".to_string()]);
    assert_eq!(
        std::fs::read(server.downloads.join("once.txt")).expect("read"),
        payload
    );

    session.close().await;
}

// ---------------------------------------------------------------------------
// F6 — an expired transfer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f6_a_transfer_nobody_dialled_expires_and_stops_being_usable() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0x88; 16]).expect("id");
    let payload = b"too slow".to_vec();
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "stale.txt".into(),
            size_bytes: payload.len() as u64,
            mime_type: String::new(),
            sha256: sha256_of(&payload).to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    let accept = match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Accept(a)) => a,
        other => panic!("expected acceptance, got {other:?}"),
    };
    let challenge = StreamChallenge::from_bytes(&accept.stream_challenge).expect("challenge");
    let mac = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &phone.fingerprint,
        &id,
    );

    // Nobody dials. The harness configures a short `stream_open_timeout`, so
    // this exercises the production reaper rather than a test shortcut.
    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(snapshot.state, TransferState::Failed);
    assert_eq!(snapshot.failure, Some(FailureReason::TimedOut));

    // The challenge it was holding is gone, so a late dialer with a
    // *correct* MAC still gets nothing.
    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: mac.to_vec(),
        },
    )
    .await
    .expect("write");
    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
    assert_eq!(ready.status(), pb::DataStreamStatus::Rejected);

    assert!(
        server.partial_files().is_empty(),
        "the temp file must be gone"
    );
    assert!(server.completed_files().is_empty());
    session.close().await;
}

// ---------------------------------------------------------------------------
// A hostile offer, driven by hand
// ---------------------------------------------------------------------------

/// Runs one transfer from a raw client that controls every field.
///
/// Returns the desktop's final `files.v1` message. `bytes_to_send` is what
/// actually goes down the data stream, which the caller may deliberately make
/// disagree with `size_bytes` — that is the point of several tests below.
#[allow(clippy::too_many_arguments)]
async fn hostile_transfer(
    server: &TestServer,
    phone: &TestClient,
    captured: &Captured,
    session: &ConnectedSession,
    id: TransferId,
    filename: &str,
    size_bytes: u64,
    sha256: [u8; 32],
    bytes_to_send: &[u8],
) -> pb::FileControl {
    send_files_control(
        session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: filename.into(),
            size_bytes,
            mime_type: String::new(),
            sha256: sha256.to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    let first = captured.next_control(GRACE).await;
    let accept = match first.body {
        Some(pb::file_control::Body::Accept(a)) => a,
        // A refusal before the stream is a legitimate outcome; hand it back.
        _ => return first,
    };

    let challenge = StreamChallenge::from_bytes(&accept.stream_challenge).expect("challenge");
    let mac = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &phone.fingerprint,
        &id,
    );

    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: mac.to_vec(),
        },
    )
    .await
    .expect("write auth");

    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("ready");
    assert_eq!(
        ready.status(),
        pb::DataStreamStatus::Ready,
        "the stream itself should be accepted here: {:?}",
        ready.reason()
    );

    use tokio::io::AsyncWriteExt;
    let _ = io.write_all(bytes_to_send).await;
    let _ = io.shutdown().await;

    captured.next_control(GRACE).await
}

// ---------------------------------------------------------------------------
// F7 / F8 — hostile filenames
// ---------------------------------------------------------------------------

/// F7 and F8 together: whatever a peer calls a file, it lands in the download
/// directory under a bare name, and nothing is written anywhere else.
#[tokio::test]
async fn f7_f8_a_hostile_filename_cannot_escape_the_download_directory() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    // A canary outside the download directory. If any of these names escaped,
    // this is what they would reach.
    let outside = server
        .downloads
        .parent()
        .expect("parent")
        .join("victim.txt");
    std::fs::write(&outside, b"untouched").expect("write canary");

    let cases: [(&str, &str); 6] = [
        // F7: relative traversal, both separator flavours.
        ("../../victim.txt", "victim.txt"),
        ("..\\..\\victim.txt", "victim.txt"),
        ("subdir/../../../victim.txt", "victim.txt"),
        // F8: absolute paths.
        ("/etc/cron.d/omnibridge", "omnibridge"),
        ("/home/yuri/.bashrc", ".bashrc"),
        ("C:\\Windows\\System32\\drivers\\etc\\hosts", "hosts"),
    ];

    for (index, (hostile, expected_basename)) in cases.into_iter().enumerate() {
        let id = TransferId::from_bytes(&[0xa0 + index as u8; 16]).expect("id");
        let payload = format!("payload {index}").into_bytes();

        let reply = hostile_transfer(
            &server,
            &phone,
            &captured,
            &session,
            id,
            hostile,
            payload.len() as u64,
            sha256_of(&payload),
            &payload,
        )
        .await;

        assert!(
            matches!(reply.body, Some(pb::file_control::Body::Complete(_))),
            "{hostile:?} should be accepted under a safe name, got {:?}",
            reply.body
        );

        // The transfer's own record shows the sanitized name, not the raw one.
        let snapshot = server.transfers.snapshot_one(id).await.expect("record");
        assert_eq!(snapshot.filename, expected_basename, "for {hostile:?}");
        assert_eq!(snapshot.state, TransferState::Completed);

        // Whatever was stored is a direct child of the download directory.
        let stored = snapshot.stored_at.expect("a stored path");
        assert_eq!(
            stored.parent(),
            Some(server.downloads.as_path()),
            "{hostile:?} was written outside the download directory: {stored:?}"
        );
        assert_eq!(std::fs::read(&stored).expect("read"), payload);
    }

    // The canary is untouched, and no directory was created either.
    assert_eq!(std::fs::read(&outside).expect("read"), b"untouched");
    assert!(
        std::fs::read_dir(&server.downloads)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .all(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false)),
        "no subdirectory may be created in the download directory"
    );

    session.close().await;
}

/// A name that sanitizes to nothing at all is refused rather than renamed.
///
/// Inventing a name for a file whose own name was hostile would hide the
/// attack from the user.
#[tokio::test]
async fn a_filename_that_sanitizes_to_nothing_is_rejected() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    for (index, hostile) in ["..", ".", "/", "../../", "", "CON", "\u{0}"]
        .into_iter()
        .enumerate()
    {
        let id = TransferId::from_bytes(&[0xb0 + index as u8; 16]).expect("id");
        send_files_control(
            &session,
            pb::file_control::Body::Offer(pb::FileOffer {
                transfer_id: id.to_vec(),
                filename: hostile.into(),
                size_bytes: 4,
                mime_type: String::new(),
                sha256: sha256_of(b"abcd").to_vec(),
                timestamp_unix_ms: 0,
            }),
        )
        .await;

        match captured.next_control(GRACE).await.body {
            Some(pb::file_control::Body::Reject(r)) => {
                assert_eq!(
                    r.reason(),
                    pb::TransferFailureReason::BadMetadata,
                    "for {hostile:?}"
                );
            }
            other => panic!("{hostile:?} should be rejected, got {other:?}"),
        }
        // Refused before a human was involved.
        assert_eq!(server.approvals.asked(), 0);
    }

    assert!(server.completed_files().is_empty());
    session.close().await;
}

// ---------------------------------------------------------------------------
// F9 — the hash does not match
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f9_a_file_whose_hash_does_not_match_is_discarded_not_stored() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xc1; 16]).expect("id");
    let honest = b"what the offer promised".to_vec();
    let actual = b"what was actually sent!".to_vec();
    assert_eq!(honest.len(), actual.len(), "same length, different bytes");

    let reply = hostile_transfer(
        &server,
        &phone,
        &captured,
        &session,
        id,
        "swapped.bin",
        honest.len() as u64,
        // The offer promises one file and the stream delivers another.
        sha256_of(&honest),
        &actual,
    )
    .await;

    match reply.body {
        Some(pb::file_control::Body::Failed(f)) => {
            assert_eq!(f.reason(), pb::TransferFailureReason::Integrity);
        }
        other => panic!("expected an integrity failure, got {other:?}"),
    }

    let snapshot = server.transfers.snapshot_one(id).await.expect("record");
    assert_eq!(snapshot.state, TransferState::Failed);
    assert_eq!(snapshot.failure, Some(FailureReason::Integrity));
    assert!(snapshot.stored_at.is_none());

    // Nothing was promoted, and the bad bytes are gone from disk entirely.
    assert!(
        server.completed_files().is_empty(),
        "a file that failed its hash must never appear: {:?}",
        server.completed_files()
    );
    assert!(
        server.partial_files().is_empty(),
        "the partial file must be deleted, not left behind: {:?}",
        server.partial_files()
    );

    session.close().await;
}

// ---------------------------------------------------------------------------
// F10 / F12 — a stream that stops early
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f10_f12_a_truncated_stream_fails_and_leaves_nothing_behind() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xc2; 16]).expect("id");
    let full = write_sample_bytes(4000);

    let reply = hostile_transfer(
        &server,
        &phone,
        &captured,
        &session,
        id,
        "cut-short.bin",
        full.len() as u64,
        sha256_of(&full),
        // Fewer bytes than promised, then a clean close.
        &full[..1500],
    )
    .await;

    match reply.body {
        Some(pb::file_control::Body::Failed(f)) => {
            assert_eq!(f.reason(), pb::TransferFailureReason::Integrity);
        }
        other => panic!("a short stream must fail, got {other:?}"),
    }

    assert!(server.completed_files().is_empty());
    assert!(server.partial_files().is_empty());
    session.close().await;
}

// ---------------------------------------------------------------------------
// F11 — a stream that keeps going past its own offer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f11_a_stream_longer_than_its_offer_is_refused() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xc3; 16]).expect("id");
    let promised = write_sample_bytes(1000);
    let mut oversized = promised.clone();
    oversized.extend_from_slice(&write_sample_bytes(5000));

    let reply = hostile_transfer(
        &server,
        &phone,
        &captured,
        &session,
        id,
        "overflowing.bin",
        // The offer says 1000 bytes; 6000 are written.
        promised.len() as u64,
        sha256_of(&promised),
        &oversized,
    )
    .await;

    match reply.body {
        Some(pb::file_control::Body::Failed(f)) => {
            assert_eq!(f.reason(), pb::TransferFailureReason::Integrity);
        }
        other => panic!("an oversized stream must fail, got {other:?}"),
    }

    let snapshot = server.transfers.snapshot_one(id).await.expect("record");
    // Only the declared number of bytes was ever accepted, so the overflow
    // could not have reached the disk even transiently.
    assert_eq!(snapshot.bytes_transferred, promised.len() as u64);
    assert!(server.completed_files().is_empty());
    assert!(server.partial_files().is_empty());

    session.close().await;
}

// ---------------------------------------------------------------------------
// F16 — oversized metadata
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f16_oversized_metadata_is_refused_without_allocating_for_it() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    struct Case {
        name: &'static str,
        offer: pb::FileOffer,
        expect: pb::TransferFailureReason,
    }

    let base = |id: u8| pb::FileOffer {
        transfer_id: vec![id; 16],
        filename: "ok.txt".into(),
        size_bytes: 4,
        mime_type: String::new(),
        sha256: sha256_of(b"abcd").to_vec(),
        timestamp_unix_ms: 0,
    };

    let cases = vec![
        Case {
            name: "a filename of tens of kilobytes",
            offer: pb::FileOffer {
                filename: "A".repeat(40_000),
                ..base(0xd1)
            },
            expect: pb::TransferFailureReason::BadMetadata,
        },
        Case {
            name: "an over-long MIME type",
            offer: pb::FileOffer {
                mime_type: "x/".to_string() + &"y".repeat(limits::MAX_MIME_TYPE_BYTES),
                ..base(0xd2)
            },
            expect: pb::TransferFailureReason::BadMetadata,
        },
        Case {
            name: "a hash that is not 32 bytes",
            offer: pb::FileOffer {
                sha256: vec![0; 31],
                ..base(0xd3)
            },
            expect: pb::TransferFailureReason::BadMetadata,
        },
        Case {
            name: "an empty hash",
            offer: pb::FileOffer {
                sha256: Vec::new(),
                ..base(0xd4)
            },
            expect: pb::TransferFailureReason::BadMetadata,
        },
        Case {
            name: "a file larger than this machine accepts",
            offer: pb::FileOffer {
                size_bytes: u64::MAX,
                ..base(0xd5)
            },
            expect: pb::TransferFailureReason::TooLarge,
        },
    ];

    for case in cases {
        send_files_control(&session, pb::file_control::Body::Offer(case.offer)).await;
        match captured.next_control(GRACE).await.body {
            Some(pb::file_control::Body::Reject(r)) => {
                assert_eq!(r.reason(), case.expect, "for {}", case.name);
            }
            other => panic!("{} should be rejected, got {other:?}", case.name),
        }
    }

    // Not one of them reached a human or created a record.
    assert_eq!(server.approvals.asked(), 0);
    assert!(server.transfers.snapshot().await.is_empty());
    assert!(server.received_files().is_empty());

    // A malformed transfer id is a protocol error, not an offer to refuse.
    // The session survives it.
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: vec![0; 8],
            ..base(0xd6)
        }),
    )
    .await;
    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());

    session.close().await;
}

// ---------------------------------------------------------------------------
// F17 — a duplicate FILE_COMPLETE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f17_a_duplicate_file_complete_changes_nothing() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    // The desktop sends; the raw phone answers the control protocol by hand
    // so it can send FILE_COMPLETE twice.
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("twice.bin");
    let payload = write_sample_file(&source, 2000);

    let id = server
        .transfers
        .offer_file(phone.fingerprint, source)
        .await
        .expect("offer");

    // Offer -> accept -> ready.
    match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Offer(o)) => {
            assert_eq!(o.filename, "twice.bin");
            assert_eq!(o.size_bytes, payload.len() as u64);
            assert_eq!(o.sha256, sha256_of(&payload).to_vec());
        }
        other => panic!("expected an offer, got {other:?}"),
    }
    send_files_control(
        &session,
        pb::file_control::Body::Accept(pb::FileAccept {
            transfer_id: id.to_vec(),
            stream_challenge: Vec::new(),
        }),
    )
    .await;

    let ready = match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Ready(r)) => r,
        other => panic!("expected FILE_READY, got {other:?}"),
    };
    let challenge = StreamChallenge::from_bytes(&ready.stream_challenge).expect("challenge");
    let mac = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &phone.fingerprint,
        &id,
    );

    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: mac.to_vec(),
        },
    )
    .await
    .expect("auth");
    let ok: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("ready");
    assert_eq!(ok.status(), pb::DataStreamStatus::Ready);

    // Read the file the desktop sends us.
    use tokio::io::AsyncReadExt;
    let mut received = Vec::new();
    io.read_to_end(&mut received).await.expect("read");
    assert_eq!(received, payload);

    // First completion: accepted.
    send_files_control(
        &session,
        pb::file_control::Body::Complete(pb::FileComplete {
            transfer_id: id.to_vec(),
        }),
    )
    .await;
    let first = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(first.state, TransferState::Completed);

    // Second and third completions: the transfer is terminal, so the state
    // machine refuses the transition and nothing changes.
    for _ in 0..2 {
        send_files_control(
            &session,
            pb::file_control::Body::Complete(pb::FileComplete {
                transfer_id: id.to_vec(),
            }),
        )
        .await;
    }
    // A cancel after completion must not resurrect or reopen it either.
    send_files_control(
        &session,
        pb::file_control::Body::Cancel(pb::FileCancel {
            transfer_id: id.to_vec(),
            reason: pb::TransferFailureReason::CancelledByUser as i32,
        }),
    )
    .await;

    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());
    let after = server.transfers.snapshot_one(id).await.expect("record");
    assert_eq!(after.state, TransferState::Completed, "still completed");
    assert_eq!(after.failure, None, "and still not a failure");

    session.close().await;
}

/// A challenge cannot exist before both sides have agreed, so a data stream
/// cannot be opened ahead of the receiver's approval.
///
/// This is why `FileOffer` has no `stream_challenge` field — the type makes
/// the early-dial case unrepresentable — and why `FileReady` exists.
#[tokio::test]
async fn a_data_stream_cannot_be_opened_before_the_transfer_is_agreed() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("early.bin");
    write_sample_file(&source, 1000);

    let id = server
        .transfers
        .offer_file(phone.fingerprint, source)
        .await
        .expect("offer");

    // The offer has arrived. It carries no challenge — the field does not
    // exist — so the only thing a dialer could try is a guess.
    match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Offer(_)) => {}
        other => panic!("expected an offer, got {other:?}"),
    }

    // Dialing now, before sending FILE_ACCEPT, must fail: the transfer has
    // not become active and holds no challenge to prove.
    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: vec![0u8; 32],
        },
    )
    .await
    .expect("auth");
    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
    assert_eq!(
        ready.status(),
        pb::DataStreamStatus::Rejected,
        "no stream before the transfer is agreed"
    );

    // The offer is still live and can proceed normally afterwards.
    let snapshot = server.transfers.snapshot_one(id).await.expect("record");
    assert_eq!(snapshot.state, TransferState::Offered);

    session.close().await;
}

/// A data stream that never authenticates is dropped rather than held open.
#[tokio::test]
async fn a_data_stream_that_says_nothing_does_not_hold_a_connection_slot() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    // Open a stream and send nothing at all.
    let silent = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");

    // The daemon keeps working for everyone else while that socket sits idle.
    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("unaffected.bin");
    let payload = write_sample_file(&source, 5000);
    let id = phone
        .transfers
        .offer_file(server.fingerprint, source)
        .await
        .expect("offer");
    let snapshot = wait_for_terminal(&phone.transfers, id, GRACE).await;
    assert_eq!(
        snapshot.state,
        TransferState::Completed,
        "{:?}",
        snapshot.failure
    );
    assert_eq!(
        std::fs::read(server.downloads.join("unaffected.bin")).expect("read"),
        payload
    );

    drop(silent);
    session.close().await;
}

/// A frame claiming to be enormous costs a rejection, not an allocation.
#[tokio::test]
async fn an_oversized_data_stream_frame_is_refused_before_allocating() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");

    use tokio::io::AsyncWriteExt;
    // Four gigabytes, claimed in four bytes.
    io.write_all(&u32::MAX.to_be_bytes()).await.expect("write");
    io.flush().await.expect("flush");

    // The daemon drops the connection rather than trying to buffer it, and
    // stays healthy.
    let mut buf = [0u8; 1];
    use tokio::io::AsyncReadExt;
    let _ = tokio::time::timeout(Duration::from_secs(5), io.read(&mut buf)).await;

    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());
    session.close().await;
}

// ---------------------------------------------------------------------------
// F13 — cancellation
// ---------------------------------------------------------------------------

/// A slow, controllable sender.
///
/// Sends the first chunk, then waits to be released. That gives a test a
/// transfer that is genuinely mid-copy — bytes on disk, more to come — which
/// is the only state in which cancelling, disconnecting or revoking is
/// interesting.
async fn start_stalled_transfer(
    server: &TestServer,
    phone: &TestClient,
    captured: &Captured,
    session: &ConnectedSession,
    id: TransferId,
    total: usize,
) -> tokio_rustls::client::TlsStream<tokio::net::TcpStream> {
    let payload = write_sample_bytes(total);
    send_files_control(
        session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "slow.bin".into(),
            size_bytes: total as u64,
            mime_type: String::new(),
            sha256: sha256_of(&payload).to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    let accept = match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Accept(a)) => a,
        other => panic!("expected acceptance, got {other:?}"),
    };
    let challenge = StreamChallenge::from_bytes(&accept.stream_challenge).expect("challenge");
    let mac = compute_stream_mac(
        common::test_profile(),
        &challenge,
        &server.fingerprint,
        &phone.fingerprint,
        &id,
    );

    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id.to_vec(),
            mac: mac.to_vec(),
        },
    )
    .await
    .expect("auth");
    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("ready");
    assert_eq!(ready.status(), pb::DataStreamStatus::Ready);

    // A first slice, so the receiver is demonstrably mid-copy, and then
    // nothing: the rest of the file never arrives unless a test sends it.
    use tokio::io::AsyncWriteExt;
    io.write_all(&payload[..1024]).await.expect("first chunk");
    io.flush().await.expect("flush");

    wait_for_bytes(&server.transfers, id, 1024, GRACE).await;
    io
}

/// Waits until a transfer has taken at least `want` bytes.
async fn wait_for_bytes(
    manager: &Arc<pliwee_capability_files::TransferManager>,
    id: TransferId,
    want: u64,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if manager
            .snapshot_one(id)
            .await
            .is_some_and(|s| s.bytes_transferred >= want)
        {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("transfer {id} never reached {want} bytes");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn f13_cancelling_mid_transfer_stops_it_and_deletes_the_partial_file() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xe1; 16]).expect("id");
    let _io = start_stalled_transfer(&server, &phone, &captured, &session, id, 2_000_000).await;

    // Mid-copy: bytes are on disk and a partial file exists.
    let mid = server.transfers.snapshot_one(id).await.expect("record");
    assert_eq!(mid.state, TransferState::Transferring);
    assert!(mid.bytes_transferred > 0 && mid.bytes_transferred < mid.size_bytes);
    assert!(
        !server.partial_files().is_empty(),
        "a .part file should exist now"
    );

    // The user cancels on the receiving side.
    assert!(server.transfers.cancel(id).await);

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(snapshot.state, TransferState::Cancelled);
    assert_eq!(snapshot.failure, Some(FailureReason::CancelledByUser));
    assert!(snapshot.stored_at.is_none());

    // No partial file survives, and nothing was promoted.
    assert!(
        server.partial_files().is_empty(),
        "cancellation must delete the partial file: {:?}",
        server.partial_files()
    );
    assert!(server.completed_files().is_empty());

    // Cancellation is propagated to the other side over the control channel,
    // which kept working throughout the copy — the whole point of the split.
    match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Cancel(c)) => {
            assert_eq!(c.reason(), pb::TransferFailureReason::CancelledByUser);
            assert_eq!(c.transfer_id, id.to_vec());
        }
        other => panic!("expected a cancel to reach the peer, got {other:?}"),
    }

    // Cancelling again is a no-op, not an error or a state change.
    assert!(!server.transfers.cancel(id).await);
    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());

    session.close().await;
}

#[tokio::test]
async fn f13_a_receiver_can_decline_an_offer_outright() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    server.approvals.set_accept(false);
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xe2; 16]).expect("id");
    let payload = b"unwanted".to_vec();
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "unwanted.txt".into(),
            size_bytes: payload.len() as u64,
            mime_type: String::new(),
            sha256: sha256_of(&payload).to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    match captured.next_control(GRACE).await.body {
        Some(pb::file_control::Body::Cancel(c)) => {
            assert_eq!(c.reason(), pb::TransferFailureReason::DeclinedByUser);
        }
        other => panic!("expected a decline, got {other:?}"),
    }

    assert_eq!(
        server.approvals.asked(),
        1,
        "the human was asked exactly once"
    );
    let snapshot = server.transfers.snapshot_one(id).await.expect("record");
    assert_eq!(snapshot.state, TransferState::Cancelled);
    assert!(server.partial_files().is_empty());
    assert!(server.completed_files().is_empty());

    session.close().await;
}

#[tokio::test]
async fn f13_an_offer_nobody_answers_times_out_rather_than_hanging() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    // The human walks away. The harness's accept timeout is short.
    server.approvals.set_stall(true);
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xe3; 16]).expect("id");
    send_files_control(
        &session,
        pb::file_control::Body::Offer(pb::FileOffer {
            transfer_id: id.to_vec(),
            filename: "ignored.txt".into(),
            size_bytes: 5,
            mime_type: String::new(),
            sha256: sha256_of(b"hello").to_vec(),
            timestamp_unix_ms: 0,
        }),
    )
    .await;

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert!(
        matches!(
            snapshot.state,
            TransferState::Cancelled | TransferState::Failed
        ),
        "an unanswered offer must settle, not hang: {}",
        snapshot.state
    );

    // The peer is told, so it does not wait forever either.
    let reply = captured.next_control(GRACE).await;
    assert!(
        matches!(
            reply.body,
            Some(pb::file_control::Body::Cancel(_)) | Some(pb::file_control::Body::Failed(_))
        ),
        "the sender must be told: {:?}",
        reply.body
    );

    assert!(server.partial_files().is_empty());
    session.close().await;
}

// ---------------------------------------------------------------------------
// F14 — the control session dies mid-transfer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f14_losing_the_control_session_mid_transfer_fails_it_and_cleans_up() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xf1; 16]).expect("id");
    // The data stream is deliberately kept alive. It is a *separate* TCP
    // connection, so killing the control session does not break it — which
    // is exactly why this case needs handling at all.
    let _io = start_stalled_transfer(&server, &phone, &captured, &session, id, 2_000_000).await;

    assert!(!server.partial_files().is_empty());

    // The phone drops off the network.
    session.close().await;

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(
        snapshot.state,
        TransferState::Failed,
        "a transfer whose session died must not sit in `transferring` forever"
    );
    assert_eq!(snapshot.failure, Some(FailureReason::Transport));
    assert!(snapshot.stored_at.is_none());

    assert!(
        server.partial_files().is_empty(),
        "the partial file must be removed: {:?}",
        server.partial_files()
    );
    assert!(server.completed_files().is_empty());
}

/// The terminal state and the partial file's removal are one step.
///
/// When they were two steps (#105), f14 — which polls every 10 ms — only
/// rarely landed between them. This observer does not sleep, and sixteen
/// transfers at once make that window all but certain to be hit if it is
/// ever reopened.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f14_a_failed_transfer_is_never_visible_with_its_partial_file() {
    async fn one(seed: u8) {
        let server = TestServer::start().await;
        let (phone, captured) = TestClient::new_raw("phone");
        let session = paired(&server, &phone).await;

        let id = TransferId::from_bytes(&[seed; 16]).expect("id");
        let _io = start_stalled_transfer(&server, &phone, &captured, &session, id, 2_000_000).await;
        assert!(!server.partial_files().is_empty());

        session.close().await;

        let deadline = tokio::time::Instant::now() + GRACE;
        let snapshot = loop {
            let current = server.transfers.snapshot_one(id).await.expect("record");
            if current.state.is_terminal() {
                break current;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "transfer {id} did not settle within {GRACE:?} (state: {})",
                current.state
            );
            tokio::task::yield_now().await;
        };
        assert_eq!(snapshot.state, TransferState::Failed);
        assert!(
            server.partial_files().is_empty(),
            "`failed` was observable while the partial file was still on disk: {:?}",
            server.partial_files()
        );
    }

    let runs: Vec<_> = (0..16u8).map(|n| tokio::spawn(one(0xa0 + n))).collect();
    for run in runs {
        run.await.expect("a run panicked; its message is above");
    }
}

// ---------------------------------------------------------------------------
// F15 — revocation during a transfer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f15_revoking_a_peer_mid_transfer_stops_it_immediately() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xf2; 16]).expect("id");
    let _io = start_stalled_transfer(&server, &phone, &captured, &session, id, 2_000_000).await;

    let mid = server.transfers.snapshot_one(id).await.expect("record");
    assert_eq!(mid.state, TransferState::Transferring);
    assert!(!server.partial_files().is_empty());

    // The user unpairs the phone while its file is still arriving.
    {
        let mut store = server.state.store.lock().await;
        assert!(store.revoke_peer(&phone.fingerprint).expect("revoke"));
    }
    server
        .transfers
        .cancel_peer(
            &phone.fingerprint,
            pliwee_capability_files::transfer::FailureReason::Revoked,
        )
        .await;

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(snapshot.failure, Some(FailureReason::Revoked));
    assert!(snapshot.stored_at.is_none());
    assert!(
        server.partial_files().is_empty(),
        "a revoked peer's partial file must not be left behind"
    );
    assert!(server.completed_files().is_empty());

    // And the revoked peer cannot start another one, nor open a stream.
    let id2 = TransferId::from_bytes(&[0xf3; 16]).expect("id");
    let mut io = open_data_stream(server.addr, &phone.identity, server.fingerprint)
        .await
        .expect("TLS still completes: revocation is an application decision");
    stream::write_frame(
        &mut io,
        &pb::DataStreamAuth {
            protocol_version: 1,
            transfer_id: id2.to_vec(),
            mac: vec![0u8; 32],
        },
    )
    .await
    .expect("auth");
    let ready: pb::DataStreamReady = stream::read_frame(&mut io).await.expect("reply");
    assert_eq!(ready.status(), pb::DataStreamStatus::Rejected);
    assert_eq!(ready.reason(), pb::TransferFailureReason::NotAuthorized);
}

/// The reaper is the backstop: even with no explicit `cancel_peer` call, a
/// transfer whose peer lost its grant does not keep running.
#[tokio::test]
async fn f15_a_transfer_whose_grant_disappears_is_reaped() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let id = TransferId::from_bytes(&[0xf4; 16]).expect("id");
    let _io = start_stalled_transfer(&server, &phone, &captured, &session, id, 2_000_000).await;

    // Only the grant is withdrawn. Nothing else is told about it — the
    // periodic re-authorization has to notice on its own.
    server.set_grant(phone.fingerprint, "files.v1", false).await;

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(snapshot.failure, Some(FailureReason::Revoked));
    assert!(server.partial_files().is_empty());
    assert!(server.completed_files().is_empty());

    session.close().await;
}

// ---------------------------------------------------------------------------
// F18 — two transfers at once
// ---------------------------------------------------------------------------

#[tokio::test]
async fn f18_two_simultaneous_transfers_stay_isolated() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");

    // Deliberately different sizes and contents, so a crossed stream or a
    // shared buffer would corrupt at least one of them.
    let a_path = dir.path().join("alpha.bin");
    let b_path = dir.path().join("beta.bin");
    let a = write_sample_file(&a_path, 300_001);
    let mut b = write_sample_file(&b_path, 150_007);
    b.reverse();
    std::fs::write(&b_path, &b).expect("write beta");

    let (id_a, id_b) = tokio::join!(
        phone.transfers.offer_file(server.fingerprint, a_path),
        phone.transfers.offer_file(server.fingerprint, b_path),
    );
    let id_a = id_a.expect("offer alpha");
    let id_b = id_b.expect("offer beta");
    assert_ne!(id_a, id_b, "each transfer gets its own id");

    let snap_a = wait_for_terminal(&phone.transfers, id_a, Duration::from_secs(60)).await;
    let snap_b = wait_for_terminal(&phone.transfers, id_b, Duration::from_secs(60)).await;
    assert_eq!(
        snap_a.state,
        TransferState::Completed,
        "{:?}",
        snap_a.failure
    );
    assert_eq!(
        snap_b.state,
        TransferState::Completed,
        "{:?}",
        snap_b.failure
    );

    // Both files are intact and each has its own content.
    assert_eq!(
        std::fs::read(server.downloads.join("alpha.bin")).expect("read"),
        a
    );
    assert_eq!(
        std::fs::read(server.downloads.join("beta.bin")).expect("read"),
        b
    );
    assert!(server.partial_files().is_empty());

    session.close().await;
}

/// Cancelling one transfer leaves the other alone.
#[tokio::test]
async fn f18_cancelling_one_transfer_does_not_disturb_another() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    let session = paired(&server, &phone).await;

    let stalled = TransferId::from_bytes(&[0x51; 16]).expect("id");
    let _io =
        start_stalled_transfer(&server, &phone, &captured, &session, stalled, 2_000_000).await;

    // A second, well-behaved transfer alongside it.
    let good = TransferId::from_bytes(&[0x52; 16]).expect("id");
    let payload = write_sample_bytes(20_000);
    let reply = hostile_transfer(
        &server,
        &phone,
        &captured,
        &session,
        good,
        "good.bin",
        payload.len() as u64,
        sha256_of(&payload),
        &payload,
    )
    .await;
    assert!(
        matches!(reply.body, Some(pb::file_control::Body::Complete(_))),
        "{:?}",
        reply.body
    );

    // Cancel only the stalled one.
    assert!(server.transfers.cancel(stalled).await);
    let cancelled = wait_for_terminal(&server.transfers, stalled, GRACE).await;
    assert_eq!(cancelled.state, TransferState::Cancelled);

    // The completed one is untouched, on disk and in the record.
    let survivor = server.transfers.snapshot_one(good).await.expect("record");
    assert_eq!(survivor.state, TransferState::Completed);
    assert_eq!(
        std::fs::read(server.downloads.join("good.bin")).expect("read"),
        payload
    );
    assert!(
        server.partial_files().is_empty(),
        "only the cancelled transfer's partial file existed, and it is gone"
    );

    session.close().await;
}

/// The concurrency limit is enforced per peer.
#[tokio::test]
async fn too_many_transfers_at_once_are_refused() {
    let server = TestServer::start().await;
    let (phone, captured) = TestClient::new_raw("phone");
    // Nobody answers, so every offer stays live and occupies a slot.
    server.approvals.set_stall(true);
    let session = paired(&server, &phone).await;

    let payload = b"queued".to_vec();
    let mut refusals = 0;

    for index in 0..(limits::MAX_CONCURRENT_TRANSFERS_PER_PEER + 2) {
        let id = TransferId::from_bytes(&[0x60 + index as u8; 16]).expect("id");
        send_files_control(
            &session,
            pb::file_control::Body::Offer(pb::FileOffer {
                transfer_id: id.to_vec(),
                filename: format!("queued{index}.bin"),
                size_bytes: payload.len() as u64,
                mime_type: String::new(),
                sha256: sha256_of(&payload).to_vec(),
                timestamp_unix_ms: 0,
            }),
        )
        .await;

        if index >= limits::MAX_CONCURRENT_TRANSFERS_PER_PEER {
            match captured.next_control(GRACE).await.body {
                Some(pb::file_control::Body::Reject(r)) => {
                    assert_eq!(r.reason(), pb::TransferFailureReason::TooManyTransfers);
                    refusals += 1;
                }
                other => panic!("offer {index} past the limit should be refused: {other:?}"),
            }
        }
    }

    assert_eq!(refusals, 2, "both offers past the limit were refused");
    // The session is fine; a refused offer is not a fatal error.
    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());
    session.close().await;
}

/// A peer that floods the session and never reads its replies cannot stop the
/// daemon serving anyone else.
///
/// The shape being probed: a capability's `on_message` is awaited *inside* the
/// session's `select!` loop, and that same loop drains the outbound queue and
/// writes it to the socket. A peer that keeps writing while refusing to read
/// backs the write side up; once the 32-slot outbound queue fills, a handler
/// that waited indefinitely to enqueue its reply would block the only task
/// that could make room. `battery.v1` can never reach that state because it
/// never replies from `on_message`; `files.v1` replies to every malformed
/// offer, so it can.
///
/// **What this test does and does not establish.** It asserts the property
/// that matters operationally — one hostile peer cannot deny service to
/// another — and that holds because every session runs in its own task. It
/// does *not* reproduce the stuck-session state itself: the author could not
/// construct an input that reaches it, and the test passes with or without
/// the bounded send in `send_control`. That bound is therefore hardening
/// against a hazard visible in the code, not a fix for a demonstrated
/// failure, and it is described that way rather than credited with more.
#[tokio::test]
async fn a_peer_that_floods_and_never_reads_cannot_wedge_the_daemon() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");

    // Pair and grant through an ordinary session, then drop it.
    {
        let token = server.open_pairing(Duration::from_secs(30)).await;
        let session = phone
            .connect(server.addr, server.fingerprint, Some(&token))
            .await
            .expect("pairing");
        server.set_grant(phone.fingerprint, "files.v1", true).await;
        session.close().await;
    }

    // Now a deliberately rude client: it completes the handshake and then
    // writes offers as fast as it can, never reading a single reply.
    let mut tls = phone
        .tls_connect(server.addr, server.fingerprint)
        .await
        .expect("TLS");
    let handshake = pliwee_core::session::connect_handshake(
        &mut tls,
        &phone.host,
        server.fingerprint,
        common::test_profile(),
        None,
    )
    .await
    .expect("handshake");
    assert!(matches!(
        handshake,
        pliwee_core::session::ClientHandshake::Established(_, _)
    ));

    // Envelopes built by hand, continuing the sequence the handshake used.
    // Every offer is malformed, so every one produces a reply that this
    // client will never read.
    for (sequence, index) in (2u64..).zip(0..400u32) {
        let mut id = [0u8; 16];
        id[..4].copy_from_slice(&index.to_be_bytes());
        let payload = <pb::FileControl as prost::Message>::encode_to_vec(&pb::FileControl {
            body: Some(pb::file_control::Body::Offer(pb::FileOffer {
                transfer_id: id.to_vec(),
                filename: "..".into(),
                size_bytes: 1,
                mime_type: String::new(),
                sha256: vec![0; 32],
                timestamp_unix_ms: 0,
            })),
        });

        let mut message_id = vec![0u8; 16];
        message_id[..4].copy_from_slice(&index.to_be_bytes());
        message_id[15] = 0xaa;

        let envelope = pliwee_proto::v1::Envelope {
            protocol_version: 1,
            message_id,
            sequence,
            timestamp_unix_ms: 0,
            correlation_id: Vec::new(),
            body: Some(pliwee_proto::v1::envelope::Body::CapabilityMessage(
                pliwee_proto::v1::CapabilityMessage {
                    capability_id: "files.v1".to_string(),
                    payload,
                },
            )),
        };
        // Once the daemon stops draining, our own socket buffer fills and
        // this write blocks. That is the point at which the daemon must have
        // given up rather than be stuck waiting for us.
        if tokio::time::timeout(
            Duration::from_secs(5),
            pliwee_core::framing::write_envelope(&mut tls, &envelope),
        )
        .await
        .is_err()
        {
            break;
        }
    }

    // The daemon must still be serving everyone else. A second, well-behaved
    // device pairs, connects and completes a transfer while the rude one is
    // still attached.
    let good = TestClient::new("well-behaved");
    let token = server.open_pairing(Duration::from_secs(30)).await;
    let first = good
        .connect(server.addr, server.fingerprint, Some(&token))
        .await
        .expect("the daemon must still accept connections");
    server.set_grant(good.fingerprint, "files.v1", true).await;
    first.close().await;
    let session = good
        .connect(server.addr, server.fingerprint, None)
        .await
        .expect("reconnect");

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("unblocked.bin");
    let payload = write_sample_file(&source, 30_000);
    let id = good
        .transfers
        .offer_file(server.fingerprint, source)
        .await
        .expect("offer");

    let snapshot = wait_for_terminal(&good.transfers, id, GRACE).await;
    assert_eq!(
        snapshot.state,
        TransferState::Completed,
        "a flooding peer must not stop the daemon serving anyone else: {:?}",
        snapshot.failure
    );
    assert_eq!(
        std::fs::read(server.downloads.join("unblocked.bin")).expect("read"),
        payload
    );

    session.close().await;
    drop(tls);
}

/// H3: when the *receiver* cancels, the sender must say so.
///
/// Cancelling a receive does two things on two different TLS connections: it
/// tears the data stream down, and it sends FILE_CANCEL on the control
/// session. Those race, and the stream's end normally wins — so the sender
/// used to attribute every receiver-side cancellation to the network and end
/// in `Failed(Transport)`, which is wrong and, worse, indistinguishable from
/// a genuine connection drop.
///
/// Observed on hardware first: cancelling a 400 MiB transfer on the tablet
/// left the desktop reporting "the connection ended mid-transfer".
#[tokio::test]
async fn a_receiver_side_cancel_reaches_the_sender_as_a_cancellation() {
    let server = TestServer::start().await;
    let phone = TestClient::new("phone");
    let session = paired(&server, &phone).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("big.bin");
    // Big enough that the copy is still running when we cancel it.
    write_sample_file(&source, limits::COPY_BUFFER_BYTES * 512);

    let id = server
        .transfers
        .offer_file(phone.fingerprint, source)
        .await
        .expect("offer");

    // Cancel from the receiving side, mid-copy.
    wait_for_bytes(&phone.transfers, id, 1, GRACE).await;
    assert!(phone.transfers.cancel(id).await);

    let snapshot = wait_for_terminal(&server.transfers, id, GRACE).await;
    assert_eq!(
        snapshot.state,
        TransferState::Cancelled,
        "the sender should report a cancellation, not {:?}",
        snapshot.failure
    );
    assert_eq!(snapshot.failure, Some(FailureReason::CancelledByUser));

    // Nothing was stored, and the session is still usable afterwards.
    assert!(phone.received_files().is_empty());
    assert!(session.handle.ping(Duration::from_secs(5)).await.is_some());
    session.close().await;
}
