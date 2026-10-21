import { useMemo, useRef, useState, type PointerEvent } from "react";
import type { Profile } from "../lib/api.ts";
import { ConfirmDialog } from "../components/ConfirmDialog.tsx";
import { DownloadIcon, EditIcon, SelectIcon, StarIcon } from "../components/Icons.tsx";
import { ProtocolBadge } from "./ProtocolBadge.tsx";

interface SidebarProps {
  profiles: Profile[];
  query: string;
  /** 열린 세션이 있는 프로필 (상태 점 표시) */
  connectedIds: Set<string>;
  /** 탭이 열려 있는 프로필 (삭제 확인에서 함께 닫힐 세션 수 표시) */
  openIds: Set<string>;
  activeProfileId: string | null;
  onOpen: (profile: Profile) => void;
  onEdit: (profile: Profile) => void;
  onImport: () => void;
  /** 여러 연결 삭제. 열린 세션도 함께 닫습니다. */
  onDeleteMany: (profiles: Profile[]) => Promise<void>;
  /** 끌어서 터미널 창을 나눌 수 있는 연결인지 */
  canDrag: (profile: Profile) => boolean;
  /** 연결을 끌기 시작함 (손을 뗄 때까지의 처리는 받는 쪽에서) */
  onDragStart: (profile: Profile, x: number, y: number) => void;
}

/** 이만큼 움직여야 클릭이 아니라 끌기로 봄 (px) */
const DRAG_THRESHOLD = 6;

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

export function Sidebar({
  profiles,
  query,
  connectedIds,
  openIds,
  activeProfileId,
  onOpen,
  onEdit,
  onImport,
  onDeleteMany,
  canDrag,
  onDragStart,
}: SidebarProps) {
  const [selecting, setSelecting] = useState(false);
  /** 끌기가 끝난 직후 같은 버튼에서 생기는 클릭을 무시 */
  const dragged = useRef(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  const filtered = useMemo(() => profiles.filter((p) => matchesQuery(p, query)), [profiles, query]);
  const groups = useMemo(() => {
    const map = new Map<string, Profile[]>();
    const favorites = filtered.filter((p) => p.favorite);
    if (favorites.length > 0) map.set(FAVORITES, favorites);
    for (const p of filtered) {
      const key = p.group ?? UNGROUPED;
      map.set(key, [...(map.get(key) ?? []), p]);
    }
    return [...map.entries()];
  }, [filtered]);

  // 삭제되었거나 목록에 없는 항목은 선택에서 뺌
  const chosen = profiles.filter((p) => selected.has(p.id));
  const allVisibleSelected = filtered.length > 0 && filtered.every((p) => selected.has(p.id));

  const setMany = (items: Profile[], on: boolean) =>
    setSelected((s) => {
      const next = new Set(s);
      for (const p of items) {
        if (on) next.add(p.id);
        else next.delete(p.id);
      }
      return next;
    });

  const stopSelecting = () => {
    setSelecting(false);
    setSelected(new Set());
  };

  const remove = async () => {
    setConfirming(false);
    setBusy(true);
    try {
      await onDeleteMany(chosen);
      stopSelecting();
    } finally {
      setBusy(false);
    }
  };

  if (profiles.length === 0) {
    return (
      <nav aria-label="연결 목록" className="sidebar">
        <p className="sidebar-empty">저장된 연결이 없습니다. 오른쪽 위의 “새 연결”로 추가하세요.</p>
        <ImportButton onImport={onImport} />
      </nav>
    );
  }

  const openCount = chosen.filter((p) => openIds.has(p.id)).length;

  const pointerDown = (e: PointerEvent, p: Profile) => {
    if (selecting || e.button !== 0 || !canDrag(p)) return;
    const startX = e.clientX;
    const startY = e.clientY;
    dragged.current = false;
    const move = (ev: globalThis.PointerEvent) => {
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD) return;
      dragged.current = true;
      done();
      onDragStart(p, ev.clientX, ev.clientY);
    };
    const done = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", done);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", done);
  };

  return (
    <nav aria-label="연결 목록" className="sidebar">
      {selecting && (
        <div className="sidebar-tools">
          <label className="checkbox sidebar-select-all">
            <input
              type="checkbox"
              checked={allVisibleSelected}
              onChange={() => setMany(filtered, !allVisibleSelected)}
            />
            <span>전체 선택</span>
          </label>
          <button
            type="button"
            className="button button-danger button-small"
            disabled={chosen.length === 0 || busy}
            onClick={() => setConfirming(true)}
          >
            {chosen.length}개 삭제
          </button>
          <button type="button" className="button button-ghost button-small" onClick={stopSelecting}>
            취소
          </button>
        </div>
      )}
      {groups.length === 0 && <p className="sidebar-empty">“{query}”와 일치하는 연결이 없습니다.</p>}
      {groups.map(([name, items]) => {
        const groupSelected = items.every((p) => selected.has(p.id));
        return (
          <section key={name} className="host-group">
            <h3 className="host-group-title">
              {selecting ? (
                <label className="host-group-check">
                  <input
                    type="checkbox"
                    aria-label={`${name} 그룹 전체 선택`}
                    checked={groupSelected}
                    onChange={() => setMany(items, !groupSelected)}
                  />
                  <span>{name}</span>
                </label>
              ) : (
                <span>{name}</span>
              )}
              <span className="mono">{items.length}</span>
            </h3>
            <ul>
              {items.map((p) => {
                const isChecked = selected.has(p.id);
                return (
                  <li
                    key={p.id}
                    className={`host-row ${p.id === activeProfileId && !selecting ? "is-active" : ""} ${
                      selecting && isChecked ? "is-selected" : ""
                    }`}
                  >
                    <button
                      type="button"
                      className="host-open"
                      role={selecting ? "checkbox" : undefined}
                      aria-checked={selecting ? isChecked : undefined}
                      aria-label={selecting ? `${p.name} 선택` : undefined}
                      onPointerDown={(e) => pointerDown(e, p)}
                      onClick={() => {
                        if (dragged.current) {
                          dragged.current = false;
                          return;
                        }
                        if (selecting) setMany([p], !isChecked);
                        else onOpen(p);
                      }}
                      title={
                        selecting
                          ? `${p.name} 선택`
                          : canDrag(p)
                            ? `${p.name}에 연결 (터미널 위로 끌어 놓으면 창을 나눠 엽니다)`
                            : `${p.name}에 연결`
                      }
                    >
                      {selecting && (
                        <input
                          type="checkbox"
                          className="host-check"
                          checked={isChecked}
                          readOnly
                          tabIndex={-1}
                          aria-hidden="true"
                        />
                      )}
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
                    {!selecting && (
                      <>
                        <button
                          type="button"
                          className="icon-button host-select"
                          aria-label={`${p.name} 선택해서 삭제`}
                          title="선택해서 삭제"
                          onClick={() => {
                            setSelected(new Set([p.id]));
                            setSelecting(true);
                          }}
                        >
                          <SelectIcon />
                        </button>
                        <button
                          type="button"
                          className="icon-button host-edit"
                          aria-label={`${p.name} 편집`}
                          title="편집"
                          onClick={() => onEdit(p)}
                        >
                          <EditIcon />
                        </button>
                      </>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
      <ImportButton onImport={onImport} />
      {confirming && (
        <ConfirmDialog
          title={`연결 ${chosen.length}개를 삭제할까요?`}
          confirmLabel={`${chosen.length}개 삭제`}
          onConfirm={() => void remove()}
          onCancel={() => setConfirming(false)}
        >
          <p className="dialog-text">
            {chosen
              .slice(0, 8)
              .map((p) => p.name)
              .join(", ")}
            {chosen.length > 8 ? ` 외 ${chosen.length - 8}개` : ""}
          </p>
          <p className="dialog-text muted">
            저장된 비밀번호(키체인)도 함께 지웁니다.
            {openCount > 0 && ` 열려 있는 세션 ${openCount}개는 연결을 끊고 탭을 닫습니다.`}
          </p>
        </ConfirmDialog>
      )}
    </nav>
  );
}

function ImportButton({ onImport }: { onImport: () => void }) {
  return (
    <div className="sidebar-footer">
      <button type="button" className="button button-secondary button-small sidebar-import" onClick={onImport}>
        <DownloadIcon />
        ~/.ssh/config 가져오기
      </button>
    </div>
  );
}
