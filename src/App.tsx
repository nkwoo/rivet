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
import { TabBar, type TabInfo } from "./terminal/TabBar.tsx";
import { TerminalTab, type TabState } from "./terminal/TerminalTab.tsx";
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

  const openProfile = useCallback(
    (profile: Profile) => {
      if (profile.protocol === "rdp") {
        showToast(`${profile.protocol.toUpperCase()} 연결은 아직 지원하지 않습니다.`);
        return;
      }
      const id = crypto.randomUUID();
      setTabs((t) => [...t, { id, profile }]);
      setActiveTab(id);
    },
    [showToast],
  );

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

  const deleteProfile = async (profile: Profile) => {
    await api.deleteProfile(profile.id);
    setProfileDialog(null);
    loadProfiles();
    showToast(`“${profile.name}” 연결을 삭제했습니다.`);
  };

  // macOS ⌘W (menu.rs): 열린 대화상자 → 활성 탭(연결 종료) → 창 순서로 닫음
  const closeTopmost = () => {
    if (profileDialog) setProfileDialog(null);
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

  const activeProfileId = tabs.find((t) => t.id === activeTab)?.profile.id ?? null;
  const activeState = activeTab ? (tabStates[activeTab] ?? null) : null;

  return (
    <div className="app">
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
            {tabs.map((t) => (
              <TerminalTab
                key={t.id}
                profile={t.profile}
                active={t.id === activeTab}
                onState={(state) => setTabState(t.id, state)}
                onReviewKnownHosts={(host, port) => setKnownHosts({ highlight: { host, port } })}
              />
            ))}
          </div>
        </main>
      </div>

      <StatusBar app={info} state={activeState} />

      {profileDialog && (
        <ProfileDialog
          profile={profileDialog.profile}
          groups={groups}
          onSave={saveProfile}
          onDelete={deleteProfile}
          onForgetSecret={(p) => api.forgetSecret(p.id)}
          onClose={() => setProfileDialog(null)}
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
