import type { Profile } from "../lib/api.ts";

/**
 * 탭 하나의 분할 창 배치. 잎은 터미널 창(SSH 세션 하나), 가지는 두 칸으로 나눈 영역입니다.
 * 창을 나누거나 닫아도 세션이 다시 만들어지지 않도록, 화면에는 창을 모두 같은 층에 두고
 * 이 트리로 계산한 위치(비율)만 바꿉니다.
 */
export type PaneNode =
  | { kind: "pane"; id: string; profile: Profile }
  | {
      kind: "split";
      id: string;
      /** row: 좌우로 나눔, column: 상하로 나눔 */
      direction: SplitDirection;
      /** 첫 번째 칸이 차지하는 비율 (0~1) */
      ratio: number;
      first: PaneNode;
      second: PaneNode;
    };

export type SplitDirection = "row" | "column";
export type DropSide = "left" | "right" | "top" | "bottom";
export type FocusDirection = DropSide;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 경계선: `rect`는 나뉜 영역 전체, `at`은 경계 위치(0~1, 영역 기준) */
export interface Divider {
  splitId: string;
  direction: SplitDirection;
  rect: Rect;
  at: number;
}

/** 한 칸이 가질 수 있는 최소 비율 */
export const MIN_RATIO = 0.1;

export function clampRatio(ratio: number): number {
  return Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, ratio));
}

export function paneList(node: PaneNode): Extract<PaneNode, { kind: "pane" }>[] {
  return node.kind === "pane" ? [node] : [...paneList(node.first), ...paneList(node.second)];
}

export function findPane(node: PaneNode, id: string): Extract<PaneNode, { kind: "pane" }> | null {
  return paneList(node).find((p) => p.id === id) ?? null;
}

/**
 * `targetId` 창을 `side` 쪽으로 나누고 그 자리에 `pane`을 넣습니다.
 * 대상 창을 찾지 못하면 트리를 그대로 돌려줍니다.
 */
export function splitPane(
  node: PaneNode,
  targetId: string,
  side: DropSide,
  pane: Extract<PaneNode, { kind: "pane" }>,
  splitId: string,
): PaneNode {
  if (node.kind === "pane") {
    if (node.id !== targetId) return node;
    const direction: SplitDirection = side === "left" || side === "right" ? "row" : "column";
    const before = side === "left" || side === "top";
    return {
      kind: "split",
      id: splitId,
      direction,
      ratio: 0.5,
      first: before ? pane : node,
      second: before ? node : pane,
    };
  }
  const first = splitPane(node.first, targetId, side, pane, splitId);
  const second = first === node.first ? splitPane(node.second, targetId, side, pane, splitId) : node.second;
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

/** 창을 빼고 남은 형제가 부모 자리를 차지합니다. 마지막 창이면 null. */
export function removePane(node: PaneNode, id: string): PaneNode | null {
  if (node.kind === "pane") return node.id === id ? null : node;
  const first = removePane(node.first, id);
  if (first === null) return node.second;
  const second = removePane(node.second, id);
  if (second === null) return node.first;
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

export function setRatio(node: PaneNode, splitId: string, ratio: number): PaneNode {
  if (node.kind === "pane") return node;
  if (node.id === splitId) return { ...node, ratio: clampRatio(ratio) };
  const first = setRatio(node.first, splitId, ratio);
  const second = setRatio(node.second, splitId, ratio);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

/** 같은 프로필을 쓰는 창의 프로필 정보를 바꿉니다 (프로필 편집 후 표시 이름 갱신). */
export function updateProfile(node: PaneNode, profile: Profile): PaneNode {
  if (node.kind === "pane") return node.profile.id === profile.id ? { ...node, profile } : node;
  const first = updateProfile(node.first, profile);
  const second = updateProfile(node.second, profile);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}

/** 프로필 목록에 든 창을 모두 뺍니다. 남는 창이 없으면 null. */
export function removeProfiles(node: PaneNode, profileIds: Set<string>): PaneNode | null {
  let current: PaneNode | null = node;
  for (const p of paneList(node)) {
    if (current && profileIds.has(p.profile.id)) current = removePane(current, p.id);
  }
  return current;
}

/** 각 창과 경계선의 위치를 탭 영역 기준 비율(0~1)로 계산합니다. */
export function computeLayout(
  node: PaneNode,
  rect: Rect = { x: 0, y: 0, w: 1, h: 1 },
): { panes: Map<string, Rect>; dividers: Divider[] } {
  const panes = new Map<string, Rect>();
  const dividers: Divider[] = [];
  const walk = (n: PaneNode, r: Rect) => {
    if (n.kind === "pane") {
      panes.set(n.id, r);
      return;
    }
    dividers.push({ splitId: n.id, direction: n.direction, rect: r, at: n.ratio });
    if (n.direction === "row") {
      const w = r.w * n.ratio;
      walk(n.first, { x: r.x, y: r.y, w, h: r.h });
      walk(n.second, { x: r.x + w, y: r.y, w: r.w - w, h: r.h });
    } else {
      const h = r.h * n.ratio;
      walk(n.first, { x: r.x, y: r.y, w: r.w, h });
      walk(n.second, { x: r.x, y: r.y + h, w: r.w, h: r.h - h });
    }
  };
  walk(node, rect);
  return { panes, dividers };
}

/**
 * 끌어 놓은 위치가 창의 어느 쪽인지 정합니다. 가장 가까운 가장자리 쪽으로 나눕니다.
 * (x, y)는 창 안의 상대 위치(0~1)입니다.
 */
export function dropSideAt(x: number, y: number): DropSide {
  const candidates: [DropSide, number][] = [
    ["left", x],
    ["right", 1 - x],
    ["top", y],
    ["bottom", 1 - y],
  ];
  candidates.sort((a, b) => a[1] - b[1]);
  return candidates[0]?.[0] ?? "right";
}

/**
 * `fromId` 창에서 `direction` 쪽으로 이웃한 창을 찾습니다.
 * 그쪽 방향으로 맞닿아 있고 겹치는 길이가 가장 긴 창을 고릅니다. 없으면 null.
 */
export function neighborPane(node: PaneNode, fromId: string, direction: FocusDirection): string | null {
  const { panes } = computeLayout(node);
  const from = panes.get(fromId);
  if (!from) return null;
  const EPS = 1e-6;
  const overlap = (a0: number, a1: number, b0: number, b1: number) => Math.min(a1, b1) - Math.max(a0, b0);
  let best: { id: string; score: number } | null = null;
  for (const [id, r] of panes) {
    if (id === fromId) continue;
    let touches = false;
    let score = 0;
    switch (direction) {
      case "left":
        touches = Math.abs(r.x + r.w - from.x) < EPS;
        score = overlap(r.y, r.y + r.h, from.y, from.y + from.h);
        break;
      case "right":
        touches = Math.abs(from.x + from.w - r.x) < EPS;
        score = overlap(r.y, r.y + r.h, from.y, from.y + from.h);
        break;
      case "top":
        touches = Math.abs(r.y + r.h - from.y) < EPS;
        score = overlap(r.x, r.x + r.w, from.x, from.x + from.w);
        break;
      case "bottom":
        touches = Math.abs(from.y + from.h - r.y) < EPS;
        score = overlap(r.x, r.x + r.w, from.x, from.x + from.w);
        break;
    }
    if (touches && score > EPS && (!best || score > best.score)) best = { id, score };
  }
  return best?.id ?? null;
}
