import { ShieldCheckIcon, WarningIcon } from "./components/Icons.tsx";
import type { AppInfo } from "./App.tsx";
import type { TabState } from "./terminal/TerminalTab.tsx";
import { useI18n } from "./i18n/index.ts";
import { checkNow } from "./update/updates.ts";

interface StatusBarProps {
  app: AppInfo | null;
  state: TabState | null;
}

/** 연결 상태와 호스트 키 검증 결과를 항상 노출 */
export function StatusBar({ app, state }: StatusBarProps) {
  const { m } = useI18n();
  return (
    <footer className="statusbar">
      {state === null && <span className="muted">{m.status.none}</span>}
      {state?.kind === "connecting" && (
        <span className="status-item">
          <span className="dot dot-wait" />
          {m.status.connecting}
        </span>
      )}
      {state?.kind === "connected" && (
        <>
          <span className="status-item">
            <span className="dot dot-on" />
            {m.status.connected}
          </span>
          {state.info.via && state.info.via.length > 0 && (
            <span title={m.status.viaTitle}>{m.status.via(state.info.via.join(" → "))}</span>
          )}
          {state.info.transport === "ssh" && (
            <>
              <span title={m.status.kexTitle}>
                {state.info.kex} · {state.info.cipher}
              </span>
              <span className="status-item status-ok" title={m.status.hostKeyTitle}>
                <ShieldCheckIcon />
                {m.status.hostKeyVerified(state.info.hostKeyAlgorithm, state.info.fingerprint)}
              </span>
            </>
          )}
          {(state.info.transport === "tlsCa" || state.info.transport === "tlsPinned") && (
            <>
              <span>
                {state.info.kex}
                {state.info.cipher && ` · ${m.status.rdpAuth[state.info.cipher] ?? state.info.cipher}`}
              </span>
              <span
                className="status-item status-ok"
                title={m.status.certTitle(state.info.fingerprint)}
              >
                <ShieldCheckIcon />
                {state.info.transport === "tlsCa"
                  ? m.status.certCa
                  : m.status.certPinned(state.info.fingerprint.slice(0, 23))}
              </span>
            </>
          )}
          {state.info.transport === "plain" && (
            <span className="status-item status-warn" title={m.status.plainTitle}>
              <WarningIcon />
              {m.status.plain}
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
      <button
        type="button"
        className={`statusbar-version ${state?.kind === "connected" && state.detail ? "" : "statusbar-end"}`}
        title={m.update.check}
        onClick={checkNow}
      >
        {app ? `${app.name} ${app.version}` : "Rivet"}
      </button>
    </footer>
  );
}
