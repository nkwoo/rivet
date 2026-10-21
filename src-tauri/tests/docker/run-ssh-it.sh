#!/usr/bin/env bash
# Docker OpenSSH 서버를 띄우고 SSH 통합 테스트(#[ignore])를 실행합니다.
# 사용: src-tauri/tests/docker/run-ssh-it.sh [테스트 이름 필터]
# 1GB 전송 확인: RIVET_IT_BIG_MB=1024 src-tauri/tests/docker/run-ssh-it.sh sftp_large
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
MANIFEST="$HERE/../../Cargo.toml"
PORT="${RIVET_IT_SSH_PORT:-2222}"
NAME="rivet-it-ssh"
IMAGE="rivet-it-openssh"

WORK="$(mktemp -d)"
cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

ssh-keygen -q -t ed25519 -N '' -C rivet-it -f "$WORK/id_ed25519"
chmod 644 "$WORK/id_ed25519.pub"

docker build -q -t "$IMAGE" "$HERE/openssh" >/dev/null
docker rm -f "$NAME" >/dev/null 2>&1 || true
# host.docker.internal: SSH 터널 경유 RDP 테스트에서 컨테이너가 호스트의 테스트 RDP 서버로 접속 (리눅스 포함)
docker run -d --name "$NAME" -p "127.0.0.1:$PORT:22" --add-host=host.docker.internal:host-gateway \
  -v "$WORK/id_ed25519.pub:/etc/rivet/authorized_keys:ro" "$IMAGE" >/dev/null

# sshd가 배너를 보낼 때까지 대기
for _ in $(seq 1 50); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$PORT" && head -c 4 <&3 | grep -q SSH) 2>/dev/null; then
    break
  fi
  sleep 0.2
done

RIVET_IT_SSH_PORT="$PORT" RIVET_IT_SSH_KEY="$WORK/id_ed25519" RIVET_IT_BIG_MB="${RIVET_IT_BIG_MB:-64}" \
  cargo test --manifest-path "$MANIFEST" --lib -- --ignored --test-threads=1 "${@:-ssh_integration}"
