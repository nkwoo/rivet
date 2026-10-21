import { useRef, type CSSProperties, type PointerEvent, type ReactNode } from "react";
import { computeLayout, findPane, type DropSide, type PaneNode, type Rect } from "./layout.ts";

interface SplitViewProps {
  layout: PaneNode;
  visible: boolean;
  /** 창이 2개 이상일 때 포커스된 창 (테두리 강조) */
  focusedId: string;
  /** 사이드바에서 연결을 끌어 오는 중일 때 놓일 자리 */
  dropTarget: { paneId: string; side: DropSide } | null;
  renderPane: (pane: Extract<PaneNode, { kind: "pane" }>) => ReactNode;
  onRatio: (splitId: string, ratio: number) => void;
}

const pct = (v: number) => `${v * 100}%`;

function rectStyle(r: Rect): CSSProperties {
  return { left: pct(r.x), top: pct(r.y), width: pct(r.w), height: pct(r.h) };
}

/** 놓일 자리 표시: 대상 창의 절반 */
function dropStyle(r: Rect, side: DropSide): CSSProperties {
  switch (side) {
    case "left":
      return rectStyle({ ...r, w: r.w / 2 });
    case "right":
      return rectStyle({ ...r, x: r.x + r.w / 2, w: r.w / 2 });
    case "top":
      return rectStyle({ ...r, h: r.h / 2 });
    case "bottom":
      return rectStyle({ ...r, y: r.y + r.h / 2, h: r.h / 2 });
  }
}

/**
 * 탭 하나의 창들을 배치합니다. 창은 트리 구조와 관계없이 모두 이 요소의 자식으로 두고
 * 위치만 바꾸므로, 나누거나 닫아도 남은 창(터미널과 SSH 세션)은 다시 만들어지지 않습니다.
 */
export function SplitView({ layout, visible, focusedId, dropTarget, renderPane, onRatio }: SplitViewProps) {
  const root = useRef<HTMLDivElement>(null);
  const { panes, dividers } = computeLayout(layout);
  const split = panes.size > 1;
  // 창 순서가 바뀌어도 DOM 순서는 유지되도록 ID로 정렬 (React가 요소를 옮기면 xterm 캔버스가 다시 그려짐)
  const paneNodes = [...panes.keys()].sort().map((id) => {
    const node = findPane(layout, id);
    return node ? { node, rect: panes.get(id) as Rect } : null;
  });

  const startResize = (splitId: string, direction: "row" | "column", area: Rect) => (e: PointerEvent) => {
    const el = root.current;
    if (!el || e.button !== 0) return;
    e.preventDefault();
    const box = el.getBoundingClientRect();
    document.body.classList.add(direction === "row" ? "is-resizing-col" : "is-resizing-row");
    const move = (ev: globalThis.PointerEvent) => {
      const ratio =
        direction === "row"
          ? ((ev.clientX - box.left) / box.width - area.x) / area.w
          : ((ev.clientY - box.top) / box.height - area.y) / area.h;
      onRatio(splitId, ratio);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("is-resizing-col", "is-resizing-row");
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const dropRect = dropTarget ? panes.get(dropTarget.paneId) : undefined;

  return (
    <div ref={root} className={`split-view ${split ? "is-split" : ""}`} hidden={!visible}>
      {paneNodes.map(
        (p) =>
          p && (
            <div
              key={p.node.id}
              className={`split-pane ${split && p.node.id === focusedId ? "is-focused" : ""}`}
              style={rectStyle(p.rect)}
              data-pane-id={p.node.id}
            >
              {renderPane(p.node)}
            </div>
          ),
      )}
      {dividers.map((d) => {
        const style: CSSProperties =
          d.direction === "row"
            ? { left: pct(d.rect.x + d.rect.w * d.at), top: pct(d.rect.y), height: pct(d.rect.h) }
            : { top: pct(d.rect.y + d.rect.h * d.at), left: pct(d.rect.x), width: pct(d.rect.w) };
        return (
          <div
            key={d.splitId}
            role="separator"
            aria-orientation={d.direction === "row" ? "vertical" : "horizontal"}
            aria-valuenow={Math.round(d.at * 100)}
            aria-valuemin={10}
            aria-valuemax={90}
            className={`split-divider split-divider-${d.direction}`}
            style={style}
            onPointerDown={startResize(d.splitId, d.direction, d.rect)}
            onDoubleClick={() => onRatio(d.splitId, 0.5)}
            title="끌어서 크기 조절, 두 번 눌러 반씩 나누기"
          />
        );
      })}
      {dropTarget && dropRect && (
        <div className="split-drop" style={dropStyle(dropRect, dropTarget.side)} aria-hidden="true" />
      )}
    </div>
  );
}
