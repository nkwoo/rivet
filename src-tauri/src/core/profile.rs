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
/// RDP 해상도 범위 ([MS-RDPEDISP] 2.2.2.2.1과 같은 기준)
const RDP_MIN_DIM: u16 = 200;
const RDP_MAX_DIM: u16 = 8192;
/// 점프 호스트를 거치는 단계 수 상한 (A → B → C → D → 대상)
pub const MAX_JUMP_HOPS: usize = 4;

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

/// RDP 키보드 입력 방식
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyboardMode {
    /// 로컬(맥) 입력기가 조합한 글자를 유니코드로 보냄
    Local,
    /// 물리 키를 스캔 코드로 보내 원격 Windows 입력기가 조합함 (자음·모음이 바로 보여 기본값)
    #[default]
    Remote,
}

/// 원격 입력기 모드에서 원격의 한/영 전환에 쓰는 키
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HangulKey {
    RightAlt,
    /// 한국어 키보드의 한/영 키 (Lang1)
    #[default]
    Lang1,
    ShiftSpace,
}

/// RDP 화면·키보드 설정
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RdpSettings {
    /// 고정 해상도. 없으면 탭 크기에 맞추고 탭 크기가 바뀌면 원격 해상도도 바꿉니다.
    #[serde(default)]
    pub width: Option<u16>,
    #[serde(default)]
    pub height: Option<u16>,
    /// 원격 화면 배율(%) 100~500. 없으면 100%, `hidpi`면 디스플레이 배율을 따릅니다.
    #[serde(default)]
    pub scale: Option<u16>,
    /// 창에 맞춤일 때 Retina 등 고해상도 디스플레이의 실제 픽셀 수로 접속
    #[serde(default)]
    pub hidpi: bool,
    #[serde(default)]
    pub keyboard: KeyboardMode,
    #[serde(default)]
    pub hangul_key: HangulKey,
}

impl RdpSettings {
    fn check(&self) -> Result<()> {
        let dim = |n: u16| (RDP_MIN_DIM..=RDP_MAX_DIM).contains(&n);
        match (self.width, self.height) {
            (None, None) => {}
            (Some(w), Some(h)) if dim(w) && dim(h) && w % 2 == 0 => {}
            _ => {
                return Err(Error::Invalid(format!(
                    "RDP resolution must be an even width and height in {RDP_MIN_DIM}-{RDP_MAX_DIM}"
                )))
            }
        }
        if let Some(scale) = self.scale {
            if !(100..=500).contains(&scale) {
                return Err(Error::Invalid("RDP scale must be 100-500%".into()));
            }
        }
        Ok(())
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
    /// RDP 프로필의 화면 설정 (다른 프로토콜은 `None`)
    pub rdp: Option<RdpSettings>,
    /// 점프 호스트로 쓸 SSH 프로필 ID (SSH/SFTP 프로필만)
    pub jump_profile_id: Option<String>,
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
    #[serde(default)]
    pub rdp: Option<RdpSettings>,
    #[serde(default)]
    pub jump_profile_id: Option<String>,
}

impl ProfileInput {
    /// 앞뒤 공백을 정리하고 값을 검증합니다.
    pub fn normalized(mut self) -> Result<Self> {
        self.name = self.name.trim().to_owned();
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        self.key_path = non_empty(self.key_path);
        self.group = non_empty(self.group);
        self.jump_profile_id = non_empty(self.jump_profile_id);
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
        } else {
            self.jump_profile_id = None;
        }
        if self.protocol == Protocol::Rdp {
            if self.auth_method != AuthMethod::Password {
                return Err(Error::Invalid("RDP supports password auth only".into()));
            }
            self.rdp.unwrap_or_default().check()?;
        } else {
            self.rdp = None;
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
                       group_name, tags, notes, favorite, created_at, updated_at, rdp_settings, \
                       jump_profile_id";

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

    /// 점프 호스트 참조를 검증합니다: 존재하는 SSH/SFTP 프로필이고, 자기 자신으로 돌아오는 순환이 없고,
    /// 단계 수가 상한 이내여야 합니다.
    fn check_jump(&self, self_id: Option<&str>, jump_id: Option<&str>) -> Result<()> {
        let mut next = jump_id.map(str::to_owned);
        let mut hops = 0;
        while let Some(id) = next {
            hops += 1;
            if Some(id.as_str()) == self_id {
                return Err(Error::Invalid(
                    "jump host chain loops back to this profile".into(),
                ));
            }
            if hops > MAX_JUMP_HOPS {
                return Err(Error::Invalid(format!(
                    "at most {MAX_JUMP_HOPS} jump hosts (or the chain loops)"
                )));
            }
            let jump = self
                .get(&id)
                .map_err(|_| Error::Invalid("jump host profile does not exist".into()))?;
            if !matches!(jump.protocol, Protocol::Ssh | Protocol::Sftp) {
                return Err(Error::Invalid("jump host must be an SSH profile".into()));
            }
            next = jump.jump_profile_id;
        }
        Ok(())
    }

    /// 접속 순서대로 정렬한 점프 호스트 목록 (첫 번째 단계가 앞)
    pub fn jump_chain(&self, profile: &Profile) -> Result<Vec<Profile>> {
        let mut chain = Vec::new();
        let mut next = profile.jump_profile_id.clone();
        while let Some(id) = next {
            if chain.len() >= MAX_JUMP_HOPS
                || id == profile.id
                || chain.iter().any(|p: &Profile| p.id == id)
            {
                return Err(Error::Invalid("invalid jump host chain".into()));
            }
            let jump = self.get(&id)?;
            next = jump.jump_profile_id.clone();
            chain.push(jump);
        }
        chain.reverse();
        Ok(chain)
    }

    pub fn create(&self, input: ProfileInput) -> Result<Profile> {
        let input = input.normalized()?;
        self.check_jump(None, input.jump_profile_id.as_deref())?;
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now();
        let tags = serde_json::to_string(&input.tags).map_err(|e| Error::Invalid(e.to_string()))?;
        let rdp = rdp_json(input.rdp)?;
        self.db.with(|c| {
            c.execute(
                &format!("INSERT INTO profiles ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14, ?15)"),
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
                    rdp,
                    input.jump_profile_id,
                ],
            )
        })?;
        self.get(&id)
    }

    pub fn update(&self, id: &str, input: ProfileInput) -> Result<Profile> {
        let input = input.normalized()?;
        self.check_jump(Some(id), input.jump_profile_id.as_deref())?;
        let tags = serde_json::to_string(&input.tags).map_err(|e| Error::Invalid(e.to_string()))?;
        let rdp = rdp_json(input.rdp)?;
        let changed = self.db.with(|c| {
            c.execute(
                "UPDATE profiles SET name = ?2, protocol = ?3, host = ?4, port = ?5, username = ?6, \
                 auth_method = ?7, key_path = ?8, group_name = ?9, tags = ?10, notes = ?11, \
                 favorite = ?12, updated_at = ?13, rdp_settings = ?14, jump_profile_id = ?15 WHERE id = ?1",
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
                    rdp,
                    input.jump_profile_id,
                ],
            )
        })?;
        if changed == 0 {
            return Err(Error::NotFound(format!("profile {id}")));
        }
        self.get(id)
    }

    /// 삭제합니다. 이 프로필을 점프 호스트로 쓰던 프로필은 직접 연결로 바뀝니다.
    pub fn delete(&self, id: &str) -> Result<()> {
        let changed = self.db.with(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "UPDATE profiles SET jump_profile_id = NULL WHERE jump_profile_id = ?1",
                [id],
            )?;
            let changed = tx.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
            tx.commit()?;
            Ok(changed)
        })?;
        if changed == 0 {
            return Err(Error::NotFound(format!("profile {id}")));
        }
        Ok(())
    }
}

fn rdp_json(settings: Option<RdpSettings>) -> Result<Option<String>> {
    settings
        .map(|s| serde_json::to_string(&s))
        .transpose()
        .map_err(|e| Error::Invalid(e.to_string()))
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
        jump_profile_id: row.get(15)?,
        rdp: row
            .get::<_, Option<String>>(14)?
            .and_then(|json| serde_json::from_str(&json).ok()),
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
            rdp: None,
            jump_profile_id: None,
        }
    }

    #[test]
    fn jump_hosts_are_validated_chained_and_cleared_on_delete() {
        let s = store();
        let a = s.create(input("bastion")).unwrap();
        let mut b_in = input("inner");
        b_in.jump_profile_id = Some(a.id.clone());
        let b = s.create(b_in).unwrap();
        let mut t_in = input("target");
        t_in.jump_profile_id = Some(b.id.clone());
        let t = s.create(t_in.clone()).unwrap();
        let chain: Vec<_> = s
            .jump_chain(&t)
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(chain, ["bastion", "inner"]);

        // 순환: bastion이 target을 거치게 하면 거부
        let mut loop_in = input("bastion");
        loop_in.jump_profile_id = Some(t.id.clone());
        assert!(matches!(s.update(&a.id, loop_in), Err(Error::Invalid(_))));
        // 자기 자신, 없는 프로필, SSH가 아닌 프로필은 거부
        let mut self_in = t_in.clone();
        self_in.jump_profile_id = Some(t.id.clone());
        assert!(s.update(&t.id, self_in).is_err());
        let mut missing = input("x");
        missing.jump_profile_id = Some("nope".into());
        assert!(s.create(missing).is_err());
        let mut rdp_in = input("win");
        rdp_in.protocol = Protocol::Rdp;
        let rdp = s.create(rdp_in).unwrap();
        let mut via_rdp = input("y");
        via_rdp.jump_profile_id = Some(rdp.id.clone());
        assert!(s.create(via_rdp).is_err());

        // 중간 단계를 지우면 그 다음 프로필은 직접 연결이 됨
        s.delete(&b.id).unwrap();
        assert_eq!(s.get(&t.id).unwrap().jump_profile_id, None);
        assert!(s.jump_chain(&s.get(&t.id).unwrap()).unwrap().is_empty());
    }

    #[test]
    fn jump_chain_depth_is_limited() {
        let s = store();
        let mut prev: Option<String> = None;
        for i in 0..=MAX_JUMP_HOPS {
            let mut i_in = input(&format!("hop{i}"));
            i_in.jump_profile_id = prev.clone();
            prev = Some(s.create(i_in).unwrap().id);
        }
        let mut too_deep = input("target");
        too_deep.jump_profile_id = prev;
        assert!(s.create(too_deep).is_err());
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
    fn rdp_settings_are_stored_for_rdp_profiles_only() {
        let s = store();
        let settings = RdpSettings {
            width: Some(1920),
            height: Some(1080),
            scale: Some(125),
            hidpi: false,
            keyboard: KeyboardMode::Remote,
            hangul_key: HangulKey::Lang1,
        };
        let mut i = input("win");
        i.protocol = Protocol::Rdp;
        i.port = 3389;
        i.rdp = Some(settings);
        let p = s.create(i.clone()).unwrap();
        assert_eq!(s.get(&p.id).unwrap().rdp, Some(settings));

        // 다른 프로토콜로 바꾸면 RDP 설정은 버림
        i.protocol = Protocol::Ssh;
        assert_eq!(s.update(&p.id, i).unwrap().rdp, None);
    }

    #[test]
    fn rdp_settings_json_defaults_and_rejects_unknown_values() {
        // 이전 버전이 저장한 JSON (키보드 항목 없음)
        let old: RdpSettings =
            serde_json::from_str(r#"{"width":null,"height":null,"scale":null,"hidpi":true}"#)
                .unwrap();
        assert_eq!(old.keyboard, KeyboardMode::Remote);
        assert_eq!(old.hangul_key, HangulKey::Lang1);
        let json = serde_json::to_value(RdpSettings {
            keyboard: KeyboardMode::Local,
            hangul_key: HangulKey::RightAlt,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(json["keyboard"], "local");
        assert_eq!(json["hangulKey"], "rightAlt");
        assert!(serde_json::from_str::<RdpSettings>(r#"{"keyboard":"other"}"#).is_err());
    }

    #[test]
    fn rejects_invalid_rdp_settings() {
        let bad = [
            RdpSettings {
                width: Some(1921),
                height: Some(1080),
                ..Default::default()
            },
            RdpSettings {
                width: Some(100),
                height: Some(100),
                ..Default::default()
            },
            RdpSettings {
                width: Some(1920),
                height: None,
                ..Default::default()
            },
            RdpSettings {
                scale: Some(50),
                ..Default::default()
            },
        ];
        for settings in bad {
            let mut i = input("win");
            i.protocol = Protocol::Rdp;
            i.rdp = Some(settings);
            assert!(i.normalized().is_err(), "{settings:?}");
        }
        let mut i = input("win");
        i.protocol = Protocol::Rdp;
        i.auth_method = AuthMethod::Key;
        i.key_path = Some("~/.ssh/id".into());
        assert!(i.normalized().is_err(), "RDP key auth");
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
