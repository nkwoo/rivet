import { CloseIcon, PlusIcon } from "../components/Icons.tsx";
import type { Profile } from "../lib/api.ts";

export interface TabInfo {
  id: string;
  profile: Profile;
}

interface TabBarProps {
  tabs: TabInfo[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onNew: () => void;
}

export function TabBar({ tabs, activeId, onSelect, onClose, onNew }: TabBarProps) {
  return (
    <div role="tablist" aria-label="열린 세션" className="tabbar">
      {tabs.map((t) => {
        const selected = t.id === activeId;
        return (
          <div key={t.id} className={`tab ${selected ? "is-active" : ""}`}>
            <button
              type="button"
              role="tab"
              aria-selected={selected}
              className="tab-label"
              onClick={() => onSelect(t.id)}
              onAuxClick={(e) => e.button === 1 && onClose(t.id)}
            >
              <span className="tab-proto mono">{t.profile.protocol.toUpperCase()}</span>
              {t.profile.name}
            </button>
            <button
              type="button"
              className="tab-close"
              aria-label={`${t.profile.name} 탭 닫기`}
              onClick={() => onClose(t.id)}
            >
              <CloseIcon />
            </button>
          </div>
        );
      })}
      <button type="button" className="tab-new" aria-label="새 연결" onClick={onNew}>
        <PlusIcon />
      </button>
    </div>
  );
}
