import { useMemo } from "react";
import type { Profile } from "../lib/api.ts";
import { EditIcon, StarIcon } from "../components/Icons.tsx";
import { ProtocolBadge } from "./ProtocolBadge.tsx";

interface SidebarProps {
  profiles: Profile[];
  query: string;
  /** 열린 세션이 있는 프로필 (상태 점 표시) */
  connectedIds: Set<string>;
  activeProfileId: string | null;
  onOpen: (profile: Profile) => void;
  onEdit: (profile: Profile) => void;
}

const UNGROUPED = "그룹 없음";
const FAVORITES = "즐겨찾기";

/** 검색어가 이름, 호스트, 사용자, 그룹, 태그 중 하나에 포함되면 일치 */
export function matchesQuery(p: Profile, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [p.name, p.host, p.username, p.group ?? "", ...p.tags].some((v) =>
    v.toLowerCase().includes(q),
  );
}

export function Sidebar({ profiles, query, connectedIds, activeProfileId, onOpen, onEdit }: SidebarProps) {
  const groups = useMemo(() => {
    const filtered = profiles.filter((p) => matchesQuery(p, query));
    const map = new Map<string, Profile[]>();
    const favorites = filtered.filter((p) => p.favorite);
    if (favorites.length > 0) map.set(FAVORITES, favorites);
    for (const p of filtered) {
      const key = p.group ?? UNGROUPED;
      map.set(key, [...(map.get(key) ?? []), p]);
    }
    return [...map.entries()];
  }, [profiles, query]);

  if (profiles.length === 0) {
    return (
      <nav aria-label="연결 목록" className="sidebar">
        <p className="sidebar-empty">저장된 연결이 없습니다. 오른쪽 위의 “새 연결”로 추가하세요.</p>
      </nav>
    );
  }

  return (
    <nav aria-label="연결 목록" className="sidebar">
      {groups.length === 0 && <p className="sidebar-empty">“{query}”와 일치하는 연결이 없습니다.</p>}
      {groups.map(([name, items]) => (
        <section key={name} className="host-group">
          <h3 className="host-group-title">
            <span>{name}</span>
            <span className="mono">{items.length}</span>
          </h3>
          <ul>
            {items.map((p) => (
              <li key={p.id} className={`host-row ${p.id === activeProfileId ? "is-active" : ""}`}>
                <button
                  type="button"
                  className="host-open"
                  onClick={() => onOpen(p)}
                  title={`${p.name}에 연결`}
                >
                  <ProtocolBadge protocol={p.protocol} />
                  <span className="host-text">
                    <span className="host-name">
                      {p.name}
                      {p.favorite && (
                        <span className="host-star" aria-label="즐겨찾기">
                          <StarIcon filled />
                        </span>
                      )}
                    </span>
                    <span className="host-addr mono">
                      {p.username ? `${p.username}@` : ""}
                      {p.host}
                      {p.port !== 22 ? `:${p.port}` : ""}
                    </span>
                  </span>
                  <span
                    className={`dot ${connectedIds.has(p.id) ? "dot-on" : ""}`}
                    aria-label={connectedIds.has(p.id) ? "연결됨" : undefined}
                  />
                </button>
                <button
                  type="button"
                  className="icon-button host-edit"
                  aria-label={`${p.name} 편집`}
                  onClick={() => onEdit(p)}
                >
                  <EditIcon />
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </nav>
  );
}
