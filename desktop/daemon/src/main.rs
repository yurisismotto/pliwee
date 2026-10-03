//! `pliweed` — the user-session daemon.
//!
//! Runs unprivileged, as the user: under `systemd --user` on Linux, as a
//! per-user `launchd` agent on macOS. It binds a high TCP port, a Unix socket
//! in a directory only its user can reach, and an mDNS responder. It needs no
//! root, no capabilities, and no system-wide state.
//!
//! Nothing in this file names a platform. Every question with a different
//! answer on Linux and on macOS is asked of [`platform`], which is where the
//! answers are.

use std::sync::Arc;

use clap::Parser;
use pliwee_capability_battery::{BatteryCapability, BatteryState};
use pliwee_capability_clipboard::{ClipboardCapability, ClipboardManager};
use pliwee_capability_files::{
    Destination, FilesCapability, FilesConfig, StreamRole, TransferApproval, TransferManager,
};
use pliwee_capability_notifications::{NotificationManager, NotificationsCapability};
use pliwee_control::transport::ControlTransport;
use pliwee_core::capability::CapabilityRegistry;
use pliwee_daemon::{
    approval::FileApproval,
    listener, mdns, platform, server,
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
    /// Defaults to `<XDG downloads>/Pliwee` (`~/Downloads/Pliwee` on macOS). Peers can never influence this:
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

    // stderr for the journal on Linux; on macOS the agent's own log file
    // when `launchd` started it, stderr otherwise.
    platform::init_logging(&args.log);
    tracing::info!(
        platform = platform::NAME,
        version = env!("CARGO_PKG_VERSION"),
        "pliweed starting"
    );

    // Installing the process-wide crypto provider explicitly, rather than
    // relying on a default, so that the choice of backend is visible in the
    // source and cannot be changed by a transitive dependency's feature flag.
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("a rustls crypto provider was already installed"))?;

    // ---- local state ------------------------------------------------------
    // On Linux this is where an OmniBridge identity is carried over before
    // the store is opened (ADR-0020 D9/D12), and where startup stops if one
    // exists and cannot be read. On macOS there is nothing to carry over. An
    // explicit `--data-dir` is the operator's own choice of directory and is
    // used exactly as given on both.
    let (data_dir, migrated_from) = platform::resolve_data_dir(args.data_dir)?;
    platform::startup_notes();

    // The adapter composes the store. On Linux: XDG paths, 0600/0700 modes,
    // `Platform::Linux`, `/etc/hostname`. On macOS: `~/Library/Application
    // Support`, the private key in the login keychain, the computer name.
    // `pliwee-core` decides the policy, the adapter decides where and how.
    let store = platform::open_store(&data_dir)?;

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
    if let Some(source) = platform::local_battery().await {
        battery = battery.with_local_source(source);
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
        platform::explain_download_dir_error(&e, destination.dir());
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
    let clipboard_backend = platform::clipboard_backend();
    if let Err(why) = clipboard_backend.watch_availability() {
        // What that means for sending by hand differs by platform, so the
        // platform says it.
        platform::explain_clipboard_watch(&why);
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
    //
    // When there is no notification server the adapter returns `NoSink`, not a
    // sink that accepts and discards — see `platform::notification_sink`.
    let notification_sink = platform::notification_sink().await;
    // A session whose lock state cannot be determined is treated as locked, by
    // the type rather than by a check a caller could forget. It is the
    // fail-closed direction and it is the one a privacy control must take.
    let notification_lock = platform::lock_source().await;
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
    let transport = platform::control_transport()?;
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
    // On Linux: the KDE/GNOME tray item and the D-Bus activation self-heal,
    // both owned by the agent because it is the process that is always here.
    // On macOS: nothing — the menu-bar item is `Pliwee.app`'s. Held, never
    // awaited, and deliberately not in the `select!` below: a tray icon is
    // not load-bearing, and losing one must not end the process.
    let _desktop_shell = platform::spawn_desktop_shell();

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
