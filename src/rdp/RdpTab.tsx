import { useCallback, useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { api, isApiError, toBytes, type Profile, type RdpInputEvent, type SessionEvent } from "../lib/api.ts";
import { CertChanged, CertPrompt } from "../security/HostKeyDialogs.tsx";
import { SecretPrompt } from "../profiles/SecretPrompt.tsx";
import {
  CheckIcon,
  ClipboardIcon,
  ExitFullscreenIcon,
  FullscreenIcon,
  MonitorIcon,
} from "../components/Icons.tsx";
import type { TabState } from "../terminal/TerminalTab.tsx";
import { applyMessage, cursorCss } from "./frames.ts";
import { SC, scancode } from "./keymap.ts";

type Overlay =
  | { kind: "none" }
  | { kind: "secret"; error?: string }
  | { kind: "certPrompt"; event: Extract<SessionEvent, { type: "certPrompt" }> }
  | { kind: "certChanged"; event: Extract<SessionEvent, { type: "certChanged" }> };

interface RdpTabProps {
  profile: Profile;
  active: boolean;
  onState: (state: TabState) => void;
  onReviewKnownHosts: (host: string, port: number) => void;
  /** 전체 화면: 앱의 상단/사이드바/상태 표시줄을 숨김 */
  onImmersive: (on: boolean) => void;
}

const ERROR_TEXT: Record<string, string> = {
  connect: "서버에 연결할 수 없습니다",
  timeout: "연결 시간이 초과되었습니다",
  authFailed: "인증에 실패했습니다",
  certChanged: "인증서가 바뀌어 연결을 차단했습니다",
  certRejected: "인증서를 승인하지 않아 연결을 취소했습니다",
  tls: "TLS 연결에 실패했습니다",
  rdpNegotiation: "서버와 보안 방식을 협상하지 못했습니다",
  rdpProtocol: "RDP 프로토콜 오류",
  connectionLost: "연결이 끊어졌습니다",
};

function errorText(code: string, message: string): string {
  const text = ERROR_TEXT[code];
  return text ? `${text} (${message})` : message;
}

const IS_MAC = navigator.userAgent.includes("Mac");
/** 탭 크기를 모를 때 쓰는 크기 */
const FALLBACK = { width: 1280, height: 800 };
const RESIZE_DELAY = 400;

type ViewMode = "fit" | "actual";

/**
 * 원격 화면 크기 요청: 고정 해상도 또는 탭 크기 (Retina 접속이면 실제 픽셀 수).
 * 탭보다 커지면 스크롤 막대가 생겨 크기가 다시 바뀌므로 짝수로 내림합니다.
 */
function desiredSize(profile: Profile, stage: HTMLElement | null) {
  const s = profile.rdp;
  const dpr = s?.hidpi ? window.devicePixelRatio || 1 : 1;
  const scale = s?.scale ?? Math.max(100, Math.min(500, Math.round(dpr * 100)));
  if (s?.width && s.height) return { width: s.width, height: s.height, scale };
  const w = stage && stage.clientWidth > 0 ? stage.clientWidth : FALLBACK.width;
  const h = stage && stage.clientHeight > 0 ? stage.clientHeight : FALLBACK.height;
  return { width: Math.floor((w * dpr) / 2) * 2, height: Math.floor(h * dpr), scale };
}

/**
 * RDP 연결 하나를 맡는 탭. 화면은 바이너리 Channel로 받은 변경 영역을 캔버스에 그리고,
 * 키보드/마우스는 묶어서 순서대로 보냅니다. 키보드는 숨은 입력란이 받아 IME(한글 등) 조합을 지원합니다.
 */
export function RdpTab({ profile, active, onState, onReviewKnownHosts, onImmersive }: RdpTabProps) {
  /** 탭 전체 영역. 스크롤 막대의 영향을 받지 않아 원격 해상도 계산에 씀 (스크롤되는 화면 영역을 재면 크기가 계속 바뀜) */
  const root = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const ime = useRef<HTMLTextAreaElement>(null);
  const session = useRef<string | null>(null);
  const generation = useRef(0);
  const lastError = useRef<{ code: string; message: string } | null>(null);
  const connectedSeen = useRef(false);
  const queue = useRef<RdpInputEvent[]>([]);
  const sending = useRef(false);
  const wheelRest = useRef({ x: 0, y: 0 });
  /** macOS ⌘: 다른 키와 함께 누르면 Ctrl, 혼자 누르면 Windows 키 */
  const meta = useRef({ down: false, ctrlSent: false, used: false, right: false });
  const frameCount = useRef(0);
  /** 진단 표시용 1초 누적값 */
  const stats = useRef({ bytes: 0, drawMs: 0, drawMax: 0, inputLag: [] as number[], inputAt: 0 });
  const onStateRef = useRef(onState);
  onStateRef.current = onState;

  const [overlay, setOverlay] = useState<Overlay>({ kind: "none" });
  const [status, setStatus] = useState<TabState>({ kind: "connecting" });
  const [connected, setConnected] = useState(false);
  const [desktop, setDesktop] = useState<{ width: number; height: number } | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("fit");
  const [clipboard, setClipboard] = useState(true);
  const [fullscreen, setFullscreen] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState(false);
  /** 맥 입력기 모드: 조합 중인 글자와 미리보기 위치 (화면 영역 기준, 마지막 클릭 위치) */
  const [composing, setComposing] = useState("");
  const caret = useRef<{ x: number; y: number } | null>(null);
  const remoteIme = profile.rdp?.keyboard === "remote";
  const hangulKey = profile.rdp?.hangulKey ?? "rightAlt";
  const [diag, setDiag] = useState<string>("측정 중…");
  const [stageSize, setStageSize] = useState({ width: 0, height: 0 });
  const [toolbarHidden, setToolbarHidden] = useState(false);

  const hidpiScale = profile.rdp?.hidpi && !profile.rdp.width ? window.devicePixelRatio || 1 : 1;

  const statusRef = useRef<TabState>(status);
  const report = useCallback((state: TabState) => {
    statusRef.current = state;
    setStatus(state);
    onStateRef.current(state);
  }, []);

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.setTimeout(() => setToast((t) => (t === message ? null : t)), 2500);
  }, []);

  // ---------- 입력 전송 (순서 유지, 연속 이동은 마지막 것만) ----------

  const flush = useCallback(async () => {
    if (sending.current) return;
    sending.current = true;
    try {
      while (queue.current.length > 0 && session.current) {
        const batch = queue.current.splice(0, 200);
        await api.rdpInput(session.current, batch);
      }
    } catch {
      queue.current = [];
    } finally {
      sending.current = false;
    }
  }, []);

  const send = useCallback(
    (...events: RdpInputEvent[]) => {
      if (!session.current || !connectedSeen.current) return;
      if (events.some((e) => (e.t === "k" && e.down) || (e.t === "b" && e.down) || e.t === "u")) {
        stats.current.inputAt ||= performance.now();
      }
      for (const e of events) {
        const last = queue.current[queue.current.length - 1];
        if (e.t === "m" && last?.t === "m") queue.current[queue.current.length - 1] = e;
        else queue.current.push(e);
      }
      void flush();
    },
    [flush],
  );

  const releaseAll = useCallback(() => {
    meta.current = { down: false, ctrlSent: false, used: false, right: false };
    setComposing("");
    send({ t: "r" });
  }, [send]);

  // ---------- 연결 ----------

  const handleEvent = useCallback(
    (event: SessionEvent) => {
      switch (event.type) {
        case "certPrompt":
          setOverlay({ kind: "certPrompt", event });
          break;
        case "certChanged":
          setOverlay({ kind: "certChanged", event });
          break;
        case "connected": {
          const { type: _t, ...info } = event;
          lastError.current = null;
          connectedSeen.current = true;
          setOverlay({ kind: "none" });
          setConnected(true);
          report({ kind: "connected", info });
          ime.current?.focus();
          break;
        }
        case "clipboard":
          showToast(
            event.direction === "toRemote"
              ? `클립보드 텍스트를 ${profile.name}에 보냈습니다`
              : `${profile.name}에서 복사한 텍스트를 클립보드에 넣었습니다`,
          );
          break;
        case "error":
          lastError.current = { code: event.code, message: event.message };
          break;
        case "closed": {
          session.current = null;
          connectedSeen.current = false;
          queue.current = [];
          setConnected(false);
          const err = lastError.current;
          if (err?.code === "authFailed") {
            setOverlay({ kind: "secret", error: `인증에 실패했습니다. 비밀번호를 확인하세요. (${err.message})` });
            report({ kind: "error", message: "인증 실패" });
          } else if (err) {
            if (err.code !== "certChanged") setOverlay({ kind: "none" });
            report({ kind: "error", message: errorText(err.code, err.message) });
          } else {
            report({ kind: "closed", message: "원격 데스크톱 연결이 종료되었습니다." });
          }
          break;
        }
        default:
          break;
      }
    },
    [profile.name, report, showToast],
  );

  const connect = useCallback(
    async (secret: string | null, saveSecret: boolean) => {
      const gen = ++generation.current;
      const current = () => generation.current === gen;
      setOverlay({ kind: "none" });
      lastError.current = null;
      connectedSeen.current = false;
      report({ kind: "connecting" });

      const events = new Channel<SessionEvent>();
      events.onmessage = (event) => current() && handleEvent(event);
      const frames = new Channel<ArrayBuffer>();
      frames.onmessage = (data) => {
        const c = canvas.current;
        if (!current() || !c) return;
        const bytes = toBytes(data);
        const t0 = performance.now();
        const msg = applyMessage(bytes, c);
        if (msg.kind === "frame") {
          const st = stats.current;
          const drawMs = performance.now() - t0;
          st.bytes += bytes.byteLength;
          st.drawMs += drawMs;
          st.drawMax = Math.max(st.drawMax, drawMs);
          // 키/클릭 뒤 첫 화면 갱신까지 (네트워크·서버 처리 포함)
          if (st.inputAt) {
            st.inputLag.push(t0 - st.inputAt);
            st.inputAt = 0;
          }
          frameCount.current++;
          setDesktop((d) => (d?.width === msg.width && d.height === msg.height ? d : { width: msg.width, height: msg.height }));
          if (session.current) void api.rdpFrameAck(session.current).catch(() => {});
        } else if (msg.kind === "pointer") {
          c.style.cursor = cursorCss(msg.pointer, hidpiScale);
        }
      };

      const size = desiredSize(profile, root.current);
      try {
        const id = await api.rdpConnect({ profileId: profile.id, ...size, secret, saveSecret, events, frames });
        if (!current()) {
          void api.sshClose(id);
          return;
        }
        session.current = id;
      } catch (e) {
        if (!current()) return;
        if (isApiError(e) && e.code === "passwordRequired") {
          setOverlay({ kind: "secret" });
          report({ kind: "closed", message: "비밀번호가 필요합니다" });
        } else {
          report({ kind: "error", message: isApiError(e) ? errorText(e.code, e.message) : String(e) });
        }
      }
    },
    [profile, handleEvent, report, hidpiScale],
  );

  const disconnect = useCallback(() => {
    generation.current++;
    const id = session.current;
    session.current = null;
    connectedSeen.current = false;
    setConnected(false);
    if (id) void api.sshClose(id);
    report({ kind: "closed", message: "연결을 끊었습니다." });
  }, [report]);

  // 최초 연결과 정리
  useEffect(() => {
    void connect(null, false);
    return () => {
      generation.current++;
      if (session.current) void api.sshClose(session.current);
      session.current = null;
    };
    // 탭 하나는 프로필 하나에 고정되므로 마운트 시 한 번만 연결
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 탭 크기 추적 + 창에 맞춤 해상도면 원격 해상도도 따라 바꿈
  useEffect(() => {
    const el = root.current;
    if (!el || !active) return;
    let timer: number | undefined;
    const measure = () => {
      setStageSize({ width: el.clientWidth, height: el.clientHeight });
      if (profile.rdp?.width || el.clientWidth === 0) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        const id = session.current;
        if (!id || !connectedSeen.current) return;
        const s = desiredSize(profile, el);
        void api.rdpResize(id, s.width, s.height, s.scale).catch(() => {});
      }, RESIZE_DELAY);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [active, profile]);

  // 탭 활성화/창 포커스: 입력란 포커스, 로컬 클립보드 변경 알림 / 포커스 잃음: 눌린 키 해제
  useEffect(() => {
    if (!active) {
      releaseAll();
      return;
    }
    ime.current?.focus();
    const syncClipboard = () => {
      if (session.current && connectedSeen.current) void api.rdpClipboard(session.current).catch(() => {});
    };
    syncClipboard();
    const onBlur = () => releaseAll();
    window.addEventListener("focus", syncClipboard);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", syncClipboard);
      window.removeEventListener("blur", onBlur);
    };
  }, [active, connected, releaseAll]);

  // 프레임 수 → 상태 표시줄 (보이는 탭만)
  useEffect(() => {
    if (!active || !connected) return;
    const timer = window.setInterval(() => {
      const fps = frameCount.current;
      frameCount.current = 0;
      const s = statusRef.current;
      const c = canvas.current;
      if (s.kind === "connected" && c) report({ ...s, detail: `${c.width}×${c.height} · ${fps} fps` });

      const st = stats.current;
      const lag = [...st.inputLag].sort((a, b) => a - b);
      const lagText = lag.length ? `${Math.round(lag[Math.floor(lag.length / 2)] ?? 0)} ms` : "–";
      setDiag(
        `${fps} fps · 그리기 평균 ${fps ? (st.drawMs / fps).toFixed(1) : "0.0"} ms (최대 ${st.drawMax.toFixed(1)}) · ` +
          `수신 ${(st.bytes / 1024 / 1024).toFixed(1)} MiB/s · 입력→화면 ${lagText}`,
      );
      stats.current = { bytes: 0, drawMs: 0, drawMax: 0, inputLag: [], inputAt: st.inputAt };
    }, 1000);
    return () => window.clearInterval(timer);
  }, [active, connected, report]);

  // 전체 화면: 앱 테두리를 숨기고 창을 전체 화면으로. 탭을 떠나면 해제
  const windowFullscreen = useRef(false);
  useEffect(() => {
    const on = fullscreen && active;
    onImmersive(on);
    // 이 탭이 켠 전체 화면만 끕니다 (사용자가 직접 켠 창 전체 화면은 건드리지 않음)
    if (windowFullscreen.current !== on) {
      windowFullscreen.current = on;
      try {
        void getCurrentWindow()
          .setFullscreen(on)
          .catch(() => {});
      } catch {
        // 창 API를 쓸 수 없으면 앱 테두리만 숨김
      }
    }
    if (!on) {
      setToolbarHidden(false);
      return;
    }
    const timer = window.setTimeout(() => setToolbarHidden(true), 2000);
    return () => window.clearTimeout(timer);
  }, [fullscreen, active, onImmersive]);

  useEffect(() => () => onImmersive(false), [onImmersive]);

  // ---------- 마우스 ----------

  const toRemote = (e: { clientX: number; clientY: number }) => {
    const c = canvas.current;
    if (!c) return null;
    const r = c.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return null;
    const x = Math.round(((e.clientX - r.left) * c.width) / r.width);
    const y = Math.round(((e.clientY - r.top) * c.height) / r.height);
    return { x: Math.max(0, Math.min(c.width - 1, x)), y: Math.max(0, Math.min(c.height - 1, y)) };
  };

  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const p = toRemote(e);
    if (p) send({ t: "m", ...p });
  };

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    ime.current?.focus();
    const st = stage.current?.getBoundingClientRect();
    if (st) caret.current = { x: e.clientX - st.left, y: e.clientY - st.top };
    e.currentTarget.setPointerCapture(e.pointerId);
    const p = toRemote(e);
    if (p) send({ t: "m", ...p }, { t: "b", button: e.button, down: true });
  };

  const onPointerUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const p = toRemote(e);
    if (p) send({ t: "m", ...p }, { t: "b", button: e.button, down: false });
  };

  const onWheel = useCallback(
    (e: WheelEvent) => {
      e.preventDefault();
      // 픽셀 단위로 맞춘 뒤 부호를 바꿔 보냄 (위로 굴리면 양수). 한 칸 = 120
      const unit = e.deltaMode === 1 ? 50 : e.deltaMode === 2 ? 1900 : 1;
      const rest = wheelRest.current;
      rest.y += -e.deltaY * unit;
      rest.x += -e.deltaX * unit;
      const events: RdpInputEvent[] = [];
      for (const [axis, vertical] of [
        ["y", true],
        ["x", false],
      ] as const) {
        const units = Math.trunc(Math.max(-32768, Math.min(32767, rest[axis])));
        if (units !== 0) {
          events.push({ t: "w", vertical, delta: units });
          rest[axis] -= units;
        }
      }
      if (events.length) send(...events);
    },
    [send],
  );

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    c.addEventListener("wheel", onWheel, { passive: false });
    return () => c.removeEventListener("wheel", onWheel);
  }, [onWheel]);

  // ---------- 키보드 ----------

  const key = (code: number, down: boolean) => send({ t: "k", code, down });

  /** 원격 입력기의 한/영 전환 키를 보냄 (원격 입력기 모드) */
  const toggleHangul = () => {
    if (hangulKey === "lang1") {
      key(SC.lang1, true);
      key(SC.lang1, false);
    } else if (hangulKey === "shiftSpace") {
      key(SC.shift, true);
      key(SC.space, true);
      key(SC.space, false);
      key(SC.shift, false);
    } else {
      key(SC.rightAlt, true);
      key(SC.rightAlt, false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // 맥 입력기 모드: 조합 중인 키는 입력란에 맡기고, 조합이 끝나면 글자로 보냄.
    // 원격 입력기 모드: 입력란이 읽기 전용이라 맥 입력기가 끼어들지 않고 모든 키를 스캔 코드로 보냄
    if (!remoteIme && (e.nativeEvent.isComposing || e.keyCode === 229)) return;
    const sc = scancode(e.code);
    if (sc === null) return;
    e.preventDefault();
    if (IS_MAC && (e.code === "MetaLeft" || e.code === "MetaRight")) {
      meta.current = { down: true, ctrlSent: false, used: false, right: e.code === "MetaRight" };
      return;
    }
    if (IS_MAC && meta.current.down && !meta.current.ctrlSent) {
      key(SC.ctrl, true);
      meta.current.ctrlSent = true;
    }
    meta.current.used = meta.current.down;
    key(sc, true);
  };

  const onKeyUp = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const sc = scancode(e.code);
    if (sc === null) return;
    e.preventDefault();
    if (IS_MAC && (e.code === "MetaLeft" || e.code === "MetaRight")) {
      const m = meta.current;
      if (m.ctrlSent) key(SC.ctrl, false);
      else if (!m.used && m.right && remoteIme) {
        // 원격 입력기 모드에서 오른쪽 ⌘ 단독 = 원격 한/영 전환
        toggleHangul();
      } else if (!m.used) {
        key(SC.win, true);
        key(SC.win, false);
      }
      meta.current = { down: false, ctrlSent: false, used: false, right: false };
      return;
    }
    key(sc, false);
  };

  const sendText = (text: string) => {
    const clean = [...text].filter((c) => c >= " ").join("");
    for (let i = 0; i < clean.length; i += 200) send({ t: "u", text: clean.slice(i, i + 200) });
  };

  const onCompositionUpdate = (e: React.CompositionEvent<HTMLTextAreaElement>) => {
    if (!remoteIme) setComposing(e.data ?? "");
  };

  const onCompositionEnd = (e: React.CompositionEvent<HTMLTextAreaElement>) => {
    setComposing("");
    if (!remoteIme && e.data) sendText(e.data);
    e.currentTarget.value = "";
  };

  const onInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const native = e.nativeEvent as InputEvent;
    if (native.isComposing || remoteIme) return;
    // 조합 없이 들어온 글자 (이모지 선택기, 일부 IME의 확정 문자 등)
    if (native.inputType === "insertText" && native.data) sendText(native.data);
    e.currentTarget.value = "";
  };

  const ctrlAltDel = () => {
    key(SC.ctrl, true);
    key(SC.alt, true);
    key(SC.del, true);
    key(SC.del, false);
    key(SC.alt, false);
    key(SC.ctrl, false);
    ime.current?.focus();
  };

  const toggleClipboard = () => {
    const next = !clipboard;
    setClipboard(next);
    if (session.current) void api.rdpClipboard(session.current, next).catch(() => {});
    showToast(next ? "클립보드 공유를 켰습니다" : "클립보드 공유를 껐습니다");
    ime.current?.focus();
  };

  // ---------- 화면 배치 ----------

  const canvasStyle = (() => {
    if (!desktop) return { width: 0, height: 0 };
    const natural = { width: desktop.width / hidpiScale, height: desktop.height / hidpiScale };
    if (viewMode === "actual" || stageSize.width === 0) return natural;
    const s = Math.min(stageSize.width / natural.width, stageSize.height / natural.height);
    return { width: natural.width * s, height: natural.height * s };
  })();
  const percent = desktop && canvasStyle.width ? Math.round((canvasStyle.width / (desktop.width / hidpiScale)) * 100) : 100;

  const target = `${profile.username || "(사용자 없음)"}@${profile.host}:${profile.port}`;
  const ended = !connected && status.kind !== "connecting";

  return (
    <div ref={root} className="rdp-tab" hidden={!active}>
      <div
        ref={stage}
        className={`rdp-stage ${viewMode === "actual" ? "is-scroll" : ""}`}
        // 전체 화면에서 숨은 툴바는 포인터를 위쪽 가장자리로 옮기면 다시 보임 (캔버스 밖 여백 포함)
        onPointerMove={(e) => fullscreen && setToolbarHidden(e.clientY - e.currentTarget.getBoundingClientRect().top > 48)}
      >
        <canvas
          ref={canvas}
          className="rdp-canvas"
          hidden={!connected || !desktop}
          style={{ width: canvasStyle.width, height: canvasStyle.height }}
          aria-label={`${profile.name} 원격 데스크톱`}
          onPointerMove={onPointerMove}
          onPointerDown={onPointerDown}
          onPointerUp={onPointerUp}
          onContextMenu={(e) => e.preventDefault()}
        />
        <textarea
          ref={ime}
          className="rdp-ime"
          aria-label="원격 키보드 입력"
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          onKeyDown={onKeyDown}
          onKeyUp={onKeyUp}
          readOnly={remoteIme}
          onCompositionUpdate={onCompositionUpdate}
          onCompositionEnd={onCompositionEnd}
          onInput={onInput}
          onBlur={() => releaseAll()}
        />
        {composing && connected && (
          <div
            className="rdp-preedit"
            aria-hidden="true"
            style={
              caret.current
                ? {
                    left: Math.min(caret.current.x, stageSize.width - 80),
                    top: Math.max(0, Math.min(caret.current.y + 14, stageSize.height - 40)),
                  }
                : { left: "50%", bottom: 24, transform: "translateX(-50%)" }
            }
          >
            {composing}
          </div>
        )}
        {!(connected && desktop) && (
          <div className="session-placeholder rdp-placeholder">
            <MonitorIcon size={40} />
            <p className={status.kind === "error" ? "status-error" : "muted"}>
              {status.kind === "connecting"
                ? `${target}에 연결하는 중…`
                : status.kind === "connected"
                  ? "화면을 기다리는 중…"
                  : status.kind === "error" || status.kind === "closed"
                    ? status.message
                    : ""}
            </p>
            {ended && overlay.kind === "none" && (
              <button type="button" className="button button-secondary button-small" onClick={() => connect(null, false)}>
                다시 연결
              </button>
            )}
          </div>
        )}
      </div>

      {connected && (
        <div
          role="toolbar"
          aria-label="원격 데스크톱 제어"
          className={`rdp-toolbar ${toolbarHidden ? "is-hidden" : ""}`}
          onPointerEnter={() => setToolbarHidden(false)}
          // 툴바를 눌러도 키보드 포커스는 원격 입력란에 남김 (누른 뒤 바로 원격에 타이핑)
          onPointerDown={(e) => e.preventDefault()}
        >
          <span className="rdp-toolbar-name mono">{profile.name}</span>
          <button
            type="button"
            className="rdp-tool"
            title={viewMode === "fit" ? "원본 크기로 보기" : "창에 맞춰 보기"}
            onClick={() => setViewMode((m) => (m === "fit" ? "actual" : "fit"))}
          >
            {viewMode === "fit" ? `창에 맞춤 ${percent}%` : "원본 크기"}
          </button>
          <button
            type="button"
            className="rdp-tool rdp-tool-icon"
            aria-label={fullscreen ? "전체 화면 끝내기" : "전체 화면"}
            title={fullscreen ? "전체 화면 끝내기" : "전체 화면"}
            onClick={() => setFullscreen((f) => !f)}
          >
            {fullscreen ? <ExitFullscreenIcon /> : <FullscreenIcon />}
          </button>
          <button
            type="button"
            className={`rdp-tool rdp-tool-icon ${clipboard ? "is-on" : ""}`}
            aria-label="클립보드 공유"
            aria-pressed={clipboard}
            title={clipboard ? "클립보드 공유 켜짐 (텍스트)" : "클립보드 공유 꺼짐"}
            onClick={toggleClipboard}
          >
            <ClipboardIcon />
          </button>
          {remoteIme && (
            <button type="button" className="rdp-tool" title="원격 입력기 한/영 전환 (오른쪽 ⌘)" onClick={toggleHangul}>
              한/영
            </button>
          )}
          <button type="button" className="rdp-tool mono" onClick={ctrlAltDel}>
            Ctrl+Alt+Del
          </button>
          <button
            type="button"
            className={`rdp-tool ${diagnostics ? "is-on" : ""}`}
            aria-pressed={diagnostics}
            title="화면 전송 성능 진단"
            onClick={() => setDiagnostics((d) => !d)}
          >
            진단
          </button>
          <span className="rdp-toolbar-sep" />
          <button type="button" className="rdp-tool rdp-tool-danger" onClick={disconnect}>
            연결 끊기
          </button>
          {diagnostics && (
            <div role="status" className="rdp-diag mono">
              {diag}
            </div>
          )}
        </div>
      )}

      {active && toast && (
        <div role="status" className="rdp-toast">
          <CheckIcon />
          {toast}
        </div>
      )}

      {active && overlay.kind === "secret" && (
        <SecretPrompt
          kind="password"
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
      {active && overlay.kind === "certPrompt" && (
        <CertPrompt
          name={profile.name}
          host={overlay.event.host}
          port={overlay.event.port}
          fingerprint={overlay.event.fingerprint}
          problem={overlay.event.problem}
          onTrust={() => {
            setOverlay({ kind: "none" });
            if (session.current) void api.sshHostKeyRespond(session.current, true).catch(() => {});
          }}
          onCancel={() => {
            setOverlay({ kind: "none" });
            if (session.current) void api.sshHostKeyRespond(session.current, false).catch(() => {});
          }}
        />
      )}
      {active && overlay.kind === "certChanged" && (
        <CertChanged
          name={profile.name}
          host={overlay.event.host}
          port={overlay.event.port}
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
