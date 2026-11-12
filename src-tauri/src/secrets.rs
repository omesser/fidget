//! Secrets the user must not hand-edit.
//!
//! The Director API key is one: it belongs in the OS secret store, not in
//! `settings.json`, where a sync or a backup would copy it in plain text.
//! `SecretStore` is the seam — `MemoryStore` in tests, `KeyringStore` in the
//! process — so tests can read and write the key without touching Keychain.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use keyring::Entry;

/// Account name in the OS secret store, not a settings key.
pub const DIRECTOR_API_KEY: &str = "director-api-key";

/// Read and write secrets without naming where they live.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// In-memory store for tests. Never touches Keychain.
#[cfg(test)]
pub struct MemoryStore {
    inner: Mutex<HashMap<String, String>>,
}

#[cfg(test)]
impl MemoryStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let map = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(map.get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let mut map = self.inner.lock().map_err(|e| e.to_string())?;
        map.insert(account.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let mut map = self.inner.lock().map_err(|e| e.to_string())?;
        map.remove(account);
        Ok(())
    }
}

/// OS secret store via `keyring`. Constructed only at process startup.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self {
            service: "fidget".to_string(),
        }
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = Entry::new(&self.service, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let entry = Entry::new(&self.service, account).map_err(|e| e.to_string())?;
        entry.set_password(value).map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let entry = Entry::new(&self.service, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

// Only the Windows and Linux stores have tests: they can reach a real one.
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;

    #[cfg(windows)]
    mod windows_credential_manager {
        use super::*;
        use std::sync::Mutex;

        static CREDENTIAL_MANAGER_LOCK: Mutex<()> = Mutex::new(());

        struct CredentialGuard {
            store: KeyringStore,
            account: String,
            original: Option<String>,
        }

        impl CredentialGuard {
            fn new(account: &str) -> Self {
                let store = KeyringStore::new();
                let original = store.get(account).unwrap_or(None);
                Self {
                    store,
                    account: account.to_string(),
                    original,
                }
            }
        }

        impl Drop for CredentialGuard {
            fn drop(&mut self) {
                match &self.original {
                    Some(value) => {
                        let _ = self.store.set(&self.account, value);
                    }
                    None => {
                        let _ = self.store.delete(&self.account);
                    }
                }
            }
        }

        #[test]
        fn round_trip_director_key_through_credential_manager() {
            let _lock = CREDENTIAL_MANAGER_LOCK.lock().unwrap();
            let guard = CredentialGuard::new(DIRECTOR_API_KEY);
            let store = &guard.store;

            let sentinel = format!("sk-windows-cm-roundtrip-{}", uuid::Uuid::new_v4());

            store.set(DIRECTOR_API_KEY, &sentinel).unwrap();

            let retrieved = store.get(DIRECTOR_API_KEY).unwrap();
            assert_eq!(retrieved.as_deref(), Some(sentinel.as_str()));

            store.delete(DIRECTOR_API_KEY).unwrap();
            let after_delete = store.get(DIRECTOR_API_KEY).unwrap();
            assert_eq!(after_delete, None);
        }

        #[test]
        fn missing_credential_is_none_not_error() {
            let _lock = CREDENTIAL_MANAGER_LOCK.lock().unwrap();
            let guard = CredentialGuard::new(DIRECTOR_API_KEY);
            let store = &guard.store;

            store.delete(DIRECTOR_API_KEY).unwrap();

            let result = store.get(DIRECTOR_API_KEY).unwrap();
            assert_eq!(result, None);
        }

        #[test]
        fn deleting_missing_credential_is_ok() {
            let _lock = CREDENTIAL_MANAGER_LOCK.lock().unwrap();
            let guard = CredentialGuard::new(DIRECTOR_API_KEY);
            let store = &guard.store;

            store.delete(DIRECTOR_API_KEY).unwrap();

            let result = store.delete(DIRECTOR_API_KEY);
            assert!(result.is_ok());
        }
    }

    /// Linux Secret Service tests. They need gnome-keyring under
    /// dbus-run-session, as Ubuntu CI runs it, and restore any existing
    /// `director-api-key`.
    #[cfg(target_os = "linux")]
    mod linux_secret_service {
        use super::*;
        use std::sync::Mutex;

        static SECRET_SERVICE_LOCK: Mutex<()> = Mutex::new(());

        struct CredentialGuard {
            store: KeyringStore,
            account: String,
            original: Option<String>,
        }

        impl CredentialGuard {
            fn new(account: &str) -> Self {
                let store = KeyringStore::new();
                let original = store.get(account).unwrap_or(None);
                Self {
                    store,
                    account: account.to_string(),
                    original,
                }
            }
        }

        impl Drop for CredentialGuard {
            fn drop(&mut self) {
                match &self.original {
                    Some(value) => {
                        let _ = self.store.set(&self.account, value);
                    }
                    None => {
                        let _ = self.store.delete(&self.account);
                    }
                }
            }
        }

        #[test]
        fn round_trip_director_key_through_secret_service() {
            let _lock = SECRET_SERVICE_LOCK.lock().unwrap();
            let guard = CredentialGuard::new(DIRECTOR_API_KEY);
            let store = &guard.store;

            let sentinel = format!("sk-linux-ss-roundtrip-{}", uuid::Uuid::new_v4());

            store.set(DIRECTOR_API_KEY, &sentinel).unwrap();

            let retrieved = store.get(DIRECTOR_API_KEY).unwrap();
            assert_eq!(retrieved.as_deref(), Some(sentinel.as_str()));

            store.delete(DIRECTOR_API_KEY).unwrap();
            let after_delete = store.get(DIRECTOR_API_KEY).unwrap();
            assert_eq!(after_delete, None);
        }

        #[test]
        fn missing_credential_is_none_not_error() {
            let _lock = SECRET_SERVICE_LOCK.lock().unwrap();
            let guard = CredentialGuard::new(DIRECTOR_API_KEY);
            let store = &guard.store;

            store.delete(DIRECTOR_API_KEY).unwrap();

            let result = store.get(DIRECTOR_API_KEY).unwrap();
            assert_eq!(result, None);
        }

        #[test]
        fn deleting_missing_credential_is_ok() {
            let _lock = SECRET_SERVICE_LOCK.lock().unwrap();
            let guard = CredentialGuard::new(DIRECTOR_API_KEY);
            let store = &guard.store;

            store.delete(DIRECTOR_API_KEY).unwrap();

            let result = store.delete(DIRECTOR_API_KEY);
            assert!(result.is_ok());
        }
    }
}
