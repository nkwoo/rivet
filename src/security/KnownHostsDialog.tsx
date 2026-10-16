import { useCallback, useEffect, useState } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog.tsx";
import { Dialog } from "../components/Dialog.tsx";
import { ShieldAlertIcon } from "../components/Icons.tsx";
import { api, errorMessage, type CertPin, type KnownHost } from "../lib/api.ts";

interface KnownHostsDialogProps {
  /** 키/인증서 변경 경고에서 열었을 때 강조할 호스트 */
  highlight?: { host: string; port: number } | undefined;
  onClose: () => void;
}

/** 삭제 확인 중인 항목 */
type Pending = { kind: "host" | "pin"; host: string; port: number };

/** 신뢰한 SSH 호스트 키와 고정한 TLS 인증서 관리 */
export function KnownHostsDialog({ highlight, onClose }: KnownHostsDialogProps) {
  const [hosts, setHosts] = useState<KnownHost[] | null>(null);
  const [pins, setPins] = useState<CertPin[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .listKnownHosts()
      .then(setHosts)
      .catch((e: unknown) => setError(errorMessage(e)));
    api
      .listCertPins()
      .then(setPins)
      .catch((e: unknown) => setError(errorMessage(e)));
  }, []);

  useEffect(load, [load]);

  const [pending, setPending] = useState<Pending | null>(null);

  const remove = async (target: Pending) => {
    setPending(null);
    try {
      if (target.kind === "host") await api.removeKnownHost(target.host, target.port);
      else await api.removeCertPin(target.host, target.port);
      load();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const isHighlighted = (h: { host: string; port: number }) =>
    highlight !== undefined && h.host === highlight.host.toLowerCase() && h.port === highlight.port;

  if (pending) {
    const what = pending.kind === "host" ? "저장된 호스트 키" : "고정한 인증서";
    return (
      <ConfirmDialog
        title={`${what} 삭제`}
        confirmLabel="삭제"
        onConfirm={() => void remove(pending)}
        onCancel={() => setPending(null)}
      >
        <p className="dialog-text">
          <span className="mono">
            {pending.host}:{pending.port}
          </span>
          의 {what}를 삭제할까요?
        </p>
        <p className="dialog-text muted">
          다음 연결 때 {pending.kind === "host" ? "새 지문" : "인증서 지문"}을 다시 확인해야 합니다.
        </p>
      </ConfirmDialog>
    );
  }

  return (
    <Dialog
      title="저장된 호스트 키와 인증서"
      icon={
        <span className="dialog-icon dialog-icon-info">
          <ShieldAlertIcon />
        </span>
      }
      onClose={onClose}
      wide
      footer={
        <button type="button" className="button button-ghost" onClick={onClose} data-autofocus>
          닫기
        </button>
      }
    >
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      <h3 className="dialog-section">SSH 호스트 키</h3>
      {hosts === null ? (
        <p className="muted">불러오는 중…</p>
      ) : hosts.length === 0 ? (
        <p className="muted">아직 신뢰한 서버가 없습니다.</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>호스트</th>
              <th>키</th>
              <th>저장일</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {hosts.map((h) => (
              <tr key={`${h.host}:${h.port}`} className={isHighlighted(h) ? "row-warn" : ""}>
                <td className="mono">
                  {h.host}:{h.port}
                </td>
                <td className="mono">
                  <div>{h.algorithm}</div>
                  <div className="fingerprint muted">{h.fingerprint}</div>
                </td>
                <td>{new Date(h.addedAt * 1000).toLocaleDateString()}</td>
                <td>
                  <button
                    type="button"
                    className="button button-small button-danger-outline"
                    onClick={() => setPending({ kind: "host", host: h.host, port: h.port })}
                  >
                    삭제
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3 className="dialog-section">고정한 TLS 인증서 (FTPS)</h3>
      {pins === null ? (
        <p className="muted">불러오는 중…</p>
      ) : pins.length === 0 ? (
        <p className="muted">고정한 인증서가 없습니다. 운영체제가 신뢰하는 인증서는 고정하지 않습니다.</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>호스트</th>
              <th>SHA-256</th>
              <th>저장일</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {pins.map((p) => (
              <tr key={`${p.host}:${p.port}`} className={isHighlighted(p) ? "row-warn" : ""}>
                <td className="mono">
                  {p.host}:{p.port}
                </td>
                <td className="mono fingerprint muted">{p.fingerprint}</td>
                <td>{new Date(p.addedAt * 1000).toLocaleDateString()}</td>
                <td>
                  <button
                    type="button"
                    className="button button-small button-danger-outline"
                    onClick={() => setPending({ kind: "pin", host: p.host, port: p.port })}
                  >
                    삭제
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Dialog>
  );
}
