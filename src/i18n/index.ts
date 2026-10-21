import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ko, type Messages } from "./ko.ts";
import { en } from "./en.ts";

export type { Messages };
export type Locale = "ko" | "en";
/** 사용자가 고른 언어. `system`은 OS(웹뷰) 언어를 따름 */
export type LocalePreference = Locale | "system";

const DICTIONARIES: Record<Locale, Messages> = { ko, en };
const STORAGE_KEY = "rivet.locale";

/** OS 언어가 한국어면 한국어, 그 밖에는 영어 */
export function systemLocale(): Locale {
  const langs = navigator.languages.length > 0 ? navigator.languages : [navigator.language];
  return langs[0]?.toLowerCase().startsWith("ko") ? "ko" : "en";
}

function readPreference(): LocalePreference {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === "ko" || v === "en" ? v : "system";
  } catch {
    return "system";
  }
}

let preference: LocalePreference = readPreference();
let locale: Locale = preference === "system" ? systemLocale() : preference;
const listeners = new Set<() => void>();

/** 웹뷰 밖(macOS 메뉴)과 문서 언어를 현재 언어에 맞춤 */
function applyLocale() {
  document.documentElement.lang = locale;
  void invoke("app_set_menu_locale", { locale }).catch(() => {});
}

function resolve() {
  const resolved = preference === "system" ? systemLocale() : preference;
  if (resolved !== locale) {
    locale = resolved;
    applyLocale();
  }
  listeners.forEach((l) => l());
}

export function setLocalePreference(next: LocalePreference) {
  preference = next;
  try {
    if (next === "system") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // 저장하지 못해도 이번 실행에서는 적용
  }
  resolve();
}

/** 컴포넌트 밖(전송 큐 문구 등)에서 쓰는 현재 언어의 문구 */
export function messages(): Messages {
  return DICTIONARIES[locale];
}

export function currentLocale(): Locale {
  return locale;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** 언어 설정이 바뀌면 다시 그립니다 */
export function useI18n(): { m: Messages; locale: Locale; preference: LocalePreference } {
  useSyncExternalStore(subscribe, () => `${preference}:${locale}`);
  return { m: DICTIONARIES[locale], locale, preference };
}

applyLocale();
// "시스템 설정"을 고른 상태에서 OS 언어가 바뀌면 따라감
window.addEventListener("languagechange", () => preference === "system" && resolve());
