//! Pliwee core: identity, pairing, transport and the capability model.
//!
//! This crate is deliberately free of any daemon, CLI or GUI concerns. It has
//! no global state and does no logging of user content. Everything it needs
//! from the host application arrives through [`session::SessionHost`], which
//! is what makes the whole protocol testable in-process.
//!
//! Layering, bottom to top:
//!
//! ```text
//!   framing      length-prefixed protobuf frames
//!   tls          TLS 1.3 + SPKI pinning
//!   session      handshake, pairing, replay guard, routing
//!   capability   plugin registry; the transport knows no feature names
//! ```

pub mod capability;
pub mod clipboard_policy;
pub mod discovery;
pub mod duplicate_session;
pub mod error;
pub mod fingerprint;
pub mod framing;
pub mod identity;
pub mod notification_policy;
pub mod notifications;
pub mod pairing;
pub mod platform;
pub mod profile;
pub mod qr;
pub mod secret_store;
pub mod session;
pub mod store;
pub mod tls;

pub use error::{Error, PairingError, Result};
pub use fingerprint::Fingerprint;
pub use profile::Profile;

/// Default TCP port. Above 1024 so the daemon never needs privileges.
/// Advertised over mDNS, so a conflicting deployment can simply use another.
pub const DEFAULT_PORT: u16 = 55432;

/// ALPN identifier for the control session (canonical profile). Negotiated
/// by both ends, so a client that reaches an unrelated TLS service (or vice
/// versa) fails fast during the handshake instead of exchanging garbage
/// frames. Which ALPN a connection negotiated also fixes its identity
/// [`Profile`] (ADR-0020 §D4).
pub const ALPN_PROTOCOL: &[u8] = b"pliwee/1";

/// Control-session ALPN of the legacy OmniBridge profile. Accepted through
/// the Pliwee v1.x line; see [`Profile::OmniBridge`].
pub const LEGACY_ALPN_PROTOCOL: &[u8] = b"omnibridge/1";

/// ALPN identifier for a bulk data stream, canonical profile (see ADR-0012,
/// ADR-0013).
///
/// A data stream is a second TLS 1.3 connection to the *same* port, with the
/// *same* mutual authentication and the *same* pinned identities. ALPN is
/// what tells the listener which of the two it just accepted, before a single
/// application byte is read.
///
/// Sharing the port is deliberate: it means file transfer inherits the
/// listener, the discovery record and the dual-stack binding that are already
/// certified, and adds no second thing to find, firewall or advertise.
pub const ALPN_DATA_PROTOCOL: &[u8] = b"pliwee-data/1";

/// Data-stream ALPN of the legacy OmniBridge profile.
pub const LEGACY_ALPN_DATA_PROTOCOL: &[u8] = b"omnibridge-data/1";

/// DNS-SD service type used for LAN discovery (canonical profile).
pub const SERVICE_TYPE: &str = "_pliwee._tcp.local.";

/// DNS-SD service type of the legacy OmniBridge profile. The daemon
/// advertises one instance under both types, so an un-upgraded OmniBridge
/// 1.0.0 app still finds it.
pub const LEGACY_SERVICE_TYPE: &str = "_omnibridge._tcp.local.";
