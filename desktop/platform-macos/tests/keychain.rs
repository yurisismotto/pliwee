//! The Keychain secret store against the real login keychain.
//!
//! Ignored by default, because it writes to the login keychain of whoever
//! runs it. Every item it creates is under a service name of its own, unique
//! per run, and is deleted before the test returns — including on failure,
//! through [`Cleanup`]. Run on purpose:
//!
//! ```text
//! cargo test -p pliwee-macos --test keychain -- --ignored
//! ```

#![cfg(target_os = "macos")]

use std::sync::Arc;

use pliwee_core::secret_store::{SecretStore, IDENTITY_SECRET};
use pliwee_core::store::{Store, StoreConfig};
use pliwee_macos::keychain::KeychainSecretStore;

/// A service name no real Pliwee install uses, unique to this process.
fn test_service() -> String {
    format!(
        "io.github.yurisismotto.pliwee.test.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// Deletes the test's item however the test ends.
struct Cleanup(KeychainSecretStore);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.delete_secret(IDENTITY_SECRET);
    }
}

#[test]
#[ignore = "writes to the login keychain; run explicitly"]
fn a_secret_round_trips_and_absence_is_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = KeychainSecretStore::with_service(dir.path(), test_service());
    let _cleanup = Cleanup(store.clone());

    assert_eq!(
        store
            .read_secret(IDENTITY_SECRET)
            .expect("read before write"),
        None,
        "a secret that was never written is absent, and only that is None"
    );
    assert!(store.verify_protection(IDENTITY_SECRET).is_err());

    store
        .write_secret(IDENTITY_SECRET, b"first")
        .expect("write");
    assert_eq!(
        store.read_secret(IDENTITY_SECRET).expect("read"),
        Some(b"first".to_vec())
    );
    store.verify_protection(IDENTITY_SECRET).expect("present");

    // An existing item is replaced, not duplicated.
    store
        .write_secret(IDENTITY_SECRET, b"second")
        .expect("overwrite");
    assert_eq!(
        store.read_secret(IDENTITY_SECRET).expect("read"),
        Some(b"second".to_vec())
    );

    store.delete_secret(IDENTITY_SECRET).expect("delete");
    assert_eq!(store.read_secret(IDENTITY_SECRET).expect("read"), None);
}

#[test]
#[ignore = "writes to the login keychain; run explicitly"]
fn two_data_directories_never_share_a_key() {
    let service = test_service();
    let a_dir = tempfile::tempdir().expect("tempdir");
    let b_dir = tempfile::tempdir().expect("tempdir");
    let a = KeychainSecretStore::with_service(a_dir.path(), &service);
    let b = KeychainSecretStore::with_service(b_dir.path(), &service);
    let _ca = Cleanup(a.clone());
    let _cb = Cleanup(b.clone());

    a.write_secret(IDENTITY_SECRET, b"a").expect("write a");
    assert_eq!(b.read_secret(IDENTITY_SECRET).expect("read b"), None);
}

#[test]
#[ignore = "writes to the login keychain; run explicitly"]
fn an_identity_kept_in_the_keychain_survives_a_restart() {
    // The property that matters to a user: the agent stops and starts, and
    // it is still the same device to every phone it was paired with.
    let dir = tempfile::tempdir().expect("tempdir");
    let secrets = KeychainSecretStore::with_service(dir.path(), test_service());
    let _cleanup = Cleanup(secrets.clone());

    let open = || {
        Store::open_with(StoreConfig {
            secrets: Arc::new(secrets.clone()),
            backend: Arc::new(pliwee_core::identity::SoftwareBacking),
            platform: pliwee_proto::v1::Platform::Unspecified,
            default_device_name: "keychain test".into(),
        })
        .expect("open store")
    };

    let first = open();
    let fingerprint = first.identity().fingerprint();
    let device_id = first.identity().device_id().to_string();
    drop(first);

    // No key material on disk: the directory holds the state document only.
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| !n.ends_with(".key")),
        "a private key file was written beside the state: {names:?}"
    );
    assert!(names.iter().any(|n| n == "state.json"), "{names:?}");

    let second = open();
    assert_eq!(second.identity().fingerprint(), fingerprint);
    assert_eq!(second.identity().device_id(), device_id);
}
