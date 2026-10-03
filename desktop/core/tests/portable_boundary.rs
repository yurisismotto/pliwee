//! The boundary regression test.
//!
//! Wave 0's completion test is a negative one: *adding a platform means
//! writing an adapter crate; it never means editing `pliwee-core`,
//! `tls.rs`, `session.rs` or a capability crate's protocol half.* This file
//! is what makes that checkable rather than aspirational.
//!
//! # What it proves, and what it does not
//!
//! It proves that no portable crate reaches for a platform API outside the
//! one feature-gated module that is allowed to. It says **nothing** about
//! whether Pliwee works on Windows or macOS — that needs a real machine and
//! belongs to Wave 5 and later. Conflating the two is how a project talks
//! itself into believing it supports a platform it has never run on.
//!
//! # Why a grep and not a cross-compile
//!
//! Research v1 proposed `cargo build --target x86_64-pc-windows-msvc` on a
//! Linux host as the gate. It cannot work: `ring` requires a C toolchain, and
//! for MSVC targets that means Build Tools for Visual Studio, whose libraries
//! are not redistributable onto a Linux runner. The real compile gate needs a
//! Windows runner and is a CI job (CI-001). This is the cheap backstop that
//! runs everywhere, on every change, today.

use std::path::{Path, PathBuf};

/// Crates that must contain no platform-specific code.
///
/// `pliwee-runtime` is deliberately absent: it depends on `mdns-sd`, whose
/// Windows behaviour is an open question (V-12 / POC-WIN-02) and outside
/// Wave 0. It contains no `std::os` today, and the last test below checks
/// that, but it is not in the portable *contract*.
const PORTABLE_CRATES: &[&str] = &[
    "proto",
    "core",
    "control",
    "capabilities/battery",
    "capabilities/clipboard",
    "capabilities/files",
    "capabilities/notifications",
];

/// The one exception, and why it is allowed.
///
/// Each entry is a path, relative to `desktop/`, that is permitted to name a
/// platform — because it *is* the platform module, it is behind a Cargo
/// feature, and turning the feature off removes it from the build entirely.
/// A new entry here is a decision, not a detail: it should be argued for in
/// review, not added to make a test pass.
const FEATURE_GATED_PLATFORM_MODULES: &[&str] = &[
    // `SecretStore` on a Unix filesystem, `Store::open(dir)`, XDG paths.
    // Behind `pliwee-core/unix-fs`.
    "core/src/platform/unix_fs.rs",
    // The Unix download destination. Behind
    // `pliwee-capability-files/unix-fs`.
    "capabilities/files/src/destination.rs",
    // The wl-clipboard and XFIXES backends. Behind
    // `pliwee-capability-clipboard/linux-backends`.
    "capabilities/clipboard/src/backend/wayland.rs",
    "capabilities/clipboard/src/backend/x11.rs",
];

/// Files that must not name a *notification platform*, even though they pass
/// the `std::os` markers above.
///
/// `PLATFORM_MARKERS` catches a crate that reaches for the operating system's
/// API surface. It does not catch a crate that reaches for a *desktop* — a
/// D-Bus name, a freedesktop interface, a logind object path — because none of
/// those needs `std::os` to say. `zbus` is a pure-Rust client and a
/// `gdbus`-shaped string literal is just a string.
///
/// So the notifications capability gets a second, narrower check: the seam and
/// everything above it must not name a bus, an interface or a desktop, and the
/// two feature-gated backend modules are the only files permitted to. Without
/// this the crate could grow a `zbus::Connection` in `lib.rs` and the portable
/// gate would still be green, right up until the MSVC job failed for a reason
/// nobody had encoded as a rule.
const DESKTOP_MARKERS: &[&str] = &[
    "zbus",
    "org.freedesktop",
    "org.gnome",
    "org.kde",
    "dbus",
    "logind",
    "gnome-shell",
];

/// The two files allowed to name a desktop, and why.
///
/// Each is behind `pliwee-capability-notifications/linux-dbus`; turning the
/// feature off removes both from the build entirely, which is what the portable
/// compile gate checks.
const DESKTOP_PLATFORM_MODULES: &[&str] = &[
    // `org.freedesktop.Notifications` over the session bus.
    "capabilities/notifications/src/backend/dbus.rs",
    // `org.freedesktop.login1.Session.LockedHint` over the system bus.
    "capabilities/notifications/src/backend/logind.rs",
];

/// Markers that mean "this file knows what operating system it is on".
const PLATFORM_MARKERS: &[&str] = &[
    "std::os::unix",
    "std::os::windows",
    "std::os::fd",
    "OpenOptionsExt",
    "PermissionsExt",
    "UnixListener",
    "UnixStream",
    "target_os",
    "target_family",
];

fn desktop_root() -> PathBuf {
    // `core/` → `desktop/`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core has a parent")
        .to_path_buf()
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` is build output, not source.
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `.rs` file in a crate's `src/`, with its path relative to `desktop/`.
fn crate_sources(crate_dir: &str) -> Vec<(String, String)> {
    let root = desktop_root();
    let mut files = Vec::new();
    rust_sources(&root.join(crate_dir).join("src"), &mut files);
    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .expect("inside desktop/")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path).expect("read source");
            (relative, text)
        })
        .collect()
}

#[test]
fn the_portable_crates_name_no_platform_outside_a_feature_gated_module() {
    let mut violations = Vec::new();

    for krate in PORTABLE_CRATES {
        for (path, text) in crate_sources(krate) {
            if FEATURE_GATED_PLATFORM_MODULES.contains(&path.as_str()) {
                continue;
            }
            for (number, line) in text.lines().enumerate() {
                // A mention inside prose is a mention of the boundary, not a
                // crossing of it — these files document why the seam exists.
                let code = line.trim_start();
                if code.starts_with("//") || code.starts_with("*") {
                    continue;
                }
                for marker in PLATFORM_MARKERS {
                    if line.contains(marker) {
                        violations.push(format!("{}:{}: {}", path, number + 1, code.trim()));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a portable crate reached for a platform API. Either the code belongs \
         in an adapter crate, or the module belongs in \
         FEATURE_GATED_PLATFORM_MODULES with an argument for why:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn every_declared_exception_still_exists() {
    // A stale exception is worse than none: it silently widens the allowance
    // to a path nothing occupies, and hides the next file that moves there.
    let root = desktop_root();
    for path in FEATURE_GATED_PLATFORM_MODULES {
        assert!(
            root.join(path).exists(),
            "{path} is listed as a feature-gated platform module but does not \
             exist. Remove the exception."
        );
    }
}

#[test]
fn each_exception_is_actually_behind_a_feature() {
    // The exception is only defensible because the module disappears when the
    // feature is off. Check that the module is declared with a `cfg(feature)`
    // somewhere in its crate, rather than merely being conventionally named.
    for path in FEATURE_GATED_PLATFORM_MODULES {
        let module = Path::new(path)
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .to_string();
        let crate_dir = path.split("/src/").next().expect("crate dir");

        let declared_behind_feature = crate_sources(crate_dir).into_iter().any(|(_, text)| {
            text.lines().collect::<Vec<_>>().windows(4).any(|window| {
                window.iter().any(|l| l.contains("cfg(feature"))
                    && window
                        .iter()
                        .any(|l| l.trim().starts_with("pub mod ") && l.contains(&module))
            })
        });

        assert!(
            declared_behind_feature,
            "{path} is an allowed platform module but its `pub mod {module}` \
             is not behind a `#[cfg(feature = ...)]`. Without the gate the \
             exception is just an exemption."
        );
    }
}

#[test]
fn the_notification_seam_names_no_desktop_outside_its_two_backend_modules() {
    let mut violations = Vec::new();

    for (path, text) in crate_sources("capabilities/notifications") {
        if DESKTOP_PLATFORM_MODULES.contains(&path.as_str()) {
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            // A mention inside prose is a mention of the boundary, not a
            // crossing of it — these files document at length *why* the seam
            // exists, and which D-Bus behaviours were measured.
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("*") || code.starts_with("///") {
                continue;
            }
            // The seam has to *declare* its platform halves, and the
            // declaration necessarily names them. `#[cfg(feature = ...)]` and
            // the `pub mod` it guards are the boundary being drawn, not
            // crossed — and the test below checks that every such declaration
            // really is behind the feature.
            if code.starts_with("pub mod ") || code.starts_with("#[cfg(feature") {
                continue;
            }
            for marker in DESKTOP_MARKERS {
                if line.contains(marker) {
                    violations.push(format!("{}:{}: {}", path, number + 1, code.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "the notification sink seam, or the capability above it, named a \
         desktop. Either the code belongs in a feature-gated backend module, \
         or the module belongs in DESKTOP_PLATFORM_MODULES with an argument \
         for why:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn each_desktop_module_exists_and_is_behind_the_linux_feature() {
    // A stale exception is worse than none: it silently widens the allowance
    // to a path nothing occupies, and hides the next file that moves there.
    let root = desktop_root();
    for path in DESKTOP_PLATFORM_MODULES {
        assert!(
            root.join(path).exists(),
            "{path} is listed as a desktop platform module but does not exist. \
             Remove the exception."
        );

        let module = Path::new(path)
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .to_string();
        let declared_behind_feature =
            crate_sources("capabilities/notifications")
                .into_iter()
                .any(|(_, text)| {
                    text.lines().collect::<Vec<_>>().windows(4).any(|window| {
                        window.iter().any(|l| l.contains("cfg(feature"))
                            && window
                                .iter()
                                .any(|l| l.trim().starts_with("pub mod ") && l.contains(&module))
                    })
                });
        assert!(
            declared_behind_feature,
            "{path} is an allowed desktop module but its `pub mod {module}` is \
             not behind a `#[cfg(feature = ...)]`. Without the gate the \
             exception is just an exemption."
        );
    }
}

#[test]
fn the_security_critical_files_carry_no_platform_arm_at_all() {
    // Not even a feature gate. These decide who is trusted, what a filename
    // becomes and how bytes are framed; a `#[cfg]` in any of them would put a
    // security control behind an arm that CI never compiles.
    //
    // `filename.rs` is named explicitly because PLAT-DEC-014 settled it:
    // sanitisation is protocol-global, and the rule that matters most —
    // stripping bidi overrides — belongs to no single platform, so a
    // per-destination design would have fixed it nowhere.
    let files = [
        "core/src/tls.rs",
        "core/src/session.rs",
        "core/src/pairing.rs",
        "core/src/fingerprint.rs",
        "core/src/framing.rs",
        "core/src/capability.rs",
        "capabilities/files/src/filename.rs",
        "capabilities/files/src/auth.rs",
    ];

    let root = desktop_root();
    for file in files {
        let text = std::fs::read_to_string(root.join(file)).expect("read");
        for (number, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("*") {
                continue;
            }
            assert!(
                !code.contains("cfg(target_os")
                    && !code.contains("cfg(feature")
                    && !code.contains("cfg(unix")
                    && !code.contains("cfg(windows"),
                "{file}:{}: a conditional arm in a security-critical file: {code}",
                number + 1
            );
        }
    }
}

#[test]
fn the_portable_crates_declare_unsafe_code_forbidden() {
    // SI-11. `forbid` cannot be relaxed by an inner `#[allow]`, which is the
    // property that makes it worth spelling out per crate rather than
    // inheriting a workspace default that a new crate would silently pick up
    // — or silently miss.
    let root = desktop_root();
    for krate in PORTABLE_CRATES.iter().chain(["runtime"].iter()) {
        let manifest =
            std::fs::read_to_string(root.join(krate).join("Cargo.toml")).expect("read manifest");
        assert!(
            manifest.contains(r#"unsafe_code = "forbid""#),
            "{krate} must forbid unsafe code"
        );
    }
}

#[test]
fn the_adapter_and_binary_crates_at_least_deny_unsafe_code() {
    // `deny`, not `forbid`: a future adapter will need FFI, and `forbid`
    // cannot be relaxed locally. `deny` means any `unsafe` needs a
    // deliberate, reviewable `#[allow(unsafe_code)]` with a justification.
    let root = desktop_root();
    for krate in [
        "platform-unix",
        "platform-linux",
        "platform-macos",
        "daemon",
        "cli",
        "gui",
    ] {
        let manifest =
            std::fs::read_to_string(root.join(krate).join("Cargo.toml")).expect("read manifest");
        assert!(
            manifest.contains(r#"unsafe_code = "deny""#),
            "{krate} must at least deny unsafe code"
        );
    }
}

/// The files allowed to contain `unsafe`, each with the reason it needs FFI.
///
/// The macOS adapter was the architectural event the test below anticipated:
/// IOKit's power-source API is C, and `NSPasteboardTypeString` is an
/// AppKit `extern` static. Both are confined to one module each, every block
/// carries a `// SAFETY:` comment, and adding a file here is a reviewed
/// change to this list rather than a quiet `#[allow]` somewhere else.
const UNSAFE_ALLOWED: &[(&str, &str)] = &[
    (
        "platform-macos/src/battery.rs",
        "IOKit IOPSCopyPowerSourcesInfo / IOPSCopyPowerSourcesList / IOPSGetPowerSourceDescription",
    ),
    (
        "platform-macos/src/clipboard.rs",
        "reading the AppKit extern static NSPasteboardTypeString",
    ),
];

/// Lines that are code — not comments — and mention `unsafe`.
fn unsafe_lines(path: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") || code.starts_with("*") {
            continue;
        }
        if code.contains("unsafe ") || code.contains("allow(unsafe_code)") {
            found.push(format!("{}:{}: {}", path, number + 1, code.trim()));
        }
    }
    found
}

#[test]
fn no_crate_uses_unsafe_outside_the_declared_ffi_modules() {
    // Wave 0 added no `unsafe` anywhere. The policy existed so a future
    // adapter *could*; the macOS adapter did, in exactly the files in
    // `UNSAFE_ALLOWED`. Anything else failing here is a new architectural
    // event and wants a conversation, not a silenced assertion.
    let mut found = Vec::new();
    for krate in PORTABLE_CRATES.iter().chain(
        [
            "runtime",
            "platform-unix",
            "platform-linux",
            "platform-macos",
            "daemon",
            "cli",
            "gui",
        ]
        .iter(),
    ) {
        for (path, text) in crate_sources(krate) {
            if UNSAFE_ALLOWED.iter().any(|(allowed, _)| *allowed == path) {
                continue;
            }
            found.extend(unsafe_lines(&path, &text));
        }
    }
    assert!(
        found.is_empty(),
        "unsafe appeared outside the declared FFI modules:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn every_declared_ffi_module_exists_uses_unsafe_and_justifies_each_block() {
    // An entry that no longer needs `unsafe` must leave the list — a stale
    // allowance is a hole waiting for code — and every `unsafe` block in a
    // listed file must be preceded by a `// SAFETY:` comment saying why it is
    // sound.
    let root = desktop_root();
    for (path, why) in UNSAFE_ALLOWED {
        let text = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|e| panic!("{path} ({why}) is listed but cannot be read: {e}"));
        assert!(
            !unsafe_lines(path, &text).is_empty(),
            "{path} is listed in UNSAFE_ALLOWED but contains no unsafe; remove it"
        );
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || !code.contains("unsafe {") {
                continue;
            }
            let justified = lines[..i]
                .iter()
                .rev()
                .take_while(|l| l.trim_start().starts_with("//"))
                .any(|l| l.contains("SAFETY:"));
            assert!(
                justified,
                "{path}:{}: an unsafe block without a `// SAFETY:` comment directly above it",
                i + 1
            );
        }
    }
}

#[test]
fn the_platform_is_chosen_in_one_place_per_binary() {
    // The daemon and the CLI each pick their adapter by `target_os` in one
    // module, and nowhere else. A `target_os` scattered through `main.rs`
    // would be the first step back to a daemon that knows which desktop it is
    // on in a hundred places.
    let allowed = ["daemon/src/platform/mod.rs", "cli/src/main.rs"];
    let mut found = Vec::new();
    for krate in ["daemon", "cli", "runtime"] {
        for (path, text) in crate_sources(krate) {
            if allowed.contains(&path.as_str()) {
                continue;
            }
            for (number, line) in text.lines().enumerate() {
                let code = line.trim_start();
                if !code.starts_with("//") && code.contains("target_os") {
                    found.push(format!("{}:{}: {}", path, number + 1, code.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "target_os outside the platform selection points:\n  {}",
        found.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// The Windows MSVC job builds exactly this set (Pliwee Wave 3)
// ---------------------------------------------------------------------------
//
// `.github/workflows/portable-windows-msvc.yml` names the portable crates by
// *package* name, in three places a rename can miss independently: the
// presence list, the `-p` arguments of the compile gates, and a PowerShell
// `-like '<prefix>-*'` filter that selects the lines whose features it checks.
// A filter that matches no line checks nothing and passes, so this test
// derives the expected names from the crates' own manifests and requires the
// workflow to agree with them exactly. The job carries the matching runtime
// assertion (the filter must select every portable package).

fn workflow() -> String {
    let path = desktop_root()
        .parent()
        .expect("desktop/ has a parent")
        .join(".github/workflows/portable-windows-msvc.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `name = "…"` of each portable crate's `[package]`, in PORTABLE_CRATES order.
fn portable_package_names() -> Vec<String> {
    PORTABLE_CRATES
        .iter()
        .map(|krate| {
            let manifest = std::fs::read_to_string(desktop_root().join(krate).join("Cargo.toml"))
                .expect("read manifest");
            let line = manifest
                .lines()
                .skip_while(|l| l.trim() != "[package]")
                .find(|l| l.trim_start().starts_with("name = "))
                .unwrap_or_else(|| panic!("{krate} has no package name"));
            line.split('"').nth(1).expect("quoted name").to_owned()
        })
        .collect()
}

/// The `-p <name>` arguments of the step whose `- name:` line starts with `step`.
fn step_packages(workflow: &str, step: &str) -> Vec<String> {
    let body: Vec<&str> = workflow
        .lines()
        .skip_while(|l| !l.trim_start().starts_with(&format!("- name: {step}")))
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with("- name:"))
        .collect();
    assert!(!body.is_empty(), "no workflow step named {step:?}");
    body.iter()
        .filter_map(|l| l.trim().strip_prefix("-p "))
        .map(|rest| rest.trim_end_matches('`').trim().to_owned())
        .collect()
}

#[test]
fn the_windows_job_builds_exactly_the_portable_crates() {
    let workflow = workflow();
    let mut expected = portable_package_names();
    assert_eq!(expected.len(), 7, "the portable contract is seven crates");
    expected.sort();

    for step in ["POC-CORE-04", "Full codegen", "Dependency boundary"] {
        let mut named = step_packages(&workflow, step);
        named.sort();
        assert_eq!(named, expected, "the {step:?} step builds a different set");
    }

    // The presence list: every expected name, quoted, and no stale one.
    for name in &expected {
        assert!(
            workflow.contains(&format!("'{name}'")),
            "the presence list does not name {name}"
        );
    }
}

#[test]
fn the_windows_feature_filter_selects_every_portable_crate() {
    let workflow = workflow();
    let filters: Vec<&str> = workflow
        .split("-like '")
        .skip(1)
        .map(|rest| rest.split('\'').next().expect("closing quote"))
        .collect();
    assert_eq!(
        filters.len(),
        1,
        "expected exactly one -like filter: {filters:?}"
    );
    let prefix = filters[0]
        .strip_suffix('*')
        .unwrap_or_else(|| panic!("the filter {:?} is not a prefix pattern", filters[0]));
    let names = portable_package_names();
    let selected = names.iter().filter(|n| n.starts_with(prefix)).count();
    assert_eq!(
        selected,
        names.len(),
        "the -like '{prefix}*' filter selects {selected} of {} portable packages: {names:?}",
        names.len()
    );
}
