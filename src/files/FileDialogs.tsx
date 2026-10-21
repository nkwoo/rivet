import { useState, type FormEvent } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { FolderPlusIcon, TrashIcon, WarningIcon } from "../components/Icons.tsx";
import { errorMessage, type JobInfo } from "../lib/api.ts";
import { formatDate, formatSize } from "./transfers.ts";
import { messages, useI18n } from "../i18n/index.ts";

/** 원격 이름 한 개로 쓸 수 있는지 (백엔드도 같은 규칙으로 다시 검사) */
function nameProblem(name: string): string | null {
  const m = messages().files;
  if (name === "" || name === "." || name === "..") return m.nameEmpty;
  if (/[/\\]/.test(name)) return m.nameSlash;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(name)) return m.nameControl;
  return null;
}

interface NameDialogProps {
  title: string;
  initial: string;
  confirm: string;
  onSubmit: (name: string) => Promise<void>;
  onClose: () => void;
}

export function NameDialog({ title, initial, confirm, onSubmit, onClose }: NameDialogProps) {
  const { m } = useI18n();
  const [name, setName] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmed = name.trim();
    const problem = nameProblem(trimmed);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    try {
      await onSubmit(trimmed);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={title}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <FolderPlusIcon />
        </span>
      }
      onClose={onClose}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onClose}>
            {m.common.cancel}
          </button>
          <button type="submit" form="name-form" className="button button-primary" disabled={busy}>
            {confirm}
          </button>
        </>
      }
    >
      <form id="name-form" className="form" onSubmit={submit} autoComplete="off">
        <label className="field">
          <span className="label">{m.common.name}</span>
          <input className="mono" value={name} spellCheck={false} onChange={(e) => setName(e.target.value)} />
        </label>
        {error && <p className="form-error">{error}</p>}
      </form>
    </Dialog>
  );
}

interface DeleteDialogProps {
  names: string[];
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function DeleteDialog({ names, onConfirm, onClose }: DeleteDialogProps) {
  const { m } = useI18n();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const confirm = async () => {
    setBusy(true);
    try {
      await onConfirm();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={names.length === 1 ? m.files.deleteOne(names[0] ?? "") : m.files.deleteMany(names.length)}
      tone="danger"
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <TrashIcon />
        </span>
      }
      onClose={onClose}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onClose} data-autofocus>
            {m.common.cancel}
          </button>
          <button type="button" className="button button-danger" onClick={confirm} disabled={busy}>
            {m.common.delete}
          </button>
        </>
      }
    >
      <p className="dialog-text">{m.files.deleteBody}</p>
      {error && <p className="form-error">{error}</p>}
    </Dialog>
  );
}

interface ConflictDialogProps {
  job: JobInfo;
  remaining: number;
  onAnswer: (overwrite: boolean, applyToAll: boolean) => void;
}

/** 같은 이름이 있을 때 (정책이 "매번 묻기"인 경우) */
export function ConflictDialog({ job, remaining, onAnswer }: ConflictDialogProps) {
  const { m } = useI18n();
  const [all, setAll] = useState(false);
  if (job.state.kind !== "conflict") return null;
  return (
    <Dialog
      title={m.files.conflictTitle}
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <WarningIcon />
        </span>
      }
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={() => onAnswer(false, all)} data-autofocus>
            {m.files.skip}
          </button>
          <button type="button" className="button button-primary" onClick={() => onAnswer(true, all)}>
            {m.files.overwrite}
          </button>
        </>
      }
    >
      <p className="dialog-text">
        <span className="mono">{job.name}</span>
      </p>
      <dl className="facts">
        <dt>{job.direction === "upload" ? m.files.existingRemote : m.files.existingLocal}</dt>
        <dd className="mono">
          {formatSize(job.state.existingSize)} · {formatDate(job.state.existingModified)}
        </dd>
        <dt>{m.files.incoming}</dt>
        <dd className="mono">{formatSize(job.size)}</dd>
        <dt>{m.files.target}</dt>
        <dd className="mono">{job.target}</dd>
      </dl>
      {remaining > 1 && (
        <label className="checkbox">
          <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
          <span>{m.files.applyToRest(remaining - 1)}</span>
        </label>
      )}
    </Dialog>
  );
}
