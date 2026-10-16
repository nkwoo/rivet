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

export interface ConnectionInfo {
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
  | ({ type: "connected" } & ConnectionInfo)
  | { type: "closed"; exitStatus: number | null }
  | { type: "error"; code: string; message: string };

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
  sshWrite: (sessionId: string, data: string) => invoke<void>("ssh_write", { sessionId, data }),
  sshResize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("ssh_resize", { sessionId, cols, rows }),
  sshClose: (sessionId: string) => invoke<void>("ssh_close", { sessionId }),
  sshHostKeyRespond: (sessionId: string, accept: boolean) =>
    invoke<void>("ssh_host_key_respond", { sessionId, accept }),
};

/** Channel로 받은 바이너리 메시지를 바이트 배열로 변환 */
export function toBytes(data: unknown): Uint8Array {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (Array.isArray(data)) return Uint8Array.from(data as number[]);
  return new Uint8Array();
}
