//! `pliwee` — control the local daemon.
//!
//! Talks to the daemon over its Unix control socket. It holds no keys, no
//! trust store and no protocol logic: if the daemon is not running, every
//! command fails cleanly rather than doing something partial.

use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use pliwee_control::{
    BatteryReport, ClipboardFlag, ClipboardStatusReport, DeviceReport, Event, NotificationSetting,
    NotificationsStatusReport, Request, Response, TransferReport,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

// The platform adapter: where the agent's socket is, and what to say when it
// is not there. Chosen by target here and in `Cargo.toml`, nowhere else.
#[cfg(not(target_os = "macos"))]
use pliwee_linux as platform;
#[cfg(target_os = "macos")]
use pliwee_macos as platform;

use platform::control_socket_path;

#[derive(Parser, Debug)]
#[command(name = "pliwee", about = "Pliwee control", version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Daemon status, identity and live connections.
    Status,
    /// List paired devices.
    Devices,
    /// Open a pairing window and show a QR code.
    Pair {
        /// Seconds the window stays open.
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Revoke a pairing, by device id or fingerprint prefix.
    Unpair { device: String },
    /// Round-trip a PING over the live session with a device.
    Ping { device: String },

    /// Take a revoked device out of the device list, keeping the revocation.
    ///
    /// The device stays revoked: it is still refused if it connects, and it
    /// still has to pair again from scratch to come back. This only stops it
    /// being listed.
    ///
    /// Addressed by **full fingerprint hex**, not by a device id or a prefix.
    /// A revoked record about to be removed has had its name and id cleared,
    /// and identity here has to be the pinned key rather than anything two
    /// devices could share.
    RemoveFromList {
        /// Full device fingerprint hex. Omit when using --all.
        fingerprint: Option<String>,
        /// Remove every revoked device that is still listed.
        #[arg(long)]
        all: bool,
    },

    /// Allow a device to use a capability.
    ///
    /// Pairing says who a device is; this says what it may do. `files.v1` is
    /// never granted automatically, because writing files to your disk is a
    /// side effect and ADR-0008 requires those to be explicit.
    Grant {
        /// Device id, or a fingerprint prefix of at least 8 characters.
        device: String,
        /// Capability id, e.g. `files.v1`.
        capability: String,
    },

    /// Withdraw a capability from a device. Takes effect immediately,
    /// including on a transfer that is already running.
    Revoke { device: String, capability: String },

    /// Send a file to a paired device.
    ///
    /// The device may be named by its device id or by an unambiguous
    /// fingerprint prefix. An ambiguous name is an error: sending a file to
    /// the wrong device because a prefix matched two of them is not a
    /// failure mode worth having.
    Send {
        device: String,
        /// Path to a regular file on this machine. Only its basename is sent.
        file: std::path::PathBuf,
    },

    /// List transfers this daemon has seen since it started.
    Transfers,

    /// Cancel a running transfer, by id or by an unambiguous id prefix.
    Cancel { transfer: String },

    /// Share text clipboards with a paired device.
    #[command(subcommand)]
    Clipboard(ClipboardCommand),

    /// Show a paired device's notifications on this desktop.
    #[command(subcommand)]
    Notifications(NotificationsCommand),
}

#[derive(Subcommand, Debug)]
enum NotificationsCommand {
    /// What notification mirroring can do here, and the policy for each
    /// device.
    ///
    /// Shows the notification server this session actually has, where the
    /// lock state is read from, and how many notifications are currently
    /// mirrored. It never lists the notifications themselves: there is no
    /// notification history anywhere in Pliwee, and this is not one.
    Status,

    /// Show or stop showing a device's notifications here.
    Mirror {
        device: String,
        #[arg(value_enum)]
        state: Toggle,
    },

    /// What to show while this desktop is locked.
    ///
    /// Defaults to `app-only`: the application's name, with no title and no
    /// body. A locked screen is the case the setting exists for — somebody
    /// walking past a desk should not be able to read a phone's messages off
    /// it — so `full` is a deliberate choice, not a convenience.
    WhenLocked {
        device: String,
        #[arg(value_enum)]
        policy: WhenLocked,
    },

    /// Let closing a notification here dismiss it on the device too.
    ///
    /// Stored, and **inert in this release**: nothing sends a dismissal yet.
    /// The setting exists so that the default a later release must respect is
    /// already written down, and it defaults off.
    DismissSync {
        device: String,
        #[arg(value_enum)]
        state: Toggle,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum WhenLocked {
    /// Everything, as if unlocked.
    Full,
    /// The application's name only.
    AppOnly,
    /// Nothing at all, and existing notifications are closed.
    Suppress,
}

impl WhenLocked {
    fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::AppOnly => "app-only",
            Self::Suppress => "suppress",
        }
    }
}

#[derive(Subcommand, Debug)]
enum ClipboardCommand {
    /// What clipboard sharing can do here, and the policy for each device.
    ///
    /// Shows whether this session can detect local clipboard changes at all,
    /// which is what `auto-send` needs — on GNOME it cannot, and knowing that
    /// before turning the flag on is the point.
    Status,

    /// Send the current clipboard to a device.
    ///
    /// Explicit and manual: it does not need `auto-send`, only the
    /// `clipboard.v1` grant and `send` policy. The device may be named by its
    /// device id or by an unambiguous fingerprint prefix — an ambiguous name
    /// is an error, because sending a password to the wrong device because a
    /// prefix matched two of them is not a failure mode worth having.
    Send {
        device: String,
        /// Ask the receiving device to treat this clip as sensitive.
        ///
        /// On Android it sets `ClipDescription.EXTRA_IS_SENSITIVE`, so the
        /// system hides the preview and clipboard managers skip it. It is a
        /// presentation hint, not encryption and not an access control.
        #[arg(long)]
        sensitive: bool,
    },

    /// Apply a clip that arrived while `auto-receive` was off.
    Apply { device: String },

    /// Allow or stop sending this machine's clipboard to a device.
    Allow {
        device: String,
        #[arg(value_enum)]
        direction: Direction,
        #[arg(value_enum)]
        state: Toggle,
    },

    /// Automatically push local clipboard changes to a device.
    ///
    /// Off by default, and worth reading twice before turning on: it means
    /// *everything you copy* — passwords, tokens, recovery codes — goes to
    /// that device as you copy it. Unlike Android, a Linux desktop offers no
    /// reliable "this clip is sensitive" signal to filter on.
    AutoSend {
        device: String,
        #[arg(value_enum)]
        state: Toggle,
    },

    /// Apply clips from a device to this clipboard as they arrive.
    ///
    /// Off by default. With it off, a clip is held in memory and applied only
    /// when you run `pliwee clipboard apply`, so a paired device cannot
    /// replace what you are about to paste.
    AutoReceive {
        device: String,
        #[arg(value_enum)]
        state: Toggle,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Direction {
    /// This machine may send its clipboard to that device.
    Send,
    /// That device may send its clipboard to this machine.
    Receive,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Toggle {
    On,
    Off,
}

impl Toggle {
    fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let path = control_socket_path();
    let stream = UnixStream::connect(&path).await.map_err(|e| {
        anyhow::anyhow!(
            "cannot reach the daemon at {} ({e}).\n{}",
            path.display(),
            platform::START_HINT
        )
    })?;

    match args.command {
        Command::Status => simple(stream, Request::Status).await,
        Command::Devices => simple(stream, Request::Devices).await,
        Command::Ping { device } => simple(stream, Request::Ping { device }).await,
        Command::Unpair { device } => simple(stream, Request::Unpair { device }).await,
        Command::RemoveFromList { fingerprint, all } => match (fingerprint, all) {
            (Some(_), true) => {
                anyhow::bail!("give a fingerprint or --all, not both")
            }
            (None, false) => {
                anyhow::bail!("give a device fingerprint, or --all to remove every revoked device")
            }
            (Some(fingerprint), false) => {
                simple(stream, Request::HideRevokedDevice { fingerprint }).await
            }
            (None, true) => simple(stream, Request::HideAllRevokedDevices).await,
        },
        Command::Pair { ttl } => pair(stream, ttl).await,
        Command::Grant { device, capability } => {
            simple(
                stream,
                Request::Grant {
                    device,
                    capability,
                    granted: true,
                },
            )
            .await
        }
        Command::Revoke { device, capability } => {
            simple(
                stream,
                Request::Grant {
                    device,
                    capability,
                    granted: false,
                },
            )
            .await
        }
        Command::Transfers => simple(stream, Request::Transfers).await,
        Command::Cancel { transfer } => simple(stream, Request::CancelTransfer { transfer }).await,
        Command::Send { device, file } => send_file(stream, device, file).await,
        Command::Clipboard(clipboard) => {
            let request = match clipboard {
                ClipboardCommand::Status => Request::ClipboardStatus,
                ClipboardCommand::Send { device, sensitive } => {
                    Request::ClipboardSend { device, sensitive }
                }
                ClipboardCommand::Apply { device } => Request::ClipboardApply { device },
                ClipboardCommand::Allow {
                    device,
                    direction,
                    state,
                } => Request::ClipboardPolicy {
                    device,
                    flag: match direction {
                        Direction::Send => ClipboardFlag::Send,
                        Direction::Receive => ClipboardFlag::Receive,
                    },
                    enabled: state.enabled(),
                },
                ClipboardCommand::AutoSend { device, state } => Request::ClipboardPolicy {
                    device,
                    flag: ClipboardFlag::AutoSend,
                    enabled: state.enabled(),
                },
                ClipboardCommand::AutoReceive { device, state } => Request::ClipboardPolicy {
                    device,
                    flag: ClipboardFlag::AutoReceive,
                    enabled: state.enabled(),
                },
            };
            simple(stream, request).await
        }
        Command::Notifications(notifications) => {
            let request = match notifications {
                NotificationsCommand::Status => Request::NotificationsStatus,
                NotificationsCommand::Mirror { device, state } => Request::NotificationsPolicy {
                    device,
                    setting: NotificationSetting::Mirror {
                        enabled: state.enabled(),
                    },
                },
                NotificationsCommand::WhenLocked { device, policy } => {
                    Request::NotificationsPolicy {
                        device,
                        setting: NotificationSetting::WhenLocked {
                            policy: policy.as_str().to_string(),
                        },
                    }
                }
                NotificationsCommand::DismissSync { device, state } => {
                    Request::NotificationsPolicy {
                        device,
                        setting: NotificationSetting::DismissSync {
                            enabled: state.enabled(),
                        },
                    }
                }
            };
            simple(stream, request).await
        }
    }
}

async fn simple(stream: UnixStream, request: Request) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();

    write_json(&mut write, &request).await?;
    let Some(line) = lines.next_line().await? else {
        anyhow::bail!("daemon closed the connection without replying");
    };

    match serde_json::from_str::<Response>(&line)? {
        Response::Status(s) => {
            println!("Pliwee");
            println!("  device      {} ({})", s.device_name, s.device_id);
            println!("  fingerprint {}", s.fingerprint_short);
            // Local truth, never a wire claim: a peer's assertion about its
            // own key storage is unverifiable, so it is shown here and never
            // advertised (PLAT-DEC-012).
            println!("  key         {}-backed", s.key_backing);
            println!(
                "  listening   port {} ({})",
                s.listen_port, s.listen_families
            );
            println!(
                "  protocol    v{}..v{}",
                s.protocol_version_min, s.protocol_version_max
            );
            println!("  capabilities {}", s.capabilities.join(", "));
            println!("  paired      {} device(s)", s.paired_devices);
            if s.pairing_active {
                println!("  pairing     window OPEN");
            }
            // ADR-0020 D9: where this identity came from, if it was carried
            // over. The source directory is named because it still exists and
            // is what an OmniBridge downgrade would start on.
            if let Some(m) = &s.migrated_from {
                println!(
                    "  migrated    from {}{} (source left unchanged)",
                    m.source,
                    if m.this_run { ", on this start" } else { "" }
                );
            }
            if !s.legacy_partial_files.is_empty() {
                println!(
                    "\n  {} interrupted OmniBridge transfer(s), left in place:",
                    s.legacy_partial_files.len()
                );
                for f in &s.legacy_partial_files {
                    println!("    {f}");
                }
            }

            if s.devices.is_empty() {
                println!("\n  no paired devices. Run: pliwee pair");
            } else {
                println!("\n  devices:");
                for d in &s.devices {
                    print_device(d, "    ");
                }
            }
        }
        Response::Devices(devices) => {
            if devices.is_empty() {
                println!("no paired devices. Run: pliwee pair");
                return Ok(());
            }
            for d in &devices {
                print_device(d, "   ");
            }
        }
        Response::Transfers(transfers) => {
            if transfers.is_empty() {
                println!("no transfers since the daemon started.");
                return Ok(());
            }
            for t in &transfers {
                print_transfer(t, "  ");
                println!();
            }
        }
        Response::Clipboard(report) => print_clipboard_status(&report),
        Response::Notifications(report) => print_notifications_status(&report),
        Response::Pong { rtt_ms } => println!("pong in {rtt_ms} ms"),
        Response::Ok { message } => println!("{message}"),
        Response::Error { message } => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Prints one device, keeping "paired", "connected" and "last known battery"
/// visibly separate.
///
/// They are three different facts and conflating them is how a dead session
/// ends up displayed as a live one. `paired` comes from the trust store,
/// `state` from whether a session exists right now, and a battery reading is
/// shown only with its age and only while a session is live.
fn print_device(d: &DeviceReport, indent: &str) {
    println!("{indent}{}  {}", d.device_name, d.device_id);
    println!("{indent}   platform    {}", d.platform);
    println!("{indent}   fingerprint {}", d.fingerprint_short);
    println!("{indent}   paired      {}", yes_no(d.paired));
    println!("{indent}   connected   {}", yes_no(d.connected));
    println!("{indent}   state       {}", d.state);
    if let Some(secs) = d.silent_secs {
        println!("{indent}   last frame  {} ago", human_duration(secs));
    }
    if let Some(secs) = d.last_seen_secs_ago {
        if !d.connected {
            println!("{indent}   last seen   {} ago", human_duration(secs));
        }
    }
    println!(
        "{indent}   granted     {}",
        if d.granted_capabilities.is_empty() {
            "-".to_string()
        } else {
            d.granted_capabilities.join(", ")
        }
    );
    if let Some(b) = &d.battery {
        println!("{indent}   battery     {}", battery_line(b));
    }
}

/// Renders `pliwee clipboard status`.
///
/// Three facts are kept visibly apart, because collapsing them is how a user
/// comes to believe sync is running when it is not: whether the capability is
/// *granted*, whether the *policy* permits a direction, and whether this
/// session can technically do it at all.
fn print_clipboard_status(report: &ClipboardStatusReport) {
    if !report.enabled {
        println!("clipboard.v1 is not enabled: {}", report.backend_detail);
        return;
    }

    println!("Clipboard");
    println!("  backend              {}", report.backend);
    println!("  detail               {}", report.backend_detail);
    // Three capabilities, named separately and in the order they stop
    // working. A desktop can have the first and neither of the others, which
    // is exactly what every current Ubuntu LTS and Debian Stable is, and one
    // collapsed "clipboard: available" line would be false on all three.
    println!(
        "  ordinary clipboard   {}",
        if report.backend_available {
            "available"
        } else {
            "unavailable"
        }
    );
    if !report.backend_available {
        println!("                       {}", report.backend_detail);
    }
    println!(
        "  sensitive clipboard  {}",
        if report.sensitive_available {
            "available"
        } else {
            "unavailable"
        }
    );
    if !report.sensitive_available {
        // Said here rather than only in the failure text, which is the whole
        // point: before this, a person learned that a password would not
        // arrive at the moment a password did not arrive.
        println!("                       {}", report.sensitive_detail);
        println!(
            "                       A clip arriving with sensitive_hint set will be \
             REFUSED rather"
        );
        println!(
            "                       than written unmarked. Ordinary clipboard sharing is \
             unaffected."
        );
    }
    // Kept apart from the two above, because it fails for a third reason:
    // auto-send needs a *compositor* that reports changes, sensitive marking
    // needs a `wl-copy` that has the flag, and neither implies the other.
    println!(
        "  auto-send            {}",
        if report.watch_available {
            "supported on this session"
        } else {
            "NOT supported here — this session cannot detect clipboard changes"
        }
    );
    // Its own line, because until finding F-2 the product asserted this one
    // rather than printing it: four places said "manual send still works"
    // whenever auto-send did not, and on a compositor with no data-control and
    // no reachable Xwayland it does not. Both read a selection this process
    // does not own; if the watcher cannot, neither can a send.
    //
    // Derived from the same flag rather than probed, deliberately: probing
    // would mean a real `wl-paste`, which on exactly the session in question
    // blocks for the backend timeout — `clipboard status` would hang for the
    // length of its own diagnosis.
    println!(
        "  manual send          {}",
        manual_send_line(report, platform::MANUAL_SEND_NEEDS_WATCH)
    );
    println!("  receiving            supported — writing a clip needs no data-control protocol");
    // Printed so the bounded-growth property is observable rather than merely
    // documented. Neither cache holds content.
    println!(
        "  caches               {} event id(s), {} suppression entr(ies)",
        report.event_cache_entries, report.suppression_cache_entries
    );

    if report.peers.is_empty() {
        println!("\n  no paired devices. Run: pliwee pair");
        return;
    }

    println!("\n  devices:");
    for p in &report.peers {
        println!("    {}  {}", p.device_name, p.device_id);
        println!("      fingerprint  {}", p.fingerprint_short);
        println!(
            "      clipboard.v1 {}",
            match (p.revoked, p.granted) {
                // Revocation is the bigger fact and comes first: telling
                // someone to grant a capability on a device that is no longer
                // paired would send them down the wrong path.
                (true, _) => "unavailable — this device's pairing was revoked",
                (false, true) => "granted",
                (false, false) => "NOT granted (run: pliwee grant <device> clipboard.v1)",
            }
        );
        println!("      connected    {}", yes_no(p.connected));
        println!(
            "      policy       send={} receive={} auto-send={} auto-receive={}",
            on_off(p.allow_send),
            on_off(p.allow_receive),
            on_off(p.auto_send),
            on_off(p.auto_receive),
        );
        if p.auto_send && !report.watch_available {
            println!(
                "      warning      auto-send is on but this session cannot \
                 detect clipboard changes, so nothing is pushed"
            );
        }
        if let Some(outcome) = &p.last_outcome {
            println!("      last result  {outcome}");
        }
    }

    if !report.pending.is_empty() {
        println!("\n  waiting to be applied (auto-receive is off):");
        for clip in &report.pending {
            // Size, hash prefix and age — never the text. A preview here
            // would defeat the whole point of not carrying content on the
            // control socket.
            println!(
                "    from {} ({})  {} bytes  sha256:{}  {}{} ago",
                clip.device_name,
                clip.fingerprint_short,
                clip.bytes,
                clip.hash_prefix,
                if clip.sensitive { "SENSITIVE  " } else { "" },
                human_duration(clip.age_secs),
            );
            println!(
                "      apply with: pliwee clipboard apply {}",
                clip.fingerprint_short.replace(' ', "").to_lowercase()
            );
        }
    }
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn battery_line(b: &BatteryReport) -> String {
    format!(
        "{}% ({}, {} old{})",
        b.percentage,
        b.charging_state,
        human_duration(b.age_secs),
        if b.stale { "; STALE" } else { "" },
    )
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

fn human_duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        s => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// Prints one transfer.
fn print_transfer(t: &TransferReport, indent: &str) {
    // The short id is what a user types into `pliwee cancel`.
    println!(
        "{indent}{}  {} {} {}",
        &t.transfer_id[..8],
        t.direction,
        if t.direction == "sending" { "->" } else { "<-" },
        t.device_name,
    );
    println!("{indent}   file        {}", t.filename);
    println!("{indent}   size        {}", human_bytes(t.size_bytes));
    println!("{indent}   state       {}", t.state);
    if let Some(reason) = &t.failure {
        println!("{indent}   reason      {reason}");
    }
    if let Some(path) = &t.stored_at {
        println!("{indent}   stored at   {path}");
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Renders a one-line progress bar, rewritten in place.
fn progress_line(t: &TransferReport) -> String {
    match t.percentage {
        Some(pct) => {
            let filled = (pct as usize * 24) / 100;
            let bar: String = std::iter::repeat_n('=', filled)
                .chain(std::iter::repeat_n(' ', 24 - filled))
                .collect();
            format!(
                "  [{bar}] {pct:>3}%  {} / {}",
                human_bytes(t.bytes_transferred),
                human_bytes(t.size_bytes)
            )
        }
        // A zero-byte file has no meaningful percentage; showing 100% or 0%
        // would both be lies of a sort.
        None => format!("  {} ({})", t.state, human_bytes(t.size_bytes)),
    }
}

/// Offers a file and follows it to a terminal state.
async fn send_file(
    stream: UnixStream,
    device: String,
    file: std::path::PathBuf,
) -> anyhow::Result<()> {
    // Resolved locally so a typo fails here, with a clear message, rather
    // than as a daemon-side error about a path the user did not type.
    let path = file
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", file.display()))?;

    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();

    write_json(
        &mut write,
        &Request::Send {
            device,
            path: path.to_string_lossy().into_owned(),
        },
    )
    .await?;

    let mut named = false;
    while let Some(line) = lines.next_line().await? {
        if let Ok(Response::Error { message }) = serde_json::from_str::<Response>(&line) {
            eprintln!("error: {message}");
            std::process::exit(1);
        }

        match serde_json::from_str::<Event>(&line)? {
            Event::TransferProgress(t) => {
                if !named {
                    println!(
                        "{} -> {} ({})",
                        t.filename,
                        t.device_name,
                        human_bytes(t.size_bytes)
                    );
                    named = true;
                }
                // `\r` and no newline: one line that updates in place.
                use std::io::Write as _;
                print!("\r{}", progress_line(&t));
                let _ = std::io::stdout().flush();
            }

            Event::Finished { status, detail } => {
                println!();
                match status.as_str() {
                    "completed" => println!("Sent. {detail}"),
                    "cancelled" => println!("Cancelled: {detail}"),
                    other => {
                        eprintln!("Transfer {other}: {detail}");
                        std::process::exit(1);
                    }
                }
                return Ok(());
            }

            // Not part of a send stream. The file-approval events belong to
            // a `watch_file_offers` stream and cannot arrive here.
            Event::PairingReady { .. }
            | Event::ConfirmRequest { .. }
            | Event::FileApprovalReady { .. }
            | Event::FileOfferRequest(_)
            | Event::FileOfferWithdrawn { .. } => continue,
        }
    }
    Ok(())
}

/// Runs an interactive pairing session.
///
/// The pairing window lives exactly as long as this process: closing the
/// terminal closes the window. That is a feature, not a limitation.
async fn pair(stream: UnixStream, ttl: Option<u64>) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();

    write_json(&mut write, &Request::Pair { ttl_secs: ttl }).await?;

    while let Some(line) = lines.next_line().await? {
        // The daemon may send a plain Response (e.g. an error) instead of an
        // Event, so try both rather than failing on the first mismatch.
        if let Ok(Response::Error { message }) = serde_json::from_str::<Response>(&line) {
            eprintln!("error: {message}");
            std::process::exit(1);
        }

        match serde_json::from_str::<Event>(&line)? {
            Event::PairingReady {
                payload,
                qr_ascii,
                expires_in_secs,
            } => {
                println!("{qr_ascii}");
                println!("Scan this with Pliwee on your phone.");
                println!("Expires in {expires_in_secs}s. The code is single-use.\n");
                println!("If your phone cannot scan, the payload is:\n  {payload}\n");
            }

            Event::ConfirmRequest {
                device_name,
                device_id,
                fingerprint_short,
                ..
            } => {
                println!("A device proved it holds the pairing code:\n");
                println!("  name        {device_name}");
                println!("  device id   {device_id}");
                println!("  fingerprint {fingerprint_short}");
                println!("\nCheck that the fingerprint matches the one shown on the phone.");
                print!("Pair with this device? [y/N] ");

                let accept = read_yes_no().await;
                write_json(&mut write, &Request::Confirm { accept }).await?;
            }

            // Not part of a pairing stream.
            Event::TransferProgress(_)
            | Event::FileApprovalReady { .. }
            | Event::FileOfferRequest(_)
            | Event::FileOfferWithdrawn { .. } => continue,

            Event::Finished { status, detail } => {
                match status.as_str() {
                    "paired" => println!("\nPaired with {detail}."),
                    "declined" => println!("\nDeclined. {detail} was not paired."),
                    "expired" => println!("\nPairing window expired. Run `pliwee pair` again."),
                    other => println!("\nPairing ended: {other} ({detail})"),
                }
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn read_yes_no() -> bool {
    use std::io::Write as _;
    use tokio::io::AsyncBufReadExt;

    // The prompt is written with `print!`, so it sits in `std::io::stdout`'s
    // line buffer with no newline to push it out. Flushing tokio's stdout
    // here would flush a different handle and leave the question invisible:
    // the operator would see the fingerprint, no prompt, and a silent
    // decline 60 seconds later. Flush the handle the prompt was written to.
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    let mut reader = BufReader::new(tokio::io::stdin());
    // A read failure or EOF means nobody answered: the safe default is no.
    match tokio::time::timeout(Duration::from_secs(60), reader.read_line(&mut line)).await {
        Ok(Ok(_)) => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        _ => false,
    }
}

async fn write_json<W: AsyncWriteExt + Unpin>(w: &mut W, value: &Request) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

/// Renders `pliwee notifications status`.
///
/// Every line here is a count, a state or a platform identifier. **No line can
/// carry a notification's title, body or application name**, because no field
/// on the report can — which is what makes the output safe to paste into a bug
/// report without reading it first.
fn print_notifications_status(report: &NotificationsStatusReport) {
    if !report.enabled {
        println!("notifications.v1 is not enabled: {}", report.backend_detail);
        return;
    }

    println!("Notifications");
    println!("  server        {}", report.backend_detail);
    println!(
        "  reachable     {}",
        if report.available {
            "yes"
        } else {
            "NO — this desktop announces no SINK role, so nothing is sent to it"
        }
    );
    println!(
        "  body markup   {}",
        if report.body_markup {
            "yes — bodies are escaped before they are sent"
        } else {
            "no"
        }
    );
    println!(
        "  persistence   {}",
        if report.persistence {
            "yes — notifications stay in the list until acknowledged"
        } else {
            "no — notifications are transient banners"
        }
    );
    println!("  lock state    {}", report.lock_detail);
    println!(
        "  screen        {}",
        if report.locked { "LOCKED" } else { "unlocked" }
    );
    println!("  mirrored now  {}", report.mirrors);

    if report.peers.is_empty() {
        println!("\n  no paired devices.");
        return;
    }

    println!("\n  Devices");
    for peer in &report.peers {
        println!("    {} ({})", peer.device_name, peer.fingerprint_short);
        println!(
            "      notifications.v1 {}",
            match (peer.granted, peer.revoked) {
                (_, true) => "device REVOKED".to_string(),
                (true, false) => "granted".to_string(),
                (false, false) =>
                    "NOT granted (run: pliwee grant <device> notifications.v1)".to_string(),
            }
        );
        println!(
            "      mirror {}   when locked {}   dismiss-sync {}",
            if peer.allow_mirror { "on " } else { "off" },
            peer.when_locked,
            // Truthful about the three ways "on" can still do nothing: the
            // device may not be connected, it may not claim it can act on a
            // dismissal, and this desktop may not be able to tell a human
            // dismissal from a banner timing out. A bare "on" in any of those
            // states would be the switch lying.
            match (
                peer.allow_dismiss_sync,
                peer.connected,
                peer.peer_is_dismiss_target,
                peer.local_reports_dismissals,
            ) {
                (false, ..) => "off".to_string(),
                (true, false, ..) => "on (the device is not connected)".to_string(),
                (true, true, _, false) => "on (this desktop cannot report dismissals)".to_string(),
                (true, true, false, true) =>
                    "on (the device has not said it will act on one)".to_string(),
                (true, true, true, true) => "on".to_string(),
            }
        );
        if peer.dismissals_sent > 0 || peer.dismissals_refused > 0 {
            println!(
                "      dismissals: {} sent, {} declined by the device",
                peer.dismissals_sent, peer.dismissals_refused
            );
        }
        println!(
            "      showing {} of {} mirrored{}",
            peer.displayed,
            peer.mirrors,
            if peer.evicted > 0 {
                format!(", {} closed at the ceiling", peer.evicted)
            } else {
                String::new()
            }
        );

        if peer.connected {
            println!(
                "      roles: this desktop announced {} (epoch {}); the device {} (epoch {})",
                peer.local_roles,
                peer.local_epoch,
                if peer.peer_is_source {
                    "can source notifications"
                } else {
                    "claims no source role"
                },
                peer.peer_epoch
            );
            println!(
                "      dismissal: this desktop {}; the device {}",
                if peer.local_reports_dismissals {
                    "reports human dismissals"
                } else {
                    "cannot report human dismissals"
                },
                if peer.peer_is_dismiss_target {
                    "will act on a dismiss request"
                } else {
                    "claims no dismiss-target role"
                }
            );
            if peer.snapshot_open {
                println!("      a snapshot is in progress");
            }
            if peer.queued > 0 || peer.coalesced > 0 || peer.dropped > 0 {
                println!(
                    "      queue: {} pending, {} coalesced, {} dropped",
                    peer.queued, peer.coalesced, peer.dropped
                );
            }
        } else {
            println!("      not connected");
        }
    }
}

/// The `manual send` line of `pliwee clipboard status`.
///
/// `needs_watch` is the adapter's statement of whether a manual send fails
/// whenever change watching does — true on Linux (finding F-2), false on
/// macOS, where the watch is absent by policy and reading works.
fn manual_send_line(report: &ClipboardStatusReport, needs_watch: bool) -> &'static str {
    if needs_watch {
        if report.watch_available {
            "supported on this session"
        } else {
            "NOT supported here — sending reads the selection the same way \
             auto-send watches it"
        }
    } else if report.backend_available {
        "supported — reading the clipboard does not depend on a change watch here"
    } else {
        "NOT supported here — this session has no working clipboard backend"
    }
}

#[cfg(test)]
mod tests {
    use super::Args;
    use clap::CommandFactory;

    /// `pliwee --version` and the usage line name the installed binary.
    /// The packages ship `/usr/bin/pliwee` and nothing else, so any other
    /// name here sends the reader looking for a command that does not exist.
    #[test]
    fn the_command_identifies_itself_as_pliwee() {
        let mut cmd = Args::command();
        assert_eq!(cmd.get_name(), "pliwee");
        let version = cmd.render_version();
        assert_eq!(
            version.trim_end(),
            format!("pliwee {}", env!("CARGO_PKG_VERSION"))
        );
        let usage = cmd.render_usage().to_string();
        assert!(usage.contains("pliwee"), "usage: {usage}");
        for text in [version, usage, cmd.render_long_help().to_string()] {
            assert!(
                !text.to_ascii_lowercase().contains("omnibridge"),
                "the CLI still names the retired binary: {text}"
            );
        }
    }

    fn clipboard_report(
        backend_available: bool,
        watch_available: bool,
    ) -> pliwee_control::ClipboardStatusReport {
        pliwee_control::ClipboardStatusReport {
            enabled: true,
            backend: "test".into(),
            backend_detail: String::new(),
            backend_available,
            watch_available,
            sensitive_available: true,
            sensitive_detail: String::new(),
            event_cache_entries: 0,
            suppression_cache_entries: 0,
            peers: vec![],
            pending: vec![],
        }
    }

    #[test]
    fn where_manual_send_needs_the_watch_the_line_follows_it_unchanged() {
        // The Linux rule (finding F-2), exactly as it was printed before.
        use super::manual_send_line;
        assert_eq!(
            manual_send_line(&clipboard_report(true, true), true),
            "supported on this session"
        );
        assert!(manual_send_line(&clipboard_report(true, false), true).starts_with("NOT supported"));
    }

    #[test]
    fn where_it_does_not_a_missing_watch_does_not_disable_manual_send() {
        // The macOS case: no watch by policy, reading works.
        use super::manual_send_line;
        assert!(manual_send_line(&clipboard_report(true, false), false).starts_with("supported"));
        assert!(
            manual_send_line(&clipboard_report(false, false), false).starts_with("NOT supported")
        );
    }

    #[test]
    fn this_platform_declares_the_rule_it_is_known_to_have() {
        assert_eq!(
            super::platform::MANUAL_SEND_NEEDS_WATCH,
            !cfg!(target_os = "macos")
        );
    }
}
