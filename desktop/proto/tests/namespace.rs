//! The protobuf namespace, asserted from descriptors.
//!
//! The package name is a cross-implementation identity: `prost` derives the
//! generated Rust module path from it (`OUT_DIR/pliwee.v1.rs`, included as
//! `pliwee_proto::v1`), and `java_package` decides where the Kotlin
//! classes land. A rename that updated one and missed the other would leave
//! two implementations that still compile and no longer agree about what they
//! are speaking.
//!
//! A grep over the `.proto` text would be defeated by a comment or a line
//! break, so this reads the compiled descriptors — the same ones the build
//! script feeds to `prost-build`. The Kotlin mirror is the
//! `generated protobuf types live in the pliwee namespace` case in
//! `android/app/src/test/.../WireIdentityTest.kt`.
//!
//! Recorded in ADR-0018; renamed from `omnibridge` to `pliwee` by ADR-0020
//! (Pliwee Wave 3). The protobuf package never reaches the wire, so it has no
//! legacy form: `omnibridge` is a dead namespace like the two before it.

use std::path::PathBuf;

use prost_types::FileDescriptorSet;

const FILES: [&str; 6] = [
    "pliwee/v1/envelope.proto",
    "pliwee/v1/core.proto",
    "pliwee/v1/capabilities/battery_v1.proto",
    "pliwee/v1/capabilities/files_v1.proto",
    "pliwee/v1/capabilities/clipboard_v1.proto",
    "pliwee/v1/capabilities/notifications_v1.proto",
];

fn descriptors() -> FileDescriptorSet {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/proto")
        .canonicalize()
        .expect("the protocol directory should exist");
    protox::compile(FILES, [&root]).expect("the schema should compile")
}

#[test]
fn every_file_declares_a_pliwee_package() {
    for file in descriptors().file {
        let name = file.name().to_owned();
        let package = file.package().to_owned();
        assert!(
            package == "pliwee.v1" || package == "pliwee.v1.capabilities",
            "{name} declares package {package:?}"
        );
    }
}

#[test]
fn capability_schemas_are_in_the_capabilities_package() {
    for file in descriptors().file {
        let expected = if file.name().contains("/capabilities/") {
            "pliwee.v1.capabilities"
        } else {
            "pliwee.v1"
        };
        assert_eq!(file.package(), expected, "{}", file.name());
    }
}

#[test]
fn the_java_package_tracks_the_proto_package() {
    // Kotlin resolves `io.github.yurisismotto.pliwee.proto.Envelope` from
    // this option, not from the package above. They are two independent
    // strings that have to move together.
    for file in descriptors().file {
        let java = file
            .options
            .as_ref()
            .and_then(|o| o.java_package.clone())
            .unwrap_or_else(|| panic!("{} declares no java_package", file.name()));
        let expected = file
            .package()
            .replace("pliwee.v1", "io.github.yurisismotto.pliwee.proto");
        assert_eq!(java, expected, "{}", file.name());
    }
}

#[test]
fn no_pre_rename_namespace_survives() {
    for file in descriptors().file {
        for dead in ["anyflow", "fedroid", "omnibridge"] {
            assert!(!file.package().contains(dead), "{}", file.name());
            assert!(!file.name().contains(dead));
            let java = file
                .options
                .as_ref()
                .map(|o| o.java_package())
                .unwrap_or("");
            assert!(!java.contains(dead), "{}: {java}", file.name());
        }
    }
}

// ---------------------------------------------------------------------------
// Wire neutrality of the Pliwee rename (Wave 3)
// ---------------------------------------------------------------------------

/// The namespace root every schema lives under.
const ROOT: &str = "pliwee";

/// Every wire-relevant fact of the schema set, one line per fact, with the
/// namespace root replaced by `<root>`.
fn field_table() -> String {
    use prost_types::DescriptorProto;

    fn strip(s: &str) -> String {
        s.replace(&format!(".{ROOT}."), ".<root>.")
            .replace(&format!("{ROOT}."), "<root>.")
            .replace(&format!("{ROOT}/"), "<root>/")
    }
    fn message(out: &mut Vec<String>, scope: &str, m: &DescriptorProto) {
        let scope = format!("{scope}.{}", m.name());
        out.push(format!("message {scope}"));
        for f in &m.field {
            out.push(format!(
                "  field {scope}.{} = {} label={:?} type={:?} type_name={} oneof={:?} proto3_optional={}",
                f.name(),
                f.number(),
                f.label(),
                f.r#type(),
                strip(f.type_name()),
                f.oneof_index,
                f.proto3_optional(),
            ));
        }
        for o in &m.oneof_decl {
            out.push(format!("  oneof {scope}.{}", o.name()));
        }
        for r in &m.reserved_range {
            out.push(format!("  reserved {scope} {:?}..{:?}", r.start, r.end));
        }
        for r in &m.reserved_name {
            out.push(format!("  reserved_name {scope} {r}"));
        }
        for e in &m.enum_type {
            for v in &e.value {
                out.push(format!(
                    "  enum {scope}.{}.{} = {}",
                    e.name(),
                    v.name(),
                    v.number()
                ));
            }
        }
        for n in &m.nested_type {
            message(out, &scope, n);
        }
    }

    let mut out = Vec::new();
    for file in descriptors().file {
        let package = strip(file.package());
        out.push(format!("file {} package={package}", strip(file.name())));
        for d in &file.dependency {
            out.push(format!("  import {}", strip(d)));
        }
        let o = file.options.clone().unwrap_or_default();
        out.push(format!(
            "  java_package={} outer={} multiple={}",
            strip(o.java_package()),
            o.java_outer_classname(),
            o.java_multiple_files(),
        ));
        for e in &file.enum_type {
            for v in &e.value {
                out.push(format!(
                    "  enum {package}.{}.{} = {}",
                    e.name(),
                    v.name(),
                    v.number()
                ));
            }
        }
        for m in &file.message_type {
            message(&mut out, &package, m);
        }
    }
    out.push(String::new());
    out.join("\n")
}

/// Taken from the descriptors **before** the rename (at `23a4503`, package
/// `omnibridge.v1`) and never regenerated since, only appended to by the
/// intentional changes listed below: the rename is wire-neutral
/// only if every field number, type, label, oneof and enum value is what it
/// was. A schema change that is meant to happen updates this file in its own
/// commit, not in a rename.
///
/// Intentional changes since the snapshot, each additive:
///
/// * GitHub #90 — `SessionCloseReason`, `SessionClose` and
///   `Envelope.session_close = 25` (protocol version 2 scaffolding,
///   MULTI-DEVICE-MESH-V2.md §2). Every line that was here before stands
///   unchanged.
const PRE_RENAME_FIELD_TABLE: &str = include_str!("descriptor-field-table.txt");

#[test]
fn the_field_table_is_identical_to_the_pre_rename_snapshot() {
    let table = field_table();
    // Non-vacuous: 6 files and 117 fields were measured before the rename;
    // #90 added two (`SessionClose.reason`, `Envelope.session_close`).
    assert_eq!(table.lines().filter(|l| l.starts_with("file ")).count(), 6);
    assert_eq!(
        table.lines().filter(|l| l.starts_with("  field ")).count(),
        119
    );
    for (i, (now, then)) in table
        .lines()
        .zip(PRE_RENAME_FIELD_TABLE.lines())
        .enumerate()
    {
        assert_eq!(
            now,
            then,
            "descriptor field table differs at line {}",
            i + 1
        );
    }
    assert_eq!(table, PRE_RENAME_FIELD_TABLE);
}
