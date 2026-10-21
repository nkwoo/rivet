import { ShieldCheckIcon, WarningIcon } from "./components/Icons.tsx";
import type { AppInfo } from "./App.tsx";
import type { TabState } from "./terminal/TerminalTab.tsx";

interface StatusBarProps {
  app: AppInfo | null;
  state: TabState | null;
}

/** 연결 상태와 호스트 키 검증 결과를 항상 노출 */
export function StatusBar({ app, state }: StatusBarProps) {
  return (
    <footer className="statusbar">
      {state === null && <span className="muted">연결 없음</span>}
      {state?.kind === "connecting" && (
        <span className="status-item">
          <span className="dot dot-wait" />
          연결 중…
        </span>
      )}
      {state?.kind === "connected" && (
        <>
          <span className="status-item">
            <span className="dot dot-on" />
            연결됨
          </span>
          {state.info.via && state.info.via.length > 0 && (
            <span title="점프 호스트를 거쳐 연결됨">경유 {state.info.via.join(" → ")}</span>
          )}
          {state.info.transport === "ssh" && (
            <>
              <span title="키 교환 · 암호화">
                {state.info.kex} · {state.info.cipher}
              </span>
              <span className="status-item status-ok" title="known_hosts와 일치하는 호스트 키">
                <ShieldCheckIcon />
                {state.info.hostKeyAlgorithm} {state.info.fingerprint} 검증됨
              </span>
            </>
          )}
          {(state.info.transport === "tlsCa" || state.info.transport === "tlsPinned") && (
            <>
              <span>{state.info.kex}</span>
              <span
                className="status-item status-ok"
                title={`서버 인증서 SHA-256 ${state.info.fingerprint}`}
              >
                <ShieldCheckIcon />
                {state.info.transport === "tlsCa"
                  ? "인증 기관이 검증한 인증서"
                  : `고정한 인증서와 일치 ${state.info.fingerprint.slice(0, 23)}…`}
              </span>
            </>
          )}
          {state.info.transport === "plain" && (
            <span className="status-item status-warn" title="비밀번호와 파일 내용이 암호화되지 않습니다">
              <WarningIcon />
              FTP · 암호화되지 않은 연결
            </span>
          )}
        </>
      )}
      {state?.kind === "closed" && <span className="muted">{state.message}</span>}
      {state?.kind === "error" && (
        <span className="status-item status-error">
          <span className="dot dot-error" />
          {state.message}
        </span>
      )}
      {state?.kind === "connected" && state.detail && (
        <span className="statusbar-end mono">{state.detail}</span>
      )}
      <span className={`muted ${state?.kind === "connected" && state.detail ? "" : "statusbar-end"}`}>
        {app ? `${app.name} ${app.version}` : "Rivet"}
      </span>
    </footer>
  );
}
