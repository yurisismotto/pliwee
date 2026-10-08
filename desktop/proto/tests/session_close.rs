//! `SessionClose`: dormant protocol-version-2 wire scaffolding (GitHub #90).
//!
//! MULTI-DEVICE-MESH-V2.md §2 fixes the shape: Envelope body field 25,
//! `UNSPECIFIED = 0`, `SUPERSEDED = 1`. Those numbers are the wire, so they are
//! asserted from the compiled descriptors and from known-answer bytes, not
//! from the generated Rust names alone. A renumbering would still compile and
//! still round-trip against itself; it would not interoperate.
//!
//! The message is inert until protocol version 2 is negotiated, and this
//! build negotiates 1. `no_production_source_sends_session_close` keeps it
//! that way until the issue that activates it says otherwise.

use std::path::{Path, PathBuf};

use pliwee_proto::v1;
use pliwee_proto::Message;
use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{DescriptorProto, FileDescriptorSet};

fn descriptors() -> FileDescriptorSet {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/proto")
        .canonicalize()
        .expect("the protocol directory should exist");
    protox::compile(["pliwee/v1/envelope.proto"], [&root]).expect("the schema should compile")
}

fn message(name: &str) -> DescriptorProto {
    descriptors()
        .file
        .into_iter()
        .flat_map(|f| f.message_type)
        .find(|m| m.name() == name)
        .unwrap_or_else(|| panic!("message {name} should exist"))
}

#[test]
fn session_close_is_envelope_body_field_25() {
    let envelope = message("Envelope");
    let field = envelope
        .field
        .iter()
        .find(|f| f.name() == "session_close")
        .expect("Envelope.session_close should exist");
    assert_eq!(field.number(), 25);
    assert_eq!(field.r#type(), Type::Message);
    assert_eq!(field.type_name(), ".pliwee.v1.SessionClose");

    let body = envelope
        .oneof_decl
        .iter()
        .position(|o| o.name() == "body")
        .expect("Envelope.body should exist");
    assert_eq!(
        field.oneof_index,
        Some(body as i32),
        "must be in oneof body"
    );

    // Exactly one field claims 25.
    assert_eq!(
        envelope.field.iter().filter(|f| f.number() == 25).count(),
        1
    );
}

#[test]
fn session_close_carries_only_a_reason() {
    let m = message("SessionClose");
    assert_eq!(m.field.len(), 1, "{:?}", m.field);
    let reason = &m.field[0];
    assert_eq!(reason.name(), "reason");
    assert_eq!(reason.number(), 1);
    assert_eq!(reason.label(), Label::Optional);
    assert_eq!(reason.r#type(), Type::Enum);
    assert_eq!(reason.type_name(), ".pliwee.v1.SessionCloseReason");
}

#[test]
fn session_close_reasons_have_stable_numbers() {
    let reasons = descriptors()
        .file
        .into_iter()
        .flat_map(|f| f.enum_type)
        .find(|e| e.name() == "SessionCloseReason")
        .expect("SessionCloseReason should exist");
    let values: Vec<(String, i32)> = reasons
        .value
        .iter()
        .map(|v| (v.name().to_owned(), v.number()))
        .collect();
    assert_eq!(
        values,
        [
            ("SESSION_CLOSE_REASON_UNSPECIFIED".to_owned(), 0),
            ("SESSION_CLOSE_REASON_SUPERSEDED".to_owned(), 1),
        ]
    );
    assert_eq!(v1::SessionCloseReason::Unspecified as i32, 0);
    assert_eq!(v1::SessionCloseReason::Superseded as i32, 1);
}

fn superseded() -> v1::envelope::Body {
    v1::envelope::Body::SessionClose(v1::SessionClose {
        reason: v1::SessionCloseReason::Superseded as i32,
    })
}

#[test]
fn a_superseded_close_round_trips() {
    let original = v1::Envelope {
        protocol_version: 2,
        message_id: vec![7u8; 16],
        sequence: 9,
        timestamp_unix_ms: 1_700_000_000_000,
        correlation_id: Vec::new(),
        body: Some(superseded()),
    };
    let decoded = v1::Envelope::decode(&original.encode_to_vec()[..]).expect("decode");
    assert_eq!(decoded, original);
    match decoded.body {
        Some(v1::envelope::Body::SessionClose(c)) => {
            assert_eq!(c.reason(), v1::SessionCloseReason::Superseded);
        }
        other => panic!("wrong body: {other:?}"),
    }
}

#[test]
fn a_superseded_close_has_known_answer_bytes() {
    // Tag (25 << 3) | 2 = 202 = varint CA 01; length 2; then
    // SessionClose.reason: tag (1 << 3) | 0 = 08, value 01.
    let only_body = v1::Envelope {
        body: Some(superseded()),
        ..Default::default()
    };
    assert_eq!(only_body.encode_to_vec(), [0xCA, 0x01, 0x02, 0x08, 0x01]);

    let decoded = v1::Envelope::decode(&[0xCA, 0x01, 0x02, 0x08, 0x01][..]).expect("decode");
    assert_eq!(decoded.body, Some(superseded()));
}

#[test]
fn an_unknown_reason_degrades_to_unspecified() {
    // A later SPEC may add reasons; an old V2 peer must not crash on one, and
    // must not read it as SUPERSEDED. This is prost's behaviour; Java-lite
    // reports `UNRECOGNIZED` instead, which Android must treat the same way.
    let close = v1::SessionClose { reason: 9999 };
    let decoded = v1::SessionClose::decode(&close.encode_to_vec()[..]).expect("decode");
    assert_eq!(decoded.reason, 9999);
    assert_eq!(decoded.reason(), v1::SessionCloseReason::Unspecified);
}

// ---------------------------------------------------------------------------
// Dormancy: no production code sends it yet
// ---------------------------------------------------------------------------

/// Every `extension` file below `dir` that sits under a `src` directory (or
/// anywhere, once `in_src`), skipping build output and test trees.
fn production_sources(dir: &Path, extension: &str, in_src: bool, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(
                name,
                "target" | "build" | ".gradle" | "tests" | "test" | "androidTest"
            ) {
                continue;
            }
            production_sources(&path, extension, in_src || name == "src", out);
        } else if in_src && path.extension().and_then(|e| e.to_str()) == Some(extension) {
            out.push(path);
        }
    }
}

/// The production files under `root`, and those whose text contains any of
/// `needles`.
fn mentioning(
    root: &Path,
    in_src: bool,
    extension: &str,
    needles: &[&str],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    production_sources(root, extension, in_src, &mut files);
    let hits = files
        .iter()
        .filter(|p| {
            let text =
                std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            needles.iter().any(|n| text.contains(n))
        })
        .cloned()
        .collect();
    (files, hits)
}

#[test]
fn no_production_source_sends_session_close() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root should exist");

    // Desktop. The positive control proves the scan reads the file that
    // builds every established-session body: if it stopped seeing
    // `Body::CapabilityMessage` there, an empty result would mean nothing.
    let desktop = repo.join("desktop");
    let session_rs = desktop.join("core/src/session.rs");
    let (files, control) = mentioning(&desktop, false, "rs", &["Body::CapabilityMessage("]);
    assert!(
        files.contains(&session_rs),
        "scan missed {}",
        session_rs.display()
    );
    assert!(control.contains(&session_rs), "positive control not found");
    let (_, hits) = mentioning(&desktop, false, "rs", &["SessionClose", "session_close"]);
    assert!(
        hits.is_empty(),
        "production Rust mentions SessionClose: {hits:?}"
    );

    // Android. Same shape, anchored on the session dispatch.
    let android = repo.join("android/app/src/main");
    let peer_kt = android.join("java/io/github/yurisismotto/pliwee/net/PeerConnection.kt");
    let (files, control) = mentioning(
        &android,
        true,
        "kt",
        &["Envelope.BodyCase.CAPABILITY_MESSAGE"],
    );
    assert!(
        files.contains(&peer_kt),
        "scan missed {}",
        peer_kt.display()
    );
    assert!(control.contains(&peer_kt), "positive control not found");
    let (_, hits) = mentioning(
        &android,
        true,
        "kt",
        &["SessionClose", "sessionClose", "SESSION_CLOSE"],
    );
    assert!(
        hits.is_empty(),
        "production Kotlin mentions SessionClose: {hits:?}"
    );
}
