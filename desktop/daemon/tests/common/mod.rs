//! Test harness: a full second device, over real TLS, in-process.
//!
//! Nothing here weakens security to make tests pass. The client uses the same
//! `pliwee_core::tls::client_config` as production code, with real
//! certificate pinning. Tests that expect a rejection get one from the actual
//! verifier, not from a stub.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pliwee_capability_battery::{BatteryCapability, BatteryState};
use pliwee_core::capability::CapabilityRegistry;
use pliwee_core::error::{PairingError, Result};
use pliwee_core::identity::LocalIdentity;
use pliwee_core::pairing::PairingToken;
use pliwee_core::session::{
    self, ClientHandshake, PeerStatus, SessionHandle, SessionHost, SessionId,
};
use pliwee_core::store::Store;
use pliwee_core::{Fingerprint, Profile};
use pliwee_daemon::state::DaemonState;
use pliwee_proto::v1;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// The wire identity profile this test process runs its clients under.
///
/// The Wave 5 plan runs `wire`, `e2e`, `files` and `sessions` **twice**, once
/// per profile (ADR-0020 §D4). The run is chosen by `PLIWEE_TEST_PROFILE`:
/// `pliwee` (also the default, when unset) or `omnibridge`. Anything else
/// panics — a typo must not quietly run the canonical profile again and call
/// it the legacy run. Every session the harness establishes asserts that it
/// really negotiated this profile, so a green run is evidence of the profile
/// it names.
pub fn test_profile() -> Profile {
    match std::env::var("PLIWEE_TEST_PROFILE") {
        Err(std::env::VarError::NotPresent) => Profile::Pliwee,
        Ok(name) => Profile::from_name(&name).unwrap_or_else(|| {
            panic!("PLIWEE_TEST_PROFILE={name:?} is not a profile; use pliwee or omnibridge")
        }),
        Err(e) => panic!("PLIWEE_TEST_PROFILE is unreadable: {e}"),
    }
}

/// The other profile, for no-hybrid tests.
pub fn other_profile(profile: Profile) -> Profile {
    match profile {
        Profile::Pliwee => Profile::OmniBridge,
        Profile::OmniBridge => Profile::Pliwee,
    }
}

/// Installs the crypto provider once per test process.
pub fn init_crypto() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// Server side (the Fedora daemon)
// ---------------------------------------------------------------------------

pub struct TestServer {
    pub addr: SocketAddr,
    pub state: Arc<DaemonState>,
    pub fingerprint: Fingerprint,
    pub battery: Arc<BatteryState>,
    /// Which address families this server's listener actually accepts on.
    pub families: pliwee_daemon::listener::Families,
    /// `files.v1`, in the acceptor role: this is the end that listens.
    pub transfers: Arc<TransferManager>,
    /// `clipboard.v1`, with an in-memory clipboard so the suite never touches
    /// the developer's real one.
    pub clipboard: Arc<ClipboardManager>,
    pub clipboard_backend: Arc<MemoryBackend>,
    /// `notifications.v1`, with an in-memory notification server and a
    /// steerable lock, so the suite never posts a notification on the
    /// developer's desktop and never has to lock their screen.
    pub notifications: Arc<NotificationManager>,
    pub notification_sink: Arc<MemorySink>,
    pub notification_lock: Arc<MemoryLock>,
    /// Where this server stores received files.
    pub downloads: std::path::PathBuf,
    /// Steers the "does a human accept this file?" answer.
    ///
    /// Present on every server, but only *wired* to `files.v1` when the
    /// server was started with [`ApprovalMode::Switch`]. A broker-backed
    /// server leaves this at zero asks, which is itself the assertion that
    /// the real seam is the one being exercised.
    pub approvals: Arc<ApprovalSwitch>,
    /// The production approval seam, when this server was built with it.
    ///
    /// The very object `files.v1` asks and the control server attaches a
    /// provider to — not a second one that happens to look the same.
    pub file_approval: Option<Arc<FileApproval>>,
    _dir: tempfile::TempDir,
}

/// Which `TransferApproval` a [`TestServer`] is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// An in-test switch. What almost every `files.v1` test wants: the
    /// question of *who* answers is not what those tests are about.
    Switch,
    /// The production [`FileApproval`], so a test drives the real path a
    /// desktop UI drives — control socket, provider attachment and all.
    Broker,
}

impl TestServer {
    /// A server on IPv4 loopback. The default for tests that do not care
    /// about address families.
    pub async fn start() -> Self {
        Self::start_inner(false, ApprovalMode::Switch).await
    }

    /// A server whose `files.v1` asks the production approval seam.
    ///
    /// Nothing else differs. The trust store, the TLS, the reaper and the
    /// transfer state machine are the ones that ship; only the thing that
    /// answers "does a human accept this?" is swapped for the object a
    /// desktop UI would attach to.
    pub async fn start_with_approval_provider() -> Self {
        Self::start_inner(false, ApprovalMode::Broker).await
    }

    /// A server bound the way the real daemon binds, so both address
    /// families are exercised where the host has them.
    pub async fn start_dual_stack() -> Self {
        Self::start_inner(true, ApprovalMode::Switch).await
    }

    async fn start_inner(dual_stack: bool, approval_mode: ApprovalMode) -> Self {
        init_crypto();
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        let fingerprint = store.identity().fingerprint();

        let battery = Arc::new(BatteryState::default());

        let downloads = dir.path().join("downloads");
        let approvals = Arc::new(ApprovalSwitch::default());
        let file_approval = match approval_mode {
            ApprovalMode::Switch => None,
            // `false`: never the unattended override. A test that wants that
            // behaviour asks for it explicitly, and no test gets it by
            // default — which is the same rule the daemon follows.
            ApprovalMode::Broker => Some(Arc::new(FileApproval::new(false))),
        };
        let approval: Arc<dyn TransferApproval> = match &file_approval {
            Some(broker) => Arc::clone(broker) as Arc<dyn TransferApproval>,
            None => Arc::clone(&approvals) as Arc<dyn TransferApproval>,
        };
        let transfers = TransferManager::new(
            StreamRole::Acceptor,
            fingerprint,
            FilesConfig {
                destination: Destination::new(downloads.clone()),
                max_file_bytes: 64 * 1024 * 1024,
                // Short, so the timeout paths are exercised for real rather
                // than skipped or faked. The production defaults are minutes;
                // a suite that waited them out would never be run.
                // Short enough that the timeout paths run for real, long
                // enough that a loaded machine does not reap a transfer a
                // test is still setting up. Production defaults are minutes.
                accept_timeout: Duration::from_secs(3),
                stream_open_timeout: Duration::from_millis(700),
            },
            approval,
        );

        // A memory clipboard, never the machine's own: a test suite that
        // overwrote the developer's clipboard would be intolerable, and one
        // that read it could leak into a failure message.
        let clipboard_backend = Arc::new(MemoryBackend::new());
        let clipboard = ClipboardManager::new(
            Arc::clone(&clipboard_backend) as Arc<dyn ClipboardBackend>,
            store.identity().device_id().to_string(),
        );

        // An in-memory notification server, never the session's own: a suite
        // that posted a column of test notifications into the developer's
        // shade on every `cargo test` would be intolerable, and one that read
        // the real lock state would behave differently depending on whether
        // the screen happened to be locked while it ran.
        let notification_sink = Arc::new(MemorySink::new());
        let notification_lock = Arc::new(MemoryLock::new());
        let notifications = NotificationManager::new(
            Arc::clone(&notification_sink) as Arc<dyn NotificationSink>,
            Arc::clone(&notification_lock) as Arc<dyn LockSource>,
        )
        .await;

        let registry = CapabilityRegistry::builder()
            .register(Arc::new(BatteryCapability::new(Arc::clone(&battery))))
            .register(Arc::new(FilesCapability::new(Arc::clone(&transfers))))
            .register(Arc::new(ClipboardCapability::new(Arc::clone(&clipboard))))
            .register(Arc::new(NotificationsCapability::new(Arc::clone(
                &notifications,
            ))))
            .build();

        let tls = pliwee_core::tls::server_config(store.identity()).expect("server config");
        let acceptor = TlsAcceptor::from(tls);

        let mut state_builder = DaemonState::new(store, registry, Arc::clone(&battery))
            .with_transfers(Arc::clone(&transfers))
            .with_clipboard(Arc::clone(&clipboard))
            .with_notifications(Arc::clone(&notifications));
        if let Some(broker) = &file_approval {
            state_builder = state_builder.with_file_approval(Arc::clone(broker));
        }
        let state = Arc::new(state_builder);

        // The real authorizer: the trust store, asked fresh every time. The
        // grant rules under test are the production ones.
        transfers
            .set_authorizer(Arc::clone(&state) as Arc<dyn FilesAuthorizer>)
            .await;
        transfers.spawn_reaper();

        // Same rule for the clipboard: the real trust store answers every
        // grant question, so the tests exercise production authorization.
        notifications
            .set_authorizer(Arc::clone(&state) as Arc<dyn NotificationAuthorizer>)
            .await;
        notifications.spawn_platform_pumps();

        clipboard
            .set_authorizer(Arc::clone(&state) as Arc<dyn ClipboardAuthorizer>)
            .await;

        let (listeners, addr, families) = if dual_stack {
            let bound = pliwee_daemon::listener::bind_endpoints(0).expect("bind");
            let addr = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, bound.port));
            (bound.listeners, addr, bound.families)
        } else {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            (
                vec![listener],
                addr,
                pliwee_daemon::listener::Families {
                    ipv4: true,
                    ipv6: false,
                },
            )
        };

        let accept_state = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = pliwee_daemon::listener::run(listeners, acceptor, accept_state).await;
        });

        Self {
            addr,
            state,
            fingerprint,
            battery,
            families,
            transfers,
            clipboard,
            clipboard_backend,
            notifications,
            notification_sink,
            notification_lock,
            downloads,
            approvals,
            file_approval,
            _dir: dir,
        }
    }

    /// Where this server's identity and trust store live.
    ///
    /// Exposed so the persistence audit can read every byte the daemon
    /// writes, rather than trusting that it wrote the right things.
    pub fn data_dir(&self) -> std::path::PathBuf {
        self._dir.path().to_path_buf()
    }

    /// Sets one peer's clipboard policy through the real store.
    pub async fn set_clipboard_policy(&self, peer: Fingerprint, policy: ClipboardPolicy) {
        {
            let mut store = self.state.store.lock().await;
            store
                .set_clipboard_policy(&peer, policy)
                .expect("persist policy");
        }
        self.state.notify_clipboard_policy_changed();
    }

    /// Sets one peer's notification policy through the real store.
    pub async fn set_notification_policy(&self, peer: Fingerprint, policy: NotificationPolicy) {
        {
            let mut store = self.state.store.lock().await;
            store
                .set_notification_policy(&peer, policy)
                .expect("persist policy");
        }
    }

    /// Grants or withdraws a capability for a peer, through the real store.
    /// Every capability this peer currently holds, sorted.
    ///
    /// Read back from the store rather than remembered, so a test that asserts
    /// "nothing else changed" is asserting it about what was persisted.
    pub async fn granted_capabilities(&self, peer: Fingerprint) -> Vec<String> {
        let store = self.state.store.lock().await;
        let mut out: Vec<String> = store
            .peer_record(&peer)
            .map(|record| {
                record
                    .granted_capabilities
                    .iter()
                    .filter(|(_, granted)| **granted)
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    /// Whether the pairing itself survives. A permission is not a revocation.
    pub async fn is_paired(&self, peer: Fingerprint) -> bool {
        let store = self.state.store.lock().await;
        store.peer_record(&peer).is_some_and(|r| !r.revoked)
    }

    /// This peer's stored clipboard policy, for the isolation assertions.
    /// The notification policy the trust store actually holds.
    ///
    /// Read back from the store rather than from the capability, because the
    /// property under test is what was *persisted* — a setting the daemon
    /// accepted and did not write would look identical from the runtime.
    pub async fn notification_policy(
        &self,
        peer: Fingerprint,
    ) -> pliwee_core::notification_policy::NotificationPolicy {
        let store = self.state.store.lock().await;
        store
            .peer_record(&peer)
            .map(|r| r.notification_policy)
            .unwrap_or_default()
    }

    pub async fn clipboard_policy(&self, peer: Fingerprint) -> ClipboardPolicy {
        let store = self.state.store.lock().await;
        store
            .peer_record(&peer)
            .map(|r| r.clipboard_policy)
            .unwrap_or_default()
    }

    pub async fn set_grant(&self, peer: Fingerprint, capability: &str, granted: bool) {
        let mut store = self.state.store.lock().await;
        store
            .set_capability_grant(&peer, capability, granted)
            .expect("persist grant");
    }

    /// Everything this server has stored, by filename.
    pub fn received_files(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.downloads) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }

    /// Partial files left in the download directory. Must be empty after any
    /// transfer that did not complete.
    pub fn partial_files(&self) -> Vec<String> {
        self.received_files()
            .into_iter()
            .filter(|n| is_partial(n))
            .collect()
    }

    /// Files that were verified and promoted — what a user would actually
    /// see. A transfer still in flight has a `.part` file in the directory,
    /// and that is not a received file.
    pub fn completed_files(&self) -> Vec<String> {
        self.received_files()
            .into_iter()
            .filter(|n| !is_partial(n))
            .collect()
    }

    /// Opens a pairing window with an auto-accepting operator, and returns
    /// the token a client would have read from the QR code.
    pub async fn open_pairing(&self, ttl: Duration) -> PairingToken {
        self.open_pairing_with(ttl, true).await
    }

    /// Opens a pairing window whose operator answers `accept`.
    pub async fn open_pairing_with(&self, ttl: Duration, accept: bool) -> PairingToken {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let token_b32 = self.state.begin_pairing(ttl, tx).await.expect("begin");

        // Stands in for the human at the terminal.
        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                let _ = request.reply.send(accept);
            }
        });

        PairingToken::from_base32(&token_b32).expect("token")
    }
}

// ---------------------------------------------------------------------------
// Client side (the Android phone)
// ---------------------------------------------------------------------------

/// A second device with its own identity, trust store and capability set.
pub struct TestClient {
    pub identity: Arc<LocalIdentity>,
    pub fingerprint: Fingerprint,
    pub host: Arc<dyn SessionHost>,
    pub battery: Arc<BatteryState>,
    /// `files.v1`, in the dialer role: a phone never listens.
    pub transfers: Arc<TransferManager>,
    pub downloads: std::path::PathBuf,
    pub approvals: Arc<ApprovalSwitch>,
    /// This device's own `files.v1` grant for the desktop.
    pub grants: Arc<GrantSwitch>,
    _dir: tempfile::TempDir,
    trusted: Arc<Mutex<Vec<Fingerprint>>>,
    files_armed: Arc<std::sync::atomic::AtomicBool>,
}

struct ClientHost {
    info: v1::DeviceInfo,
    registry: CapabilityRegistry,
    trusted: Arc<Mutex<Vec<Fingerprint>>>,
}

#[async_trait::async_trait]
impl SessionHost for ClientHost {
    fn local_device_info(&self) -> v1::DeviceInfo {
        self.info.clone()
    }
    fn registry(&self) -> CapabilityRegistry {
        self.registry.clone()
    }
    async fn lookup_peer(&self, fingerprint: &Fingerprint) -> PeerStatus {
        if self.trusted.lock().await.contains(fingerprint) {
            PeerStatus::Trusted {
                device_id: "server".into(),
                device_name: "Fedora".into(),
                granted_capabilities: vec![
                    "battery.v1".into(),
                    "files.v1".into(),
                    "clipboard.v1".into(),
                    "notifications.v1".into(),
                ],
            }
        } else {
            PeerStatus::Unknown
        }
    }
    async fn pairing_mode_active(&self) -> bool {
        false
    }
    async fn verify_pairing_proof(
        &self,
        _profile: Profile,
        _initiator: &Fingerprint,
        _nonce: &[u8],
        _proof: &[u8],
    ) -> std::result::Result<[u8; 32], PairingError> {
        // The client never acts as a pairing responder in this Sprint.
        Err(PairingError::NotInPairingMode)
    }
    async fn confirm_pairing(&self, _d: &v1::DeviceInfo, _f: &Fingerprint) -> bool {
        false
    }
    async fn store_peer(
        &self,
        _device: &v1::DeviceInfo,
        fingerprint: &Fingerprint,
        _caps: &[String],
        _version: u32,
    ) -> Result<()> {
        self.trusted.lock().await.push(*fingerprint);
        Ok(())
    }
}

impl TestClient {
    /// A client that speaks `files.v1` by hand: the real handler is replaced
    /// with a capture, so a test can send whatever it likes.
    pub fn new_raw(name: &str) -> (Self, Arc<Captured>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let captured = Arc::new(Captured { rx: Mutex::new(rx) });
        let client = Self::build(name, Some(tx), None);
        (client, captured)
    }

    /// A client that speaks `clipboard.v1` by hand.
    ///
    /// The same idea as [`new_raw`], and needed for the same reason: a
    /// security test has to be able to send what a correct implementation
    /// never would — a duplicate event id, an oversized clip, a malformed
    /// frame — and to read exactly what the desktop replies.
    ///
    /// [`new_raw`]: Self::new_raw
    pub fn new_raw_clipboard(name: &str) -> (Self, Arc<CapturedClipboard>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let captured = Arc::new(CapturedClipboard { rx: Mutex::new(rx) });
        let client = Self::build(name, None, Some(tx));
        (client, captured)
    }

    /// A client that speaks `notifications.v1` by hand.
    ///
    /// The same idea as [`new_raw_clipboard`], and needed for the same reason:
    /// only a hand-written client can send what a correct source never
    /// would — a bad-width identifier, an unpaired `END`, a `DismissRequest`
    /// from a device that sources nothing — and read exactly what the desktop
    /// replies.
    ///
    /// [`new_raw_clipboard`]: Self::new_raw_clipboard
    pub fn new_raw_notifications(name: &str) -> (Self, Arc<CapturedNotifications>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let captured = Arc::new(CapturedNotifications { rx: Mutex::new(rx) });
        let client = Self::build_with(name, None, None, Some(tx));
        (client, captured)
    }

    pub fn new(name: &str) -> Self {
        Self::build(name, None, None)
    }

    fn build(
        name: &str,
        capture: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
        clipboard_capture: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    ) -> Self {
        Self::build_with(name, capture, clipboard_capture, None)
    }

    fn build_with(
        name: &str,
        capture: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
        clipboard_capture: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
        notification_capture: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    ) -> Self {
        init_crypto();
        let identity =
            Arc::new(LocalIdentity::generate(name, v1::Platform::Android).expect("identity"));
        let fingerprint = identity.fingerprint();
        let battery = Arc::new(BatteryState::default());

        let dir = tempfile::tempdir().expect("tempdir");
        let downloads = dir.path().join("downloads");
        let approvals = Arc::new(ApprovalSwitch::default());
        let grants = Arc::new(GrantSwitch::default());
        grants.set(true);

        let transfers = TransferManager::new(
            StreamRole::Dialer,
            fingerprint,
            FilesConfig {
                destination: Destination::new(downloads.clone()),
                max_file_bytes: 64 * 1024 * 1024,
                accept_timeout: Duration::from_secs(3),
                stream_open_timeout: Duration::from_millis(700),
            },
            Arc::clone(&approvals) as Arc<dyn TransferApproval>,
        );

        let mut builder = CapabilityRegistry::builder()
            .register(Arc::new(BatteryCapability::new(Arc::clone(&battery))));
        builder = match capture {
            Some(tx) => builder.register(Arc::new(CapturingCapability {
                id: "files.v1".to_string(),
                tx,
            })),
            None => builder.register(Arc::new(FilesCapability::new(Arc::clone(&transfers)))),
        };
        builder = match clipboard_capture {
            Some(tx) => builder.register(Arc::new(CapturingCapability {
                id: "clipboard.v1".to_string(),
                tx,
            })),
            // A client that is not driving the clipboard by hand still has to
            // *advertise* it, or the capability would never be negotiated and
            // every clipboard test would silently test nothing.
            None => builder.register(Arc::new(CapturingCapability {
                id: "clipboard.v1".to_string(),
                tx: tokio::sync::mpsc::unbounded_channel().0,
            })),
        };
        builder = match notification_capture {
            Some(tx) => builder.register(Arc::new(CapturingCapability {
                id: "notifications.v1".to_string(),
                tx,
            })),
            // A client that is not driving notifications by hand still has to
            // *advertise* the capability, or it would never be negotiated and
            // every notification test would silently test nothing.
            None => builder.register(Arc::new(CapturingCapability {
                id: "notifications.v1".to_string(),
                tx: tokio::sync::mpsc::unbounded_channel().0,
            })),
        };
        let registry = builder.build();
        let trusted = Arc::new(Mutex::new(Vec::new()));

        let host: Arc<dyn SessionHost> = Arc::new(ClientHost {
            info: identity.device_info(),
            registry,
            trusted: Arc::clone(&trusted),
        });

        Self {
            identity,
            fingerprint,
            host,
            battery,
            transfers,
            downloads,
            approvals,
            grants,
            _dir: dir,
            trusted,
            files_armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Finishes wiring `files.v1` now that the server's address is known.
    ///
    /// Called by [`connect`]; also callable directly by a test that drives
    /// the data stream by hand.
    ///
    /// [`connect`]: Self::connect
    pub async fn arm_files(&self, addr: SocketAddr, pinned: Fingerprint) {
        // Idempotent: a client that reconnects (which the grant flow
        // requires) must not end up with two reapers.
        if self
            .files_armed
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        self.transfers
            .set_authorizer(Arc::clone(&self.grants) as Arc<dyn FilesAuthorizer>)
            .await;
        self.transfers
            .set_dialer(Arc::new(TestDialer {
                addr,
                identity: Arc::clone(&self.identity),
                pinned,
            }))
            .await;
        self.transfers.spawn_reaper();
    }

    /// Everything this device has stored, by filename.
    pub fn received_files(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.downloads) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }

    /// Marks a server as already-trusted, simulating a persisted pairing.
    pub async fn trust(&self, fingerprint: Fingerprint) {
        self.trusted.lock().await.push(fingerprint);
    }

    pub async fn is_trusted(&self, fingerprint: &Fingerprint) -> bool {
        self.trusted.lock().await.contains(fingerprint)
    }

    /// Opens a real TLS connection with the given pinned server identity,
    /// under this run's [`test_profile`].
    pub async fn tls_connect(
        &self,
        addr: SocketAddr,
        pinned: Fingerprint,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
        self.tls_connect_with_profile(addr, pinned, test_profile())
            .await
    }

    /// Opens a real TLS connection offering exactly `profile`'s control ALPN.
    pub async fn tls_connect_with_profile(
        &self,
        addr: SocketAddr,
        pinned: Fingerprint,
        profile: Profile,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
        let config = pliwee_core::tls::client_config(&self.identity, pinned, profile)?;
        let connector = TlsConnector::from(config);
        let tcp = TcpStream::connect(addr).await?;
        // The name is irrelevant: our verifier pins the key and ignores it.
        let name = rustls_pki_types::ServerName::try_from("pliwee.invalid").expect("static name");
        let tls = connector.connect(name, tcp).await?;
        // As every client does: the server selected exactly the ALPN offered.
        pliwee_core::tls::require_negotiated(
            tls.get_ref().1,
            profile,
            pliwee_core::tls::ConnectionKind::Control,
        )?;
        Ok(tls)
    }

    /// Full connect + handshake, under this run's [`test_profile`]. `token`
    /// triggers the pairing exchange.
    pub async fn connect(
        &self,
        addr: SocketAddr,
        pinned: Fingerprint,
        token: Option<&PairingToken>,
    ) -> Result<ConnectedSession> {
        self.connect_with_profile(addr, pinned, token, test_profile())
            .await
    }

    /// Full connect + handshake under an explicit profile.
    pub async fn connect_with_profile(
        &self,
        addr: SocketAddr,
        pinned: Fingerprint,
        token: Option<&PairingToken>,
        profile: Profile,
    ) -> Result<ConnectedSession> {
        self.arm_files(addr, pinned).await;
        let mut tls = self.tls_connect_with_profile(addr, pinned, profile).await?;
        match session::connect_handshake(&mut tls, &self.host, pinned, profile, token).await? {
            ClientHandshake::Established(established, state) => {
                let host = Arc::clone(&self.host);
                let capabilities = established.negotiated_capabilities.clone();
                let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

                let notifier = Arc::new(HandleNotifier {
                    inner: Arc::clone(&self.host),
                    tx: Mutex::new(Some(ready_tx)),
                });
                let host_for_run: Arc<dyn SessionHost> = notifier;
                let _ = host;

                let task = tokio::spawn(async move {
                    session::run_session(tls, host_for_run, established, state).await
                });

                let handle = ready_rx
                    .await
                    .map_err(|_| pliwee_core::Error::Protocol("session ended before it started"))?;
                // The run is evidence of the profile it names only if the
                // session really negotiated it.
                assert_eq!(
                    handle.profile(),
                    profile,
                    "the session did not run under the requested profile"
                );

                Ok(ConnectedSession {
                    handle,
                    task,
                    negotiated_capabilities: capabilities,
                })
            }
            ClientHandshake::PairingRequired => {
                Err(pliwee_core::Error::Pairing(PairingError::NotInPairingMode))
            }
        }
    }
}

/// Wraps a host so the test can capture the `SessionHandle`.
struct HandleNotifier {
    inner: Arc<dyn SessionHost>,
    tx: Mutex<Option<tokio::sync::oneshot::Sender<SessionHandle>>>,
}

#[async_trait::async_trait]
impl SessionHost for HandleNotifier {
    fn local_device_info(&self) -> v1::DeviceInfo {
        self.inner.local_device_info()
    }
    fn registry(&self) -> CapabilityRegistry {
        self.inner.registry()
    }
    async fn lookup_peer(&self, f: &Fingerprint) -> PeerStatus {
        self.inner.lookup_peer(f).await
    }
    async fn pairing_mode_active(&self) -> bool {
        self.inner.pairing_mode_active().await
    }
    async fn verify_pairing_proof(
        &self,
        profile: Profile,
        i: &Fingerprint,
        n: &[u8],
        p: &[u8],
    ) -> std::result::Result<[u8; 32], PairingError> {
        self.inner.verify_pairing_proof(profile, i, n, p).await
    }
    async fn confirm_pairing(&self, d: &v1::DeviceInfo, f: &Fingerprint) -> bool {
        self.inner.confirm_pairing(d, f).await
    }
    async fn store_peer(
        &self,
        d: &v1::DeviceInfo,
        f: &Fingerprint,
        c: &[String],
        v: u32,
    ) -> Result<()> {
        self.inner.store_peer(d, f, c, v).await
    }
    async fn on_established(&self, peer: &Fingerprint, handle: SessionHandle) {
        if let Some(tx) = self.tx.lock().await.take() {
            let _ = tx.send(handle.clone());
        }
        self.inner.on_established(peer, handle).await;
    }
    async fn on_closed(&self, peer: &Fingerprint, session_id: SessionId) {
        self.inner.on_closed(peer, session_id).await;
    }
}

impl std::fmt::Debug for ConnectedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedSession")
            .field("peer", &self.handle.peer())
            .field("capabilities", &self.negotiated_capabilities)
            .finish()
    }
}

pub struct ConnectedSession {
    pub handle: SessionHandle,
    pub task: tokio::task::JoinHandle<Result<()>>,
    pub negotiated_capabilities: Vec<String>,
}

impl ConnectedSession {
    pub async fn close(self) {
        self.handle.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

/// Polls until `f` returns true, or panics after `timeout`.
pub async fn wait_until<F, Fut>(timeout: Duration, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if f().await {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("condition not met within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---------------------------------------------------------------------------
// files.v1 harness
// ---------------------------------------------------------------------------
//
// Everything below drives the *real* capability over the *real* transport:
// a real TLS data stream, real pinning, the real MAC. A test that expects a
// refusal gets it from the code that runs in production, never from a stub.

use pliwee_capability_clipboard::backend::{ClipboardBackend, MemoryBackend};
use pliwee_capability_clipboard::{
    ClipboardAuthorizer, ClipboardCapability, ClipboardManager, ClipboardPolicy,
};
use pliwee_capability_files::transfer::{FailureReason, TransferId, TransferState};
use pliwee_capability_files::{
    DataStreamDialer, DataStreamIo, Destination, FilesAuthorizer, FilesCapability, FilesConfig,
    IncomingOffer, StreamRole, TransferApproval, TransferManager, TransferSnapshot,
};
use pliwee_capability_notifications::backend::{
    LockSource, MemoryLock, MemorySink, NotificationSink,
};
use pliwee_capability_notifications::{
    NotificationAuthorizer, NotificationManager, NotificationPolicy, NotificationsCapability,
};
use pliwee_daemon::approval::FileApproval;

fn is_partial(name: &str) -> bool {
    name.starts_with(".pliwee-") || name.ends_with(".part")
}

/// A `TransferApproval` a test can steer.
///
/// Also counts how many times it was asked, which is how "the peer was
/// refused before anyone was bothered" is asserted: a prompt that never
/// appeared is the difference between a check that ran early and one that ran
/// too late.
pub struct ApprovalSwitch {
    accept: std::sync::atomic::AtomicBool,
    /// When set, the approval never answers. Stands in for a human who walked
    /// away, so the accept timeout can be exercised.
    stall: std::sync::atomic::AtomicBool,
    asked: std::sync::atomic::AtomicUsize,
    /// When set, the approval waits for [`ApprovalSwitch::release`] and then
    /// answers. Stands in for a human who answers *late*, after something
    /// else has already ended the transfer.
    hold: std::sync::atomic::AtomicBool,
    released: tokio::sync::Notify,
    holding: std::sync::atomic::AtomicUsize,
    answered_after_hold: std::sync::atomic::AtomicUsize,
}

impl Default for ApprovalSwitch {
    fn default() -> Self {
        Self {
            accept: std::sync::atomic::AtomicBool::new(true),
            stall: std::sync::atomic::AtomicBool::new(false),
            asked: std::sync::atomic::AtomicUsize::new(0),
            hold: std::sync::atomic::AtomicBool::new(false),
            released: tokio::sync::Notify::new(),
            holding: std::sync::atomic::AtomicUsize::new(0),
            answered_after_hold: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl ApprovalSwitch {
    pub fn set_accept(&self, accept: bool) {
        self.accept
            .store(accept, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_stall(&self, stall: bool) {
        self.stall
            .store(stall, std::sync::atomic::Ordering::Relaxed);
    }

    /// How many times a human was asked.
    pub fn asked(&self) -> usize {
        self.asked.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_hold(&self, hold: bool) {
        self.hold.store(hold, std::sync::atomic::Ordering::Relaxed);
    }

    /// Lets one held question answer.
    pub fn release(&self) {
        self.released.notify_one();
    }

    /// How many questions are waiting on [`ApprovalSwitch::release`] now.
    pub fn holding(&self) -> usize {
        self.holding.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many held questions have since been answered: the evidence that
    /// a late answer was actually handed back, not merely released.
    pub fn answered_after_hold(&self) -> usize {
        self.answered_after_hold
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait::async_trait]
impl TransferApproval for ApprovalSwitch {
    async fn confirm_receive(&self, _offer: &IncomingOffer) -> bool {
        self.asked
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.stall.load(std::sync::atomic::Ordering::Relaxed) {
            std::future::pending::<()>().await;
        }
        if self.hold.load(std::sync::atomic::Ordering::Relaxed) {
            self.holding
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.released.notified().await;
            self.holding
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            self.answered_after_hold
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.accept.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// A `files.v1` grant the test controls directly.
///
/// Used for the phone side only. The desktop's authorizer is the real
/// `DaemonState`, reading the real trust store, because the desktop is where
/// the grant rules under test actually live.
#[derive(Default)]
pub struct GrantSwitch {
    granted: std::sync::atomic::AtomicBool,
}

impl GrantSwitch {
    pub fn set(&self, granted: bool) {
        self.granted
            .store(granted, std::sync::atomic::Ordering::Relaxed);
    }
}

#[async_trait::async_trait]
impl FilesAuthorizer for GrantSwitch {
    async fn is_authorized(&self, _peer: &Fingerprint) -> bool {
        self.granted.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Dials data streams at a fixed address with a fixed pinned identity —
/// exactly what the phone does, using the same `data_stream_client_config`.
pub struct TestDialer {
    pub addr: SocketAddr,
    pub identity: Arc<LocalIdentity>,
    pub pinned: Fingerprint,
}

#[async_trait::async_trait]
impl DataStreamDialer for TestDialer {
    async fn dial(&self, _peer: &Fingerprint, profile: Profile) -> Result<Box<dyn DataStreamIo>> {
        let stream =
            open_data_stream_with_profile(self.addr, &self.identity, self.pinned, profile).await?;
        Ok(Box::new(stream))
    }
}

/// Opens a raw, authenticated data-stream connection.
///
/// Public so a test can play a hostile dialer: complete a genuine TLS
/// handshake with a real identity and then send whatever it likes as the
/// first frame.
///
/// Offers this run's [`test_profile`] data ALPN, which is the profile every
/// control session in the run negotiated.
pub async fn open_data_stream(
    addr: SocketAddr,
    identity: &LocalIdentity,
    pinned: Fingerprint,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    open_data_stream_with_profile(addr, identity, pinned, test_profile()).await
}

/// Opens a raw data-stream connection offering exactly `profile`'s data ALPN.
pub async fn open_data_stream_with_profile(
    addr: SocketAddr,
    identity: &LocalIdentity,
    pinned: Fingerprint,
    profile: Profile,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let config = pliwee_core::tls::data_stream_client_config(identity, pinned, profile)?;
    let connector = TlsConnector::from(config);
    let tcp = TcpStream::connect(addr).await?;
    let name = rustls_pki_types::ServerName::try_from("pliwee.invalid").expect("static name");
    let tls = connector.connect(name, tcp).await?;
    pliwee_core::tls::require_negotiated(
        tls.get_ref().1,
        profile,
        pliwee_core::tls::ConnectionKind::Data,
    )?;
    Ok(tls)
}

/// Waits for a transfer to reach a terminal state and returns its snapshot.
pub async fn wait_for_terminal(
    manager: &Arc<TransferManager>,
    id: TransferId,
    timeout: Duration,
) -> TransferSnapshot {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(snapshot) = manager.snapshot_one(id).await {
            if snapshot.state.is_terminal() {
                return snapshot;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            let state = manager
                .snapshot_one(id)
                .await
                .map(|s| s.state.to_string())
                .unwrap_or_else(|| "gone".into());
            panic!("transfer {id} did not settle within {timeout:?} (state: {state})");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits for a transfer to reach a particular state.
pub async fn wait_for_state(
    manager: &Arc<TransferManager>,
    id: TransferId,
    want: TransferState,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if manager
            .snapshot_one(id)
            .await
            .is_some_and(|s| s.state == want)
        {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            let state = manager
                .snapshot_one(id)
                .await
                .map(|s| s.state.to_string())
                .unwrap_or_else(|| "gone".into());
            panic!("transfer {id} never reached {want} within {timeout:?} (state: {state})");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Asserts a transfer failed for exactly this reason.
pub fn assert_failed_with(snapshot: &TransferSnapshot, reason: FailureReason) {
    assert!(
        matches!(
            snapshot.state,
            TransferState::Failed | TransferState::Cancelled
        ),
        "expected a failure, got {}",
        snapshot.state
    );
    assert_eq!(snapshot.failure, Some(reason), "wrong failure reason");
}

/// A capability that records every payload instead of acting on it.
///
/// Registered in place of the real `files.v1` handler on a "raw" client, so a
/// test can speak the protocol by hand: send a hostile offer, read what the
/// desktop replies, and drive the data stream itself. Without this, every
/// test would be limited to what the well-behaved implementation is willing
/// to send — which is precisely not what a security test wants.
pub struct CapturingCapability {
    id: String,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

#[async_trait::async_trait]
impl pliwee_core::capability::Capability for CapturingCapability {
    fn id(&self) -> &str {
        &self.id
    }

    async fn on_message(
        &self,
        _ctx: &pliwee_core::capability::CapabilityContext,
        payload: &[u8],
    ) -> Result<()> {
        let _ = self.tx.send(payload.to_vec());
        Ok(())
    }
}

/// The raw clipboard side of a client: what the desktop said, undigested.
pub struct CapturedClipboard {
    rx: Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl CapturedClipboard {
    /// Waits for the next `clipboard.v1` message from the desktop.
    pub async fn next_control(&self, timeout: Duration) -> clip_pb::ClipboardControl {
        let mut rx = self.rx.lock().await;
        let payload = tokio::time::timeout(timeout, rx.recv())
            .await
            .expect("the desktop said nothing in time")
            .expect("the capture channel closed");
        <clip_pb::ClipboardControl as prost::Message>::decode(&payload[..])
            .expect("decodable ClipboardControl")
    }

    /// The next message, as a result, failing if it is anything else.
    pub async fn next_result(&self, timeout: Duration) -> clip_pb::ClipboardResult {
        match self.next_control(timeout).await.body {
            Some(clip_pb::clipboard_control::Body::Result(r)) => r,
            other => panic!("expected a ClipboardResult, got {other:?}"),
        }
    }

    /// Everything received so far, without waiting.
    pub async fn drain(&self) -> Vec<clip_pb::ClipboardControl> {
        let mut rx = self.rx.lock().await;
        let mut out = Vec::new();
        while let Ok(payload) = rx.try_recv() {
            out.push(
                <clip_pb::ClipboardControl as prost::Message>::decode(&payload[..])
                    .expect("decodable ClipboardControl"),
            );
        }
        out
    }

    /// Asserts nothing arrived within `window`.
    ///
    /// A negative assertion needs a real wait: checking immediately would
    /// pass even when a message was one scheduler tick away.
    pub async fn expect_silence(&self, window: Duration) {
        tokio::time::sleep(window).await;
        let messages = self.drain().await;
        assert!(
            messages.is_empty(),
            "expected silence, got {} message(s): {messages:?}",
            messages.len()
        );
    }
}

/// The raw side of a client: what the desktop said, undigested.
pub struct Captured {
    rx: Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl Captured {
    /// Waits for the next `files.v1` message from the desktop.
    pub async fn next_control(&self, timeout: Duration) -> pb::FileControl {
        let mut rx = self.rx.lock().await;
        let payload = tokio::time::timeout(timeout, rx.recv())
            .await
            .expect("the desktop said nothing in time")
            .expect("the capture channel closed");
        <pb::FileControl as prost::Message>::decode(&payload[..]).expect("decodable FileControl")
    }

    /// Discards everything received so far.
    ///
    /// Needed after a test deliberately provokes a burst of replies, so the
    /// next assertion reads a fresh message rather than the backlog.
    pub async fn drain(&self) {
        let mut rx = self.rx.lock().await;
        while rx.try_recv().is_ok() {}
    }

    /// Waits for the next message, returning `None` if the desktop stays
    /// quiet. Used to assert that something did *not* happen.
    pub async fn next_control_or_silence(&self, timeout: Duration) -> Option<pb::FileControl> {
        let mut rx = self.rx.lock().await;
        let payload = tokio::time::timeout(timeout, rx.recv()).await.ok()??;
        <pb::FileControl as prost::Message>::decode(&payload[..]).ok()
    }
}

pub use pliwee_proto::v1::capabilities as pb;
/// The same module, under a name the clipboard helpers read better with.
pub use pliwee_proto::v1::capabilities as clip_pb;

/// The raw notification side of a client: what the desktop said, undigested.
pub struct CapturedNotifications {
    rx: Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl CapturedNotifications {
    /// Waits for the next `notifications.v1` message from the desktop.
    pub async fn next_control(&self, timeout: Duration) -> clip_pb::NotificationControl {
        let payload = tokio::time::timeout(timeout, async {
            let mut rx = self.rx.lock().await;
            rx.recv().await
        })
        .await
        .expect("the desktop answered within the timeout")
        .expect("the capture channel is open");
        <clip_pb::NotificationControl as prost::Message>::decode(&payload[..])
            .expect("decodable NotificationControl")
    }

    /// The next `NotificationResult`.
    pub async fn next_result(&self, timeout: Duration) -> clip_pb::NotificationResult {
        match self.next_control(timeout).await.body {
            Some(clip_pb::notification_control::Body::Result(r)) => r,
            other => panic!("expected a NotificationResult, got {other:?}"),
        }
    }

    /// The next `DismissRequest` the desktop sends.
    ///
    /// The only message that travels desktop → phone and asks the phone to do
    /// something, so a test that expects one says so by name: a helper that
    /// accepted any body would let a missing dismissal look like a pass.
    pub async fn next_dismiss(&self, timeout: Duration) -> clip_pb::DismissRequest {
        match self.next_control(timeout).await.body {
            Some(clip_pb::notification_control::Body::Dismiss(d)) => d,
            other => panic!("expected a DismissRequest, got {other:?}"),
        }
    }

    /// The next `NotificationRoles` announcement.
    pub async fn next_roles(&self, timeout: Duration) -> clip_pb::NotificationRoles {
        match self.next_control(timeout).await.body {
            Some(clip_pb::notification_control::Body::Roles(r)) => r,
            other => panic!("expected a NotificationRoles, got {other:?}"),
        }
    }

    /// Everything queued so far, without waiting.
    pub async fn drain(&self) -> Vec<clip_pb::NotificationControl> {
        let mut rx = self.rx.lock().await;
        let mut out = Vec::new();
        while let Ok(payload) = rx.try_recv() {
            out.push(
                <clip_pb::NotificationControl as prost::Message>::decode(&payload[..])
                    .expect("decodable NotificationControl"),
            );
        }
        out
    }
}

/// Sends one `notifications.v1` control message over a live session.
pub async fn send_notification_control(
    session: &ConnectedSession,
    body: clip_pb::notification_control::Body,
) -> bool {
    let payload = <clip_pb::NotificationControl as prost::Message>::encode_to_vec(
        &clip_pb::NotificationControl { body: Some(body) },
    );
    session
        .handle
        .send_capability(pliwee_core::capability::OutboundMessage {
            capability_id: "notifications.v1".to_string(),
            payload,
        })
        .await
}

/// Sends one `files.v1` control message over a live session.
pub async fn send_files_control(session: &ConnectedSession, body: pb::file_control::Body) -> bool {
    let payload =
        <pb::FileControl as prost::Message>::encode_to_vec(&pb::FileControl { body: Some(body) });
    session
        .handle
        .send_capability(pliwee_core::capability::OutboundMessage {
            capability_id: "files.v1".to_string(),
            payload,
        })
        .await
}

/// Writes a file of `size` pseudo-random-but-deterministic bytes.
///
/// Deterministic so a hash mismatch in a test is reproducible, and not all
/// zeros so a truncation or an off-by-one is actually visible in the digest.
pub fn write_sample_file(path: &std::path::Path, size: usize) -> Vec<u8> {
    let bytes = write_sample_bytes(size);
    std::fs::write(path, &bytes).expect("write sample");
    bytes
}

/// The same deterministic bytes, without touching the filesystem.
pub fn write_sample_bytes(size: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size);
    let mut x: u32 = 0x9e37_79b9;
    for _ in 0..size {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        bytes.push((x >> 24) as u8);
    }
    bytes
}

pub fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

// ---------------------------------------------------------------------------
// notifications.v1 fixtures.
//
// They live here rather than in `notifications.rs` because two test binaries
// need them: that one, and `notification_log_privacy.rs`. The second exists
// because a `tracing` capture cannot safely share a process with tests that do
// not install a subscriber — see its header for the mechanism.
// ---------------------------------------------------------------------------

pub const ORIGIN: &str = "0123456789abcdef0123456789abcdef";
pub fn id_bytes(seed: u16) -> Vec<u8> {
    let mut out = vec![0u8; 16];
    out[0] = (seed >> 8) as u8;
    out[1] = (seed & 0xff) as u8;
    out
}
pub fn content_hash(seed: u16, body: &str) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[0] = (seed & 0xff) as u8;
    for (index, byte) in body.bytes().enumerate() {
        out[1 + index % 31] ^= byte;
    }
    out
}
pub fn upsert(seed: u16, title: &str, body: &str) -> clip_pb::NotificationUpsert {
    clip_pb::NotificationUpsert {
        notification_id: id_bytes(seed),
        origin_device_id: ORIGIN.to_string(),
        app_id: "com.example.chat".to_string(),
        app_label: "Chat".to_string(),
        title: title.to_string(),
        body: body.to_string(),
        posted_at_unix_ms: 1_700_000_000_000,
        importance: clip_pb::NotificationImportance::Normal as i32,
        privacy: clip_pb::NotificationPrivacy::Private as i32,
        category: clip_pb::NotificationCategory::Message as i32,
        content_hash: content_hash(seed, body),
        ..clip_pb::NotificationUpsert::default()
    }
}
pub fn roles_body(
    roles: &[clip_pb::NotificationRole],
    epoch: u32,
) -> clip_pb::notification_control::Body {
    clip_pb::notification_control::Body::Roles(clip_pb::NotificationRoles {
        roles: roles.iter().map(|r| *r as i32).collect(),
        epoch,
    })
}

/// What the **desktop** negotiated for this peer's live session.
///
/// Deliberately not `ConnectedSession::negotiated_capabilities`, which is the
/// dialling side's view and is the plain intersection of the two advertised
/// sets. The grant filter lives on the answering side — it is the desktop that
/// decides what this peer is allowed to do — so the desktop's vector is the
/// one every test here is about. Reading the client's instead would have made
/// the whole suite pass against no fix at all.
/// How long the fixtures below wait for the daemon to catch up.
///
/// Private to this module: each test binary keeps its own `TIMEOUT` for its
/// own assertions, and a `pub` one here would silently shadow-compete with
/// them through `use common::*`.
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn desktop_negotiated(
    server: &TestServer,
    peer: pliwee_core::Fingerprint,
) -> Vec<String> {
    let deadline = std::time::Instant::now() + FIXTURE_TIMEOUT;
    loop {
        if let Some(handle) = server.state.session_for(&peer).await {
            return handle.negotiated_capabilities().to_vec();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the desktop never registered a session for this peer"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
