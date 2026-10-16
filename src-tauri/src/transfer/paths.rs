//! 원격에서 받은 파일명 검증과 로컬/원격 경로 조립.
//!
//! 원격 서버가 보낸 이름은 신뢰하지 않습니다. 다운로드 대상은 항상 "대상 폴더 + 이름 한 개"로만
//! 만들고, 이름이 폴더 밖을 가리키거나(`..`, 구분자, 절대 경로, 드라이브 문자) Windows에서
//! 특수한 의미를 갖는 경우(예약 이름, 금지 문자, 끝의 점/공백) 거부합니다.
//! 규칙은 실행 중인 OS와 무관하게 테스트할 수 있도록 [`NameRules`]로 분리했습니다.

use std::path::{Component, Path, PathBuf};

/// 파일명 한 개의 최대 길이 (바이트). 대부분의 파일 시스템 한도
pub const MAX_NAME_BYTES: usize = 255;

/// 다운로드 중인 파일에 붙이는 확장자. 완료되면 원래 이름으로 바꿉니다.
pub const PARTIAL_SUFFIX: &str = ".rivetpart";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("empty file name")]
    Empty,
    #[error("file name is too long")]
    TooLong,
    #[error("file name refers to a parent or current folder")]
    DotName,
    #[error("file name contains a path separator")]
    Separator,
    #[error("file name contains control characters")]
    Control,
    #[error("file name is an absolute path or drive")]
    Absolute,
    #[error("file name contains characters Windows does not allow")]
    WindowsChar,
    #[error("file name is reserved on Windows")]
    WindowsReserved,
    #[error("file name ends with a dot or space")]
    WindowsTrailing,
    #[error("target resolves outside the destination folder")]
    Escapes,
    #[error("target is a symbolic link")]
    Symlink,
}

/// 로컬 파일명 규칙. 저장할 OS 기준으로 고릅니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRules {
    Unix,
    Windows,
}

impl NameRules {
    pub const fn current() -> Self {
        if cfg!(windows) {
            NameRules::Windows
        } else {
            NameRules::Unix
        }
    }
}

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// 원격 파일명 한 개를 검증합니다. 이름을 바꾸지 않고, 문제가 있으면 거부합니다.
///
/// 구분자(`/`, `\`)와 드라이브 문자는 어떤 OS에 저장하든 거부합니다. 같은 프로필을
/// macOS와 Windows에서 쓸 때 결과가 달라지지 않게 하기 위해서입니다.
pub fn validate_name(name: &str, rules: NameRules) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(NameError::TooLong);
    }
    if name == "." || name == ".." {
        return Err(NameError::DotName);
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(NameError::Control);
    }
    if name.contains('/') || name.contains('\\') {
        return Err(NameError::Separator);
    }
    if has_drive_prefix(name) {
        return Err(NameError::Absolute);
    }
    if rules == NameRules::Windows {
        validate_windows(name)?;
    }
    Ok(())
}

fn has_drive_prefix(name: &str) -> bool {
    let mut chars = name.chars();
    matches!((chars.next(), chars.next()), (Some(d), Some(':')) if d.is_ascii_alphabetic())
}

fn validate_windows(name: &str) -> Result<(), NameError> {
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(NameError::WindowsChar);
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(NameError::WindowsTrailing);
    }
    // "CON", "con.txt", "NUL .tar.gz" 모두 장치 이름으로 해석됩니다.
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    if WINDOWS_RESERVED
        .iter()
        .any(|r| r.eq_ignore_ascii_case(stem))
    {
        return Err(NameError::WindowsReserved);
    }
    Ok(())
}

/// 로컬 대상 폴더 안에 원격 파일명 한 개를 붙여 다운로드 경로를 만듭니다.
///
/// 이름 검증 후에도 조립 결과가 `dest_dir`의 바로 아래인지 다시 확인하고,
/// 이미 있는 대상이 심볼릭 링크면 따라가지 않도록 거부합니다.
pub fn local_target(dest_dir: &Path, name: &str, rules: NameRules) -> Result<PathBuf, NameError> {
    validate_name(name, rules)?;
    let target = dest_dir.join(name);
    let mut extra = target
        .strip_prefix(dest_dir)
        .map_err(|_| NameError::Escapes)?
        .components();
    if !matches!(
        (extra.next(), extra.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err(NameError::Escapes);
    }
    if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(NameError::Symlink);
    }
    Ok(target)
}

/// 다운로드 중에 쓰는 임시 파일 경로 (`name.rivetpart`)
pub fn partial_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(PARTIAL_SUFFIX);
    target.with_file_name(name)
}

/// 원격 폴더 경로에 이름 한 개를 붙입니다. SFTP 경로는 항상 `/` 구분자를 씁니다.
///
/// 원격에 만들 이름(업로드)도 폴더 밖을 가리키지 않도록 같은 기본 규칙으로 검증합니다.
pub fn remote_join(dir: &str, name: &str) -> Result<String, NameError> {
    validate_name(name, NameRules::Unix)?;
    Ok(if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    })
}

/// 원격 경로의 상위 폴더. 루트(`/`)의 상위는 없습니다.
pub fn remote_parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind('/') {
        Some(0) => Some("/".into()),
        Some(i) => Some(trimmed[..i].into()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use NameRules::{Unix, Windows};

    #[test]
    fn accepts_ordinary_names() {
        for name in [
            "app.js",
            ".bashrc",
            "보고서 2026.pdf",
            "a..b",
            "...hidden",
            "12:30.log",
        ] {
            assert_eq!(validate_name(name, Unix), Ok(()), "{name}");
        }
        for name in [
            "app.js",
            ".bashrc",
            "보고서 2026.pdf",
            "CONSOLE.txt",
            "com10",
            "nul_file",
        ] {
            assert_eq!(validate_name(name, Windows), Ok(()), "{name}");
        }
    }

    #[test]
    fn rejects_traversal_and_separators_on_every_os() {
        let cases = [
            ("", NameError::Empty),
            (".", NameError::DotName),
            ("..", NameError::DotName),
            ("../../.ssh/authorized_keys", NameError::Separator),
            ("..\\..\\Windows\\win.ini", NameError::Separator),
            ("/etc/passwd", NameError::Separator),
            ("\\\\server\\share", NameError::Separator),
            ("sub/file", NameError::Separator),
            ("C:", NameError::Absolute),
            ("C:evil.exe", NameError::Absolute),
            ("z:file", NameError::Absolute),
            ("evil\0.txt", NameError::Control),
            ("line\nbreak", NameError::Control),
            ("bell\u{7}", NameError::Control),
            ("del\u{7f}", NameError::Control),
        ];
        for rules in [Unix, Windows] {
            for (name, err) in &cases {
                assert_eq!(
                    validate_name(name, rules).as_ref(),
                    Err(err),
                    "{name:?} {rules:?}"
                );
            }
        }
        assert_eq!(
            validate_name(&"a".repeat(256), Unix),
            Err(NameError::TooLong)
        );
        assert_eq!(validate_name(&"a".repeat(255), Unix), Ok(()));
    }

    #[test]
    fn rejects_windows_reserved_names_and_characters() {
        for name in [
            "CON",
            "con",
            "Con.txt",
            "NUL",
            "nul.tar.gz",
            "AUX",
            "PRN",
            "COM1",
            "com9.log",
            "LPT1",
            "lpt3.txt",
            "COM¹",
            "CONIN$",
            "CON .txt",
        ] {
            assert_eq!(
                validate_name(name, Windows),
                Err(NameError::WindowsReserved),
                "{name}"
            );
            // macOS/Linux에서는 평범한 이름
            assert_eq!(validate_name(name, Unix), Ok(()), "{name}");
        }
        for name in [
            "a<b",
            "a>b",
            "ab:c",
            "a\"b",
            "a|b",
            "what?",
            "*.txt",
            "file::$DATA",
        ] {
            assert_eq!(
                validate_name(name, Windows),
                Err(NameError::WindowsChar),
                "{name}"
            );
        }
        for name in ["trailing.", "trailing ", "dots..."] {
            assert_eq!(
                validate_name(name, Windows),
                Err(NameError::WindowsTrailing),
                "{name}"
            );
        }
    }

    #[test]
    fn local_target_stays_inside_destination() {
        let dir = tempfile::tempdir().unwrap();
        let target = local_target(dir.path(), "report.pdf", Unix).unwrap();
        assert_eq!(target, dir.path().join("report.pdf"));
        for name in ["../x", "..", "/abs", "C:\\x", "a/b"] {
            assert!(local_target(dir.path(), name, Unix).is_err(), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn local_target_refuses_existing_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("victim"), dir.path().join("link")).unwrap();
        assert_eq!(
            local_target(dir.path(), "link", Unix),
            Err(NameError::Symlink)
        );
    }

    #[test]
    fn partial_path_appends_suffix() {
        assert_eq!(
            partial_path(Path::new("/d/a.tar.gz")),
            PathBuf::from("/d/a.tar.gz.rivetpart")
        );
    }

    #[test]
    fn remote_paths() {
        assert_eq!(remote_join("/var/www", "a.js").unwrap(), "/var/www/a.js");
        assert_eq!(remote_join("/", "etc").unwrap(), "/etc");
        assert!(remote_join("/var", "../etc").is_err());
        assert!(remote_join("/var", "..").is_err());
        assert_eq!(remote_parent("/var/www/").as_deref(), Some("/var"));
        assert_eq!(remote_parent("/var").as_deref(), Some("/"));
        assert_eq!(remote_parent("/"), None);
        assert_eq!(remote_parent("relative"), None);
    }
}
