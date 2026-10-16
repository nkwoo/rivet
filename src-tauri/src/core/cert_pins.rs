//! TLS 인증서 지문 고정 저장소 (SQLite).
//!
//! OS 신뢰 저장소로 검증되지 않는 인증서(자체 서명 등)는 지문을 사용자에게 보여주고 승인받아
//! 저장합니다(TOFU). 저장된 지문과 다른 인증서는 차단하며, 사용자가 항목을 직접 삭제해야만
//! 새 지문을 저장할 수 있습니다. known_hosts와 같은 규칙입니다.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::core::db::{now, Database};
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertPin {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinStatus {
    Pinned,
    Unknown,
    Changed { known: CertPin },
}

fn normalize_host(host: &str) -> String {
    host.trim().to_ascii_lowercase()
}

/// 지문 표기를 비교용으로 맞춥니다 (`AB:CD:..` 대문자)
fn normalize_fingerprint(fp: &str) -> String {
    fp.trim().to_ascii_uppercase()
}

pub struct CertPinStore {
    db: Arc<Database>,
}

impl CertPinStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 이 호스트에 고정된 지문
    pub fn pinned(&self, host: &str, port: u16) -> Result<Option<String>> {
        Ok(self.get(host, port)?.map(|p| p.fingerprint))
    }

    fn get(&self, host: &str, port: u16) -> Result<Option<CertPin>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT host, port, fingerprint, added_at FROM cert_pins WHERE host = ?1 AND port = ?2",
                params![normalize_host(host), port],
                |r| {
                    Ok(CertPin {
                        host: r.get(0)?,
                        port: r.get(1)?,
                        fingerprint: r.get(2)?,
                        added_at: r.get(3)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn check(&self, host: &str, port: u16, fingerprint: &str) -> Result<PinStatus> {
        Ok(match self.get(host, port)? {
            None => PinStatus::Unknown,
            Some(known) if known.fingerprint == normalize_fingerprint(fingerprint) => {
                PinStatus::Pinned
            }
            Some(known) => PinStatus::Changed { known },
        })
    }

    /// 처음 보는 인증서 지문을 저장합니다. 다른 지문이 이미 있으면 거부합니다.
    pub fn pin(&self, host: &str, port: u16, fingerprint: &str) -> Result<()> {
        match self.check(host, port, fingerprint)? {
            PinStatus::Pinned => Ok(()),
            PinStatus::Changed { .. } => Err(Error::Invalid(
                "a different certificate is already pinned for this host; remove it first".into(),
            )),
            PinStatus::Unknown => {
                self.db.with(|c| {
                    c.execute(
                        "INSERT INTO cert_pins (host, port, fingerprint, added_at) VALUES (?1, ?2, ?3, ?4)",
                        params![
                            normalize_host(host),
                            port,
                            normalize_fingerprint(fingerprint),
                            now()
                        ],
                    )
                })?;
                Ok(())
            }
        }
    }

    pub fn list(&self) -> Result<Vec<CertPin>> {
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT host, port, fingerprint, added_at FROM cert_pins ORDER BY host, port",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(CertPin {
                    host: r.get(0)?,
                    port: r.get(1)?,
                    fingerprint: r.get(2)?,
                    added_at: r.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn remove(&self, host: &str, port: u16) -> Result<()> {
        let changed = self.db.with(|c| {
            c.execute(
                "DELETE FROM cert_pins WHERE host = ?1 AND port = ?2",
                params![normalize_host(host), port],
            )
        })?;
        if changed == 0 {
            return Err(Error::NotFound(format!("pinned certificate {host}:{port}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> CertPinStore {
        CertPinStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn pin_check_change_and_remove() {
        let s = store();
        assert_eq!(
            s.check("Ftp.Example", 21, "aa:bb").unwrap(),
            PinStatus::Unknown
        );
        s.pin("Ftp.Example", 21, "aa:bb").unwrap();
        assert_eq!(
            s.check("ftp.example", 21, "AA:BB").unwrap(),
            PinStatus::Pinned
        );
        assert_eq!(
            s.check("ftp.example", 990, "AA:BB").unwrap(),
            PinStatus::Unknown
        );
        assert!(matches!(
            s.check("ftp.example", 21, "CC:DD").unwrap(),
            PinStatus::Changed { known } if known.fingerprint == "AA:BB"
        ));
        assert!(s.pin("ftp.example", 21, "CC:DD").is_err());
        assert_eq!(s.list().unwrap().len(), 1);
        s.remove("ftp.example", 21).unwrap();
        assert!(matches!(
            s.remove("ftp.example", 21),
            Err(Error::NotFound(_))
        ));
        s.pin("ftp.example", 21, "CC:DD").unwrap();
    }
}
