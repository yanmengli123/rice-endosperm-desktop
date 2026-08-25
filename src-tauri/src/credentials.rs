use std::{path::Path, sync::Mutex};

use chrono::Utc;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use tauri_plugin_stronghold::stronghold::Stronghold;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::{
    diagnostics,
    error::{AppError, AppResult},
};

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
        let (password, key_created) = load_or_create_unlock_key()?;
        let vault_path = app_data_dir.join("credentials.hold");
        match Stronghold::new(vault_path.clone(), password.clone()) {
            Ok(vault) => Ok(Self {
                vault: Mutex::new(vault),
            }),
            Err(_) if key_created => {
                // 走到这里说明解锁密钥是新生成的，但已有快照无法用它解密——
                // 典型场景是系统凭据库里的解锁密钥被清理工具删除。把旧快照
                // 备份后重建空 vault：应用可以继续启动，已存 API Key 需要
                // 用户重新配置。没有这条自愈路径时应用会每次启动都失败。
                let backup_path = app_data_dir.join(format!(
                    "credentials.stale-{}.hold",
                    Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
                ));
                std::fs::rename(&vault_path, &backup_path)
                    .map_err(|error| AppError::CredentialStore(error.to_string()))?;
                diagnostics::log(
                    "WARN",
                    "credential_vault_rebuilt",
                    &format!(
                        "unlock key was regenerated; stale snapshot moved to {}",
                        backup_path.display()
                    ),
                );
                let vault = Stronghold::new(vault_path, password)
                    .map_err(|error| AppError::CredentialStore(error.to_string()))?;
                Ok(Self {
                    vault: Mutex::new(vault),
                })
            }
            Err(error) => Err(AppError::CredentialStore(error.to_string())),
        }
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
        let mut previous = vault
            .store()
            .get(API_KEY_RECORD)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        vault
            .store()
            .insert(API_KEY_RECORD.to_vec(), api_key.as_bytes().to_vec(), None)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        if let Err(save_error) = vault.save() {
            let rollback = match previous.take() {
                Some(secret) => vault.store().insert(API_KEY_RECORD.to_vec(), secret, None),
                None => vault.store().delete(API_KEY_RECORD),
            };
            if let Err(rollback_error) = rollback {
                return Err(AppError::CredentialStore(format!(
                    "凭证落盘失败且内存回滚失败: {save_error}; {rollback_error}"
                )));
            }
            return Err(AppError::CredentialStore(save_error.to_string()));
        }
        if let Some(secret) = previous.as_mut() {
            secret.zeroize();
        }
        Ok(())
    }

    pub fn api_key(&self) -> AppResult<SecretString> {
        let vault = self.lock()?;
        let bytes = vault
            .store()
            .get(API_KEY_RECORD)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?
            .ok_or(AppError::MissingCredential)?;
        // 缓冲直接 move 进 String → SecretString，全程只存在一份明文，
        // SecretString 在 drop 时清零；UTF-8 校验失败时也显式清零。
        match String::from_utf8(bytes) {
            Ok(key) => Ok(SecretString::from(key)),
            Err(error) => {
                let mut bytes = error.into_bytes();
                bytes.zeroize();
                Err(AppError::CredentialStore("凭证内容损坏".into()))
            }
        }
    }

    pub fn delete_api_key(&self) -> AppResult<()> {
        let vault = self.lock()?;
        let mut previous = vault
            .store()
            .get(API_KEY_RECORD)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        vault
            .store()
            .delete(API_KEY_RECORD)
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        if let Err(save_error) = vault.save() {
            if let Some(secret) = previous.take()
                && let Err(rollback_error) =
                    vault.store().insert(API_KEY_RECORD.to_vec(), secret, None)
            {
                return Err(AppError::CredentialStore(format!(
                    "凭证删除落盘失败且内存回滚失败: {save_error}; {rollback_error}"
                )));
            }
            return Err(AppError::CredentialStore(save_error.to_string()));
        }
        if let Some(secret) = previous.as_mut() {
            secret.zeroize();
        }
        Ok(())
    }

    // ---- P2b 多账号：按作用域隔离的凭据与会话存储 ----
    //
    // ACTIVE 记录（b"yuxi-api-key"）只是"当前选中账号"的缓存；真源是
    // b"yuxi-api-key:{scope}" / b"yuxi-session:{scope}"。切换账号 = 把
    // 作用域记录拷回 ACTIVE + 更新 app_settings 指针。

    fn api_key_record_for(scope: &str) -> Vec<u8> {
        format!("yuxi-api-key:{scope}").into_bytes()
    }

    fn session_record_for(scope: &str) -> Vec<u8> {
        format!("yuxi-session:{scope}").into_bytes()
    }

    pub fn save_api_key_for_scope(&self, scope: &str, api_key: &str) -> AppResult<()> {
        validate_api_key(api_key)?;
        let vault = self.lock()?;
        vault
            .store()
            .insert(
                Self::api_key_record_for(scope),
                api_key.as_bytes().to_vec(),
                None,
            )
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        vault
            .save()
            .map_err(|error| AppError::CredentialStore(error.to_string()))
    }

    pub fn api_key_for_scope(&self, scope: &str) -> AppResult<Option<SecretString>> {
        let vault = self.lock()?;
        match vault
            .store()
            .get(&Self::api_key_record_for(scope))
            .map_err(|error| AppError::CredentialStore(error.to_string()))?
        {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(key) => Ok(Some(SecretString::from(key))),
                Err(error) => {
                    let mut raw = error.into_bytes();
                    raw.zeroize();
                    Err(AppError::CredentialStore("凭证内容损坏".into()))
                }
            },
            None => Ok(None),
        }
    }

    pub fn save_session_blob(&self, scope: &str, json: &str) -> AppResult<()> {
        let vault = self.lock()?;
        vault
            .store()
            .insert(
                Self::session_record_for(scope),
                json.as_bytes().to_vec(),
                None,
            )
            .map_err(|error| AppError::CredentialStore(error.to_string()))?;
        vault
            .save()
            .map_err(|error| AppError::CredentialStore(error.to_string()))
    }

    pub fn session_blob(&self, scope: &str) -> AppResult<Option<String>> {
        let vault = self.lock()?;
        match vault
            .store()
            .get(&Self::session_record_for(scope))
            .map_err(|error| AppError::CredentialStore(error.to_string()))?
        {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(json) => Ok(Some(json)),
                Err(error) => {
                    let mut raw = error.into_bytes();
                    raw.zeroize();
                    Err(AppError::CredentialStore("会话内容损坏".into()))
                }
            },
            None => Ok(None),
        }
    }

    pub fn delete_scope_records(&self, scope: &str) -> AppResult<()> {
        let vault = self.lock()?;
        for record in [
            Self::api_key_record_for(scope),
            Self::session_record_for(scope),
        ] {
            if vault
                .store()
                .contains_key(&record)
                .map_err(|error| AppError::CredentialStore(error.to_string()))?
            {
                vault
                    .store()
                    .delete(&record)
                    .map_err(|error| AppError::CredentialStore(error.to_string()))?;
            }
        }
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

/// 旧版服务端无法返回用户 UID 时，使用完整 API Key 的不可逆摘要隔离本地账号。
/// 不能使用展示用 key_prefix：它只有很短的可见前缀，碰撞会把两个账号的会话混在一起。
pub fn api_key_scope_id(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    format!("api-key-sha256:{digest:x}")
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

fn load_or_create_unlock_key() -> AppResult<(Vec<u8>, bool)> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|error| AppError::CredentialStore(error.to_string()))?;
    match entry.get_secret() {
        Ok(secret) if secret.len() == 32 => Ok((secret, false)),
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
            Ok((secret, true))
        }
        Err(error) => Err(AppError::CredentialStore(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::{api_key_hint, api_key_scope_id, validate_api_key};

    #[test]
    fn validates_expected_key_shape_without_exposing_it() {
        assert!(validate_api_key("yxkey_1234567890abcdefghijkl").is_ok());
        assert!(validate_api_key("sk-not-a-yuxi-key").is_err());
        assert_eq!(
            api_key_hint("yxkey_1234567890abcdefghijkl"),
            "yxkey_123456••••••••"
        );
    }

    #[test]
    fn derives_collision_resistant_account_scope_without_exposing_key() {
        let first = api_key_scope_id("yxkey_1234567890abcdefghijkl");
        let second = api_key_scope_id("yxkey_1234567890abcdefghijkm");
        assert!(first.starts_with("api-key-sha256:"));
        assert_eq!(first.len(), "api-key-sha256:".len() + 64);
        assert_ne!(first, second);
        assert!(!first.contains("yxkey_"));
    }
}
