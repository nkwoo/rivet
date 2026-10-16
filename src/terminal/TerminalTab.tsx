import { useCallback, useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import { api, isApiError, toBytes, type ConnectionInfo, type Profile, type SessionEvent } from "../lib/api.ts";
import { HostKeyChanged, HostKeyPrompt } from "../security/HostKeyDialogs.tsx";
import { SecretPrompt } from "../profiles/SecretPrompt.tsx";
import { FileBrowser } from "../files/FileBrowser.tsx";
import { FolderIcon, TerminalIcon } from "../components/Icons.tsx";

export type TabState =
  | { kind: "connecting" }
  | { kind: "connected"; info: ConnectionInfo }
  | { kind: "closed"; message: string }
  | { kind: "error"; message: string };

type Overlay =
  | { kind: "none" }
  | { kind: "secret"; mode: "password" | "passphrase"; error?: string }
  | { kind: "hostKeyPrompt"; event: Extract<SessionEvent, { type: "hostKeyPrompt" }> }
  | { kind: "hostKeyChanged"; event: Extract<SessionEvent, { type: "hostKeyChanged" }> };

interface TerminalTabProps {
  profile: Profile;
  active: boolean;
  onState: (state: TabState) => void;
  onReviewKnownHosts: (host: string, port: number) => void;
}

/** 한 번에 보내는 입력 크기 (UTF-16 단위). UTF-8로 최대 48KiB라 백엔드 상한 64KiB 안에 들어감 */
const WRITE_CHUNK = 16 * 1024;

const THEME = {
  background: "#0B0D10",
  foreground: "#D7D4CE",
  cursor: "#E8E6E1",
  cursorAccent: "#0B0D10",
  selectionBackground: "#3A414B",
  black: "#1A1D22",
  red: "#E5735A",
  green: "#8FCB9B",
  yellow: "#E8C267",
  blue: "#6CA6E0",
  magenta: "#C49BE0",
  cyan: "#6FC3C9",
  white: "#D7D4CE",
  brightBlack: "#5A626E",
  brightRed: "#F27B2B",
  brightGreen: "#A9DDB3",
  brightYellow: "#F2D58C",
  brightBlue: "#9CC4EE",
  brightMagenta: "#D7B8EE",
  brightCyan: "#9ADBE0",
  brightWhite: "#F4F1EA",
};

/** 서로게이트 쌍이 잘리지 않는 위치에서 문자열을 자름 */
function takeChunk(s: string): string {
  if (s.length <= WRITE_CHUNK) return s;
  const last = s.charCodeAt(WRITE_CHUNK - 1);
  return s.slice(0, last >= 0xd800 && last <= 0xdbff ? WRITE_CHUNK - 1 : WRITE_CHUNK);
}

function describeExit(exitStatus: number | null): string {
  return exitStatus === null ? "세션이 종료되었습니다." : `세션이 종료되었습니다 (종료 코드 ${exitStatus}).`;
}

type View = "terminal" | "files";

/**
 * SSH 연결 하나를 맡는 탭. 셸은 xterm.js에, 파일 화면은 같은 연결의 SFTP에 붙습니다.
 * 출력은 바이너리 Channel로 받고, 입력은 순서를 지키도록 한 번에 하나씩 보냅니다.
 * SFTP 프로필은 셸 없이 파일 화면만 보여줍니다.
 */
export function TerminalTab({ profile, active, onState, onReviewKnownHosts }: TerminalTabProps) {
  const container = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<string | null>(null);
  const disposed = useRef(false);
  const lastError = useRef<{ code: string; message: string } | null>(null);
  const pending = useRef("");
  const writing = useRef(false);
  /** `connected` 이벤트가 `ssh_connect` 응답보다 먼저 올 수 있어 따로 기록 */
  const connectedSeen = useRef(false);
  const onStateRef = useRef(onState);
  onStateRef.current = onState;

  const filesOnly = profile.protocol === "sftp";
  const [overlay, setOverlay] = useState<Overlay>({ kind: "none" });
  const [ended, setEnded] = useState(false);
  const [status, setStatus] = useState<TabState>({ kind: "connecting" });
  /** 연결된 세션 ID (파일 화면이 SFTP를 열 때 사용) */
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [view, setView] = useState<View>(filesOnly ? "files" : "terminal");

  const report = useCallback((state: TabState) => {
    setStatus(state);
    onStateRef.current(state);
  }, []);

  const flush = useCallback(async () => {
    if (writing.current) return;
    writing.current = true;
    try {
      while (pending.current && session.current) {
        const chunk = takeChunk(pending.current);
        pending.current = pending.current.slice(chunk.length);
        await api.sshWrite(session.current, chunk);
      }
    } catch {
      pending.current = "";
    } finally {
      writing.current = false;
    }
  }, []);

  const handleEvent = useCallback(
    (event: SessionEvent) => {
      switch (event.type) {
        case "hostKeyPrompt":
          setOverlay({ kind: "hostKeyPrompt", event });
          break;
        case "hostKeyChanged":
          setOverlay({ kind: "hostKeyChanged", event });
          break;
        case "connected": {
          const { type: _t, ...info } = event;
          lastError.current = null;
          setOverlay({ kind: "none" });
          connectedSeen.current = true;
          if (session.current) setSessionId(session.current);
          report({ kind: "connected", info });
          term.current?.focus();
          break;
        }
        case "error":
          lastError.current = { code: event.code, message: event.message };
          break;
        case "closed": {
          session.current = null;
          pending.current = "";
          connectedSeen.current = false;
          setSessionId(null);
          setEnded(true);
          const err = lastError.current;
          if (err?.code === "authFailed" && profile.authMethod === "password") {
            // 저장된/입력한 비밀번호가 틀림: 다시 입력받음
            setOverlay({ kind: "secret", mode: "password", error: "인증에 실패했습니다. 비밀번호를 확인하세요." });
            report({ kind: "error", message: "인증 실패" });
          } else if (err) {
            if (err.code !== "hostKeyChanged") setOverlay({ kind: "none" });
            term.current?.write(`\r\n\x1b[31m${err.message}\x1b[0m\r\n`);
            report({ kind: "error", message: err.message });
          } else {
            const message = describeExit(event.exitStatus);
            term.current?.write(`\r\n\x1b[90m${message}\x1b[0m\r\n`);
            report({ kind: "closed", message });
          }
          break;
        }
      }
    },
    [profile.authMethod, report],
  );

  const connect = useCallback(
    async (secret: string | null, saveSecret: boolean) => {
      const t = term.current;
      if (!t || disposed.current) return;
      setEnded(false);
      setOverlay({ kind: "none" });
      lastError.current = null;
      connectedSeen.current = false;
      report({ kind: "connecting" });
      t.write(`\x1b[90m${profile.username}@${profile.host}:${profile.port}에 연결하는 중…\x1b[0m\r\n`);

      // 개발 모드 StrictMode의 재마운트 등으로 터미널이 바뀌었으면 이전 연결의 메시지는 버림
      const current = () => !disposed.current && term.current === t;
      const events = new Channel<SessionEvent>();
      events.onmessage = (event) => current() && handleEvent(event);
      const output = new Channel<ArrayBuffer>();
      output.onmessage = (data) => current() && t.write(toBytes(data));

      try {
        const id = await api.sshConnect({
          profileId: profile.id,
          cols: t.cols,
          rows: t.rows,
          secret,
          saveSecret,
          events,
          output,
        });
        if (!current()) {
          void api.sshClose(id);
          return;
        }
        session.current = id;
        if (connectedSeen.current) setSessionId(id);
      } catch (e) {
        if (!current()) return;
        setEnded(true);
        if (isApiError(e) && e.code === "passwordRequired") {
          setOverlay({ kind: "secret", mode: "password" });
        } else if (isApiError(e) && e.code === "passphraseRequired") {
          setOverlay({ kind: "secret", mode: "passphrase" });
        } else if (isApiError(e) && e.code === "keyLoad" && secret !== null) {
          setOverlay({ kind: "secret", mode: "passphrase", error: "패스프레이즈가 올바르지 않습니다." });
        } else {
          const message = isApiError(e) ? e.message : String(e);
          t.write(`\x1b[31m${message}\x1b[0m\r\n`);
          report({ kind: "error", message });
        }
      }
    },
    [profile, handleEvent, report],
  );

  // 터미널 생성과 최초 연결
  useEffect(() => {
    const el = container.current;
    if (!el) return;
    disposed.current = false;
    const t = new Terminal({
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13.5,
      lineHeight: 1.25,
      cursorBlink: true,
      scrollback: 10000,
      allowProposedApi: false,
      theme: THEME,
    });
    const f = new FitAddon();
    t.loadAddon(f);
    t.open(el);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      t.loadAddon(webgl);
    } catch {
      // WebGL을 쓸 수 없으면 기본 DOM 렌더러 사용
    }
    f.fit();
    term.current = t;
    fit.current = f;

    const dataSub = t.onData((data) => {
      if (!session.current) return;
      pending.current += data;
      void flush();
    });
    let resizeTimer: number | undefined;
    const resizeSub = t.onResize(({ cols, rows }) => {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        if (session.current) void api.sshResize(session.current, cols, rows).catch(() => {});
      }, 80);
    });

    void connect(null, false);

    return () => {
      disposed.current = true;
      window.clearTimeout(resizeTimer);
      if (session.current) void api.sshClose(session.current);
      session.current = null;
      dataSub.dispose();
      resizeSub.dispose();
      t.dispose();
      term.current = null;
    };
    // 탭 하나는 프로필 하나에 고정되므로 마운트 시 한 번만 연결
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 창/패널 크기 변화에 맞춰 터미널 크기 조정 (보이는 탭의 터미널 화면만)
  useEffect(() => {
    const el = container.current;
    if (!el || !active || view !== "terminal") return;
    const refit = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) fit.current?.fit();
    };
    refit();
    term.current?.focus();
    const observer = new ResizeObserver(refit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, view]);

  const respondHostKey = (accept: boolean) => {
    const id = session.current;
    setOverlay({ kind: "none" });
    if (id) void api.sshHostKeyRespond(id, accept).catch(() => {});
  };

  const target = `${profile.username}@${profile.host}:${profile.port}`;

  const reconnect = (
    <button type="button" className="button button-secondary button-small" onClick={() => connect(null, false)}>
      다시 연결
    </button>
  );

  return (
    <div className="terminal-tab" hidden={!active}>
      {!filesOnly && (
        <div className="session-bar">
          <div role="tablist" aria-label="화면" className="view-switch">
            <button
              type="button"
              role="tab"
              aria-selected={view === "terminal"}
              className={view === "terminal" ? "is-on" : undefined}
              onClick={() => setView("terminal")}
            >
              <TerminalIcon />
              터미널
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={view === "files"}
              className={view === "files" ? "is-on" : undefined}
              disabled={!sessionId && view !== "files"}
              title={sessionId ? "같은 연결로 파일 전송" : "연결된 뒤 사용할 수 있습니다"}
              onClick={() => setView("files")}
            >
              <FolderIcon />
              파일
            </button>
          </div>
        </div>
      )}
      <div className="session-body">
        <div className="terminal-pane" hidden={view !== "terminal"}>
          <div ref={container} className="terminal-host" />
          {ended && overlay.kind === "none" && <div className="terminal-actions">{reconnect}</div>}
        </div>
        {view === "files" &&
          (sessionId ? (
            <FileBrowser key={sessionId} sessionId={sessionId} profile={profile} active={active} />
          ) : (
            <div className="session-placeholder">
              <p className={status.kind === "error" ? "status-error" : "muted"}>
                {status.kind === "connecting"
                  ? `${target}에 연결하는 중…`
                  : status.kind === "error" || status.kind === "closed"
                    ? status.message
                    : ""}
              </p>
              {ended && overlay.kind === "none" && reconnect}
            </div>
          ))}
      </div>
      {active && overlay.kind === "secret" && (
        <SecretPrompt
          kind={overlay.mode}
          name={profile.name}
          target={target}
          error={overlay.error}
          onSubmit={(secret, save) => void connect(secret, save)}
          onCancel={() => {
            setOverlay({ kind: "none" });
            report({ kind: "closed", message: "연결 취소됨" });
          }}
        />
      )}
      {active && overlay.kind === "hostKeyPrompt" && (
        <HostKeyPrompt
          name={profile.name}
          host={overlay.event.host}
          port={overlay.event.port}
          algorithm={overlay.event.algorithm}
          fingerprint={overlay.event.fingerprint}
          onTrust={() => respondHostKey(true)}
          onCancel={() => respondHostKey(false)}
        />
      )}
      {active && overlay.kind === "hostKeyChanged" && (
        <HostKeyChanged
          name={profile.name}
          host={overlay.event.host}
          port={overlay.event.port}
          algorithm={overlay.event.algorithm}
          expectedFingerprint={overlay.event.expectedFingerprint}
          actualFingerprint={overlay.event.actualFingerprint}
          onReview={() => {
            setOverlay({ kind: "none" });
            onReviewKnownHosts(overlay.event.host, overlay.event.port);
          }}
          onClose={() => setOverlay({ kind: "none" })}
        />
      )}
    </div>
  );
}
