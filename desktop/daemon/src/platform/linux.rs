//! The Linux row: `systemd --user`, XDG, D-Bus.
//!
//! Every line of this module was in `main.rs` before the macOS adapter
//! existed, and moved here unchanged in behaviour. What it does on Fedora,
//! Ubuntu and Debian is exactly what `pliweed` did before.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pliwee_capability_battery::{LocalBattery, LocalBatterySource, UPowerReader};
use pliwee_capability_clipboard::backend::ClipboardBackend;
use pliwee_capability_notifications::backend::{
    dbus::DbusSink, logind::LogindLock, LockSource, NoSink, NotificationSink, UnknownLock,
};
use pliwee_control::MigrationReport;

pub use pliwee_linux::{
    bind, control_socket_path, open_store, UnixControlListener, UnixControlTransport,
};

/// The platform's name, for the startup log line.
pub const NAME: &str = "linux";

/// Logging to stderr, where `systemd` hands it to the journal.
pub fn init_logging(filter: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.to_string().into()),
        )
        // No timestamps: journald adds its own, and duplicating them makes
        // `journalctl` output harder to read.
        .without_time()
        .init();
}

/// The data directory, with an OmniBridge identity carried over first.
///
/// ADR-0020 D9/D12. Before the store is opened on `~/.local/share/pliwee`,
/// an identity still under `~/.local/share/omnibridge` is copied across —
/// or, if it exists and cannot be read, startup stops here and names it.
/// Opening the new directory without asking would be a "first run" over a
/// live identity: a new key, and every pairing silently gone.
///
/// An explicit `--data-dir` is the operator's own choice of directory and
/// is used exactly as given.
pub fn resolve_data_dir(
    explicit: Option<PathBuf>,
) -> anyhow::Result<(PathBuf, Option<MigrationReport>)> {
    let dir = match explicit {
        Some(dir) => return Ok((dir, None)),
        None => pliwee_linux::DataDirs::from_env(),
    };
    let origin = pliwee_linux::migrate_data_dir(&dir).map_err(|e| {
        tracing::error!(path = %e.path.display(), "refusing to start: {e}");
        anyhow::anyhow!("{e}")
    })?;
    let report = match &origin {
        pliwee_linux::DataDirOrigin::Migrated(r) => {
            tracing::info!(
                source = %r.source.display(),
                destination = %dir.canonical.display(),
                "migrated from {}: identity and trust store copied into {}; \
                 the source directory was not modified",
                r.source.display(),
                dir.canonical.display()
            );
            Some(migration_report(r, true))
        }
        pliwee_linux::DataDirOrigin::Existing {
            migrated_from: Some(r),
        } => {
            tracing::info!(
                source = %r.source.display(),
                migrated_at_unix = r.migrated_at_unix,
                "local state was migrated from {} by an earlier start; \
                 nothing to migrate",
                r.source.display()
            );
            Some(migration_report(r, false))
        }
        pliwee_linux::DataDirOrigin::Existing {
            migrated_from: None,
        }
        | pliwee_linux::DataDirOrigin::NoLegacyState => None,
    };
    Ok((dir.canonical, report))
}

fn migration_report(record: &pliwee_linux::MigrationRecord, this_run: bool) -> MigrationReport {
    MigrationReport {
        source: record.source.display().to_string(),
        migrated_at_unix: record.migrated_at_unix,
        this_run,
    }
}

/// Things worth saying once at startup that are about this platform's
/// service manager rather than about Pliwee.
pub fn startup_notes() {
    // ---- systemd: an account still enabled under the OmniBridge name ------
    // Read-only. The package ships omnibridged.service as an alias of
    // pliweed.service, so such an account starts this daemon at login; what it
    // cannot do is report itself enabled under the new name. Say so, once,
    // with the one command that fixes it (ADR-0020; rebrand Wave 7, B5).
    pliwee_linux::systemd_transition::log(&pliwee_linux::systemd_transition::classify(
        &pliwee_linux::systemd_transition::wants_dir_from_env(),
    ));
}

/// This machine's own battery, from UPower, or `None` when there is nothing
/// to report.
pub async fn local_battery() -> Option<Arc<dyn LocalBatterySource>> {
    match UPowerReader::detect().await {
        LocalBattery::Present(upower) => {
            tracing::info!("UPower available; this machine will report its own battery");
            Some(Arc::new(upower))
        }
        LocalBattery::Absent => {
            tracing::info!(
                "UPower available; no system battery present; battery.v1 is receive-only"
            );
            None
        }
        LocalBattery::Unavailable => {
            tracing::info!("no local battery source; battery.v1 is receive-only");
            None
        }
    }
}

/// The best clipboard backend for this session.
pub fn clipboard_backend() -> Arc<dyn ClipboardBackend> {
    pliwee_capability_clipboard::backend::detect()
}

/// The freedesktop notification server, or [`NoSink`].
pub async fn notification_sink() -> Arc<dyn NotificationSink> {
    match DbusSink::connect().await {
        Some(sink) => Arc::new(sink),
        // Not a sink that accepts and discards: that would announce `SINK` and
        // then swallow every notification a phone sent, with the phone having
        // no way to know. `NoSink` reports unavailable, the role narrows, and
        // the peer is told the truth.
        None => Arc::new(NoSink),
    }
}

/// logind's `LockedHint`, or [`UnknownLock`] — treated as locked.
pub async fn lock_source() -> Arc<dyn LockSource> {
    match LogindLock::connect().await {
        Some(lock) => Arc::new(lock),
        None => {
            tracing::warn!(
                "no logind session to read LockedHint from; this desktop will                  be treated as locked, so notifications will be reduced"
            );
            Arc::new(UnknownLock)
        }
    }
}

/// Says why the download directory could not be prepared, with the fix that
/// applies on this platform.
pub fn explain_download_dir_error(e: &std::io::Error, dir: &Path) {
    if e.kind() == std::io::ErrorKind::ReadOnlyFilesystem {
        // The packaged user unit grants ~/Downloads only; any other
        // download directory needs its own grant (pliweed.service).
        tracing::warn!(
            error = %e,
            dir = %dir.display(),
            "could not prepare the download directory: the service's sandbox does not allow \
             writing there, so received files cannot be stored. To allow it, run \
             `systemctl --user edit pliweed.service`, add `[Service]` and \
             `ReadWritePaths=-{}`, then `systemctl --user restart pliweed.service`",
            dir.parent().unwrap_or(dir).display()
        );
    } else {
        tracing::warn!(error = %e, "could not prepare the download directory");
    }
}

/// The control endpoint at this session's default location.
pub fn control_transport() -> anyhow::Result<UnixControlTransport> {
    Ok(pliwee_linux::default_control_transport())
}

/// What the agent holds for the desktop shell. Dropping it withdraws the tray
/// item.
pub struct DesktopShell {
    _tray: pliwee_linux::tray::TrayHandle,
}

/// The tray item and the D-Bus activation self-heal.
pub fn spawn_desktop_shell() -> DesktopShell {
    // A `StatusNotifierItem` on the session bus, which is how KDE Plasma shows
    // an application in its system tray. The daemon owns it because the daemon
    // is the process that is always here: the GUI is two windows a person
    // opens and closes, and keeping one alive forever to hold an icon would
    // have made Pliwee a product with two resident processes.
    //
    // Held, never awaited, and deliberately **not** in the `select!` in
    // `main`. Everything in that race is load-bearing — the network listener,
    // the control server, the interrupt — and the first of them to finish
    // ends the process. A tray icon is not in that class: if the session has
    // no tray host, or the shell restarts, or the item cannot be published at
    // all, the right outcome is a log line and a daemon that goes on moving
    // files. `tray::spawn` supervises its own task so that even a panic is
    // written down rather than swallowed.
    //
    // On a session with no `org.kde.StatusNotifierWatcher` — every GNOME
    // session, which is most of them — this publishes the item, finds no host,
    // says so once, and then waits event-driven for one to appear. It never
    // polls.
    let tray = pliwee_linux::tray::spawn(pliwee_linux::tray::ActivatorChoice::SessionBus);

    // ---- D-Bus activation self-heal ---------------------------------------
    //
    // A package installs the GUI's D-Bus service file as root, and the user's
    // *already running* session bus does not read it until something says so.
    // Until then the tray item above activates nothing: clicking Pliwee on
    // a correctly installed machine returns ServiceUnknown. Root cannot fix
    // that — it has no route to a user's session bus — but this process runs
    // as the user, in the session, and can. Audit §8.2.
    //
    // At most one ReloadConfig, on the session bus only, and the outcome is a
    // log line whatever it is. Spawned rather than awaited because a bus that
    // is slow to answer is not a reason for the listener below to start late,
    // and because there is nothing downstream that depends on the answer.
    tokio::spawn(async {
        let outcome = pliwee_linux::activation::self_heal_desktop_activation().await;
        pliwee_linux::activation::log(&outcome);
    });

    DesktopShell { _tray: tray }
}
