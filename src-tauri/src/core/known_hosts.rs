//! SSH known_hosts 저장소 (SQLite).
//!
//! 미등록 키는 SHA256 지문을 보여주고 승인 후 저장, 변경된 키는 연결을 차단합니다.
//! 바뀐 키는 덮어쓰지 않으며, 사용자가 기존 항목을 명시적으로 삭제해야만 새 키를 저장할 수 있습니다.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;

use crate::core::db::{now, Database};
use crate::error::{Error, Result};

/// 저장된 호스트 키
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// 저장된 키와 일치
    Trusted,
    /// 처음 보는 호스트
    Unknown,
    /// 저장된 키와 다름 (연결 차단 대상)
    Changed { known: KnownHost },
}

/// `SHA256:<base64>` 형식 지문 (`ssh-keygen -lf`와 같은 표기)
pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// 호스트 이름은 대소문자를 구분하지 않으므로 소문자로 저장합니다.
fn normalize_host(host: &str) -> String {
    host.trim().to_ascii_lowercase()
}

/// 비교용 OpenSSH 공개키 문자열 (주석 제외)
fn encode_key(key: &PublicKey) -> Result<String> {
    let mut key = key.clone();
    key.set_comment("");
    key.to_openssh()
        .map_err(|e| Error::Invalid(format!("cannot encode host key: {e}")))
}

pub struct KnownHostsStore {
    db: Arc<Database>,
}

impl KnownHostsStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus> {
        let host = normalize_host(host);
        let stored: Option<(String, KnownHost)> = self.db.with(|c| {
            c.query_row(
                "SELECT public_key, host, port, algorithm, fingerprint, added_at \
                 FROM known_hosts WHERE host = ?1 AND port = ?2",
                params![host, port],
                |r| {
                    Ok((
                        r.get(0)?,
                        KnownHost {
                            host: r.get(1)?,
                            port: r.get(2)?,
                            algorithm: r.get(3)?,
                            fingerprint: r.get(4)?,
                            added_at: r.get(5)?,
                        },
                    ))
                },
            )
            .optional()
        })?;

        let Some((stored_key, known)) = stored else {
            return Ok(HostKeyStatus::Unknown);
        };
        // 저장된 값이 손상되어 파싱할 수 없으면 '변경'으로 취급해 차단합니다.
        let matches = PublicKey::from_openssh(&stored_key)
            .map(|k| k.key_data() == key.key_data())
            .unwrap_or(false);
        Ok(if matches {
            HostKeyStatus::Trusted
        } else {
            HostKeyStatus::Changed { known }
        })
    }

    /// 처음 보는 호스트의 키를 저장합니다. 다른 키가 이미 저장되어 있으면 거부합니다.
    pub fn trust(&self, host: &str, port: u16, key: &PublicKey) -> Result<()> {
        match self.check(host, port, key)? {
            HostKeyStatus::Trusted => Ok(()),
            HostKeyStatus::Changed { .. } => Err(Error::Invalid(
                "a different key is already saved for this host; remove it first".into(),
            )),
            HostKeyStatus::Unknown => {
                let encoded = encode_key(key)?;
                self.db.with(|c| {
                    c.execute(
                        "INSERT INTO known_hosts (host, port, algorithm, public_key, fingerprint, added_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            normalize_host(host),
                            port,
                            key.algorithm().as_str(),
                            encoded,
                            fingerprint(key),
                            now(),
                        ],
                    )
                })?;
                Ok(())
            }
        }
    }

    pub fn list(&self) -> Result<Vec<KnownHost>> {
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT host, port, algorithm, fingerprint, added_at FROM known_hosts \
                 ORDER BY host, port",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(KnownHost {
                    host: r.get(0)?,
                    port: r.get(1)?,
                    algorithm: r.get(2)?,
                    fingerprint: r.get(3)?,
                    added_at: r.get(4)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn remove(&self, host: &str, port: u16) -> Result<()> {
        let changed = self.db.with(|c| {
            c.execute(
                "DELETE FROM known_hosts WHERE host = ?1 AND port = ?2",
                params![normalize_host(host), port],
            )
        })?;
        if changed == 0 {
            return Err(Error::NotFound(format!("known host {host}:{port}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ssh-keygen -t ed25519 으로 만든 테스트용 공개키
    const KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILivYAx9W//3gPpBQ8sYmjiLyHYjP1Ow9bzH+Vs7rGm6 a@test";
    const KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH1ArEwSuXOuvCHDno5dXaabz69ztbFwskyJdWJes6uG b@test";

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).unwrap()
    }

    fn store() -> KnownHostsStore {
        KnownHostsStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn unknown_then_trusted_after_approval() {
        let s = store();
        let a = key(KEY_A);
        assert_eq!(s.check("Web-01", 22, &a).unwrap(), HostKeyStatus::Unknown);
        s.trust("Web-01", 22, &a).unwrap();
        assert_eq!(s.check("web-01", 22, &a).unwrap(), HostKeyStatus::Trusted);
        // 포트가 다르면 다른 호스트
        assert_eq!(s.check("web-01", 2222, &a).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn changed_key_is_reported_and_cannot_be_overwritten() {
        let s = store();
        s.trust("web-01", 22, &key(KEY_A)).unwrap();
        let b = key(KEY_B);
        match s.check("web-01", 22, &b).unwrap() {
            HostKeyStatus::Changed { known } => {
                assert_eq!(known.fingerprint, fingerprint(&key(KEY_A)));
            }
            other => panic!("expected Changed, got {other:?}"),
        }
        assert!(s.trust("web-01", 22, &b).is_err());

        // 명시적으로 삭제한 뒤에만 새 키를 저장할 수 있음
        s.remove("web-01", 22).unwrap();
        s.trust("web-01", 22, &b).unwrap();
        assert_eq!(s.check("web-01", 22, &b).unwrap(), HostKeyStatus::Trusted);
    }

    #[test]
    fn corrupted_stored_key_is_treated_as_changed() {
        let s = store();
        s.trust("h", 22, &key(KEY_A)).unwrap();
        s.db.with(|c| c.execute("UPDATE known_hosts SET public_key = 'garbage'", []))
            .unwrap();
        assert!(matches!(
            s.check("h", 22, &key(KEY_A)).unwrap(),
            HostKeyStatus::Changed { .. }
        ));
    }

    #[test]
    fn fingerprint_uses_openssh_sha256_format() {
        let fp = fingerprint(&key(KEY_A));
        assert!(fp.starts_with("SHA256:"));
        assert!(!fp.ends_with('='));
    }

    #[test]
    fn list_and_remove() {
        let s = store();
        s.trust("b", 22, &key(KEY_A)).unwrap();
        s.trust("a", 22, &key(KEY_B)).unwrap();
        let hosts: Vec<_> = s.list().unwrap().into_iter().map(|h| h.host).collect();
        assert_eq!(hosts, ["a", "b"]);
        assert!(matches!(s.remove("zzz", 22), Err(Error::NotFound(_))));
    }
}
