//! Which desktop this agent is running on.
//!
//! The one place in `pliweed` that says so. `main.rs` composes the agent from
//! the functions this module exports and never names a platform itself; each
//! submodule answers the same questions — where state lives, which battery,
//! which clipboard, which notification sink, where the control socket is,
//! what to say at startup — for one operating system, by calling its adapter
//! crate.
//!
//! | Module | Adapter | Lifetime |
//! | --- | --- | --- |
//! | [`linux`](self) | `pliwee-linux` | `systemd --user` unit |
//! | [`macos`](self) | `pliwee-macos` | per-user `launchd` agent, registered by `Pliwee.app` |
//!
//! The selection is by `target_os`, at compile time, and nowhere else in the
//! crate. Every non-macOS target keeps the Linux row it has always had, so
//! nothing that compiled before this module existed stops compiling.

#[cfg(not(target_os = "macos"))]
mod linux;
#[cfg(not(target_os = "macos"))]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
