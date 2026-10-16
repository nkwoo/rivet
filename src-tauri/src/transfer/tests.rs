//! 메모리 `RemoteFs`(악성 서버 흉내 포함)로 전송 큐를 검증합니다.

use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};

use async_trait::async_trait;

use super::*;
use crate::protocols::{FsEntry, Listing};
use crate::transfer::paths::remote_parent;

#[derive(Clone)]
enum Node {
    Dir,
    File { data: Vec<u8>, mtime: i64 },
    Symlink,
}

#[derive(Default)]
struct State {
    nodes: BTreeMap<String, Node>,
    /// 실제 파일 없이 목록에만 끼워 넣는 항목 (악성 서버 흉내)
    injected: HashMap<String, Vec<FsEntry>>,
}

#[derive(Default, Clone)]
struct MemFs {
    state: Arc<Mutex<State>>,
    /// 남은 횟수만큼 `open_read`가 연결 끊김으로 실패
    flaky_reads: Arc<AtomicU32>,
    /// 참이면 `open_write`가 권한 오류
    deny_writes: Arc<std::sync::atomic::AtomicBool>,
}

impl MemFs {
    fn dir(&self, path: &str) {
        self.state
            .lock()
            .unwrap()
            .nodes
            .insert(path.into(), Node::Dir);
    }
    fn file(&self, path: &str, data: &[u8], mtime: i64) {
        self.state.lock().unwrap().nodes.insert(
            path.into(),
            Node::File {
                data: data.to_vec(),
                mtime,
            },
        );
    }
    fn symlink(&self, path: &str) {
        self.state
            .lock()
            .unwrap()
            .nodes
            .insert(path.into(), Node::Symlink);
    }
    fn inject(&self, dir: &str, name: &str) {
        self.state
            .lock()
            .unwrap()
            .injected
            .entry(dir.into())
            .or_default()
            .push(FsEntry {
                name: name.into(),
                kind: EntryKind::File,
                size: 4,
                modified: Some(0),
                permissions: Some(0o644),
            });
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        match self.state.lock().unwrap().nodes.get(path) {
            Some(Node::File { data, .. }) => Some(data.clone()),
            _ => None,
        }
    }
    fn entry(name: &str, node: &Node) -> FsEntry {
        let (kind, size, modified) = match node {
            Node::Dir => (EntryKind::Dir, 0, None),
            Node::File { data, mtime } => (EntryKind::File, data.len() as u64, Some(*mtime)),
            Node::Symlink => (EntryKind::Symlink, 0, None),
        };
        FsEntry {
            name: name.into(),
            kind,
            size,
            modified,
            permissions: None,
        }
    }
}

struct MemWriter {
    fs: MemFs,
    path: String,
    buf: Vec<u8>,
}

impl AsyncWrite for MemWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.buf.extend_from_slice(data);
        Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let data = self.buf.clone();
        self.fs.file(&self.path, &data, 2_000_000_000);
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl RemoteFs for MemFs {
    async fn home(&self) -> Result<String, FsError> {
        Ok("/".into())
    }
    async fn list(&self, path: &str) -> Result<Listing, FsError> {
        let state = self.state.lock().unwrap();
        let mut entries: Vec<FsEntry> = state
            .nodes
            .iter()
            .filter(|(p, _)| remote_parent(p).as_deref() == Some(path))
            .map(|(p, n)| MemFs::entry(&last_segment(p), n))
            .collect();
        entries.extend(state.injected.get(path).cloned().unwrap_or_default());
        Ok(Listing {
            path: path.into(),
            parent: remote_parent(path),
            entries,
            truncated: false,
        })
    }
    async fn stat(&self, path: &str) -> Result<Option<FsEntry>, FsError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .nodes
            .get(path)
            .map(|n| MemFs::entry(&last_segment(path), n)))
    }
    async fn mkdir(&self, path: &str) -> Result<(), FsError> {
        if self.state.lock().unwrap().nodes.contains_key(path) {
            return Err(FsError::AlreadyExists);
        }
        self.dir(path);
        Ok(())
    }
    async fn remove_file(&self, path: &str) -> Result<(), FsError> {
        self.state.lock().unwrap().nodes.remove(path);
        Ok(())
    }
    async fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        self.state.lock().unwrap().nodes.remove(path);
        Ok(())
    }
    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        let mut s = self.state.lock().unwrap();
        let node = s.nodes.remove(from).ok_or(FsError::NotFound)?;
        s.nodes.insert(to.into(), node);
        Ok(())
    }
    async fn open_read(&self, path: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>, FsError> {
        if self
            .flaky_reads
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(FsError::ConnectionLost);
        }
        let data = self.read(path).ok_or(FsError::NotFound)?;
        Ok(Box::new(std::io::Cursor::new(data)))
    }
    async fn open_write(&self, path: &str) -> Result<Box<dyn AsyncWrite + Send + Unpin>, FsError> {
        if self.deny_writes.load(Ordering::SeqCst) {
            return Err(FsError::PermissionDenied);
        }
        Ok(Box::new(MemWriter {
            fs: self.clone(),
            path: path.into(),
            buf: Vec::new(),
        }))
    }
}

#[derive(Default)]
struct Collect(Mutex<Vec<TransferEvent>>);
impl TransferSink for Collect {
    fn send(&self, event: TransferEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn setup(rules: NameRules) -> (Arc<TransferManager>, MemFs, Arc<Collect>) {
    let manager = TransferManager::with_rules(rules);
    let sink = Arc::new(Collect::default());
    manager.set_sink(sink.clone());
    (manager, MemFs::default(), sink)
}

/// 모든 작업이 끝나거나(또는 `until` 조건이 참이 될 때까지) 기다립니다.
async fn wait_for(manager: &TransferManager, until: impl Fn(&[JobInfo]) -> bool) -> Vec<JobInfo> {
    for _ in 0..500 {
        let jobs = manager.snapshot();
        if until(&jobs) {
            return jobs;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out: {:#?}", manager.snapshot());
}

async fn wait_all(manager: &TransferManager) -> Vec<JobInfo> {
    wait_for(manager, |jobs| jobs.iter().all(|j| j.state.is_finished())).await
}

fn by_name<'a>(jobs: &'a [JobInfo], name: &str) -> &'a JobInfo {
    jobs.iter()
        .find(|j| j.name == name)
        .unwrap_or_else(|| panic!("no job {name}: {jobs:#?}"))
}

fn code(job: &JobInfo) -> &str {
    match &job.state {
        JobState::Failed { code, .. } => code,
        other => panic!("{} not failed: {other:?}", job.name),
    }
}

#[tokio::test]
async fn malicious_remote_names_are_blocked_and_nothing_escapes() {
    for rules in [NameRules::Unix, NameRules::Windows] {
        let (manager, fs, _) = setup(rules);
        fs.dir("/srv");
        fs.dir("/srv/site");
        fs.file("/srv/site/index.html", b"<h1>", 10);
        fs.dir("/srv/site/assets");
        fs.file("/srv/site/assets/app.js", b"js", 10);
        for evil in [
            "../../.ssh/authorized_keys",
            "..",
            ".",
            "/etc/passwd",
            "..\\..\\evil.bat",
            "C:evil.exe",
            "nul\0byte",
        ] {
            fs.inject("/srv/site", evil);
        }
        fs.inject("/srv/site/assets", "CON");
        fs.inject("/srv/site/assets", "aux.js");

        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("Downloads");
        std::fs::create_dir(&dest).unwrap();
        manager
            .enqueue_download(
                Arc::new(fs.clone()),
                "s1",
                vec!["/srv/site".into()],
                dest.to_str().unwrap(),
                OverwritePolicy::Overwrite,
            )
            .await
            .unwrap();
        let jobs = wait_all(&manager).await;

        assert_eq!(by_name(&jobs, "site/index.html").state, JobState::Done);
        assert_eq!(by_name(&jobs, "site/assets/app.js").state, JobState::Done);
        assert_eq!(
            std::fs::read(dest.join("site/index.html")).unwrap(),
            b"<h1>"
        );
        for evil in [
            "site/../../.ssh/authorized_keys",
            "site/..",
            "site/.",
            "site//etc/passwd",
            "site/..\\..\\evil.bat",
            "site/C:evil.exe",
            "site/nul\0byte",
        ] {
            assert_eq!(
                code(by_name(&jobs, evil)),
                "pathBlocked",
                "{evil} {rules:?}"
            );
        }
        let windows_only = ["site/assets/CON", "site/assets/aux.js"];
        for name in windows_only {
            match rules {
                NameRules::Windows => assert_eq!(code(by_name(&jobs, name)), "pathBlocked"),
                NameRules::Unix => {
                    // 주입한 항목은 실제 파일이 없어 stat에서 NotFound로 실패해야 함 (경로 검사는 통과)
                    assert_eq!(code(by_name(&jobs, name)), "fsNotFound")
                }
            }
        }
        // 대상 폴더 밖에는 아무것도 생기지 않음
        let outside: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(outside, vec![std::ffi::OsString::from("Downloads")]);
        // 차단된 항목은 다시 시도할 수 없음
        let blocked = by_name(&jobs, "site/..");
        assert!(manager.retry(&blocked.id).is_err());
    }
}

#[tokio::test]
async fn top_level_dot_names_are_blocked() {
    let (manager, fs, _) = setup(NameRules::Unix);
    fs.dir("/srv");
    let dest = tempfile::tempdir().unwrap();
    manager
        .enqueue_download(
            Arc::new(fs),
            "s1",
            vec!["/srv/..".into(), "/".into()],
            dest.path().to_str().unwrap(),
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|j| code(j) == "pathBlocked"));
}

#[tokio::test]
async fn upload_and_download_round_trip_with_progress() {
    let (manager, fs, sink) = setup(NameRules::current());
    fs.dir("/home");
    let local = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..(CHUNK * 3 + 17)).map(|i| (i % 251) as u8).collect();
    std::fs::write(local.path().join("big.bin"), &data).unwrap();
    std::fs::create_dir(local.path().join("dir")).unwrap();
    std::fs::write(local.path().join("dir/inner.txt"), b"inner").unwrap();

    let fs_arc: Arc<dyn RemoteFs> = Arc::new(fs.clone());
    manager
        .enqueue_upload(
            fs_arc.clone(),
            "s1",
            vec![
                local.path().join("big.bin").to_string_lossy().into(),
                local.path().join("dir").to_string_lossy().into(),
            ],
            "/home",
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert!(jobs.iter().all(|j| j.state == JobState::Done), "{jobs:#?}");
    assert_eq!(fs.read("/home/big.bin").unwrap(), data);
    assert_eq!(fs.read("/home/dir/inner.txt").unwrap(), b"inner");
    assert_eq!(by_name(&jobs, "big.bin").transferred, data.len() as u64);

    let last_progress = sink.0.lock().unwrap().iter().rev().find_map(|e| match e {
        TransferEvent::Progress { transferred, .. } => Some(*transferred),
        _ => None,
    });
    assert!(last_progress.is_some());

    // 내려받기: 수정 시각 보존, 임시 파일 정리
    let dest = tempfile::tempdir().unwrap();
    fs.file("/home/old.txt", b"old", 1_600_000_000);
    manager.clear_finished();
    manager
        .enqueue_download(
            fs_arc,
            "s1",
            vec!["/home/big.bin".into(), "/home/old.txt".into()],
            dest.path().to_str().unwrap(),
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().all(|j| j.state == JobState::Done), "{jobs:#?}");
    assert_eq!(std::fs::read(dest.path().join("big.bin")).unwrap(), data);
    let meta = std::fs::metadata(dest.path().join("old.txt")).unwrap();
    assert_eq!(local::modified_secs(&meta), Some(1_600_000_000));
    assert!(!dest.path().join("big.bin.rivetpart").exists());
}

#[tokio::test]
async fn overwrite_policies() {
    let local = tempfile::tempdir().unwrap();
    let src = local.path().join("a.txt");
    std::fs::write(&src, b"new").unwrap();
    let src_mtime = local::modified_secs(&std::fs::metadata(&src).unwrap()).unwrap();

    let cases = [
        (OverwritePolicy::Skip, src_mtime - 100, false),
        (OverwritePolicy::Overwrite, src_mtime + 100, true),
        (OverwritePolicy::Newer, src_mtime - 100, true),
        (OverwritePolicy::Newer, src_mtime + 100, false),
    ];
    for (policy, remote_mtime, overwritten) in cases {
        let (manager, fs, _) = setup(NameRules::Unix);
        fs.dir("/d");
        fs.file("/d/a.txt", b"old", remote_mtime);
        manager
            .enqueue_upload(
                Arc::new(fs.clone()),
                "s1",
                vec![src.to_string_lossy().into()],
                "/d",
                policy,
            )
            .await
            .unwrap();
        let jobs = wait_all(&manager).await;
        let expected: &[u8] = if overwritten { b"new" } else { b"old" };
        assert_eq!(fs.read("/d/a.txt").unwrap(), expected, "{policy:?}");
        assert_eq!(
            matches!(jobs[0].state, JobState::Skipped { .. }),
            !overwritten,
            "{policy:?}"
        );
    }
}

#[tokio::test]
async fn ask_policy_waits_for_answer() {
    let (manager, fs, _) = setup(NameRules::Unix);
    fs.dir("/d");
    fs.file("/d/a.txt", b"old", 5);
    fs.file("/d/b.txt", b"old", 5);
    let local = tempfile::tempdir().unwrap();
    for n in ["a.txt", "b.txt"] {
        std::fs::write(local.path().join(n), b"new").unwrap();
    }
    manager
        .enqueue_upload(
            Arc::new(fs.clone()),
            "s1",
            ["a.txt", "b.txt"]
                .iter()
                .map(|n| local.path().join(n).to_string_lossy().into())
                .collect(),
            "/d",
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    let jobs = wait_for(&manager, |jobs| {
        jobs.iter().all(|j| {
            matches!(
                j.state,
                JobState::Conflict {
                    existing_size: 3,
                    ..
                }
            )
        })
    })
    .await;
    manager
        .resolve(&by_name(&jobs, "a.txt").id, ConflictAction::Overwrite)
        .unwrap();
    manager
        .resolve(&by_name(&jobs, "b.txt").id, ConflictAction::Skip)
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert_eq!(by_name(&jobs, "a.txt").state, JobState::Done);
    assert!(matches!(
        by_name(&jobs, "b.txt").state,
        JobState::Skipped { .. }
    ));
    assert_eq!(fs.read("/d/a.txt").unwrap(), b"new");
    assert_eq!(fs.read("/d/b.txt").unwrap(), b"old");
}

#[tokio::test]
async fn transient_errors_retry_automatically() {
    let (manager, fs, _) = setup(NameRules::current());
    fs.file("/f.bin", b"payload", 1);
    fs.flaky_reads.store(1, Ordering::SeqCst);
    let dest = tempfile::tempdir().unwrap();
    manager
        .enqueue_download(
            Arc::new(fs),
            "s1",
            vec!["/f.bin".into()],
            dest.path().to_str().unwrap(),
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert_eq!(jobs[0].state, JobState::Done);
    assert_eq!(jobs[0].attempt, 2);
}

#[tokio::test]
async fn permanent_errors_fail_and_can_be_retried_manually() {
    let (manager, fs, _) = setup(NameRules::Unix);
    fs.dir("/d");
    fs.deny_writes.store(true, Ordering::SeqCst);
    let local = tempfile::tempdir().unwrap();
    let src = local.path().join("a.txt");
    std::fs::write(&src, b"x").unwrap();
    manager
        .enqueue_upload(
            Arc::new(fs.clone()),
            "s1",
            vec![src.to_string_lossy().into()],
            "/d",
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert_eq!(code(&jobs[0]), "permissionDenied");
    assert_eq!(jobs[0].attempt, 1);

    fs.deny_writes.store(false, Ordering::SeqCst);
    manager.retry(&jobs[0].id).unwrap();
    let jobs = wait_for(&manager, |j| j[0].state == JobState::Done).await;
    assert_eq!(jobs[0].state, JobState::Done);
    assert_eq!(fs.read("/d/a.txt").unwrap(), b"x");
}

#[tokio::test]
async fn symlinks_are_not_followed() {
    let (manager, fs, _) = setup(NameRules::current());
    fs.dir("/d");
    fs.symlink("/d/link");
    fs.file("/d/real.txt", b"r", 1);
    let dest = tempfile::tempdir().unwrap();
    manager
        .enqueue_download(
            Arc::new(fs),
            "s1",
            vec!["/d".into()],
            dest.path().to_str().unwrap(),
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert_eq!(
        by_name(&jobs, "d/link").state,
        JobState::Skipped {
            reason: "symlink".into()
        }
    );
    assert_eq!(by_name(&jobs, "d/real.txt").state, JobState::Done);
}

#[cfg(unix)]
#[tokio::test]
async fn local_symlinks_are_not_uploaded() {
    let (manager, fs, _) = setup(NameRules::Unix);
    fs.dir("/d");
    let local = tempfile::tempdir().unwrap();
    std::fs::create_dir(local.path().join("src")).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", local.path().join("src/hosts")).unwrap();
    manager
        .enqueue_upload(
            Arc::new(fs.clone()),
            "s1",
            vec![local.path().join("src").to_string_lossy().into()],
            "/d",
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_all(&manager).await;
    assert!(matches!(
        by_name(&jobs, "src/hosts").state,
        JobState::Skipped { .. }
    ));
    assert!(fs.read("/d/src/hosts").is_none());
}

#[tokio::test]
async fn cancel_session_cancels_pending_conflicts() {
    let (manager, fs, _) = setup(NameRules::Unix);
    fs.dir("/d");
    fs.file("/d/a.txt", b"old", 5);
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("a.txt"), b"new").unwrap();
    manager
        .enqueue_upload(
            Arc::new(fs.clone()),
            "s1",
            vec![local.path().join("a.txt").to_string_lossy().into()],
            "/d",
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    wait_for(&manager, |j| {
        matches!(j[0].state, JobState::Conflict { .. })
    })
    .await;
    manager.cancel_session("other");
    assert!(matches!(
        manager.snapshot()[0].state,
        JobState::Conflict { .. }
    ));
    manager.cancel_session("s1");
    assert_eq!(manager.snapshot()[0].state, JobState::Cancelled);
    manager.clear_finished();
    assert!(manager.snapshot().is_empty());
}

#[test]
fn events_serialize_for_the_webview() {
    let json = serde_json::to_value(TransferEvent::Progress {
        id: "j".into(),
        transferred: 10,
        bytes_per_sec: 5,
    })
    .unwrap();
    assert_eq!(json["type"], "progress");
    assert_eq!(json["bytesPerSec"], 5);
    let state = serde_json::to_value(JobState::Failed {
        code: "pathBlocked".into(),
        message: "m".into(),
        retryable: false,
    })
    .unwrap();
    assert_eq!(state["kind"], "failed");
}
