//! `pliweed` — the Pliwee Agent, composed for one desktop platform.
//!
//! After Wave 0 this crate is a *composition*, not an implementation: it
//! wires the portable agent (`pliwee-runtime`) to a platform adapter —
//! `pliwee-linux`, or `pliwee-macos` on a Mac, chosen in [`platform`] — and
//! adds a `main`. The modules below are re-exports so
//! that existing callers — the integration tests above all — keep one import
//! path while the code behind it lives where it belongs.
//!
//! | Module | Now lives in | Why |
//! | --- | --- | --- |
//! | `control` | `pliwee-control` | the CLI/GUI contract, shared without inheriting the agent |
//! | `listener`, `mdns`, `state`, `approval` | `pliwee-runtime` | portable; no platform surface |
//! | `server` | `pliwee-runtime` + the adapter | the protocol is portable, the endpoint is not |

pub use pliwee_runtime::{approval, listener, mdns, state};

/// The platform adapter this build composes: Linux, or macOS.
pub mod platform;

/// The local control protocol.
pub mod control {
    /// Where this platform's agent puts its control socket.
    pub use crate::platform::control_socket_path;
    pub use pliwee_control::*;
}

/// The control-endpoint server.
///
/// [`run`] is portable and takes any bound endpoint; [`bind`] is the
/// Unix-domain implementation of one, shared by the Linux and macOS adapters
/// through `pliwee-unix`.
///
/// [`run`]: pliwee_runtime::server::run
/// [`bind`]: crate::platform::bind
pub mod server {
    pub use pliwee_runtime::server::run;

    /// Binds a control socket at `path`.
    ///
    /// Kept as an `anyhow`-returning wrapper because that is the shape the
    /// agent and its tests already use; the typed
    /// [`pliwee_control::transport::BindError`] is available from the
    /// adapter's own `bind` for callers that need to tell "already owned"
    /// from a generic I/O failure.
    pub fn bind(path: &std::path::Path) -> anyhow::Result<crate::platform::UnixControlListener> {
        Ok(crate::platform::bind(path)?)
    }
}
