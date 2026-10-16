import type { Protocol } from "../lib/api.ts";

export function ProtocolBadge({ protocol }: { protocol: Protocol }) {
  return <span className={`badge badge-${protocol}`}>{protocol.toUpperCase()}</span>;
}
