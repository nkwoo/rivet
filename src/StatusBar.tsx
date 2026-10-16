import { ShieldCheckIcon } from "./components/Icons.tsx";
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
          <span>
            {state.info.kex} · {state.info.cipher}
          </span>
          <span className="status-item status-ok" title="known_hosts와 일치하는 호스트 키">
            <ShieldCheckIcon />
            {state.info.hostKeyAlgorithm} {state.info.fingerprint} 검증됨
          </span>
        </>
      )}
      {state?.kind === "closed" && <span className="muted">{state.message}</span>}
      {state?.kind === "error" && (
        <span className="status-item status-error">
          <span className="dot dot-error" />
          {state.message}
        </span>
      )}
      <span className="statusbar-end muted">{app ? `${app.name} ${app.version}` : "Rivet"}</span>
    </footer>
  );
}
