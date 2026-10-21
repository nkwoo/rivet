import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import type { FsEntry, Listing } from "../lib/api.ts";
import { ArrowUpIcon, FileIcon, FolderIcon, LinkIcon, RefreshIcon } from "../components/Icons.tsx";
import { formatDate, formatPermissions, formatSize } from "./transfers.ts";
import { useI18n } from "../i18n/index.ts";

export type Side = "local" | "remote";

export interface PaneState {
  listing: Listing | null;
  loading: boolean;
  error: string | null;
  selected: Set<string>;
}

export const emptyPane = (): PaneState => ({ listing: null, loading: true, error: null, selected: new Set() });

interface FilePaneProps {
  side: Side;
  title: string;
  label: string;
  /** 경로 앞에 붙여 보여줄 접두사 (예: `deploy@web-01:`) */
  pathPrefix?: string;
  pane: PaneState;
  /** 다른 창에서 끌어온 항목이 이 창 위에 있음 */
  dropActive: boolean;
  actions?: ReactNode;
  onSelect: (selected: Set<string>) => void;
  onNavigate: (path: string) => void;
  onRefresh: () => void;
  /** 파일을 두 번 누르면 반대쪽으로 전송 */
  onActivateFile: (entry: FsEntry) => void;
  onDragStart: (e: PointerEvent, names: string[]) => void;
}

function sortEntries(entries: FsEntry[]): FsEntry[] {
  const rank = (e: FsEntry) => (e.kind === "dir" ? 0 : 1);
  return [...entries].sort(
    (a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }),
  );
}

function EntryIcon({ entry }: { entry: FsEntry }) {
  if (entry.kind === "dir") return <FolderIcon />;
  if (entry.kind === "symlink") return <LinkIcon />;
  return <FileIcon />;
}

/** 로컬/원격 공통 파일 목록. 선택은 클릭, ⌘/Ctrl+클릭(토글), Shift+클릭(범위), 방향키로 합니다. */
export function FilePane({
  side,
  title,
  label,
  pathPrefix,
  pane,
  dropActive,
  actions,
  onSelect,
  onNavigate,
  onRefresh,
  onActivateFile,
  onDragStart,
}: FilePaneProps) {
  const { m } = useI18n();
  const { listing, loading, error, selected } = pane;
  const body = useRef<HTMLDivElement>(null);
  const entries = useMemo(() => sortEntries(listing?.entries ?? []), [listing]);
  const anchor = useRef<string | null>(null);
  const [pathText, setPathText] = useState(listing?.path ?? "");

  useEffect(() => setPathText(listing?.path ?? ""), [listing?.path]);

  const open = (entry: FsEntry) => {
    if (!listing) return;
    if (entry.kind === "dir" || entry.kind === "symlink") {
      const sep = listing.path.includes("\\") && !listing.path.includes("/") ? "\\" : "/";
      onNavigate(listing.path.endsWith(sep) ? listing.path + entry.name : listing.path + sep + entry.name);
    } else if (entry.kind === "file") {
      onActivateFile(entry);
    }
  };

  const click = (e: React.MouseEvent, entry: FsEntry) => {
    if (e.shiftKey && anchor.current) {
      const a = entries.findIndex((x) => x.name === anchor.current);
      const b = entries.findIndex((x) => x.name === entry.name);
      const [from, to] = a < b ? [a, b] : [b, a];
      onSelect(new Set(entries.slice(from, to + 1).map((x) => x.name)));
      return;
    }
    anchor.current = entry.name;
    if (e.metaKey || e.ctrlKey) {
      const next = new Set(selected);
      if (next.has(entry.name)) next.delete(entry.name);
      else next.add(entry.name);
      onSelect(next);
    } else {
      onSelect(new Set([entry.name]));
    }
  };

  const pointerDown = (e: PointerEvent, entry: FsEntry) => {
    if (e.button !== 0 || e.shiftKey || e.metaKey || e.ctrlKey) return;
    const names = selected.has(entry.name) ? [...selected] : [entry.name];
    onDragStart(e, names);
  };

  const keyDown = (e: KeyboardEvent) => {
    if (!listing) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      onSelect(new Set(entries.map((x) => x.name)));
      return;
    }
    if (e.key === "Backspace" && listing.parent) {
      e.preventDefault();
      onNavigate(listing.parent);
      return;
    }
    const current = entries.findIndex((x) => x.name === anchor.current);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.max(0, Math.min(entries.length - 1, current + (e.key === "ArrowDown" ? 1 : -1)));
      const entry = entries[next];
      if (!entry) return;
      anchor.current = entry.name;
      onSelect(new Set([entry.name]));
      document.getElementById(rowId(side, entry.name))?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && current >= 0) {
      e.preventDefault();
      const entry = entries[current];
      if (entry) open(entry);
    }
  };

  const selectedSize = entries.filter((x) => selected.has(x.name)).reduce((sum, x) => sum + x.size, 0);
  const totalSize = entries.reduce((sum, x) => sum + x.size, 0);
  const footer =
    selected.size > 0
      ? m.files.selectedSummary(entries.length, selected.size, formatSize(selectedSize))
      : m.files.summary(entries.length, formatSize(totalSize));

  return (
    <section aria-label={label} className={`pane ${dropActive ? "is-drop" : ""}`} data-pane={side}>
      <div className="pane-head">
        <span className="pane-side">{title}</span>
        <form
          className="pane-path"
          onSubmit={(e) => {
            e.preventDefault();
            if (pathText.trim()) onNavigate(pathText.trim());
          }}
        >
          {pathPrefix && <span className="pane-prefix mono">{pathPrefix}</span>}
          <input
            className="mono"
            aria-label={m.files.path(title)}
            value={pathText}
            spellCheck={false}
            onChange={(e) => setPathText(e.target.value)}
          />
        </form>
        {actions}
        <button
          type="button"
          className="icon-button icon-button-boxed icon-button-sm"
          aria-label={m.files.refresh}
          title={m.files.refresh}
          onClick={onRefresh}
        >
          <RefreshIcon />
        </button>
        <button
          type="button"
          className="icon-button icon-button-boxed icon-button-sm"
          aria-label={m.files.up}
          title={m.files.upTitle}
          disabled={!listing?.parent}
          onClick={() => listing?.parent && onNavigate(listing.parent)}
        >
          <ArrowUpIcon />
        </button>
      </div>

      <div ref={body} className="pane-body" tabIndex={0} onKeyDown={keyDown} aria-busy={loading}>
        {error ? (
          <p className="pane-message pane-error">{error}</p>
        ) : !listing ? (
          <p className="pane-message muted">{m.common.loading}</p>
        ) : (
          <table className="file-table">
            <thead>
              <tr>
                <th scope="col">{m.common.name}</th>
                <th scope="col" className="num">
                  {m.files.size}
                </th>
                <th scope="col">{m.files.modified}</th>
                <th scope="col">{m.files.permissions}</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => {
                const isSelected = selected.has(entry.name);
                return (
                  <tr
                    key={entry.name}
                    id={rowId(side, entry.name)}
                    aria-selected={isSelected}
                    className={isSelected ? "is-selected" : undefined}
                    onClick={(e) => click(e, entry)}
                    // 끌기 중 텍스트가 선택되지 않도록 기본 동작을 막고, 키보드 이동을 위해 직접 포커스
                    onMouseDown={(e) => {
                      e.preventDefault();
                      body.current?.focus({ preventScroll: true });
                    }}
                    onDoubleClick={() => open(entry)}
                    onPointerDown={(e) => pointerDown(e, entry)}
                  >
                    <td>
                      <span className={`file-name file-${entry.kind}`}>
                        <EntryIcon entry={entry} />
                        <span className="file-label">{entry.name}</span>
                      </span>
                    </td>
                    <td className="num mono">{entry.kind === "file" ? formatSize(entry.size) : "—"}</td>
                    <td className="mono">{formatDate(entry.modified)}</td>
                    <td className="mono muted">{formatPermissions(entry.kind, entry.permissions)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {listing && entries.length === 0 && !error && <p className="pane-message muted">{m.files.emptyFolder}</p>}
      </div>

      <div className="pane-foot">
        {footer}
        {listing?.truncated && <span className="pane-warn">{m.files.truncated}</span>}
      </div>
    </section>
  );
}

function rowId(side: Side, name: string) {
  return `row-${side}-${encodeURIComponent(name)}`;
}
