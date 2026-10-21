import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RivetMark } from "./RivetMark.tsx";
import { api, errorMessage, type Profile, type ProfileInput } from "./lib/api.ts";
import { PlusIcon, SearchIcon, ShieldCheckIcon } from "./components/Icons.tsx";
import { Sidebar } from "./profiles/Sidebar.tsx";
import { ProfileDialog } from "./profiles/ProfileDialog.tsx";
import { KnownHostsDialog } from "./security/KnownHostsDialog.tsx";
import { ImportSshConfigDialog } from "./profiles/ImportSshConfigDialog.tsx";
import { TabBar, type TabInfo } from "./terminal/TabBar.tsx";
import { TerminalTab, type TabState } from "./terminal/TerminalTab.tsx";
import { SplitView } from "./terminal/SplitView.tsx";
import {
  dropSideAt,
  findPane,
  neighborPane,
  paneList,
  removePane,
  removeProfiles as removeProfilePanes,
  setRatio,
  splitPane,
  updateProfile,
  type DropSide,
  type FocusDirection,
  type SplitDirection,
} from "./terminal/layout.ts";
import { RdpTab } from "./rdp/RdpTab.tsx";
import { StatusBar } from "./StatusBar.tsx";
import { LanguageSelect } from "./components/LanguageSelect.tsx";
import { messages, useI18n } from "./i18n/index.ts";

export interface AppInfo {
  name: string;
  version: string;
}

type ProfileDialogState = { profile: Profile | null } | null;
/** 사이드바에서 연결을 끌어 터미널 창 위에 놓는 중 */
type ProfileDrag = { profile: Profile; x: number; y: number; target: { paneId: string; side: DropSide } | null };

const IS_MAC = navigator.userAgent.includes("Mac");

/** 창을 나눌 수 있는 프로필 (셸이 있는 SSH만) */
const splittable = (profile: Profile) => profile.protocol === "ssh";

/** 분할·창 이동 단축키. macOS는 iTerm2, 그 밖에는 Windows Terminal과 같은 조합 */
function paneShortcut(
  e: KeyboardEvent,
): { kind: "split"; direction: SplitDirection } | { kind: "focus"; direction: FocusDirection } | { kind: "close" } | null {
  const arrows: Record<string, FocusDirection> = {
    ArrowLeft: "left",
    ArrowRight: "right",
    ArrowUp: "top",
    ArrowDown: "bottom",
  };
  if (IS_MAC) {
    if (e.metaKey && !e.ctrlKey && !e.altKey && e.code === "KeyD")
      return { kind: "split", direction: e.shiftKey ? "column" : "row" };
    const arrow = arrows[e.key];
    if (e.metaKey && e.altKey && !e.ctrlKey && arrow) return { kind: "focus", direction: arrow };
    return null;
  }
  if (e.altKey && e.shiftKey && !e.ctrlKey && e.code === "Equal") return { kind: "split", direction: "row" };
  if (e.altKey && e.shiftKey && !e.ctrlKey && e.code === "Minus") return { kind: "split", direction: "column" };
  const arrow = arrows[e.key];
  if (e.altKey && !e.ctrlKey && !e.shiftKey && arrow) return { kind: "focus", direction: arrow };
  if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyW") return { kind: "close" };
  return null;
}
type KnownHostsState = { highlight?: { host: string; port: number } } | null;

export default function App() {
  const { m } = useI18n();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [query, setQuery] = useState("");
  const [tabs, setTabs] = useState<TabInfo[]>([]);
  const [activeTab, setActiveTab] = useState<string | null>(null);
  /** 창(pane) ID별 연결 상태 */
  const [tabStates, setTabStates] = useState<Record<string, TabState>>({});
  const [profileDrag, setProfileDrag] = useState<ProfileDrag | null>(null);
  const [profileDialog, setProfileDialog] = useState<ProfileDialogState>(null);
  const [knownHosts, setKnownHosts] = useState<KnownHostsState>(null);
  const [toast, setToast] = useState<string | null>(null);
  /** RDP 전체 화면: 상단/사이드바/탭/상태 표시줄을 숨김 */
  const [immersive, setImmersive] = useState(false);
  const [importing, setImporting] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.setTimeout(() => setToast((t) => (t === message ? null : t)), 4000);
  }, []);

  const loadProfiles = useCallback(() => {
    api
      .listProfiles()
      .then(setProfiles)
      .catch((e: unknown) => showToast(messages().app.loadFailed(errorMessage(e))));
  }, [showToast]);

  useEffect(() => {
    invoke<AppInfo>("app_info")
      .then(setInfo)
      .catch(() => setInfo(null));
    loadProfiles();
  }, [loadProfiles]);

  // ⌘K / Ctrl+K: 검색으로 이동
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const groups = useMemo(
    () => [...new Set(profiles.map((p) => p.group).filter((g): g is string => g !== null))].sort(),
    [profiles],
  );

  const allPanes = useMemo(() => tabs.flatMap((t) => paneList(t.layout)), [tabs]);

  const connectedIds = useMemo(
    () => new Set(allPanes.filter((p) => tabStates[p.id]?.kind === "connected").map((p) => p.profile.id)),
    [allPanes, tabStates],
  );

  /** 남아 있는 창의 상태만 남김 */
  const pruneStates = (next: TabInfo[]) => {
    const alive = new Set(next.flatMap((t) => paneList(t.layout).map((p) => p.id)));
    setTabStates((s) => Object.fromEntries(Object.entries(s).filter(([id]) => alive.has(id))));
  };

  const openProfile = useCallback((profile: Profile) => {
    const id = crypto.randomUUID();
    const paneId = crypto.randomUUID();
    setTabs((t) => [...t, { id, layout: { kind: "pane", id: paneId, profile }, focused: paneId }]);
    setActiveTab(id);
  }, []);

  const closeTab = (id: string) => {
    const index = tabs.findIndex((t) => t.id === id);
    const next = tabs.filter((t) => t.id !== id);
    setTabs(next);
    if (id === activeTab) setActiveTab(next[Math.min(index, next.length - 1)]?.id ?? null);
    pruneStates(next);
  };

  /** 창 하나를 닫습니다. 탭의 마지막 창이면 탭을 닫습니다. */
  const closePane = (tabId: string, paneId: string) => {
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab) return;
    const layout = removePane(tab.layout, paneId);
    if (!layout) {
      closeTab(tabId);
      return;
    }
    const remaining = paneList(layout);
    const focused = tab.focused === paneId ? (remaining[0]?.id ?? paneId) : tab.focused;
    const next = tabs.map((t) => (t.id === tabId ? { ...t, layout, focused } : t));
    setTabs(next);
    pruneStates(next);
  };

  /** `paneId` 창을 나눠 `profile`(없으면 같은 프로필)로 새 연결을 엽니다. */
  const splitTabPane = (tabId: string, paneId: string, side: DropSide, profile?: Profile) => {
    setTabs((ts) =>
      ts.map((t) => {
        if (t.id !== tabId) return t;
        const target = findPane(t.layout, paneId);
        if (!target) return t;
        const newPane = { kind: "pane" as const, id: crypto.randomUUID(), profile: profile ?? target.profile };
        if (!splittable(newPane.profile) || !splittable(target.profile)) return t;
        return { ...t, layout: splitPane(t.layout, paneId, side, newPane, crypto.randomUUID()), focused: newPane.id };
      }),
    );
    setActiveTab(tabId);
  };

  const focusPane = useCallback((tabId: string, paneId: string) => {
    // 창 안을 누를 때마다 불리므로, 바뀐 것이 없으면 상태를 그대로 둠
    setTabs((ts) =>
      ts.some((t) => t.id === tabId && t.focused !== paneId)
        ? ts.map((t) => (t.id === tabId ? { ...t, focused: paneId } : t))
        : ts,
    );
  }, []);

  const setTabState = useCallback((id: string, state: TabState) => {
    setTabStates((s) => ({ ...s, [id]: state }));
  }, []);

  const saveProfile = async (input: ProfileInput, connect: boolean) => {
    const existing = profileDialog?.profile;
    const saved = existing ? await api.updateProfile(existing.id, input) : await api.createProfile(input);
    setProfileDialog(null);
    loadProfiles();
    // 열린 창의 표시 이름 갱신 (이미 연결된 세션에는 영향 없음)
    setTabs((t) => t.map((tab) => ({ ...tab, layout: updateProfile(tab.layout, saved) })));
    if (connect) openProfile(saved);
  };

  /** 창과 탭을 닫습니다. 창이 사라지면 그 창의 세션도 끊깁니다 (각 창의 정리 코드). */
  const closeTabsOf = (profileIds: Set<string>) => {
    if (!allPanes.some((p) => profileIds.has(p.profile.id))) return;
    const index = tabs.findIndex((t) => t.id === activeTab);
    const next = tabs.flatMap((t) => {
      const layout = removeProfilePanes(t.layout, profileIds);
      if (!layout) return [];
      const focused = findPane(layout, t.focused) ? t.focused : (paneList(layout)[0]?.id ?? t.focused);
      return [{ ...t, layout, focused }];
    });
    setTabs(next);
    if (!next.some((t) => t.id === activeTab)) setActiveTab(next[Math.min(index, next.length - 1)]?.id ?? null);
    pruneStates(next);
    setImmersive(false);
  };

  /** 연결을 삭제하고, 그 연결로 열린 세션을 끊습니다. 일부가 실패하면 나머지는 지우고 알립니다. */
  const removeProfiles = async (targets: Profile[]) => {
    const removed = new Set<string>();
    const failed: string[] = [];
    for (const p of targets) {
      try {
        await api.deleteProfile(p.id);
        removed.add(p.id);
      } catch (e) {
        failed.push(`${p.name} (${errorMessage(e)})`);
      }
    }
    closeTabsOf(removed);
    loadProfiles();
    if (failed.length > 0) showToast(m.app.deleteFailed(failed.join(", ")));
    else if (targets.length === 1 && targets[0]) showToast(m.app.deletedOne(targets[0].name));
    else showToast(m.app.deletedMany(removed.size));
  };

  const deleteProfile = async (profile: Profile) => {
    await removeProfiles([profile]);
    setProfileDialog(null);
  };

  const active = tabs.find((t) => t.id === activeTab) ?? null;

  // macOS ⌘W (menu.rs): 열린 대화상자 → 포커스된 분할 창 → 활성 탭(연결 종료) → 창 순서로 닫음
  const closeTopmost = () => {
    if (profileDialog) setProfileDialog(null);
    else if (importing) setImporting(false);
    else if (knownHosts) setKnownHosts(null);
    else if (active) closePane(active.id, active.focused);
    else void getCurrentWindow().close();
  };
  const closeTopmostRef = useRef(closeTopmost);
  closeTopmostRef.current = closeTopmost;

  useEffect(() => {
    const unlisten = listen("menu:close-tab", () => closeTopmostRef.current());
    return () => void unlisten.then((f) => f());
  }, []);

  // 분할 단축키. 터미널(xterm)보다 먼저 받도록 캡처 단계에서 처리
  const onPaneKey = (e: KeyboardEvent) => {
    const action = paneShortcut(e);
    // 대화상자(앱 전체 또는 창 안의 비밀번호·호스트 키 확인)가 열려 있으면 무시
    if (!action || !active || document.querySelector(".backdrop")) return;
    if (action.kind === "close") {
      e.preventDefault();
      e.stopPropagation();
      closePane(active.id, active.focused);
      return;
    }
    const focusedPane = findPane(active.layout, active.focused);
    if (!focusedPane || !splittable(focusedPane.profile)) return;
    e.preventDefault();
    e.stopPropagation();
    if (action.kind === "split") {
      splitTabPane(active.id, active.focused, action.direction === "row" ? "right" : "bottom");
    } else {
      const next = neighborPane(active.layout, active.focused, action.direction);
      if (next) focusPane(active.id, next);
    }
  };
  const onPaneKeyRef = useRef(onPaneKey);
  onPaneKeyRef.current = onPaneKey;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => onPaneKeyRef.current(e);
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  /** 사이드바에서 SSH 연결을 끌기 시작: 터미널 창 위에 놓으면 그 창을 나눠 엽니다. */
  const startProfileDrag = (profile: Profile, x: number, y: number) => {
    const targetAt = (cx: number, cy: number): ProfileDrag["target"] => {
      const el = document.elementFromPoint(cx, cy)?.closest<HTMLElement>(".split-view:not([hidden]) [data-pane-id]");
      const paneId = el?.dataset.paneId;
      const tab = tabs.find((t) => t.id === activeTab);
      const target = paneId && tab ? findPane(tab.layout, paneId) : null;
      if (!el || !target || !splittable(target.profile)) return null;
      const box = el.getBoundingClientRect();
      return { paneId: target.id, side: dropSideAt((cx - box.left) / box.width, (cy - box.top) / box.height) };
    };
    setProfileDrag({ profile, x, y, target: targetAt(x, y) });
    document.body.classList.add("is-dragging");
    const move = (ev: PointerEvent) =>
      setProfileDrag({ profile, x: ev.clientX, y: ev.clientY, target: targetAt(ev.clientX, ev.clientY) });
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("is-dragging");
      setProfileDrag(null);
      const target = targetAt(ev.clientX, ev.clientY);
      if (target && activeTab) splitTabPane(activeTab, target.paneId, target.side, profile);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const openIds = useMemo(() => new Set(allPanes.map((p) => p.profile.id)), [allPanes]);
  const activeProfileId = active ? (findPane(active.layout, active.focused)?.profile.id ?? null) : null;
  const activeState = active ? (tabStates[active.focused] ?? null) : null;

  return (
    <div className={`app ${immersive ? "is-immersive" : ""}`}>
      <header className="topbar">
        <div className="brand">
          <RivetMark size={26} />
          <span className="brand-word">Rivet</span>
        </div>
        <label className="search">
          <SearchIcon />
          <span className="visually-hidden">{m.app.searchLabel}</span>
          <input
            ref={searchRef}
            type="search"
            placeholder={m.app.searchPlaceholder}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <kbd className="mono">⌘K</kbd>
        </label>
        <div className="topbar-actions">
          <button type="button" className="button button-primary" onClick={() => setProfileDialog({ profile: null })}>
            <PlusIcon />
            {m.common.newConnection}
          </button>
          <button
            type="button"
            className="icon-button icon-button-boxed"
            aria-label={m.app.knownHosts}
            title={m.app.knownHosts}
            onClick={() => setKnownHosts({})}
          >
            <ShieldCheckIcon />
          </button>
          <LanguageSelect />
        </div>
      </header>

      <div className="workspace">
        <Sidebar
          profiles={profiles}
          query={query}
          connectedIds={connectedIds}
          activeProfileId={activeProfileId}
          onOpen={openProfile}
          onEdit={(profile) => setProfileDialog({ profile })}
          onImport={() => setImporting(true)}
          openIds={openIds}
          onDeleteMany={removeProfiles}
          canDrag={splittable}
          onDragStart={startProfileDrag}
        />
        <main className="main">
          {tabs.length > 0 && (
            <TabBar
              tabs={tabs}
              activeId={activeTab}
              onSelect={setActiveTab}
              onClose={closeTab}
              onNew={() => setProfileDialog({ profile: null })}
            />
          )}
          <div className="terminals">
            {tabs.length === 0 && (
              <div className="welcome">
                <div className="lockup">
                  <RivetMark size={72} />
                  <span className="wordmark">Rivet</span>
                </div>
                <p className="muted">{m.app.welcome}</p>
              </div>
            )}
            {tabs.map((t) => {
              const visible = t.id === activeTab;
              const split = t.layout.kind === "split";
              return (
                <SplitView
                  key={t.id}
                  layout={t.layout}
                  visible={visible}
                  focusedId={t.focused}
                  dropTarget={visible && profileDrag?.target ? profileDrag.target : null}
                  onRatio={(splitId, ratio) =>
                    setTabs((ts) => ts.map((x) => (x.id === t.id ? { ...x, layout: setRatio(x.layout, splitId, ratio) } : x)))
                  }
                  renderPane={(p) =>
                    p.profile.protocol === "rdp" ? (
                      <RdpTab
                        profile={p.profile}
                        active={visible}
                        onState={(state) => setTabState(p.id, state)}
                        onReviewKnownHosts={(host, port) => setKnownHosts({ highlight: { host, port } })}
                        onNotice={showToast}
                        onImmersive={setImmersive}
                      />
                    ) : (
                      <TerminalTab
                        profile={p.profile}
                        active={visible}
                        focused={p.id === t.focused}
                        onFocus={() => focusPane(t.id, p.id)}
                        pane={
                          splittable(p.profile)
                            ? {
                                split,
                                onSplit: (direction) =>
                                  splitTabPane(t.id, p.id, direction === "row" ? "right" : "bottom"),
                                onClose: () => closePane(t.id, p.id),
                              }
                            : undefined
                        }
                        onState={(state) => setTabState(p.id, state)}
                        onReviewKnownHosts={(host, port) => setKnownHosts({ highlight: { host, port } })}
                        onNotice={showToast}
                      />
                    )
                  }
                />
              );
            })}
          </div>
        </main>
      </div>

      <StatusBar app={info} state={activeState} />

      {profileDialog && (
        <ProfileDialog
          profile={profileDialog.profile}
          groups={groups}
          profiles={profiles}
          onSave={saveProfile}
          onDelete={deleteProfile}
          onForgetSecret={(p) => api.forgetSecret(p.id)}
          onClose={() => setProfileDialog(null)}
        />
      )}
      {importing && (
        <ImportSshConfigDialog
          profiles={profiles}
          onImported={(count) => {
            loadProfiles();
            showToast(m.app.imported(count));
          }}
          onClose={() => setImporting(false)}
        />
      )}
      {knownHosts && <KnownHostsDialog highlight={knownHosts.highlight} onClose={() => setKnownHosts(null)} />}
      {profileDrag && (
        <div className="drag-ghost" style={{ left: profileDrag.x + 14, top: profileDrag.y + 10 }} aria-hidden="true">
          {profileDrag.target ? m.app.dropToSplit(profileDrag.profile.name) : profileDrag.profile.name}
        </div>
      )}
      {toast && (
        <div role="status" className="toast">
          {toast}
        </div>
      )}
    </div>
  );
}
