import { useState, type FormEvent } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog.tsx";
import { Dialog } from "../components/Dialog.tsx";
import { WarningIcon } from "../components/Icons.tsx";
import { RivetMark } from "../RivetMark.tsx";
import { useI18n } from "../i18n/index.ts";
import {
  DEFAULT_PORTS,
  errorMessage,
  type AuthMethod,
  type Profile,
  type ProfileInput,
  type Protocol,
  type RdpSettings,
} from "../lib/api.ts";

const PROTOCOLS: Protocol[] = ["ssh", "sftp", "ftp", "ftps", "rdp"];
const AUTH_METHODS: AuthMethod[] = ["key", "password", "agent"];

const RESOLUTIONS: [number, number][] = [
  [1280, 720],
  [1366, 768],
  [1600, 900],
  [1920, 1080],
  [2560, 1440],
];
const SCALES = [100, 125, 150, 175, 200];
const DEFAULT_RDP: RdpSettings = {
  width: null,
  height: null,
  scale: null,
  hidpi: false,
  keyboard: "remote",
  hangulKey: "lang1",
};

interface ProfileDialogProps {
  /** 없으면 새 연결 */
  profile: Profile | null;
  groups: string[];
  /** 점프 호스트 후보를 고르기 위한 전체 연결 목록 */
  profiles: Profile[];
  onSave: (input: ProfileInput, connect: boolean) => Promise<void>;
  onDelete: (profile: Profile) => Promise<void>;
  onForgetSecret: (profile: Profile) => Promise<void>;
  onClose: () => void;
}

function initialInput(p: Profile | null): ProfileInput {
  if (p) {
    const { id: _id, createdAt: _c, updatedAt: _u, ...rest } = p;
    return rest;
  }
  return {
    name: "",
    protocol: "ssh",
    host: "",
    port: 22,
    username: "",
    authMethod: "key",
    keyPath: "~/.ssh/id_ed25519",
    group: null,
    tags: [],
    notes: "",
    favorite: false,
    rdp: null,
    jumpProfileId: null,
  };
}

/** 1–65535 범위의 10진수 포트만 받아들임 */
export function parsePort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text)) return null;
  const port = Number(text);
  return port >= 1 && port <= 65535 ? port : null;
}

export function ProfileDialog({
  profile,
  groups,
  profiles,
  onSave,
  onDelete,
  onForgetSecret,
  onClose,
}: ProfileDialogProps) {
  const { m } = useI18n();
  const [form, setForm] = useState<ProfileInput>(() => initialInput(profile));
  const [tagText, setTagText] = useState(form.tags.join(", "));
  // 입력 중 빈 값을 허용하려고 문자열로 두고, 저장할 때 숫자로 검증
  const [portText, setPortText] = useState(String(form.port));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const set = <K extends keyof ProfileInput>(key: K, value: ProfileInput[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const pickProtocol = (protocol: Protocol) => {
    // 기본 포트를 쓰고 있었다면 새 프로토콜의 기본 포트로 바꿈
    if (portText === String(DEFAULT_PORTS[form.protocol])) setPortText(String(DEFAULT_PORTS[protocol]));
    setForm((f) => ({
      ...f,
      protocol,
      authMethod: protocol === "ssh" || protocol === "sftp" ? f.authMethod : "password",
    }));
  };

  const isSsh = form.protocol === "ssh" || form.protocol === "sftp";
  const rdp = { ...DEFAULT_RDP, ...form.rdp };
  const setRdp = (patch: Partial<RdpSettings>) => set("rdp", { ...rdp, ...patch });

  const submit = async (e: FormEvent, connect: boolean) => {
    e.preventDefault();
    const port = parsePort(portText);
    if (port === null) {
      setError(m.profile.badPort);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const tags = tagText
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
      await onSave(
        {
          ...form,
          port,
          tags,
          keyPath: form.authMethod === "key" ? form.keyPath : null,
          jumpProfileId: isSsh || form.protocol === "rdp" ? form.jumpProfileId : null,
          rdp: form.protocol === "rdp" ? rdp : null,
        },
        connect,
      );
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  const [confirmingDelete, setConfirmingDelete] = useState(false);

  const remove = async () => {
    if (!profile) return;
    setConfirmingDelete(false);
    setBusy(true);
    try {
      await onDelete(profile);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  const forget = async () => {
    if (!profile) return;
    try {
      await onForgetSecret(profile);
      setNotice(m.profile.secretForgotten);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  // 확인 대화상자를 겹쳐 띄우면 Esc가 바깥 대화상자까지 닫으므로 잠시 바꿔 그립니다.
  // 입력 중인 값은 이 컴포넌트의 상태에 남아 있어 취소하면 그대로 돌아옵니다.
  if (profile && confirmingDelete) {
    return (
      <ConfirmDialog
        title={m.profile.deleteTitle}
        confirmLabel={m.common.delete}
        onConfirm={() => void remove()}
        onCancel={() => setConfirmingDelete(false)}
      >
        <p className="dialog-text">{m.profile.deleteConfirm(profile.name)}</p>
        <p className="dialog-text muted">{m.profile.deleteSecrets}</p>
      </ConfirmDialog>
    );
  }

  return (
    <Dialog
      title={profile ? m.profile.editTitle : m.common.newConnection}
      icon={<RivetMark size={28} />}
      onClose={onClose}
      wide
      footer={
        <>
          {profile && (
            <button type="button" className="button button-ghost button-danger" onClick={() => setConfirmingDelete(true)} disabled={busy}>
              {m.common.delete}
            </button>
          )}
          <span className="spacer" />
          <button type="button" className="button button-ghost" onClick={onClose}>
            {m.common.cancel}
          </button>
          <button type="submit" form="profile-form" className="button button-secondary" disabled={busy}>
            {m.common.save}
          </button>
          <button type="button" className="button button-primary" disabled={busy} onClick={(e) => submit(e, true)}>
            {m.profile.saveAndConnect}
          </button>
        </>
      }
    >
      <form id="profile-form" className="form" onSubmit={(e) => submit(e, false)}>
        <fieldset className="field">
          <legend className="label">{m.profile.protocol}</legend>
          <div className="segmented" role="group">
            {PROTOCOLS.map((p) => (
              <button
                key={p}
                type="button"
                aria-pressed={form.protocol === p}
                className={form.protocol === p ? "is-on" : ""}
                onClick={() => pickProtocol(p)}
              >
                {p.toUpperCase()}
              </button>
            ))}
          </div>
          {form.protocol === "rdp" && (
            <p className="hint">
              {m.profile.rdpHintBefore}
              <code className="mono">DOMAIN\user</code>
              {m.profile.rdpHintOr}
              <code className="mono">user@domain</code>
              {m.profile.rdpHintAfter}
            </p>
          )}
          {form.protocol === "ftps" && (
            <p className="hint">{m.profile.ftpsHint}</p>
          )}
        </fieldset>

        {form.protocol === "ftp" && (
          <div role="alert" className="callout callout-warn">
            <WarningIcon />
            <span>{m.profile.plainFtpWarning}</span>
          </div>
        )}

        <div className="grid-4">
          <label className="field span-4">
            <span className="label">{m.common.name}</span>
            <input required maxLength={100} value={form.name} onChange={(e) => set("name", e.target.value)} />
          </label>
          <label className="field span-3">
            <span className="label">{m.common.host}</span>
            <input
              required
              className="mono"
              maxLength={253}
              spellCheck={false}
              autoCapitalize="off"
              value={form.host}
              onChange={(e) => set("host", e.target.value)}
            />
          </label>
          <label className="field">
            <span className="label">{m.profile.port}</span>
            <input
              required
              className="mono"
              type="text"
              inputMode="numeric"
              autoComplete="off"
              maxLength={5}
              value={portText}
              onChange={(e) => setPortText(e.target.value.replace(/\D/g, ""))}
            />
          </label>
          <label className="field span-2">
            <span className="label">{m.profile.username}</span>
            <input
              className="mono"
              required={isSsh}
              placeholder={form.protocol === "ftp" || form.protocol === "ftps" ? m.profile.anonymous : undefined}
              maxLength={128}
              spellCheck={false}
              autoCapitalize="off"
              value={form.username}
              onChange={(e) => set("username", e.target.value)}
            />
          </label>
          <label className="field span-2">
            <span className="label">{m.common.group}</span>
            <input
              list="profile-groups"
              maxLength={64}
              value={form.group ?? ""}
              onChange={(e) => set("group", e.target.value || null)}
            />
            <datalist id="profile-groups">
              {groups.map((g) => (
                <option key={g} value={g} />
              ))}
            </datalist>
          </label>
        </div>

        {isSsh && (
          <>
            <fieldset className="field">
              <legend className="label">{m.profile.authLabel}</legend>
              <div className="radios">
                {AUTH_METHODS.map((method) => (
                  <label key={method} className="radio">
                    <input
                      type="radio"
                      name="auth"
                      checked={form.authMethod === method}
                      onChange={() => set("authMethod", method)}
                    />
                    {m.profile.auth[method]}
                  </label>
                ))}
              </div>
            </fieldset>
            {form.authMethod === "key" && (
              <label className="field">
                <span className="label">{m.profile.keyFile}</span>
                <input
                  className="mono"
                  required
                  spellCheck={false}
                  placeholder="~/.ssh/id_ed25519"
                  value={form.keyPath ?? ""}
                  onChange={(e) => set("keyPath", e.target.value || null)}
                />
              </label>
            )}
          </>
        )}

        {(isSsh || form.protocol === "rdp") && (
          <label className="field">
            <span className="label">{m.profile.jumpHost}</span>
            <select
              value={form.jumpProfileId ?? ""}
              onChange={(e) => set("jumpProfileId", e.target.value || null)}
            >
              <option value="">{m.profile.direct}</option>
              {profiles
                .filter((p) => (p.protocol === "ssh" || p.protocol === "sftp") && p.id !== profile?.id)
                .map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.username}@{p.host}
                    {p.port !== 22 ? `:${p.port}` : ""})
                  </option>
                ))}
            </select>
            {form.protocol === "rdp" && form.jumpProfileId && (
              <span className="hint">{m.profile.rdpJumpHint}</span>
            )}
          </label>
        )}

        {form.protocol === "rdp" && (
          <div className="grid-4">
            <label className="field span-2">
              <span className="label">{m.profile.resolution}</span>
              <select
                value={rdp.width && rdp.height ? `${rdp.width}x${rdp.height}` : "fit"}
                onChange={(e) => {
                  const [w, h] = e.target.value.split("x").map(Number);
                  setRdp(w && h ? { width: w, height: h, hidpi: false } : { width: null, height: null });
                }}
              >
                <option value="fit">{m.profile.fitResolution}</option>
                {RESOLUTIONS.map(([w, h]) => (
                  <option key={`${w}x${h}`} value={`${w}x${h}`}>
                    {w} × {h}
                  </option>
                ))}
              </select>
            </label>
            <label className="field span-2">
              <span className="label">{m.profile.scale}</span>
              <select
                value={rdp.scale === null ? "auto" : String(rdp.scale)}
                onChange={(e) => setRdp({ scale: e.target.value === "auto" ? null : Number(e.target.value) })}
              >
                <option value="auto">{m.profile.scaleAuto}</option>
                {SCALES.map((v) => (
                  <option key={v} value={v}>
                    {v}%
                  </option>
                ))}
              </select>
            </label>
            {rdp.width === null && (
              <label className="checkbox span-4">
                <input type="checkbox" checked={rdp.hidpi} onChange={(e) => setRdp({ hidpi: e.target.checked })} />
                <span>{m.profile.hidpi}</span>
              </label>
            )}
            <label className="field span-2">
              <span className="label">{m.profile.keyboard}</span>
              <select
                value={rdp.keyboard}
                onChange={(e) => setRdp({ keyboard: e.target.value === "local" ? "local" : "remote" })}
              >
                <option value="remote">{m.profile.keyboardRemote}</option>
                <option value="local">{m.profile.keyboardLocal}</option>
              </select>
            </label>
            {rdp.keyboard === "remote" && (
              <label className="field span-2">
                <span className="label">{m.profile.hangulKey}</span>
                <select
                  value={rdp.hangulKey}
                  onChange={(e) => setRdp({ hangulKey: e.target.value as RdpSettings["hangulKey"] })}
                >
                  <option value="lang1">{m.profile.hangulKeyLang1}</option>
                  <option value="rightAlt">{m.profile.hangulKeyRightAlt}</option>
                  <option value="shiftSpace">Shift+Space</option>
                </select>
              </label>
            )}
            <p className="hint span-4">
              {rdp.keyboard === "local"
                ? m.profile.keyboardLocalHint
                : m.profile.keyboardRemoteHint}
            </p>
          </div>
        )}

        <div className="grid-4">
          <label className="field span-3">
            <span className="label">{m.profile.tags}</span>
            <input value={tagText} onChange={(e) => setTagText(e.target.value)} />
          </label>
          <label className="checkbox field-inline">
            <input type="checkbox" checked={form.favorite} onChange={(e) => set("favorite", e.target.checked)} />
            {m.profile.favorite}
          </label>
        </div>

        <label className="field">
          <span className="label">{m.profile.notes}</span>
          <textarea rows={2} maxLength={4096} value={form.notes} onChange={(e) => set("notes", e.target.value)} />
        </label>

        <p className="hint">
          {m.profile.secretsHint}
          {profile && (
            <>
              {" "}
              <button type="button" className="link-button" onClick={forget}>
                {m.profile.forgetSecret}
              </button>
            </>
          )}
        </p>
        {notice && <p className="hint hint-ok">{notice}</p>}
        {error && (
          <p role="alert" className="form-error">
            {error}
          </p>
        )}
      </form>
    </Dialog>
  );
}
