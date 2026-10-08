//! Daemon-wide shared state, and the [`SessionHost`] implementation that
//! connects the protocol engine to the trust store and the human.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use pliwee_capability_clipboard::{ClipboardAuthorizer, ClipboardManager, ClipboardPolicy};
use pliwee_capability_files::{FilesAuthorizer, TransferManager};
use pliwee_capability_notifications::{NotificationAuthorizer, NotificationManager};
use pliwee_core::capability::CapabilityRegistry;
use pliwee_core::error::{PairingError, Result};
use pliwee_core::notification_policy::NotificationPolicy;
use pliwee_core::pairing::PairingSession;
use pliwee_core::session::{PeerStatus, SessionHandle, SessionHost, SessionId};
use pliwee_core::store::{Store, TrustedPeer};
use pliwee_core::Fingerprint;
use pliwee_proto::v1;
use tokio::sync::{oneshot, Mutex, RwLock};

use crate::renegotiate::{Decision, LiveSession, Renegotiation};

/// A pending "is this device you?" question waiting on a human.
pub struct ConfirmRequest {
    pub device: v1::DeviceInfo,
    pub fingerprint: Fingerprint,
    pub reply: oneshot::Sender<bool>,
}

pub struct DaemonState {
    pub store: Mutex<Store>,
    pub registry: CapabilityRegistry,
    pub battery: Arc<pliwee_capability_battery::BatteryState>,
    /// `files.v1`, when the capability is enabled. `None` leaves the daemon
    /// with no file transfer at all rather than a half-wired one.
    pub transfers: Option<Arc<TransferManager>>,
    /// `clipboard.v1`. Same reasoning as `transfers`: absent rather than
    /// half-wired.
    pub clipboard: Option<Arc<ClipboardManager>>,
    /// `notifications.v1`. Same reasoning again. `None` is also what a daemon
    /// composed without a notification sink gets — which is a normal state,
    /// not a broken one: the capability is simply not registered.
    pub notifications: Option<Arc<NotificationManager>>,
    /// Where an incoming file offer goes to be put to a human.
    ///
    /// The *same* object `files.v1` was constructed with, held here so the
    /// control server can attach a provider to it. It is not a second
    /// approval path: the capability asks this and only this, and a daemon
    /// composed without it simply has no way for a graphical client to
    /// answer — which is the pre-existing headless behaviour, unchanged.
    pub file_approval: Option<Arc<crate::approval::FileApproval>>,

    /// The single open pairing window, if any.
    pairing: Mutex<Option<PairingSession>>,
    /// Where to send confirmation questions. Present only while a `pliwee
    /// pair` control session is attached: with no operator watching there is
    /// nobody to answer, and auto-accepting would defeat the whole point.
    confirm_tx: Mutex<Option<tokio::sync::mpsc::Sender<ConfirmRequest>>>,

    sessions: RwLock<HashMap<Fingerprint, SessionHandle>>,

    /// Which peers have been asked to reconnect because a grant widened past
    /// what their live session negotiated. See [`crate::renegotiate`].
    renegotiation: Renegotiation,

    /// When each peer's last session ended, for reporting a device as
    /// disconnected-since rather than merely absent. In memory only: it is
    /// operational display data, not something worth writing to disk.
    last_seen: RwLock<HashMap<Fingerprint, SystemTime>>,

    device_info: v1::DeviceInfo,

    /// The port actually bound at startup, which is not necessarily the one
    /// in settings: a `--port` override or a port-0 bind both change it, and
    /// advertising the wrong one would hand out a QR code nobody can dial.
    listen_port: std::sync::atomic::AtomicU16,

    /// The address families the listener really accepts on, for reporting.
    listen_families: std::sync::OnceLock<String>,

    /// What startup found about where the local state came from. Set once by
    /// the agent before it serves anything; reported by `status`.
    local_state: std::sync::OnceLock<LocalStateReport>,
}

/// Facts about this machine's local state that only startup can know.
#[derive(Debug, Clone, Default)]
pub struct LocalStateReport {
    /// The identity was carried over from an OmniBridge data directory.
    pub migrated_from: Option<crate::control::MigrationReport>,
    /// Leftover `.omnibridge-*.part` files. Listed, never removed.
    pub legacy_partial_files: Vec<String>,
}

impl DaemonState {
    pub fn new(
        store: Store,
        registry: CapabilityRegistry,
        battery: Arc<pliwee_capability_battery::BatteryState>,
    ) -> Self {
        let device_info = store.identity().device_info();
        Self {
            store: Mutex::new(store),
            registry,
            battery,
            transfers: None,
            clipboard: None,
            notifications: None,
            file_approval: None,
            pairing: Mutex::new(None),
            confirm_tx: Mutex::new(None),
            sessions: RwLock::new(HashMap::new()),
            renegotiation: Renegotiation::new(),
            last_seen: RwLock::new(HashMap::new()),
            device_info,
            listen_port: std::sync::atomic::AtomicU16::new(0),
            listen_families: std::sync::OnceLock::new(),
            local_state: std::sync::OnceLock::new(),
        }
    }

    /// Records what startup found about the local state. First call wins.
    pub fn set_local_state(&self, report: LocalStateReport) {
        let _ = self.local_state.set(report);
    }

    /// What startup found, or nothing to report.
    pub fn local_state(&self) -> LocalStateReport {
        self.local_state.get().cloned().unwrap_or_default()
    }

    /// Attaches the file-transfer manager.
    ///
    /// Separate from [`new`] because the manager needs this state as its
    /// authorizer, and this state needs the manager: one of the two has to be
    /// built first. The daemon builds the manager, constructs the state with
    /// it, and then hands the state back as the authorizer.
    ///
    /// [`new`]: Self::new
    pub fn with_transfers(mut self, transfers: Arc<TransferManager>) -> Self {
        self.transfers = Some(transfers);
        self
    }

    /// Attaches the clipboard manager. Separate from [`new`] for the same
    /// circular-construction reason as [`with_transfers`].
    ///
    /// [`new`]: Self::new
    /// [`with_transfers`]: Self::with_transfers
    pub fn with_clipboard(mut self, clipboard: Arc<ClipboardManager>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    /// Attaches the notification manager. Same circular-construction reason
    /// as [`with_transfers`] and [`with_clipboard`].
    ///
    /// [`with_transfers`]: Self::with_transfers
    /// [`with_clipboard`]: Self::with_clipboard
    pub fn with_notifications(mut self, notifications: Arc<NotificationManager>) -> Self {
        self.notifications = Some(notifications);
        self
    }

    /// Attaches the file-approval seam.
    ///
    /// Takes the very `Arc` that was handed to [`TransferManager::new`] as
    /// its [`TransferApproval`]. Passing a *different* instance here would
    /// compile and would be silently useless — the control server would
    /// attach a provider to an object nothing ever asks — so the daemon
    /// builds one and clones it.
    ///
    /// [`TransferApproval`]: pliwee_capability_files::TransferApproval
    pub fn with_file_approval(mut self, approval: Arc<crate::approval::FileApproval>) -> Self {
        self.file_approval = Some(approval);
        self
    }

    /// Closes every notification a peer has on this screen, now.
    ///
    /// Called from every path that withdraws a `notifications.v1` grant or
    /// revokes a pairing. The inbound authorization needs no telling — it is
    /// re-read from the trust store on every message — but the notifications
    /// **already on the screen** are state this daemon put there, and leaving
    /// them up after the user said "not this device" would mean a revocation
    /// that took effect only for notifications that had not arrived yet.
    ///
    /// There is deliberately no grace here. A grace is for a peer that may
    /// come back; a revoked peer is one the user has just said should not be
    /// on this screen.
    pub async fn notify_notifications_revoked(&self, peer: &Fingerprint) {
        if let Some(notifications) = &self.notifications {
            notifications.revoke_peer(peer).await;
        }
    }

    /// Tells the clipboard watcher that a grant or policy may have changed.
    ///
    /// Called from every path that edits a grant, a policy or a pairing. It
    /// is what makes "revoke stops future sync immediately" true for the
    /// outbound direction: the watcher re-reads the peer list and stops
    /// entirely once nobody is left who wants it.
    pub fn notify_clipboard_policy_changed(&self) {
        if let Some(clipboard) = &self.clipboard {
            clipboard.policy_changed();
        }
    }

    /// Records the port the listener actually bound.
    pub fn set_listen_port(&self, port: u16) {
        self.listen_port
            .store(port, std::sync::atomic::Ordering::Relaxed);
    }

    /// Records which address families the listener accepts on.
    pub fn set_listen_families(&self, families: crate::listener::Families) {
        let _ = self.listen_families.set(families.to_string());
    }

    /// The address families the listener accepts on, or `"unknown"` before
    /// the listener has reported in.
    pub fn listen_families(&self) -> String {
        self.listen_families
            .get()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// The bound port, falling back to the configured one before the
    /// listener has started.
    pub async fn listen_port(&self) -> u16 {
        match self.listen_port.load(std::sync::atomic::Ordering::Relaxed) {
            0 => self.store.lock().await.settings().listen_port,
            port => port,
        }
    }

    /// Opens a pairing window, replacing any previous one.
    ///
    /// Replacing rather than refusing is deliberate: the previous window's
    /// token is dropped (and zeroed), so a stale QR left on screen stops
    /// working the moment a new one is generated.
    pub async fn begin_pairing(
        &self,
        ttl: Duration,
        confirm_tx: tokio::sync::mpsc::Sender<ConfirmRequest>,
    ) -> Result<String> {
        let session = PairingSession::new(ttl)?;
        let token_b32 = session.token().to_base32();
        *self.pairing.lock().await = Some(session);
        *self.confirm_tx.lock().await = Some(confirm_tx);
        Ok(token_b32)
    }

    /// Closes the pairing window and detaches the confirmation channel.
    pub async fn end_pairing(&self) {
        *self.pairing.lock().await = None;
        *self.confirm_tx.lock().await = None;
    }

    pub async fn pairing_remaining(&self) -> Option<Duration> {
        self.pairing
            .lock()
            .await
            .as_ref()
            .filter(|s| !s.is_exhausted())
            .map(|s| s.remaining())
    }

    /// Registers a session as *the* session for its peer.
    ///
    /// One peer means one session. When a phone reconnects after a network
    /// drop its previous socket is often still half-open on this side, so a
    /// second session arrives while the first has not noticed it is dead.
    /// The older one is shut down here rather than left to time out, so the
    /// daemon never holds two sessions with one device.
    pub async fn register_session(&self, handle: SessionHandle) {
        // Whatever reconnect was asked for on this peer's behalf, it is over:
        // a session has come up, and it is this one that any further grant
        // change will be judged against.
        self.renegotiation.clear(&handle.peer()).await;
        let displaced = {
            let mut sessions = self.sessions.write().await;
            sessions.insert(handle.peer(), handle)
        };
        if let Some(old) = displaced {
            tracing::info!(
                peer = %old.peer().to_display_short(),
                session = old.id(),
                "replaced by a newer session; closing the old one"
            );
            // Not awaited under the map lock: `shutdown` waits on the old
            // session's command channel, and that session may itself be
            // trying to take the same lock to unregister.
            tokio::spawn(async move { old.shutdown().await });
        }
    }

    /// Removes a session, but only if it is still the registered one.
    ///
    /// The `session_id` check is the whole point: a superseded session's
    /// close arrives *after* its replacement registered, and removing by
    /// fingerprint alone would evict the live session and report a connected
    /// device as offline.
    pub async fn unregister_session(&self, peer: &Fingerprint, session_id: SessionId) {
        let mut sessions = self.sessions.write().await;
        if sessions.get(peer).is_some_and(|h| h.id() == session_id) {
            sessions.remove(peer);
            drop(sessions);
            self.last_seen
                .write()
                .await
                .insert(*peer, SystemTime::now());
        }
    }

    /// Removes whatever session a peer has, regardless of its id. Used by
    /// revocation, where the intent is "this device has no session, full
    /// stop" rather than "this particular session ended".
    pub async fn drop_session(&self, peer: &Fingerprint) {
        let removed = self.sessions.write().await.remove(peer);
        if removed.is_some() {
            self.last_seen
                .write()
                .await
                .insert(*peer, SystemTime::now());
        }
    }

    /// When this peer's last session ended, if one ever did in this run.
    pub async fn last_seen(&self, peer: &Fingerprint) -> Option<SystemTime> {
        self.last_seen.read().await.get(peer).copied()
    }

    pub async fn session_handles(&self) -> Vec<SessionHandle> {
        self.sessions.read().await.values().cloned().collect()
    }

    pub async fn session_for(&self, peer: &Fingerprint) -> Option<SessionHandle> {
        self.sessions.read().await.get(peer).cloned()
    }

    /// Converges a live session on a grant that was made after its handshake.
    ///
    /// A session's capability set is fixed at `HELLO`, so a capability granted
    /// afterwards has no negotiated channel to announce a role on and no way
    /// to accept the peer's — which is exactly the state a person reached by
    /// enabling `notifications.v1` on a phone that was already connected, and
    /// could only leave by pressing Disconnect and Connect by hand (N3 §G4,
    /// N4 §17).
    ///
    /// The correction is to end that session. The peer's own connection
    /// coordinator redials on its ordinary transient backoff — nothing here
    /// retries, schedules or waits — and the new handshake reads the grant
    /// that now exists.
    ///
    /// Returns the decision so the caller can log why nothing happened, which
    /// is the more common and more confusing case.
    pub async fn renegotiate_after_grant(
        &self,
        peer: &Fingerprint,
        capability: &str,
        granted: bool,
    ) -> Decision {
        let handle = self.session_for(peer).await;
        let negotiated = handle
            .as_ref()
            .map(|h| h.negotiated_capabilities().to_vec())
            .unwrap_or_default();
        let session = handle.as_ref().map(|h| LiveSession {
            id: h.id(),
            negotiated: &negotiated,
        });

        let decision = self
            .renegotiation
            .on_grant_changed(peer, capability, granted, session)
            .await;

        if decision.is_reconnect() {
            if let Some(handle) = handle {
                tracing::info!(
                    peer = %peer.to_display_short(),
                    session = handle.id(),
                    capability,
                    "granted a capability this session cannot use; ending it so \
                     the device reconnects and negotiates again"
                );
                // Not awaited inline for the same reason `register_session`
                // does not await a displaced session's shutdown: the session
                // may be taking the locks this caller holds on its way out.
                tokio::spawn(async move { handle.shutdown().await });
            }
        } else {
            tracing::debug!(
                peer = %peer.to_display_short(),
                capability,
                decision = decision.as_str(),
                "no session renegotiation needed"
            );
        }

        decision
    }

    /// Forgets any outstanding reconnect request for a peer.
    pub async fn clear_renegotiation(&self, peer: &Fingerprint) {
        self.renegotiation.clear(peer).await;
    }

    pub fn device_info(&self) -> v1::DeviceInfo {
        self.device_info.clone()
    }

    /// Resolves a user-supplied device selector: an exact device id, or a
    /// case-insensitive fingerprint prefix of at least 8 hex characters.
    ///
    /// The minimum length exists so a one-character prefix cannot silently
    /// match the wrong device in a destructive command like `unpair`. An
    /// ambiguous prefix is an error, never a guess.
    pub async fn resolve_device(&self, selector: &str) -> std::result::Result<Fingerprint, String> {
        let store = self.store.lock().await;
        let needle = selector.trim().to_ascii_lowercase();

        // `listed_peers`, so a hidden tombstone can never be named by a
        // selector: it has no device id and no name to match on, it is not on
        // any screen a person could read a prefix off, and every command that
        // takes a selector is one that would make no sense against it.
        // The empty-id guard matters because a tombstone's device id *is*
        // empty — an empty selector must not resolve to one.
        if !needle.is_empty() {
            if let Some(p) = store.listed_peers().find(|p| p.device_id == needle) {
                return Ok(p.fingerprint);
            }
        }

        if needle.len() < 8 {
            return Err(format!(
                "no device with id '{selector}'; a fingerprint prefix must be \
                 at least 8 characters"
            ));
        }

        let matches: Vec<&TrustedPeer> = store
            .listed_peers()
            .filter(|p| p.fingerprint.to_hex().starts_with(&needle))
            .collect();

        match matches.as_slice() {
            [one] => Ok(one.fingerprint),
            [] => Err(format!("no device matches '{selector}'")),
            many => Err(format!(
                "'{}' is ambiguous: it matches {} devices",
                selector,
                many.len()
            )),
        }
    }
}

/// The `files.v1` grant check, asked fresh every time.
///
/// Reads the trust store rather than a cached set, so a revocation that
/// happened one second ago is already in force — including against a data
/// stream that is mid-copy.
#[async_trait::async_trait]
impl FilesAuthorizer for DaemonState {
    async fn is_authorized(&self, peer: &Fingerprint) -> bool {
        let store = self.store.lock().await;
        // `trusted_peer` already excludes revoked devices; `allows` re-checks
        // that and the per-capability grant.
        store
            .trusted_peer(peer)
            .is_some_and(|p| p.allows(pliwee_capability_files::CAPABILITY_ID))
    }
}

/// The `clipboard.v1` grant *and* policy check, asked fresh every time.
///
/// One call answers both questions on purpose. A caller that had to ask
/// "is it granted?" and "what is the policy?" separately could do the second
/// and forget the first, and the failure would be silent — a revoked device
/// whose stored policy still said `allow_receive`. Returning
/// [`ClipboardPolicy::DENIED`] for an unknown, revoked or ungranted peer makes
/// that mistake unrepresentable.
#[async_trait::async_trait]
impl ClipboardAuthorizer for DaemonState {
    async fn policy_for(&self, peer: &Fingerprint) -> ClipboardPolicy {
        let store = self.store.lock().await;
        match store.trusted_peer(peer) {
            // `trusted_peer` already excludes revoked devices; `allows`
            // re-checks that and the per-capability grant.
            Some(p) if p.allows(pliwee_capability_clipboard::CAPABILITY_ID) => p.clipboard_policy,
            _ => ClipboardPolicy::DENIED,
        }
    }

    async fn auto_send_peers(&self) -> Vec<Fingerprint> {
        let store = self.store.lock().await;
        store
            .peers()
            .filter(|p| p.allows(pliwee_capability_clipboard::CAPABILITY_ID))
            .filter(|p| p.clipboard_policy.may_auto_send())
            .map(|p| p.fingerprint)
            .collect()
    }
}

/// The `notifications.v1` grant *and* policy check, asked fresh every time.
///
/// One call answers both questions, for the reason [`ClipboardAuthorizer`]'s
/// does: a caller that had to ask "is it granted?" and "what is the policy?"
/// separately could do the second and forget the first, and the failure would
/// be silent — a revoked device whose stored policy still said `allow_mirror`.
/// Returning [`NotificationPolicy::DENIED`] for an unknown, revoked or
/// ungranted peer makes that mistake unrepresentable.
///
/// It matters more here than it does for the clipboard. The transport filters
/// the negotiated capability list against the grant when the session is
/// *built*, so a grant withdrawn afterwards is not noticed there at all; this
/// is the only thing standing between a revoked device and the screen.
#[async_trait::async_trait]
impl NotificationAuthorizer for DaemonState {
    async fn policy_for(&self, peer: &Fingerprint) -> NotificationPolicy {
        let store = self.store.lock().await;
        match store.trusted_peer(peer) {
            // `trusted_peer` already excludes revoked devices; `allows`
            // re-checks that and the per-capability grant.
            Some(p) if p.allows(pliwee_capability_notifications::CAPABILITY_ID) => {
                p.notification_policy
            }
            _ => NotificationPolicy::DENIED,
        }
    }
}

#[async_trait::async_trait]
impl SessionHost for DaemonState {
    fn local_device_info(&self) -> v1::DeviceInfo {
        self.device_info.clone()
    }

    fn registry(&self) -> CapabilityRegistry {
        self.registry.clone()
    }

    async fn lookup_peer(&self, fingerprint: &Fingerprint) -> PeerStatus {
        let store = self.store.lock().await;
        match store.peer_record(fingerprint) {
            None => PeerStatus::Unknown,
            Some(p) if p.revoked => PeerStatus::Revoked,
            Some(p) => PeerStatus::Trusted {
                device_id: p.device_id.clone(),
                device_name: p.device_name.clone(),
                granted_capabilities: p
                    .granted_capabilities
                    .iter()
                    .filter(|(_, granted)| **granted)
                    .map(|(id, _)| id.clone())
                    .collect(),
            },
        }
    }

    async fn pairing_mode_active(&self) -> bool {
        // A window with no operator attached is not usable, so it does not
        // count as active.
        let has_operator = self.confirm_tx.lock().await.is_some();
        let window_open = self
            .pairing
            .lock()
            .await
            .as_ref()
            .is_some_and(|s| !s.is_exhausted());
        has_operator && window_open
    }

    async fn verify_pairing_proof(
        &self,
        profile: pliwee_core::Profile,
        initiator: &Fingerprint,
        nonce: &[u8],
        proof: &[u8],
    ) -> std::result::Result<[u8; 32], PairingError> {
        let responder = self.device_info.identity_fingerprint.clone();
        let responder = Fingerprint::from_hex(&responder).map_err(|_| PairingError::BadProof)?;

        let mut guard = self.pairing.lock().await;
        let Some(session) = guard.as_mut() else {
            return Err(PairingError::NotInPairingMode);
        };

        let result = session.verify_and_consume(profile, &responder, initiator, nonce, proof);

        // Once the window can no longer be used, drop it immediately so its
        // token is zeroed rather than lingering in memory.
        if session.is_exhausted() {
            *guard = None;
        }
        result
    }

    async fn confirm_pairing(&self, device: &v1::DeviceInfo, fingerprint: &Fingerprint) -> bool {
        let tx = { self.confirm_tx.lock().await.clone() };
        let Some(tx) = tx else {
            return false;
        };

        let (reply, rx) = oneshot::channel();
        let request = ConfirmRequest {
            device: device.clone(),
            fingerprint: *fingerprint,
            reply,
        };
        if tx.send(request).await.is_err() {
            return false;
        }
        rx.await.unwrap_or(false)
    }

    async fn store_peer(
        &self,
        device: &v1::DeviceInfo,
        fingerprint: &Fingerprint,
        negotiated_capabilities: &[String],
        protocol_version: u32,
    ) -> Result<()> {
        let mut store = self.store.lock().await;
        let auto_grant = store.settings().auto_grant.clone();

        // Grants are intersected with the local auto-grant policy. A peer
        // advertising a capability never grants itself that capability.
        let granted = negotiated_capabilities
            .iter()
            .map(|c| (c.clone(), auto_grant.iter().any(|a| a == c)))
            .collect();

        let peer = TrustedPeer {
            device_id: device.device_id.clone(),
            device_name: pliwee_core::discovery::sanitize_device_name(&device.device_name),
            platform: device.platform,
            fingerprint: *fingerprint,
            paired_at_unix: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            granted_capabilities: granted,
            last_protocol_version: protocol_version,
            revoked: false,
            // A fresh pairing is always a visible record. Pairing over a
            // hidden tombstone therefore brings the device back into the
            // list, and brings back nothing else: this literal is the whole
            // record, so no grant, policy or name from the old relationship
            // can survive here.
            hidden: false,
            // Safe defaults. `clipboard.v1` is not in `auto_grant`, so every
            // flag here is inert until someone grants the capability by hand
            // — and even then the two automatic directions stay off.
            clipboard_policy: ClipboardPolicy::default(),
            // Same again for `notifications.v1`, which is also absent from
            // `auto_grant`: nothing here does anything until a human grants
            // the capability, and a locked desktop then shows an app name
            // rather than a message.
            notification_policy: NotificationPolicy::default(),
            // A pairing proves a key, not a route. Hints are learned only
            // from later authenticated sessions (Mesh V2 SPEC §5).
            address_hints: Vec::new(),
        };
        store.add_peer(peer)
    }

    async fn on_established(&self, _peer: &Fingerprint, handle: SessionHandle) {
        self.register_session(handle).await;
    }

    async fn on_closed(&self, peer: &Fingerprint, session_id: SessionId) {
        self.unregister_session(peer, session_id).await;
    }
}
