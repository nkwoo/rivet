import { GlobeIcon } from "./Icons.tsx";
import { setLocalePreference, useI18n, type LocalePreference } from "../i18n/index.ts";

/** 상단 막대의 언어 선택. 고른 값은 이 기기에만 저장됩니다. */
export function LanguageSelect() {
  const { m, preference } = useI18n();
  return (
    <label className="lang-select" title={m.language.label}>
      <GlobeIcon />
      <span className="visually-hidden">{m.language.label}</span>
      <select
        value={preference}
        onChange={(e) => setLocalePreference(e.target.value as LocalePreference)}
      >
        <option value="system">{m.language.system}</option>
        <option value="ko">{m.language.ko}</option>
        <option value="en">{m.language.en}</option>
      </select>
    </label>
  );
}
