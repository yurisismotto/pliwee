//! Identity, fingerprinting, QR payload and trust-store persistence.

use std::os::unix::fs::PermissionsExt;

use pliwee_core::clipboard_policy::ClipboardPolicy;
use pliwee_core::identity::LocalIdentity;
use pliwee_core::notification_policy::NotificationPolicy;
use pliwee_core::pairing::PairingToken;
use pliwee_core::qr::QrPayload;
use pliwee_core::store::{Settings, Store, TrustedPeer};
use pliwee_core::{Fingerprint, Profile};
use pliwee_proto::v1::Platform;

fn identity() -> LocalIdentity {
    LocalIdentity::generate("Test Device", Platform::Linux).expect("generate identity")
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

#[test]
fn generated_identities_are_distinct() {
    let a = identity();
    let b = identity();
    assert_ne!(a.device_id(), b.device_id());
    assert_ne!(a.fingerprint(), b.fingerprint());
}

#[test]
fn device_id_is_128_bits_of_hex() {
    let id = identity();
    assert_eq!(id.device_id().len(), 32);
    assert!(id.device_id().bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn fingerprint_is_stable_and_derived_from_the_certificate() {
    let id = identity();
    let from_cert = Fingerprint::from_certificate_der(id.certificate_der()).expect("fingerprint");
    assert_eq!(from_cert, id.fingerprint());
    assert_eq!(id.fingerprint(), id.fingerprint());
}

#[test]
fn fingerprint_hex_round_trips() {
    let id = identity();
    let hex = id.fingerprint().to_hex();
    assert_eq!(hex.len(), 64);
    assert_eq!(
        Fingerprint::from_hex(&hex).expect("parse"),
        id.fingerprint()
    );
}

#[test]
fn malformed_fingerprints_are_rejected() {
    assert!(Fingerprint::from_hex("").is_err());
    assert!(Fingerprint::from_hex("zz").is_err());
    // Right length, wrong alphabet.
    assert!(Fingerprint::from_hex(&"g".repeat(64)).is_err());
    // Right alphabet, wrong length.
    assert!(Fingerprint::from_hex(&"ab".repeat(31)).is_err());
    // Uppercase is not the canonical form and must not be silently accepted,
    // or two spellings of one identity could disagree as map keys.
    assert!(Fingerprint::from_hex(&"AB".repeat(32)).is_err());
}

#[test]
fn identity_debug_never_leaks_the_private_key() {
    let id = identity();
    let rendered = format!("{id:?}");
    assert!(rendered.contains("<redacted>"));
    let key_hex = data_encoding::HEXLOWER.encode(id.private_key_pkcs8_der());
    assert!(!rendered.contains(&key_hex));
}

#[test]
fn device_info_carries_the_fingerprint_not_an_address() {
    let id = identity();
    let info = id.device_info();
    assert_eq!(info.identity_fingerprint, id.fingerprint().to_hex());
    assert_eq!(info.device_id, id.device_id());
    // Nothing in DeviceInfo should look like a network address.
    let serialized = format!("{info:?}");
    assert!(!serialized.contains("192.168"));
}

#[test]
fn certificate_carries_no_subject_alt_names() {
    // Our verifier ignores hostnames on purpose. Shipping a SAN would invite
    // a future change that starts trusting it.
    let id = identity();
    let (_, cert) = x509_parser::parse_x509_certificate(id.certificate_der()).expect("parse cert");
    assert!(
        cert.tbs_certificate
            .subject_alternative_name()
            .ok()
            .flatten()
            .is_none(),
        "identity certificates must not carry SANs"
    );
}

// ---------------------------------------------------------------------------
// QR payload
// ---------------------------------------------------------------------------

#[test]
fn qr_payload_round_trips() {
    let id = identity();
    let token = PairingToken::generate().expect("token");
    let addrs = vec!["192.168.1.10:55432".parse().expect("addr")];

    let encoded = QrPayload::encode(&id.fingerprint(), &token, id.device_id(), &addrs);
    let parsed = QrPayload::parse(&encoded).expect("parse");

    // The daemon emits the canonical scheme only (ADR-0020 §D4).
    assert!(encoded.starts_with("pliwee1:"), "{encoded}");
    assert_eq!(parsed.profile, Profile::Pliwee);
    assert_eq!(parsed.fingerprint, id.fingerprint());
    assert_eq!(parsed.device_id, id.device_id());
    assert_eq!(parsed.addresses, addrs);
    assert_eq!(parsed.token().expect("token").as_bytes(), token.as_bytes());
}

#[test]
fn qr_payload_handles_ipv6_addresses() {
    let id = identity();
    let token = PairingToken::generate().expect("token");
    let addrs: Vec<std::net::SocketAddr> = vec![
        "[fe80::1]:55432".parse().expect("v6"),
        "10.0.0.5:55432".parse().expect("v4"),
    ];
    let encoded = QrPayload::encode(&id.fingerprint(), &token, id.device_id(), &addrs);
    let parsed = QrPayload::parse(&encoded).expect("parse");
    assert_eq!(parsed.addresses, addrs);
}

#[test]
fn qr_payload_rejects_hostile_input() {
    assert!(QrPayload::parse("").is_err());
    assert!(QrPayload::parse("http://evil.example/").is_err());
    // Right scheme, truncated — under either accepted scheme.
    assert!(QrPayload::parse("pliwee1:").is_err());
    assert!(QrPayload::parse("omnibridge1:").is_err());
    // Wrong scheme version.
    let id = identity();
    let token = PairingToken::generate().expect("token");
    let good = QrPayload::encode(&id.fingerprint(), &token, id.device_id(), &[]);
    assert!(QrPayload::parse(&good.replacen("pliwee1", "pliwee9", 1)).is_err());
    assert!(QrPayload::parse(&good.replacen("pliwee1", "omnibridge9", 1)).is_err());
    // Oversized payload must be refused before parsing.
    assert!(QrPayload::parse(&"a".repeat(100_000)).is_err());
}

#[test]
fn qr_payload_rejects_a_tampered_fingerprint() {
    let id = identity();
    let token = PairingToken::generate().expect("token");
    let encoded = QrPayload::encode(&id.fingerprint(), &token, id.device_id(), &[]);
    let tampered = encoded.replace(&id.fingerprint().to_hex(), &"ab".repeat(20));
    assert!(QrPayload::parse(&tampered).is_err());
}

#[test]
fn qr_payload_drops_unparseable_addresses_but_keeps_the_rest() {
    // Addresses are hints; one bad entry must not make a valid code unusable.
    let id = identity();
    let token = PairingToken::generate().expect("token");
    let encoded = format!(
        "pliwee1:{}:{}:{}:not-an-address,10.0.0.7:55432",
        id.fingerprint().to_hex(),
        token.to_base32(),
        id.device_id()
    );
    let parsed = QrPayload::parse(&encoded).expect("parse");
    assert_eq!(parsed.addresses.len(), 1);
    assert_eq!(parsed.addresses[0].to_string(), "10.0.0.7:55432");
}

/// The QR parse matrix of the Wave 5 plan: `pliwee1` and `omnibridge1` are
/// accepted and each fixes its profile; `pliwee2` and `omnibridge2` are
/// recognised as a newer format and rejected as such (ADR-0011's rule);
/// `anyflow1` is not a pairing code at all.
#[test]
fn qr_scheme_matrix() {
    let id = identity();
    let token = PairingToken::generate().expect("token");
    let body = format!(
        "{}:{}:{}:10.0.0.7:55432",
        id.fingerprint().to_hex(),
        token.to_base32(),
        id.device_id()
    );
    let parse = |scheme: &str| QrPayload::parse(&format!("{scheme}:{body}"));

    let canonical = parse("pliwee1").expect("pliwee1 accepted");
    assert_eq!(canonical.profile, Profile::Pliwee);
    let legacy = parse("omnibridge1").expect("omnibridge1 accepted");
    assert_eq!(legacy.profile, Profile::OmniBridge);
    // Same payload body, same pinned identity: only the profile differs.
    assert_eq!(canonical.fingerprint, legacy.fingerprint);

    for newer in ["pliwee2", "omnibridge2"] {
        let err = parse(newer).err().expect("a newer version is refused");
        assert_eq!(
            err.to_string(),
            pliwee_core::Error::Protocol("unsupported QR payload version").to_string(),
            "{newer}"
        );
    }
    for foreign in ["anyflow1", "fedroid1", "PLIWEE1", "pliwee", "pliwee1x"] {
        let err = parse(foreign).err().expect("not a pairing code");
        assert_eq!(
            err.to_string(),
            pliwee_core::Error::Protocol("unknown QR scheme").to_string(),
            "{foreign}"
        );
    }
}

/// Certificates of *new* identities carry `CN=pliwee:<device-id>`. Nothing
/// parses the CN — trust is the SPKI pin — so this is a label, checked so it
/// cannot drift silently.
#[test]
fn a_new_identity_certificate_names_pliwee() {
    let id = identity();
    let (_, cert) = x509_parser::parse_x509_certificate(id.certificate_der()).expect("x509");
    let cn: Vec<&str> = cert
        .subject()
        .iter_common_name()
        .map(|a| a.as_str().expect("utf8 CN"))
        .collect();
    assert_eq!(cn, vec![format!("pliwee:{}", id.device_id()).as_str()]);
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

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

#[test]
fn store_generates_an_identity_on_first_run_and_reloads_it() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first_fingerprint = {
        let store = Store::open(dir.path()).expect("open");
        store.identity().fingerprint()
    };

    let store = Store::open(dir.path()).expect("reopen");
    assert_eq!(
        store.identity().fingerprint(),
        first_fingerprint,
        "identity must survive a restart, or every pairing would break"
    );
}

#[test]
fn private_key_is_written_with_restrictive_permissions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _store = Store::open(dir.path()).expect("open");

    let key_mode = std::fs::metadata(dir.path().join("identity.key"))
        .expect("stat key")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(key_mode, 0o600, "private key must be owner-only");

    let dir_mode = std::fs::metadata(dir.path())
        .expect("stat dir")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        dir_mode & 0o077,
        0,
        "data directory must not be group/world accessible"
    );
}

#[test]
fn store_refuses_to_load_a_world_readable_private_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let _store = Store::open(dir.path()).expect("open");
    }

    let key_path = dir.path().join("identity.key");
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644))
        .expect("loosen permissions");

    // `expect_err`, not `.err().expect()`: clippy::err_expect flags the
    // latter, and it was already doing so before Wave 0.
    let err = Store::open(dir.path()).expect_err("must refuse a readable key");
    assert!(
        format!("{err}").contains("must not be group- or world-accessible"),
        "unexpected error: {err}"
    );
}

#[test]
fn peers_persist_across_restarts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fingerprint = identity().fingerprint();

    {
        let mut store = Store::open(dir.path()).expect("open");
        store
            .add_peer(peer(fingerprint, "Galaxy S25"))
            .expect("add");
    }

    let store = Store::open(dir.path()).expect("reopen");
    let loaded = store.trusted_peer(&fingerprint).expect("peer must persist");
    assert_eq!(loaded.device_name, "Galaxy S25");
    assert!(loaded.allows("battery.v1"));
}

/// The placeholder a hand-built `Settings` carries is the product's name.
#[test]
fn the_placeholder_device_name_is_the_product_name() {
    assert_eq!(Settings::default().device_name, "Pliwee Device");
}

/// A default device name only ever applies to an identity created from now
/// on. A name already in `state.json` — including an OmniBridge-era default —
/// belongs to the user, and peers already store it: reopening must not
/// rename the device.
#[test]
fn a_stored_device_name_survives_the_rename() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let _store = Store::open(dir.path()).expect("open");
    }

    let path = dir.path().join("state.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
    let name = &mut state["settings"]["device_name"];
    assert!(
        name.is_string(),
        "state.json must carry settings.device_name"
    );
    *name = "OmniBridge Desktop".into();
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&state).expect("serialize"),
    )
    .expect("write");

    let store = Store::open(dir.path()).expect("reopen");
    assert_eq!(store.settings().device_name, "OmniBridge Desktop");
}

#[test]
fn revocation_survives_a_restart_and_hides_the_peer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fingerprint = identity().fingerprint();

    {
        let mut store = Store::open(dir.path()).expect("open");
        store
            .add_peer(peer(fingerprint, "Galaxy S25"))
            .expect("add");
        assert!(store.revoke_peer(&fingerprint).expect("revoke"));
    }

    let store = Store::open(dir.path()).expect("reopen");
    assert!(
        store.trusted_peer(&fingerprint).is_none(),
        "a revoked peer must not be returned as trusted"
    );

    let record = store.peer_record(&fingerprint).expect("record is kept");
    assert!(record.revoked);
    assert!(
        record.granted_capabilities.is_empty(),
        "revocation must drop capability grants, not just set a flag"
    );
    assert!(!record.allows("battery.v1"));
}

#[test]
fn revoking_an_unknown_peer_is_not_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    assert!(!store
        .revoke_peer(&identity().fingerprint())
        .expect("revoke"));
}

#[test]
fn capability_grants_are_independent_of_what_a_peer_advertises() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fingerprint = identity().fingerprint();
    let mut store = Store::open(dir.path()).expect("open");

    let mut p = peer(fingerprint, "Galaxy S25");
    p.granted_capabilities.clear();
    store.add_peer(p).expect("add");

    let loaded = store.trusted_peer(&fingerprint).expect("peer");
    assert!(
        !loaded.allows("battery.v1"),
        "nothing is granted by default"
    );

    store
        .set_capability_grant(&fingerprint, "battery.v1", true)
        .expect("grant");
    assert!(store
        .trusted_peer(&fingerprint)
        .expect("peer")
        .allows("battery.v1"));
}

#[test]
fn a_newer_schema_version_is_refused_rather_than_misread() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let _store = Store::open(dir.path()).expect("open");
    }

    let path = dir.path().join("state.json");
    let raw = std::fs::read_to_string(&path).expect("read");
    // Written against whatever the current schema is, rather than against the
    // literal `1`, so that a future schema bump does not silently turn this
    // test into a no-op that passes because the replacement never happened.
    let bumped = raw.replace(
        &format!("\"schema_version\": {}", pliwee_core::store::SCHEMA_VERSION),
        "\"schema_version\": 99",
    );
    assert_ne!(raw, bumped, "schema_version must be present in state.json");
    std::fs::write(&path, bumped).expect("write");

    let err = Store::open(dir.path()).expect_err("must refuse a future schema");
    assert!(format!("{err}").contains("newer than supported"), "{err}");
}

#[test]
fn store_never_persists_message_or_clipboard_content() {
    // A structural guard: the on-disk state should contain only identity,
    // settings and peers. If a future change starts writing payloads here,
    // this test is meant to be the thing that notices.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = Store::open(dir.path()).expect("open");
    store
        .add_peer(peer(identity().fingerprint(), "Galaxy S25"))
        .expect("add");

    let raw = std::fs::read_to_string(dir.path().join("state.json")).expect("read");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("json");
    // serde_json's map is ordered, so compare against a sorted expectation.
    let keys: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "certificate_der_b64",
            "device_id",
            // Added by Wave 0. Confirmed against this test's own instruction:
            // it holds `"software"` — an enum naming how the private key is
            // protected — and no user content. It exists so that "this device
            // never had hardware backing" can be told apart from "the
            // hardware backing has gone away", which is the difference
            // between a legitimate software key and a refusal to start.
            "key_backing",
            "peers",
            "schema_version",
            "settings"
        ],
        "state.json gained a top-level field; confirm it holds no user content"
    );
}

// ---------------------------------------------------------------------------
// Cross-language SPKI fingerprint fixtures
// ---------------------------------------------------------------------------
//
// `protocol/testdata/identity-{a,b}.der` are real certificates emitted by
// `cargo run -p pliwee-core --example gen_test_vectors`. The Kotlin suite
// reads the same two files and must derive the same fingerprints, which makes
// "the identity is SHA-256 over the DER SubjectPublicKeyInfo" a checked
// contract between the two implementations rather than a shared convention.
//
// The fixtures and these values are frozen (ADR-0020 D10; see
// `frozen_vectors.rs`). They are never regenerated to remove a historical name.

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()))
}

const FIXTURE_A_FINGERPRINT: &str =
    "1b759fb323a5c5260a0f762692d1f42458c5102f68e1e271699f1fa694769821";
const FIXTURE_B_FINGERPRINT: &str =
    "f6b9ec37af6fa7cde4ea5377399c067cdfcb09def4d2ac701e11c95521d8efb7";

#[test]
fn fixture_certificates_have_the_expected_spki_fingerprints() {
    let a = Fingerprint::from_certificate_der(&fixture("identity-a.der")).expect("fixture a");
    let b = Fingerprint::from_certificate_der(&fixture("identity-b.der")).expect("fixture b");

    assert_eq!(a.to_hex(), FIXTURE_A_FINGERPRINT);
    assert_eq!(b.to_hex(), FIXTURE_B_FINGERPRINT);
    assert_ne!(a, b, "the two fixtures must be different identities");
}

#[test]
fn fingerprint_covers_the_public_key_not_the_whole_certificate() {
    // Same claim the Kotlin suite makes: hashing the certificate bytes must
    // not accidentally be what `from_certificate_der` does, or reissuing a
    // certificate would silently break every existing pairing.
    let der = fixture("identity-a.der");
    let over_certificate = Fingerprint::from_spki_der(&der);
    let over_spki = Fingerprint::from_certificate_der(&der).expect("fixture a");
    assert_ne!(over_certificate, over_spki);
    assert_eq!(over_spki.to_hex(), FIXTURE_A_FINGERPRINT);
}
