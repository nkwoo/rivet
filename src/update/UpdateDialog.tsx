import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { Dialog } from "../components/Dialog.tsx";
import { DownloadIcon, WarningIcon } from "../components/Icons.tsx";
import { formatSize } from "../files/transfers.ts";
import { useI18n } from "../i18n/index.ts";
import {
  checkNow,
  closeUpdates,
  install,
  openUpdates,
  scheduleAutoCheck,
  setAutoCheck,
  useUpdates,
  type UpdateStatus,
} from "./updates.ts";

/** 시작 시 자동 확인과 macOS 메뉴 "업데이트 확인…"을 연결합니다. App에 한 번만 둡니다. */
export function useUpdateTriggers() {
  useEffect(() => scheduleAutoCheck(), []);
  useEffect(() => {
    const unlisten = listen("menu:check-update", () => checkNow());
    return () => void unlisten.then((f) => f());
  }, []);
}

/** 상단 막대: 새 버전이 있을 때만 보임 */
export function UpdateButton() {
  const { m } = useI18n();
  const { status } = useUpdates();
  const info = "info" in status ? status.info : null;
  if (!info || status.kind === "error") return null;
  return (
    <button type="button" className="button button-update" onClick={openUpdates} title={m.update.availableTitle}>
      <DownloadIcon />
      {m.update.button(info.version)}
    </button>
  );
}

function percent(status: Extract<UpdateStatus, { kind: "downloading" }>) {
  return status.total ? Math.min(100, Math.round((status.downloaded / status.total) * 100)) : null;
}

interface UpdateDialogProps {
  /** 설치하면 끊기는 열린 탭 수 */
  openSessions: number;
}

export function UpdateDialog({ openSessions }: UpdateDialogProps) {
  const { m, locale } = useI18n();
  const { status, open, autoCheck } = useUpdates();
  if (!open) return null;

  const info = "info" in status ? status.info : null;
  const working = status.kind === "downloading" || status.kind === "installing";
  const canInstall = status.kind === "available" || (status.kind === "error" && info !== null);

  let body;
  switch (status.kind) {
    case "idle":
    case "checking":
      body = <p className="muted">{m.update.checking}</p>;
      break;
    case "upToDate":
      body = <p>{m.update.upToDate}</p>;
      break;
    default:
      body = info ? (
        <>
          <p>
            {m.update.versions(info.currentVersion, info.version)}
            {info.publishedAt !== null && (
              <span className="muted"> · {new Date(info.publishedAt * 1000).toLocaleDateString(locale)}</span>
            )}
          </p>
          {info.notes && (
            <div className="update-notes">
              <h3>{m.update.notes}</h3>
              {/* 원격에서 받은 문구이므로 HTML로 해석하지 않고 그대로 보여줌 */}
              <pre>{info.notes}</pre>
            </div>
          )}
          {openSessions > 0 && !working && (
            <div role="alert" className="callout callout-warn">
              <WarningIcon />
              {m.update.sessionsWarning(openSessions)}
            </div>
          )}
        </>
      ) : null;
  }

  return (
    <Dialog
      title={info ? m.update.titleAvailable(info.version) : m.update.title}
      // 내려받는 중·설치 중에는 closeUpdates가 무시함
      onClose={closeUpdates}
      footer={
        <>
          <label className="checkbox update-auto">
            <input type="checkbox" checked={autoCheck} onChange={(e) => setAutoCheck(e.target.checked)} />
            {m.update.autoCheck}
          </label>
          <button type="button" className="button button-ghost" onClick={closeUpdates} disabled={working}>
            {canInstall ? m.update.later : m.common.close}
          </button>
          {(canInstall || working) && (
            <button type="button" className="button button-primary" onClick={() => void install()} disabled={working}>
              {m.update.install}
            </button>
          )}
        </>
      }
    >
      {body}
      {status.kind === "downloading" && (
        <div className="update-progress" role="status">
          <span className="queue-bar" aria-hidden="true">
            <span className="queue-fill is-down" style={{ width: `${percent(status) ?? 0}%` }} />
          </span>
          <span className="mono muted">
            {status.total
              ? `${formatSize(status.downloaded)} / ${formatSize(status.total)}`
              : formatSize(status.downloaded)}
          </span>
        </div>
      )}
      {status.kind === "installing" && (
        <p role="status" className="muted">
          {m.update.installing}
        </p>
      )}
      {status.kind === "error" && (
        <p role="alert" className="form-error">
          {info ? m.update.installFailed(status.message) : m.update.checkFailed(status.message)}
        </p>
      )}
    </Dialog>
  );
}
