//! The macOS row: a per-user `launchd` agent, `~/Library`, the login
//! keychain, IOKit and `NSPasteboard`.
//!
//! The same composition as the Linux row, with each platform question
//! answered by `pliwee-macos`. Nothing here decides a protocol, a policy or a
//! security property; those are all above this module, shared.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pliwee_capability_battery::LocalBatterySource;
use pliwee_capability_clipboard::backend::ClipboardBackend;
use pliwee_capability_notifications::backend::{LockSource, NoSink, NotificationSink, UnknownLock};
use pliwee_control::MigrationReport;
use pliwee_macos::battery::MacBattery;
use pliwee_macos::RuntimePaths;

pub use pliwee_macos::{
    bind, control_socket_path, open_store, UnixControlListener, UnixControlTransport,
};

/// The platform's name, for the startup log line.
pub const NAME: &str = "macos";

/// A log file larger than this is moved aside at startup, keeping one
/// previous generation. There is no journal on macOS to rotate it for us.
const LOG_ROTATE_BYTES: u64 = 8 * 1024 * 1024;

/// Logging to `~/Library/Logs/Pliwee/pliweed.log` when running as the
/// `launchd` agent, and to stderr otherwise.
///
/// `launchd` discards an agent's stderr unless its plist names a file, and a
/// plist cannot name one under the user's home: it has no `~` and no
/// variables. So the agent recognises its own job — `launchd` sets
/// `XPC_SERVICE_NAME` to the job's label — and opens the log itself, in the
/// directory Console.app shows under "Log Reports". Run from a terminal, it
/// logs to the terminal, with timestamps because nothing else adds them.
pub fn init_logging(filter: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| filter.to_string().into());

    let under_launchd =
        std::env::var("XPC_SERVICE_NAME").as_deref() == Ok(pliwee_macos::LAUNCHD_LABEL);
    if under_launchd {
        match open_log_file(&RuntimePaths::for_current_user().log_file()) {
            Ok(file) => {
                tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_ansi(false)
                    .with_writer(Arc::new(file))
                    .init();
                return;
            }
            Err(e) => {
                // Not fatal, and said where `launchd` *might* keep it: an
                // agent that cannot log is still an agent that moves files.
                eprintln!("pliweed: could not open the log file: {e}; logging to stderr");
            }
        }
    }
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn open_log_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    // 0600: the log carries device names and fingerprints, which are not
    // secret but are nobody else's business on a shared Mac.
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

/// The data directory: `--data-dir` as given, otherwise
/// `~/Library/Application Support/Pliwee`.
///
/// There is no migration step. OmniBridge never ran on macOS, so there is no
/// legacy identity to carry over and nothing to report.
pub fn resolve_data_dir(
    explicit: Option<PathBuf>,
) -> anyhow::Result<(PathBuf, Option<MigrationReport>)> {
    Ok((
        explicit.unwrap_or_else(|| RuntimePaths::for_current_user().data_dir()),
        None,
    ))
}

/// Nothing to say: `launchd` has no equivalent of the OmniBridge-era unit
/// name that the Linux row reports.
pub fn startup_notes() {}

/// This Mac's own battery, from IOKit, or `None` when there is nothing to
/// report.
pub async fn local_battery() -> Option<Arc<dyn LocalBatterySource>> {
    match tokio::task::spawn_blocking(MacBattery::detect).await {
        Ok(MacBattery::Present(battery)) => {
            tracing::info!("IOKit reports an internal battery; this Mac will report it");
            Some(battery.into_source())
        }
        Ok(MacBattery::Absent) => {
            tracing::info!("this Mac has no internal battery; battery.v1 is receive-only");
            None
        }
        Ok(MacBattery::Unavailable) | Err(_) => {
            tracing::info!("no local battery source; battery.v1 is receive-only");
            None
        }
    }
}

/// The general pasteboard.
pub fn clipboard_backend() -> Arc<dyn ClipboardBackend> {
    Arc::new(pliwee_macos::clipboard::PasteboardBackend::new())
}

/// No notification sink on macOS yet — see `pliwee-macos`'s crate docs.
///
/// [`NoSink`] rather than a sink that accepts and discards: this device then
/// announces no `SINK` role, and a phone does not send it notifications it
/// would never show.
pub async fn notification_sink() -> Arc<dyn NotificationSink> {
    tracing::info!(
        "notifications.v1: no notification sink on macOS yet; this Mac will not \
         announce the SINK role"
    );
    Arc::new(NoSink)
}

/// No lock source on macOS yet. [`UnknownLock`] is treated as locked, the
/// fail-closed direction; with no sink it changes nothing a person can see.
pub async fn lock_source() -> Arc<dyn LockSource> {
    Arc::new(UnknownLock)
}

/// Says why the download directory could not be prepared.
///
/// On macOS the likely cause is privacy protection: `~/Downloads` is guarded
/// by TCC, and the first write by an application raises a consent prompt.
/// Declining it leaves the folder unwritable to the agent until the person
/// allows it in System Settings.
pub fn explain_download_dir_error(e: &std::io::Error, dir: &Path) {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        tracing::warn!(
            error = %e,
            dir = %dir.display(),
            "could not prepare the download directory: macOS did not allow Pliwee to \
             write there, so received files cannot be stored. Allow it in System \
             Settings → Privacy & Security → Files and Folders, then restart Pliwee"
        );
    } else {
        tracing::warn!(error = %e, "could not prepare the download directory");
    }
}

/// The control endpoint at this user's default location, or the reason it
/// cannot be used.
pub fn control_transport() -> anyhow::Result<UnixControlTransport> {
    Ok(pliwee_macos::default_control_transport()?)
}

/// Nothing: the menu-bar item belongs to `Pliwee.app`, which is a client of
/// the control socket. The agent has no window-server presence of its own.
pub struct DesktopShell;

pub fn spawn_desktop_shell() -> DesktopShell {
    DesktopShell
}
