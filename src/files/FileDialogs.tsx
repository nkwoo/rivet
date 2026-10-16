import { useState, type FormEvent } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { FolderPlusIcon, TrashIcon, WarningIcon } from "../components/Icons.tsx";
import { errorMessage, type JobInfo } from "../lib/api.ts";
import { formatDate, formatSize } from "./transfers.ts";

/** 원격 이름 한 개로 쓸 수 있는지 (백엔드도 같은 규칙으로 다시 검사) */
function nameProblem(name: string): string | null {
  if (name === "" || name === "." || name === "..") return "이름을 입력하세요.";
  if (/[/\\]/.test(name)) return "이름에 / 또는 \\를 넣을 수 없습니다.";
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(name)) return "이름에 제어 문자를 넣을 수 없습니다.";
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
            취소
          </button>
          <button type="submit" form="name-form" className="button button-primary" disabled={busy}>
            {confirm}
          </button>
        </>
      }
    >
      <form id="name-form" className="form" onSubmit={submit} autoComplete="off">
        <label className="field">
          <span className="label">이름</span>
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
      title={names.length === 1 ? `“${names[0]}” 삭제` : `${names.length}개 항목 삭제`}
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
            취소
          </button>
          <button type="button" className="button button-danger" onClick={confirm} disabled={busy}>
            삭제
          </button>
        </>
      }
    >
      <p className="dialog-text">원격 서버에서 삭제하며 되돌릴 수 없습니다. 폴더는 비어 있을 때만 삭제됩니다.</p>
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
  const [all, setAll] = useState(false);
  if (job.state.kind !== "conflict") return null;
  const where = job.direction === "upload" ? "원격" : "로컬";
  return (
    <Dialog
      title="같은 이름의 파일이 있습니다"
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <WarningIcon />
        </span>
      }
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={() => onAnswer(false, all)} data-autofocus>
            건너뛰기
          </button>
          <button type="button" className="button button-primary" onClick={() => onAnswer(true, all)}>
            덮어쓰기
          </button>
        </>
      }
    >
      <p className="dialog-text">
        <span className="mono">{job.name}</span>
      </p>
      <dl className="facts">
        <dt>{where}에 있는 파일</dt>
        <dd className="mono">
          {formatSize(job.state.existingSize)} · {formatDate(job.state.existingModified)}
        </dd>
        <dt>보낼 파일</dt>
        <dd className="mono">{formatSize(job.size)}</dd>
        <dt>대상</dt>
        <dd className="mono">{job.target}</dd>
      </dl>
      {remaining > 1 && (
        <label className="checkbox">
          <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
          <span>남은 {remaining - 1}개에도 같은 선택 적용</span>
        </label>
      )}
    </Dialog>
  );
}
