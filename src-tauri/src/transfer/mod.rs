//! 전송 큐: 동시 전송 수 제한, 진행률, 재시도, 덮어쓰기 정책.
//!
//! - 작업 하나는 파일 하나입니다. 폴더는 큐에 넣을 때 펼치며, 깊이와 항목 수에 상한을 둡니다.
//! - 원격 이름은 [`paths`]로 검증하고, 대상 폴더 밖을 가리키면 그 항목만 "경로 검사 차단"으로 남깁니다.
//! - 심볼릭 링크는 따라가지 않고 건너뜁니다.
//! - 다운로드는 `이름.rivetpart`에 쓴 뒤 완료되면 원래 이름으로 바꿉니다.
//! - 동시 전송 수는 세마포어로 제한하며, 덮어쓰기 응답을 기다리는 동안에는 자리를 비워 둡니다.

pub mod local;
pub mod paths;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{oneshot, Semaphore};
use tokio::task::AbortHandle;

use crate::error::{Error, Result};
use crate::protocols::{EntryKind, FsError, RemoteFs};
use paths::{local_target, partial_path, remote_join, NameRules};

/// 동시에 진행하는 전송 수
pub const CONCURRENCY: usize = 3;
/// 일시적 오류(연결 끊김, 시간 초과)에서 자동으로 다시 시도하는 횟수를 포함한 총 시도 수
const MAX_ATTEMPTS: u32 = 3;
const RETRY_BACKOFF: Duration = Duration::from_secs(1);
/// 폴더를 펼칠 때 깊이와 항목 수 상한
const MAX_TREE_DEPTH: usize = 32;
const MAX_TREE_ITEMS: usize = 10_000;
const CHUNK: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

pub type JobId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Upload,
    Download,
}

/// 대상에 같은 이름이 있을 때의 처리
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverwritePolicy {
    Ask,
    Overwrite,
    Skip,
    /// 원본이 더 최근에 수정된 경우에만 덮어쓰기
    Newer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictAction {
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum JobState {
    Queued,
    Running,
    /// 같은 이름이 있어 사용자 응답을 기다림
    Conflict {
        existing_size: u64,
        existing_modified: Option<i64>,
    },
    Done,
    /// `exists`, `notNewer`, `symlink`
    Skipped {
        reason: String,
    },
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
    Cancelled,
}

impl JobState {
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            JobState::Done
                | JobState::Skipped { .. }
                | JobState::Failed { .. }
                | JobState::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: JobId,
    pub session_id: String,
    pub direction: Direction,
    /// 표시용 이름 (폴더 안 항목은 상대 경로)
    pub name: String,
    pub source: String,
    pub target: String,
    pub size: u64,
    pub transferred: u64,
    pub attempt: u32,
    pub state: JobState,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TransferEvent {
    /// 작업 추가 또는 상태 변경 (전체 정보)
    Job {
        job: JobInfo,
    },
    Progress {
        id: JobId,
        transferred: u64,
        bytes_per_sec: u64,
    },
    Removed {
        ids: Vec<JobId>,
    },
}

pub trait TransferSink: Send + Sync + 'static {
    fn send(&self, event: TransferEvent);
}

#[derive(Clone)]
struct Plan {
    fs: Arc<dyn RemoteFs>,
    direction: Direction,
    source: String,
    target: String,
    policy: OverwritePolicy,
    source_modified: Option<i64>,
}

struct Record {
    info: JobInfo,
    /// 끝난 상태로 추가된 항목(경로 차단 등)은 계획이 없습니다.
    plan: Option<Plan>,
    abort: Option<AbortHandle>,
    reply: Option<oneshot::Sender<ConflictAction>>,
}

/// 큐에 넣기 전 펼친 항목
enum Item {
    Transfer {
        name: String,
        size: u64,
        plan: Plan,
    },
    Finished {
        name: String,
        source: String,
        target: String,
        state: JobState,
    },
}

impl Item {
    fn blocked(name: String, source: String, target: String, e: FsError) -> Self {
        Item::Finished {
            name,
            source,
            target,
            state: failed_state(&e),
        }
    }

    fn skipped(name: String, source: String, target: String, reason: &str) -> Self {
        Item::Finished {
            name,
            source,
            target,
            state: JobState::Skipped {
                reason: reason.into(),
            },
        }
    }
}

fn failed_state(e: &FsError) -> JobState {
    JobState::Failed {
        code: e.code().into(),
        message: e.to_string(),
        retryable: !matches!(e, FsError::Name(_) | FsError::Symlink),
    }
}

/// russh-sftp 파일 핸들은 프로토콜 오류를 `io::Error`로 감싸 돌려주므로 원래 오류를 꺼냅니다.
fn io_to_fs(e: std::io::Error) -> FsError {
    let inner = e
        .get_ref()
        .and_then(|i| i.downcast_ref::<russh_sftp::client::error::Error>())
        .cloned();
    match inner {
        Some(sftp) => sftp.into(),
        None => local::map_io(e),
    }
}

fn last_segment(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_owned()
}

enum Decision {
    Proceed,
    Skip(&'static str),
    Ask { size: u64, modified: Option<i64> },
}

fn decide(
    policy: OverwritePolicy,
    source_modified: Option<i64>,
    existing: Option<(EntryKind, u64, Option<i64>)>,
) -> Result<Decision, FsError> {
    let Some((kind, size, modified)) = existing else {
        return Ok(Decision::Proceed);
    };
    match kind {
        EntryKind::File => {}
        EntryKind::Symlink => return Err(FsError::Symlink),
        _ => return Err(FsError::AlreadyExists),
    }
    Ok(match policy {
        OverwritePolicy::Overwrite => Decision::Proceed,
        OverwritePolicy::Skip => Decision::Skip("exists"),
        OverwritePolicy::Newer => match (source_modified, modified) {
            (Some(src), Some(dst)) if src > dst => Decision::Proceed,
            _ => Decision::Skip("notNewer"),
        },
        OverwritePolicy::Ask => Decision::Ask { size, modified },
    })
}

pub struct TransferManager {
    jobs: Mutex<HashMap<JobId, Record>>,
    order: Mutex<Vec<JobId>>,
    permits: Arc<Semaphore>,
    sink: RwLock<Option<Arc<dyn TransferSink>>>,
    rules: NameRules,
}

impl TransferManager {
    pub fn new() -> Arc<Self> {
        Self::with_rules(NameRules::current())
    }

    pub fn with_rules(rules: NameRules) -> Arc<Self> {
        Arc::new(Self {
            jobs: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(CONCURRENCY)),
            sink: RwLock::new(None),
            rules,
        })
    }

    fn jobs(&self) -> std::sync::MutexGuard<'_, HashMap<JobId, Record>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn order(&self) -> std::sync::MutexGuard<'_, Vec<JobId>> {
        self.order.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 진행 상황을 받을 곳을 바꿉니다. (웹뷰 새로고침 시 다시 등록)
    pub fn set_sink(&self, sink: Arc<dyn TransferSink>) {
        *self.sink.write().unwrap_or_else(PoisonError::into_inner) = Some(sink);
    }

    fn emit(&self, event: TransferEvent) {
        let sink = self
            .sink
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(sink) = sink {
            sink.send(event);
        }
    }

    pub fn snapshot(&self) -> Vec<JobInfo> {
        let jobs = self.jobs();
        self.order()
            .iter()
            .filter_map(|id| jobs.get(id).map(|r| r.info.clone()))
            .collect()
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut JobInfo)) {
        let info = self.jobs().get_mut(id).map(|r| {
            f(&mut r.info);
            r.info.clone()
        });
        if let Some(job) = info {
            self.emit(TransferEvent::Job { job });
        }
    }

    fn set_state(&self, id: &str, state: JobState) {
        self.update(id, |info| info.state = state);
    }

    fn progress(&self, id: &str, transferred: u64, bytes_per_sec: u64) {
        if let Some(r) = self.jobs().get_mut(id) {
            r.info.transferred = transferred;
        }
        self.emit(TransferEvent::Progress {
            id: id.into(),
            transferred,
            bytes_per_sec,
        });
    }

    fn add(
        self: &Arc<Self>,
        session_id: &str,
        direction: Direction,
        items: Vec<Item>,
    ) -> Vec<JobInfo> {
        let mut added = Vec::with_capacity(items.len());
        let mut start = Vec::new();
        {
            let mut jobs = self.jobs();
            let mut order = self.order();
            for item in items {
                let id = uuid::Uuid::new_v4().to_string();
                let (info, plan) = match item {
                    Item::Transfer { name, size, plan } => (
                        JobInfo {
                            id: id.clone(),
                            session_id: session_id.into(),
                            direction,
                            name,
                            source: plan.source.clone(),
                            target: plan.target.clone(),
                            size,
                            transferred: 0,
                            attempt: 0,
                            state: JobState::Queued,
                        },
                        Some(plan),
                    ),
                    Item::Finished {
                        name,
                        source,
                        target,
                        state,
                    } => (
                        JobInfo {
                            id: id.clone(),
                            session_id: session_id.into(),
                            direction,
                            name,
                            source,
                            target,
                            size: 0,
                            transferred: 0,
                            attempt: 0,
                            state,
                        },
                        None,
                    ),
                };
                if plan.is_some() {
                    start.push(id.clone());
                }
                added.push(info.clone());
                order.push(id.clone());
                jobs.insert(
                    id,
                    Record {
                        info,
                        plan,
                        abort: None,
                        reply: None,
                    },
                );
            }
        }
        for job in &added {
            self.emit(TransferEvent::Job { job: job.clone() });
        }
        for id in start {
            self.start(id);
        }
        added
    }

    fn start(self: &Arc<Self>, id: JobId) {
        let me = self.clone();
        let job = id.clone();
        let task = tokio::spawn(async move { me.run(job).await });
        if let Some(r) = self.jobs().get_mut(&id) {
            r.abort = Some(task.abort_handle());
        }
    }

    /// 로컬 파일/폴더를 원격 폴더로 올립니다.
    pub async fn enqueue_upload(
        self: &Arc<Self>,
        fs: Arc<dyn RemoteFs>,
        session_id: &str,
        sources: Vec<String>,
        target_dir: &str,
        policy: OverwritePolicy,
    ) -> Result<Vec<JobInfo>> {
        let paths: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
        let walked = tokio::task::spawn_blocking(move || walk_local(&paths))
            .await
            .map_err(|e| Error::Invalid(e.to_string()))??;

        let mut items = Vec::with_capacity(walked.len());
        // 상대 경로 → 원격 폴더 경로 (부모 폴더 생성에 실패하면 그 아래는 모두 실패 처리)
        let mut remote_dirs: HashMap<String, Result<String, String>> = HashMap::new();
        for w in walked {
            let display = w.rel.join("/");
            let source = w.path.to_string_lossy().into_owned();
            let parent_rel = w.rel[..w.rel.len() - 1].join("/");
            let parent = if parent_rel.is_empty() {
                Ok(target_dir.to_owned())
            } else {
                remote_dirs
                    .get(&parent_rel)
                    .cloned()
                    .unwrap_or_else(|| Err("parent folder missing".into()))
            };
            let name = w.rel[w.rel.len() - 1].clone();
            let parent = match parent {
                Ok(p) => p,
                Err(msg) => {
                    let e = FsError::Remote(format!("parent folder failed: {msg}"));
                    items.push(Item::blocked(display, source, String::new(), e));
                    continue;
                }
            };
            let target = match remote_join(&parent, &name) {
                Ok(t) => t,
                Err(e) => {
                    items.push(Item::blocked(display, source, parent, e.into()));
                    continue;
                }
            };
            match w.kind {
                EntryKind::Dir => {
                    let made = match fs.mkdir(&target).await {
                        Ok(()) | Err(FsError::AlreadyExists) => Ok(target.clone()),
                        Err(e) => Err(e.to_string()),
                    };
                    if let Err(msg) = &made {
                        let e = FsError::Remote(msg.clone());
                        items.push(Item::blocked(display.clone(), source, target, e));
                    }
                    remote_dirs.insert(display, made);
                }
                EntryKind::File => items.push(Item::Transfer {
                    name: display,
                    size: w.size,
                    plan: Plan {
                        fs: fs.clone(),
                        direction: Direction::Upload,
                        source,
                        target,
                        policy,
                        source_modified: w.modified,
                    },
                }),
                EntryKind::Symlink | EntryKind::Other => {
                    items.push(Item::skipped(display, source, target, "symlink"))
                }
            }
        }
        Ok(self.add(session_id, Direction::Upload, items))
    }

    /// 원격 파일/폴더를 로컬 폴더로 내려받습니다.
    pub async fn enqueue_download(
        self: &Arc<Self>,
        fs: Arc<dyn RemoteFs>,
        session_id: &str,
        sources: Vec<String>,
        dest_dir: &str,
        policy: OverwritePolicy,
    ) -> Result<Vec<JobInfo>> {
        let dest = PathBuf::from(dest_dir);
        let is_dir = tokio::fs::symlink_metadata(&dest)
            .await
            .map(|m| m.is_dir())
            .map_err(local::map_io)?;
        if !is_dir {
            return Err(FsError::NotADirectory.into());
        }

        let mut items = Vec::new();
        // (원격 경로, 표시 이름, 로컬 부모 폴더, 깊이)
        let mut stack: Vec<(String, String, PathBuf, usize)> = sources
            .into_iter()
            .rev()
            .map(|s| {
                let name = last_segment(&s);
                (s, name, dest.clone(), 0)
            })
            .collect();
        let mut seen = 0usize;

        while let Some((remote, display, local_parent, depth)) = stack.pop() {
            seen += 1;
            if seen > MAX_TREE_ITEMS {
                return Err(FsError::TooManyEntries.into());
            }
            let name = last_segment(&display);
            let target = match local_target(&local_parent, &name, self.rules) {
                Ok(t) => t,
                Err(e) => {
                    let shown = local_parent.to_string_lossy().into_owned();
                    items.push(Item::blocked(display, remote, shown, e.into()));
                    continue;
                }
            };
            let target_str = target.to_string_lossy().into_owned();
            let entry = match fs.stat(&remote).await {
                Ok(Some(e)) => e,
                Ok(None) => {
                    items.push(Item::blocked(
                        display,
                        remote,
                        target_str,
                        FsError::NotFound,
                    ));
                    continue;
                }
                Err(e) => {
                    items.push(Item::blocked(display, remote, target_str, e));
                    continue;
                }
            };
            match entry.kind {
                EntryKind::File => items.push(Item::Transfer {
                    name: display,
                    size: entry.size,
                    plan: Plan {
                        fs: fs.clone(),
                        direction: Direction::Download,
                        source: remote,
                        target: target_str,
                        policy,
                        source_modified: entry.modified,
                    },
                }),
                EntryKind::Dir if depth >= MAX_TREE_DEPTH => {
                    items.push(Item::blocked(
                        display,
                        remote,
                        target_str,
                        FsError::Remote("folder nesting is too deep".into()),
                    ));
                }
                EntryKind::Dir => {
                    if let Err(e) = ensure_local_dir(&target).await {
                        items.push(Item::blocked(display, remote, target_str, e));
                        continue;
                    }
                    let listing = match fs.list(&remote).await {
                        Ok(l) if l.truncated => Err(FsError::TooManyEntries),
                        other => other,
                    };
                    let listing = match listing {
                        Ok(l) => l,
                        Err(e) => {
                            items.push(Item::blocked(display, remote, target_str, e));
                            continue;
                        }
                    };
                    for child in listing.entries.into_iter().rev() {
                        let child_display = format!("{display}/{}", child.name);
                        match remote_join(&remote, &child.name) {
                            Ok(child_remote) => {
                                stack.push((child_remote, child_display, target.clone(), depth + 1))
                            }
                            Err(e) => items.push(Item::blocked(
                                child_display,
                                format!("{remote}/{}", child.name),
                                target_str.clone(),
                                e.into(),
                            )),
                        }
                    }
                }
                EntryKind::Symlink | EntryKind::Other => {
                    items.push(Item::skipped(display, remote, target_str, "symlink"))
                }
            }
        }
        Ok(self.add(session_id, Direction::Download, items))
    }

    async fn run(self: Arc<Self>, id: JobId) {
        let Some(plan) = self.jobs().get(&id).and_then(|r| r.plan.clone()) else {
            return;
        };
        let Ok(mut permit) = self.permits.clone().acquire_owned().await else {
            return;
        };

        let existing = match existing_target(&plan).await {
            Ok(e) => e,
            Err(e) => return self.set_state(&id, failed_state(&e)),
        };
        match decide(plan.policy, plan.source_modified, existing) {
            Ok(Decision::Proceed) => {}
            Ok(Decision::Skip(reason)) => {
                return self.set_state(
                    &id,
                    JobState::Skipped {
                        reason: reason.into(),
                    },
                )
            }
            Ok(Decision::Ask { size, modified }) => {
                // 응답을 기다리는 동안 다른 전송이 진행되도록 자리를 비웁니다.
                drop(permit);
                let (tx, rx) = oneshot::channel();
                let registered = match self.jobs().get_mut(&id) {
                    Some(r) => {
                        r.reply = Some(tx);
                        true
                    }
                    None => false,
                };
                if !registered {
                    return;
                }
                self.set_state(
                    &id,
                    JobState::Conflict {
                        existing_size: size,
                        existing_modified: modified,
                    },
                );
                match rx.await {
                    Ok(ConflictAction::Overwrite) => {}
                    Ok(ConflictAction::Skip) | Err(_) => {
                        return self.set_state(
                            &id,
                            JobState::Skipped {
                                reason: "exists".into(),
                            },
                        )
                    }
                }
                self.set_state(&id, JobState::Queued);
                permit = match self.permits.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => return,
                };
            }
            Err(e) => return self.set_state(&id, failed_state(&e)),
        }

        let mut attempt = 0;
        loop {
            attempt += 1;
            self.update(&id, |info| {
                info.state = JobState::Running;
                info.attempt = attempt;
                info.transferred = 0;
            });
            let result = match plan.direction {
                Direction::Upload => self.upload(&id, &plan).await,
                Direction::Download => self.download(&id, &plan).await,
            };
            match result {
                Ok(()) => return self.set_state(&id, JobState::Done),
                Err(e) if e.is_transient() && attempt < MAX_ATTEMPTS => {
                    tracing::warn!(code = e.code(), attempt, "transfer failed; retrying");
                    tokio::time::sleep(RETRY_BACKOFF * attempt).await;
                }
                Err(e) => {
                    tracing::warn!(code = e.code(), "transfer failed");
                    drop(permit);
                    return self.set_state(&id, failed_state(&e));
                }
            }
        }
    }

    async fn upload(&self, id: &str, plan: &Plan) -> Result<(), FsError> {
        let mut src = tokio::fs::File::open(&plan.source)
            .await
            .map_err(local::map_io)?;
        let mut dst = plan.fs.open_write(&plan.target).await?;
        self.pump(id, &mut src, &mut dst).await?;
        dst.shutdown().await.map_err(io_to_fs)
    }

    async fn download(&self, id: &str, plan: &Plan) -> Result<(), FsError> {
        let target = PathBuf::from(&plan.target);
        let part = partial_path(&target);
        refuse_symlink(&part).await?;
        let mut src = plan.fs.open_read(&plan.source).await?;
        let mut dst = tokio::fs::File::create(&part)
            .await
            .map_err(local::map_io)?;

        let result = async {
            self.pump(id, &mut src, &mut dst).await?;
            dst.flush().await.map_err(local::map_io)?;
            Ok::<_, FsError>(())
        }
        .await;
        let file = dst.into_std().await;
        if let Err(e) = result {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(e);
        }
        if let Some(secs) = plan.source_modified.and_then(|s| u64::try_from(s).ok()) {
            // 수정 시각 보존 실패는 전송 실패로 보지 않습니다.
            let _ = file.set_modified(UNIX_EPOCH + Duration::from_secs(secs));
        }
        drop(file);
        refuse_symlink(&target).await?;
        tokio::fs::rename(&part, &target)
            .await
            .map_err(local::map_io)
    }

    async fn pump(
        &self,
        id: &str,
        src: &mut (dyn AsyncRead + Send + Unpin),
        dst: &mut (dyn AsyncWrite + Send + Unpin),
    ) -> Result<u64, FsError> {
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        let mut last_emit = Instant::now();
        let mut last_total = 0u64;
        loop {
            let n = src.read(&mut buf).await.map_err(io_to_fs)?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await.map_err(io_to_fs)?;
            total += n as u64;
            let elapsed = last_emit.elapsed();
            if elapsed >= PROGRESS_INTERVAL {
                let rate = ((total - last_total) as f64 / elapsed.as_secs_f64()) as u64;
                self.progress(id, total, rate);
                last_emit = Instant::now();
                last_total = total;
            }
        }
        self.progress(id, total, 0);
        Ok(total)
    }

    /// "같은 이름 있음" 대화상자의 응답
    pub fn resolve(&self, id: &str, action: ConflictAction) -> Result<()> {
        let reply = self
            .jobs()
            .get_mut(id)
            .and_then(|r| r.reply.take())
            .ok_or_else(|| Error::NotFound(format!("pending conflict for {id}")))?;
        let _ = reply.send(action);
        Ok(())
    }

    pub fn cancel(&self, id: &str) {
        let (abort, part) = {
            let mut jobs = self.jobs();
            let Some(r) = jobs.get_mut(id) else { return };
            if r.info.state.is_finished() {
                return;
            }
            r.reply = None;
            let part = (r.info.direction == Direction::Download)
                .then(|| partial_path(Path::new(&r.info.target)));
            (r.abort.take(), part)
        };
        if let Some(abort) = abort {
            abort.abort();
        }
        if let Some(part) = part {
            let _ = std::fs::remove_file(part);
        }
        self.set_state(id, JobState::Cancelled);
    }

    /// 세션(탭)이 닫히면 그 세션의 남은 작업을 취소합니다.
    pub fn cancel_session(&self, session_id: &str) {
        let ids: Vec<JobId> = self
            .jobs()
            .values()
            .filter(|r| r.info.session_id == session_id && !r.info.state.is_finished())
            .map(|r| r.info.id.clone())
            .collect();
        for id in ids {
            self.cancel(&id);
        }
    }

    /// 실패하거나 취소된 작업을 다시 시작합니다.
    pub fn retry(self: &Arc<Self>, id: &str) -> Result<()> {
        {
            let mut jobs = self.jobs();
            let r = jobs
                .get_mut(id)
                .ok_or_else(|| Error::NotFound(format!("transfer {id}")))?;
            let retryable = match &r.info.state {
                JobState::Failed { retryable, .. } => *retryable,
                JobState::Cancelled => true,
                _ => false,
            };
            if !retryable || r.plan.is_none() {
                return Err(Error::Invalid("this transfer cannot be retried".into()));
            }
            r.info.state = JobState::Queued;
            r.info.transferred = 0;
            r.info.attempt = 0;
        }
        self.update(id, |_| {});
        self.start(id.to_owned());
        Ok(())
    }

    /// 끝난 작업을 목록에서 지웁니다.
    pub fn clear_finished(&self) {
        let removed: Vec<JobId> = {
            let mut jobs = self.jobs();
            let removed: Vec<JobId> = jobs
                .values()
                .filter(|r| r.info.state.is_finished())
                .map(|r| r.info.id.clone())
                .collect();
            for id in &removed {
                jobs.remove(id);
            }
            removed
        };
        if removed.is_empty() {
            return;
        }
        self.order().retain(|id| !removed.contains(id));
        self.emit(TransferEvent::Removed { ids: removed });
    }
}

async fn existing_target(plan: &Plan) -> Result<Option<(EntryKind, u64, Option<i64>)>, FsError> {
    match plan.direction {
        Direction::Upload => Ok(plan
            .fs
            .stat(&plan.target)
            .await?
            .map(|e| (e.kind, e.size, e.modified))),
        Direction::Download => match tokio::fs::symlink_metadata(&plan.target).await {
            Ok(meta) => {
                let e = local::entry_from_metadata(String::new(), &meta);
                Ok(Some((e.kind, meta.len(), e.modified)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(local::map_io(e)),
        },
    }
}

async fn refuse_symlink(path: &Path) -> Result<(), FsError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(m) if m.file_type().is_symlink() => Err(FsError::Symlink),
        _ => Ok(()),
    }
}

async fn ensure_local_dir(path: &Path) -> Result<(), FsError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(m) if m.file_type().is_symlink() => Err(FsError::Symlink),
        Ok(_) => Err(FsError::AlreadyExists),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(path).await.map_err(local::map_io)
        }
        Err(e) => Err(local::map_io(e)),
    }
}

struct LocalItem {
    path: PathBuf,
    /// 선택한 항목 이름부터의 상대 경로 구성요소
    rel: Vec<String>,
    kind: EntryKind,
    size: u64,
    modified: Option<i64>,
}

/// 업로드할 로컬 항목을 펼칩니다. 폴더 다음에 그 내용이 오도록 순서를 지킵니다.
fn walk_local(sources: &[PathBuf]) -> Result<Vec<LocalItem>, FsError> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for src in sources.iter().rev() {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(FsError::NotFound)?
            .to_owned();
        stack.push((src.clone(), vec![name]));
    }
    while let Some((path, rel)) = stack.pop() {
        if out.len() >= MAX_TREE_ITEMS {
            return Err(FsError::TooManyEntries);
        }
        let meta = std::fs::symlink_metadata(&path).map_err(local::map_io)?;
        let entry = local::entry_from_metadata(String::new(), &meta);
        if entry.kind == EntryKind::Dir {
            if rel.len() > MAX_TREE_DEPTH {
                return Err(FsError::Remote("folder nesting is too deep".into()));
            }
            let mut children: Vec<(PathBuf, String)> = std::fs::read_dir(&path)
                .map_err(local::map_io)?
                .filter_map(|c| c.ok())
                .filter_map(|c| Some((c.path(), c.file_name().into_string().ok()?)))
                .collect();
            children.sort_by(|a, b| b.1.cmp(&a.1));
            for (child, name) in children {
                let mut child_rel = rel.clone();
                child_rel.push(name);
                stack.push((child, child_rel));
            }
        }
        out.push(LocalItem {
            path,
            rel,
            kind: entry.kind,
            size: entry.size,
            modified: entry.modified,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
