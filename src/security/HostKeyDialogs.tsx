import { Dialog } from "../components/Dialog.tsx";
import { KeyIcon, ShieldAlertIcon } from "../components/Icons.tsx";
import { useI18n } from "../i18n/index.ts";

interface HostKeyPromptProps {
  name: string;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  onTrust: () => void;
  onCancel: () => void;
}

/** 처음 보는 서버: 지문을 보여주고 사용자 승인을 받음 */
export function HostKeyPrompt({ name, host, port, algorithm, fingerprint, onTrust, onCancel }: HostKeyPromptProps) {
  const { m } = useI18n();
  return (
    <Dialog
      title={m.hostKey.firstTitle(name)}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <KeyIcon />
        </span>
      }
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onCancel}>
            {m.common.cancel}
          </button>
          <button type="button" className="button button-light" onClick={onTrust} data-autofocus>
            {m.hostKey.trust}
          </button>
        </>
      }
    >
      <p className="dialog-text">
        {m.hostKey.firstBefore}
        <code className="mono">ssh-keygen -lf /etc/ssh/ssh_host_*_key.pub</code>
        {m.hostKey.firstAfter}
      </p>
      <dl className="facts">
        <dt>{m.common.host}</dt>
        <dd className="mono">
          {host}:{port}
        </dd>
        <dt>{m.hostKey.keyType}</dt>
        <dd className="mono">{algorithm}</dd>
        <dt>{m.hostKey.fingerprint}</dt>
        <dd className="mono fingerprint">{fingerprint}</dd>
      </dl>
    </Dialog>
  );
}

interface HostKeyChangedProps {
  name: string;
  host: string;
  port: number;
  algorithm: string;
  expectedFingerprint: string;
  actualFingerprint: string;
  onReview: () => void;
  onClose: () => void;
}

/** 저장된 키와 다른 키: 연결을 차단했음을 알림. 무시하고 연결하는 선택지는 없음 */
export function HostKeyChanged(props: HostKeyChangedProps) {
  const { m } = useI18n();
  return (
    <Dialog
      tone="danger"
      title={m.hostKey.changedTitle(props.name)}
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <ShieldAlertIcon />
        </span>
      }
      onClose={props.onClose}
      footer={
        <>
          <button type="button" className="button button-danger-outline" onClick={props.onReview}>
            {m.hostKey.review}
          </button>
          <button type="button" className="button button-danger" onClick={props.onClose} data-autofocus>
            {m.common.close}
          </button>
        </>
      }
    >
      <p className="dialog-text">{m.hostKey.changedBody}</p>
      <dl className="facts">
        <dt>{m.common.host}</dt>
        <dd className="mono">
          {props.host}:{props.port}
        </dd>
        <dt>{m.hostKey.savedFingerprint}</dt>
        <dd className="mono fingerprint">{props.expectedFingerprint}</dd>
        <dt>{m.hostKey.receivedFingerprint}</dt>
        <dd className="mono fingerprint">
          {props.actualFingerprint} <span className="muted">({props.algorithm})</span>
        </dd>
      </dl>
      <p className="dialog-text muted">{m.hostKey.changedHint}</p>
    </Dialog>
  );
}

interface CertPromptProps {
  name: string;
  host: string;
  port: number;
  fingerprint: string;
  problem: string;
  onTrust: () => void;
  onCancel: () => void;
}

/** 운영체제가 신뢰하지 않는 TLS 인증서: 지문을 보여주고 고정할지 묻습니다 */
export function CertPrompt({ name, host, port, fingerprint, problem, onTrust, onCancel }: CertPromptProps) {
  const { m } = useI18n();
  return (
    <Dialog
      title={m.cert.promptTitle(name)}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <KeyIcon />
        </span>
      }
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onCancel}>
            {m.common.cancel}
          </button>
          <button type="button" className="button button-light" onClick={onTrust} data-autofocus>
            {m.cert.pin}
          </button>
        </>
      }
    >
      <p className="dialog-text">
        {m.cert.problem[problem] ?? m.cert.problem.invalid} {m.cert.promptBody}
      </p>
      <dl className="facts">
        <dt>{m.common.host}</dt>
        <dd className="mono">
          {host}:{port}
        </dd>
        <dt>SHA-256</dt>
        <dd className="mono fingerprint">{fingerprint}</dd>
      </dl>
    </Dialog>
  );
}

interface CertChangedProps {
  name: string;
  host: string;
  port: number;
  expectedFingerprint: string;
  actualFingerprint: string;
  onReview: () => void;
  onClose: () => void;
}

/** 고정한 인증서와 다름: 연결을 차단했음을 알림. 무시하고 연결하는 선택지는 없음 */
export function CertChanged(props: CertChangedProps) {
  const { m } = useI18n();
  return (
    <Dialog
      tone="danger"
      title={m.cert.changedTitle(props.name)}
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <ShieldAlertIcon />
        </span>
      }
      onClose={props.onClose}
      footer={
        <>
          <button type="button" className="button button-danger-outline" onClick={props.onReview}>
            {m.cert.review}
          </button>
          <button type="button" className="button button-danger" onClick={props.onClose} data-autofocus>
            {m.common.close}
          </button>
        </>
      }
    >
      <p className="dialog-text">{m.cert.changedBody}</p>
      <dl className="facts">
        <dt>{m.common.host}</dt>
        <dd className="mono">
          {props.host}:{props.port}
        </dd>
        <dt>{m.cert.pinnedFingerprint}</dt>
        <dd className="mono fingerprint">{props.expectedFingerprint}</dd>
        <dt>{m.hostKey.receivedFingerprint}</dt>
        <dd className="mono fingerprint">{props.actualFingerprint}</dd>
      </dl>
      <p className="dialog-text muted">{m.cert.changedHint}</p>
    </Dialog>
  );
}
