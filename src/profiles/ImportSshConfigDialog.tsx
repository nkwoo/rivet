import { useEffect, useMemo, useState } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { TerminalIcon } from "../components/Icons.tsx";
import { api, errorMessage, type Profile, type SshConfigHost } from "../lib/api.ts";

interface ImportSshConfigDialogProps {
  profiles: Profile[];
  onImported: (count: number) => void;
  onClose: () => void;
}

const DEFAULT_GROUP = "SSH config";

function sameTarget(p: Profile, h: SshConfigHost): boolean {
  return (
    (p.protocol === "ssh" || p.protocol === "sftp") &&
    p.host.toLowerCase() === h.host.toLowerCase() &&
    p.port === h.port &&
    p.username === h.user
  );
}

/** `~/.ssh/config`의 호스트를 골라 SSH 연결로 가져옵니다. 이미 있는 연결은 기본으로 선택하지 않습니다. */
export function ImportSshConfigDialog({ profiles, onImported, onClose }: ImportSshConfigDialogProps) {
  const [hosts, setHosts] = useState<SshConfigHost[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [group, setGroup] = useState(DEFAULT_GROUP);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [failures, setFailures] = useState<string[]>([]);

  const existing = useMemo(
    () => new Set((hosts ?? []).filter((h) => profiles.some((p) => sameTarget(p, h))).map((h) => h.alias)),
    [hosts, profiles],
  );

  useEffect(() => {
    api
      .sshConfigScan()
      .then((list) => {
        setHosts(list);
        setSelected(new Set(list.filter((h) => !profiles.some((p) => sameTarget(p, h))).map((h) => h.alias)));
      })
      .catch((e: unknown) => setError(errorMessage(e)));
    // 대화상자를 열 때 한 번만 읽음
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (alias: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(alias)) next.delete(alias);
      else next.add(alias);
      return next;
    });

  const allSelected = hosts !== null && hosts.length > 0 && hosts.every((h) => selected.has(h.alias));

  const importSelected = async () => {
    if (!hosts) return;
    setBusy(true);
    setError(null);
    const failed: string[] = [];
    let count = 0;
    for (const h of hosts.filter((x) => selected.has(x.alias))) {
      try {
        await api.createProfile({
          name: h.alias.slice(0, 100),
          protocol: "ssh",
          host: h.host,
          port: h.port,
          username: h.user,
          authMethod: h.identityFile ? "key" : "agent",
          keyPath: h.identityFile,
          group: group.trim() || null,
          tags: [],
          notes: h.proxyJump ? `ProxyJump ${h.proxyJump}` : "",
          favorite: false,
          rdp: null,
        });
        count++;
      } catch (e) {
        failed.push(`${h.alias}: ${errorMessage(e)}`);
      }
    }
    setBusy(false);
    if (failed.length === 0) {
      onImported(count);
    } else {
      setFailures(failed);
      if (count > 0) onImported(count);
    }
  };

  return (
    <Dialog
      wide
      title="~/.ssh/config 가져오기"
      icon={
        <span className="dialog-icon dialog-icon-info">
          <TerminalIcon />
        </span>
      }
      onClose={onClose}
      footer={
        <>
          <span className="spacer" />
          <button type="button" className="button button-ghost" onClick={onClose}>
            {failures.length > 0 ? "닫기" : "취소"}
          </button>
          <button
            type="button"
            className="button button-primary"
            disabled={busy || selected.size === 0 || failures.length > 0}
            onClick={() => void importSelected()}
          >
            {selected.size}개 가져오기
          </button>
        </>
      }
    >
      {hosts === null && !error && <p className="dialog-text muted">설정 파일을 읽는 중…</p>}
      {hosts?.length === 0 && (
        <p className="dialog-text muted">
          가져올 호스트가 없습니다. <code className="mono">~/.ssh/config</code>에 와일드카드가 아닌{" "}
          <code className="mono">Host</code> 항목이 있어야 합니다.
        </p>
      )}
      {hosts && hosts.length > 0 && (
        <>
          <p className="dialog-text muted">
            키 파일(IdentityFile)이 있으면 개인 키, 없으면 ssh-agent 인증으로 만듭니다. 비밀번호와 패스프레이즈는
            가져오지 않습니다. 같은 호스트·포트·사용자의 연결이 이미 있으면 선택하지 않습니다.
          </p>
          <label className="field">
            <span className="label">그룹</span>
            <input maxLength={64} value={group} onChange={(e) => setGroup(e.target.value)} />
          </label>
          <div className="import-list">
            <table className="table">
              <thead>
                <tr>
                  <th>
                    <input
                      type="checkbox"
                      aria-label="모두 선택"
                      checked={allSelected}
                      onChange={() =>
                        setSelected(allSelected ? new Set() : new Set(hosts.map((h) => h.alias)))
                      }
                    />
                  </th>
                  <th>이름</th>
                  <th>주소</th>
                  <th>인증</th>
                </tr>
              </thead>
              <tbody>
                {hosts.map((h) => (
                  <tr key={h.alias}>
                    <td>
                      <input
                        type="checkbox"
                        aria-label={`${h.alias} 선택`}
                        checked={selected.has(h.alias)}
                        onChange={() => toggle(h.alias)}
                      />
                    </td>
                    <td>
                      {h.alias}
                      {existing.has(h.alias) && <span className="tag">이미 있음</span>}
                    </td>
                    <td className="mono">
                      {h.user}@{h.host}
                      {h.port !== 22 ? `:${h.port}` : ""}
                      {h.proxyJump && <div className="muted">경유 {h.proxyJump}</div>}
                    </td>
                    <td className="mono">{h.identityFile ?? "ssh-agent"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
      {failures.length > 0 && (
        <div role="alert" className="form-error">
          일부 호스트를 가져오지 못했습니다.
          <ul>
            {failures.map((f) => (
              <li key={f} className="mono">
                {f}
              </li>
            ))}
          </ul>
        </div>
      )}
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
    </Dialog>
  );
}
