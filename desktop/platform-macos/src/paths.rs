//! Where the agent keeps things on a Mac.
//!
//! One function of one input — the home directory — so that the agent, the
//! CLI and the tests all derive the same paths, and so that `Pliwee.app`'s
//! Swift `RuntimePaths` can be checked against this file line for line.
//!
//! | What | Where |
//! | --- | --- |
//! | identity state (`state.json`) | `~/Library/Application Support/Pliwee/` |
//! | identity private key | the login keychain — see [`crate::keychain`] |
//! | control socket | `~/Library/Application Support/Pliwee/run/control.sock` |
//! | agent log | `~/Library/Logs/Pliwee/pliweed.log` |
//! | received files | `~/Downloads/Pliwee/` (the shared `files.v1` default) |
//!
//! # Why the socket is not in `$TMPDIR`
//!
//! Linux puts it in `$XDG_RUNTIME_DIR`, which is per-user, 0700, and emptied
//! at logout. macOS has no exact equivalent. Three candidates were weighed:
//!
//! * **`/tmp/pliwee-<uid>`** — short, but `/tmp` is shared: another local
//!   user can create the directory first, and every client would then be
//!   talking to whatever they put there. Rejected.
//! * **`$TMPDIR` (`/var/folders/…/T/`)** — per-user and short, but its value
//!   differs between a shell, a GUI app and a `launchd` job unless each one
//!   asks `confstr` for it, and the system cleans it on its own schedule. A
//!   socket the agent is still listening on must not vanish underneath it.
//! * **`~/Library/Application Support/Pliwee/run/`** — chosen. Under the
//!   user's home, so no other user can pre-create it; the same path for every
//!   process of the user without asking anything but `$HOME`; never cleaned
//!   by the system; and it is where Apple says an application's own support
//!   files go. `run/` keeps the socket out of the directory that holds the
//!   identity, as `$XDG_RUNTIME_DIR` does on Linux.
//!
//! The cost is length. `sun_path` holds 103 bytes on macOS, and this path is
//! 52 bytes plus the length of the home directory — which leaves room for a
//! home directory of 51 bytes, `/Users/` and a 44-character account name. A
//! longer one is refused by [`RuntimePaths::check_control_socket`] with that
//! explanation, rather than truncated, moved somewhere else, or left to
//! `bind`'s bare `InvalidInput`.

use std::path::{Path, PathBuf};

/// The directory name under `Application Support` and `Logs`.
///
/// The product name, as Finder shows it, rather than the bundle identifier:
/// both are common, and this is the one a person can find.
pub const APP_DIR: &str = "Pliwee";

/// Every path the macOS agent uses, derived from one home directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    home: PathBuf,
}

/// Why a path cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The control socket path does not fit in `sockaddr_un`.
    SocketPathTooLong {
        path: PathBuf,
        len: usize,
        max: usize,
    },
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SocketPathTooLong { path, len, max } => write!(
                f,
                "the control socket path {} is {len} bytes, and a Unix socket \
                 path on macOS can be at most {max}. The home directory is \
                 too long for Pliwee's socket location; this is a known \
                 limitation, see macos/README.md",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PathError {}

impl RuntimePaths {
    /// Paths under an explicit home directory. What the tests use.
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    /// Paths for the user running this process.
    ///
    /// `$HOME` when it is set to an absolute path — `launchd` sets it for
    /// every agent, and a shell has it — otherwise the account database's
    /// answer, which is what `std::env::home_dir` falls back to.
    pub fn for_current_user() -> Self {
        Self::from_home(home_dir(std::env::var_os("HOME")))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `~/Library/Application Support/Pliwee` — the identity's data
    /// directory, the macOS counterpart of `~/.local/share/pliwee`.
    pub fn data_dir(&self) -> PathBuf {
        self.home
            .join("Library")
            .join("Application Support")
            .join(APP_DIR)
    }

    /// The directory holding the control socket. Made 0700 by `bind`.
    pub fn run_dir(&self) -> PathBuf {
        self.data_dir().join("run")
    }

    /// The control socket.
    pub fn control_socket(&self) -> PathBuf {
        self.run_dir().join("control.sock")
    }

    /// `~/Library/Logs/Pliwee` — where Console.app looks for a user's logs.
    pub fn logs_dir(&self) -> PathBuf {
        self.home.join("Library").join("Logs").join(APP_DIR)
    }

    /// The agent's log file when it runs as the `launchd` agent.
    pub fn log_file(&self) -> PathBuf {
        self.logs_dir().join("pliweed.log")
    }

    /// Refuses a control socket path that `sockaddr_un` cannot hold.
    pub fn check_control_socket(&self) -> Result<(), PathError> {
        let path = self.control_socket();
        let len = path.as_os_str().len();
        let max = pliwee_unix::max_socket_path_len();
        if len > max {
            return Err(PathError::SocketPathTooLong { path, len, max });
        }
        Ok(())
    }
}

fn home_dir(env_home: Option<std::ffi::OsString>) -> PathBuf {
    env_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            // Not deprecated since Rust 1.86 (its Windows behaviour was the
            // problem, and was fixed); the allow keeps 1.85-era toolchains
            // quiet. On macOS it reads the account database via getpwuid_r.
            #[allow(deprecated)]
            std::env::home_dir()
        })
        // An account with no home directory at all is broken; the working
        // directory is what the Linux adapter falls back to in the same case.
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_path_is_derived_from_the_home_directory() {
        let p = RuntimePaths::from_home("/Users/ana");
        assert_eq!(
            p.data_dir(),
            PathBuf::from("/Users/ana/Library/Application Support/Pliwee")
        );
        assert_eq!(
            p.control_socket(),
            PathBuf::from("/Users/ana/Library/Application Support/Pliwee/run/control.sock")
        );
        assert_eq!(
            p.log_file(),
            PathBuf::from("/Users/ana/Library/Logs/Pliwee/pliweed.log")
        );
    }

    #[test]
    fn the_socket_is_not_in_the_identity_directory_itself() {
        // `bind` makes the socket's parent 0700 and replaces a stale socket
        // file there. Doing either inside the directory that holds
        // `state.json` would tie the identity's directory to the socket's
        // lifecycle for no reason.
        let p = RuntimePaths::from_home("/Users/ana");
        assert_ne!(p.control_socket().parent(), Some(p.data_dir().as_path()));
        assert!(p.control_socket().starts_with(p.data_dir()));
    }

    #[test]
    fn an_ordinary_home_directory_fits_in_sun_path() {
        RuntimePaths::from_home("/Users/ana")
            .check_control_socket()
            .expect("a short home fits");
    }

    #[test]
    fn the_documented_headroom_is_the_real_headroom() {
        // The module docs promise a 51-byte home directory fits and a 52-byte
        // one does not. Measured against the same constant `bind` is tested
        // against in `pliwee-unix`, not against a number copied here.
        let fits = format!("/Users/{}", "a".repeat(44));
        assert_eq!(fits.len(), 51);
        RuntimePaths::from_home(&fits)
            .check_control_socket()
            .expect("a 51-byte home must fit");

        let too_long = format!("/Users/{}", "a".repeat(45));
        match RuntimePaths::from_home(&too_long).check_control_socket() {
            Err(PathError::SocketPathTooLong { len, max, .. }) => {
                assert_eq!(len, max + 1);
            }
            other => panic!("expected SocketPathTooLong, got {other:?}"),
        }
    }

    #[test]
    fn a_relative_home_is_not_trusted() {
        // The same rule `pliwee-core` applies to XDG variables: a relative
        // value would resolve against whatever directory the process happens
        // to be started in.
        let home = home_dir(Some("relative/home".into()));
        assert!(home.is_absolute() || home == Path::new("."), "{home:?}");
        assert_ne!(home, PathBuf::from("relative/home"));
    }

    #[test]
    fn an_absolute_home_is_used_as_given() {
        assert_eq!(
            home_dir(Some("/Users/ana".into())),
            PathBuf::from("/Users/ana")
        );
    }
}
