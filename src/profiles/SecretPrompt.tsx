import { useRef, type FormEvent } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { LockIcon } from "../components/Icons.tsx";
import { useI18n } from "../i18n/index.ts";

interface SecretPromptProps {
  kind: "password" | "passphrase";
  name: string;
  target: string;
  error?: string | undefined;
  onSubmit: (secret: string, save: boolean) => void;
  onCancel: () => void;
}

/**
 * 비밀번호/패스프레이즈 입력. 값은 React 상태에 두지 않고 제출 즉시 입력란을 비웁니다.
 */
export function SecretPrompt({ kind, name, target, error, onSubmit, onCancel }: SecretPromptProps) {
  const input = useRef<HTMLInputElement>(null);
  const save = useRef<HTMLInputElement>(null);
  const { m } = useI18n();
  const label = kind === "password" ? m.secret.password : m.secret.passphrase;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const el = input.current;
    if (!el || el.value === "") return;
    const value = el.value;
    el.value = "";
    onSubmit(value, save.current?.checked ?? false);
  };

  return (
    <Dialog
      title={m.secret.title(name, label)}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <LockIcon />
        </span>
      }
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onCancel}>
            {m.common.cancel}
          </button>
          <button type="submit" form="secret-form" className="button button-primary">
            {m.common.connect}
          </button>
        </>
      }
    >
      <form id="secret-form" className="form" onSubmit={submit} autoComplete="off">
        <p className="dialog-text mono muted">{target}</p>
        <label className="field">
          <span className="label">{label}</span>
          <input ref={input} type="password" autoComplete="off" spellCheck={false} />
        </label>
        <label className="checkbox">
          <input ref={save} type="checkbox" />
          <span>{m.secret.save}</span>
        </label>
        {error && (
          <p role="alert" className="form-error">
            {error}
          </p>
        )}
      </form>
    </Dialog>
  );
}
