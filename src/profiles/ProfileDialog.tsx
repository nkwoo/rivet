import { useState, type FormEvent } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { WarningIcon } from "../components/Icons.tsx";
import { RivetMark } from "../RivetMark.tsx";
import {
  DEFAULT_PORTS,
  errorMessage,
  type AuthMethod,
  type Profile,
  type ProfileInput,
  type Protocol,
} from "../lib/api.ts";

const PROTOCOLS: Protocol[] = ["ssh", "sftp", "ftp", "ftps", "rdp"];
const AUTH_LABELS: Record<AuthMethod, string> = {
  key: "개인 키",
  password: "비밀번호",
  agent: "ssh-agent",
};

interface ProfileDialogProps {
  /** 없으면 새 연결 */
  profile: Profile | null;
  groups: string[];
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
  };
}

/** 1–65535 범위의 10진수 포트만 받아들임 */
export function parsePort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text)) return null;
  const port = Number(text);
  return port >= 1 && port <= 65535 ? port : null;
}

export function ProfileDialog({ profile, groups, onSave, onDelete, onForgetSecret, onClose }: ProfileDialogProps) {
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

  const submit = async (e: FormEvent, connect: boolean) => {
    e.preventDefault();
    const port = parsePort(portText);
    if (port === null) {
      setError("포트는 1에서 65535 사이의 숫자여야 합니다.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const tags = tagText
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
      await onSave({ ...form, port, tags, keyPath: form.authMethod === "key" ? form.keyPath : null }, connect);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!profile) return;
    if (!window.confirm(`“${profile.name}” 연결을 삭제할까요? 키체인에 저장된 비밀번호도 함께 삭제됩니다.`)) return;
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
      setNotice("키체인에 저장된 비밀 값을 삭제했습니다.");
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <Dialog
      title={profile ? "연결 편집" : "새 연결"}
      icon={<RivetMark size={28} />}
      onClose={onClose}
      wide
      footer={
        <>
          {profile && (
            <button type="button" className="button button-ghost button-danger" onClick={remove} disabled={busy}>
              삭제
            </button>
          )}
          <span className="spacer" />
          <button type="button" className="button button-ghost" onClick={onClose}>
            취소
          </button>
          <button type="submit" form="profile-form" className="button button-secondary" disabled={busy}>
            저장
          </button>
          {form.protocol === "ssh" && (
            <button type="button" className="button button-primary" disabled={busy} onClick={(e) => submit(e, true)}>
              저장 후 연결
            </button>
          )}
        </>
      }
    >
      <form id="profile-form" className="form" onSubmit={(e) => submit(e, false)}>
        <fieldset className="field">
          <legend className="label">프로토콜</legend>
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
          {form.protocol !== "ssh" && (
            <p className="hint">현재 버전은 SSH 터미널만 연결할 수 있습니다. 이 프로필은 저장해 두었다가 이후 버전에서 사용할 수 있습니다.</p>
          )}
        </fieldset>

        {form.protocol === "ftp" && (
          <div role="alert" className="callout callout-warn">
            <WarningIcon />
            <span>평문 FTP는 비밀번호와 파일을 암호화하지 않고 전송합니다. 서버가 지원하면 FTPS나 SFTP를 사용하세요.</span>
          </div>
        )}

        <div className="grid-4">
          <label className="field span-4">
            <span className="label">이름</span>
            <input required maxLength={100} value={form.name} onChange={(e) => set("name", e.target.value)} />
          </label>
          <label className="field span-3">
            <span className="label">호스트</span>
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
            <span className="label">포트</span>
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
            <span className="label">사용자명</span>
            <input
              className="mono"
              required={isSsh}
              maxLength={128}
              spellCheck={false}
              autoCapitalize="off"
              value={form.username}
              onChange={(e) => set("username", e.target.value)}
            />
          </label>
          <label className="field span-2">
            <span className="label">그룹</span>
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
              <legend className="label">인증</legend>
              <div className="radios">
                {(Object.keys(AUTH_LABELS) as AuthMethod[]).map((m) => (
                  <label key={m} className="radio">
                    <input
                      type="radio"
                      name="auth"
                      checked={form.authMethod === m}
                      onChange={() => set("authMethod", m)}
                    />
                    {AUTH_LABELS[m]}
                  </label>
                ))}
              </div>
            </fieldset>
            {form.authMethod === "key" && (
              <label className="field">
                <span className="label">키 파일</span>
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

        <div className="grid-4">
          <label className="field span-3">
            <span className="label">태그 (쉼표로 구분)</span>
            <input value={tagText} onChange={(e) => setTagText(e.target.value)} />
          </label>
          <label className="checkbox field-inline">
            <input type="checkbox" checked={form.favorite} onChange={(e) => set("favorite", e.target.checked)} />
            즐겨찾기
          </label>
        </div>

        <label className="field">
          <span className="label">메모</span>
          <textarea rows={2} maxLength={4096} value={form.notes} onChange={(e) => set("notes", e.target.value)} />
        </label>

        <p className="hint">
          비밀번호와 키 패스프레이즈는 연결할 때 입력받으며, 저장을 선택하면 시스템 키체인(macOS 키체인 / Windows 자격 증명
          관리자)에만 보관합니다. Rivet은 비밀 값을 자체 파일에 기록하지 않습니다.
          {profile && (
            <>
              {" "}
              <button type="button" className="link-button" onClick={forget}>
                저장된 비밀 값 삭제
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
