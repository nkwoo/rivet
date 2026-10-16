import type { ReactNode } from "react";
import { Dialog } from "./Dialog.tsx";

interface ConfirmDialogProps {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * 되돌릴 수 없는 작업 확인. macOS WKWebView(wry)는 `window.confirm`을 구현하지 않아
 * 항상 false가 반환되므로 앱 안의 대화상자를 씁니다. 실수로 확정하지 않도록 취소에 포커스를 둡니다.
 */
export function ConfirmDialog({ title, children, confirmLabel, onConfirm, onCancel }: ConfirmDialogProps) {
  return (
    <Dialog
      title={title}
      tone="danger"
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onCancel} data-autofocus>
            취소
          </button>
          <button type="button" className="button button-danger" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}
