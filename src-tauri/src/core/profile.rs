//! 연결 프로필 CRUD (SQLite).
//!
//! 비밀번호와 패스프레이즈는 이 저장소에 절대 기록하지 않습니다 (`secrets` 사용).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::core::db::{now, Database};
use crate::error::{Error, Result};

const MAX_NAME: usize = 100;
const MAX_HOST: usize = 253;
const MAX_USERNAME: usize = 128;
const MAX_PATH: usize = 1024;
const MAX_GROUP: usize = 64;
const MAX_TAGS: usize = 16;
const MAX_TAG: usize = 32;
const MAX_NOTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ssh,
    Sftp,
    Ftp,
    Ftps,
    Rdp,
}

impl Protocol {
    fn as_str(self) -> &'static str {
        match self {
            Protocol::Ssh => "ssh",
            Protocol::Sftp => "sftp",
            Protocol::Ftp => "ftp",
            Protocol::Ftps => "ftps",
            Protocol::Rdp => "rdp",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "ssh" => Protocol::Ssh,
            "sftp" => Protocol::Sftp,
            "ftp" => Protocol::Ftp,
            "ftps" => Protocol::Ftps,
            "rdp" => Protocol::Rdp,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    Key,
    Agent,
}

impl AuthMethod {
    fn as_str(self) -> &'static str {
        match self {
            AuthMethod::Password => "password",
            AuthMethod::Key => "key",
            AuthMethod::Agent => "agent",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "password" => AuthMethod::Password,
            "key" => AuthMethod::Key,
            "agent" => AuthMethod::Agent,
            _ => return None,
        })
    }
}

/// 저장된 연결 프로필. 비밀 값 필드는 의도적으로 없습니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub key_path: Option<String>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 프로필 생성/수정 입력 (웹뷰에서 들어오므로 신뢰하지 않고 검증합니다)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    pub auth_method: AuthMethod,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub favorite: bool,
}

impl ProfileInput {
    /// 앞뒤 공백을 정리하고 값을 검증합니다.
    pub fn normalized(mut self) -> Result<Self> {
        self.name = self.name.trim().to_owned();
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        self.key_path = non_empty(self.key_path);
        self.group = non_empty(self.group);
        self.tags = self
            .tags
            .into_iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        self.tags.dedup();

        check_text("name", &self.name, 1, MAX_NAME)?;
        check_host(&self.host)?;
        if self.port == 0 {
            return Err(Error::Invalid("port must be 1-65535".into()));
        }
        check_text("username", &self.username, 0, MAX_USERNAME)?;
        if let Some(path) = &self.key_path {
            check_text("keyPath", path, 1, MAX_PATH)?;
        }
        if let Some(group) = &self.group {
            check_text("group", group, 1, MAX_GROUP)?;
        }
        if self.tags.len() > MAX_TAGS {
            return Err(Error::Invalid(format!("at most {MAX_TAGS} tags")));
        }
        for tag in &self.tags {
            check_text("tag", tag, 1, MAX_TAG)?;
        }
        if self.notes.chars().count() > MAX_NOTES {
            return Err(Error::Invalid(format!("notes longer than {MAX_NOTES}")));
        }
        if matches!(self.protocol, Protocol::Ssh | Protocol::Sftp) {
            if self.username.is_empty() {
                return Err(Error::Invalid("username is required for SSH".into()));
            }
            if self.auth_method == AuthMethod::Key && self.key_path.is_none() {
                return Err(Error::Invalid("key file is required for key auth".into()));
            }
        }
        Ok(self)
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn check_text(field: &str, value: &str, min: usize, max: usize) -> Result<()> {
    let len = value.chars().count();
    if len < min || len > max {
        return Err(Error::Invalid(format!("{field} must be {min}-{max} chars")));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::Invalid(format!(
            "{field} contains control characters"
        )));
    }
    Ok(())
}

/// 호스트 이름 또는 IP. 공백/제어 문자, 옵션처럼 보이는 값(`-oProxyCommand` 등)을 거부합니다.
fn check_host(host: &str) -> Result<()> {
    check_text("host", host, 1, MAX_HOST)?;
    let valid = !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'));
    if !valid {
        return Err(Error::Invalid("host contains invalid characters".into()));
    }
    Ok(())
}

/// 프로필 저장소
pub struct ProfileStore {
    db: Arc<Database>,
}

const COLUMNS: &str = "id, name, protocol, host, port, username, auth_method, key_path, \
                       group_name, tags, notes, favorite, created_at, updated_at";

impl ProfileStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 그룹, 이름 순으로 정렬된 전체 목록
    pub fn list(&self) -> Result<Vec<Profile>> {
        self.db.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM profiles \
                 ORDER BY group_name IS NULL, group_name COLLATE NOCASE, name COLLATE NOCASE"
            ))?;
            let rows = stmt.query_map([], from_row)?;
            rows.collect()
        })
    }

    pub fn get(&self, id: &str) -> Result<Profile> {
        self.db
            .with(|c| {
                c.query_row(
                    &format!("SELECT {COLUMNS} FROM profiles WHERE id = ?1"),
                    [id],
                    from_row,
                )
                .optional()
            })?
            .ok_or_else(|| Error::NotFound(format!("profile {id}")))
    }

    pub fn create(&self, input: ProfileInput) -> Result<Profile> {
        let input = input.normalized()?;
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now();
        let tags = serde_json::to_string(&input.tags).map_err(|e| Error::Invalid(e.to_string()))?;
        self.db.with(|c| {
            c.execute(
                &format!("INSERT INTO profiles ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)"),
                params![
                    id,
                    input.name,
                    input.protocol.as_str(),
                    input.host,
                    input.port,
                    input.username,
                    input.auth_method.as_str(),
                    input.key_path,
                    input.group,
                    tags,
                    input.notes,
                    input.favorite,
                    ts,
                ],
            )
        })?;
        self.get(&id)
    }

    pub fn update(&self, id: &str, input: ProfileInput) -> Result<Profile> {
        let input = input.normalized()?;
        let tags = serde_json::to_string(&input.tags).map_err(|e| Error::Invalid(e.to_string()))?;
        let changed = self.db.with(|c| {
            c.execute(
                "UPDATE profiles SET name = ?2, protocol = ?3, host = ?4, port = ?5, username = ?6, \
                 auth_method = ?7, key_path = ?8, group_name = ?9, tags = ?10, notes = ?11, \
                 favorite = ?12, updated_at = ?13 WHERE id = ?1",
                params![
                    id,
                    input.name,
                    input.protocol.as_str(),
                    input.host,
                    input.port,
                    input.username,
                    input.auth_method.as_str(),
                    input.key_path,
                    input.group,
                    tags,
                    input.notes,
                    input.favorite,
                    now(),
                ],
            )
        })?;
        if changed == 0 {
            return Err(Error::NotFound(format!("profile {id}")));
        }
        self.get(id)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let changed = self
            .db
            .with(|c| c.execute("DELETE FROM profiles WHERE id = ?1", [id]))?;
        if changed == 0 {
            return Err(Error::NotFound(format!("profile {id}")));
        }
        Ok(())
    }
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Profile> {
    let protocol: String = row.get(2)?;
    let auth: String = row.get(6)?;
    let tags: String = row.get(9)?;
    let invalid = |col: usize, what: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("unknown {what}").into(),
        )
    };
    Ok(Profile {
        id: row.get(0)?,
        name: row.get(1)?,
        protocol: Protocol::parse(&protocol).ok_or_else(|| invalid(2, "protocol"))?,
        host: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        auth_method: AuthMethod::parse(&auth).ok_or_else(|| invalid(6, "auth method"))?,
        key_path: row.get(7)?,
        group: row.get(8)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        notes: row.get(10)?,
        favorite: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ProfileStore {
        ProfileStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn input(name: &str) -> ProfileInput {
        ProfileInput {
            name: name.into(),
            protocol: Protocol::Ssh,
            host: "10.0.4.11".into(),
            port: 22,
            username: "deploy".into(),
            auth_method: AuthMethod::Password,
            key_path: None,
            group: Some("Production".into()),
            tags: vec!["web".into()],
            notes: String::new(),
            favorite: false,
        }
    }

    #[test]
    fn create_get_update_delete_round_trip() {
        let s = store();
        let created = s.create(input("web-01")).unwrap();
        assert_eq!(s.get(&created.id).unwrap(), created);
        assert_eq!(created.tags, vec!["web".to_string()]);

        let mut changed = input("web-01b");
        changed.port = 2222;
        changed.auth_method = AuthMethod::Key;
        changed.key_path = Some("~/.ssh/id_ed25519".into());
        let updated = s.update(&created.id, changed).unwrap();
        assert_eq!(updated.name, "web-01b");
        assert_eq!(updated.port, 2222);
        assert_eq!(updated.created_at, created.created_at);

        s.delete(&created.id).unwrap();
        assert!(matches!(s.get(&created.id), Err(Error::NotFound(_))));
        assert!(matches!(s.delete(&created.id), Err(Error::NotFound(_))));
    }

    #[test]
    fn list_orders_by_group_then_name_with_ungrouped_last() {
        let s = store();
        let mut a = input("zeta");
        a.group = None;
        s.create(a).unwrap();
        s.create(input("beta")).unwrap();
        let mut c = input("alpha");
        c.group = Some("Legacy".into());
        s.create(c).unwrap();
        let names: Vec<_> = s.list().unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["alpha", "beta", "zeta"]);
    }

    #[test]
    fn update_missing_profile_is_not_found() {
        assert!(matches!(
            store().update("nope", input("x")),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn normalizes_whitespace_and_empty_optionals() {
        let mut i = input("  web  ");
        i.host = " example.com ".into();
        i.group = Some("   ".into());
        i.tags = vec![" a ".into(), "".into()];
        let n = i.normalized().unwrap();
        assert_eq!(n.name, "web");
        assert_eq!(n.host, "example.com");
        assert_eq!(n.group, None);
        assert_eq!(n.tags, vec!["a".to_string()]);
    }

    type Mutation = dyn Fn(&mut ProfileInput);

    #[test]
    fn rejects_invalid_input() {
        let cases: Vec<Box<Mutation>> = vec![
            Box::new(|i| i.name = String::new()),
            Box::new(|i| i.name = "a\nb".into()),
            Box::new(|i| i.host = "-oProxyCommand=evil".into()),
            Box::new(|i| i.host = "bad host".into()),
            Box::new(|i| i.host = "a".repeat(300)),
            Box::new(|i| i.port = 0),
            Box::new(|i| i.username = String::new()),
            Box::new(|i| i.username = "ro\rot".into()),
            Box::new(|i| {
                i.auth_method = AuthMethod::Key;
                i.key_path = None;
            }),
            Box::new(|i| i.tags = (0..20).map(|n| n.to_string()).collect()),
        ];
        for (n, mutate) in cases.iter().enumerate() {
            let mut i = input("x");
            mutate(&mut i);
            assert!(
                matches!(i.normalized(), Err(Error::Invalid(_))),
                "case {n} should be rejected"
            );
        }
    }

    #[test]
    fn accepts_ipv6_and_hostnames() {
        for host in ["::1", "[fe80::1]", "db-01.internal", "my_host"] {
            let mut i = input("x");
            i.host = host.into();
            assert!(i.normalized().is_ok(), "{host}");
        }
    }

    #[test]
    fn input_rejects_unknown_fields_such_as_password() {
        let json = serde_json::json!({
            "name": "x", "protocol": "ssh", "host": "h", "port": 22,
            "username": "u", "authMethod": "password", "password": "hunter2"
        });
        assert!(serde_json::from_value::<ProfileInput>(json).is_err());
    }
}
