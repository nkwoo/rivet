import { useSyncExternalStore } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/api.ts";

/** 새 버전 정보 (`core/update.rs`의 `UpdateInfo`) */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  /** 게시 시각 (유닉스 초) */
  publishedAt: number | null;
  notes: string;
}

type UpdateEvent = { kind: "progress"; downloaded: number; total: number | null } | { kind: "installing" };

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "downloading"; info: UpdateInfo; downloaded: number; total: number | null }
  | { kind: "installing"; info: UpdateInfo }
  | { kind: "error"; message: string; info: UpdateInfo | null };

interface UpdateState {
  status: UpdateStatus;
  /** 대화상자 표시 여부. 자동 확인은 새 버전이 있어도 대화상자를 띄우지 않고 상단 막대에만 알립니다. */
  open: boolean;
  autoCheck: boolean;
}

const AUTO_CHECK_KEY = "rivet.updates.autoCheck";
/** 시작 직후 연결 작업과 겹치지 않게 조금 기다렸다가 확인 */
const AUTO_CHECK_DELAY_MS = 5000;

function readAutoCheck(): boolean {
  try {
    return localStorage.getItem(AUTO_CHECK_KEY) !== "off";
  } catch {
    return true;
  }
}

let state: UpdateState = { status: { kind: "idle" }, open: false, autoCheck: readAutoCheck() };
const listeners = new Set<() => void>();

function update(next: Partial<UpdateState>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useUpdates(): UpdateState {
  return useSyncExternalStore(subscribe, () => state);
}

function busy(status: UpdateStatus) {
  return status.kind === "checking" || status.kind === "downloading" || status.kind === "installing";
}

async function check() {
  if (busy(state.status)) return;
  update({ status: { kind: "checking" } });
  try {
    const info = await invoke<UpdateInfo | null>("update_check");
    update({ status: info ? { kind: "available", info } : { kind: "upToDate" } });
  } catch (e) {
    update({ status: { kind: "error", message: errorMessage(e), info: null } });
  }
}

/** 메뉴·상태 표시줄에서 직접 확인: 결과를 대화상자로 보여줌 */
export function checkNow() {
  update({ open: true });
  if (state.status.kind !== "available") void check();
}

/** 상단 막대 알림에서 대화상자 열기 */
export function openUpdates() {
  update({ open: true });
}

export function closeUpdates() {
  if (state.status.kind === "downloading" || state.status.kind === "installing") return;
  // 새 버전 알림은 남기고, 확인 결과(최신/오류)는 닫으면 지움
  const keep = state.status.kind === "available";
  update({ open: false, status: keep ? state.status : { kind: "idle" } });
}

/** 시작할 때 한 번 조용히 확인. 실패해도 알리지 않습니다. */
export function scheduleAutoCheck(): () => void {
  if (!state.autoCheck) return () => {};
  const timer = window.setTimeout(() => {
    void check().then(() => {
      if (state.status.kind === "error" && !state.open) update({ status: { kind: "idle" } });
    });
  }, AUTO_CHECK_DELAY_MS);
  return () => window.clearTimeout(timer);
}

export function setAutoCheck(on: boolean) {
  try {
    if (on) localStorage.removeItem(AUTO_CHECK_KEY);
    else localStorage.setItem(AUTO_CHECK_KEY, "off");
  } catch {
    // 저장하지 못해도 이번 실행에서는 적용
  }
  update({ autoCheck: on });
}

/** 내려받아 설치한 뒤 앱을 다시 시작합니다. 성공하면 돌아오지 않습니다. */
export async function install() {
  const current = state.status;
  let info: UpdateInfo;
  if (current.kind === "available") info = current.info;
  else if (current.kind === "error" && current.info) info = current.info;
  else return;
  update({ status: { kind: "downloading", info, downloaded: 0, total: null } });
  try {
    // 설치할 업데이트는 백엔드가 마지막 확인 결과로 정하고, 설치를 시작하면 비웁니다. 실패 후 다시 시도하면 확인부터
    if (current.kind === "error") {
      const again = await invoke<UpdateInfo | null>("update_check");
      if (!again) {
        update({ status: { kind: "upToDate" } });
        return;
      }
      info = again;
    }
    const events = new Channel<UpdateEvent>();
    events.onmessage = (e) => {
      if (e.kind === "progress")
        update({ status: { kind: "downloading", info, downloaded: e.downloaded, total: e.total } });
      else update({ status: { kind: "installing", info } });
    };
    await invoke("update_install", { events });
  } catch (e) {
    update({ status: { kind: "error", message: errorMessage(e), info } });
  }
}
