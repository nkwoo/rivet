//! SQLite 연결과 스키마 마이그레이션.
//!
//! 스키마 버전은 `PRAGMA user_version`으로 관리합니다. 마이그레이션은 추가만 하고
//! 이미 배포된 항목은 수정하지 않습니다.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use rusqlite::Connection;

use crate::error::Result;

/// 순서대로 적용되는 마이그레이션. 인덱스 + 1이 해당 스키마 버전입니다.
const MIGRATIONS: &[&str] = &[
    // v1: 연결 프로필, known_hosts
    r#"
    CREATE TABLE profiles (
        id          TEXT PRIMARY KEY NOT NULL,
        name        TEXT NOT NULL,
        protocol    TEXT NOT NULL,
        host        TEXT NOT NULL,
        port        INTEGER NOT NULL,
        username    TEXT NOT NULL DEFAULT '',
        auth_method TEXT NOT NULL,
        key_path    TEXT,
        group_name  TEXT,
        tags        TEXT NOT NULL DEFAULT '[]',
        notes       TEXT NOT NULL DEFAULT '',
        favorite    INTEGER NOT NULL DEFAULT 0,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE INDEX profiles_group_name ON profiles (group_name, name);

    -- 호스트:포트마다 키 하나만 저장합니다. 다른 키가 오면 알고리즘이 달라도 '변경'으로 봅니다.
    CREATE TABLE known_hosts (
        host        TEXT NOT NULL,
        port        INTEGER NOT NULL,
        algorithm   TEXT NOT NULL,
        public_key  TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        added_at    INTEGER NOT NULL,
        PRIMARY KEY (host, port)
    );
    "#,
    // v2: TLS 인증서 지문 고정 (FTPS, 이후 RDP)
    r#"
    CREATE TABLE cert_pins (
        host        TEXT NOT NULL,
        port        INTEGER NOT NULL,
        fingerprint TEXT NOT NULL,
        added_at    INTEGER NOT NULL,
        PRIMARY KEY (host, port)
    );
    "#,
];

/// 앱 전체가 공유하는 SQLite 연결.
///
/// 쿼리는 짧고 동시성이 낮으므로 연결 하나를 `Mutex`로 보호합니다.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 연결을 잠그고 `f`를 실행합니다.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        // 다른 스레드가 패닉했더라도 SQLite 연결 자체는 일관된 상태이므로 계속 사용합니다.
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(f(&mut conn)?)
    }

    #[cfg(test)]
    pub fn schema_version(&self) -> Result<u32> {
        self.with(|c| c.pragma_query_value(None, "user_version", |r| r.get(0)))
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = (index + 1) as i64;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "database migrated");
    }
    Ok(())
}

/// 현재 시각(유닉스 초)
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_new_database_to_latest_version() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as u32);
    }

    #[test]
    fn reopening_existing_database_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rivet.db");
        {
            let db = Database::open(&path).unwrap();
            db.with(|c| {
                c.execute(
                    "INSERT INTO known_hosts VALUES ('h', 22, 'ssh-ed25519', 'k', 'f', 0)",
                    [],
                )
            })
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as u32);
        let count: i64 = db
            .with(|c| c.query_row("SELECT COUNT(*) FROM known_hosts", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(count, 1);
    }
}
