//! `files.v1` — authenticated file transfer between a phone and a desktop.
//!
//! # Two channels, and why
//!
//! ```text
//!   control session  (ALPN "pliwee/1")            data stream  (ALPN "pliwee-data/1")
//!   ────────────────────────────────────       ──────────────────────────────────
//!   FILE_OFFER      metadata, sha256, size     DataStreamAuth   transfer_id + MAC
//!   FILE_ACCEPT     stream challenge           DataStreamReady  go / no
//!   FILE_CANCEL     usable *during* a copy     <raw bytes>      exactly size_bytes
//!   FILE_COMPLETE   receiver's verdict
//! ```
//!
//! Both channels run under one identity profile (ADR-0020 §D4). A control
//! session that negotiated the legacy `omnibridge/1` gets data streams on
//! `omnibridge-data/1` only; a stream whose profile differs from its
//! transfer's control session is refused before any byte moves.
//!
//! The split is ADR-0012's decision and this module is its implementation.
//! `MAX_FRAME_LEN` stays at 64 KiB; no file byte ever enters an `Envelope`.
//! Because cancellation and revocation travel on the control session, they
//! keep working while a multi-gigabyte copy is in flight — a transfer can
//! never delay an unpair.
//!
//! # Where each security property lives
//!
//! | Property | Enforced by |
//! | --- | --- |
//! | only a paired device can speak at all | TLS 1.3 + SPKI pinning (`pliwee_core::tls`) |
//! | only a *granted* device may transfer | [`FilesAuthorizer`], re-checked per offer, per stream, and periodically |
//! | a stream belongs to one transfer and one peer | [`auth`] — HMAC over a single-use challenge |
//! | a filename cannot escape the download directory | [`filename::sanitize`] + [`destination`] |
//! | the bytes are the offered bytes | SHA-256 over the whole file, checked before promotion |
//! | nothing unbounded | [`limits`] |

pub mod auth;
/// The Unix filesystem destination. Feature-gated: with `unix-fs` off, this
/// crate has no `std::os` anything and `FileSink` is the only door.
#[cfg(feature = "unix-fs")]
pub mod destination;
pub mod filename;
pub mod limits;
pub mod sink;
pub mod stream;
pub mod transfer;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc, watch, Mutex, RwLock};
use tokio::time::{Duration, Instant};

use pliwee_core::capability::{Capability, CapabilityContext, OutboundMessage};
use pliwee_core::error::{Error, Result};
use pliwee_core::{Fingerprint, Profile};
use pliwee_proto::v1::capabilities as pb;
use pliwee_proto::Message;

use auth::StreamChallenge;
use limits::*;
pub use sink::{Destination, FileSink};
use transfer::{Direction, FailureReason, TransferId, TransferState};

pub const CAPABILITY_ID: &str = "files.v1";

// ---------------------------------------------------------------------------
// Host integration points
// ---------------------------------------------------------------------------

/// Asks a human whether to accept an incoming file.
///
/// There is no default implementation and no "accept everything" mode. A
/// device that is merely paired must not be able to write a file to the other
/// device without someone saying yes; auto-accept is a *future* per-peer
/// setting, not a fallback for a host that did not wire this up.
#[async_trait::async_trait]
pub trait TransferApproval: Send + Sync {
    async fn confirm_receive(&self, offer: &IncomingOffer) -> bool;
}

/// What the human is shown when deciding.
///
/// `filename` is already sanitized: the raw peer-supplied string never
/// reaches a UI, because a name full of control characters could forge the
/// prompt it appears in.
#[derive(Debug, Clone)]
pub struct IncomingOffer {
    pub transfer_id: TransferId,
    pub peer: Fingerprint,
    pub peer_device_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub mime_type: String,
}

/// Decides whether a peer may use `files.v1` **right now**.
///
/// Consulted at three separate moments — when an offer arrives, when a data
/// stream authenticates, and periodically while a transfer runs — rather than
/// once at handshake time. The handshake's answer is a snapshot, and a
/// revocation that only took effect at the next reconnect would leave a
/// revoked device streaming (F15).
#[async_trait::async_trait]
pub trait FilesAuthorizer: Send + Sync {
    async fn is_authorized(&self, peer: &Fingerprint) -> bool;
}

/// Anything a data stream can be carried over. In production this is always a
/// `tokio_rustls` TLS stream.
pub trait DataStreamIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> DataStreamIo for T {}

/// Opens a data stream to a peer.
///
/// Implemented by the side that *dials* — the phone in production, and the
/// Rust test harness when it plays one. The desktop daemon accepts streams
/// and does not implement this.
#[async_trait::async_trait]
pub trait DataStreamDialer: Send + Sync {
    /// Opens a data stream to `peer` under `profile` — the profile of the
    /// control session that negotiated the transfer. The stream must offer
    /// that profile's data ALPN and no other.
    async fn dial(&self, peer: &Fingerprint, profile: Profile) -> Result<Box<dyn DataStreamIo>>;
}

/// Which end of a data stream this device is.
///
/// Fixed by which device listens, never by which device is sending the file.
/// A phone is not a stable listener, so it always dials; the desktop always
/// accepts. Both directions of transfer work over a connection the phone
/// opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRole {
    /// Accepts data-stream connections and issues the challenges.
    Acceptor,
    /// Dials data-stream connections and proves the challenges.
    Dialer,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FilesConfig {
    /// Where received files are stored.
    pub destination: Destination,
    /// Largest single file this device will accept.
    pub max_file_bytes: u64,
    /// How long a human has to answer an offer. Defaults to
    /// [`ACCEPT_TIMEOUT`].
    pub accept_timeout: Duration,
    /// How long, after both sides agree, the dialer has to open the data
    /// stream. Defaults to [`STREAM_OPEN_TIMEOUT`].
    pub stream_open_timeout: Duration,
}

/// The default configuration, with this machine's default destination.
///
/// Only available with a compiled-in [`FileSink`]. A build without one has no
/// answer to "where do downloads go", and inventing a path would be worse
/// than making the caller say. Use [`FilesConfig::with_destination`] there.
#[cfg(feature = "unix-fs")]
impl Default for FilesConfig {
    fn default() -> Self {
        Self::with_destination(Destination::default_location())
    }
}

impl FilesConfig {
    /// The default limits and timeouts, against an explicit destination.
    pub fn with_destination(destination: Destination) -> Self {
        Self {
            destination,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            accept_timeout: ACCEPT_TIMEOUT,
            stream_open_timeout: STREAM_OPEN_TIMEOUT,
        }
    }
}

impl FilesConfig {
    /// How long a transfer may sit in each state.
    fn deadline_for(&self, state: TransferState) -> Duration {
        match state {
            // Waiting on the peer's answer, or on a human.
            TransferState::Offered | TransferState::WaitingAccept => self.accept_timeout,
            // Agreed: the dialer must now open the stream. Once it has, the
            // stream's own idle timeout takes over and the reaper stops being
            // the thing that bounds it.
            TransferState::Transferring => self.stream_open_timeout,
            // Hashing a large file takes a while; this is generous.
            TransferState::Verifying => VERDICT_TIMEOUT,
            _ => Duration::from_secs(3600),
        }
    }
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

/// A transfer as seen from outside. Everything a UI or the CLI needs, and
/// nothing that would leak a secret: the challenge is not here.
#[derive(Debug, Clone)]
pub struct TransferSnapshot {
    pub id: TransferId,
    /// Creation order within this process: lower is older.
    ///
    /// Exposed because a transfer id is 128 random bits and therefore says
    /// nothing about age, and because the map these are read out of is keyed
    /// by that id — so the order they arrive in is the order of a random
    /// number. Anything that wants to show the most recent transfer first has
    /// to be told which one that is; deriving it from list position would be
    /// the same defect U2 P1 fixed for peers.
    pub seq: u64,
    pub peer: Fingerprint,
    pub direction: Direction,
    pub filename: String,
    /// Advisory, and peer-supplied on a receive. Shown, never acted on.
    pub mime_type: String,
    pub size_bytes: u64,
    pub bytes_transferred: u64,
    pub state: TransferState,
    pub failure: Option<FailureReason>,
    /// Where the file ended up. Local information: never sent to a peer.
    pub stored_at: Option<PathBuf>,
}

impl TransferSnapshot {
    /// Percentage complete, or `None` when the size is zero.
    pub fn percentage(&self) -> Option<u8> {
        if self.size_bytes == 0 {
            return None;
        }
        let pct = (self.bytes_transferred.min(self.size_bytes) * 100) / self.size_bytes;
        Some(pct as u8)
    }
}

/// Emitted whenever a transfer changes. Consumed by the CLI to render
/// progress without polling.
#[derive(Debug, Clone)]
pub struct TransferEvent(pub TransferSnapshot);

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

struct TransferRecord {
    /// Creation order, so the oldest finished records can be dropped first.
    /// A transfer id is random and therefore says nothing about age.
    seq: u64,
    id: TransferId,
    peer: Fingerprint,
    direction: Direction,
    filename: String,
    size_bytes: u64,
    sha256: [u8; 32],
    mime_type: String,
    state: TransferState,
    bytes: Arc<AtomicU64>,
    /// Present on the acceptor side until a data stream consumes it. Taking
    /// it is what makes the challenge single-use: a second stream for the
    /// same transfer finds nothing to check against and is refused.
    challenge: Option<StreamChallenge>,
    /// Sending: the local file. Never sent to the peer.
    source: Option<PathBuf>,
    /// Receiving: the partial file being written.
    temp: Option<PathBuf>,
    stored_at: Option<PathBuf>,
    failure: Option<FailureReason>,
    cancel: watch::Sender<bool>,
    /// The control session this transfer belongs to. When the session ends,
    /// this channel closes, which is how the reaper notices a disconnect
    /// (F14) — the data stream is a *separate* TCP connection and would
    /// otherwise happily keep running after the control link died.
    session: mpsc::Sender<OutboundMessage>,
    /// Identity profile of that control session (ADR-0020 §D4). The data
    /// stream must negotiate the same one, and its MAC is keyed to this
    /// profile's domain only. Held in memory; never persisted.
    profile: Profile,
    /// When the current state stops being acceptable.
    deadline: Instant,
    /// True while a data stream is actually moving bytes for this transfer.
    ///
    /// While it is set, the reaper does not apply `deadline`: a large file
    /// legitimately takes longer than any fixed deadline, and the copy loop
    /// enforces its own idle timeout, which is the right bound. Without this,
    /// the deadline that exists to reap a transfer nobody dialled would also
    /// kill a healthy multi-gigabyte copy.
    stream_active: bool,
}

impl TransferRecord {
    fn snapshot(&self) -> TransferSnapshot {
        TransferSnapshot {
            id: self.id,
            seq: self.seq,
            peer: self.peer,
            direction: self.direction,
            filename: self.filename.clone(),
            mime_type: self.mime_type.clone(),
            size_bytes: self.size_bytes,
            bytes_transferred: self.bytes.load(Ordering::Relaxed),
            state: self.state,
            failure: self.failure,
            stored_at: self.stored_at.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// The manager
// ---------------------------------------------------------------------------

/// Owns every transfer this device is involved in.
pub struct TransferManager {
    role: StreamRole,
    local_fingerprint: Fingerprint,
    config: RwLock<FilesConfig>,
    transfers: Mutex<BTreeMap<TransferId, TransferRecord>>,
    approval: Arc<dyn TransferApproval>,
    authorizer: RwLock<Option<Arc<dyn FilesAuthorizer>>>,
    dialer: RwLock<Option<Arc<dyn DataStreamDialer>>>,
    events: broadcast::Sender<TransferEvent>,
    /// The control channel of each connected peer, so a transfer can be
    /// started from outside a session — `pliwee send`, or a tap in the
    /// phone's UI — rather than only in reply to an inbound message.
    sessions: RwLock<BTreeMap<Fingerprint, (mpsc::Sender<OutboundMessage>, Profile)>>,
}

impl TransferManager {
    pub fn new(
        role: StreamRole,
        local_fingerprint: Fingerprint,
        config: FilesConfig,
        approval: Arc<dyn TransferApproval>,
    ) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            role,
            local_fingerprint,
            config: RwLock::new(config),
            transfers: Mutex::new(BTreeMap::new()),
            approval,
            authorizer: RwLock::new(None),
            dialer: RwLock::new(None),
            events,
            sessions: RwLock::new(BTreeMap::new()),
        })
    }

    /// Wires up the grant check. Until this is set, nothing is authorized:
    /// a host that forgets to call it cannot accidentally run wide open.
    pub async fn set_authorizer(&self, authorizer: Arc<dyn FilesAuthorizer>) {
        *self.authorizer.write().await = Some(authorizer);
    }

    pub async fn set_dialer(&self, dialer: Arc<dyn DataStreamDialer>) {
        *self.dialer.write().await = Some(dialer);
    }

    pub async fn set_config(&self, config: FilesConfig) {
        *self.config.write().await = config;
    }

    pub async fn config(&self) -> FilesConfig {
        self.config.read().await.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TransferEvent> {
        self.events.subscribe()
    }

    /// Records a peer's control channel when its session becomes usable.
    ///
    /// A reconnection replaces the entry, which is what we want: the newest
    /// session is the one a new transfer should use. `profile` is that
    /// session's negotiated profile, which every transfer it carries inherits.
    pub async fn attach_session(
        &self,
        peer: Fingerprint,
        session: mpsc::Sender<OutboundMessage>,
        profile: Profile,
    ) {
        self.sessions.write().await.insert(peer, (session, profile));
    }

    /// The live control channel for a peer, if there is one.
    ///
    /// A closed channel is dropped here rather than in a disconnect callback.
    /// The callback is given only a fingerprint, and a peer that reconnects
    /// produces the *new* session's connect before the *old* session's
    /// disconnect — so removing by fingerprint there would evict the live
    /// session. Checking whether the channel is actually closed cannot make
    /// that mistake.
    pub async fn session_for(
        &self,
        peer: &Fingerprint,
    ) -> Option<(mpsc::Sender<OutboundMessage>, Profile)> {
        let mut sessions = self.sessions.write().await;
        match sessions.get(peer) {
            Some((tx, profile)) if !tx.is_closed() => Some((tx.clone(), *profile)),
            Some(_) => {
                sessions.remove(peer);
                None
            }
            None => None,
        }
    }

    async fn authorized(&self, peer: &Fingerprint) -> bool {
        match self.authorizer.read().await.clone() {
            Some(a) => a.is_authorized(peer).await,
            // Fail closed. An unwired host transfers nothing.
            None => false,
        }
    }

    // -- reporting ---------------------------------------------------------

    pub async fn snapshot(&self) -> Vec<TransferSnapshot> {
        self.transfers
            .lock()
            .await
            .values()
            .map(TransferRecord::snapshot)
            .collect()
    }

    pub async fn snapshot_one(&self, id: TransferId) -> Option<TransferSnapshot> {
        self.transfers.lock().await.get(&id).map(|r| r.snapshot())
    }

    fn publish(&self, snapshot: TransferSnapshot) {
        // A send failure only means nobody is watching.
        let _ = self.events.send(TransferEvent(snapshot));
    }

    // -- state changes -----------------------------------------------------

    /// The only place a transfer changes state.
    ///
    /// Refuses a transition that is not in the table, so a duplicate
    /// `FILE_COMPLETE` (F17), a late cancel, or a second data stream for a
    /// finished transfer are all rejected here rather than by a check
    /// remembered at each call site.
    async fn transition(
        &self,
        id: TransferId,
        next: TransferState,
        failure: Option<FailureReason>,
    ) -> bool {
        // Read before taking the transfers lock: the two are independent, and
        // taking them in different orders in different places is how a
        // deadlock is built.
        let next_deadline = Instant::now() + self.config.read().await.deadline_for(next);

        let snapshot = {
            let mut transfers = self.transfers.lock().await;
            let Some(record) = transfers.get_mut(&id) else {
                return false;
            };
            if !record.state.can_transition_to(next) {
                tracing::debug!(
                    transfer = %id,
                    from = %record.state,
                    to = %next,
                    "refused an illegal transfer state transition"
                );
                return false;
            }
            record.state = next;
            if failure.is_some() {
                record.failure = failure;
            }
            record.deadline = next_deadline;

            let mut temp = None;
            if next.is_terminal() {
                // Anything still copying stops now.
                let _ = record.cancel.send(true);
                // The challenge has no further use and is a key.
                record.challenge = None;
                temp = record.temp.take();
            }
            let snapshot = record.snapshot();

            // Removed before the lock is released, not after: the terminal
            // state is visible to `snapshot_one` the moment the lock drops,
            // and whoever reads `Failed` must not still find the partial file
            // on disk. Releasing first left exactly that window open (#105).
            // The price is one unlink under the map lock per ended receive.
            if let Some(path) = temp {
                remove_partial(id, &path).await;
            }
            snapshot
        };

        self.publish(snapshot);
        true
    }

    /// Removes the partial file of a transfer that will not complete.
    ///
    /// For the stale-accept path only: a terminal `transition` removes its
    /// own partial file. This one is for a temp file opened after that ran.
    async fn clean_up_temp(&self, id: TransferId) {
        let mut transfers = self.transfers.lock().await;
        if let Some(path) = transfers.get_mut(&id).and_then(|r| r.temp.take()) {
            remove_partial(id, &path).await;
        }
    }
}

/// Deletes a transfer's `.part` file.
///
/// A `.part` file left behind after a failure is not merely untidy: it is
/// unverified peer-supplied data sitting in the user's download folder.
async fn remove_partial(id: TransferId, path: &Path) {
    if let Err(e) = tokio::fs::remove_file(path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            // The path is local, not peer-supplied, so logging the error
            // kind is safe; the path itself is not logged.
            tracing::warn!(transfer = %id, error = %e, "could not remove a partial file");
        }
    }
}

impl TransferManager {
    /// Ends a transfer and tells the peer why.
    async fn fail(&self, id: TransferId, reason: FailureReason) {
        let next = if reason.is_cancellation() {
            TransferState::Cancelled
        } else {
            TransferState::Failed
        };

        let session = {
            let transfers = self.transfers.lock().await;
            transfers.get(&id).map(|r| r.session.clone())
        };

        if !self.transition(id, next, Some(reason)).await {
            return;
        }

        tracing::info!(
            transfer = %id,
            state = %next,
            reason = %reason,
            "transfer ended"
        );

        if let Some(tx) = session {
            let body = if reason.is_cancellation() {
                pb::file_control::Body::Cancel(pb::FileCancel {
                    transfer_id: id.to_vec(),
                    reason: transfer::reason_to_proto(reason) as i32,
                })
            } else {
                pb::file_control::Body::Failed(pb::FileFailed {
                    transfer_id: id.to_vec(),
                    reason: transfer::reason_to_proto(reason) as i32,
                })
            };
            send_control(&tx, body).await;
        }
    }

    /// Cancels a transfer on the user's instruction.
    pub async fn cancel(&self, id: TransferId) -> bool {
        let exists = {
            let transfers = self.transfers.lock().await;
            transfers.get(&id).is_some_and(|r| r.state.is_active())
        };
        if !exists {
            return false;
        }
        self.fail(id, FailureReason::CancelledByUser).await;
        true
    }

    /// Stops everything in flight with one peer.
    ///
    /// Called the moment a pairing is revoked, so revocation takes effect now
    /// rather than at the next reconnect. The periodic re-authorization check
    /// in the reaper is the backstop; this is the deterministic path.
    pub async fn cancel_peer(&self, peer: &Fingerprint, reason: FailureReason) {
        let ids: Vec<TransferId> = {
            let transfers = self.transfers.lock().await;
            transfers
                .values()
                .filter(|r| r.peer == *peer && r.state.is_active())
                .map(|r| r.id)
                .collect()
        };
        for id in ids {
            self.fail(id, reason).await;
        }
    }

    async fn active_count_for(&self, peer: &Fingerprint) -> usize {
        self.transfers
            .lock()
            .await
            .values()
            .filter(|r| r.peer == *peer && r.state.is_active())
            .count()
    }
}

/// Queues one control message for the peer.
///
/// Never blocks indefinitely. The session's writer runs in its own task, so
/// a full queue here is real backpressure — the peer is not reading as fast
/// as we are producing — and waiting on it does make room. It is bounded
/// anyway: a socket that has stopped draining must not be able to park a
/// handler, and through it the transfer it belongs to, for the rest of the
/// session. See [`CONTROL_SEND_TIMEOUT`].
///
/// (This wait was once a cycle rather than backpressure: the dispatch loop
/// both awaited `on_message` and drained the queue, so the only task that
/// could make room was the one blocked here. That is fixed in the session
/// layer, not worked around here.)
///
/// The fast path is `try_send`, which does not yield at all.
async fn send_control(tx: &mpsc::Sender<OutboundMessage>, body: pb::file_control::Body) -> bool {
    let message = OutboundMessage {
        capability_id: CAPABILITY_ID.to_string(),
        payload: pb::FileControl { body: Some(body) }.encode_to_vec(),
    };

    match tx.try_send(message) {
        Ok(()) => true,
        // The session is gone. Nothing to report and nobody to report it to.
        Err(mpsc::error::TrySendError::Closed(_)) => false,
        // A transient burst. Wait, but only for a bounded while.
        Err(mpsc::error::TrySendError::Full(message)) => {
            match tokio::time::timeout(CONTROL_SEND_TIMEOUT, tx.send(message)).await {
                Ok(Ok(())) => true,
                Ok(Err(_)) => false,
                Err(_) => {
                    tracing::warn!(
                        "the peer is not draining its session; dropping a files.v1 message"
                    );
                    false
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sending: offering a local file
// ---------------------------------------------------------------------------

impl TransferManager {
    /// Offers a local file to a peer.
    ///
    /// The file is hashed before the offer goes out, because the receiver
    /// must know the expected digest *before* the first byte arrives — that
    /// is what makes the check a verification rather than a comparison
    /// against whatever turned up. Hashing streams through a bounded buffer,
    /// so offering a large file costs no more memory than sending it.
    ///
    /// The absolute path is used to open the file and is never transmitted:
    /// the offer carries a basename and nothing else, so a receiver learns
    /// nothing about the sender's directory layout.
    pub async fn offer_file(&self, peer: Fingerprint, path: PathBuf) -> Result<TransferId> {
        if !self.authorized(&peer).await {
            return Err(Error::NotAuthorized);
        }
        let (session, profile) = self
            .session_for(&peer)
            .await
            .ok_or(Error::Protocol("that device is not connected"))?;
        if self.active_count_for(&peer).await >= MAX_CONCURRENT_TRANSFERS_PER_PEER {
            return Err(Error::Protocol(
                "too many transfers in flight with this peer",
            ));
        }

        let raw_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        // Sanitized on the way out as well as on the way in. A local file
        // cannot be hostile the way a peer's string can, but a name that
        // would be rejected on arrival is better caught here, where the user
        // is standing, than as a puzzling refusal on the other device.
        let filename = filename::sanitize(&raw_name)
            .ok_or(Error::Protocol("the file's name cannot be sent safely"))?;

        let (size_bytes, sha256) = stream::hash_file(&path).await.map_err(Error::Io)?;
        let config = self.config.read().await.clone();

        let id = auth::generate_transfer_id()?;
        let mime_type = guess_mime_type(&filename);
        let (cancel, _) = watch::channel(false);

        let record = TransferRecord {
            seq: next_seq(),
            id,
            peer,
            direction: Direction::Sending,
            filename: filename.clone(),
            size_bytes,
            sha256,
            mime_type: mime_type.clone(),
            state: TransferState::Offered,
            bytes: Arc::new(AtomicU64::new(0)),
            challenge: None,
            source: Some(path),
            temp: None,
            stored_at: None,
            failure: None,
            cancel,
            session: session.clone(),
            profile,
            deadline: Instant::now() + config.deadline_for(TransferState::Offered),
            stream_active: false,
        };

        let snapshot = record.snapshot();
        self.transfers.lock().await.insert(id, record);
        self.publish(snapshot);

        tracing::info!(
            transfer = %id,
            peer = %peer.to_display_short(),
            size = size_bytes,
            "offering a file"
        );

        let sent = send_control(
            &session,
            pb::file_control::Body::Offer(pb::FileOffer {
                transfer_id: id.to_vec(),
                filename,
                size_bytes,
                mime_type,
                sha256: sha256.to_vec(),
                timestamp_unix_ms: now_unix_ms(),
            }),
        )
        .await;

        if !sent {
            self.transition(id, TransferState::Failed, Some(FailureReason::Transport))
                .await;
            return Err(Error::Closed);
        }

        Ok(id)
    }
}

/// A coarse type from the extension, for labelling only.
///
/// Never used to decide what to do with a file — neither side executes,
/// renders or dispatches on this. It exists so a UI can show an icon.
fn guess_mime_type(name: &str) -> String {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "zip" => "application/zip",
        _ => "",
    }
    .to_string()
}

/// Monotonic creation counter for transfer records.
fn next_seq() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Inbound control messages
// ---------------------------------------------------------------------------

impl TransferManager {
    /// Handles one `FileControl` payload from a peer.
    ///
    /// `payload` is untrusted, attacker-controlled bytes. Every field is
    /// validated here, at the capability boundary, before it reaches
    /// anything that could act on it.
    pub async fn handle_control(
        self: &Arc<Self>,
        peer: Fingerprint,
        peer_device_id: String,
        session: mpsc::Sender<OutboundMessage>,
        profile: Profile,
        payload: &[u8],
    ) -> Result<()> {
        let control = pb::FileControl::decode(payload)?;
        let Some(body) = control.body else {
            return Err(Error::Protocol("empty files.v1 message"));
        };

        match body {
            pb::file_control::Body::Offer(offer) => {
                self.on_offer(peer, peer_device_id, session, profile, offer)
                    .await
            }
            pb::file_control::Body::Accept(accept) => self.on_accept(peer, accept).await,
            pb::file_control::Body::Ready(ready) => self.on_ready(peer, ready).await,
            pb::file_control::Body::Reject(reject) => {
                self.on_peer_ended(
                    peer,
                    &reject.transfer_id,
                    reject.reason(),
                    TransferState::Cancelled,
                )
                .await
            }
            pb::file_control::Body::Cancel(cancel) => {
                self.on_peer_ended(
                    peer,
                    &cancel.transfer_id,
                    cancel.reason(),
                    TransferState::Cancelled,
                )
                .await
            }
            pb::file_control::Body::Failed(failed) => {
                self.on_peer_ended(
                    peer,
                    &failed.transfer_id,
                    failed.reason(),
                    TransferState::Failed,
                )
                .await
            }
            pb::file_control::Body::Complete(complete) => {
                self.on_complete(peer, &complete.transfer_id).await
            }
        }
    }

    /// A peer wants to send us a file.
    async fn on_offer(
        self: &Arc<Self>,
        peer: Fingerprint,
        peer_device_id: String,
        session: mpsc::Sender<OutboundMessage>,
        profile: Profile,
        offer: pb::FileOffer,
    ) -> Result<()> {
        let Some(id) = TransferId::from_bytes(&offer.transfer_id) else {
            // No id means there is nothing to reject *about*; the peer is
            // malformed rather than unlucky.
            return Err(Error::Protocol(
                "files.v1 offer with a malformed transfer id",
            ));
        };

        // Authorization first, before any resource is committed. This is the
        // receiver-side check the sender cannot influence: what the peer
        // advertised at handshake time is not consulted here at all.
        if !self.authorized(&peer).await {
            tracing::warn!(
                peer = %peer.to_display_short(),
                "refused a file offer from a peer with no files.v1 grant"
            );
            reject(&session, id, FailureReason::NotAuthorized).await;
            return Ok(());
        }

        // A transfer id is single-use. Reusing one would let a peer collide
        // with a live transfer's state, or resurrect a finished one.
        if self.transfers.lock().await.contains_key(&id) {
            tracing::warn!(transfer = %id, "refused a reused transfer id");
            reject(&session, id, FailureReason::BadMetadata).await;
            return Ok(());
        }

        if let Err(reason) = self.validate_offer(&offer).await {
            reject(&session, id, reason).await;
            return Ok(());
        }

        if self.active_count_for(&peer).await >= MAX_CONCURRENT_TRANSFERS_PER_PEER {
            reject(&session, id, FailureReason::TooManyTransfers).await;
            return Ok(());
        }

        // Unwrap-free: `validate_offer` already established that the name
        // sanitizes to something usable.
        let Some(filename) = filename::sanitize(&offer.filename) else {
            reject(&session, id, FailureReason::BadMetadata).await;
            return Ok(());
        };
        let Ok(sha256) = <[u8; 32]>::try_from(offer.sha256.as_slice()) else {
            reject(&session, id, FailureReason::BadMetadata).await;
            return Ok(());
        };

        let config = self.config.read().await.clone();
        let (cancel, _) = watch::channel(false);
        let record = TransferRecord {
            seq: next_seq(),
            id,
            peer,
            direction: Direction::Receiving,
            filename: filename.clone(),
            size_bytes: offer.size_bytes,
            sha256,
            mime_type: offer.mime_type.clone(),
            state: TransferState::WaitingAccept,
            bytes: Arc::new(AtomicU64::new(0)),
            challenge: None,
            source: None,
            temp: None,
            stored_at: None,
            failure: None,
            cancel,
            session: session.clone(),
            profile,
            deadline: Instant::now() + config.deadline_for(TransferState::WaitingAccept),
            stream_active: false,
        };

        let snapshot = record.snapshot();
        self.transfers.lock().await.insert(id, record);
        self.publish(snapshot);

        // The sanitized name is what is logged. The raw one is never logged
        // at any level: it is attacker-controlled and could forge log lines.
        tracing::info!(
            transfer = %id,
            peer = %peer.to_display_short(),
            filename = %filename,
            size = offer.size_bytes,
            "incoming file offer"
        );

        // Asking a human must not block the session's message loop: it can
        // take as long as the accept timeout allows, during which pings, battery updates and
        // — critically — an unpair must all keep working.
        let manager = Arc::clone(self);
        let request = IncomingOffer {
            transfer_id: id,
            peer,
            peer_device_id,
            filename,
            size_bytes: offer.size_bytes,
            mime_type: offer.mime_type,
        };
        tokio::spawn(async move { manager.await_approval(request, session).await });

        Ok(())
    }

    /// Validates an offer's metadata against this device's limits.
    async fn validate_offer(
        &self,
        offer: &pb::FileOffer,
    ) -> std::result::Result<(), FailureReason> {
        if offer.sha256.len() != 32 {
            return Err(FailureReason::BadMetadata);
        }
        if offer.filename.len() > MAX_FILENAME_BYTES * 4 {
            // Bounded before sanitizing so a megabyte of "filename" is not
            // walked character by character. Four times the cap allows for
            // multi-byte characters that will shrink once trimmed.
            return Err(FailureReason::BadMetadata);
        }
        if offer.mime_type.len() > MAX_MIME_TYPE_BYTES {
            return Err(FailureReason::BadMetadata);
        }
        if filename::sanitize(&offer.filename).is_none() {
            return Err(FailureReason::BadMetadata);
        }
        if offer.size_bytes > self.config.read().await.max_file_bytes {
            return Err(FailureReason::TooLarge);
        }
        Ok(())
    }

    /// Runs the human decision and, on a yes, moves the transfer to active.
    async fn await_approval(
        self: Arc<Self>,
        request: IncomingOffer,
        session: mpsc::Sender<OutboundMessage>,
    ) {
        let id = request.transfer_id;
        let peer = request.peer;

        // Derived from the configured timeout rather than from the constant,
        // so a host that lengthens `accept_timeout` does not find its prompts
        // cut short by a bound it never set — and deliberately *later* than
        // the reaper's deadline for the same state, so that an offer nobody
        // answers is ended by the reaper, as `TimedOut`, rather than by this,
        // as `DeclinedByUser`. See `APPROVAL_BACKSTOP_GRACE`.
        //
        // When the reaper wins, this future is not left hanging either: the
        // host's approval provider is told the transfer ended and drops the
        // question, and the late `false` that arrives here cannot re-end an
        // already terminal transfer.
        let backstop = self
            .config
            .read()
            .await
            .accept_timeout
            .saturating_add(APPROVAL_BACKSTOP_GRACE);
        let accepted = tokio::time::timeout(backstop, self.approval.confirm_receive(&request))
            .await
            .unwrap_or(false);

        if !accepted {
            self.fail(id, FailureReason::DeclinedByUser).await;
            return;
        }

        // Re-checked after the human, not only before: a pairing can be
        // revoked while a prompt sits on screen, and the answer that matters
        // is the one true at the moment we would act.
        if !self.authorized(&peer).await {
            self.fail(id, FailureReason::Revoked).await;
            return;
        }

        // Open the temp file *before* telling the peer we are ready, so a
        // full or read-only disk is discovered now rather than after the
        // sender has started streaming.
        let destination = self.config.read().await.destination.clone();
        let temp = match tokio::task::spawn_blocking(move || destination.open_temp(id)).await {
            Ok(Ok((file, path))) => {
                drop(file);
                path
            }
            Ok(Err(e)) => {
                tracing::warn!(transfer = %id, error = %e, "could not open a temp file");
                self.fail(id, FailureReason::Storage).await;
                return;
            }
            Err(_) => {
                self.fail(id, FailureReason::Storage).await;
                return;
            }
        };

        // The challenge, if this device is the one that accepts streams.
        let challenge = match self.role {
            StreamRole::Acceptor => match StreamChallenge::generate() {
                Ok(c) => Some(c),
                Err(_) => {
                    self.fail(id, FailureReason::Storage).await;
                    return;
                }
            },
            StreamRole::Dialer => None,
        };
        let challenge_bytes = challenge
            .as_ref()
            .map(StreamChallenge::expose_for_control_message)
            .unwrap_or_default();

        {
            let mut transfers = self.transfers.lock().await;
            let Some(record) = transfers.get_mut(&id) else {
                return;
            };
            record.temp = Some(temp);
            record.challenge = challenge;
        }

        // The acceptor moves into Transferring *before* the challenge goes
        // out, so that by the time a dialer can act on it the stream will be
        // accepted. See FileReady in files_v1.proto.
        //
        // A refusal here is not a "should not happen": it is the stale-accept
        // path. Asking a human takes human time, and the reaper may have
        // ended this transfer while the prompt was on screen — the peer
        // disconnected, the offer expired, the pairing was revoked. The state
        // machine is what makes a late yes inert, and the temp file opened a
        // few lines above has to go with it: `clean_up_temp` already ran, on
        // a record that did not yet have a path to clean.
        if !self.transition(id, TransferState::Transferring, None).await {
            self.clean_up_temp(id).await;
            return;
        }

        let sent = send_control(
            &session,
            pb::file_control::Body::Accept(pb::FileAccept {
                transfer_id: id.to_vec(),
                stream_challenge: challenge_bytes,
            }),
        )
        .await;

        if !sent {
            self.fail(id, FailureReason::Transport).await;
            return;
        }

        // A dialer must now open the stream itself; an acceptor waits to be
        // dialled.
        if self.role == StreamRole::Dialer {
            let manager = Arc::clone(&self);
            tokio::spawn(async move { manager.dial_and_run(id).await });
        }
    }
}

async fn reject(session: &mpsc::Sender<OutboundMessage>, id: TransferId, reason: FailureReason) {
    send_control(
        session,
        pb::file_control::Body::Reject(pb::FileReject {
            transfer_id: id.to_vec(),
            reason: transfer::reason_to_proto(reason) as i32,
        }),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Inbound: the other side's answers
// ---------------------------------------------------------------------------

impl TransferManager {
    /// The peer accepted a file we offered.
    async fn on_accept(self: &Arc<Self>, peer: Fingerprint, accept: pb::FileAccept) -> Result<()> {
        let Some(id) = TransferId::from_bytes(&accept.transfer_id) else {
            return Err(Error::Protocol(
                "files.v1 accept with a malformed transfer id",
            ));
        };

        // Ownership check: a peer may only answer for its own transfers. Two
        // paired devices exist in this system, and one must not be able to
        // steer the other's transfer by naming its id.
        let session = match self.owned_active(id, peer, Direction::Sending).await {
            Some(s) => s,
            None => return Ok(()),
        };

        match self.role {
            // We accept streams, so we issue the challenge — but we are the
            // sender here, so it did not fit in an accept. Send it now.
            StreamRole::Acceptor => {
                let challenge = StreamChallenge::generate()?;
                let bytes = challenge.expose_for_control_message();
                {
                    let mut transfers = self.transfers.lock().await;
                    let Some(record) = transfers.get_mut(&id) else {
                        return Ok(());
                    };
                    record.challenge = Some(challenge);
                }
                if !self.transition(id, TransferState::Transferring, None).await {
                    return Ok(());
                }
                send_control(
                    &session,
                    pb::file_control::Body::Ready(pb::FileReady {
                        transfer_id: id.to_vec(),
                        stream_challenge: bytes,
                    }),
                )
                .await;
            }

            // We dial, so the accept carried the challenge we must prove.
            StreamRole::Dialer => {
                let Some(challenge) = StreamChallenge::from_bytes(&accept.stream_challenge) else {
                    self.fail(id, FailureReason::BadMetadata).await;
                    return Ok(());
                };
                {
                    let mut transfers = self.transfers.lock().await;
                    let Some(record) = transfers.get_mut(&id) else {
                        return Ok(());
                    };
                    record.challenge = Some(challenge);
                }
                if !self.transition(id, TransferState::Transferring, None).await {
                    return Ok(());
                }
                let manager = Arc::clone(self);
                tokio::spawn(async move { manager.dial_and_run(id).await });
            }
        }
        Ok(())
    }

    /// The acceptor is ready to be dialled for a transfer we are receiving.
    async fn on_ready(self: &Arc<Self>, peer: Fingerprint, ready: pb::FileReady) -> Result<()> {
        let Some(id) = TransferId::from_bytes(&ready.transfer_id) else {
            return Err(Error::Protocol(
                "files.v1 ready with a malformed transfer id",
            ));
        };
        if self.role != StreamRole::Dialer {
            // Only a dialer has any use for this. An acceptor receiving one
            // is talking to a peer that has the roles confused.
            return Ok(());
        }
        if self
            .owned_active(id, peer, Direction::Receiving)
            .await
            .is_none()
        {
            return Ok(());
        }

        let Some(challenge) = StreamChallenge::from_bytes(&ready.stream_challenge) else {
            self.fail(id, FailureReason::BadMetadata).await;
            return Ok(());
        };
        {
            let mut transfers = self.transfers.lock().await;
            let Some(record) = transfers.get_mut(&id) else {
                return Ok(());
            };
            record.challenge = Some(challenge);
        }

        let manager = Arc::clone(self);
        tokio::spawn(async move { manager.dial_and_run(id).await });
        Ok(())
    }

    /// The peer rejected, cancelled or failed a transfer.
    ///
    /// Terminal without a reply: answering a cancel with a cancel would let
    /// two peers ping-pong for as long as the session lasted.
    async fn on_peer_ended(
        &self,
        peer: Fingerprint,
        raw_id: &[u8],
        reason: pb::TransferFailureReason,
        next: TransferState,
    ) -> Result<()> {
        let Some(id) = TransferId::from_bytes(raw_id) else {
            return Ok(());
        };
        if !self.belongs_to(id, peer).await {
            return Ok(());
        }
        let reason = transfer::reason_from_proto(reason);
        tracing::info!(transfer = %id, reason = %reason, "the peer ended a transfer");
        self.transition(id, next, Some(reason)).await;
        Ok(())
    }

    /// The receiver verified and stored a file we sent.
    ///
    /// Only meaningful for a transfer we are *sending*: a FILE_COMPLETE about
    /// a transfer we are receiving would be the sender claiming our own
    /// verification result, which is not its to claim.
    async fn on_complete(&self, peer: Fingerprint, raw_id: &[u8]) -> Result<()> {
        let Some(id) = TransferId::from_bytes(raw_id) else {
            return Ok(());
        };
        if self
            .owned_active(id, peer, Direction::Sending)
            .await
            .is_none()
        {
            // Covers F17: a duplicate FILE_COMPLETE finds the transfer
            // already terminal and is refused by `owned_active`.
            return Ok(());
        }
        if self.transition(id, TransferState::Completed, None).await {
            tracing::info!(transfer = %id, "the peer confirmed it stored the file");
        }
        Ok(())
    }

    /// Returns the session channel of an active transfer that belongs to this
    /// peer and runs in the given direction; `None` if any of that is untrue.
    async fn owned_active(
        &self,
        id: TransferId,
        peer: Fingerprint,
        direction: Direction,
    ) -> Option<mpsc::Sender<OutboundMessage>> {
        let transfers = self.transfers.lock().await;
        let record = transfers.get(&id)?;
        if record.peer != peer || record.direction != direction || !record.state.is_active() {
            return None;
        }
        Some(record.session.clone())
    }

    async fn belongs_to(&self, id: TransferId, peer: Fingerprint) -> bool {
        self.transfers
            .lock()
            .await
            .get(&id)
            .is_some_and(|r| r.peer == peer)
    }
}

// ---------------------------------------------------------------------------
// The data stream
// ---------------------------------------------------------------------------

impl TransferManager {
    /// Accepts an inbound data stream.
    ///
    /// `peer` **must** come from the completed TLS handshake of this very
    /// connection, never from anything inside it. The caller is the listener,
    /// which reads it from the peer certificate exactly as it does for a
    /// control session.
    ///
    /// Every check that stands between a stranger and a file is here:
    ///
    /// 1. the transfer exists and is still active;
    /// 2. it belongs to *this* TLS peer (F2, F3);
    /// 3. the peer still holds a `files.v1` grant (F1, F15);
    /// 4. an unconsumed challenge is present (F5 — a second stream for one
    ///    transfer finds none);
    /// 5. the stream negotiated the same identity profile as the control
    ///    session that issued the challenge (ADR-0020 §D4 — a mismatch is
    ///    refused, never translated);
    /// 6. the MAC verifies under that profile's domain (F4 — a guessed id
    ///    proves nothing).
    ///
    /// `profile`, like `peer`, must come from this connection's own
    /// handshake: it is the profile of the data ALPN it negotiated.
    pub async fn accept_data_stream(
        self: &Arc<Self>,
        peer: Fingerprint,
        mut io: Box<dyn DataStreamIo>,
        protocol_version: u32,
        profile: Profile,
    ) -> Result<()> {
        let auth_frame: pb::DataStreamAuth =
            tokio::time::timeout(STREAM_AUTH_TIMEOUT, stream::read_frame(&mut io))
                .await
                .map_err(|_| Error::Protocol("data stream did not authenticate in time"))?
                .map_err(Error::Io)?;

        if auth_frame.protocol_version != protocol_version {
            return Err(Error::Protocol("data stream protocol version mismatch"));
        }

        let Some(id) = TransferId::from_bytes(&auth_frame.transfer_id) else {
            return Err(Error::Protocol("data stream with a malformed transfer id"));
        };

        let reason = self.check_stream(id, peer, profile, &auth_frame.mac).await;
        if let Err(reason) = reason {
            // The refusal is generic on the wire. A dialer that guessed an id
            // learns only that it did not work, not whether the id existed,
            // whether the MAC was close, or which check failed.
            tracing::warn!(
                transfer = %id,
                peer = %peer.to_display_short(),
                reason = %reason,
                "refused a data stream"
            );
            let _ = stream::write_frame(
                &mut io,
                &pb::DataStreamReady {
                    status: pb::DataStreamStatus::Rejected as i32,
                    reason: transfer::reason_to_proto(reason) as i32,
                },
            )
            .await;
            return Err(Error::NotAuthorized);
        }

        stream::write_frame(
            &mut io,
            &pb::DataStreamReady {
                status: pb::DataStreamStatus::Ready as i32,
                reason: 0,
            },
        )
        .await
        .map_err(Error::Io)?;

        self.run_stream(id, io).await
    }

    /// The authorization decision for one data stream.
    ///
    /// Consumes the challenge on success and only on success, which is what
    /// makes it single-use without letting a failed attempt burn a legitimate
    /// dialer's one chance.
    async fn check_stream(
        &self,
        id: TransferId,
        peer: Fingerprint,
        profile: Profile,
        mac: &[u8],
    ) -> std::result::Result<(), FailureReason> {
        // Authorization is asked before the record is touched, so a revoked
        // peer cannot even learn whether a transfer id exists.
        if !self.authorized(&peer).await {
            return Err(FailureReason::NotAuthorized);
        }

        let mut transfers = self.transfers.lock().await;
        let Some(record) = transfers.get_mut(&id) else {
            return Err(FailureReason::UnknownTransfer);
        };

        // A stream from a device other than the one that negotiated the
        // transfer. The TLS identity is the authority here.
        if record.peer != peer {
            return Err(FailureReason::UnknownTransfer);
        }
        if record.state != TransferState::Transferring {
            return Err(FailureReason::UnknownTransfer);
        }

        let Some(challenge) = record.challenge.as_ref() else {
            // Already consumed by an earlier stream, or never issued.
            return Err(FailureReason::UnknownTransfer);
        };

        // One connection, one identity: a stream under the other profile is
        // refused before its MAC is looked at, and the MAC is then verified
        // under the session's own domain only — never "under both".
        if record.profile != profile {
            tracing::warn!(
                transfer = %id,
                peer = %peer.to_display_short(),
                session_profile = %record.profile,
                stream_profile = %profile,
                "refused a data stream whose profile differs from its control session"
            );
            return Err(FailureReason::NotAuthorized);
        }

        let expected = auth::compute_stream_mac(
            record.profile,
            challenge,
            &self.local_fingerprint,
            &peer,
            &id,
        );
        if !auth::verify_stream_mac(&expected, mac) {
            return Err(FailureReason::NotAuthorized);
        }

        // Single use.
        record.challenge = None;
        record.stream_active = true;
        Ok(())
    }

    /// Dials a data stream and runs it. The dialer's half.
    async fn dial_and_run(self: Arc<Self>, id: TransferId) {
        let Some(dialer) = self.dialer.read().await.clone() else {
            tracing::error!("no data stream dialer is configured");
            self.fail(id, FailureReason::Transport).await;
            return;
        };

        let (peer, profile, challenge_mac) = {
            let mut transfers = self.transfers.lock().await;
            let Some(record) = transfers.get_mut(&id) else {
                return;
            };
            if record.state != TransferState::Transferring {
                return;
            }
            let Some(challenge) = record.challenge.take() else {
                return;
            };
            let mac = auth::compute_stream_mac(
                record.profile,
                &challenge,
                &record.peer,
                &self.local_fingerprint,
                &id,
            );
            (record.peer, record.profile, mac)
        };

        let mut io = match dialer.dial(&peer, profile).await {
            Ok(io) => io,
            Err(e) => {
                tracing::warn!(transfer = %id, error = %e, "could not open a data stream");
                self.fail(id, FailureReason::Transport).await;
                return;
            }
        };

        let auth_frame = pb::DataStreamAuth {
            protocol_version: pliwee_core::session::PROTOCOL_VERSION_MAX,
            transfer_id: id.to_vec(),
            mac: challenge_mac.to_vec(),
        };
        if stream::write_frame(&mut io, &auth_frame).await.is_err() {
            self.fail(id, FailureReason::Transport).await;
            return;
        }

        let ready: pb::DataStreamReady = match stream::read_frame(&mut io).await {
            Ok(r) => r,
            Err(_) => {
                self.fail(id, FailureReason::Transport).await;
                return;
            }
        };
        if ready.status() != pb::DataStreamStatus::Ready {
            self.fail(id, transfer::reason_from_proto(ready.reason()))
                .await;
            return;
        }

        {
            let mut transfers = self.transfers.lock().await;
            if let Some(record) = transfers.get_mut(&id) {
                record.stream_active = true;
            }
        }

        let _ = self.run_stream(id, io).await;
    }

    /// Moves the bytes, in whichever direction this transfer runs.
    async fn run_stream(&self, id: TransferId, io: Box<dyn DataStreamIo>) -> Result<()> {
        let plan = {
            let transfers = self.transfers.lock().await;
            let Some(record) = transfers.get(&id) else {
                return Ok(());
            };
            StreamPlan {
                direction: record.direction,
                size_bytes: record.size_bytes,
                sha256: record.sha256,
                source: record.source.clone(),
                temp: record.temp.clone(),
                filename: record.filename.clone(),
                bytes: Arc::clone(&record.bytes),
                cancel: record.cancel.subscribe(),
                session: record.session.clone(),
            }
        };

        match plan.direction {
            Direction::Sending => self.run_send(id, io, plan).await,
            Direction::Receiving => self.run_receive(id, io, plan).await,
        }
    }
}

struct StreamPlan {
    direction: Direction,
    size_bytes: u64,
    sha256: [u8; 32],
    source: Option<PathBuf>,
    temp: Option<PathBuf>,
    filename: String,
    bytes: Arc<AtomicU64>,
    cancel: watch::Receiver<bool>,
    session: mpsc::Sender<OutboundMessage>,
}

impl TransferManager {
    /// The sending half of a running data stream.
    async fn run_send(
        &self,
        id: TransferId,
        mut io: Box<dyn DataStreamIo>,
        mut plan: StreamPlan,
    ) -> Result<()> {
        let Some(source) = plan.source else {
            self.fail(id, FailureReason::Storage).await;
            return Ok(());
        };

        let mut file = match tokio::fs::File::open(&source).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(transfer = %id, error = %e, "could not read the file to send");
                self.fail(id, FailureReason::Storage).await;
                return Ok(());
            }
        };

        let result = stream::send_exactly(
            &mut file,
            &mut io,
            plan.size_bytes,
            &plan.bytes,
            &mut plan.cancel,
        )
        .await;

        // Whatever happened, this stream is done.
        {
            let mut transfers = self.transfers.lock().await;
            if let Some(record) = transfers.get_mut(&id) {
                record.stream_active = false;
                // The receiver still has to hash and store it; give it a
                // bounded while to say so.
                record.deadline = Instant::now() + VERDICT_TIMEOUT;
            }
        }

        match result {
            Ok(()) => {
                tracing::info!(transfer = %id, bytes = plan.size_bytes, "sent; awaiting the receiver's verdict");
                // Deliberately no state change: only the receiver's
                // FILE_COMPLETE completes a send. Declaring success because
                // our own write finished would report a file as delivered
                // that the other side may have rejected.
            }
            // A transport error is the one failure whose real cause may still
            // be in flight: a receiver that cancels closes the data stream
            // *and* sends FILE_CANCEL, and the stream's end normally arrives
            // first. Give the peer's verdict a bounded moment to land before
            // attributing the stop to the network.
            Err(FailureReason::Transport) => {
                if !self.peer_verdict_arrives(id).await {
                    self.fail(id, FailureReason::Transport).await;
                }
            }
            Err(reason) => self.fail(id, reason).await,
        }
        Ok(())
    }

    /// Waits up to [`PEER_VERDICT_GRACE`] for the peer's own reason to make
    /// this transfer terminal. Returns whether it did.
    async fn peer_verdict_arrives(&self, id: TransferId) -> bool {
        let deadline = Instant::now() + PEER_VERDICT_GRACE;
        loop {
            {
                let transfers = self.transfers.lock().await;
                match transfers.get(&id) {
                    // Gone entirely: nothing left to attribute.
                    None => return true,
                    Some(record) if record.state.is_terminal() => return true,
                    Some(_) => {}
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(PEER_VERDICT_POLL).await;
        }
    }

    /// The receiving half of a running data stream.
    ///
    /// The order here is the whole integrity story: bytes go to a temp file,
    /// the hash is checked, and only then is anything promoted to a name the
    /// user will see. A file that fails its hash is never, at any instant,
    /// visible under its final name.
    async fn run_receive(
        &self,
        id: TransferId,
        mut io: Box<dyn DataStreamIo>,
        mut plan: StreamPlan,
    ) -> Result<()> {
        let Some(temp) = plan.temp.clone() else {
            self.fail(id, FailureReason::Storage).await;
            return Ok(());
        };

        let mut file = match tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&temp)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(transfer = %id, error = %e, "could not open the partial file");
                self.fail(id, FailureReason::Storage).await;
                return Ok(());
            }
        };

        let digest = stream::receive_exactly(
            &mut io,
            &mut file,
            plan.size_bytes,
            &plan.bytes,
            &mut plan.cancel,
        )
        .await;

        {
            let mut transfers = self.transfers.lock().await;
            if let Some(record) = transfers.get_mut(&id) {
                record.stream_active = false;
            }
        }

        let digest = match digest {
            Ok(d) => d,
            Err(reason) => {
                self.fail(id, reason).await;
                return Ok(());
            }
        };

        // Flush to the filesystem before claiming the file is stored.
        if file.sync_all().await.is_err() {
            self.fail(id, FailureReason::Storage).await;
            return Ok(());
        }
        drop(file);

        if !self.transition(id, TransferState::Verifying, None).await {
            return Ok(());
        }

        // Constant-time is not required — both values are already ours and
        // neither is a secret — but a mismatch must be absolute.
        if digest != plan.sha256 {
            tracing::warn!(
                transfer = %id,
                "the received data did not match the offered hash; discarding it"
            );
            // `fail` removes the temp file, so the bad bytes do not survive.
            self.fail(id, FailureReason::Integrity).await;
            return Ok(());
        }

        let destination = self.config.read().await.destination.clone();
        let filename = plan.filename.clone();
        let temp_for_move = temp.clone();
        let promoted =
            tokio::task::spawn_blocking(move || destination.promote(&temp_for_move, &filename))
                .await;

        let final_path = match promoted {
            Ok(Ok(path)) => path,
            Ok(Err(e)) => {
                tracing::warn!(transfer = %id, error = %e, "could not store the verified file");
                self.fail(id, FailureReason::Storage).await;
                return Ok(());
            }
            Err(_) => {
                self.fail(id, FailureReason::Storage).await;
                return Ok(());
            }
        };

        {
            let mut transfers = self.transfers.lock().await;
            if let Some(record) = transfers.get_mut(&id) {
                // The temp file is gone: it *became* the final file.
                record.temp = None;
                record.stored_at = Some(final_path.clone());
            }
        }

        if !self.transition(id, TransferState::Completed, None).await {
            return Ok(());
        }

        tracing::info!(
            transfer = %id,
            filename = %plan.filename,
            bytes = plan.size_bytes,
            "received, verified and stored"
        );

        send_control(
            &plan.session,
            pb::file_control::Body::Complete(pb::FileComplete {
                transfer_id: id.to_vec(),
            }),
        )
        .await;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The reaper
// ---------------------------------------------------------------------------

impl TransferManager {
    /// Starts the background task that ends transfers which cannot finish.
    ///
    /// Three things it catches, none of which any other component would:
    ///
    /// * **a dead control session** (F14). The data stream is a *separate*
    ///   TCP connection, so a control session that dies does not break it.
    ///   Without this, a transfer whose session vanished would keep running,
    ///   or sit in `Transferring` forever — the "zombie transfer" that the
    ///   brief forbids.
    /// * **a timeout** (F6). An offer nobody answered, an acceptance nobody
    ///   dialled, a verification that hung.
    /// * **a revoked peer** (F15), as a backstop behind the deterministic
    ///   [`cancel_peer`] call that revocation makes.
    ///
    /// [`cancel_peer`]: Self::cancel_peer
    pub fn spawn_reaper(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(REAP_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                manager.reap_once().await;
            }
        })
    }

    /// One pass of the reaper. Separated so tests can drive it directly
    /// rather than waiting on a timer.
    pub async fn reap_once(&self) {
        let now = Instant::now();

        let mut doomed: Vec<(TransferId, FailureReason)> = Vec::new();
        let mut to_check: Vec<(TransferId, Fingerprint)> = Vec::new();

        {
            let transfers = self.transfers.lock().await;
            for record in transfers.values() {
                if !record.state.is_active() {
                    continue;
                }
                if record.session.is_closed() {
                    doomed.push((record.id, FailureReason::Transport));
                    continue;
                }
                if !record.stream_active && now >= record.deadline {
                    doomed.push((record.id, FailureReason::TimedOut));
                    continue;
                }
                to_check.push((record.id, record.peer));
            }
        }

        for (id, reason) in doomed {
            self.fail(id, reason).await;
        }

        // Authorization is re-asked outside the lock: the host's answer may
        // touch a store with a lock of its own, and holding two in a fixed
        // order across an await is how deadlocks are built.
        for (id, peer) in to_check {
            if !self.authorized(&peer).await {
                self.fail(id, FailureReason::Revoked).await;
            }
        }
    }

    /// Drops finished transfers older than `keep`.
    ///
    /// Bounded because a long-lived daemon would otherwise accumulate a
    /// record per file ever transferred — which is also a transfer history,
    /// and this project does not keep one (see `store.rs`). Nothing is
    /// written to disk; this only bounds memory.
    pub async fn forget_finished(&self, keep: usize) {
        let mut transfers = self.transfers.lock().await;
        let mut finished: Vec<(u64, TransferId)> = transfers
            .values()
            .filter(|r| r.state.is_terminal())
            .map(|r| (r.seq, r.id))
            .collect();
        if finished.len() <= keep {
            return;
        }
        // Oldest first, then drop everything past the newest `keep`.
        finished.sort_unstable();
        let drop_count = finished.len() - keep;
        for (_, id) in finished.into_iter().take(drop_count) {
            transfers.remove(&id);
        }
    }
}

// ---------------------------------------------------------------------------
// The capability
// ---------------------------------------------------------------------------

/// The `files.v1` handler.
///
/// Thin on purpose: it decodes nothing and decides nothing, it hands the
/// payload to the [`TransferManager`] along with the identity the transport
/// established. All the policy is in one place.
pub struct FilesCapability {
    manager: Arc<TransferManager>,
}

impl FilesCapability {
    pub fn new(manager: Arc<TransferManager>) -> Self {
        Self { manager }
    }

    pub fn manager(&self) -> Arc<TransferManager> {
        Arc::clone(&self.manager)
    }
}

#[async_trait::async_trait]
impl Capability for FilesCapability {
    fn id(&self) -> &str {
        CAPABILITY_ID
    }

    async fn on_message(&self, ctx: &CapabilityContext, payload: &[u8]) -> Result<()> {
        self.manager
            .handle_control(
                ctx.peer,
                ctx.peer_device_id.clone(),
                ctx.outbound.clone(),
                ctx.profile,
                payload,
            )
            .await
    }

    async fn on_peer_connected(&self, ctx: &CapabilityContext) -> Result<()> {
        // Recorded so a transfer can be started from outside a session. This
        // runs only when `files.v1` was both mutually supported and granted,
        // so an ungranted peer never gets an entry and `pliwee send` to it
        // fails with "not connected" rather than silently doing nothing.
        self.manager
            .attach_session(ctx.peer, ctx.outbound.clone(), ctx.profile)
            .await;
        Ok(())
    }

    async fn on_peer_disconnected(&self, peer: &Fingerprint) -> Result<()> {
        // Not the mechanism that cleans up after a disconnect — the reaper
        // is, because it can tell one session from another and this callback
        // cannot. (A peer that reconnects produces a *new* session's connect
        // before the *old* session's disconnect, so acting on fingerprint
        // alone here would kill the new session's transfers.) This is only a
        // log line.
        tracing::debug!(
            peer = %peer.to_display_short(),
            "session ended; the reaper will close any transfer that used it"
        );
        Ok(())
    }
}
