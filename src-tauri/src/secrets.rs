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
// Linux needs gnome-keyring under dbus-run-session, as Ubuntu CI runs it.
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;

    /// An account of the test's own, deleted on drop. The real
    /// `director-api-key` stays untouched, and tests running side by side, in
    /// threads or in separate processes, never share an entry.
    struct ScratchAccount {
        store: KeyringStore,
        account: String,
    }

    impl ScratchAccount {
        fn new() -> Self {
            Self {
                store: KeyringStore::new(),
                account: format!("test-{}", uuid::Uuid::new_v4()),
            }
        }
    }

    impl Drop for ScratchAccount {
        fn drop(&mut self) {
            let _ = self.store.delete(&self.account);
        }
    }

    #[test]
    fn a_secret_round_trips_through_the_os_store() {
        let scratch = ScratchAccount::new();
        let (store, account) = (&scratch.store, scratch.account.as_str());
        let sentinel = format!("sk-roundtrip-{}", uuid::Uuid::new_v4());

        store.set(account, &sentinel).unwrap();
        assert_eq!(
            store.get(account).unwrap().as_deref(),
            Some(sentinel.as_str())
        );

        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
    }

    #[test]
    fn a_missing_secret_is_none_not_an_error() {
        let scratch = ScratchAccount::new();
        assert_eq!(scratch.store.get(&scratch.account).unwrap(), None);
    }

    #[test]
    fn deleting_a_missing_secret_is_ok() {
        let scratch = ScratchAccount::new();
        assert!(scratch.store.delete(&scratch.account).is_ok());
    }
}
