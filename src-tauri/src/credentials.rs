use std::{path::Path, sync::Mutex};

use secrecy::SecretString;
use tauri_plugin_stronghold::stronghold::Stronghold;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

const KEYRING_SERVICE: &str = "cn.rice-endosperm.daoxin";
const KEYRING_ACCOUNT: &str = "stronghold-unlock-key";
const API_KEY_RECORD: &[u8] = b"yuxi-api-key";

pub struct CredentialStore {
    vault: Mutex<Stronghold>,
}

impl CredentialStore {
    pub fn open(app_data_dir: &Path) -> AppResult<Self> {
        std::fs::create_dir_all(app_data_dir)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        let password = load_or_create_unlock_key()?;
        let vault_path = app_data_dir.join("credentials.hold");
        let vault = Stronghold::new(vault_path, password)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        Ok(Self {
            vault: Mutex::new(vault),
        })
    }

    pub fn has_api_key(&self) -> AppResult<bool> {
        let vault = self.lock()?;
        vault
            .store()
            .contains_key(API_KEY_RECORD)
            .map_err(|error| AppError::CredentialStore(error.to_string()))
    }

    pub fn save_api_key(&self, api_key: &str) -> AppResult<()> {
        validate_api_key(api_key)?;
        let vault = self.lock()?;
        vault
            .store()
            .insert(API_KEY_RECORD.to_vec(), api_key.as_bytes().to_vec(), None)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        vault
            .save()
            .map_err(|error| AppError::CredentialStore(error.to_string()))
    }

    pub fn api_key(&self) -> AppResult<SecretString> {
        let vault = self.lock()?;
        let bytes = Zeroizing::new(
            vault
                .store()
                .get(API_KEY_RECORD)
                .map_err(|error| AppError::CredentialStore(error.to_string()))?
                .ok_or(AppError::MissingCredential)?,
        );
        let key = String::from_utf8(bytes.to_vec())
            .map_err(|_| AppError::CredentialStore("凭证内容损坏".into()))?;
        Ok(SecretString::from(key))
    }

    pub fn delete_api_key(&self) -> AppResult<()> {
        let vault = self.lock()?;
        vault
            .store()
            .delete(API_KEY_RECORD)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        vault
            .save()
            .map_err(|error| AppError::CredentialStore(error.to_string()))
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Stronghold>> {
        self.vault
            .lock()
            .map_err(|_| AppError::CredentialStore("安全存储锁已损坏".into()))
    }
}

pub fn api_key_hint(api_key: &str) -> String {
    let visible = api_key.chars().take(12).collect::<String>();
    format!("{visible}••••••••")
}

pub fn validate_api_key(api_key: &str) -> AppResult<()> {
    let valid = api_key.starts_with("yxkey_")
        && (24..=256).contains(&api_key.len())
        && api_key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidCredential)
    }
}

fn load_or_create_unlock_key() -> AppResult<Vec<u8>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|error| AppError::CredentialStore(error.to_string()))?;
    match entry.get_secret() {
        Ok(secret) if secret.len() == 32 => Ok(secret),
        Ok(_) => Err(AppError::CredentialStore(
            "系统凭证中的解锁密钥长度无效".into(),
        )),
        Err(keyring::Error::NoEntry) => {
            let mut secret = Vec::with_capacity(32);
            secret.extend_from_slice(Uuid::new_v4().as_bytes());
            secret.extend_from_slice(Uuid::new_v4().as_bytes());
            entry
                .set_secret(&secret)
                .map_err(|error| AppError::CredentialStore(error.to_string()))?;
            Ok(secret)
        }
        Err(error) => Err(AppError::CredentialStore(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::{api_key_hint, validate_api_key};

    #[test]
    fn validates_expected_key_shape_without_exposing_it() {
        assert!(validate_api_key("yxkey_1234567890abcdefghijkl").is_ok());
        assert!(validate_api_key("sk-not-a-yuxi-key").is_err());
        assert_eq!(
            api_key_hint("yxkey_1234567890abcdefghijkl"),
            "yxkey_123456••••••••"
        );
    }
}
