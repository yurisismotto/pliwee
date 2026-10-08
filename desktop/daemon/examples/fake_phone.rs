//! A stand-in for the Android app, for demonstrating and manually testing the
//! daemon end to end without an Android device.
//!
//! It speaks the real protocol over real TLS with real pinning: it is the
//! same code path a phone takes, driven from a terminal.
//!
//! ```bash
//! # terminal 1
//! pliweed
//! # terminal 2
//! pliwee pair                       # copy the payload it prints
//! # terminal 3
//! cargo run -p pliwee-daemon --example fake_phone -- pair '<payload>'
//! cargo run -p pliwee-daemon --example fake_phone -- connect
//!
//! # files.v1: send a file to the desktop, or sit and receive one
//! pliwee grant <device> files.v1
//! cargo run -p pliwee-daemon --example fake_phone -- send ~/photo.jpg
//! cargo run -p pliwee-daemon --example fake_phone -- receive
//! ```
//!
//! `--profile pliwee|omnibridge` picks the wire identity profile (ADR-0020
//! §D4) for `connect`, `send` and `receive`: the one ALPN offered, and so the
//! pairing and data-stream domains. The default is `pliwee`. For `pair` the
//! scanned payload's scheme fixes the profile (`pliwee1:` or `omnibridge1:`),
//! and a `--profile` that disagrees with it is refused rather than obeyed.
//!
//! For `files.v1` it plays the **dialer**, exactly as a phone does: it opens
//! the data stream in both directions of transfer and proves the challenge
//! the desktop issued.
//!
//! Its identity is stored under `--data-dir` (default:
//! `/tmp/omnibridge-fake-phone`) so that "reconnect without pairing again" can
//! actually be demonstrated across runs.

use std::sync::Arc;
use std::time::Duration;

use pliwee_capability_battery::{BatteryCapability, BatteryReading, BatteryState};
use pliwee_capability_files::{
    DataStreamDialer, DataStreamIo, Destination, FilesAuthorizer, FilesCapability, FilesConfig,
    IncomingOffer, StreamRole, TransferApproval, TransferManager,
};
use pliwee_core::capability::CapabilityRegistry;
use pliwee_core::error::{PairingError, Result};
use pliwee_core::qr::QrPayload;
use pliwee_core::session::{self, ClientHandshake, PeerStatus, SessionHandle, SessionHost};
use pliwee_core::store::Store;
use pliwee_core::tls::ConnectionKind;
use pliwee_core::{Fingerprint, Profile};
use pliwee_proto::v1;
use pliwee_proto::v1::capabilities::ChargingState;
use tokio::sync::Mutex;
use tokio_rustls::TlsConnector;

/// Minimal host: a trust store on disk plus a capability registry.
struct PhoneHost {
    info: v1::DeviceInfo,
    registry: CapabilityRegistry,
    store: Mutex<Store>,
}

impl PhoneHost {
    /// The TLS client config a data stream dials with.
    ///
    /// Built from the same identity and the same pinned fingerprint as the
    /// control session; only the ALPN differs, and it is the data ALPN of the
    /// transfer's own profile.
    async fn data_stream_config(
        &self,
        pinned: Fingerprint,
        profile: Profile,
    ) -> Result<Arc<rustls::ClientConfig>> {
        let store = self.store.lock().await;
        pliwee_core::tls::data_stream_client_config(store.identity(), pinned, profile)
    }
}

/// Opens data streams to the desktop. A phone always dials.
struct PhoneDialer {
    address: std::net::SocketAddr,
    pinned: Fingerprint,
    host: Arc<PhoneHost>,
}

#[async_trait::async_trait]
impl DataStreamDialer for PhoneDialer {
    async fn dial(&self, _peer: &Fingerprint, profile: Profile) -> Result<Box<dyn DataStreamIo>> {
        let config = self.host.data_stream_config(self.pinned, profile).await?;
        let connector = TlsConnector::from(config);
        let tcp = tokio::net::TcpStream::connect(self.address)
            .await
            .map_err(pliwee_core::Error::Io)?;
        let _ = tcp.set_nodelay(true);
        let name = rustls_pki_types::ServerName::try_from("pliwee.invalid")
            .map_err(|_| pliwee_core::Error::Protocol("bad static server name"))?;
        let tls = connector
            .connect(name, tcp)
            .await
            .map_err(pliwee_core::Error::Io)?;
        // The desktop must have selected exactly the data ALPN offered.
        pliwee_core::tls::require_negotiated(tls.get_ref().1, profile, ConnectionKind::Data)?;
        Ok(Box::new(tls))
    }
}

/// A demo tool has no human to ask, so it accepts. The real phone shows a
/// prompt; the desktop daemon declines unless started with an explicit flag.
struct AcceptEverything;

#[async_trait::async_trait]
impl TransferApproval for AcceptEverything {
    async fn confirm_receive(&self, offer: &IncomingOffer) -> bool {
        println!(
            "accepting {} ({} bytes) from {}",
            offer.filename,
            offer.size_bytes,
            offer.peer.to_display_short()
        );
        true
    }
}

struct AlwaysAuthorized;

#[async_trait::async_trait]
impl FilesAuthorizer for AlwaysAuthorized {
    async fn is_authorized(&self, _peer: &Fingerprint) -> bool {
        true
    }
}

/// Connects, then either offers a file or waits to be offered one, printing
/// progress until every transfer has settled.
async fn run_files(
    host: Arc<PhoneHost>,
    transfers: Arc<TransferManager>,
    address: std::net::SocketAddr,
    pinned: Fingerprint,
    profile: Profile,
    to_send: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let client_config = {
        let store = host.store.lock().await;
        pliwee_core::tls::client_config(store.identity(), pinned, profile)?
    };

    let connector = TlsConnector::from(client_config);
    let tcp = tokio::net::TcpStream::connect(address).await?;
    tcp.set_nodelay(true)?;
    let name = rustls_pki_types::ServerName::try_from("pliwee.invalid")?;
    let mut tls = connector.connect(name, tcp).await?;
    pliwee_core::tls::require_negotiated(tls.get_ref().1, profile, ConnectionKind::Control)?;
    println!("TLS established under profile {profile}");

    let session_host: Arc<dyn SessionHost> = host.clone();
    let handshake =
        session::connect_handshake(&mut tls, &session_host, pinned, profile, None).await?;
    let (established, state) = match handshake {
        ClientHandshake::Established(e, s) => (e, s),
        ClientHandshake::PairingRequired => {
            anyhow::bail!("the daemon does not know this device; run `pair` first")
        }
    };

    println!(
        "session established with {}, capabilities: {:?}",
        established.peer.to_display_short(),
        established.negotiated_capabilities
    );
    if !established
        .negotiated_capabilities
        .iter()
        .any(|c| c == "files.v1")
    {
        anyhow::bail!(
            "files.v1 was not granted. Run: pliwee grant <device> files.v1, \
             then reconnect."
        );
    }

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let notifier: Arc<dyn SessionHost> = Arc::new(Notifier {
        inner: session_host,
        tx: Mutex::new(Some(ready_tx)),
    });

    let task =
        tokio::spawn(async move { session::run_session(tls, notifier, established, state).await });
    let handle = ready_rx.await?;

    // Progress, printed as it happens.
    let mut events = transfers.subscribe();
    let printer = tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let snapshot = event.0;
            match snapshot.percentage() {
                Some(pct) => println!(
                    "  {} {} {}% ({}/{}) [{}]",
                    snapshot.id,
                    snapshot.filename,
                    pct,
                    snapshot.bytes_transferred,
                    snapshot.size_bytes,
                    snapshot.state
                ),
                None => println!(
                    "  {} {} [{}]",
                    snapshot.id, snapshot.filename, snapshot.state
                ),
            }
            if let Some(path) = snapshot.stored_at {
                println!("  stored at {}", path.display());
            }
        }
    });

    if let Some(path) = to_send {
        let id = transfers
            .offer_file(established_peer(&handle), path)
            .await?;
        println!("offered transfer {id}");
    }

    // Wait until nothing is in flight any more, or the hold window expires.
    let hold = Duration::from_secs(
        std::env::var("FAKE_PHONE_HOLD_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
    );
    let deadline = tokio::time::Instant::now() + hold;
    loop {
        let snapshot = transfers.snapshot().await;
        let settled = !snapshot.is_empty() && snapshot.iter().all(|t| t.state.is_terminal());
        if settled || tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    for snapshot in transfers.snapshot().await {
        println!(
            "final: {} {} {} {}",
            snapshot.id,
            snapshot.filename,
            snapshot.state,
            snapshot
                .failure
                .map(|f| f.to_string())
                .unwrap_or_else(|| "-".into())
        );
    }

    printer.abort();
    handle.shutdown().await;
    let _ = task.await;
    Ok(())
}

fn established_peer(handle: &SessionHandle) -> Fingerprint {
    handle.peer()
}

#[async_trait::async_trait]
impl SessionHost for PhoneHost {
    fn local_device_info(&self) -> v1::DeviceInfo {
        self.info.clone()
    }
    fn registry(&self) -> CapabilityRegistry {
        self.registry.clone()
    }
    async fn lookup_peer(&self, fingerprint: &Fingerprint) -> PeerStatus {
        match self.store.lock().await.trusted_peer(fingerprint) {
            Some(p) => PeerStatus::Trusted {
                device_id: p.device_id.clone(),
                device_name: p.device_name.clone(),
                granted_capabilities: vec!["battery.v1".into()],
            },
            None => PeerStatus::Unknown,
        }
    }
    async fn pairing_mode_active(&self) -> bool {
        false
    }
    async fn verify_pairing_proof(
        &self,
        _profile: Profile,
        _i: &Fingerprint,
        _n: &[u8],
        _p: &[u8],
    ) -> std::result::Result<[u8; 32], PairingError> {
        Err(PairingError::NotInPairingMode)
    }
    async fn confirm_pairing(&self, _d: &v1::DeviceInfo, _f: &Fingerprint) -> bool {
        false
    }
    async fn store_peer(
        &self,
        device: &v1::DeviceInfo,
        fingerprint: &Fingerprint,
        capabilities: &[String],
        version: u32,
    ) -> Result<()> {
        let mut store = self.store.lock().await;
        store.add_peer(pliwee_core::store::TrustedPeer {
            device_id: device.device_id.clone(),
            device_name: device.device_name.clone(),
            platform: device.platform,
            fingerprint: *fingerprint,
            paired_at_unix: 0,
            granted_capabilities: capabilities.iter().map(|c| (c.clone(), true)).collect(),
            last_protocol_version: version,
            revoked: false,
            hidden: false,
            clipboard_policy: Default::default(),
            notification_policy: Default::default(),
            address_hints: Vec::new(),
        })
    }
    async fn on_established(&self, _peer: &Fingerprint, _handle: SessionHandle) {}
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    // `--profile <name>` may appear anywhere; it is taken out so the
    // positional arguments keep their places. An unknown name is an error,
    // never a silent default.
    let requested_profile = match args.iter().position(|a| a == "--profile") {
        Some(at) => {
            let name = args
                .get(at + 1)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("--profile needs pliwee or omnibridge"))?;
            args.drain(at..=at + 1);
            Some(Profile::from_name(&name).ok_or_else(|| {
                anyhow::anyhow!("unknown profile {name:?}; use pliwee or omnibridge")
            })?)
        }
        None => None,
    };
    let profile = requested_profile.unwrap_or(Profile::Pliwee);
    let command = args.get(1).map(String::as_str).unwrap_or("help");

    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("crypto provider already installed"))?;

    let data_dir = std::env::var("FAKE_PHONE_DIR")
        .unwrap_or_else(|_| "/tmp/omnibridge-fake-phone".to_string());
    let store = Store::open(&data_dir)?;
    println!(
        "fake phone identity: {}",
        store.identity().fingerprint().to_display_short()
    );

    let battery_state = Arc::new(BatteryState::default());

    // files.v1, in the dialer role. The download directory is under the fake
    // phone's own data dir so a demo never writes into the operator's real
    // Downloads folder.
    let downloads = std::path::PathBuf::from(&data_dir).join("downloads");
    let transfers = TransferManager::new(
        StreamRole::Dialer,
        store.identity().fingerprint(),
        FilesConfig {
            destination: Destination::new(downloads.clone()),
            ..FilesConfig::default()
        },
        Arc::new(AcceptEverything),
    );

    let registry = CapabilityRegistry::builder()
        .register(Arc::new(BatteryCapability::new(Arc::clone(&battery_state))))
        .register(Arc::new(FilesCapability::new(Arc::clone(&transfers))))
        .build();

    // Presented as an Android device so the demo output reads the way the
    // real app would. Only the fingerprint is load-bearing; the name and
    // platform are cosmetic metadata the daemon sanitizes before display.
    let mut info = store.identity().device_info();
    info.device_name = "Fake Phone (dev tool)".to_string();
    info.platform = v1::Platform::Android as i32;

    let host = Arc::new(PhoneHost {
        info,
        registry,
        store: Mutex::new(store),
    });

    // Trusts whatever the desktop is. A demo tool, not a policy: the real
    // phone reads its own trust store here.
    transfers.set_authorizer(Arc::new(AlwaysAuthorized)).await;

    match command {
        "pair" => {
            let raw = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("usage: fake_phone pair '<qr payload>'"))?;
            let payload = QrPayload::parse(raw)?;
            // The scheme decides. A switch that disagrees is a mistake by
            // the operator, and obeying it would pair under a profile the
            // code never named.
            if let Some(requested) = requested_profile {
                if requested != payload.profile {
                    anyhow::bail!(
                        "--profile {requested} contradicts the payload's {} scheme",
                        payload.profile.qr_scheme()
                    );
                }
            }
            let address = *payload
                .addresses
                .first()
                .ok_or_else(|| anyhow::anyhow!("the QR payload carried no address"))?;
            let token = payload.token()?;
            println!(
                "pairing with {} at {address}",
                payload.fingerprint.to_display_short()
            );
            run(
                host,
                address,
                payload.fingerprint,
                payload.profile,
                Some(token),
            )
            .await
        }
        "send" | "receive" => {
            let (fingerprint, address) = {
                let store = host.store.lock().await;
                let peer = store
                    .peers()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("not paired yet; run `pair` first"))?;
                let address: std::net::SocketAddr = std::env::var("FAKE_PHONE_ADDR")
                    .unwrap_or_else(|_| "127.0.0.1:55432".to_string())
                    .parse()?;
                (peer.fingerprint, address)
            };

            transfers
                .set_dialer(Arc::new(PhoneDialer {
                    address,
                    pinned: fingerprint,
                    host: Arc::clone(&host),
                }))
                .await;
            transfers.spawn_reaper();

            let to_send = if command == "send" {
                Some(std::path::PathBuf::from(args.get(2).ok_or_else(|| {
                    anyhow::anyhow!("usage: fake_phone send <file>")
                })?))
            } else {
                println!("waiting for a file in {}", downloads.display());
                None
            };

            run_files(host, transfers, address, fingerprint, profile, to_send).await
        }
        "connect" => {
            let (fingerprint, address) = {
                let store = host.store.lock().await;
                let peer = store
                    .peers()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("not paired yet; run `pair` first"))?;
                let address: std::net::SocketAddr = args
                    .get(2)
                    .map(String::as_str)
                    .unwrap_or("127.0.0.1:55432")
                    .parse()?;
                (peer.fingerprint, address)
            };
            println!(
                "connecting to {} at {address}",
                fingerprint.to_display_short()
            );
            run(host, address, fingerprint, profile, None).await
        }
        _ => {
            eprintln!(
                "usage: fake_phone [--profile pliwee|omnibridge] pair '<qr payload>' \
                 | connect [addr:port] | send <file> | receive"
            );
            std::process::exit(2);
        }
    }
}

async fn run(
    host: Arc<PhoneHost>,
    address: std::net::SocketAddr,
    pinned: Fingerprint,
    profile: Profile,
    token: Option<pliwee_core::pairing::PairingToken>,
) -> anyhow::Result<()> {
    let identity_config = {
        let store = host.store.lock().await;
        pliwee_core::tls::client_config(store.identity(), pinned, profile)?
    };

    let connector = TlsConnector::from(identity_config);
    let tcp = tokio::net::TcpStream::connect(address).await?;
    tcp.set_nodelay(true)?;
    let name = rustls_pki_types::ServerName::try_from("pliwee.invalid")?;
    let mut tls = connector.connect(name, tcp).await?;
    pliwee_core::tls::require_negotiated(tls.get_ref().1, profile, ConnectionKind::Control)?;
    println!("TLS established under profile {profile} and server identity pinned");

    let session_host: Arc<dyn SessionHost> = host.clone();
    let handshake =
        session::connect_handshake(&mut tls, &session_host, pinned, profile, token.as_ref())
            .await?;

    let (established, state) = match handshake {
        ClientHandshake::Established(e, s) => (e, s),
        ClientHandshake::PairingRequired => {
            anyhow::bail!("the daemon does not know this device; run `pair` first")
        }
    };

    println!(
        "session established with {} ({}), capabilities: {:?}",
        established.device.device_name,
        established.peer.to_display_short(),
        established.negotiated_capabilities
    );

    let capabilities = established.negotiated_capabilities.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let notifier: Arc<dyn SessionHost> = Arc::new(Notifier {
        inner: session_host,
        tx: Mutex::new(Some(ready_tx)),
    });

    let task =
        tokio::spawn(async move { session::run_session(tls, notifier, established, state).await });

    let handle = ready_rx.await?;

    if let Some(rtt) = handle.ping(Duration::from_secs(5)).await {
        println!("PING/PONG round trip: {} ms", rtt.as_millis());
    } else {
        println!("no PONG received");
    }

    if capabilities.iter().any(|c| c == "battery.v1") {
        let reading = BatteryReading {
            percentage: 87,
            charging_state: ChargingState::Charging,
            peer_timestamp_unix_ms: 0,
        };
        handle
            .send_capability(BatteryCapability::encode(&reading))
            .await;
        println!("sent battery.v1: 87% charging");
    }

    // Stay connected briefly so `pliwee status` can be run against a live
    // session in another terminal.
    tokio::time::sleep(Duration::from_secs(
        std::env::var("FAKE_PHONE_HOLD_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3),
    ))
    .await;

    handle.shutdown().await;
    let _ = task.await;
    println!("disconnected");
    Ok(())
}

/// Captures the `SessionHandle` when the session starts.
struct Notifier {
    inner: Arc<dyn SessionHost>,
    tx: Mutex<Option<tokio::sync::oneshot::Sender<SessionHandle>>>,
}

#[async_trait::async_trait]
impl SessionHost for Notifier {
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
    async fn on_closed(&self, peer: &Fingerprint, session_id: pliwee_core::session::SessionId) {
        self.inner.on_closed(peer, session_id).await;
    }
}
