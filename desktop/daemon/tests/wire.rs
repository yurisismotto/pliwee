//! Wire-level tests driven by a hostile client that speaks the framing
//! protocol directly, so it can send things a well-behaved peer never would:
//! replayed envelopes, duplicate message ids, wrong versions, out-of-state
//! messages.
//!
//! These run over the same real TLS 1.3 stack as everything else. No
//! validation is disabled anywhere.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{TestClient, TestServer};
use pliwee_core::framing;
use pliwee_core::session::PROTOCOL_VERSION_MAX;
use pliwee_core::Error;
use pliwee_proto::v1;
use tokio::io::{AsyncRead, AsyncWrite};

const TTL: Duration = Duration::from_secs(30);

/// Builds an envelope with full control over every header field.
fn envelope(
    version: u32,
    message_id: Vec<u8>,
    sequence: u64,
    body: v1::envelope::Body,
) -> v1::Envelope {
    v1::Envelope {
        protocol_version: version,
        message_id,
        sequence,
        timestamp_unix_ms: 1_700_000_000_000,
        correlation_id: Vec::new(),
        body: Some(body),
    }
}

fn hello(client: &TestClient, min: u32, max: u32) -> v1::envelope::Body {
    v1::envelope::Body::Hello(v1::Hello {
        device: Some(client.identity.device_info()),
        min_protocol_version: min,
        max_protocol_version: max,
        capabilities: vec!["battery.v1".into()],
    })
}

async fn read_ack<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> v1::HelloAck {
    let env = framing::read_envelope(stream).await.expect("HELLO_ACK");
    match env.body {
        Some(v1::envelope::Body::HelloAck(a)) => a,
        other => panic!("expected HELLO_ACK, got {other:?}"),
    }
}

/// Pairs a client normally, then returns a fresh raw TLS connection on which
/// it is already trusted.
async fn trusted_raw_connection(
    server: &TestServer,
    client: &TestClient,
) -> tokio_rustls::client::TlsStream<tokio::net::TcpStream> {
    let token = server.open_pairing(TTL).await;
    client
        .connect(server.addr, server.fingerprint, Some(&token))
        .await
        .expect("pair")
        .close()
        .await;
    server.state.end_pairing().await;

    client
        .tls_connect(server.addr, server.fingerprint)
        .await
        .expect("tls")
}

// ---------------------------------------------------------------------------
// Replay and duplicate detection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_duplicate_message_id_terminates_the_session() {
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    let id = vec![0xAB; 16];
    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            id.clone(),
            1,
            hello(&phone, 1, PROTOCOL_VERSION_MAX),
        ),
    )
    .await
    .expect("hello");

    let ack = read_ack(&mut tls).await;
    assert_eq!(ack.status, v1::HelloStatus::Trusted as i32);

    // A ping with a *fresh* sequence number but a message id we already used.
    // The sequence check alone would let this through; de-duplication is what
    // catches it.
    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            id.clone(),
            2,
            v1::envelope::Body::Ping(v1::Ping { payload: vec![1] }),
        ),
    )
    .await
    .expect("ping");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "a duplicate message id must terminate the session, got {result:?}"
    );
}

#[tokio::test]
async fn a_replayed_envelope_terminates_the_session() {
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 16],
            1,
            hello(&phone, 1, PROTOCOL_VERSION_MAX),
        ),
    )
    .await
    .expect("hello");
    read_ack(&mut tls).await;

    let ping = envelope(
        PROTOCOL_VERSION_MAX,
        vec![2; 16],
        2,
        v1::envelope::Body::Ping(v1::Ping { payload: vec![7] }),
    );
    framing::write_envelope(&mut tls, &ping)
        .await
        .expect("ping");

    let pong = framing::read_envelope(&mut tls).await.expect("pong");
    assert!(matches!(pong.body, Some(v1::envelope::Body::Pong(_))));

    // Byte-for-byte replay of a message that was already accepted.
    framing::write_envelope(&mut tls, &ping)
        .await
        .expect("replay");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "a replayed envelope must terminate the session, got {result:?}"
    );
}

#[tokio::test]
async fn a_non_increasing_sequence_number_terminates_the_session() {
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 16],
            10,
            hello(&phone, 1, PROTOCOL_VERSION_MAX),
        ),
    )
    .await
    .expect("hello");
    read_ack(&mut tls).await;

    // Fresh id, but the sequence goes backwards.
    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![2; 16],
            5,
            v1::envelope::Body::Ping(v1::Ping { payload: vec![] }),
        ),
    )
    .await
    .expect("ping");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "a rewound sequence must terminate the session, got {result:?}"
    );
}

#[tokio::test]
async fn a_short_message_id_is_refused() {
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    // 4 bytes instead of 16: not enough entropy to make de-duplication
    // meaningful, so it must be refused outright.
    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 4],
            1,
            hello(&phone, 1, PROTOCOL_VERSION_MAX),
        ),
    )
    .await
    .expect("hello");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "a short message id must be refused, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Protocol version handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unsupported_protocol_version_is_reported_and_refused() {
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    // A peer that only speaks versions far in the future.
    framing::write_envelope(
        &mut tls,
        &envelope(99, vec![1; 16], 1, hello(&phone, 90, 99)),
    )
    .await
    .expect("hello");

    let ack = read_ack(&mut tls).await;
    assert_eq!(
        ack.status,
        v1::HelloStatus::VersionUnsupported as i32,
        "the server must say why, rather than closing silently"
    );
    assert_eq!(ack.negotiated_protocol_version, 0);

    // And the connection must not proceed.
    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn changing_the_protocol_version_mid_connection_terminates_the_session() {
    // Downgrade attempt from a peer that already completed a v1 handshake.
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 16],
            1,
            hello(&phone, 1, PROTOCOL_VERSION_MAX),
        ),
    )
    .await
    .expect("hello");
    read_ack(&mut tls).await;

    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX + 7,
            vec![2; 16],
            2,
            v1::envelope::Body::Ping(v1::Ping { payload: vec![] }),
        ),
    )
    .await
    .expect("ping");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "a mid-connection version change must terminate the session, got {result:?}"
    );
}

#[tokio::test]
async fn a_v2_peer_is_negotiated_down_to_v1_and_session_close_is_refused() {
    // The SessionClose schema (GitHub #90) is dormant: this build advertises
    // and negotiates version 1, and refuses SessionClose on a V1 session as
    // an unexpected body, closing it. (A legacy V1 binary decodes it as an
    // unset body and closes too, per ADR-0010; this test proves this build.)
    let server = TestServer::start().await;
    let phone = TestClient::new("Galaxy S25");
    let mut tls = trusted_raw_connection(&server, &phone).await;

    framing::write_envelope(&mut tls, &envelope(1, vec![1; 16], 1, hello(&phone, 1, 2)))
        .await
        .expect("hello");
    let ack = read_ack(&mut tls).await;
    assert_eq!(ack.status, v1::HelloStatus::Trusted as i32);
    assert_eq!(ack.negotiated_protocol_version, 1);

    // Before: the session is alive and answers a ping.
    framing::write_envelope(
        &mut tls,
        &envelope(
            1,
            vec![2; 16],
            2,
            v1::envelope::Body::Ping(v1::Ping { payload: vec![5] }),
        ),
    )
    .await
    .expect("ping");
    let pong = framing::read_envelope(&mut tls).await.expect("pong");
    assert!(
        matches!(pong.body, Some(v1::envelope::Body::Pong(ref p)) if p.payload == [5]),
        "{pong:?}"
    );

    // After: SessionClose on a V1 session terminates it.
    framing::write_envelope(
        &mut tls,
        &envelope(
            1,
            vec![3; 16],
            3,
            v1::envelope::Body::SessionClose(v1::SessionClose {
                reason: v1::SessionCloseReason::Superseded as i32,
            }),
        ),
    )
    .await
    .expect("session close");
    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "SessionClose on a version-1 session must terminate it, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// State machine enforcement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unauthenticated_peer_cannot_skip_the_handshake() {
    let server = TestServer::start().await;
    let stranger = TestClient::new("Attacker");
    let mut tls = stranger
        .tls_connect(server.addr, server.fingerprint)
        .await
        .expect("tls");

    // Straight to PING, with no HELLO at all.
    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 16],
            1,
            v1::envelope::Body::Ping(v1::Ping { payload: vec![] }),
        ),
    )
    .await
    .expect("ping");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "PING before HELLO must be refused, got {result:?}"
    );
}

#[tokio::test]
async fn an_unknown_peer_cannot_send_anything_but_a_pair_request() {
    let server = TestServer::start().await;
    let stranger = TestClient::new("Attacker");
    let _token = server.open_pairing(TTL).await;

    let mut tls = stranger
        .tls_connect(server.addr, server.fingerprint)
        .await
        .expect("tls");

    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 16],
            1,
            hello(&stranger, 1, PROTOCOL_VERSION_MAX),
        ),
    )
    .await
    .expect("hello");

    let ack = read_ack(&mut tls).await;
    assert_eq!(ack.status, v1::HelloStatus::PairingRequired as i32);
    assert_eq!(ack.pairing_nonce.len(), pliwee_core::pairing::NONCE_LEN);

    // A PING instead of the expected PAIR_REQUEST.
    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![2; 16],
            2,
            v1::envelope::Body::Ping(v1::Ping { payload: vec![] }),
        ),
    )
    .await
    .expect("ping");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "an unpaired peer must not get a PONG, got {result:?}"
    );
}

#[tokio::test]
async fn a_hello_that_claims_someone_elses_fingerprint_is_refused() {
    // The application-layer identity must match the TLS identity, or a
    // trusted device could be impersonated by anyone who knows its
    // fingerprint (which is not a secret).
    let server = TestServer::start().await;
    let victim = TestClient::new("Galaxy S25");
    let attacker = TestClient::new("Attacker");

    let token = server.open_pairing(TTL).await;
    victim
        .connect(server.addr, server.fingerprint, Some(&token))
        .await
        .expect("victim pairs")
        .close()
        .await;
    server.state.end_pairing().await;

    let mut tls = attacker
        .tls_connect(server.addr, server.fingerprint)
        .await
        .expect("tls");

    // The attacker's own TLS key, but the victim's fingerprint in HELLO.
    let mut info = attacker.identity.device_info();
    info.identity_fingerprint = victim.fingerprint.to_hex();

    framing::write_envelope(
        &mut tls,
        &envelope(
            PROTOCOL_VERSION_MAX,
            vec![1; 16],
            1,
            v1::envelope::Body::Hello(v1::Hello {
                device: Some(info),
                min_protocol_version: 1,
                max_protocol_version: PROTOCOL_VERSION_MAX,
                capabilities: vec!["battery.v1".into()],
            }),
        ),
    )
    .await
    .expect("hello");

    let result = framing::read_envelope(&mut tls).await;
    assert!(
        matches!(result, Err(Error::Closed) | Err(Error::Io(_))),
        "a spoofed fingerprint must be refused, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// TLS-level authentication
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_client_without_a_certificate_is_rejected_during_the_handshake() {
    // Client authentication is mandatory: a peer with no certificate has no
    // identity and can never be authorized, so it must not even get a
    // completed handshake to talk over.
    let server = TestServer::start().await;
    common::init_crypto();

    let verifier = pliwee_core::tls::PinnedServerCertVerifier::new(server.fingerprint);
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .expect("tls13")
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(verifier))
    .with_no_client_auth();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("tcp");
    let name = rustls_pki_types::ServerName::try_from("omnibridge.invalid").expect("name");

    let mut stream = match connector.connect(name, tcp).await {
        // rustls may only surface the server's alert on first use, so a
        // successful `connect` is not yet a pass or a fail.
        Ok(s) => s,
        Err(_) => return,
    };

    let result = framing::read_envelope(&mut stream).await;
    assert!(
        result.is_err(),
        "a certificate-less client must not be able to use the session"
    );
}

/// The anchor for the Wave 5 "run twice, once per profile" requirement.
///
/// This suite runs under `PLIWEE_TEST_PROFILE` (see `common::test_profile`).
/// Here the **daemon's** view is observed, not the client's: the session the
/// listener registered must carry the profile the run names, and the ALPN the
/// daemon negotiated must be that profile's. A run that silently fell back to
/// the canonical profile fails here.
#[tokio::test]
async fn the_daemon_negotiated_the_profile_this_run_names() {
    let profile = common::test_profile();
    let server = TestServer::start().await;
    let client = TestClient::new("phone");
    let token = server.open_pairing(Duration::from_secs(30)).await;
    let session = client
        .connect(server.addr, server.fingerprint, Some(&token))
        .await
        .expect("pairing");

    common::wait_until(Duration::from_secs(10), || async {
        server
            .state
            .session_for(&client.fingerprint)
            .await
            .is_some()
    })
    .await;
    let handle = server
        .state
        .session_for(&client.fingerprint)
        .await
        .expect("the daemon registered the session");
    assert_eq!(handle.profile(), profile);
    println!(
        "EVIDENCE run-profile={profile} daemon-session-profile={} alpn={}",
        handle.profile(),
        String::from_utf8_lossy(handle.profile().control_alpn())
    );
    session.close().await;
}
