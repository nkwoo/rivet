import { useSyncExternalStore } from "react";
import { Channel } from "@tauri-apps/api/core";
import { api, type JobInfo, type TransferEvent } from "../lib/api.ts";

/** 전송 큐 전역 상태. 백엔드 이벤트 채널은 앱에서 한 번만 등록합니다. */
interface Snapshot {
  jobs: JobInfo[];
  /** 작업별 최근 전송 속도 (바이트/초) */
  rates: Record<string, number>;
}

let state: Snapshot = { jobs: [], rates: {} };
const listeners = new Set<() => void>();
let subscribed = false;

function set(next: Snapshot) {
  state = next;
  listeners.forEach((l) => l());
}

function apply(event: TransferEvent) {
  switch (event.type) {
    case "job": {
      const { job } = event;
      const index = state.jobs.findIndex((j) => j.id === job.id);
      const jobs = index === -1 ? [...state.jobs, job] : state.jobs.map((j, i) => (i === index ? job : j));
      const rates = job.state.kind === "running" ? state.rates : withoutKey(state.rates, job.id);
      set({ jobs, rates });
      break;
    }
    case "progress":
      set({
        jobs: state.jobs.map((j) => (j.id === event.id ? { ...j, transferred: event.transferred } : j)),
        rates: event.bytesPerSec > 0 ? { ...state.rates, [event.id]: event.bytesPerSec } : state.rates,
      });
      break;
    case "removed": {
      const removed = new Set(event.ids);
      set({ jobs: state.jobs.filter((j) => !removed.has(j.id)), rates: state.rates });
      break;
    }
  }
}

function withoutKey(rates: Record<string, number>, key: string) {
  if (!(key in rates)) return rates;
  const { [key]: _removed, ...rest } = rates;
  return rest;
}

/** 백엔드 전송 이벤트 구독 (중복 호출 무시) */
export function ensureTransferSubscription() {
  if (subscribed) return;
  subscribed = true;
  const channel = new Channel<TransferEvent>();
  channel.onmessage = apply;
  api
    .transferSubscribe(channel)
    .then((jobs) => set({ jobs, rates: {} }))
    .catch(() => {
      subscribed = false;
    });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useTransfers(): Snapshot {
  return useSyncExternalStore(subscribe, () => state);
}

export function isFinished(job: JobInfo) {
  const k = job.state.kind;
  return k === "done" || k === "skipped" || k === "failed" || k === "cancelled";
}

// ---------- 표시용 형식 ----------

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

export function formatDate(secs: number | null): string {
  if (secs === null) return "—";
  const d = new Date(secs * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatPermissions(kind: string, mode: number | null): string {
  if (mode === null) return "—";
  const type = kind === "dir" ? "d" : kind === "symlink" ? "l" : "-";
  const bits = "rwxrwxrwx";
  let out = type;
  for (let i = 0; i < 9; i++) out += mode & (1 << (8 - i)) ? bits[i] : "-";
  return out;
}

const ERROR_TEXT: Record<string, string> = {
  pathBlocked: "경로 검사로 차단됨",
  fsNotFound: "파일이나 폴더가 없습니다",
  permissionDenied: "권한이 없습니다",
  alreadyExists: "이미 있습니다",
  notADirectory: "폴더가 아닙니다",
  tooManyEntries: "항목이 너무 많습니다",
  symlink: "심볼릭 링크는 따라가지 않습니다",
  timeout: "응답 시간이 초과되었습니다",
  connectionLost: "연결이 끊어졌습니다",
  dataConnection: "서버의 데이터 포트에 연결할 수 없습니다 (FTP 패시브 모드). 방화벽이나 포트 포워딩을 확인하세요",
};

/** 백엔드 에러 코드를 한국어 문구로 바꿉니다. 모르는 코드는 서버 메시지를 그대로 씁니다. */
export function fsErrorText(code: string, message: string): string {
  return ERROR_TEXT[code] ?? message;
}

export const SKIP_TEXT: Record<string, string> = {
  exists: "건너뜀 · 이미 있음",
  notNewer: "건너뜀 · 더 최신 아님",
  symlink: "건너뜀 · 심볼릭 링크",
};
