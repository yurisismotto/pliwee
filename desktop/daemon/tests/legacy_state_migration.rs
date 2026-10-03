//! Gate G4 at the daemon binary: no identity is ever regenerated while legacy
//! OmniBridge state exists (ADR-0020 D9/D12; implementation plan, Wave 4).
//!
//! The real daemon is started as a process, with `HOME` pointed at a scratch
//! directory and `XDG_DATA_HOME` **unset**, so the path resolution under test
//! is the one a packaged install uses. What it reports over its control socket
//! is compared field by field with what was in the legacy store before it
//! started.
//!
//! The legacy state here is written by `Store::open`, the code path OmniBridge
//! 1.0.0 itself used. It is *not* the captured state of a real v1.0.0 install;
//! that fixture comes from packaged guests and is a separate gate.
//!
//! **Linux only.** What this gate measures — `~/.local/share/omnibridge`
//! carried into `~/.local/share/pliwee`, a socket under `XDG_RUNTIME_DIR` — is
//! the Linux adapter's behaviour. OmniBridge never shipped on macOS, so the
//! macOS agent has nothing to migrate and keeps its state under
//! `~/Library/Application Support`; run there, this file would measure the
//! absence of a feature the platform does not have. The whole file is
//! compiled out on macOS rather than skipped test by test, so it cannot
//! report a PASS for something that did not run — the same classification
//! `capabilities/notifications/tests/real_dbus.rs` uses for its feature.

#![cfg(not(target_os = "macos"))]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pliwee_control::{Request, Response, StatusReport};
use pliwee_core::clipboard_policy::ClipboardPolicy;
use pliwee_core::notification_policy::NotificationPolicy;
use pliwee_core::store::{Store, TrustedPeer};
use pliwee_core::Fingerprint;
use sha2::{Digest, Sha256};

struct Home {
    _root: tempfile::TempDir,
    home: PathBuf,
    runtime: PathBuf,
    log: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let runtime = root.path().join("run");
        std::fs::create_dir_all(home.join(".local/share")).expect("mkdir");
        std::fs::create_dir_all(home.join("Downloads")).expect("mkdir");
        std::fs::create_dir(&runtime).expect("mkdir");
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let log = root.path().join("daemon.log");
        Self {
            _root: root,
            home,
            runtime,
            log,
        }
    }

    fn legacy(&self) -> PathBuf {
        self.home.join(".local/share/omnibridge")
    }

    fn canonical(&self) -> PathBuf {
        self.home.join(".local/share/pliwee")
    }

    fn socket(&self) -> PathBuf {
        self.runtime.join("pliwee/control.sock")
    }

    /// Starts the daemon binary in this home. The session bus address points
    /// at nothing, so no tray item or notification backend reaches the real
    /// desktop session running these tests.
    fn spawn(&self) -> Child {
        let log = std::fs::File::create(&self.log).expect("log");
        Command::new(env!("CARGO_BIN_EXE_pliweed"))
            .args(["--port", "0", "--no-mdns"])
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_DOWNLOAD_DIR", self.home.join("Downloads"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.runtime.join("no-bus").display()),
            )
            .env("RUST_LOG", "info")
            .stdin(Stdio::null())
            // `tracing` writes to stdout, the final error to stderr: both
            // are the daemon's account of this run.
            .stdout(log.try_clone().expect("log"))
            .stderr(log)
            .spawn()
            .expect("spawn the daemon")
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Waits for the control socket and asks for `status`.
    fn status(&self, child: &mut Child) -> StatusReport {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(code) = child.try_wait().expect("wait") {
                panic!(
                    "daemon exited ({code}) before answering:\n{}",
                    self.log_text()
                );
            }
            if let Ok(mut stream) = UnixStream::connect(self.socket()) {
                let mut line = serde_json::to_string(&Request::Status).expect("json");
                line.push('\n');
                stream.write_all(line.as_bytes()).expect("write");
                let mut reply = String::new();
                BufReader::new(stream).read_line(&mut reply).expect("read");
                match serde_json::from_str::<Response>(&reply).expect("parse") {
                    Response::Status(s) => return s,
                    other => panic!("unexpected reply {other:?}"),
                }
            }
            assert!(
                Instant::now() < deadline,
                "no control socket at {} within 60s:\n{}",
                self.socket().display(),
                self.log_text()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn stop(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn sha256_files(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .expect("list")
        .map(|e| {
            let e = e.expect("entry");
            let bytes = std::fs::read(e.path()).expect("read");
            (
                e.file_name().to_string_lossy().into_owned(),
                data_hex(&Sha256::digest(&bytes)),
            )
        })
        .collect();
    out.sort();
    out
}

fn data_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn peer(name: &str, grants: &[&str]) -> TrustedPeer {
    TrustedPeer {
        device_id: format!("{name}-id"),
        device_name: name.to_string(),
        platform: 2,
        fingerprint: Fingerprint::from_spki_der(name.as_bytes()),
        paired_at_unix: 1_790_000_000,
        granted_capabilities: grants.iter().map(|g| (g.to_string(), true)).collect(),
        last_protocol_version: 1,
        revoked: false,
        hidden: false,
        clipboard_policy: ClipboardPolicy::default(),
        notification_policy: NotificationPolicy::default(),
    }
}

/// What OmniBridge 1.0.0 would have left behind: an identity, a granted peer
/// with non-default policies, and a revoked one.
struct Before {
    fingerprint: String,
    device_id: String,
    listed: String,
    paired: usize,
    grants: Vec<(String, Vec<String>)>,
}

fn write_legacy_state(dir: &Path) -> Before {
    let mut store = Store::open(dir).expect("legacy store");
    let phone = peer("phone", &["clipboard.v1", "files.v1"]);
    let fp = phone.fingerprint;
    store.add_peer(phone).expect("add");
    store
        .set_clipboard_policy(
            &fp,
            ClipboardPolicy {
                allow_send: false,
                ..ClipboardPolicy::default()
            },
        )
        .expect("clipboard policy");
    store
        .set_notification_policy(
            &fp,
            NotificationPolicy {
                allow_mirror: false,
                ..NotificationPolicy::default()
            },
        )
        .expect("notification policy");
    let old = peer("old-laptop", &["files.v1"]);
    let old_fp = old.fingerprint;
    store.add_peer(old).expect("add");
    store.revoke_peer(&old_fp).expect("revoke");
    before_of(&store)
}

fn before_of(store: &Store) -> Before {
    let mut grants: Vec<_> = store
        .listed_peers()
        .map(|p| {
            (
                p.fingerprint.to_hex(),
                p.granted_capabilities
                    .iter()
                    .filter(|(_, on)| **on)
                    .map(|(k, _)| k.clone())
                    .collect(),
            )
        })
        .collect();
    grants.sort();
    Before {
        fingerprint: store.identity().fingerprint().to_hex(),
        device_id: store.identity().device_id().to_string(),
        listed: listed(store),
        paired: store.listed_peers().filter(|p| !p.revoked).count(),
        grants,
    }
}

fn listed(store: &Store) -> String {
    let peers: Vec<_> = store.peers().collect();
    serde_json::to_string(&peers).expect("json")
}

fn status_grants(s: &StatusReport) -> Vec<(String, Vec<String>)> {
    let mut g: Vec<_> = s
        .devices
        .iter()
        .map(|d| {
            let mut caps = d.granted_capabilities.clone();
            caps.sort();
            (d.fingerprint.clone(), caps)
        })
        .collect();
    g.sort();
    g
}

#[test]
fn an_omnibridge_identity_is_carried_over_and_never_regenerated() {
    let h = Home::new();
    let before = write_legacy_state(&h.legacy());
    assert_eq!(before.paired, 1);
    assert_carried_over(&h, &before);
}

/// The Wave 4 integration gate on state captured from a **real** OmniBridge
/// 1.0.0 install, one distribution per run:
///
/// ```text
/// W4_CAPTURED_STATE=<dir> cargo test -p pliwee-daemon \
///     --test legacy_state_migration -- --ignored captured
/// ```
///
/// `<dir>` is the archived `~/.local/share/omnibridge` of a guest on which
/// the packaged 1.0.0 daemon ran, paired with a peer and had grants and
/// policies set — extracted with its modes preserved. Without it this test
/// fails: a gate whose input is absent has measured nothing.
#[test]
#[ignore = "needs state captured from a real OmniBridge 1.0.0 install"]
fn captured_v1_0_0_state_is_carried_over() {
    let src = std::env::var_os("W4_CAPTURED_STATE")
        .map(PathBuf::from)
        .expect("NOT EXECUTED: W4_CAPTURED_STATE is not set");
    for name in ["identity.key", "state.json"] {
        let len = std::fs::metadata(src.join(name))
            .unwrap_or_else(|e| panic!("{}: {e}", src.join(name).display()))
            .len();
        assert!(len > 0, "{name} in the capture is empty");
    }

    let h = Home::new();
    copy_dir(&src, &h.legacy());
    // "Before" is read from a second copy: opening a store may write to it,
    // and the legacy directory under test must be exactly the capture.
    let scratch = tempfile::tempdir().expect("tempdir");
    let reference = scratch.path().join("omnibridge");
    copy_dir(&src, &reference);
    let before = before_of(&Store::open(&reference).expect("the capture must open"));
    assert!(
        before.paired > 0,
        "the capture has no paired peer; it cannot show that pairings survive"
    );
    assert_carried_over(&h, &before);
}

/// Copies one flat directory, keeping the modes, which are part of what is
/// being tested.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("mkdir");
    for e in std::fs::read_dir(src).expect("list") {
        let e = e.expect("entry");
        std::fs::copy(e.path(), dst.join(e.file_name())).expect("copy");
    }
    let mode = std::fs::metadata(src).expect("stat").permissions().mode();
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// O1 → first start → O2 → second start, with every equality asserted.
fn assert_carried_over(h: &Home, before: &Before) {
    let legacy_hashes = sha256_files(&h.legacy());
    // O1's precondition, observed rather than assumed.
    assert!(!h.canonical().exists(), "P must start absent");

    // ---- first start: migrates -------------------------------------------
    let mut child = h.spawn();
    let s = h.status(&mut child);
    stop(child);
    let log = h.log_text();

    // Anchor: the daemon itself said, in this run, where it migrated from.
    // The full this-run wording: a later start's "was migrated from … by an
    // earlier start" contains the shorter phrase too.
    let anchor = format!(
        "migrated from {}: identity and trust store copied",
        h.legacy().display()
    );
    assert!(log.contains(&anchor), "no '{anchor}' in:\n{log}");

    assert_eq!(s.fingerprint, before.fingerprint, "fingerprint changed");
    assert_eq!(s.device_id, before.device_id, "device id changed");
    assert_eq!(s.paired_devices, before.paired, "paired count changed");
    assert_eq!(status_grants(&s), before.grants, "grants changed");
    let m = s
        .migrated_from
        .as_ref()
        .expect("status reports the migration");
    assert_eq!(m.source, h.legacy().display().to_string());
    assert!(m.this_run);

    // The legacy directory is byte-for-byte what it was.
    assert_eq!(sha256_files(&h.legacy()), legacy_hashes, "L was modified");
    let record = std::fs::read(h.canonical().join("MIGRATED_FROM")).expect("record");

    // Every peer record — policies and the revocation included — survived.
    let after = Store::open(h.canonical()).expect("open P");
    assert_eq!(listed(&after), before.listed, "trust store differs");
    drop(after);

    // ---- second start: nothing re-migrated -------------------------------
    let mut child = h.spawn();
    let s2 = h.status(&mut child);
    stop(child);
    let log2 = h.log_text();
    assert!(!log2.contains(&anchor), "migrated twice:\n{log2}");
    assert!(log2.contains("by an earlier start"), "{log2}");
    assert_eq!(s2.fingerprint, before.fingerprint);
    assert_eq!(s2.device_id, before.device_id);
    assert_eq!(s2.paired_devices, before.paired);
    assert!(!s2.migrated_from.as_ref().expect("still reported").this_run);
    assert_eq!(
        std::fs::read(h.canonical().join("MIGRATED_FROM")).expect("record"),
        record,
        "MIGRATED_FROM was rewritten"
    );
    assert_eq!(sha256_files(&h.legacy()), legacy_hashes, "L was modified");
}

#[test]
fn unreadable_legacy_state_refuses_to_start_and_creates_no_identity() {
    let h = Home::new();
    write_legacy_state(&h.legacy());
    std::fs::set_permissions(h.legacy(), std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read_dir(h.legacy()).is_ok() {
        std::fs::set_permissions(h.legacy(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
        panic!("running with CAP_DAC_OVERRIDE: EACCES cannot be produced here");
    }
    // Observation 1: no identity in P.
    assert!(!h.canonical().join("identity.key").exists());

    let mut child = h.spawn();
    let deadline = Instant::now() + Duration::from_secs(60);
    let code = loop {
        if let Some(code) = child.try_wait().expect("wait") {
            break code;
        }
        if Instant::now() > deadline {
            stop(child);
            panic!("the daemon kept running on unreadable legacy state");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    std::fs::set_permissions(h.legacy(), std::fs::Permissions::from_mode(0o700)).expect("chmod");

    let log = h.log_text();
    assert!(!code.success(), "exit {code}:\n{log}");
    assert!(
        log.contains(&h.legacy().display().to_string()),
        "the refusal does not name {}:\n{log}",
        h.legacy().display()
    );
    // Observation 2: still no identity in P, and no P at all.
    assert!(!h.canonical().join("identity.key").exists(), "{log}");
    assert!(!h.canonical().exists(), "{log}");
    assert!(!h.socket().exists(), "a refused daemon bound its socket");
}

#[test]
fn a_fresh_home_is_a_first_run_under_the_pliwee_directory_only() {
    let h = Home::new();
    let mut child = h.spawn();
    let s = h.status(&mut child);
    stop(child);

    assert!(s.migrated_from.is_none());
    assert!(h.canonical().join("identity.key").exists());
    assert!(!h.legacy().exists(), "a legacy directory was created");
    let store = Store::open(h.canonical()).expect("open");
    assert_eq!(store.identity().fingerprint().to_hex(), s.fingerprint);
}

#[test]
fn interrupted_omnibridge_transfers_are_reported_and_left_in_place() {
    let h = Home::new();
    let legacy_dl = h.home.join("Downloads/OmniBridge");
    std::fs::create_dir_all(&legacy_dl).expect("mkdir");
    let part = legacy_dl.join(".omnibridge-00ff.part");
    std::fs::write(&part, b"half a file").expect("write");
    std::fs::write(legacy_dl.join("done.txt"), b"a whole file").expect("write");

    let mut child = h.spawn();
    let s = h.status(&mut child);
    stop(child);

    assert_eq!(s.legacy_partial_files, vec![part.display().to_string()]);
    assert_eq!(std::fs::read(&part).expect("still there"), b"half a file");
    assert!(legacy_dl.join("done.txt").exists());
    assert!(
        h.home.join("Downloads/Pliwee").is_dir(),
        "new files go to Downloads/Pliwee"
    );
}
