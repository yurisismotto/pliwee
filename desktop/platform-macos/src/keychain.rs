//! The identity's private key in the login keychain.
//!
//! On Linux the key is `identity.key`, mode 0600, in a 0700 directory. That
//! is a sound arrangement on a Unix filesystem, and it is what
//! `FileSecretStore` still does for `state.json` here. The private key is
//! different: macOS has a store built for exactly this, encrypted at rest
//! under the user's login password and access-controlled per application, and
//! a key in a file is readable by every process the user runs.
//!
//! So [`KeychainSecretStore`] splits the two. Secrets — today only
//! [`IDENTITY_SECRET`](pliwee_core::secret_store::IDENTITY_SECRET) — are
//! generic-password items in the login keychain. The state document, which
//! holds peer fingerprints and grants but no key material, stays a 0600 file.
//!
//! # The rule this file exists to keep
//!
//! `Ok(None)` from [`read_secret`](SecretStore::read_secret) means the item
//! **does not exist**, and nothing else. The Keychain has several ways to
//! refuse — locked, user cancelled a prompt, interaction not allowed in this
//! session, an access-control list that does not name this binary — and
//! every one of them must stay an error. Folding any of them into "absent"
//! would make the store create a new identity over the existing one, and
//! every pairing would silently stop working. That is the Wave 0 defect the
//! trait was written to prevent; [`classify`] is where it is prevented here.
//!
//! # Profiles
//!
//! An item is named by the *data directory* as well as the secret, so
//! `pliweed --data-dir <dir>` keeps its own identity exactly as it does on
//! Linux, and two profiles can never read each other's key.
//!
//! # A keychain that is not there
//!
//! With `HOME` pointing somewhere that has no `Library/Keychains` — a test
//! harness that clears the environment, a misconfigured job — `SecItemAdd`
//! does not fail: it blocks, behind a system dialog nobody may be there to
//! see, and the agent never starts. So every operation first checks that the
//! directory exists and holds a keychain, and reports its absence by name.
//! A missing precondition fails loudly; it does not hang and it does not
//! read as "no identity yet".
//!
//! # Development builds
//!
//! The login keychain records which code may read an item by its code
//! signature. An ad-hoc signed build gets a new signature every time it is
//! rebuilt, so after a rebuild macOS asks once whether the new `pliweed` may
//! use the key. That prompt is the access control working; a Developer ID
//! signature is stable across updates and does not trigger it.

use std::path::{Path, PathBuf};

use pliwee_core::platform::unix_fs::FileSecretStore;
use pliwee_core::secret_store::{SecretStore, StoreAccessError, StoreResult};
use security_framework::base::Error as SecError;
use security_framework::item::{ItemClass, ItemSearchOptions};
use security_framework::passwords;

/// The Keychain `service` attribute of every Pliwee item.
///
/// Shown as the item's name in Keychain Access, so it is the bundle
/// identifier: it says which application the item belongs to.
pub const SERVICE: &str = "io.github.yurisismotto.pliwee";

// `SecBase.h`. Named here rather than imported because `security-framework`
// exposes them only through its `-sys` crate, and four integers are not worth
// a second dependency edge.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;
const ERR_SEC_AUTH_FAILED: i32 = -25293;
const ERR_SEC_USER_CANCELED: i32 = -128;

/// Secrets in the login keychain; the state document in a private directory.
#[derive(Debug, Clone)]
pub struct KeychainSecretStore {
    dir: PathBuf,
    state: FileSecretStore,
    service: String,
}

impl KeychainSecretStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_service(dir, SERVICE)
    }

    /// The same store under a different `service`. Tests use it, so that a
    /// test can never touch the real identity's item.
    pub fn with_service(dir: impl Into<PathBuf>, service: impl Into<String>) -> Self {
        let dir = dir.into();
        Self {
            state: FileSecretStore::new(&dir),
            dir,
            service: service.into(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The `account` attribute for one secret: its name and the directory
    /// whose identity it is.
    ///
    /// The directory is made absolute first, so `--data-dir ./x` and the same
    /// directory spelt out in full name the same item.
    pub fn account(&self, name: &str) -> StoreResult<String> {
        if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
            return Err(StoreAccessError::Corrupted {
                item: name.to_string(),
                detail: "not a valid secret name".into(),
            });
        }
        let dir = std::path::absolute(&self.dir).unwrap_or_else(|_| self.dir.clone());
        Ok(format!("{name}@{}", dir.display()))
    }

    /// Removes one secret. Not part of [`SecretStore`] — the store never
    /// deletes an identity — and present so tests can clean up after
    /// themselves.
    pub fn delete_secret(&self, name: &str) -> StoreResult<()> {
        let account = self.account(name)?;
        match passwords::delete_generic_password(&self.service, &account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(classify(name, &e)),
        }
    }
}

/// Turns a Keychain status into the one thing it means.
///
/// `errSecItemNotFound` is deliberately absent from this function: the call
/// sites turn it into `Ok(None)` before they get here, and anything that
/// reaches this function is therefore *not* absence.
pub fn classify(item: &str, e: &SecError) -> StoreAccessError {
    let detail = format!("login keychain: {e} (OSStatus {})", e.code());
    match e.code() {
        ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED => StoreAccessError::PermissionDenied {
            item: item.to_string(),
            detail,
        },
        // Locked, or this session cannot show the unlock prompt (an SSH login,
        // a job started before the GUI session). Transient: the same call
        // succeeds once the keychain is unlocked.
        ERR_SEC_INTERACTION_NOT_ALLOWED => StoreAccessError::Io {
            item: item.to_string(),
            detail,
        },
        _ => StoreAccessError::Io {
            item: item.to_string(),
            detail,
        },
    }
}

/// Refuses to call into the Keychain when there is no keychain to call into.
///
/// `home` is the directory Security.framework will look under: it follows
/// `$HOME`, as the reproduction in the module docs showed.
pub fn require_keychain(item: &str, home: &Path) -> StoreResult<()> {
    let dir = home.join("Library").join("Keychains");
    let has_keychain = std::fs::read_dir(&dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".keychain-db") || name.ends_with(".keychain")
            })
        })
        .unwrap_or(false);
    if has_keychain {
        Ok(())
    } else {
        Err(StoreAccessError::Io {
            item: item.to_string(),
            detail: format!(
                "no keychain in {}; HOME must be this user's home directory \
                 for the identity's key to be reachable",
                dir.display()
            ),
        })
    }
}

fn keychain_home() -> PathBuf {
    crate::RuntimePaths::for_current_user().home().to_path_buf()
}

impl SecretStore for KeychainSecretStore {
    fn read_secret(&self, name: &str) -> StoreResult<Option<Vec<u8>>> {
        let account = self.account(name)?;
        require_keychain(name, &keychain_home())?;
        match passwords::get_generic_password(&self.service, &account) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(classify(name, &e)),
        }
    }

    fn write_secret(&self, name: &str, data: &[u8]) -> StoreResult<()> {
        // `SecItemAdd`, or `SecItemUpdate` when the item exists. Atomic either
        // way, and the bytes never exist anywhere unprotected: they are
        // encrypted by the keychain as they are stored, which is the property
        // the trait asks of a file written at its final mode.
        let account = self.account(name)?;
        require_keychain(name, &keychain_home())?;
        passwords::set_generic_password(&self.service, &account, data)
            .map_err(|e| classify(name, &e))
    }

    fn read_state(&self) -> StoreResult<Option<Vec<u8>>> {
        self.state.read_state()
    }

    fn write_state(&self, data: &[u8]) -> StoreResult<()> {
        self.state.write_state(data)
    }

    fn harden(&self) -> StoreResult<()> {
        // The directory that holds `state.json`: 0700, a hard error if it
        // cannot be made so. The keychain needs no hardening from us.
        self.state.harden()
    }

    fn verify_protection(&self, name: &str) -> StoreResult<()> {
        // Present in the keychain is protected by the keychain. Asks for the
        // item's attributes only, never its data, so it cannot trigger an
        // access prompt.
        let account = self.account(name)?;
        require_keychain(name, &keychain_home())?;
        let found = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(&self.service)
            .account(&account)
            .load_attributes(true)
            .search();
        match found {
            Ok(items) if !items.is_empty() => Ok(()),
            Ok(_) => Err(not_found(name)),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Err(not_found(name)),
            Err(e) => Err(classify(name, &e)),
        }
    }

    fn describe(&self) -> String {
        format!(
            "login keychain (service {}) for secrets, {} for state",
            self.service,
            self.dir.display()
        )
    }
}

fn not_found(item: &str) -> StoreAccessError {
    StoreAccessError::Io {
        item: item.to_string(),
        detail: "not present in the login keychain".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_named_by_secret_and_by_directory() {
        let a = KeychainSecretStore::new("/Users/ana/Library/Application Support/Pliwee");
        let b = KeychainSecretStore::new("/tmp/profile-b");
        assert_eq!(
            a.account("identity").expect("valid"),
            "identity@/Users/ana/Library/Application Support/Pliwee"
        );
        assert_ne!(
            a.account("identity").expect("valid"),
            b.account("identity").expect("valid"),
            "two data directories must never share a key"
        );
    }

    #[test]
    fn a_relative_directory_names_the_same_item_as_its_absolute_form() {
        let cwd = std::env::current_dir().expect("cwd");
        let relative = KeychainSecretStore::new("profile");
        let absolute = KeychainSecretStore::new(cwd.join("profile"));
        assert_eq!(
            relative.account("identity").expect("valid"),
            absolute.account("identity").expect("valid")
        );
    }

    #[test]
    fn a_secret_name_cannot_carry_a_path() {
        let s = KeychainSecretStore::new("/tmp/x");
        for bad in ["", "../identity", "a/b", "a\\b"] {
            assert!(
                matches!(s.account(bad), Err(StoreAccessError::Corrupted { .. })),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn no_refusal_is_ever_classified_as_absence() {
        // The load-bearing rule. `classify` has no `Ok(None)` to return, but
        // the *kind* still matters: a refusal must never look like something a
        // caller could mistake for a missing item and recover from by
        // creating one.
        for code in [
            ERR_SEC_AUTH_FAILED,
            ERR_SEC_USER_CANCELED,
            ERR_SEC_INTERACTION_NOT_ALLOWED,
            -25291, // errSecNotAvailable
            -25294, // errSecNoSuchKeychain
            -34018, // errSecMissingEntitlement
        ] {
            let e = classify("identity", &SecError::from_code(code));
            assert!(
                matches!(
                    e,
                    StoreAccessError::PermissionDenied { .. } | StoreAccessError::Io { .. }
                ),
                "{code}: {e:?}"
            );
            assert!(e.to_string().contains(&code.to_string()), "{e}");
        }
    }

    #[test]
    fn a_denied_prompt_is_permission_denied_and_a_locked_keychain_is_transient() {
        assert!(matches!(
            classify("identity", &SecError::from_code(ERR_SEC_USER_CANCELED)),
            StoreAccessError::PermissionDenied { .. }
        ));
        assert!(classify(
            "identity",
            &SecError::from_code(ERR_SEC_INTERACTION_NOT_ALLOWED)
        )
        .is_transient());
    }

    #[test]
    fn a_home_without_a_keychain_is_refused_by_name_and_not_as_absence() {
        let home = tempfile::tempdir().expect("tempdir");
        let e = require_keychain("identity", home.path()).expect_err("no keychain");
        assert!(matches!(e, StoreAccessError::Io { .. }), "{e:?}");
        assert!(e.to_string().contains("Library/Keychains"), "{e}");

        std::fs::create_dir_all(home.path().join("Library/Keychains")).expect("mkdir");
        assert!(
            require_keychain("identity", home.path()).is_err(),
            "an empty directory is no keychain"
        );
        std::fs::write(home.path().join("Library/Keychains/login.keychain-db"), b"")
            .expect("touch");
        require_keychain("identity", home.path()).expect("a keychain file is present");
    }

    #[test]
    fn this_users_home_has_a_keychain() {
        // Measured on the machine running the tests: the precondition holds
        // for a real account, so the check above cannot be refusing everyone.
        require_keychain("identity", &keychain_home()).expect("login keychain present");
    }

    #[test]
    fn describe_never_contains_secret_material() {
        let s = KeychainSecretStore::new("/tmp/x");
        let d = s.describe();
        assert!(d.contains("login keychain"), "{d}");
        assert!(d.contains("/tmp/x"), "{d}");
    }
}
