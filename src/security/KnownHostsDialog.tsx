import { useCallback, useEffect, useState } from "react";
import { Dialog } from "../components/Dialog.tsx";
import { ShieldAlertIcon } from "../components/Icons.tsx";
import { api, errorMessage, type KnownHost } from "../lib/api.ts";

interface KnownHostsDialogProps {
  /** 키 변경 경고에서 열었을 때 강조할 호스트 */
  highlight?: { host: string; port: number } | undefined;
  onClose: () => void;
}

export function KnownHostsDialog({ highlight, onClose }: KnownHostsDialogProps) {
  const [hosts, setHosts] = useState<KnownHost[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .listKnownHosts()
      .then(setHosts)
      .catch((e: unknown) => setError(errorMessage(e)));
  }, []);

  useEffect(load, [load]);

  const remove = async (h: KnownHost) => {
    const ok = window.confirm(
      `${h.host}:${h.port}의 저장된 호스트 키를 삭제할까요?\n다음 연결 때 새 지문을 다시 확인해야 합니다.`,
    );
    if (!ok) return;
    try {
      await api.removeKnownHost(h.host, h.port);
      load();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const isHighlighted = (h: KnownHost) =>
    highlight !== undefined && h.host === highlight.host.toLowerCase() && h.port === highlight.port;

  return (
    <Dialog
      title="저장된 호스트 키"
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
                  <button type="button" className="button button-small button-danger-outline" onClick={() => remove(h)}>
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
