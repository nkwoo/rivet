import { CloseIcon, PlusIcon } from "../components/Icons.tsx";
import { findPane, paneList, type PaneNode } from "./layout.ts";

export interface TabInfo {
  id: string;
  /** 탭 안의 창 배치. 분할하지 않은 탭은 창 하나 */
  layout: PaneNode;
  /** 포커스된 창 ID */
  focused: string;
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
        const panes = paneList(t.layout);
        const profile = (findPane(t.layout, t.focused) ?? panes[0])?.profile;
        if (!profile) return null;
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
              <span className="tab-proto mono">{profile.protocol.toUpperCase()}</span>
              {profile.name}
              {panes.length > 1 && (
                <span className="tab-panes mono" title={panes.map((p) => p.profile.name).join(", ")}>
                  {panes.length}
                </span>
              )}
            </button>
            <button
              type="button"
              className="tab-close"
              aria-label={`${profile.name} 탭 닫기`}
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
