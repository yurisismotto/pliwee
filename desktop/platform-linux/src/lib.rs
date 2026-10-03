//! The Linux adapter.
//!
//! Everything the Pliwee Agent needs that is specific to a Linux desktop
//! session, and nothing else:
//!
//! * the control endpoint — a Unix domain socket under `$XDG_RUNTIME_DIR`,
//!   implementing [`pliwee_control::transport::ControlTransport`];
//! * the client half of that endpoint, so the CLI and the GUI can reach the
//!   agent without depending on the agent;
//! * the store adapter — `$XDG_DATA_HOME/pliwee`, 0600 keys in a 0700
//!   directory — assembled from the pieces in `pliwee-core`, together with
//!   the carry-over of an OmniBridge identity from `$XDG_DATA_HOME/omnibridge`
//!   (ADR-0020 D9).
//!
//! # What is *not* here
//!
//! Protocol, TLS, pairing, pinning, the session state machine, the capability
//! registry, filename sanitisation, clipboard policy. None of it is
//! platform-specific and none of it may move here. If a future change wants
//! to put a protocol decision in an adapter, that is the signal that the seam
//! is in the wrong place.
//!
//! # The Pliwee Agent
//!
//! "Agent" is the portable name for the always-on user-session process. It is
//! one concept with a different lifetime on each platform:
//!
//! | Platform | Lifetime |
//! | --- | --- |
//! | **Linux** | `systemd --user` unit — *this crate* |
//! | Windows | a user-session agent, not a Windows Service |
//! | macOS | a `LoginItem` / `SMAppService` agent |
//! | Android | a foreground service |
//! | iOS | foreground-oriented, with no background socket |
//!
//! Wave 0 implements the Linux row and only the Linux row. The rows below it
//! are recorded so that the shape of this crate — bind an endpoint, resolve
//! paths, enforce local protection — is legible as *one row of a table*
//! rather than as the way Pliwee works.
//!
//! # The desktop-shell adapter
//!
//! [`tray`] is the third thing in this crate and the newest: a
//! `StatusNotifierItem` on the session bus, so that KDE Plasma can show
//! Pliwee in its system tray. It belongs here for the same reason the control
//! endpoint does — it is a *Linux desktop session* concept with no portable
//! meaning, and the portable crates must never learn the words "D-Bus" or
//! "tray". It is behind the `tray` feature, which is on by default for the
//! agent and off for the GUI and the CLI, neither of which has any business
//! owning a tray icon.

use std::path::{Path, PathBuf};

pub mod legacy_migration;
pub use legacy_migration::{
    migrate_config_file, migrate_data_dir, ConfigFiles, ConfigOrigin, DataDirOrigin, DataDirs,
    MigrationError, MigrationRecord,
};
pub use pliwee_core::platform::unix_fs::{default_data_dir, default_device_name, FileSecretStore};

/// Reading, never writing, how the account enables the daemon: the
/// OmniBridge-era `omnibridged.service` link (ADR-0020, rebrand Wave 7).
pub mod systemd_transition;

#[cfg(feature = "tray")]
pub mod tray;

/// Repairing the desktop application's D-Bus activation after an install into
/// a live session. Best effort, user session bus only, at most one
/// `ReloadConfig`.
#[cfg(feature = "desktop-activation")]
pub mod activation;

/// Opens the store at `dir` using this platform's storage and identity
/// backing.
///
/// The one place that says "this machine is a Linux machine". Before Wave 0
/// the value was hardcoded inside `pliwee-core`'s persistence layer, which
/// meant the storage code decided what kind of device this was.
pub fn open_store(dir: impl AsRef<Path>) -> pliwee_core::Result<pliwee_core::store::Store> {
    use std::sync::Arc;
    pliwee_core::store::Store::open_with(pliwee_core::store::StoreConfig {
        secrets: Arc::new(FileSecretStore::new(dir.as_ref())),
        backend: Arc::new(pliwee_core::identity::SoftwareBacking),
        platform: pliwee_proto::v1::Platform::Linux,
        default_device_name: default_device_name(),
    })
}

// ---------------------------------------------------------------------------
// The control endpoint
// ---------------------------------------------------------------------------

/// Path of the control socket: `$XDG_RUNTIME_DIR/pliwee/control.sock`.
///
/// `XDG_RUNTIME_DIR` is per-user and mode 0700, so the socket is not
/// reachable by other local users. If it is unset (an unusual login), we fall
/// back to a per-uid path under `/tmp` and create it 0700 ourselves.
///
/// Renamed from `omnibridge` without any migration (ADR-0020 D9): the socket
/// is volatile, and the daemon and every client ship together.
pub fn control_socket_path() -> PathBuf {
    control_socket_path_from(std::env::var_os("XDG_RUNTIME_DIR"), nix_uid())
}

fn control_socket_path_from(runtime_dir: Option<std::ffi::OsString>, uid: u32) -> PathBuf {
    let base = runtime_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/pliwee-{uid}")));
    base.join("pliwee").join("control.sock")
}

fn nix_uid() -> u32 {
    // Avoids a `libc`/`nix` dependency for one number. `/proc/self/status` is
    // always present on Linux, which is the only platform this adapter is
    // for — that is what makes reading it acceptable here and unacceptable in
    // the portable crates.
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|l| l.split_whitespace().next().map(str::to_string))
        })
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// The Unix-domain-socket control transport, from `pliwee-unix`.
///
/// It lived here until the macOS adapter needed the same code: nothing in it
/// is Linux-specific, and a second copy of the live-owner rule in `bind`
/// would be a second place for that rule to be wrong. Re-exported so every
/// existing `pliwee_linux::bind`, `pliwee_linux::connect` and
/// `pliwee_linux::UnixControlTransport` keeps meaning what it meant.
pub use pliwee_unix::{bind, connect, UnixControlListener, UnixControlTransport};

/// The control transport at this session's default location,
/// [`control_socket_path`].
///
/// A free function rather than the `UnixControlTransport::default_endpoint()`
/// it replaces, because the transport type now belongs to `pliwee-unix`,
/// which knows how to bind a socket and deliberately not where this platform
/// keeps one.
pub fn default_control_transport() -> UnixControlTransport {
    UnixControlTransport::new(control_socket_path())
}

/// What to tell a person whose client cannot reach the agent.
pub const START_HINT: &str = "Start it with: systemctl --user start pliweed.service";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_socket_path_follows_the_runtime_directory() {
        // `XDG_RUNTIME_DIR` is only read, never set: mutating the environment
        // would race every other test in this binary.
        let path = control_socket_path();
        assert!(path.ends_with("pliwee/control.sock"), "{path:?}");
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            assert!(path.starts_with(PathBuf::from(runtime)), "{path:?}");
        }
    }

    #[test]
    fn without_a_runtime_directory_the_socket_falls_back_to_a_per_uid_tmp_path() {
        assert_eq!(
            control_socket_path_from(None, 1000),
            PathBuf::from("/tmp/pliwee-1000/pliwee/control.sock")
        );
        assert_eq!(
            control_socket_path_from(Some("/run/user/1000".into()), 1000),
            PathBuf::from("/run/user/1000/pliwee/control.sock")
        );
    }

    #[test]
    fn the_store_adapter_reports_this_platform_and_a_software_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(dir.path()).expect("open");
        assert_eq!(
            store.identity().platform(),
            pliwee_proto::v1::Platform::Linux
        );
        assert_eq!(
            store.key_backing(),
            pliwee_core::identity::KeyBacking::Software
        );
    }
}
