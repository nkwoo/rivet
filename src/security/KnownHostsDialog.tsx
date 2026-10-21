import { useCallback, useEffect, useState } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog.tsx";
import { Dialog } from "../components/Dialog.tsx";
import { ShieldAlertIcon } from "../components/Icons.tsx";
import { api, errorMessage, type CertPin, type KnownHost } from "../lib/api.ts";
import { useI18n } from "../i18n/index.ts";

interface KnownHostsDialogProps {
  /** 키/인증서 변경 경고에서 열었을 때 강조할 호스트 */
  highlight?: { host: string; port: number } | undefined;
  onClose: () => void;
}

/** 삭제 확인 중인 항목 */
type Pending = { kind: "host" | "pin"; host: string; port: number };

/** 신뢰한 SSH 호스트 키와 고정한 TLS 인증서 관리 */
export function KnownHostsDialog({ highlight, onClose }: KnownHostsDialogProps) {
  const { m, locale } = useI18n();
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
    const host = pending.kind === "host";
    return (
      <ConfirmDialog
        title={host ? m.knownHosts.deleteHostTitle : m.knownHosts.deletePinTitle}
        confirmLabel={m.common.delete}
        onConfirm={() => void remove(pending)}
        onCancel={() => setPending(null)}
      >
        <p className="dialog-text">
          {(host ? m.knownHosts.deleteHostConfirm : m.knownHosts.deletePinConfirm)[0]}
          <span className="mono">
            {pending.host}:{pending.port}
          </span>
          {(host ? m.knownHosts.deleteHostConfirm : m.knownHosts.deletePinConfirm)[1]}
        </p>
        <p className="dialog-text muted">{host ? m.knownHosts.deleteHostHint : m.knownHosts.deletePinHint}</p>
      </ConfirmDialog>
    );
  }

  return (
    <Dialog
      title={m.app.knownHosts}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <ShieldAlertIcon />
        </span>
      }
      onClose={onClose}
      wide
      footer={
        <button type="button" className="button button-ghost" onClick={onClose} data-autofocus>
          {m.common.close}
        </button>
      }
    >
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      <h3 className="dialog-section">{m.knownHosts.sshKeys}</h3>
      {hosts === null ? (
        <p className="muted">{m.common.loading}</p>
      ) : hosts.length === 0 ? (
        <p className="muted">{m.knownHosts.noHosts}</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>{m.common.host}</th>
              <th>{m.knownHosts.key}</th>
              <th>{m.knownHosts.added}</th>
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
                <td>{new Date(h.addedAt * 1000).toLocaleDateString(locale)}</td>
                <td>
                  <button
                    type="button"
                    className="button button-small button-danger-outline"
                    onClick={() => setPending({ kind: "host", host: h.host, port: h.port })}
                  >
                    {m.common.delete}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3 className="dialog-section">{m.knownHosts.pins}</h3>
      {pins === null ? (
        <p className="muted">{m.common.loading}</p>
      ) : pins.length === 0 ? (
        <p className="muted">{m.knownHosts.noPins}</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>{m.common.host}</th>
              <th>SHA-256</th>
              <th>{m.knownHosts.added}</th>
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
                <td>{new Date(p.addedAt * 1000).toLocaleDateString(locale)}</td>
                <td>
                  <button
                    type="button"
                    className="button button-small button-danger-outline"
                    onClick={() => setPending({ kind: "pin", host: p.host, port: p.port })}
                  >
                    {m.common.delete}
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
