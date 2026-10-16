import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  errorMessage,
  isApiError,
  type FsEntry,
  type Listing,
  type OverwritePolicy,
  type Profile,
  protocolLabel,
} from "../lib/api.ts";
import { DownloadIcon, FolderPlusIcon, EditIcon, TrashIcon, UploadIcon, WarningIcon } from "../components/Icons.tsx";
import { FilePane, emptyPane, type PaneState, type Side } from "./FilePane.tsx";
import { TransferQueue } from "./TransferQueue.tsx";
import { ConflictDialog, DeleteDialog, NameDialog } from "./FileDialogs.tsx";
import { ensureTransferSubscription, fsErrorText, useTransfers } from "./transfers.ts";

interface FileBrowserProps {
  sessionId: string;
  profile: Profile;
  /** 보이는 탭인지 (OS 파일 드롭은 보이는 탭만 받음) */
  active: boolean;
}

type DialogState =
  | { kind: "none" }
  | { kind: "mkdir" }
  | { kind: "rename"; name: string }
  | { kind: "delete"; entries: FsEntry[] };

interface DragState {
  from: Side;
  count: number;
  x: number;
  y: number;
  over: Side | null;
}

const POLICIES: { value: OverwritePolicy; label: string }[] = [
  { value: "ask", label: "매번 묻기" },
  { value: "newer", label: "더 최신이면 덮어쓰기" },
  { value: "overwrite", label: "덮어쓰기" },
  { value: "skip", label: "건너뛰기" },
];

function joinRemote(dir: string, name: string) {
  return dir.endsWith("/") ? dir + name : `${dir}/${name}`;
}

function joinLocal(dir: string, name: string) {
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

function listError(e: unknown): string {
  return isApiError(e) ? fsErrorText(e.code, e.message) : errorMessage(e);
}

/** 로컬 ↔ 원격 2단 탐색기와 전송 큐. 원격은 탭의 SSH 연결을 공유하는 SFTP입니다. */
export function FileBrowser({ sessionId, profile, active }: FileBrowserProps) {
  const [local, setLocal] = useState<PaneState>(emptyPane);
  const [remote, setRemote] = useState<PaneState>(emptyPane);
  const [policy, setPolicy] = useState<OverwritePolicy>("ask");
  const [dialog, setDialog] = useState<DialogState>({ kind: "none" });
  const [drag, setDrag] = useState<DragState | null>(null);
  const [osDropOver, setOsDropOver] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const { jobs: allJobs, rates } = useTransfers();
  const jobs = useMemo(() => allJobs.filter((j) => j.sessionId === sessionId), [allJobs, sessionId]);

  // 최신 상태를 이벤트 핸들러(포인터/OS 드롭)에서 읽기 위한 참조
  const latest = useRef({ local, remote, policy });
  latest.current = { local, remote, policy };

  const loadLocal = useCallback((path: string) => {
    setLocal((p) => ({ ...p, loading: true }));
    api
      .localList(path)
      .then((listing: Listing) =>
        setLocal((p) => ({
          listing,
          loading: false,
          error: null,
          selected: listing.path === p.listing?.path ? p.selected : new Set(),
        })),
      )
      .catch((e: unknown) => setLocal((p) => ({ ...p, loading: false, error: p.listing ? p.error : listError(e) })));
  }, []);

  const loadRemote = useCallback(
    (path: string) => {
      setRemote((p) => ({ ...p, loading: true }));
      api
        .sftpList(sessionId, path)
        .then((listing: Listing) =>
          setRemote((p) => ({
            listing,
            loading: false,
            error: null,
            selected: listing.path === p.listing?.path ? p.selected : new Set(),
          })),
        )
        .catch((e: unknown) => {
          setRemote((p) => ({ ...p, loading: false, error: p.listing ? p.error : listError(e) }));
          if (latest.current.remote.listing) setNotice(listError(e));
        });
    },
    [sessionId],
  );

  // 이동 실패는 현재 목록을 유지하고 알림만 띄움
  const navigateLocal = (path: string) => {
    api
      .localList(path)
      .then((listing) => setLocal({ listing, loading: false, error: null, selected: new Set() }))
      .catch((e: unknown) => setNotice(listError(e)));
  };

  const navigateRemote = (path: string) => {
    api
      .sftpList(sessionId, path)
      .then((listing) => setRemote({ listing, loading: false, error: null, selected: new Set() }))
      .catch((e: unknown) => setNotice(listError(e)));
  };

  useEffect(() => {
    ensureTransferSubscription();
    api
      .localHome()
      .then(loadLocal)
      .catch((e: unknown) => setLocal((p) => ({ ...p, loading: false, error: listError(e) })));
    api
      .sftpOpen(sessionId)
      .then(loadRemote)
      .catch((e: unknown) => setRemote((p) => ({ ...p, loading: false, error: listError(e) })));
  }, [sessionId, loadLocal, loadRemote]);

  // 전송이 끝나면 받는 쪽 목록을 새로 고침 (여러 건이면 한 번만)
  const doneCount = useMemo(() => {
    let up = 0;
    let down = 0;
    for (const j of jobs) {
      if (j.state.kind !== "done") continue;
      if (j.direction === "upload") up++;
      else down++;
    }
    return { up, down };
  }, [jobs]);
  const lastDone = useRef(doneCount);
  useEffect(() => {
    const prev = lastDone.current;
    lastDone.current = doneCount;
    const timer = window.setTimeout(() => {
      const { local: l, remote: r } = latest.current;
      if (doneCount.up > prev.up && r.listing) loadRemote(r.listing.path);
      if (doneCount.down > prev.down && l.listing) loadLocal(l.listing.path);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [doneCount, loadLocal, loadRemote]);

  const upload = useCallback(
    (sources: string[]) => {
      const { remote: r, policy: pol } = latest.current;
      if (!r.listing || sources.length === 0) return;
      api.transferUpload(sessionId, sources, r.listing.path, pol).catch((e: unknown) => setNotice(listError(e)));
    },
    [sessionId],
  );

  const download = useCallback(
    (sources: string[]) => {
      const { local: l, policy: pol } = latest.current;
      if (!l.listing || sources.length === 0) return;
      api.transferDownload(sessionId, sources, l.listing.path, pol).catch((e: unknown) => setNotice(listError(e)));
    },
    [sessionId],
  );

  const transferFrom = useCallback(
    (from: Side, names: string[]) => {
      const { local: l, remote: r } = latest.current;
      if (from === "local" && l.listing) upload(names.map((n) => joinLocal(l.listing!.path, n)));
      if (from === "remote" && r.listing) download(names.map((n) => joinRemote(r.listing!.path, n)));
    },
    [upload, download],
  );

  // ---------- 두 창 사이 끌어 놓기 (포인터 이벤트로 구현: Windows에서 HTML5 DnD가 막히는 문제 회피) ----------

  const paneAt = (x: number, y: number): Side | null => {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-pane]");
    if (!el || !root.current?.contains(el)) return null;
    return el.dataset.pane === "local" || el.dataset.pane === "remote" ? el.dataset.pane : null;
  };

  const startDrag = (from: Side) => (e: PointerEvent, names: string[]) => {
    const startX = e.clientX;
    const startY = e.clientY;
    let dragging = false;
    const move = (ev: globalThis.PointerEvent) => {
      if (!dragging && Math.hypot(ev.clientX - startX, ev.clientY - startY) < 6) return;
      dragging = true;
      setDrag({ from, count: names.length, x: ev.clientX, y: ev.clientY, over: paneAt(ev.clientX, ev.clientY) });
    };
    const up = (ev: globalThis.PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setDrag(null);
      if (!dragging) return;
      const target = paneAt(ev.clientX, ev.clientY);
      if (target && target !== from) transferFrom(from, names);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  // ---------- Finder/탐색기에서 원격 창으로 끌어 놓기 ----------

  useEffect(() => {
    if (!active) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const overRemote = (pos: { x: number; y: number }) => {
      const scale = window.devicePixelRatio || 1;
      return paneAt(pos.x / scale, pos.y / scale) === "remote";
    };
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") setOsDropOver(overRemote(p.position));
        else if (p.type === "drop") {
          setOsDropOver(false);
          if (overRemote(p.position)) upload(p.paths);
        } else setOsDropOver(false);
      })
      .then((f) => {
        if (disposed) f();
        else unlisten = f;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [active, upload]);

  // ---------- 원격 폴더 작업 ----------

  const remoteDir = remote.listing?.path ?? null;
  const remoteSelected = remote.listing?.entries.filter((e) => remote.selected.has(e.name)) ?? [];
  const refreshRemote = () => remoteDir && loadRemote(remoteDir);

  const conflicts = jobs.filter((j) => j.state.kind === "conflict");
  const answerConflict = (overwrite: boolean, applyToAll: boolean) => {
    const targets = applyToAll ? conflicts : conflicts.slice(0, 1);
    for (const j of targets) {
      api.transferResolve(j.id, overwrite ? "overwrite" : "skip").catch(() => undefined);
    }
  };

  const target = `${profile.username}@${profile.host}`;

  return (
    <div className="files" ref={root}>
      <header className="files-head">
        <span className="files-title">전송</span>
        <span className={`files-badge mono ${profile.protocol === "ftp" ? "is-plain" : ""}`}>
          {profile.name} · {profile.protocol === "ssh" ? "SFTP" : protocolLabel(profile.protocol)}
        </span>
        <div className="files-actions">
          <label htmlFor={`policy-${sessionId}`} className="files-policy-label">
            같은 이름이 있으면
          </label>
          <select
            id={`policy-${sessionId}`}
            className="select"
            value={policy}
            onChange={(e) => setPolicy(e.target.value as OverwritePolicy)}
          >
            {POLICIES.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="button button-secondary"
            disabled={remote.selected.size === 0}
            onClick={() => transferFrom("remote", [...remote.selected])}
          >
            <DownloadIcon />
            다운로드{remote.selected.size > 0 ? ` ${remote.selected.size}개` : ""}
          </button>
          <button
            type="button"
            className="button button-primary"
            disabled={local.selected.size === 0}
            onClick={() => transferFrom("local", [...local.selected])}
          >
            <UploadIcon />
            업로드{local.selected.size > 0 ? ` ${local.selected.size}개` : ""}
          </button>
        </div>
      </header>

      {profile.protocol === "ftp" && (
        <div role="note" className="files-plain">
          <WarningIcon />
          <span>평문 FTP 연결입니다. 비밀번호와 파일 내용이 암호화되지 않고 전송됩니다. 가능하면 FTPS나 SFTP를 사용하세요.</span>
        </div>
      )}

      {notice && (
        <div role="alert" className="files-notice">
          <span>{notice}</span>
          <button type="button" className="link-button" onClick={() => setNotice(null)}>
            닫기
          </button>
        </div>
      )}

      <div className="panes">
        <FilePane
          side="local"
          title="로컬"
          label="로컬 파일"
          pane={local}
          dropActive={drag?.from === "remote" && drag.over === "local"}
          onSelect={(selected) => setLocal((p) => ({ ...p, selected }))}
          onNavigate={navigateLocal}
          onRefresh={() => local.listing && loadLocal(local.listing.path)}
          onActivateFile={(entry) => transferFrom("local", [entry.name])}
          onDragStart={startDrag("local")}
        />
        <FilePane
          side="remote"
          title="원격"
          label={`${profile.name}의 원격 파일`}
          pathPrefix={`${target}:`}
          pane={remote}
          dropActive={(drag?.from === "local" && drag.over === "remote") || osDropOver}
          onSelect={(selected) => setRemote((p) => ({ ...p, selected }))}
          onNavigate={navigateRemote}
          onRefresh={refreshRemote}
          onActivateFile={(entry) => transferFrom("remote", [entry.name])}
          onDragStart={startDrag("remote")}
          actions={
            <>
              <button
                type="button"
                className="icon-button icon-button-boxed icon-button-sm"
                aria-label="새 폴더"
                title="새 폴더"
                disabled={!remoteDir}
                onClick={() => setDialog({ kind: "mkdir" })}
              >
                <FolderPlusIcon />
              </button>
              <button
                type="button"
                className="icon-button icon-button-boxed icon-button-sm"
                aria-label="이름 바꾸기"
                title="이름 바꾸기"
                disabled={remoteSelected.length !== 1}
                onClick={() => remoteSelected[0] && setDialog({ kind: "rename", name: remoteSelected[0].name })}
              >
                <EditIcon />
              </button>
              <button
                type="button"
                className="icon-button icon-button-boxed icon-button-sm"
                aria-label="삭제"
                title="삭제"
                disabled={remoteSelected.length === 0}
                onClick={() => setDialog({ kind: "delete", entries: remoteSelected })}
              >
                <TrashIcon />
              </button>
            </>
          }
        />
      </div>

      <TransferQueue jobs={jobs} rates={rates} onError={setNotice} />

      {drag && (
        <div className="drag-ghost" style={{ left: drag.x + 14, top: drag.y + 10 }} aria-hidden="true">
          {drag.from === "local" ? "↑ 업로드" : "↓ 다운로드"} {drag.count}개
        </div>
      )}

      {active && dialog.kind === "mkdir" && remoteDir && (
        <NameDialog
          title="새 폴더"
          initial=""
          confirm="만들기"
          onClose={() => setDialog({ kind: "none" })}
          onSubmit={async (name) => {
            await api.sftpMkdir(sessionId, remoteDir, name).catch((e: unknown) => {
              throw new Error(listError(e));
            });
            setDialog({ kind: "none" });
            refreshRemote();
          }}
        />
      )}
      {active && dialog.kind === "rename" && remoteDir && (
        <NameDialog
          title="이름 바꾸기"
          initial={dialog.name}
          confirm="바꾸기"
          onClose={() => setDialog({ kind: "none" })}
          onSubmit={async (name) => {
            await api.sftpRename(sessionId, remoteDir, dialog.name, name).catch((e: unknown) => {
              throw new Error(listError(e));
            });
            setDialog({ kind: "none" });
            refreshRemote();
          }}
        />
      )}
      {active && dialog.kind === "delete" && remoteDir && (
        <DeleteDialog
          names={dialog.entries.map((e) => e.name)}
          onClose={() => setDialog({ kind: "none" })}
          onConfirm={async () => {
            try {
              for (const entry of dialog.entries) {
                await api.sftpRemove(sessionId, remoteDir, entry.name, entry.kind === "dir");
              }
            } catch (e) {
              refreshRemote();
              throw new Error(listError(e));
            }
            setDialog({ kind: "none" });
            refreshRemote();
          }}
        />
      )}
      {active && dialog.kind === "none" && conflicts[0] && (
        <ConflictDialog key={conflicts[0].id} job={conflicts[0]} remaining={conflicts.length} onAnswer={answerConflict} />
      )}
    </div>
  );
}
