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
