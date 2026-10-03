//! The macOS adapter.
//!
//! The macOS row of the table in `pliwee-linux`'s crate docs: everything the
//! Pliwee Agent needs that is specific to a macOS user session, and nothing
//! else.
//!
//! * [`paths`] — where things live: `~/Library/Application Support/Pliwee`
//!   for state, a `run/` directory beside it for the control socket,
//!   `~/Library/Logs/Pliwee` for the agent's log;
//! * [`keychain`] — a [`SecretStore`](pliwee_core::secret_store::SecretStore)
//!   that keeps the identity's private key in the login keychain rather than
//!   in a file;
//! * [`battery`] — this Mac's own battery, from IOKit's power-source API;
//! * [`clipboard`] — the general pasteboard, through `NSPasteboard`;
//! * [`host`] — the name a peer sees for this Mac.
//!
//! The control endpoint itself is the same Unix domain socket Linux uses,
//! from `pliwee-unix`. macOS is the platform where the existing IPC
//! transfers wholesale; only *where* the socket lives is decided here.
//!
//! # What is *not* here
//!
//! Exactly what is not in `pliwee-linux`: protocol, TLS, pairing, pinning,
//! the session state machine, the capability registry, filename
//! sanitisation, clipboard policy. The Swift application in `macos/` is a
//! client of the agent's control socket and implements none of them either.
//!
//! Also not here, on purpose, and each for a stated reason:
//!
//! * **No notification sink.** Displaying a phone's notifications needs
//!   `UNUserNotificationCenter`, which needs a signed application bundle and
//!   a user authorisation that an ad-hoc development build cannot hold
//!   reliably. The agent registers `notifications.v1` with the portable
//!   `NoSink`, so it announces no `SINK` role and a phone is told the truth.
//! * **No clipboard watch.** `NSPasteboard` has no change notification, and
//!   the `ClipboardBackend` contract forbids satisfying `watch_changes` by
//!   polling. Whether to amend that contract for macOS is PLAT-DEC-009,
//!   which is open. Until it is decided, automatic sending is reported as
//!   unavailable — the state GNOME sessions were in before XFIXES — and
//!   sending by hand works.
//! * **No tray.** On Linux the agent owns the StatusNotifierItem because it
//!   is the process that is always there. On macOS the menu-bar item belongs
//!   to `Pliwee.app`, which is a native application and talks to the agent
//!   over the control socket like any other client.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::sync::Arc;

pub mod battery;
pub mod clipboard;
pub mod host;
pub mod keychain;
pub mod paths;

pub use paths::RuntimePaths;
pub use pliwee_unix::{bind, connect, UnixControlListener, UnixControlTransport};

/// The `launchd` label of the agent, as `Pliwee.app` registers it.
///
/// Must match `Label` in `macos/Resources/LaunchAgents/*.plist`, which
/// `macos/scripts/build-app.sh` checks when it assembles the bundle.
pub const LAUNCHD_LABEL: &str = "io.github.yurisismotto.pliwee.daemon";

/// The application's bundle identifier.
pub const BUNDLE_ID: &str = "io.github.yurisismotto.pliwee";

/// What to tell a person whose client cannot reach the agent.
pub const START_HINT: &str =
    "Open Pliwee and turn on \"Run Pliwee in the background\" in Settings, \
     or run pliweed in a terminal";

/// Whether sending the clipboard by hand fails whenever change watching does.
///
/// False on macOS. The watch is absent by policy — `NSPasteboard` has no
/// change notification and the clipboard contract forbids polling — not
/// because the pasteboard cannot be read: a manual send reads it with
/// `stringForType:`, which works. Deriving one from the other, as the Linux
/// row must (finding F-2), made `pliwee clipboard status` on a Mac report
/// manual sending as unsupported while the agent was sending clips.
pub const MANUAL_SEND_NEEDS_WATCH: bool = false;

/// Where the control socket lives for this user.
///
/// Panics never; an unusable path is reported by [`bind`] and by the client's
/// connect, both of which name it.
pub fn control_socket_path() -> std::path::PathBuf {
    RuntimePaths::for_current_user().control_socket()
}

/// The control transport at this user's default location.
///
/// Refuses, with the reason, a socket path longer than `sun_path` can hold
/// rather than letting `bind` fail with a bare `InvalidInput`.
pub fn default_control_transport() -> Result<UnixControlTransport, paths::PathError> {
    let paths = RuntimePaths::for_current_user();
    paths.check_control_socket()?;
    Ok(UnixControlTransport::new(paths.control_socket()))
}

/// Opens the store at `dir`: `state.json` at mode 0600 in a 0700 directory,
/// and the private key in the login keychain.
///
/// The one place that says "this machine is a Mac". The platform reported to
/// peers is `PLATFORM_UNSPECIFIED`, deliberately: `core.proto` has no macOS
/// value yet, and the two alternatives are worse — `PLATFORM_LINUX` is false,
/// and adding an enum value is a protocol change that wants its own ADR. A
/// peer that reads `UNSPECIFIED` shows no platform at all, which is what the
/// Android app does by design (`UiMapping.platformLabel`). The value is
/// supplied on every load and never persisted, so changing it later costs
/// nothing.
pub fn open_store(dir: impl AsRef<Path>) -> pliwee_core::Result<pliwee_core::store::Store> {
    // Said *before* the keychain is touched, because the touch can block: a
    // differently signed `pliweed` (an ad-hoc rebuild) makes macOS ask
    // whether it may use the key, and the agent waits for the answer. Without
    // this line the log would stop at "starting" with no hint why.
    tracing::info!(
        service = keychain::SERVICE,
        "reading this Mac's identity key from the login keychain; if macOS asks \
         whether pliweed may use it, the agent is waiting for that answer"
    );
    pliwee_core::store::Store::open_with(pliwee_core::store::StoreConfig {
        secrets: Arc::new(keychain::KeychainSecretStore::new(dir.as_ref())),
        backend: Arc::new(pliwee_core::identity::SoftwareBacking),
        platform: pliwee_proto::v1::Platform::Unspecified,
        default_device_name: host::device_name(),
    })
}
