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

/// 실패한 키체인 작업
#[derive(Debug, Clone, Copy)]
enum Op {
    Read,
    Save,
    Delete,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Save => "save",
            Op::Delete => "delete",
        }
    }
}

/// keyring 에러를 비밀 값 없이 설명 문자열로 변환합니다.
/// (`BadEncoding` 등 일부 변형은 저장된 바이트를 그대로 담고 있어 `Debug`로 찍으면 안 됩니다.)
///
/// OS가 돌려준 에러(macOS OSStatus, Windows 에러 코드)는 원인 파악에 필요하므로 설명과 함께 붙입니다.
/// 이 에러들은 키체인 항목이나 OS 상태를 설명할 뿐 비밀 값을 담지 않습니다.
fn store_error(op: Op, e: &keyring::Error) -> Error {
    let (kind, detail) = match e {
        keyring::Error::NoStorageAccess(inner) => {
            ("access denied or locked", Some(inner.to_string()))
        }
        keyring::Error::PlatformFailure(inner) => ("platform failure", Some(inner.to_string())),
        keyring::Error::NoDefaultStore => ("no keychain available on this platform", None),
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            ("stored secret has an unexpected format", None)
        }
        keyring::Error::Ambiguous(_) => ("multiple keychain entries match", None),
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => {
            ("invalid keychain entry", None)
        }
        _ => ("keychain error", None),
    };
    let msg = match detail {
        Some(d) => format!("keychain {} failed: {kind} ({d})", op.as_str()),
        None => format!("keychain {} failed: {kind}", op.as_str()),
    };
    tracing::warn!(op = op.as_str(), error = %msg, "keychain operation failed");
    Error::SecretStore(msg)
}

/// macOS 키체인 / Windows 자격 증명 관리자
pub struct KeyringStore;

impl KeyringStore {
    fn entry(profile_id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, &account(profile_id)).map_err(|e| store_error(Op::Read, &e))
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, profile_id: &str, secret: &SecretString) -> Result<()> {
        Self::entry(profile_id)?
            .set_password(secret.expose_secret())
            .map_err(|e| store_error(Op::Save, &e))
    }

    fn get(&self, profile_id: &str) -> Result<Option<SecretString>> {
        match Self::entry(profile_id)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(Op::Read, &e)),
        }
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        match Self::entry(profile_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(Op::Delete, &e)),
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
        let e = store_error(Op::Read, &keyring::Error::BadEncoding(b"hunter2".to_vec()));
        assert!(!format!("{e} {e:?}").contains("hunter2"));
    }

    #[test]
    fn store_error_names_operation_and_os_detail() {
        let os =
            std::io::Error::other("The specified item already exists in the keychain. (-25299)");
        let e = store_error(Op::Save, &keyring::Error::PlatformFailure(Box::new(os)));
        assert_eq!(
            e.to_string(),
            "secret store: keychain save failed: platform failure (The specified item already exists in the keychain. (-25299))"
        );
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
