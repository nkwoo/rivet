import { useEffect, useMemo, useState } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { TerminalIcon } from "../components/Icons.tsx";
import { api, errorMessage, type Profile, type ProfileInput, type SshConfigHost } from "../lib/api.ts";
import { useI18n } from "../i18n/index.ts";

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
  const { m } = useI18n();
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
    const chosen = hosts.filter((x) => selected.has(x.alias));
    const created = new Map<string, { profile: Profile; input: ProfileInput }>();
    for (const h of chosen) {
      const input: ProfileInput = {
        name: h.alias.slice(0, 100),
        protocol: "ssh",
        host: h.host,
        port: h.port,
        username: h.user,
        authMethod: h.identityFile ? "key" : "agent",
        keyPath: h.identityFile,
        group: group.trim() || null,
        tags: [],
        notes: "",
        favorite: false,
        rdp: null,
        jumpProfileId: null,
      };
      try {
        created.set(h.alias.toLowerCase(), { profile: await api.createProfile(input), input });
      } catch (e) {
        failed.push(`${h.alias}: ${errorMessage(e)}`);
      }
    }

    // ProxyJump: 같은 이름(별칭)의 SSH 연결을 점프 호스트로 연결 (이번에 가져온 것 또는 기존 연결)
    const findJump = (alias: string): Profile | undefined =>
      created.get(alias.toLowerCase())?.profile ??
      profiles.find(
        (p) => (p.protocol === "ssh" || p.protocol === "sftp") && p.name.toLowerCase() === alias.toLowerCase(),
      );
    for (const h of chosen) {
      const entry = created.get(h.alias.toLowerCase());
      if (!h.proxyJump || !entry) continue;
      const jump = h.proxyJump.includes(",") ? undefined : findJump(h.proxyJump.trim());
      if (!jump || jump.id === entry.profile.id) {
        failed.push(m.importConfig.jumpNotFound(h.alias, h.proxyJump));
        continue;
      }
      try {
        await api.updateProfile(entry.profile.id, { ...entry.input, jumpProfileId: jump.id });
      } catch (e) {
        failed.push(m.importConfig.jumpFailed(h.alias, errorMessage(e)));
      }
    }

    setBusy(false);
    if (failed.length === 0) {
      onImported(created.size);
    } else {
      setFailures(failed);
      if (created.size > 0) onImported(created.size);
    }
  };

  return (
    <Dialog
      wide
      title={m.sidebar.import}
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
            {failures.length > 0 ? m.common.close : m.common.cancel}
          </button>
          <button
            type="button"
            className="button button-primary"
            disabled={busy || selected.size === 0 || failures.length > 0}
            onClick={() => void importSelected()}
          >
            {m.importConfig.import(selected.size)}
          </button>
        </>
      }
    >
      {hosts === null && !error && <p className="dialog-text muted">{m.importConfig.reading}</p>}
      {hosts?.length === 0 && (
        <p className="dialog-text muted">
          {m.importConfig.noneBefore}
          <code className="mono">~/.ssh/config</code>
          {m.importConfig.noneMiddle}
          <code className="mono">Host</code>
          {m.importConfig.noneAfter}
        </p>
      )}
      {hosts && hosts.length > 0 && (
        <>
          <p className="dialog-text muted">{m.importConfig.help}</p>
          <label className="field">
            <span className="label">{m.common.group}</span>
            <input maxLength={64} value={group} onChange={(e) => setGroup(e.target.value)} />
          </label>
          <div className="import-list">
            <table className="table">
              <thead>
                <tr>
                  <th>
                    <input
                      type="checkbox"
                      aria-label={m.importConfig.selectAll}
                      checked={allSelected}
                      onChange={() =>
                        setSelected(allSelected ? new Set() : new Set(hosts.map((h) => h.alias)))
                      }
                    />
                  </th>
                  <th>{m.common.name}</th>
                  <th>{m.importConfig.address}</th>
                  <th>{m.importConfig.auth}</th>
                </tr>
              </thead>
              <tbody>
                {hosts.map((h) => (
                  <tr key={h.alias}>
                    <td>
                      <input
                        type="checkbox"
                        aria-label={m.sidebar.select(h.alias)}
                        checked={selected.has(h.alias)}
                        onChange={() => toggle(h.alias)}
                      />
                    </td>
                    <td>
                      {h.alias}
                      {existing.has(h.alias) && <span className="tag">{m.importConfig.exists}</span>}
                    </td>
                    <td className="mono">
                      {h.user}@{h.host}
                      {h.port !== 22 ? `:${h.port}` : ""}
                      {h.proxyJump && <div className="muted">{m.status.via(h.proxyJump)}</div>}
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
          {m.importConfig.failures}
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
