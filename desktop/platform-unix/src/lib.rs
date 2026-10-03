//! The POSIX pieces every Unix adapter shares.
//!
//! Today that is one thing: the control endpoint as a Unix domain socket —
//! binding it, telling a live owner from a stale file, keeping it owner-only,
//! and connecting to it from a client.
//!
//! # Why this is its own crate
//!
//! It used to live in `pliwee-linux`, because Linux was the only adapter. None
//! of it is Linux: `AF_UNIX`, `chmod` and `ECONNREFUSED` behave the same way on
//! macOS, and the live-owner rule below is a security property, not a
//! convenience. The macOS adapter needs exactly this code, and the two ways to
//! give it the code without this crate were both wrong — depending on a crate
//! that documents itself as "the Linux adapter" (and carries XDG, systemd and
//! D-Bus), or keeping a second copy of the bind logic that could drift from the
//! first.
//!
//! `pliwee-linux` re-exports everything here, so its public API did not
//! change when the code moved.
//!
//! # What is *not* here
//!
//! *Where* the socket lives. That is the one genuinely platform-specific
//! decision — `$XDG_RUNTIME_DIR` on Linux, `~/Library/Application Support` on
//! macOS — and each adapter makes it. This crate takes a path and does the
//! same correct thing with any of them.

use std::path::{Path, PathBuf};

use pliwee_control::transport::{BindError, ControlListener, ControlTransport};
use tokio::net::{UnixListener, UnixStream};

/// The Unix-domain-socket control transport.
#[derive(Debug, Clone)]
pub struct UnixControlTransport {
    path: PathBuf,
}

impl UnixControlTransport {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Releases the endpoint without holding the listener.
    ///
    /// The agent hands its listener to a task and then wants to unbind on
    /// shutdown, which is a different lifetime from the listener's. Naming it
    /// here rather than calling `remove_file` in `main` keeps "unbind the
    /// control endpoint" a concept the adapter owns — a named pipe has no
    /// file to unlink, and the binary should not have to know that.
    pub fn release(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl ControlTransport for UnixControlTransport {
    type Listener = UnixControlListener;

    fn bind(&self) -> Result<Self::Listener, BindError> {
        bind(&self.path)
    }

    fn endpoint(&self) -> String {
        self.path.display().to_string()
    }
}

/// A bound control socket.
#[derive(Debug)]
pub struct UnixControlListener {
    inner: UnixListener,
    path: PathBuf,
}

#[async_trait::async_trait]
impl ControlListener for UnixControlListener {
    type Stream = UnixStream;

    async fn accept(&self) -> std::io::Result<Self::Stream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(stream)
    }

    fn describe(&self) -> String {
        self.path.display().to_string()
    }

    fn release(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Binds the control socket, replacing a stale one left by a crash.
///
/// # Live owner versus stale file
///
/// A leftover socket file from an unclean shutdown must be removed, or `bind`
/// fails for ever. A socket file with a *live* daemon behind it must not be,
/// because removing it would silently steal the endpoint from a running
/// agent. The two are told apart by connecting: a stale socket refuses with
/// `ECONNREFUSED`, a live one accepts.
///
/// The pre-Wave-0 code removed unconditionally. That was defensible when only
/// one implementation existed and the reasoning was "a second live daemon
/// would have failed its own port bind first" — but the port bind happens
/// *after* this one in `main`, and on Windows the equivalent mistake is a
/// named-pipe squat. The distinguishable [`BindError::AlreadyOwned`] is what
/// a named-pipe implementation needs, so Linux establishes the semantics now.
pub fn bind(path: &Path) -> Result<UnixControlListener, BindError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        harden(parent, 0o700)?;
    }

    match path.try_exists() {
        Ok(true) => {
            if socket_has_live_owner(path) {
                return Err(BindError::AlreadyOwned {
                    detail: format!("{} is accepting connections", path.display()),
                });
            }
            // Stale. Safe to remove: only this user can reach the directory.
            std::fs::remove_file(path)?;
        }
        Ok(false) => {}
        // A metadata error is not absence. Refusing here is the same rule the
        // identity store follows: never act on "probably not there".
        Err(e) => {
            return Err(BindError::Io(std::io::Error::new(
                e.kind(),
                format!("{} could not be examined: {e}", path.display()),
            )))
        }
    }

    let listener = UnixListener::bind(path)?;
    harden(path, 0o600)?;
    Ok(UnixControlListener {
        inner: listener,
        path: path.to_path_buf(),
    })
}

/// Whether something is listening on this socket right now.
fn socket_has_live_owner(path: &Path) -> bool {
    // A blocking connect on a Unix socket to a local path either succeeds
    // immediately or fails immediately; there is no network round trip to
    // wait for, so this cannot hang the way a TCP probe could.
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

fn harden(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Connects to the agent's control endpoint.
///
/// The client half, so that `pliwee-cli` and `pliwee-gui` reach the agent
/// through an adapter crate rather than through the agent's own crate.
pub async fn connect(path: &Path) -> std::io::Result<UnixStream> {
    UnixStream::connect(path).await
}

/// The longest socket path this platform's `sockaddr_un` can hold, excluding
/// the terminating NUL.
///
/// `sun_path` is 108 bytes on Linux and 104 on macOS and the BSDs. A path
/// that does not fit is not truncated by the kernel — `bind` refuses it — but
/// the refusal arrives as a bare `InvalidInput` with no mention of the limit,
/// which is why the adapters check it up front and say what went wrong.
pub const fn max_socket_path_len() -> usize {
    if cfg!(any(target_os = "linux", target_os = "android")) {
        107
    } else {
        103
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn binding_twice_reports_already_owned_and_not_a_generic_io_error() {
        // The property a Windows named-pipe implementation depends on: a name
        // that is already owned must be distinguishable, so the agent can
        // abort instead of quietly choosing another name.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("control.sock");

        let _first = bind(&path).expect("first bind");
        match bind(&path) {
            Err(BindError::AlreadyOwned { .. }) => {}
            other => panic!("expected AlreadyOwned, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_stale_socket_file_is_replaced_rather_than_refused() {
        // The ordinary case after an unclean shutdown: a socket file with
        // nothing behind it. Refusing here would leave the agent unable to
        // start until someone deleted a file by hand.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("control.sock");

        {
            let _listener = bind(&path).expect("first bind");
        } // dropped: the file remains, the listener does not

        assert!(path.exists(), "the socket file must survive the drop");
        let _second = bind(&path).expect("a stale socket must be replaced");
    }

    #[tokio::test]
    async fn a_bound_socket_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("control.sock");
        let _listener = bind(&path).expect("bind");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "mode {mode:o}");
    }

    #[tokio::test]
    async fn the_socket_directory_is_made_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let parent = dir.path().join("run");
        std::fs::create_dir(&parent).expect("mkdir");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let _listener = bind(&parent.join("control.sock")).expect("bind");
        let mode = std::fs::metadata(&parent)
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "mode {mode:o}");
    }

    #[tokio::test]
    async fn release_removes_the_endpoint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("control.sock");
        let listener = bind(&path).expect("bind");
        assert!(path.exists());
        listener.release();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn a_client_reaches_a_bound_listener() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("control.sock");
        let listener = bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.expect("accept");
            stream.write_all(b"hello\n").await.expect("write");
        });
        let mut client = connect(&path).await.expect("connect");
        let mut got = String::new();
        client.read_to_string(&mut got).await.expect("read");
        server.await.expect("server task");
        assert_eq!(got, "hello\n");
    }

    #[test]
    fn a_path_longer_than_sun_path_is_refused_by_bind() {
        // The limit is real, and `max_socket_path_len` is the right number:
        // one byte over it must fail, and a path exactly at it must not fail
        // *for length*. Measured against the kernel rather than asserted, so
        // a wrong constant cannot hide behind a test that only reads it back.
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let _guard = rt.enter();
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("s");
        std::fs::create_dir(&base).expect("mkdir");
        let base_len = base.as_os_str().len() + 1; // the separator
        let max = max_socket_path_len();
        assert!(base_len < max, "temp dir too long to measure: {base:?}");

        let at_limit = base.join("x".repeat(max - base_len));
        assert_eq!(at_limit.as_os_str().len(), max);
        bind(&at_limit).expect("a path exactly at the limit must bind");

        let over = base.join("y".repeat(max - base_len + 1));
        assert!(
            bind(&over).is_err(),
            "one byte over the limit must not bind"
        );
    }
}
