//! Persistent state: local identity, trusted peers, per-capability grants.
//!
//! # What is stored
//!
//! Only what the spec allows: identity, paired devices, their pinned public
//! keys, capability permissions, settings, and up to [`MAX_ADDRESS_HINTS`]
//! recently successful addresses per trusted peer (Mesh V2 SPEC §5). No
//! clipboard content, no transfer history, no message log.
//!
//! # Format
//!
//! JSON with an explicit top-level `schema_version`. JSON because the state
//! is tiny (tens of records), human-auditable — which matters a lot for a
//! file that decides who is trusted — and trivially migratable. The version
//! field exists from commit one precisely so migrations are possible later.
//!
//! # On-disk protection
//!
//! The private key lives in `identity.key` with mode 0600 inside a 0700
//! directory under `$XDG_DATA_HOME`. On load we *verify* those modes and
//! refuse to start if they are loose, rather than silently continuing with a
//! world-readable key.
//!
//! We deliberately do not use the Secret Service (gnome-keyring) API here: a
//! `systemd --user` daemon can start before any keyring is unlocked, and a
//! daemon that blocks on a locked keyring at boot is worse than useless.
//! See ADR-0006 for the full trade-off and the TPM2 follow-up.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::clipboard_policy::ClipboardPolicy;
use crate::error::{Error, Result};
use crate::fingerprint::Fingerprint;
#[cfg(feature = "unix-fs")]
use crate::identity::SoftwareBacking;
use crate::identity::{Identity, IdentityBackend, IdentityState, KeyBacking};
use crate::notification_policy::NotificationPolicy;
use crate::secret_store::{SecretStore, StoreAccessError};

/// Bumped from 1 to 2 by Wave 0, which added `key_backing`.
///
/// Reading is one-way compatible by existing design: [`Store::open_with`]
/// refuses a `schema_version` *newer* than this one, and a version 1 file has
/// no `key_backing`, which defaults to [`KeyBacking::Software`] — correct for
/// every identity that exists today, because no other backing has ever been
/// implemented. A pre-Wave-0 install therefore upgrades in place: same
/// identity, same `device_id`, same fingerprint, same peers, no re-pairing.
pub const SCHEMA_VERSION: u32 = 2;

/// A device this one has paired with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub device_id: String,
    pub device_name: String,
    pub platform: i32,
    /// The pinned SPKI fingerprint. This, and only this, decides identity.
    pub fingerprint: Fingerprint,
    pub paired_at_unix: i64,
    /// Capabilities this peer is allowed to use, independent of what it
    /// advertises. An empty map means "nothing granted yet".
    #[serde(default)]
    pub granted_capabilities: BTreeMap<String, bool>,
    /// Highest protocol version ever negotiated with this peer. Recorded so a
    /// future release can detect and refuse a silent downgrade.
    #[serde(default)]
    pub last_protocol_version: u32,
    /// Set instead of deleting the record, so a revoked device stays
    /// recognisable and cannot silently re-pair without the user noticing.
    #[serde(default)]
    pub revoked: bool,
    /// Whether the user has taken this revoked record out of the ordinary
    /// device lists — the *tombstone* half of a revocation.
    ///
    /// Presentation, never admission. A hidden record is invisible to
    /// [`Store::listed_peers`] and therefore to Settings, the CLI device list
    /// and every selector, and it is still returned by
    /// [`Store::peer_record`], which is what [`crate::session`] asks when it
    /// decides whether to admit a handshake. Deleting the record instead
    /// would turn `REVOKED` into `UNKNOWN`: the same key would be greeted
    /// with `PAIRING_REQUIRED` and this daemon's capability list rather than
    /// `REJECTED`, and nothing would remain to say it had ever been thrown
    /// out.
    ///
    /// `#[serde(default)]` for the reason every other flag here has one: a
    /// trust store written before this field existed has no such key, and
    /// every record in it is a *visible* one. Migration is therefore the
    /// absence of a decision, which is right — hiding somebody's revoked
    /// devices without being asked is not a migration, it is a change of
    /// meaning.
    ///
    /// Hidden implies revoked, and [`Store::load`] enforces that on the way
    /// in rather than trusting the file: a record that says hidden but not
    /// revoked is the one combination that would be dangerous to believe, so
    /// it is read as revoked.
    #[serde(default)]
    pub hidden: bool,
    /// Per-peer `clipboard.v1` direction and automation settings.
    ///
    /// Stored next to the grant but deliberately separate from it: the grant
    /// says whether this device may speak clipboard at all, and this says in
    /// which directions and how automatically. Both are decided locally — no
    /// protocol message writes either — and neither holds clipboard content.
    ///
    /// `#[serde(default)]` matters here: a trust store written before this
    /// capability existed has no such object, and it must deserialize to the
    /// safe defaults (automatic directions off) rather than failing the load
    /// or, worse, defaulting to `false` across the board and silently
    /// disabling a direction the user had enabled.
    #[serde(default)]
    pub clipboard_policy: ClipboardPolicy,

    /// Per-peer `notifications.v1` display policy.
    ///
    /// Same structure and same reasoning as `clipboard_policy`: the grant says
    /// whether this device may speak notifications at all, and this says how
    /// much of each notification is displayed here and when. Both are decided
    /// locally — no protocol message writes either — and **neither holds a
    /// notification's title, body or any other content**. There is no field on
    /// this type that could.
    ///
    /// `#[serde(default)]` for the reason above it: every trust store in the
    /// field predates this capability and must load with the safe defaults
    /// rather than failing, or silently reading `false` across the board.
    #[serde(default)]
    pub notification_policy: NotificationPolicy,

    /// Addresses this peer was recently reached at, most recent first.
    ///
    /// Routing hints, never identity (Mesh V2 SPEC §5). Nothing that decides
    /// admission, authorization or a grant reads this field: the fingerprint
    /// above is the identity, and a dial to any of these addresses still has
    /// to present that pin. A hint that points at the wrong machine therefore
    /// costs a failed handshake and nothing else.
    ///
    /// At most [`MAX_ADDRESS_HINTS`], written only through
    /// [`Store::record_address_success`]. Empty on a revoked record: a device
    /// that is no longer trusted has nowhere this one should go looking for it.
    ///
    /// `#[serde(default)]` because every store written before Mesh V2 has no
    /// such key (SPEC §11: additive, default empty), and read leniently
    /// because hints may be discarded safely — a malformed entry is dropped
    /// rather than allowed to make the trust store unreadable.
    #[serde(default, deserialize_with = "lenient_address_hints")]
    pub address_hints: Vec<AddressHint>,
}

/// The most address hints kept for one peer (Mesh V2 SPEC §5).
pub const MAX_ADDRESS_HINTS: usize = 8;

/// One address a trusted peer was successfully reached at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressHint {
    /// Normalized: an IPv4-mapped IPv6 address is stored as the IPv4 address
    /// it maps, so the same machine reached both ways is one hint.
    pub address: IpAddr,
    pub port: u16,
    /// When the last successful connection to this address happened.
    ///
    /// Informational, for display. Eviction order is the order of the list,
    /// not this value, so a wall clock that steps backwards cannot evict the
    /// address that has just worked.
    pub last_success_unix: i64,
}

impl AddressHint {
    /// Builds a hint, or `None` for an address nothing could dial.
    ///
    /// IPv6 link-local addresses are refused: without the scope id they
    /// cannot be dialled, and the scope id is an interface index local to
    /// this boot, not a property of the peer.
    pub fn new(addr: SocketAddr, last_success_unix: i64) -> Option<Self> {
        let address = addr.ip().to_canonical();
        let unusable = addr.port() == 0
            || address.is_unspecified()
            || address.is_multicast()
            || address == IpAddr::V4(Ipv4Addr::BROADCAST)
            || matches!(address, IpAddr::V6(v6) if v6.is_unicast_link_local());
        if unusable {
            return None;
        }
        Some(Self {
            address,
            port: addr.port(),
            last_success_unix,
        })
    }

    /// The address to dial. Still subject to the SPKI pin.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.address, self.port)
    }

    /// Two hints are the same route when they name the same address and port.
    fn same_route(&self, other: &Self) -> bool {
        self.address == other.address && self.port == other.port
    }
}

/// Puts a hint list into the shape the store promises: normalized, usable,
/// de-duplicated keeping the first (most recent) occurrence, and bounded.
fn bound_address_hints(hints: Vec<AddressHint>) -> Vec<AddressHint> {
    let mut kept: Vec<AddressHint> = Vec::with_capacity(MAX_ADDRESS_HINTS);
    for hint in hints {
        let Some(hint) = AddressHint::new(hint.socket_addr(), hint.last_success_unix) else {
            continue;
        };
        if kept.iter().any(|k| k.same_route(&hint)) {
            continue;
        }
        kept.push(hint);
        if kept.len() == MAX_ADDRESS_HINTS {
            break;
        }
    }
    kept
}

/// Reads `address_hints` without ever failing the load.
///
/// A hint is disposable and a trust store is not: an entry that does not
/// parse is skipped, and a value that is not a list at all reads as no hints.
fn lenient_address_hints<'de, D>(deserializer: D) -> std::result::Result<Vec<AddressHint>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry {
        Hint(AddressHint),
        Other(serde::de::IgnoredAny),
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Field {
        List(Vec<Entry>),
        Other(serde::de::IgnoredAny),
    }

    Ok(match Field::deserialize(deserializer)? {
        Field::List(entries) => entries
            .into_iter()
            .filter_map(|e| match e {
                Entry::Hint(h) => Some(h),
                Entry::Other(_) => None,
            })
            .collect(),
        Field::Other(_) => Vec::new(),
    })
}

impl TrustedPeer {
    pub fn allows(&self, capability_id: &str) -> bool {
        !self.revoked
            && *self
                .granted_capabilities
                .get(capability_id)
                .unwrap_or(&false)
    }

    /// Whether this record belongs in an ordinary device list.
    ///
    /// The one place the question is answered, so that a screen, a report and
    /// a selector cannot disagree about it.
    pub fn is_listed(&self) -> bool {
        !self.hidden
    }

    /// The record reduced to the revocation itself.
    ///
    /// What survives is what admission reads: the pinned fingerprint, which
    /// *is* the identity, and `revoked`. Everything else is a fact about a
    /// relationship that has ended — the name shown in a list, the device id,
    /// the platform, when it was paired, what it was once allowed, the
    /// clipboard and notification settings someone chose for it — and none of
    /// it is needed to keep refusing the key. Dropping it is both the privacy
    /// answer and the security one: a grant that is not stored cannot come
    /// back on a re-pair.
    fn into_tombstone(self) -> Self {
        Self {
            device_id: String::new(),
            device_name: String::new(),
            platform: 0,
            fingerprint: self.fingerprint,
            paired_at_unix: 0,
            granted_capabilities: BTreeMap::new(),
            last_protocol_version: 0,
            revoked: true,
            hidden: true,
            // `DENIED`, not `default()`. Functionally the same — every
            // authorizer asks `trusted_peer` first and a tombstone is never
            // trusted — but `ClipboardPolicy::default()` serializes as
            // `allow_send: true` and `NotificationPolicy::default()` as
            // `allow_mirror: true`, because a policy's defaults are written
            // for a device somebody has granted something to. A tombstone
            // whose line in `state.json` reads `allow_mirror: true` is a
            // sentence an auditor has to reason their way out of, and one
            // that would become true if any future path ever read a policy
            // without asking about the grant. Writing the denied value costs
            // nothing and means the file says what is the case.
            clipboard_policy: ClipboardPolicy::DENIED,
            notification_policy: NotificationPolicy::DENIED,
            // Where a device used to be is history too.
            address_hints: Vec::new(),
        }
    }
}

/// What [`Store::hide_revoked_peer`] did.
///
/// Three outcomes rather than a `bool`, because "there is no such device" and
/// "that device is still trusted" need different words on screen and the
/// second must never be answered by hiding anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideOutcome {
    /// The record was revoked and is now a hidden tombstone.
    Hidden,
    /// Nothing is stored under that fingerprint.
    NotFound,
    /// The device is still trusted. Revoking is a separate, deliberate act.
    NotRevoked,
    /// It was already hidden. Nothing was written.
    AlreadyHidden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub device_name: String,
    pub listen_port: u16,
    /// Automatically grant a capability when a newly paired peer advertises
    /// it. `battery.v1` is read-only telemetry with no side effects, so it is
    /// on by default; anything with side effects must not be.
    pub auto_grant: Vec<String>,
}

impl Default for Settings {
    /// The device name here is a placeholder.
    ///
    /// Asking the machine what it is called is a platform question —
    /// `/etc/hostname`, `GetComputerNameEx`, `SCDynamicStoreCopyComputerName`
    /// — so the real answer arrives through [`StoreConfig::default_device_name`]
    /// and this value is only ever seen if a caller builds `Settings` by hand.
    fn default() -> Self {
        Self {
            device_name: "Pliwee Device".to_string(),
            listen_port: crate::DEFAULT_PORT,
            auto_grant: vec!["battery.v1".to_string()],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    schema_version: u32,
    device_id: String,
    /// DER certificate, base64. The matching private key is in a separate,
    /// stricter file.
    certificate_der_b64: String,
    /// How the private key is protected, **as recorded when the identity was
    /// created**.
    ///
    /// This is the load-bearing field of the whole fail-safe story: without a
    /// recorded expectation there is no way to tell "this device never had
    /// hardware backing" from "the hardware backing has gone away", and the
    /// correct response to those two situations is opposite (PLAT-DEC-015).
    ///
    /// `#[serde(default)]` so a schema-1 file — every install in the field —
    /// reads back as `Software`, which is what it is.
    #[serde(default)]
    key_backing: KeyBacking,
    settings: Settings,
    peers: Vec<TrustedPeer>,
}

/// Everything [`Store::open_with`] needs that is not portable.
///
/// One struct rather than four arguments so that adding a platform concern
/// later is an added field, not a changed signature at every call site.
pub struct StoreConfig {
    /// Where bytes live, and how the platform keeps them private.
    pub secrets: Arc<dyn SecretStore>,
    /// How identities are created and reloaded. Wave 0 ships exactly one:
    /// [`SoftwareBacking`].
    pub backend: Arc<dyn IdentityBackend>,
    /// What kind of machine this is.
    ///
    /// Supplied by the adapter, not decided by the storage layer. Before
    /// Wave 0 `Platform::Linux` was hardcoded in this file, which meant
    /// persistence decided the platform (audit finding C2).
    pub platform: pliwee_proto::v1::Platform,
    /// This machine's name, for a first run.
    pub default_device_name: String,
}

impl std::fmt::Debug for StoreConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreConfig")
            .field("secrets", &self.secrets.describe())
            .field("backend", &self.backend.backing())
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

/// Owns the on-disk state and the in-memory view of it.
pub struct Store {
    secrets: Arc<dyn SecretStore>,
    backend: Arc<dyn IdentityBackend>,
    identity: Identity,
    settings: Settings,
    peers: BTreeMap<Fingerprint, TrustedPeer>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("secrets", &self.secrets.describe())
            .field("identity", &self.identity)
            .field("peers", &self.peers.len())
            .finish_non_exhaustive()
    }
}

/// What a look at the stored identity found, plus the material to load it.
enum Resolution {
    /// Genuine first run. The **only** outcome that may create a key.
    Create,
    Load {
        state_bytes: Vec<u8>,
        secret: Vec<u8>,
    },
    /// Refuse to start. `message` is what the operator sees.
    Refuse {
        state: IdentityState,
        message: String,
    },
}

impl Store {
    /// Opens the store on this machine's default platform storage.
    ///
    /// Available only with the `unix-fs` feature, which is on by default.
    /// With it off, `pliwee-core` has no filesystem or environment
    /// assumption at all and [`Store::open_with`] is the only door.
    #[cfg(feature = "unix-fs")]
    pub fn open(dir: impl AsRef<std::path::Path>) -> Result<Self> {
        use crate::platform::unix_fs::{default_device_name, FileSecretStore};
        Self::open_with(StoreConfig {
            secrets: Arc::new(FileSecretStore::new(dir.as_ref())),
            backend: Arc::new(SoftwareBacking),
            platform: pliwee_proto::v1::Platform::Linux,
            default_device_name: default_device_name(),
        })
    }

    /// Opens the store against any [`SecretStore`] and any
    /// [`IdentityBackend`].
    ///
    /// # The one rule
    ///
    /// A new identity is generated **only** from
    /// [`IdentityState::NotCreated`], which means "no state document *and* no
    /// key material, both established positively". Every other outcome is a
    /// refusal to start, and nothing is written on the way out. An identity
    /// that exists but cannot be read right now is never replaced.
    pub fn open_with(config: StoreConfig) -> Result<Self> {
        let StoreConfig {
            secrets,
            backend,
            platform,
            default_device_name,
        } = config;

        secrets.harden()?;

        match classify(secrets.as_ref(), backend.as_ref()) {
            Resolution::Refuse { state, message } => {
                tracing::error!(
                    identity_state = state.code(),
                    "refusing to start: the stored identity is not usable, and \
                     replacing it would destroy every pairing"
                );
                Err(Error::Store(message))
            }
            Resolution::Create => Self::initialize(secrets, backend, platform, default_device_name),
            Resolution::Load {
                state_bytes,
                secret,
            } => Self::load(secrets, backend, platform, &state_bytes, secret),
        }
    }

    /// Reports what the stored identity looks like, without touching it.
    ///
    /// A pure observation, for `pliwee status` and for tests that need to
    /// prove nothing was regenerated.
    pub fn probe_identity(
        secrets: &dyn SecretStore,
        backend: &dyn IdentityBackend,
    ) -> IdentityState {
        match classify(secrets, backend) {
            Resolution::Create => IdentityState::NotCreated,
            Resolution::Load { .. } => IdentityState::Available,
            Resolution::Refuse { state, .. } => state,
        }
    }

    /// Reports what the identity in `dir` looks like, without touching it.
    #[cfg(feature = "unix-fs")]
    pub fn probe_identity_at(dir: impl AsRef<std::path::Path>) -> IdentityState {
        use crate::platform::unix_fs::FileSecretStore;
        Self::probe_identity(&FileSecretStore::new(dir.as_ref()), &SoftwareBacking)
    }

    fn initialize(
        secrets: Arc<dyn SecretStore>,
        backend: Arc<dyn IdentityBackend>,
        platform: pliwee_proto::v1::Platform,
        device_name: String,
    ) -> Result<Self> {
        let settings = Settings {
            device_name,
            ..Settings::default()
        };
        let (provider, secret) = backend.create(&settings.device_name, platform)?;

        // Key material first, state second. If the process dies between the
        // two, the next start sees key-without-state, which is a refusal —
        // not a silent regeneration over a key that may already have been
        // used to pair.
        secrets.write_secret(backend.secret_name(), &secret)?;

        let store = Self {
            secrets,
            backend,
            identity: Identity::new(provider),
            settings,
            peers: BTreeMap::new(),
        };
        store.persist()?;
        Ok(store)
    }

    fn load(
        secrets: Arc<dyn SecretStore>,
        backend: Arc<dyn IdentityBackend>,
        platform: pliwee_proto::v1::Platform,
        state_bytes: &[u8],
        secret: Vec<u8>,
    ) -> Result<Self> {
        // Already parsed once during classification; parsing again here keeps
        // `classify` free of ownership games and costs nothing measurable on
        // a file of tens of records.
        let state: StateFile = serde_json::from_slice(state_bytes)
            .map_err(|e| Error::Store(format!("state.json: {e}")))?;

        let cert_der = data_encoding::BASE64
            .decode(state.certificate_der_b64.as_bytes())
            .map_err(|_| Error::Store("certificate is not valid base64".into()))?;

        let provider = backend.load(
            state.device_id,
            state.settings.device_name.clone(),
            platform,
            cert_der,
            secret,
        )?;

        let peers = state
            .peers
            .into_iter()
            .map(|mut p| {
                // Hidden implies revoked, decided here rather than believed
                // from the file. A record that claims to be out of the list
                // but still trusted is the one combination that would let a
                // hand-edited or truncated store admit a device nobody can
                // see, so it is read the safe way round.
                if p.hidden {
                    p.revoked = true;
                }
                // Hints are believed only in the shape the store writes
                // them, and only for a peer that is still trusted.
                p.address_hints = if p.revoked {
                    Vec::new()
                } else {
                    bound_address_hints(std::mem::take(&mut p.address_hints))
                };
                (p.fingerprint, p)
            })
            .collect();

        Ok(Self {
            secrets,
            backend,
            identity: Identity::new(provider),
            settings: state.settings,
            peers,
        })
    }

    fn state_document(&self) -> Result<Vec<u8>> {
        let state = StateFile {
            schema_version: SCHEMA_VERSION,
            device_id: self.identity.device_id().to_string(),
            certificate_der_b64: data_encoding::BASE64.encode(self.identity.certificate_der()),
            key_backing: self.identity.backing(),
            settings: self.settings.clone(),
            peers: self.peers.values().cloned().collect(),
        };
        serde_json::to_vec_pretty(&state)
            .map_err(|e| Error::Store(format!("serializing state: {e}")))
    }

    pub fn persist(&self) -> Result<()> {
        let bytes = self.state_document()?;
        self.secrets.write_state(&bytes)?;
        Ok(())
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// How this device's private key is protected.
    ///
    /// Shown by `pliwee status`. Never advertised to a peer: a device's
    /// claim about its own key storage is unverifiable, and an unverifiable
    /// self-report is not a security property (PLAT-DEC-012).
    pub fn key_backing(&self) -> KeyBacking {
        self.identity.backing()
    }

    /// The storage this store is using, for diagnostics.
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    /// The identity backend in use.
    pub fn identity_backend(&self) -> &Arc<dyn IdentityBackend> {
        &self.backend
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Every stored record, hidden tombstones included.
    ///
    /// The security view. Callers that decide *admission* or *authorisation*
    /// want this one; callers that draw a list want [`Self::listed_peers`].
    pub fn peers(&self) -> impl Iterator<Item = &TrustedPeer> {
        self.peers.values()
    }

    /// The records that belong in an ordinary device list.
    ///
    /// Everything a person is shown, and everything a device selector may
    /// resolve, comes through here. Tombstones are absent by construction, so
    /// no screen has to remember to filter them and no CLI selector can name
    /// one.
    pub fn listed_peers(&self) -> impl Iterator<Item = &TrustedPeer> {
        self.peers.values().filter(|p| p.is_listed())
    }

    /// Visible records whose pairing has been revoked.
    ///
    /// What "Remove all revoked devices" would act on, and — because it is
    /// the same iterator — what decides whether that action is offered at
    /// all.
    pub fn revoked_listed_peers(&self) -> impl Iterator<Item = &TrustedPeer> {
        self.peers.values().filter(|p| p.revoked && p.is_listed())
    }

    /// Looks up a peer by pinned fingerprint. Revoked peers are *not*
    /// returned: to every caller they are simply not trusted.
    pub fn trusted_peer(&self, fp: &Fingerprint) -> Option<&TrustedPeer> {
        self.peers.get(fp).filter(|p| !p.revoked)
    }

    /// Looks up a peer including revoked ones (for `pliwee devices` output).
    pub fn peer_record(&self, fp: &Fingerprint) -> Option<&TrustedPeer> {
        self.peers.get(fp)
    }

    pub fn find_by_device_id(&self, device_id: &str) -> Option<&TrustedPeer> {
        self.peers.values().find(|p| p.device_id == device_id)
    }

    /// Adds or refreshes a trusted peer. Re-pairing an existing fingerprint
    /// clears a previous revocation, which is the intended way back in.
    pub fn add_peer(&mut self, mut peer: TrustedPeer) -> Result<()> {
        peer.address_hints = if peer.revoked {
            Vec::new()
        } else {
            bound_address_hints(peer.address_hints)
        };
        self.peers.insert(peer.fingerprint, peer);
        self.persist()
    }

    /// Records that `addr` reached the trusted peer pinned as `fp`.
    ///
    /// Call this only after a session to `addr` authenticated against `fp`:
    /// an address learned from discovery, or one whose handshake failed, is
    /// not a success and is not recorded (Mesh V2 SPEC §5).
    ///
    /// `addr` must be an address this device could dial. For a session the
    /// peer opened, that is the source IP with the peer's listening port —
    /// never the connection's ephemeral source port (ADR-0022 §D3), which
    /// would fill the list with routes that lead nowhere.
    ///
    /// The address moves to the front of the list with `at_unix` as its
    /// last-success time; an equivalent hint already stored is replaced
    /// rather than duplicated, and the oldest hint past
    /// [`MAX_ADDRESS_HINTS`] is dropped.
    ///
    /// Returns `false`, and writes nothing, for a fingerprint that is not a
    /// trusted peer — unknown and revoked alike — and for an address nothing
    /// could dial. A hint never creates, restores or changes a trust record.
    pub fn record_address_success(
        &mut self,
        fp: &Fingerprint,
        addr: SocketAddr,
        at_unix: i64,
    ) -> Result<bool> {
        if self.trusted_peer(fp).is_none() {
            return Ok(false);
        }
        let Some(hint) = AddressHint::new(addr, at_unix) else {
            return Ok(false);
        };

        let mut next = self.peers.clone();
        if let Some(p) = next.get_mut(fp) {
            let mut hints = Vec::with_capacity(MAX_ADDRESS_HINTS + 1);
            hints.push(hint);
            hints.append(&mut p.address_hints);
            p.address_hints = bound_address_hints(hints);
        }
        self.commit(next)?;
        Ok(true)
    }

    /// Where the trusted peer pinned as `fp` was recently reached, most
    /// recent first.
    ///
    /// Empty for an unknown or revoked fingerprint. Whatever is dialled from
    /// this list must still present `fp`; the list itself proves nothing.
    pub fn address_hints(&self, fp: &Fingerprint) -> &[AddressHint] {
        self.trusted_peer(fp)
            .map(|p| p.address_hints.as_slice())
            .unwrap_or(&[])
    }

    /// Revokes a pairing. The record is kept (with `revoked = true`) rather
    /// than deleted, so the device remains listed and a later reconnection
    /// attempt is attributable instead of appearing as a stranger.
    pub fn revoke_peer(&mut self, fp: &Fingerprint) -> Result<bool> {
        match self.peers.get_mut(fp) {
            Some(p) => {
                p.revoked = true;
                p.granted_capabilities.clear();
                // The policies go with the grants. They are inert while the
                // grant is gone — every authorizer asks both — but leaving
                // `allow_mirror` behind on a record that can be paired again
                // is a decision waiting to be resurrected by a re-pair, and
                // this is the moment the person said no.
                //
                // `DENIED` rather than `default()` for the reason
                // [`TrustedPeer::into_tombstone`] uses it: a policy's default
                // is written for a device somebody has granted something to,
                // and serializes as `allow_send`/`allow_mirror` true.
                p.clipboard_policy = ClipboardPolicy::DENIED;
                p.notification_policy = NotificationPolicy::DENIED;
                // And nothing should go looking for a device that is no
                // longer trusted.
                p.address_hints.clear();
                self.persist()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Takes one revoked device out of the visible lists, keeping the
    /// revocation.
    ///
    /// The record is replaced by [`TrustedPeer::into_tombstone`] — the pinned
    /// fingerprint and `revoked`, and nothing else — so what remains is
    /// exactly what [`Self::peer_record`] needs to keep answering
    /// [`crate::session::PeerStatus::Revoked`]. It is deliberately not a
    /// deletion: deleting would make the same key unknown, and an unknown key
    /// is greeted differently from a rejected one.
    ///
    /// Refuses a device that is still trusted. Removing something from a list
    /// is a tidying-up action and must never be a way to withdraw trust
    /// without saying so — [`Self::revoke_peer`] is the deliberate act, and
    /// it comes first.
    pub fn hide_revoked_peer(&mut self, fp: &Fingerprint) -> Result<HideOutcome> {
        let outcome = match self.peers.get(fp) {
            None => HideOutcome::NotFound,
            Some(p) if !p.revoked => HideOutcome::NotRevoked,
            Some(p) if p.hidden => HideOutcome::AlreadyHidden,
            Some(_) => HideOutcome::Hidden,
        };
        if outcome != HideOutcome::Hidden {
            return Ok(outcome);
        }

        let mut next = self.peers.clone();
        if let Some(p) = next.remove(fp) {
            next.insert(*fp, p.into_tombstone());
        }
        self.commit(next)?;
        Ok(HideOutcome::Hidden)
    }

    /// Takes *every* visible revoked device out of the lists at once.
    ///
    /// One write for the whole set rather than one per device: the store is a
    /// single document, and a per-device loop that failed halfway would leave
    /// the file describing a state nobody asked for. Returns the fingerprints
    /// that were hidden, in store order, so the caller can report a count and
    /// clear a selection that pointed at one of them.
    ///
    /// Trusted devices are not candidates and are not rewritten. Hidden ones
    /// are already hidden. Nothing here matches on a display name.
    pub fn hide_all_revoked_peers(&mut self) -> Result<Vec<Fingerprint>> {
        let targets: Vec<Fingerprint> =
            self.revoked_listed_peers().map(|p| p.fingerprint).collect();
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let mut next = self.peers.clone();
        for fp in &targets {
            if let Some(p) = next.remove(fp) {
                next.insert(*fp, p.into_tombstone());
            }
        }
        self.commit(next)?;
        Ok(targets)
    }

    /// Swaps in a new peer map, and only keeps it if it reached the disk.
    ///
    /// Without this a failed write would leave the process believing
    /// something the file does not say — which for a trust store means the
    /// in-memory answer to "is this device revoked?" could outlive a restart
    /// in one direction and not the other.
    fn commit(&mut self, peers: BTreeMap<Fingerprint, TrustedPeer>) -> Result<()> {
        let previous = std::mem::replace(&mut self.peers, peers);
        match self.persist() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.peers = previous;
                Err(e)
            }
        }
    }

    pub fn set_capability_grant(
        &mut self,
        fp: &Fingerprint,
        capability_id: &str,
        granted: bool,
    ) -> Result<()> {
        if let Some(p) = self.peers.get_mut(fp) {
            p.granted_capabilities
                .insert(capability_id.to_string(), granted);
        }
        self.persist()
    }

    /// Replaces one peer's clipboard policy.
    ///
    /// Persisted immediately, because the two questions a user asks after
    /// changing it — "did that take?" and "will it survive a restart?" —
    /// should have the same answer.
    pub fn set_clipboard_policy(
        &mut self,
        fp: &Fingerprint,
        policy: ClipboardPolicy,
    ) -> Result<()> {
        if let Some(p) = self.peers.get_mut(fp) {
            p.clipboard_policy = policy;
        }
        self.persist()
    }

    /// Replaces one peer's notification policy.
    ///
    /// Persisted immediately, for the same reason
    /// [`set_clipboard_policy`](Self::set_clipboard_policy) is.
    pub fn set_notification_policy(
        &mut self,
        fp: &Fingerprint,
        policy: NotificationPolicy,
    ) -> Result<()> {
        if let Some(p) = self.peers.get_mut(fp) {
            p.notification_policy = policy;
        }
        self.persist()
    }

    pub fn record_protocol_version(&mut self, fp: &Fingerprint, version: u32) -> Result<()> {
        if let Some(p) = self.peers.get_mut(fp) {
            if version > p.last_protocol_version {
                p.last_protocol_version = version;
            }
        }
        self.persist()
    }
}

/// Default location: `$XDG_DATA_HOME/pliwee`, else `~/.local/share/…`.
///
/// Re-exported from the Unix adapter so existing callers keep their import
/// path. It is feature-gated for the same reason the adapter is.
#[cfg(feature = "unix-fs")]
pub use crate::platform::unix_fs::default_data_dir;

// ---------------------------------------------------------------------------
// Identity classification — the safety property
// ---------------------------------------------------------------------------

/// Decides which of the six identity states this store is in.
///
/// Reads both items, and reads them *positively*: the only path to
/// [`Resolution::Create`] is `Ok(None)` from both, which means the platform
/// told us, without error, that neither exists. Any error — permission,
/// I/O, a loose mode, a corrupt file — is a refusal.
///
/// | Situation | State |
/// | --- | --- |
/// | no state document, no key material | `IDENTITY_NOT_CREATED` |
/// | both present, parse, and correspond | `IDENTITY_AVAILABLE` |
/// | key unparseable, key ≠ certificate, or state unparseable | `IDENTITY_CORRUPTED` |
/// | key protection not met (0644 key, traversable data dir) | `IDENTITY_CORRUPTED` |
/// | state present, key absent or unreadable | `IDENTITY_LOST` |
/// | key present, state absent | `IDENTITY_LOST` |
/// | recorded backing is not the one this build can open | `IDENTITY_HARDWARE_UNAVAILABLE` |
/// | transient I/O failure on either | `IDENTITY_TEMPORARILY_UNAVAILABLE` |
fn classify(secrets: &dyn SecretStore, backend: &dyn IdentityBackend) -> Resolution {
    let state_read = secrets.read_state();
    let secret_read = secrets.read_secret(backend.secret_name());

    // Storage errors first, and in the order that produces the most useful
    // message. None of them can reach `Create`.
    for (what, result) in [
        ("state.json", state_read.as_ref().err()),
        ("the identity key", secret_read.as_ref().err()),
    ] {
        if let Some(e) = result {
            return refuse_access(what, e);
        }
    }

    let state_bytes = match state_read {
        Ok(v) => v,
        // Unreachable: handled above. Refusing is still the safe arm.
        Err(e) => return refuse_access("state.json", &e),
    };
    let secret = match secret_read {
        Ok(v) => v,
        Err(e) => return refuse_access("the identity key", &e),
    };

    match (state_bytes, secret) {
        // The one and only path to a new identity.
        (None, None) => Resolution::Create,

        (Some(_), None) => refuse(IdentityState::Lost(
            "state.json describes an identity whose key material is gone. \
             Restore the key from a backup, or delete the data directory to \
             start over — which will require re-pairing every device."
                .into(),
        )),

        (None, Some(_)) => refuse(IdentityState::Lost(
            "identity key material is present but state.json, which records \
             the certificate and the trusted peers, is gone. Restore it from \
             a backup, or delete the data directory to start over — which \
             will require re-pairing every device."
                .into(),
        )),

        (Some(state_bytes), Some(secret)) => {
            let state: StateFile = match serde_json::from_slice(&state_bytes) {
                Ok(s) => s,
                Err(e) => {
                    return refuse(IdentityState::Corrupted(format!(
                        "state.json does not parse: {e}"
                    )))
                }
            };

            if state.schema_version > SCHEMA_VERSION {
                return Resolution::Refuse {
                    state: IdentityState::Corrupted(format!(
                        "state.json schema v{} is newer than supported v{}",
                        state.schema_version, SCHEMA_VERSION
                    )),
                    // Verbatim: this wording predates Wave 0 and says exactly
                    // the right thing.
                    message: format!(
                        "state.json schema v{} is newer than supported v{}; refusing to \
                         downgrade and risk losing trust records",
                        state.schema_version, SCHEMA_VERSION
                    ),
                };
            }

            if state.key_backing != backend.backing() {
                return refuse(IdentityState::HardwareUnavailable(format!(
                    "this identity was created with `{}` key backing and this \
                     build can only open `{}`. The identity has not been \
                     touched. Migrating between backings creates a new \
                     identity and re-pairs every device, so it is never done \
                     automatically",
                    state.key_backing,
                    backend.backing()
                )));
            }

            if data_encoding::BASE64
                .decode(state.certificate_der_b64.as_bytes())
                .is_err()
            {
                return refuse(IdentityState::Corrupted(
                    "the certificate in state.json is not valid base64".into(),
                ));
            }

            Resolution::Load {
                state_bytes,
                secret,
            }
        }
    }
}

fn refuse(state: IdentityState) -> Resolution {
    let message = Error::from(state.clone()).to_string();
    // `Error::Store`'s Display prefixes "identity store error: "; the caller
    // wraps it again, so strip the prefix rather than say it twice.
    let message = message
        .strip_prefix("identity store error: ")
        .unwrap_or(&message)
        .to_string();
    Resolution::Refuse { state, message }
}

/// Turns a storage-access failure into a refusal, preserving the distinction
/// the [`SecretStore`] contract exists to keep.
fn refuse_access(what: &str, e: &StoreAccessError) -> Resolution {
    match e {
        // A loose mode is reported verbatim: the message names the file, the
        // mode and the exact `chmod` that fixes it, and has done since long
        // before Wave 0.
        StoreAccessError::NotPrivate { detail, .. } => Resolution::Refuse {
            state: IdentityState::Corrupted(detail.clone()),
            message: detail.clone(),
        },
        StoreAccessError::PermissionDenied { .. } => refuse(IdentityState::Lost(format!(
            "{what} exists but cannot be read ({e}). Pliwee will not generate \
             a replacement identity over one it cannot read"
        ))),
        StoreAccessError::Io { .. } => refuse(IdentityState::TemporarilyUnavailable(format!(
            "{what} could not be read ({e}). This may be transient; retry \
             before assuming anything is lost"
        ))),
        StoreAccessError::Corrupted { .. } => {
            refuse(IdentityState::Corrupted(format!("{what}: {e}")))
        }
    }
}
