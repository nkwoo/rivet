import { Channel, invoke } from "@tauri-apps/api/core";

export type Protocol = "ssh" | "sftp" | "ftp" | "ftps" | "rdp";
export type AuthMethod = "password" | "key" | "agent";

/** 저장된 연결 프로필. 비밀 값은 포함하지 않습니다. */
export interface Profile {
  id: string;
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  group: string | null;
  tags: string[];
  notes: string;
  favorite: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface ProfileInput {
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  authMethod: AuthMethod;
  keyPath: string | null;
  group: string | null;
  tags: string[];
  notes: string;
  favorite: boolean;
}

export interface KnownHost {
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  addedAt: number;
}

export interface CertPin {
  host: string;
  port: number;
  fingerprint: string;
  addedAt: number;
}

/** 보안 상태: SSH, 인증 기관이 검증한 TLS, 고정 지문 TLS, 암호화 없음 */
export type Transport = "ssh" | "tlsCa" | "tlsPinned" | "plain";

export interface ConnectionInfo {
  transport: Transport;
  kex: string;
  cipher: string;
  hostKeyAlgorithm: string;
  fingerprint: string;
}

export type SessionEvent =
  | { type: "hostKeyPrompt"; host: string; port: number; algorithm: string; fingerprint: string }
  | {
      type: "hostKeyChanged";
      host: string;
      port: number;
      algorithm: string;
      expectedFingerprint: string;
      actualFingerprint: string;
    }
  | { type: "certPrompt"; host: string; port: number; fingerprint: string; problem: string }
  | { type: "certChanged"; host: string; port: number; expectedFingerprint: string; actualFingerprint: string }
  | ({ type: "connected" } & ConnectionInfo)
  | { type: "closed"; exitStatus: number | null }
  | { type: "error"; code: string; message: string };

export type EntryKind = "file" | "dir" | "symlink" | "other";

/** 파일 탐색기 한 줄 (로컬/원격 공통) */
export interface FsEntry {
  name: string;
  kind: EntryKind;
  size: number;
  /** Unix 초 */
  modified: number | null;
  permissions: number | null;
}

export interface Listing {
  path: string;
  parent: string | null;
  entries: FsEntry[];
  /** 항목이 너무 많아 일부만 담김 */
  truncated: boolean;
}

export type Direction = "upload" | "download";
export type OverwritePolicy = "ask" | "overwrite" | "skip" | "newer";
export type ConflictAction = "overwrite" | "skip";

export type JobState =
  | { kind: "queued" }
  | { kind: "running" }
  | { kind: "conflict"; existingSize: number; existingModified: number | null }
  | { kind: "done" }
  | { kind: "skipped"; reason: string }
  | { kind: "failed"; code: string; message: string; retryable: boolean }
  | { kind: "cancelled" };

export interface JobInfo {
  id: string;
  sessionId: string;
  direction: Direction;
  name: string;
  source: string;
  target: string;
  size: number;
  transferred: number;
  attempt: number;
  state: JobState;
}

export type TransferEvent =
  | { type: "job"; job: JobInfo }
  | { type: "progress"; id: string; transferred: number; bytesPerSec: number }
  | { type: "removed"; ids: string[] };

/** 백엔드 `CommandError` */
export interface ApiError {
  code: string;
  message: string;
}

export function isApiError(e: unknown): e is ApiError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isApiError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

export const DEFAULT_PORTS: Record<Protocol, number> = {
  ssh: 22,
  sftp: 22,
  ftp: 21,
  ftps: 990,
  rdp: 3389,
};

export const api = {
  listProfiles: () => invoke<Profile[]>("profile_list"),
  createProfile: (input: ProfileInput) => invoke<Profile>("profile_create", { input }),
  updateProfile: (id: string, input: ProfileInput) =>
    invoke<Profile>("profile_update", { id, input }),
  deleteProfile: (id: string) => invoke<void>("profile_delete", { id }),
  forgetSecret: (id: string) => invoke<void>("profile_forget_secret", { id }),

  listKnownHosts: () => invoke<KnownHost[]>("known_hosts_list"),
  listCertPins: () => invoke<CertPin[]>("cert_pins_list"),
  removeCertPin: (host: string, port: number) => invoke<void>("cert_pins_remove", { host, port }),
  removeKnownHost: (host: string, port: number) =>
    invoke<void>("known_hosts_remove", { host, port }),

  sshConnect: (args: {
    profileId: string;
    cols: number;
    rows: number;
    secret: string | null;
    saveSecret: boolean;
    events: Channel<SessionEvent>;
    output: Channel<ArrayBuffer>;
  }) => invoke<string>("ssh_connect", args),
  ftpConnect: (args: {
    profileId: string;
    secret: string | null;
    saveSecret: boolean;
    events: Channel<SessionEvent>;
  }) => invoke<string>("ftp_connect", args),
  sshWrite: (sessionId: string, data: string) => invoke<void>("ssh_write", { sessionId, data }),
  sshResize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("ssh_resize", { sessionId, cols, rows }),
  sshClose: (sessionId: string) => invoke<void>("ssh_close", { sessionId }),
  sshHostKeyRespond: (sessionId: string, accept: boolean) =>
    invoke<void>("ssh_host_key_respond", { sessionId, accept }),

  localHome: () => invoke<string>("local_home"),
  localList: (path: string) => invoke<Listing>("local_list", { path }),
  sftpOpen: (sessionId: string) => invoke<string>("sftp_open", { sessionId }),
  sftpList: (sessionId: string, path: string) => invoke<Listing>("sftp_list", { sessionId, path }),
  sftpMkdir: (sessionId: string, dir: string, name: string) =>
    invoke<void>("sftp_mkdir", { sessionId, dir, name }),
  sftpRename: (sessionId: string, dir: string, from: string, to: string) =>
    invoke<void>("sftp_rename", { sessionId, dir, from, to }),
  sftpRemove: (sessionId: string, dir: string, name: string, isDir: boolean) =>
    invoke<void>("sftp_remove", { sessionId, dir, name, isDir }),

  transferSubscribe: (events: Channel<TransferEvent>) =>
    invoke<JobInfo[]>("transfer_subscribe", { events }),
  transferUpload: (sessionId: string, sources: string[], targetDir: string, policy: OverwritePolicy) =>
    invoke<JobInfo[]>("transfer_upload", { sessionId, sources, targetDir, policy }),
  transferDownload: (sessionId: string, sources: string[], destDir: string, policy: OverwritePolicy) =>
    invoke<JobInfo[]>("transfer_download", { sessionId, sources, destDir, policy }),
  transferResolve: (jobId: string, action: ConflictAction) =>
    invoke<void>("transfer_resolve", { jobId, action }),
  transferCancel: (jobId: string) => invoke<void>("transfer_cancel", { jobId }),
  transferRetry: (jobId: string) => invoke<void>("transfer_retry", { jobId }),
  transferClearFinished: () => invoke<void>("transfer_clear_finished"),
};

/** Channel로 받은 바이너리 메시지를 바이트 배열로 변환 */
export function toBytes(data: unknown): Uint8Array {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (Array.isArray(data)) return Uint8Array.from(data as number[]);
  return new Uint8Array();
}

/** 파일 화면/상태 표시줄에 쓰는 프로토콜 이름 */
export function protocolLabel(protocol: Protocol): string {
  return protocol.toUpperCase();
}
