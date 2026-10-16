import { Dialog } from "../components/Dialog.tsx";
import { KeyIcon, ShieldAlertIcon } from "../components/Icons.tsx";

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
  return (
    <Dialog
      title={`${name}에 처음 연결합니다`}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <KeyIcon />
        </span>
      }
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onCancel}>
            취소
          </button>
          <button type="button" className="button button-light" onClick={onTrust} data-autofocus>
            신뢰하고 연결
          </button>
        </>
      }
    >
      <p className="dialog-text">
        Rivet이 이 서버를 처음 봅니다. 서버 관리자에게 지문을 확인하거나, 서버에서{" "}
        <code className="mono">ssh-keygen -lf /etc/ssh/ssh_host_*_key.pub</code> 결과와 비교하세요.
      </p>
      <dl className="facts">
        <dt>호스트</dt>
        <dd className="mono">
          {host}:{port}
        </dd>
        <dt>키 유형</dt>
        <dd className="mono">{algorithm}</dd>
        <dt>지문</dt>
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
  return (
    <Dialog
      tone="danger"
      title={`${props.name}의 호스트 키가 바뀌었습니다`}
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <ShieldAlertIcon />
        </span>
      }
      onClose={props.onClose}
      footer={
        <>
          <button type="button" className="button button-danger-outline" onClick={props.onReview}>
            저장된 키 검토
          </button>
          <button type="button" className="button button-danger" onClick={props.onClose} data-autofocus>
            닫기
          </button>
        </>
      }
    >
      <p className="dialog-text">
        서버가 Rivet에 저장된 것과 다른 키를 보냈습니다. 서버를 다시 설치했을 수도 있지만, 누군가 연결을 가로채고 있을
        수도 있습니다. Rivet이 연결을 차단했습니다.
      </p>
      <dl className="facts">
        <dt>호스트</dt>
        <dd className="mono">
          {props.host}:{props.port}
        </dd>
        <dt>저장된 지문</dt>
        <dd className="mono fingerprint">{props.expectedFingerprint}</dd>
        <dt>받은 지문</dt>
        <dd className="mono fingerprint">
          {props.actualFingerprint} <span className="muted">({props.algorithm})</span>
        </dd>
      </dl>
      <p className="dialog-text muted">
        변경이 정상임을 서버 관리자에게 확인했다면 “저장된 키 검토”에서 기존 항목을 삭제한 뒤 다시 연결하세요.
      </p>
    </Dialog>
  );
}

const CERT_PROBLEM: Record<string, string> = {
  unknownIssuer: "운영체제가 신뢰하는 인증 기관이 발급하지 않았습니다 (자체 서명 등).",
  badName: "인증서의 이름이 접속한 호스트와 다릅니다.",
  expired: "인증서의 유효 기간이 지났습니다.",
  invalid: "운영체제가 이 인증서를 신뢰하지 않습니다.",
};

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
  return (
    <Dialog
      title={`${name}의 인증서를 확인하세요`}
      icon={
        <span className="dialog-icon dialog-icon-info">
          <KeyIcon />
        </span>
      }
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="button button-ghost" onClick={onCancel}>
            취소
          </button>
          <button type="button" className="button button-light" onClick={onTrust} data-autofocus>
            이 인증서 고정하고 연결
          </button>
        </>
      }
    >
      <p className="dialog-text">
        {CERT_PROBLEM[problem] ?? CERT_PROBLEM.invalid} 서버 관리자에게 SHA-256 지문을 확인하세요. 고정하면 다음부터는
        같은 인증서일 때만 연결하고, 인증서가 바뀌면 연결을 차단합니다.
      </p>
      <dl className="facts">
        <dt>호스트</dt>
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
  return (
    <Dialog
      tone="danger"
      title={`${props.name}의 인증서가 바뀌었습니다`}
      icon={
        <span className="dialog-icon dialog-icon-danger">
          <ShieldAlertIcon />
        </span>
      }
      onClose={props.onClose}
      footer={
        <>
          <button type="button" className="button button-danger-outline" onClick={props.onReview}>
            고정된 인증서 검토
          </button>
          <button type="button" className="button button-danger" onClick={props.onClose} data-autofocus>
            닫기
          </button>
        </>
      }
    >
      <p className="dialog-text">
        서버가 고정해 둔 것과 다른 인증서를 보냈습니다. 인증서를 갱신했을 수도 있지만, 누군가 연결을 가로채고 있을
        수도 있습니다. Rivet이 연결을 차단했습니다.
      </p>
      <dl className="facts">
        <dt>호스트</dt>
        <dd className="mono">
          {props.host}:{props.port}
        </dd>
        <dt>고정된 지문</dt>
        <dd className="mono fingerprint">{props.expectedFingerprint}</dd>
        <dt>받은 지문</dt>
        <dd className="mono fingerprint">{props.actualFingerprint}</dd>
      </dl>
      <p className="dialog-text muted">
        변경이 정상임을 서버 관리자에게 확인했다면 “고정된 인증서 검토”에서 기존 항목을 삭제한 뒤 다시 연결하세요.
      </p>
    </Dialog>
  );
}
