//! `pliweed` — the user-session daemon.
//!
//! Runs unprivileged under `systemd --user`. It binds a high TCP port, a Unix
//! socket in `XDG_RUNTIME_DIR`, and an mDNS responder. It needs no root, no
//! capabilities, and no system-wide state.

use std::sync::Arc;

use clap::Parser;
use pliwee_capability_battery::{BatteryCapability, BatteryState, LocalBattery, UPowerReader};
use pliwee_capability_clipboard::{ClipboardCapability, ClipboardManager};
use pliwee_capability_files::{
    Destination, FilesCapability, FilesConfig, StreamRole, TransferApproval, TransferManager,
};
use pliwee_capability_notifications::backend::{
    dbus::DbusSink, logind::LogindLock, LockSource, NoSink, NotificationSink, UnknownLock,
};
use pliwee_capability_notifications::{NotificationManager, NotificationsCapability};
use pliwee_control::transport::ControlTransport;
use pliwee_core::capability::CapabilityRegistry;
use pliwee_daemon::{
    approval::FileApproval,
    listener, mdns, server,
    state::{DaemonState, LocalStateReport},
};
use tokio_rustls::TlsAcceptor;

#[derive(Parser, Debug)]
#[command(name = "pliweed", about = "Pliwee daemon", version)]
struct Args {
    /// Data directory (identity and trust store).
    #[arg(long)]
    data_dir: Option<std::path::PathBuf>,

    /// TCP port to listen on. Overrides the stored setting.
    #[arg(long)]
    port: Option<u16>,

    /// Do not advertise over mDNS. The daemon still accepts connections from
    /// peers that already know an address.
    #[arg(long)]
    no_mdns: bool,

    /// Log filter, e.g. `info`, `pliwee_core=debug`.
    #[arg(long, default_value = "info")]
    log: String,

    /// Directory for received files.
    ///
    /// Defaults to `<XDG downloads>/Pliwee`. Peers can never influence this:
    /// an offer carries a filename and no path at all.
    #[arg(long)]
    download_dir: Option<std::path::PathBuf>,

    /// Largest single file this machine will accept, in mebibytes.
    #[arg(long)]
    max_file_mib: Option<u64>,

    /// Accept incoming files without asking.
    ///
    /// Off by default and deliberately awkward to turn on: it removes the
    /// human from the loop, and the human is the last check on a paired but
    /// misbehaving device. Intended for unattended test rigs, not for daily
    /// use — a per-device "always allow" setting is the right answer for
    /// that, and does not exist yet.
    #[arg(long)]
    accept_files_without_asking: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log.clone().into()),
        )
        // No timestamps: journald adds its own, and duplicating them makes
        // `journalctl` output harder to read.
        .without_time()
        .init();

    // Installing the process-wide crypto provider explicitly, rather than
    // relying on a default, so that the choice of backend is visible in the
    // source and cannot be changed by a transitive dependency's feature flag.
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("a rustls crypto provider was already installed"))?;

    // ---- local state: carry an OmniBridge identity over first -------------
    // ADR-0020 D9/D12. Before the store is opened on `~/.local/share/pliwee`,
    // an identity still under `~/.local/share/omnibridge` is copied across —
    // or, if it exists and cannot be read, startup stops here and names it.
    // Opening the new directory without asking would be a "first run" over a
    // live identity: a new key, and every pairing silently gone.
    //
    // An explicit `--data-dir` is the operator's own choice of directory and
    // is used exactly as given.
    let (data_dir, migrated_from) = match args.data_dir {
        Some(dir) => (dir, None),
        None => {
            let dirs = pliwee_linux::DataDirs::from_env();
            let origin = pliwee_linux::migrate_data_dir(&dirs).map_err(|e| {
                tracing::error!(path = %e.path.display(), "refusing to start: {e}");
                anyhow::anyhow!("{e}")
            })?;
            let report = match &origin {
                pliwee_linux::DataDirOrigin::Migrated(r) => {
                    tracing::info!(
                        source = %r.source.display(),
                        destination = %dirs.canonical.display(),
                        "migrated from {}: identity and trust store copied into {}; \
                         the source directory was not modified",
                        r.source.display(),
                        dirs.canonical.display()
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
            (dirs.canonical, report)
        }
    };
    // ---- systemd: an account still enabled under the OmniBridge name ------
    // Read-only. The package ships omnibridged.service as an alias of
    // pliweed.service, so such an account starts this daemon at login; what it
    // cannot do is report itself enabled under the new name. Say so, once,
    // with the one command that fixes it (ADR-0020; rebrand Wave 7, B5).
    pliwee_linux::systemd_transition::log(&pliwee_linux::systemd_transition::classify(
        &pliwee_linux::systemd_transition::wants_dir_from_env(),
    ));

    // The Linux adapter composes the store: XDG paths, 0600/0700 modes,
    // `Platform::Linux`, `/etc/hostname`. `pliwee-core` decides the policy,
    // this decides where and how.
    let store = pliwee_linux::open_store(&data_dir)?;

    // A `--port` override applies to this run only. Silently rewriting the
    // user's stored configuration from a command-line flag is a surprise
    // nobody wants.
    if let Some(port) = args.port {
        tracing::info!(port, "using command-line port override");
    }
    let port = args.port.unwrap_or(store.settings().listen_port);

    let identity_fp = store.identity().fingerprint();
    let device_id = store.identity().device_id().to_string();
    let device_name = store.settings().device_name.clone();

    tracing::info!(
        device = %device_id,
        name = %device_name,
        fingerprint = %identity_fp.to_display_short(),
        key_backing = %store.key_backing(),
        "local identity"
    );

    // ---- capabilities -----------------------------------------------------
    let battery_state = Arc::new(BatteryState::default());
    let mut battery = BatteryCapability::new(Arc::clone(&battery_state));
    // `battery.v1` is registered either way. Registration is what lets this
    // machine *receive* the phone's battery, and a machine with no battery of
    // its own still wants that. What absence removes is the local *source* —
    // so we send nothing rather than sending a fabricated 0%.
    match UPowerReader::detect().await {
        LocalBattery::Present(upower) => {
            tracing::info!("UPower available; this machine will report its own battery");
            battery = battery.with_local_source(Arc::new(upower));
        }
        LocalBattery::Absent => {
            tracing::info!(
                "UPower available; no system battery present; battery.v1 is receive-only"
            );
        }
        LocalBattery::Unavailable => {
            tracing::info!("no local battery source; battery.v1 is receive-only");
        }
    }

    // files.v1. Note what is NOT here: an entry in `auto_grant`. Writing a
    // file to someone's disk is a side effect, so the grant is explicit
    // (`pliwee grant <device> files.v1`) per ADR-0008.
    let destination = match args.download_dir {
        Some(dir) => Destination::new(dir),
        None => Destination::default_location(),
    };
    let files_config = FilesConfig {
        destination: destination.clone(),
        max_file_bytes: args
            .max_file_mib
            .map(|mib| mib.saturating_mul(1024 * 1024))
            .unwrap_or(pliwee_capability_files::limits::DEFAULT_MAX_FILE_BYTES),
        ..FilesConfig::default()
    };
    if let Err(e) = destination.prepare() {
        if e.kind() == std::io::ErrorKind::ReadOnlyFilesystem {
            // The packaged user unit grants ~/Downloads only; any other
            // download directory needs its own grant (pliweed.service).
            tracing::warn!(
                error = %e,
                dir = %destination.dir().display(),
                "could not prepare the download directory: the service's sandbox does not allow \
                 writing there, so received files cannot be stored. To allow it, run \
                 `systemctl --user edit pliweed.service`, add `[Service]` and \
                 `ReadWritePaths=-{}`, then `systemctl --user restart pliweed.service`",
                destination.dir().parent().unwrap_or(destination.dir()).display()
            );
        } else {
            tracing::warn!(error = %e, "could not prepare the download directory");
        }
    }
    tracing::info!(
        download_dir = %destination.dir().display(),
        max_file_bytes = files_config.max_file_bytes,
        "files.v1 ready"
    );
    let legacy_partial_files = find_legacy_partial_files(destination.dir());

    if args.accept_files_without_asking {
        tracing::warn!(
            "--accept-files-without-asking is set: incoming files will NOT be \
             confirmed by a human"
        );
    }
    // One object, two roles: `files.v1` asks it whether to accept an incoming
    // file, and the control server attaches the desktop UI to it as the thing
    // that answers. Building two would compile and would silently never ask
    // anybody, so the `Arc` is cloned rather than the constructor called
    // twice.
    let approval = Arc::new(FileApproval::new(args.accept_files_without_asking));

    let transfers = TransferManager::new(
        // The desktop is the stable listener, so it is always the end that
        // accepts data streams and issues their challenges.
        StreamRole::Acceptor,
        identity_fp,
        files_config,
        Arc::clone(&approval) as Arc<dyn TransferApproval>,
    );

    // clipboard.v1. Like files.v1 it is absent from `auto_grant`: a device
    // that can write your clipboard can also read what you paste next, and
    // ADR-0008 requires a side effect that large to be granted by hand.
    //
    // The backend is probed once here so that `pliwee clipboard status` can
    // report what this session can actually do — including, on GNOME, that it
    // cannot report clipboard changes at all — instead of each command
    // discovering it separately.
    let clipboard_backend = pliwee_capability_clipboard::backend::detect();
    if let Err(why) = clipboard_backend.watch_availability() {
        tracing::info!(
            reason = %why,
            "clipboard auto-send is unavailable on this session; sending by \
             hand reads the selection the same way and is usually unavailable \
             too. Receiving is unaffected — `pliwee clipboard status` has \
             the detail"
        );
    }
    let clipboard = ClipboardManager::new(clipboard_backend, device_id.clone());
    tracing::info!(backend = %clipboard.backend().describe(), "clipboard.v1 ready");

    // notifications.v1. Absent from `auto_grant` for a stronger version of
    // the same reason `clipboard.v1` is: a device that can put notifications
    // on this screen is a device whose messages a passer-by can read, and
    // ADR-0015 §4 requires that to be granted by hand.
    //
    // The two platform seams are probed once, here, so that
    // `pliwee notifications status` reports what this session can actually do
    // instead of each command discovering it separately — and so that the
    // first role announcement is a fact rather than a hope.
    //
    // Neither probe failing is fatal. A machine with no notification server is
    // a normal, reportable state: the capability is still registered and still
    // negotiated, and it simply announces no `SINK` role, which is precisely
    // what roles exist to express (ADR-0017). Registering it unconditionally
    // is deliberate — gating the handshake on a platform condition the user
    // can change at 14:32 would mean a reconnect were needed to pick it up.
    let notification_sink: Arc<dyn NotificationSink> = match DbusSink::connect().await {
        Some(sink) => Arc::new(sink),
        // Not a sink that accepts and discards: that would announce `SINK` and
        // then swallow every notification a phone sent, with the phone having
        // no way to know. `NoSink` reports unavailable, the role narrows, and
        // the peer is told the truth.
        None => Arc::new(NoSink),
    };
    // A session whose lock state cannot be determined is treated as locked, by
    // the type rather than by a check a caller could forget. It is the
    // fail-closed direction and it is the one a privacy control must take.
    let notification_lock: Arc<dyn LockSource> = match LogindLock::connect().await {
        Some(lock) => Arc::new(lock),
        None => {
            tracing::warn!(
                "no logind session to read LockedHint from; this desktop will                  be treated as locked, so notifications will be reduced"
            );
            Arc::new(UnknownLock)
        }
    };
    let notifications = NotificationManager::new(
        Arc::clone(&notification_sink),
        Arc::clone(&notification_lock),
    )
    .await;
    tracing::info!(
        sink = %notification_sink.describe(),
        lock = %notification_lock.describe(),
        available = notifications.is_available(),
        "notifications.v1 ready"
    );

    let registry = CapabilityRegistry::builder()
        .register(Arc::new(battery))
        .register(Arc::new(FilesCapability::new(Arc::clone(&transfers))))
        .register(Arc::new(ClipboardCapability::new(Arc::clone(&clipboard))))
        .register(Arc::new(NotificationsCapability::new(Arc::clone(
            &notifications,
        ))))
        .build();
    tracing::info!(capabilities = ?registry.advertised(), "capabilities registered");

    // ---- TLS --------------------------------------------------------------
    let tls_config = pliwee_core::tls::server_config(store.identity())?;
    let acceptor = TlsAcceptor::from(tls_config);

    let state = Arc::new(
        DaemonState::new(store, registry, battery_state)
            .with_transfers(Arc::clone(&transfers))
            .with_clipboard(Arc::clone(&clipboard))
            .with_notifications(Arc::clone(&notifications))
            .with_file_approval(Arc::clone(&approval)),
    );
    state.set_local_state(LocalStateReport {
        migrated_from,
        legacy_partial_files,
    });

    // The state is the authorizer: every grant question is answered from the
    // trust store, freshly, rather than from a set captured at handshake time.
    transfers
        .set_authorizer(Arc::clone(&state) as Arc<dyn pliwee_capability_files::FilesAuthorizer>)
        .await;
    let _reaper = transfers.spawn_reaper();

    // Same rule for the clipboard: the grant and the per-peer policy are
    // answered from the trust store on every question, never from a set
    // captured at handshake time.
    clipboard
        .set_authorizer(
            Arc::clone(&state) as Arc<dyn pliwee_capability_clipboard::ClipboardAuthorizer>
        )
        .await;
    // Supervised, and idle until some peer actually asks for auto-send: with
    // nobody asking it holds no helper process and no X connection.
    let _clipboard_watcher = clipboard.spawn_watcher();

    // Same rule again for notifications: the grant and the per-peer policy are
    // answered from the trust store on every message, never from a set
    // captured at handshake time. This one carries more weight than the other
    // two — the transport's own grant filter runs when the session is built,
    // so after that point this is the only thing between a revoked device and
    // the screen.
    notifications
        .set_authorizer(
            Arc::clone(&state) as Arc<dyn pliwee_capability_notifications::NotificationAuthorizer>
        )
        .await;
    // The three platform signals: the desktop closing a notification, the
    // notification server appearing or going away, and the session locking.
    // All event-driven; none polled.
    let _notification_pumps = notifications.spawn_platform_pumps();

    // ---- listeners --------------------------------------------------------
    let bound = listener::bind_endpoints(port)?;
    let bound_port = bound.port;
    tracing::info!(
        port = bound_port,
        families = %bound.families,
        sockets = bound.listeners.len(),
        "listening"
    );

    // The control endpoint comes from the adapter, through the
    // `ControlTransport` seam. A failure to bind because another agent
    // already owns the endpoint is fatal and is *not* worked around by
    // choosing a different name — see `pliwee_control::transport`.
    let transport = pliwee_linux::UnixControlTransport::default_endpoint();
    let control_listener = match ControlTransport::bind(&transport) {
        Ok(l) => l,
        Err(e @ pliwee_control::transport::BindError::AlreadyOwned { .. }) => {
            anyhow::bail!("{e}");
        }
        Err(e) => return Err(e.into()),
    };
    tracing::info!(endpoint = %transport.endpoint(), "control endpoint ready");

    // Held for the process lifetime; dropping it withdraws the mDNS record.
    let _advertisement = if args.no_mdns {
        tracing::info!("mDNS advertisement disabled");
        None
    } else {
        match mdns::Advertisement::publish(&device_id, &device_name, bound_port, bound.families) {
            Ok(a) => Some(a),
            Err(e) => {
                // Not fatal: pairing by QR carries explicit addresses, so the
                // daemon is still usable without a working mDNS responder.
                tracing::warn!(error = %e, "mDNS advertisement failed; discovery unavailable");
                None
            }
        }
    };

    state.set_listen_port(bound_port);
    state.set_listen_families(bound.families);

    // ---- the desktop shell ------------------------------------------------
    //
    // A `StatusNotifierItem` on the session bus, which is how KDE Plasma shows
    // an application in its system tray. The daemon owns it because the daemon
    // is the process that is always here: the GUI is two windows a person
    // opens and closes, and keeping one alive forever to hold an icon would
    // have made Pliwee a product with two resident processes.
    //
    // Held, never awaited, and deliberately **not** in the `select!` below.
    // Everything in that race is load-bearing — the network listener, the
    // control server, the interrupt — and the first of them to finish ends the
    // process. A tray icon is not in that class: if the session has no tray
    // host, or the shell restarts, or the item cannot be published at all, the
    // right outcome is a log line and a daemon that goes on moving files.
    // `tray::spawn` supervises its own task so that even a panic is written
    // down rather than swallowed.
    //
    // On a session with no `org.kde.StatusNotifierWatcher` — every GNOME
    // session, which is most of them — this publishes the item, finds no host,
    // says so once, and then waits event-driven for one to appear. It never
    // polls.
    let _tray = pliwee_linux::tray::spawn(pliwee_linux::tray::ActivatorChoice::SessionBus);

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

    let net = tokio::spawn(listener::run(bound.listeners, acceptor, Arc::clone(&state)));
    let ctl = tokio::spawn(server::run(control_listener, Arc::clone(&state)));

    tokio::select! {
        r = net => r??,
        r = ctl => r??,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down");
        }
    }

    // Through the adapter, not `remove_file`: unbinding is what the concept
    // is, and a named pipe has no file to unlink.
    transport.release();
    Ok(())
}

fn migration_report(
    record: &pliwee_linux::MigrationRecord,
    this_run: bool,
) -> pliwee_daemon::control::MigrationReport {
    pliwee_daemon::control::MigrationReport {
        source: record.source.display().to_string(),
        migrated_at_unix: record.migrated_at_unix,
        this_run,
    }
}

/// Interrupted OmniBridge transfers left in the download directory in use and
/// in the one OmniBridge used. Reported by `status`; never removed.
fn find_legacy_partial_files(current: &std::path::Path) -> Vec<String> {
    use pliwee_capability_files::destination::{
        default_download_dir, legacy_partial_files, LEGACY_DOWNLOAD_SUBDIR,
    };
    let legacy = default_download_dir().join(LEGACY_DOWNLOAD_SUBDIR);
    let mut dirs = vec![current.to_path_buf()];
    if legacy != current {
        dirs.push(legacy);
    }
    let mut found = Vec::new();
    for dir in dirs {
        match legacy_partial_files(&dir) {
            Ok(files) => found.extend(files.into_iter().map(|p| p.display().to_string())),
            Err(e) => tracing::warn!(
                dir = %dir.display(),
                error = %e,
                "could not look for interrupted OmniBridge transfers"
            ),
        }
    }
    if !found.is_empty() {
        tracing::info!(
            count = found.len(),
            "interrupted OmniBridge transfers (.omnibridge-*.part) found; they \
             are left in place — see `status`"
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::CommandFactory;
    use pliwee_capability_files::destination::{DOWNLOAD_SUBDIR, LEGACY_DOWNLOAD_SUBDIR};

    /// `pliweed --help` states the download default the daemon really uses.
    /// The text is prose, so it is checked against the constant rather than
    /// trusted to follow it: W4 moved the folder and the help did not move.
    #[test]
    fn help_states_the_real_download_folder() {
        let mut cmd = Args::command();
        assert_eq!(cmd.get_name(), "pliweed");
        let help = cmd.render_long_help().to_string();
        let want = format!("<XDG downloads>/{DOWNLOAD_SUBDIR}");
        assert!(help.contains(&want), "missing {want:?} in:\n{help}");
        assert!(
            !help.contains(LEGACY_DOWNLOAD_SUBDIR),
            "the help still names the legacy folder:\n{help}"
        );
        assert_eq!(
            cmd.render_version().trim_end(),
            format!("pliweed {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
