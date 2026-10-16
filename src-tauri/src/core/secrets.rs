//! OS 보안 저장소(keyring) 래퍼.
//!
//! 키 형식: 서비스 `dev.rivet.client`, 계정 `rivet:<profile_id>`.
//! 비밀 값은 `SecretString`으로만 주고받으며, 로그/에러 메시지에 값이 들어가지 않게 합니다.

use secrecy::{ExposeSecret, SecretString};

use crate::error::{Error, Result};

const SERVICE: &str = "dev.rivet.client";

/// 프로필별 비밀 값(비밀번호 또는 키 패스프레이즈) 저장소
pub trait SecretStore: Send + Sync {
    fn set(&self, profile_id: &str, secret: &SecretString) -> Result<()>;
    /// 저장된 값이 없으면 `Ok(None)`
    fn get(&self, profile_id: &str) -> Result<Option<SecretString>>;
    /// 저장된 값이 없어도 성공으로 봅니다.
    fn delete(&self, profile_id: &str) -> Result<()>;
}

fn account(profile_id: &str) -> String {
    format!("rivet:{profile_id}")
}

/// keyring 에러를 비밀 값 없이 설명 문자열로 변환합니다.
/// (`BadEncoding` 등 일부 변형은 저장된 바이트를 그대로 담고 있어 `Debug`로 찍으면 안 됩니다.)
fn store_error(e: &keyring::Error) -> Error {
    let msg = match e {
        keyring::Error::NoStorageAccess(_) => "keychain access denied or locked",
        keyring::Error::PlatformFailure(_) => "keychain platform failure",
        keyring::Error::NoDefaultStore => "no keychain available on this platform",
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            "stored secret has an unexpected format"
        }
        keyring::Error::Ambiguous(_) => "multiple keychain entries match",
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => "invalid keychain entry",
        _ => "keychain error",
    };
    Error::SecretStore(msg.into())
}

/// macOS 키체인 / Windows 자격 증명 관리자
pub struct KeyringStore;

impl KeyringStore {
    fn entry(profile_id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, &account(profile_id)).map_err(|e| store_error(&e))
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, profile_id: &str, secret: &SecretString) -> Result<()> {
        Self::entry(profile_id)?
            .set_password(secret.expose_secret())
            .map_err(|e| store_error(&e))
    }

    fn get(&self, profile_id: &str) -> Result<Option<SecretString>> {
        match Self::entry(profile_id)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(&e)),
        }
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        match Self::entry(profile_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(&e)),
        }
    }
}

/// 테스트용 메모리 저장소
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    values: std::sync::Mutex<std::collections::HashMap<String, SecretString>>,
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn set(&self, profile_id: &str, secret: &SecretString) -> Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(account(profile_id), secret.clone());
        Ok(())
    }

    fn get(&self, profile_id: &str) -> Result<Option<SecretString>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&account(profile_id))
            .cloned())
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        self.values.lock().unwrap().remove(&account(profile_id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_uses_rivet_prefix() {
        assert_eq!(account("abc"), "rivet:abc");
    }

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        assert!(s.get("p").unwrap().is_none());
        s.set("p", &SecretString::from("hunter2")).unwrap();
        assert_eq!(s.get("p").unwrap().unwrap().expose_secret(), "hunter2");
        s.delete("p").unwrap();
        s.delete("p").unwrap();
        assert!(s.get("p").unwrap().is_none());
    }

    #[test]
    fn store_error_never_contains_secret_bytes() {
        let e = store_error(&keyring::Error::BadEncoding(b"hunter2".to_vec()));
        assert!(!format!("{e} {e:?}").contains("hunter2"));
    }

    #[test]
    fn secret_string_debug_is_redacted() {
        let s = SecretString::from("hunter2");
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    /// 실제 OS 키체인 사용. 로컬에서 `cargo test -- --ignored keyring`으로 실행합니다.
    #[test]
    #[ignore = "touches the real OS keychain"]
    fn keyring_store_round_trip() {
        let s = KeyringStore;
        let id = format!("test-{}", uuid::Uuid::new_v4());
        s.set(&id, &SecretString::from("hunter2")).unwrap();
        assert_eq!(s.get(&id).unwrap().unwrap().expose_secret(), "hunter2");
        s.delete(&id).unwrap();
        assert!(s.get(&id).unwrap().is_none());
    }
}
