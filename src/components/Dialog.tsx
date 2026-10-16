import { useEffect, useRef, type ReactNode } from "react";

interface DialogProps {
  title: string;
  /** 경고 대화상자는 alertdialog 역할과 경고 색을 사용 */
  tone?: "default" | "danger";
  icon?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  onClose?: () => void;
  wide?: boolean;
}

/** 화면 중앙 모달. Esc로 닫고, 열릴 때 첫 입력 요소에 포커스를 둡니다. */
export function Dialog({ title, tone = "default", icon, children, footer, onClose, wide }: DialogProps) {
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const el = ref.current;
    const first = el?.querySelector<HTMLElement>("input, select, textarea, button[data-autofocus]");
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <section
        ref={ref}
        role={tone === "danger" ? "alertdialog" : "dialog"}
        aria-modal="true"
        aria-label={title}
        className={`dialog ${tone === "danger" ? "dialog-danger" : ""} ${wide ? "dialog-wide" : ""}`}
      >
        <header className="dialog-head">
          {icon}
          <h2>{title}</h2>
        </header>
        <div className="dialog-body">{children}</div>
        {footer && <footer className="dialog-foot">{footer}</footer>}
      </section>
    </div>
  );
}
