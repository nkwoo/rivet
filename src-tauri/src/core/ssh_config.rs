//! `~/.ssh/config` 읽기 (연결 프로필 가져오기용).
//!
//! OpenSSH 규칙 중 가져오기에 필요한 부분만 구현합니다.
//! - 키워드는 대소문자 무시, `키 값` 또는 `키=값`, 큰따옴표로 공백 포함 값
//! - 한 호스트에 여러 블록이 맞으면 먼저 나온 값이 이김 (`Host *` 기본값은 보통 마지막)
//! - `Include`는 `~/.ssh` 기준 상대 경로와 파일 이름의 `*`/`?` 와일드카드 지원
//! - `Match` 블록은 조건을 평가할 수 없으므로 건너뜀
//!
//! 와일드카드가 없는 `Host` 별칭마다 가져오기 후보를 하나씩 만듭니다. 로컬 파일만 읽으며
//! 파일 크기와 Include 깊이에 상한을 둡니다.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// 파일 하나의 크기 상한
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Include 중첩 상한 (OpenSSH와 같은 16)
const MAX_INCLUDE_DEPTH: usize = 16;
/// 읽는 파일 수 상한
const MAX_FILES: usize = 256;

/// 가져오기 후보
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConfigHost {
    /// `Host`에 적힌 별칭 (프로필 이름으로 사용)
    pub alias: String,
    /// 실제 접속 주소 (`HostName`, 없으면 별칭)
    pub host: String,
    pub port: u16,
    /// `User`. 없으면 빈 문자열
    pub user: String,
    /// 첫 번째 `IdentityFile` (`~/` 형태 유지)
    pub identity_file: Option<String>,
    /// `ProxyJump` 원문 (점프 호스트 기능에서 사용)
    pub proxy_jump: Option<String>,
}

#[derive(Debug, Default)]
struct Block {
    /// `Host` 패턴들. `None`이면 `Match` 블록(건너뜀)
    patterns: Option<Vec<String>>,
    options: Vec<(String, String)>,
}

/// 줄을 키워드와 인자로 나눕니다. 주석·빈 줄이면 `None`
fn split_line(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, rest) = match line.find(|c: char| c.is_whitespace() || c == '=') {
        Some(i) => (&line[..i], &line[i..]),
        None => (line, ""),
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    let mut args = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        let mut arg = String::new();
        if first == '"' {
            chars.next();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                arg.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                arg.push(c);
                chars.next();
            }
        }
        if arg.starts_with('#') && first != '"' {
            break;
        }
        args.push(arg);
    }
    Some((key.to_ascii_lowercase(), args))
}

/// `*`, `?` 와일드카드 일치
fn wildcard(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => {
                t.first().is_some_and(|x| x.eq_ignore_ascii_case(c)) && go(rest, &t[1..])
            }
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

/// `Host` 패턴 목록이 별칭에 맞는지 (`!` 부정 패턴이 맞으면 제외)
fn host_matches(patterns: &[String], alias: &str) -> bool {
    let mut matched = false;
    for p in patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if wildcard(neg, alias) {
                return false;
            }
        } else if wildcard(p, alias) {
            matched = true;
        }
    }
    matched
}

fn is_pattern(s: &str) -> bool {
    s.contains(['*', '?', '!'])
}

/// 파일을 읽어 블록에 추가합니다. `Include`는 그 자리에 펼칩니다.
struct Reader<'a> {
    ssh_dir: &'a Path,
    blocks: Vec<Block>,
    files: usize,
}

impl Reader<'_> {
    fn read_file(&mut self, path: &Path, depth: usize) {
        if depth > MAX_INCLUDE_DEPTH || self.files >= MAX_FILES {
            return;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        self.files += 1;
        self.read_text(&text, depth);
    }

    fn read_text(&mut self, text: &str, depth: usize) {
        for line in text.lines() {
            let Some((key, args)) = split_line(line) else {
                continue;
            };
            match key.as_str() {
                "host" => self.blocks.push(Block {
                    patterns: Some(args),
                    options: Vec::new(),
                }),
                "match" => self.blocks.push(Block::default()),
                "include" => {
                    for arg in args {
                        for path in self.expand_include(&arg) {
                            self.read_file(&path, depth + 1);
                        }
                    }
                }
                _ => {
                    if let Some(value) = args.into_iter().next() {
                        // 첫 Host 줄 앞의 설정은 모든 호스트에 적용
                        if self.blocks.is_empty() {
                            self.blocks.push(Block {
                                patterns: Some(vec!["*".into()]),
                                options: Vec::new(),
                            });
                        }
                        if let Some(block) = self.blocks.last_mut() {
                            block.options.push((key, value));
                        }
                    }
                }
            }
        }
    }

    /// Include 인자를 실제 파일 목록으로 (상대 경로는 ~/.ssh 기준, 파일 이름 와일드카드)
    fn expand_include(&self, arg: &str) -> Vec<PathBuf> {
        let path = match arg.strip_prefix("~/") {
            Some(rest) => self.ssh_dir.parent().map(|home| home.join(rest)),
            None if Path::new(arg).is_absolute() => Some(PathBuf::from(arg)),
            None => Some(self.ssh_dir.join(arg)),
        };
        let Some(path) = path else { return Vec::new() };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !is_pattern(&name) {
            return vec![path];
        }
        let Some(dir) = path.parent() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| wildcard(&name, &e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        found.sort();
        found
    }
}

/// `%h`(접속 주소), `%r`(원격 사용자), `%d`(홈), `%%`만 펼칩니다. `~/`는 그대로 둡니다.
fn expand_tokens(value: &str, host: &str, user: &str, home: Option<&Path>) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(host),
            Some('r') => out.push_str(user),
            Some('d') => match home {
                Some(h) => out.push_str(&h.to_string_lossy()),
                None => out.push_str("%d"),
            },
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

fn resolve(blocks: &[Block], alias: &str, home: Option<&Path>) -> Option<SshConfigHost> {
    let get = |key: &str| -> Option<String> {
        blocks
            .iter()
            .filter(|b| {
                b.patterns
                    .as_deref()
                    .is_some_and(|p| host_matches(p, alias))
            })
            .flat_map(|b| b.options.iter())
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    let host = get("hostname")
        .map(|h| h.replace("%h", alias))
        .unwrap_or_else(|| alias.to_owned());
    let port = match get("port") {
        Some(p) => p.parse().ok().filter(|p| *p != 0)?,
        None => 22,
    };
    let user = get("user").unwrap_or_default();
    let identity_file = get("identityfile")
        .filter(|f| !f.eq_ignore_ascii_case("none"))
        .map(|f| expand_tokens(&f, &host, &user, home));
    let proxy_jump = get("proxyjump").filter(|j| !j.eq_ignore_ascii_case("none"));
    Some(SshConfigHost {
        alias: alias.to_owned(),
        host,
        port,
        user,
        identity_file,
        proxy_jump,
    })
}

fn hosts_from_blocks(blocks: &[Block], home: Option<&Path>) -> Vec<SshConfigHost> {
    let mut aliases: Vec<&str> = Vec::new();
    for block in blocks {
        for p in block.patterns.iter().flatten() {
            if !is_pattern(p) && !aliases.iter().any(|a| a.eq_ignore_ascii_case(p)) {
                aliases.push(p);
            }
        }
    }
    aliases
        .into_iter()
        .filter_map(|alias| resolve(blocks, alias, home))
        .collect()
}

/// 설정 파일 내용(문자열)에서 후보를 만듭니다. Include는 `ssh_dir` 기준으로 읽습니다.
#[cfg(test)]
fn parse(text: &str, ssh_dir: &Path) -> Vec<SshConfigHost> {
    let mut reader = Reader {
        ssh_dir,
        blocks: Vec::new(),
        files: 0,
    };
    reader.read_text(text, 0);
    hosts_from_blocks(&reader.blocks, ssh_dir.parent())
}

/// `~/.ssh/config`를 읽습니다. 파일이 없으면 빈 목록입니다.
/// `User`가 없는 호스트는 OpenSSH처럼 로컬 사용자명을 씁니다.
pub fn load_user_config() -> Vec<SshConfigHost> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let ssh_dir = home.join(".ssh");
    let mut reader = Reader {
        ssh_dir: &ssh_dir,
        blocks: Vec::new(),
        files: 0,
    };
    reader.read_file(&ssh_dir.join("config"), 0);
    let local_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    with_default_user(hosts_from_blocks(&reader.blocks, Some(&home)), &local_user)
}

fn with_default_user(mut hosts: Vec<SshConfigHost>, local_user: &str) -> Vec<SshConfigHost> {
    for h in &mut hosts {
        if h.user.is_empty() {
            h.user = local_user.to_owned();
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(text: &str) -> Vec<SshConfigHost> {
        parse(text, Path::new("/home/me/.ssh"))
    }

    #[test]
    fn first_value_wins_and_wildcard_blocks_give_defaults() {
        let h = hosts(
            "Host web\n  HostName 10.0.4.11\n  User deploy\n\
             Host db db-replica\n  HostName=db.internal\n  Port 2222\n\
             Host *\n  User fallback\n  Port 2200\n  IdentityFile ~/.ssh/id_ed25519\n",
        );
        assert_eq!(h.len(), 3);
        assert_eq!(
            h[0],
            SshConfigHost {
                alias: "web".into(),
                host: "10.0.4.11".into(),
                port: 2200,
                user: "deploy".into(),
                identity_file: Some("~/.ssh/id_ed25519".into()),
                proxy_jump: None,
            }
        );
        assert_eq!(
            (h[1].host.as_str(), h[1].port, h[1].user.as_str()),
            ("db.internal", 2222, "fallback")
        );
        assert_eq!(h[2].alias, "db-replica");
    }

    #[test]
    fn skips_patterns_negations_and_match_blocks() {
        let h = hosts(
            "Host *.corp !bastion.corp\n  User corp\n\
             Match host foo exec \"true\"\n  User matched\n\
             Host app.corp\n  ProxyJump bastion\n\
             Host bastion.corp\n",
        );
        let names: Vec<_> = h.iter().map(|x| x.alias.as_str()).collect();
        assert_eq!(names, ["app.corp", "bastion.corp"]);
        assert_eq!(h[0].user, "corp");
        assert_eq!(h[0].proxy_jump.as_deref(), Some("bastion"));
        // 부정 패턴에 걸려 기본 사용자 없음, Match 블록 값도 적용 안 함
        assert_eq!(h[1].user, "");
    }

    #[test]
    fn handles_quotes_comments_case_and_tokens() {
        let h = hosts(
            "# comment\nHOST box # trailing\n  hostname %h.example.com\n  user \"me\"\n\
             identityfile \"%d/keys/%r@%h\"\n  ProxyJump none\n",
        );
        assert_eq!(h[0].host, "box.example.com");
        assert_eq!(h[0].user, "me");
        assert_eq!(
            h[0].identity_file.as_deref(),
            Some("/home/me/keys/me@box.example.com")
        );
        assert_eq!(h[0].proxy_jump, None);
    }

    #[test]
    fn invalid_port_drops_host_and_identity_none_is_ignored() {
        let h = hosts("Host bad\n  Port 70000\nHost ok\n  IdentityFile none\n");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].alias, "ok");
        assert_eq!(h[0].identity_file, None);
    }

    #[test]
    fn options_before_first_host_apply_to_all() {
        let h = hosts("User global\nHost a\n");
        assert_eq!(h[0].user, "global");
    }

    #[test]
    fn includes_relative_files_with_wildcards_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("config.d")).unwrap();
        std::fs::write(
            ssh.join("config.d/10-work"),
            "Host work\n  HostName w.example\n",
        )
        .unwrap();
        std::fs::write(ssh.join("config.d/20-home"), "Host home\n").unwrap();
        std::fs::write(ssh.join("config.d/notes.txt"), "Host notes\n").unwrap();
        // 자기 자신을 포함해도 깊이 상한으로 멈춤
        std::fs::write(ssh.join("loop"), "Include loop\nHost looped\n").unwrap();
        let h = parse("Include config.d/??-*\nInclude loop\nHost main\n", &ssh);
        let names: Vec<_> = h.iter().map(|x| x.alias.as_str()).collect();
        assert_eq!(names, ["work", "home", "looped", "main"]);
        assert_eq!(h[0].host, "w.example");
    }

    #[test]
    fn oversized_file_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        let big = format!("Host big\n#{}\n", "x".repeat(MAX_FILE_BYTES as usize));
        std::fs::write(ssh.join("big"), big).unwrap();
        assert!(parse("Include big\n", &ssh).is_empty());
    }

    #[test]
    fn missing_user_falls_back_to_local_user() {
        let h = with_default_user(hosts("Host a\nHost b\n  User admin\n"), "me");
        assert_eq!((h[0].user.as_str(), h[1].user.as_str()), ("me", "admin"));
    }

    #[test]
    fn wildcard_matching() {
        assert!(wildcard("*.corp", "a.corp"));
        assert!(wildcard("web-??", "WEB-01"));
        assert!(!wildcard("web-?", "web-01"));
        assert!(host_matches(&["*".into(), "!x".into()], "y"));
        assert!(!host_matches(&["*".into(), "!x".into()], "x"));
    }
}
