import { useState, type FormEvent } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog.tsx";
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
  type RdpSettings,
} from "../lib/api.ts";

const PROTOCOLS: Protocol[] = ["ssh", "sftp", "ftp", "ftps", "rdp"];
const AUTH_LABELS: Record<AuthMethod, string> = {
  key: "개인 키",
  password: "비밀번호",
  agent: "ssh-agent",
};

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
  const rdp = { ...DEFAULT_RDP, ...form.rdp };
  const setRdp = (patch: Partial<RdpSettings>) => set("rdp", { ...rdp, ...patch });

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
      await onSave(
        {
          ...form,
          port,
          tags,
          keyPath: form.authMethod === "key" ? form.keyPath : null,
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
      setNotice("키체인에 저장된 비밀 값을 삭제했습니다.");
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  // 확인 대화상자를 겹쳐 띄우면 Esc가 바깥 대화상자까지 닫으므로 잠시 바꿔 그립니다.
  // 입력 중인 값은 이 컴포넌트의 상태에 남아 있어 취소하면 그대로 돌아옵니다.
  if (profile && confirmingDelete) {
    return (
      <ConfirmDialog
        title="연결 삭제"
        confirmLabel="삭제"
        onConfirm={() => void remove()}
        onCancel={() => setConfirmingDelete(false)}
      >
        <p className="dialog-text">“{profile.name}” 연결을 삭제할까요?</p>
        <p className="dialog-text muted">키체인에 저장된 비밀번호도 함께 삭제됩니다.</p>
      </ConfirmDialog>
    );
  }

  return (
    <Dialog
      title={profile ? "연결 편집" : "새 연결"}
      icon={<RivetMark size={28} />}
      onClose={onClose}
      wide
      footer={
        <>
          {profile && (
            <button type="button" className="button button-ghost button-danger" onClick={() => setConfirmingDelete(true)} disabled={busy}>
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
          <button type="button" className="button button-primary" disabled={busy} onClick={(e) => submit(e, true)}>
            저장 후 연결
          </button>
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
          {form.protocol === "rdp" && (
            <p className="hint">
              NLA(CredSSP)로 인증합니다. 사용자명은 <code className="mono">DOMAIN\user</code> 또는{" "}
              <code className="mono">user@domain</code> 형식도 됩니다. 서버 인증서는 지문을 확인한 뒤 고정합니다.
            </p>
          )}
          {form.protocol === "ftps" && (
            <p className="hint">
              포트 990은 암시적 TLS, 그 밖의 포트(보통 21)는 AUTH TLS로 접속합니다. 운영체제가 신뢰하지 않는 인증서는
              지문을 확인한 뒤 고정합니다.
            </p>
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
              placeholder={form.protocol === "ftp" || form.protocol === "ftps" ? "비우면 anonymous" : undefined}
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

        {form.protocol === "rdp" && (
          <div className="grid-4">
            <label className="field span-2">
              <span className="label">해상도</span>
              <select
                value={rdp.width && rdp.height ? `${rdp.width}x${rdp.height}` : "fit"}
                onChange={(e) => {
                  const [w, h] = e.target.value.split("x").map(Number);
                  setRdp(w && h ? { width: w, height: h, hidpi: false } : { width: null, height: null });
                }}
              >
                <option value="fit">창에 맞춤 (탭 크기를 따라 바뀜)</option>
                {RESOLUTIONS.map(([w, h]) => (
                  <option key={`${w}x${h}`} value={`${w}x${h}`}>
                    {w} × {h}
                  </option>
                ))}
              </select>
            </label>
            <label className="field span-2">
              <span className="label">원격 화면 배율</span>
              <select
                value={rdp.scale === null ? "auto" : String(rdp.scale)}
                onChange={(e) => setRdp({ scale: e.target.value === "auto" ? null : Number(e.target.value) })}
              >
                <option value="auto">자동</option>
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
                <span>고해상도(Retina) 디스플레이의 실제 픽셀로 접속 (선명하지만 전송량이 늘어납니다)</span>
              </label>
            )}
            <label className="field span-2">
              <span className="label">키보드 입력 방식</span>
              <select
                value={rdp.keyboard}
                onChange={(e) => setRdp({ keyboard: e.target.value === "local" ? "local" : "remote" })}
              >
                <option value="remote">원격 입력기 (한/영은 원격에서 전환)</option>
                <option value="local">맥 입력기 (한/영은 맥에서 전환)</option>
              </select>
            </label>
            {rdp.keyboard === "remote" && (
              <label className="field span-2">
                <span className="label">원격 한/영 전환 키</span>
                <select
                  value={rdp.hangulKey}
                  onChange={(e) => setRdp({ hangulKey: e.target.value as RdpSettings["hangulKey"] })}
                >
                  <option value="lang1">한/영 키</option>
                  <option value="rightAlt">오른쪽 Alt</option>
                  <option value="shiftSpace">Shift+Space</option>
                </select>
              </label>
            )}
            <p className="hint span-4">
              {rdp.keyboard === "local"
                ? "맥 입력기가 글자를 완성한 뒤 원격에 보냅니다. 조합 중인 글자는 마지막으로 클릭한 위치에 미리 보여 줍니다."
                : "키를 그대로 보내 Windows 입력기가 조합하므로 자음·모음이 바로 보입니다. 원격 한/영 전환은 툴바의 한/영 버튼이나 오른쪽 ⌘로 합니다. Windows 입력기 설정에 맞는 전환 키를 고르세요."}
            </p>
          </div>
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
