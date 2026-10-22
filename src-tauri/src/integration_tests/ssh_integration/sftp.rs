//! SFTP 통합 테스트: 셸과 연결 공유, SFTP 전용 프로필, 대용량 전송, 실제 서버의 악성 파일명.
//!
//! 대용량 파일 크기는 `RIVET_IT_BIG_MB`(기본 64)로 바꿉니다. 2단계 완료 기준 확인:
//! `RIVET_IT_BIG_MB=1024 src-tauri/tests/docker/run-ssh-it.sh sftp_large`

use std::io::Read;
use std::time::Instant;

use tokio::io::AsyncWriteExt;

use super::*;
use crate::protocols::{EntryKind, RemoteFs};
use crate::transfer::paths::NameRules;
use crate::transfer::{
    JobInfo, JobState, OverwritePolicy, TransferEvent, TransferManager, TransferSink,
};

const HOME: &str = "/home/rivet";

#[derive(Default)]
struct Events(Mutex<Vec<TransferEvent>>);
impl TransferSink for Events {
    fn send(&self, event: TransferEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// 접속 후 SFTP를 엽니다.
async fn open(h: &Harness, p: &Profile) -> (Session, Arc<dyn RemoteFs>) {
    let (id, mut s) = h.connect(p, Some(PASSWORD), false);
    s.accept_host_key().await;
    s.connected().await;
    let fs = h.manager.remote_fs(&id).await.unwrap();
    (s, fs)
}

async fn wait_jobs(manager: &TransferManager, limit: Duration) -> Vec<JobInfo> {
    tokio::time::timeout(limit, async {
        loop {
            let jobs = manager.snapshot();
            if !jobs.is_empty() && jobs.iter().all(|j| j.state.is_finished()) {
                return jobs;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("transfers did not finish: {:#?}", manager.snapshot()))
}

fn job<'a>(jobs: &'a [JobInfo], name: &str) -> &'a JobInfo {
    jobs.iter()
        .find(|j| j.name == name)
        .unwrap_or_else(|| panic!("no job {name}: {jobs:#?}"))
}

fn failed_code(j: &JobInfo) -> &str {
    match &j.state {
        JobState::Failed { code, .. } => code,
        other => panic!("{} not failed: {other:?}", j.name),
    }
}

/// 원격에 파일을 바로 씁니다. (이름 검증을 거치지 않으므로 악성 이름도 만들 수 있음)
async fn put(fs: &Arc<dyn RemoteFs>, path: &str, data: &[u8]) {
    let mut w = fs.open_write(path).await.unwrap();
    w.write_all(data).await.unwrap();
    w.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_sftp_shares_shell_connection() {
    let h = Harness::new();
    let (mut s, fs) = open(&h, &profile(AuthMethod::Password)).await;
    assert_eq!(fs.home().await.unwrap(), HOME);

    // 셸에서 만든 파일이 같은 연결의 SFTP로 보임
    s.type_line("rm -rf ~/it && mkdir ~/it && printf shell > ~/it/from-shell.txt && echo MA''RK")
        .await;
    s.output_contains("MARK").await;
    let listing = fs.list(&format!("{HOME}/it")).await.unwrap();
    assert_eq!(listing.path, format!("{HOME}/it"));
    assert_eq!(listing.parent.as_deref(), Some(HOME));
    let entry = listing
        .entries
        .iter()
        .find(|e| e.name == "from-shell.txt")
        .unwrap();
    assert_eq!((entry.kind, entry.size), (EntryKind::File, 5));

    // 업로드 → 셸에서 확인, 내려받기 → 로컬에서 확인
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("up.txt"), b"uploaded-by-rivet").unwrap();
    let manager = TransferManager::new();
    manager
        .enqueue_upload(
            fs.clone(),
            &s.id,
            vec![local.path().join("up.txt").to_string_lossy().into()],
            &format!("{HOME}/it"),
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    manager
        .enqueue_download(
            fs.clone(),
            &s.id,
            vec![format!("{HOME}/it/from-shell.txt")],
            local.path().to_str().unwrap(),
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    let jobs = wait_jobs(&manager, WAIT).await;
    assert!(jobs.iter().all(|j| j.state == JobState::Done), "{jobs:#?}");
    assert_eq!(
        std::fs::read(local.path().join("from-shell.txt")).unwrap(),
        b"shell"
    );
    s.type_line("cat ~/it/up.txt; echo").await;
    s.output_contains("uploaded-by-rivet").await;

    // 폴더 작업
    fs.mkdir(&format!("{HOME}/it/new")).await.unwrap();
    assert!(matches!(
        fs.mkdir(&format!("{HOME}/it/new")).await,
        Err(crate::protocols::FsError::AlreadyExists)
    ));
    fs.rename(&format!("{HOME}/it/new"), &format!("{HOME}/it/renamed"))
        .await
        .unwrap();
    fs.remove_dir(&format!("{HOME}/it/renamed")).await.unwrap();
    fs.remove_file(&format!("{HOME}/it/up.txt")).await.unwrap();
    assert!(fs
        .stat(&format!("{HOME}/it/up.txt"))
        .await
        .unwrap()
        .is_none());
    assert!(matches!(
        fs.list(&format!("{HOME}/it/from-shell.txt")).await,
        Err(crate::protocols::FsError::NotADirectory)
    ));
    assert!(matches!(
        fs.list("/no/such/dir").await,
        Err(crate::protocols::FsError::NotFound)
    ));
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_sftp_only_profile_without_shell() {
    let h = Harness::new();
    let mut p = profile(AuthMethod::Password);
    p.protocol = Protocol::Sftp;
    let (mut s, fs) = open(&h, &p).await;
    let listing = fs.list(HOME).await.unwrap();
    assert_eq!(listing.path, HOME);
    // 셸이 없으므로 출력이 없어야 함
    assert!(s.output.is_empty());

    h.manager.close(&s.id);
    tokio::time::timeout(WAIT, async { while s.rx.recv().await.is_some() {} })
        .await
        .unwrap();
    assert!(h.manager.remote_fs(&s.id).await.is_err());
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_sftp_large_file_with_progress() {
    let mb: u64 = std::env::var("RIVET_IT_BIG_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    let size = mb * 1024 * 1024;
    let local = tempfile::tempdir().unwrap();
    let src = local.path().join("big.bin");
    {
        // 압축이 안 되는 의사 난수 패턴
        let mut f = std::io::BufWriter::new(std::fs::File::create(&src).unwrap());
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut block = vec![0u8; 1024 * 1024];
        for _ in 0..mb {
            for chunk in block.chunks_mut(8) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                chunk.copy_from_slice(&x.to_le_bytes());
            }
            f.write_all(&block).unwrap();
        }
    }

    let h = Harness::new();
    let (s, fs) = open(&h, &profile(AuthMethod::Password)).await;
    let manager = TransferManager::new();
    let events = Arc::new(Events::default());
    manager.set_sink(events.clone());
    // 초당 4MB 이상을 가정한 넉넉한 제한 시간
    let limit = Duration::from_secs(60 + size / (4 * 1024 * 1024));

    let started = Instant::now();
    manager
        .enqueue_upload(
            fs.clone(),
            &s.id,
            vec![src.to_string_lossy().into()],
            HOME,
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_jobs(&manager, limit).await;
    let up_secs = started.elapsed().as_secs_f64();
    assert_eq!(jobs[0].state, JobState::Done, "{jobs:#?}");
    assert_eq!(jobs[0].transferred, size);
    assert_eq!(
        fs.stat(&format!("{HOME}/big.bin"))
            .await
            .unwrap()
            .unwrap()
            .size,
        size
    );

    let progress: Vec<u64> = events
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            TransferEvent::Progress { transferred, .. } => Some(*transferred),
            _ => None,
        })
        .collect();
    // 중간 진행률은 PROGRESS_INTERVAL마다만 보내므로, 전송이 그보다 넉넉히 오래 걸렸을 때만 요구.
    // 빠른 러너에서는 끝날 때 한 번만 올 수 있음
    if up_secs > 4.0 * crate::transfer::PROGRESS_INTERVAL.as_secs_f64() {
        assert!(progress.len() >= 2, "expected progress updates");
    }
    assert!(!progress.is_empty());
    assert!(progress.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(progress.last(), Some(&size));

    let dest = tempfile::tempdir().unwrap();
    manager.clear_finished();
    let started = Instant::now();
    manager
        .enqueue_download(
            fs.clone(),
            &s.id,
            vec![format!("{HOME}/big.bin")],
            dest.path().to_str().unwrap(),
            OverwritePolicy::Overwrite,
        )
        .await
        .unwrap();
    let jobs = wait_jobs(&manager, limit).await;
    let down_secs = started.elapsed().as_secs_f64();
    assert_eq!(jobs[0].state, JobState::Done, "{jobs:#?}");

    // 원본과 내려받은 파일을 1MiB씩 비교
    let mut a = std::io::BufReader::new(std::fs::File::open(&src).unwrap());
    let mut b = std::io::BufReader::new(std::fs::File::open(dest.path().join("big.bin")).unwrap());
    let (mut ba, mut bb) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    for i in 0..mb {
        a.read_exact(&mut ba).unwrap();
        b.read_exact(&mut bb).unwrap();
        assert!(ba == bb, "content differs in MiB {i}");
    }
    assert_eq!(b.read(&mut bb).unwrap(), 0, "downloaded file is longer");
    fs.remove_file(&format!("{HOME}/big.bin")).await.unwrap();
    eprintln!(
        "{mb} MiB: upload {:.1} MiB/s, download {:.1} MiB/s",
        mb as f64 / up_secs,
        mb as f64 / down_secs
    );
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_sftp_malicious_names_on_real_server() {
    let h = Harness::new();
    let (mut s, fs) = open(&h, &profile(AuthMethod::Password)).await;
    s.type_line("rm -rf ~/evil && mkdir ~/evil && ln -s /etc/passwd ~/evil/link && echo MA''RK")
        .await;
    s.output_contains("MARK").await;
    let dir = format!("{HOME}/evil");
    // 리눅스에서는 정상 이름이지만 내려받는 쪽에서는 위험한 이름들
    let names = [
        "ok.txt",
        "..\\..\\evil.bat",
        "sub\\dir.txt",
        "C:evil.exe",
        "CON",
        "aux.txt",
        "a:b",
        "ab:c",
        "trailing.",
        "bell\u{7}.txt",
    ];
    for name in names {
        put(&fs, &format!("{dir}/{name}"), b"data").await;
    }

    for rules in [NameRules::Unix, NameRules::Windows] {
        let manager = TransferManager::with_rules(rules);
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("dest");
        std::fs::create_dir(&dest).unwrap();
        manager
            .enqueue_download(
                fs.clone(),
                &s.id,
                vec![dir.clone()],
                dest.to_str().unwrap(),
                OverwritePolicy::Overwrite,
            )
            .await
            .unwrap();
        let jobs = wait_jobs(&manager, WAIT).await;

        assert_eq!(job(&jobs, "evil/ok.txt").state, JobState::Done);
        assert_eq!(
            job(&jobs, "evil/link").state,
            JobState::Skipped {
                reason: "symlink".into()
            }
        );
        for name in [
            "..\\..\\evil.bat",
            "sub\\dir.txt",
            "C:evil.exe",
            "a:b",
            "bell\u{7}.txt",
        ] {
            assert_eq!(
                failed_code(job(&jobs, &format!("evil/{name}"))),
                "pathBlocked"
            );
        }
        for name in ["CON", "aux.txt", "ab:c", "trailing."] {
            let j = job(&jobs, &format!("evil/{name}"));
            match rules {
                NameRules::Windows => assert_eq!(failed_code(j), "pathBlocked", "{name}"),
                NameRules::Unix => assert_eq!(j.state, JobState::Done, "{name}"),
            }
        }
        let outside: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(outside, vec![std::ffi::OsString::from("dest")]);
    }
}
