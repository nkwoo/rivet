//! 로컬 파일 탐색기용 목록 조회. 웹뷰에는 파일 시스템 플러그인을 주지 않고 이 모듈만 노출합니다.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::protocols::{EntryKind, FsEntry, FsError, Listing, MAX_LIST_ENTRIES};

pub fn home() -> Result<String, FsError> {
    std::env::home_dir()
        .and_then(|p| p.to_str().map(str::to_owned))
        .ok_or(FsError::NotFound)
}

/// Windows `canonicalize`가 붙이는 `\\?\` 접두사를 일반 드라이브 경로에서 제거합니다.
fn display_path(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    let stripped = s.strip_prefix(r"\\?\").filter(|rest| {
        let b = rest.as_bytes();
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    });
    Some(stripped.unwrap_or(s).to_owned())
}

pub fn entry_from_metadata(name: String, meta: &std::fs::Metadata) -> FsEntry {
    let ft = meta.file_type();
    let kind = if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_dir() {
        EntryKind::Dir
    } else if ft.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let permissions = None;
    FsEntry {
        name,
        kind,
        size: if kind == EntryKind::File {
            meta.len()
        } else {
            0
        },
        modified: modified_secs(meta),
        permissions,
    }
}

pub fn modified_secs(meta: &std::fs::Metadata) -> Option<i64> {
    let t = meta.modified().ok()?;
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

/// 폴더 목록. 심볼릭 링크는 링크 자체의 정보로 보여주고, UTF-8이 아닌 이름은 건너뜁니다.
pub fn list(path: &str) -> Result<Listing, FsError> {
    let dir: PathBuf = std::fs::canonicalize(path).map_err(map_io)?;
    if !dir.is_dir() {
        return Err(FsError::NotADirectory);
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in std::fs::read_dir(&dir).map_err(map_io)? {
        let Ok(item) = item else { continue };
        let Ok(name) = item.file_name().into_string() else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(item.path()) else {
            continue;
        };
        if entries.len() >= MAX_LIST_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(entry_from_metadata(name, &meta));
    }
    Ok(Listing {
        path: display_path(&dir).ok_or(FsError::NotFound)?,
        parent: dir.parent().and_then(display_path),
        entries,
        truncated,
    })
}

pub fn map_io(e: std::io::Error) -> FsError {
    match e.kind() {
        std::io::ErrorKind::NotFound => FsError::NotFound,
        std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => FsError::AlreadyExists,
        _ => FsError::Io(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folder_without_following_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", dir.path().join("link")).unwrap();

        let listing = list(dir.path().to_str().unwrap()).unwrap();
        let find = |n: &str| listing.entries.iter().find(|e| e.name == n).unwrap();
        assert_eq!(find("a.txt").kind, EntryKind::File);
        assert_eq!(find("a.txt").size, 5);
        assert_eq!(find("sub").kind, EntryKind::Dir);
        #[cfg(unix)]
        assert_eq!(find("link").kind, EntryKind::Symlink);
        assert!(listing.parent.is_some());
        assert!(!listing.truncated);
    }

    #[test]
    fn rejects_files_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        std::fs::write(&file, b"").unwrap();
        assert!(matches!(
            list(file.to_str().unwrap()),
            Err(FsError::NotADirectory)
        ));
        assert!(matches!(
            list(dir.path().join("missing").to_str().unwrap()),
            Err(FsError::NotFound)
        ));
    }

    #[test]
    fn strips_verbatim_prefix_only_for_drive_paths() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users")).as_deref(),
            Some(r"C:\Users")
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share")).as_deref(),
            Some(r"\\?\UNC\server\share")
        );
        assert_eq!(
            display_path(Path::new("/home/a")).as_deref(),
            Some("/home/a")
        );
    }
}
