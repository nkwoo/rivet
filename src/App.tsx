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
import { RdpTab } from "./rdp/RdpTab.tsx";
import { StatusBar } from "./StatusBar.tsx";

export interface AppInfo {
  name: string;
  version: string;
}

type ProfileDialogState = { profile: Profile | null } | null;
type KnownHostsState = { highlight?: { host: string; port: number } } | null;

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [query, setQuery] = useState("");
  const [tabs, setTabs] = useState<TabInfo[]>([]);
  const [activeTab, setActiveTab] = useState<string | null>(null);
  const [tabStates, setTabStates] = useState<Record<string, TabState>>({});
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
      .catch((e: unknown) => showToast(`연결 목록을 불러오지 못했습니다: ${errorMessage(e)}`));
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

  const connectedIds = useMemo(
    () =>
      new Set(tabs.filter((t) => tabStates[t.id]?.kind === "connected").map((t) => t.profile.id)),
    [tabs, tabStates],
  );

  const openProfile = useCallback((profile: Profile) => {
    const id = crypto.randomUUID();
    setTabs((t) => [...t, { id, profile }]);
    setActiveTab(id);
  }, []);

  const closeTab = (id: string) => {
    const index = tabs.findIndex((t) => t.id === id);
    const next = tabs.filter((t) => t.id !== id);
    setTabs(next);
    if (id === activeTab) setActiveTab(next[Math.min(index, next.length - 1)]?.id ?? null);
    setTabStates(({ [id]: _removed, ...rest }) => rest);
  };

  const setTabState = useCallback((id: string, state: TabState) => {
    setTabStates((s) => ({ ...s, [id]: state }));
  }, []);

  const saveProfile = async (input: ProfileInput, connect: boolean) => {
    const existing = profileDialog?.profile;
    const saved = existing ? await api.updateProfile(existing.id, input) : await api.createProfile(input);
    setProfileDialog(null);
    loadProfiles();
    // 열린 탭의 표시 이름 갱신 (이미 연결된 세션에는 영향 없음)
    setTabs((t) => t.map((tab) => (tab.profile.id === saved.id ? { ...tab, profile: saved } : tab)));
    if (connect) openProfile(saved);
  };

  /** 탭을 닫습니다. 탭이 사라지면 그 탭의 세션도 끊깁니다 (각 탭의 정리 코드). */
  const closeTabsOf = (profileIds: Set<string>) => {
    if (!tabs.some((t) => profileIds.has(t.profile.id))) return;
    const index = tabs.findIndex((t) => t.id === activeTab);
    const next = tabs.filter((t) => !profileIds.has(t.profile.id));
    setTabs(next);
    if (!next.some((t) => t.id === activeTab)) setActiveTab(next[Math.min(index, next.length - 1)]?.id ?? null);
    setTabStates((s) => Object.fromEntries(Object.entries(s).filter(([id]) => next.some((t) => t.id === id))));
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
    if (failed.length > 0) showToast(`삭제하지 못한 연결: ${failed.join(", ")}`);
    else if (targets.length === 1 && targets[0]) showToast(`“${targets[0].name}” 연결을 삭제했습니다.`);
    else showToast(`연결 ${removed.size}개를 삭제했습니다.`);
  };

  const deleteProfile = async (profile: Profile) => {
    await removeProfiles([profile]);
    setProfileDialog(null);
  };

  // macOS ⌘W (menu.rs): 열린 대화상자 → 활성 탭(연결 종료) → 창 순서로 닫음
  const closeTopmost = () => {
    if (profileDialog) setProfileDialog(null);
    else if (importing) setImporting(false);
    else if (knownHosts) setKnownHosts(null);
    else if (activeTab) closeTab(activeTab);
    else void getCurrentWindow().close();
  };
  const closeTopmostRef = useRef(closeTopmost);
  closeTopmostRef.current = closeTopmost;

  useEffect(() => {
    const unlisten = listen("menu:close-tab", () => closeTopmostRef.current());
    return () => void unlisten.then((f) => f());
  }, []);

  const openIds = useMemo(() => new Set(tabs.map((t) => t.profile.id)), [tabs]);
  const activeProfileId = tabs.find((t) => t.id === activeTab)?.profile.id ?? null;
  const activeState = activeTab ? (tabStates[activeTab] ?? null) : null;

  return (
    <div className={`app ${immersive ? "is-immersive" : ""}`}>
      <header className="topbar">
        <div className="brand">
          <RivetMark size={26} />
          <span className="brand-word">Rivet</span>
        </div>
        <label className="search">
          <SearchIcon />
          <span className="visually-hidden">호스트 검색</span>
          <input
            ref={searchRef}
            type="search"
            placeholder="호스트, 그룹, 태그 검색"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <kbd className="mono">⌘K</kbd>
        </label>
        <div className="topbar-actions">
          <button type="button" className="button button-primary" onClick={() => setProfileDialog({ profile: null })}>
            <PlusIcon />새 연결
          </button>
          <button
            type="button"
            className="icon-button icon-button-boxed"
            aria-label="저장된 호스트 키와 인증서"
            title="저장된 호스트 키와 인증서"
            onClick={() => setKnownHosts({})}
          >
            <ShieldCheckIcon />
          </button>
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
                <p className="muted">왼쪽 목록에서 연결을 선택하거나 새 연결을 추가하세요.</p>
              </div>
            )}
            {tabs.map((t) =>
              t.profile.protocol === "rdp" ? (
                <RdpTab
                  key={t.id}
                  profile={t.profile}
                  active={t.id === activeTab}
                  onState={(state) => setTabState(t.id, state)}
                  onReviewKnownHosts={(host, port) => setKnownHosts({ highlight: { host, port } })}
                  onImmersive={setImmersive}
                />
              ) : (
                <TerminalTab
                  key={t.id}
                  profile={t.profile}
                  active={t.id === activeTab}
                  onState={(state) => setTabState(t.id, state)}
                  onReviewKnownHosts={(host, port) => setKnownHosts({ highlight: { host, port } })}
                />
              ),
            )}
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
            showToast(`~/.ssh/config에서 연결 ${count}개를 가져왔습니다.`);
          }}
          onClose={() => setImporting(false)}
        />
      )}
      {knownHosts && <KnownHostsDialog highlight={knownHosts.highlight} onClose={() => setKnownHosts(null)} />}
      {toast && (
        <div role="status" className="toast">
          {toast}
        </div>
      )}
    </div>
  );
}
