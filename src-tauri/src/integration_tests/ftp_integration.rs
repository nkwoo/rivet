//! Docker vsftpd 서버 대상 FTP/FTPS 통합 테스트.
//!
//! 실행: `src-tauri/tests/docker/run-ftp-it.sh`
//! (포트 2121: 평문 FTP + AUTH TLS, 990: 암시적 FTPS, 인증서는 자체 서명)

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::SecretString;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::core::cert_pins::CertPinStore;
use crate::core::db::Database;
use crate::core::profile::{AuthMethod, Profile, Protocol};
use crate::core::secrets::{MemoryStore, SecretStore};
use crate::core::session::{prepare_ftp_request, SessionEvent, SessionManager, SessionSink};
use crate::protocols::{EntryKind, FsError, RemoteFs};
use crate::transfer::paths::NameRules;
use crate::transfer::{
    JobInfo, JobState, OverwritePolicy, TransferEvent, TransferManager, TransferSink,
};

const PASSWORD: &str = "rivet-it-password";
const HOME: &str = "/home/rivet";
const WAIT: Duration = Duration::from_secs(20);

fn port() -> u16 {
    std::env::var("RIVET_IT_FTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2121)
}

fn profile(protocol: Protocol) -> Profile {
    Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: "ftp-it".into(),
        protocol,
        host: "127.0.0.1".into(),
        port: port(),
        username: "rivet".into(),
        auth_method: AuthMethod::Password,
        key_path: None,
        group: None,
        tags: vec![],
        notes: String::new(),
        favorite: false,
        created_at: 0,
        updated_at: 0,
    }
}

fn implicit_profile() -> Profile {
    let mut p = profile(Protocol::Ftps);
    p.port = 990;
    p
}

struct Sink(mpsc::UnboundedSender<SessionEvent>);
impl SessionSink for Sink {
    fn output(&self, _: &[u8]) {}
    fn event(&self, event: SessionEvent) {
        let _ = self.0.send(event);
    }
}

struct Harness {
    manager: Arc<SessionManager>,
    pins: Arc<CertPinStore>,
    secrets: Arc<MemoryStore>,
}

impl Harness {
    fn new() -> Self {
        Self {
            manager: SessionManager::new(),
            pins: Arc::new(CertPinStore::new(Arc::new(
                Database::open_in_memory().unwrap(),
            ))),
            secrets: Arc::new(MemoryStore::default()),
        }
    }

    fn connect(&self, p: &Profile, secret: Option<&str>, save: bool) -> Session {
        let secrets: Arc<dyn SecretStore> = self.secrets.clone();
        let request =
            prepare_ftp_request(p, secret.map(SecretString::from), save, secrets).unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self
            .manager
            .spawn_ftp(request, self.pins.clone(), Arc::new(Sink(tx)));
        Session {
            id,
            rx,
            manager: self.manager.clone(),
        }
    }

    /// 접속하고 (필요하면 인증서를 승인한 뒤) 파일 시스템을 돌려줍니다.
    async fn open(&self, p: &Profile) -> (Session, Arc<dyn RemoteFs>, SessionEvent) {
        let mut s = self.connect(p, Some(PASSWORD), false);
        let connected = s.connected_accepting_certs().await;
        let fs = self.manager.remote_fs(&s.id).await.unwrap();
        (s, fs, connected)
    }
}

struct Session {
    id: String,
    rx: mpsc::UnboundedReceiver<SessionEvent>,
    manager: Arc<SessionManager>,
}

impl Session {
    async fn next(&mut self) -> SessionEvent {
        tokio::time::timeout(WAIT, self.rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("session ended")
    }

    /// 인증서 프롬프트가 오면 승인하고 Connected까지 기다립니다.
    async fn connected_accepting_certs(&mut self) -> SessionEvent {
        loop {
            match self.next().await {
                SessionEvent::CertPrompt { .. } => {
                    self.manager.respond_host_key(&self.id, true).unwrap()
                }
                e @ SessionEvent::Connected { .. } => return e,
                SessionEvent::Error { code, message } => {
                    panic!("unexpected error {code}: {message}")
                }
                _ => {}
            }
        }
    }

    /// 에러 코드까지 수집 (Connected가 오면 실패)
    async fn error_code(&mut self) -> String {
        loop {
            match self.next().await {
                SessionEvent::Error { code, .. } => return code,
                SessionEvent::Connected { .. } => panic!("unexpectedly connected"),
                _ => {}
            }
        }
    }
}

fn transport(e: &SessionEvent) -> (&str, &str) {
    match e {
        SessionEvent::Connected { info } => (info.transport.as_str(), info.fingerprint.as_str()),
        other => panic!("not connected: {other:?}"),
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<TransferEvent>>);
impl TransferSink for Events {
    fn send(&self, event: TransferEvent) {
        self.0.lock().unwrap().push(event);
    }
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

async fn put(fs: &Arc<dyn RemoteFs>, path: &str, data: &[u8]) {
    let mut w = fs.open_write(path).await.unwrap();
    w.write_all(data).await.unwrap();
    w.shutdown().await.unwrap();
}

/// 폴더를 비우고 다시 만듭니다 (FTP에는 재귀 삭제가 없어 한 단계만 정리)
async fn fresh_dir(fs: &Arc<dyn RemoteFs>, path: &str) {
    if let Ok(listing) = fs.list(path).await {
        for e in listing.entries {
            let child = format!("{path}/{}", e.name);
            let _ = if e.kind == EntryKind::Dir {
                fs.remove_dir(&child).await
            } else {
                fs.remove_file(&child).await
            };
        }
        let _ = fs.remove_dir(path).await;
    }
    fs.mkdir(path).await.unwrap();
}

/// 업로드 → 목록 → 내려받기 → 이름 바꾸기 → 삭제
async fn round_trip(fs: &Arc<dyn RemoteFs>, session_id: &str, dir: &str) {
    fresh_dir(fs, dir).await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("up.txt"), b"uploaded-by-rivet\r\nline2\n").unwrap();
    let manager = TransferManager::new();
    manager
        .enqueue_upload(
            fs.clone(),
            session_id,
            vec![local.path().join("up.txt").to_string_lossy().into()],
            dir,
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    let jobs = wait_jobs(&manager, WAIT).await;
    assert_eq!(jobs[0].state, JobState::Done, "{jobs:#?}");

    let listing = fs.list(dir).await.unwrap();
    assert_eq!(listing.path, dir);
    let entry = listing.entries.iter().find(|e| e.name == "up.txt").unwrap();
    // 바이너리 모드: CRLF가 그대로 보존되어 크기가 같아야 함
    assert_eq!((entry.kind, entry.size), (EntryKind::File, 25));

    let dest = tempfile::tempdir().unwrap();
    manager.clear_finished();
    manager
        .enqueue_download(
            fs.clone(),
            session_id,
            vec![format!("{dir}/up.txt")],
            dest.path().to_str().unwrap(),
            OverwritePolicy::Ask,
        )
        .await
        .unwrap();
    let jobs = wait_jobs(&manager, WAIT).await;
    assert_eq!(jobs[0].state, JobState::Done, "{jobs:#?}");
    assert_eq!(
        std::fs::read(dest.path().join("up.txt")).unwrap(),
        b"uploaded-by-rivet\r\nline2\n"
    );

    let stat = fs.stat(&format!("{dir}/up.txt")).await.unwrap().unwrap();
    assert_eq!((stat.kind, stat.size), (EntryKind::File, 25));
    assert!(stat.modified.is_some());
    assert!(fs.stat(&format!("{dir}/missing")).await.unwrap().is_none());
    assert_eq!(fs.stat(dir).await.unwrap().unwrap().kind, EntryKind::Dir);

    fs.rename(&format!("{dir}/up.txt"), &format!("{dir}/renamed.txt"))
        .await
        .unwrap();
    assert!(matches!(
        fs.mkdir(&format!("{dir}/renamed.txt")).await,
        Err(FsError::AlreadyExists)
    ));
    fs.remove_file(&format!("{dir}/renamed.txt")).await.unwrap();
    fs.remove_dir(dir).await.unwrap();
    assert!(matches!(
        fs.list("/no/such/dir").await,
        Err(FsError::NotFound)
    ));
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_plain_ftp_round_trip() {
    let h = Harness::new();
    let p = profile(Protocol::Ftp);
    let (s, fs, connected) = h.open(&p).await;
    assert_eq!(transport(&connected), ("plain", ""));
    assert_eq!(fs.home().await.unwrap(), HOME);
    round_trip(&fs, &s.id, &format!("{HOME}/plain")).await;
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_explicit_ftps_pins_certificate_on_approval() {
    let h = Harness::new();
    let p = profile(Protocol::Ftps);
    let mut s = h.connect(&p, Some(PASSWORD), true);
    let fingerprint = match s.next().await {
        SessionEvent::CertPrompt {
            fingerprint,
            problem,
            port,
            ..
        } => {
            assert_eq!(port, p.port);
            // 자체 서명 인증서: Linux 검증기는 unknownIssuer, macOS는 "표준 미준수"(invalid)로 거부
            assert!(
                ["unknownIssuer", "invalid"].contains(&problem.as_str()),
                "{problem}"
            );
            fingerprint
        }
        other => panic!("expected certificate prompt, got {other:?}"),
    };
    h.manager.respond_host_key(&s.id, true).unwrap();
    let connected = s.connected_accepting_certs().await;
    assert_eq!(transport(&connected), ("tlsPinned", fingerprint.as_str()));
    assert_eq!(
        h.pins.pinned("127.0.0.1", p.port).unwrap().as_deref(),
        Some(fingerprint.as_str())
    );
    // 인증에 성공했으므로 입력한 비밀번호가 저장됨
    assert!(h.secrets.get(&p.id).unwrap().is_some());

    // 데이터 연결도 TLS(PROT P) + 세션 재사용으로 동작
    let fs = h.manager.remote_fs(&s.id).await.unwrap();
    round_trip(&fs, &s.id, &format!("{HOME}/explicit")).await;
    h.manager.close(&s.id);

    // 두 번째 접속은 묻지 않고 저장된 비밀번호와 고정 지문으로 연결
    let mut again = h.connect(&p, None, false);
    match again.next().await {
        e @ SessionEvent::Connected { .. } => assert_eq!(transport(&e).0, "tlsPinned"),
        other => panic!("expected direct connection, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_rejected_certificate_aborts() {
    let h = Harness::new();
    let p = profile(Protocol::Ftps);
    let mut s = h.connect(&p, Some(PASSWORD), true);
    assert!(matches!(s.next().await, SessionEvent::CertPrompt { .. }));
    h.manager.respond_host_key(&s.id, false).unwrap();
    assert_eq!(s.error_code().await, "certRejected");
    assert!(h.pins.pinned("127.0.0.1", p.port).unwrap().is_none());
    assert!(h.secrets.get(&p.id).unwrap().is_none());
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_changed_certificate_is_blocked() {
    let h = Harness::new();
    let p = profile(Protocol::Ftps);
    let fake = ["AB"; 32].join(":");
    h.pins.pin("127.0.0.1", p.port, &fake).unwrap();
    let mut s = h.connect(&p, Some(PASSWORD), false);
    match s.next().await {
        SessionEvent::CertChanged {
            expected_fingerprint,
            actual_fingerprint,
            ..
        } => {
            assert_eq!(expected_fingerprint, fake);
            assert_ne!(actual_fingerprint, fake);
        }
        other => panic!("expected certificate change, got {other:?}"),
    }
    assert_eq!(s.error_code().await, "certChanged");
    // 고정 지문은 그대로 유지
    assert_eq!(h.pins.pinned("127.0.0.1", p.port).unwrap(), Some(fake));
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_implicit_ftps() {
    let h = Harness::new();
    let (s, fs, connected) = h.open(&implicit_profile()).await;
    assert_eq!(transport(&connected).0, "tlsPinned");
    round_trip(&fs, &s.id, &format!("{HOME}/implicit")).await;
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_wrong_password_fails_and_is_not_saved() {
    let h = Harness::new();
    let p = profile(Protocol::Ftp);
    let mut s = h.connect(&p, Some("wrong-password"), true);
    assert_eq!(s.error_code().await, "authFailed");
    assert!(h.secrets.get(&p.id).unwrap().is_none());
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_line_breaks_are_rejected_before_sending() {
    let h = Harness::new();
    let (_s, fs, _) = h.open(&profile(Protocol::Ftp)).await;
    for bad in [
        format!("{HOME}\r\nDELE {HOME}/x"),
        format!("{HOME}\nRMD {HOME}"),
        format!("{HOME}\0"),
    ] {
        assert!(
            matches!(fs.list(&bad).await, Err(FsError::Name(_))),
            "{bad:?}"
        );
        assert!(
            matches!(fs.mkdir(&bad).await, Err(FsError::Name(_))),
            "{bad:?}"
        );
        assert!(matches!(fs.open_read(&bad).await, Err(FsError::Name(_))));
        assert!(matches!(fs.open_write(&bad).await, Err(FsError::Name(_))));
    }
    // 프로필의 사용자명/비밀번호에 줄바꿈이 있으면 접속 전에 거부
    let mut p = profile(Protocol::Ftp);
    p.username = "rivet\r\nSITE x".into();
    let secrets: Arc<dyn SecretStore> = h.secrets.clone();
    assert!(prepare_ftp_request(
        &p,
        Some(SecretString::from(PASSWORD)),
        false,
        secrets.clone()
    )
    .is_err());
    let p = profile(Protocol::Ftp);
    assert!(
        prepare_ftp_request(&p, Some(SecretString::from("pw\r\nDELE a")), false, secrets).is_err()
    );
    // 탐색 연결은 계속 사용 가능
    assert_eq!(fs.list(HOME).await.unwrap().path, HOME);
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_large_file_over_ftps_with_progress() {
    let mb: u64 = std::env::var("RIVET_IT_BIG_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    let size = mb * 1024 * 1024;
    let local = tempfile::tempdir().unwrap();
    let src = local.path().join("big.bin");
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&src).unwrap());
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
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
    let (s, fs, _) = h.open(&profile(Protocol::Ftps)).await;
    let manager = TransferManager::new();
    let events = Arc::new(Events::default());
    manager.set_sink(events.clone());
    let limit = Duration::from_secs(60 + size / (4 * 1024 * 1024));

    let started = std::time::Instant::now();
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
    let up = started.elapsed().as_secs_f64();
    assert_eq!(jobs[0].state, JobState::Done, "{jobs:#?}");
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
    assert!(progress.len() >= 2);
    assert!(progress.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(progress.last(), Some(&size));

    let dest = tempfile::tempdir().unwrap();
    manager.clear_finished();
    let started = std::time::Instant::now();
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
    let down = started.elapsed().as_secs_f64();
    assert_eq!(jobs[0].state, JobState::Done, "{jobs:#?}");

    let mut a = std::io::BufReader::new(std::fs::File::open(&src).unwrap());
    let mut b = std::io::BufReader::new(std::fs::File::open(dest.path().join("big.bin")).unwrap());
    let (mut ba, mut bb) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    for i in 0..mb {
        a.read_exact(&mut ba).unwrap();
        b.read_exact(&mut bb).unwrap();
        assert!(ba == bb, "content differs in MiB {i}");
    }
    assert_eq!(b.read(&mut bb).unwrap(), 0);
    fs.remove_file(&format!("{HOME}/big.bin")).await.unwrap();
    eprintln!(
        "FTPS {mb} MiB: upload {:.1} MiB/s, download {:.1} MiB/s",
        mb as f64 / up,
        mb as f64 / down
    );
}

#[tokio::test]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_malicious_names_on_real_server() {
    let h = Harness::new();
    let (s, fs, _) = h.open(&profile(Protocol::Ftp)).await;
    let dir = format!("{HOME}/evil");
    fresh_dir(&fs, &dir).await;
    let names = [
        "ok.txt",
        "..\\..\\evil.bat",
        "sub\\dir.txt",
        "C:evil.exe",
        "a:b",
        "CON",
        "aux.txt",
        "ab:c",
        "trailing.",
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
        for name in ["..\\..\\evil.bat", "sub\\dir.txt", "C:evil.exe", "a:b"] {
            assert_eq!(
                failed_code(job(&jobs, &format!("evil/{name}"))),
                "pathBlocked",
                "{name}"
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

/// 로그 쓰기 대상 (테스트에서 로그 내용을 검사하기 위해 메모리에 모음)
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs Docker vsftpd: tests/docker/run-ftp-it.sh"]
async fn ftp_integration_password_never_appears_in_trace_logs() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Layer;

    // suppaftp는 `log` 크레이트로 보내는 명령을 TRACE로 기록합니다. 필터가 없는 로그(raw)에는
    // 비밀번호가 찍히고, 앱과 같은 필터를 건 로그에는 찍히지 않아야 합니다.
    let _ = tracing_log::LogTracer::init();
    let buffer = LogBuffer::default();
    let raw = LogBuffer::default();
    let (writer, raw_writer) = (buffer.clone(), raw.clone());
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(move || writer.clone())
                .with_filter(tracing_subscriber::filter::LevelFilter::TRACE)
                .with_filter(crate::secret_safe_filter()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(move || raw_writer.clone())
                .with_filter(tracing_subscriber::filter::LevelFilter::TRACE),
        );
    let _guard = tracing::subscriber::set_default(subscriber);

    let h = Harness::new();
    let (_s, fs, _) = h.open(&profile(Protocol::Ftp)).await;
    fs.list(HOME).await.unwrap();

    let logs = String::from_utf8_lossy(&buffer.0.lock().unwrap()).into_owned();
    let raw = String::from_utf8_lossy(&raw.0.lock().unwrap()).into_owned();
    assert!(
        logs.contains("ftp authenticated"),
        "log capture did not work"
    );
    assert!(
        raw.contains(PASSWORD),
        "expected the library to log the password without the filter"
    );
    assert!(!logs.contains(PASSWORD), "password leaked into logs");
}
