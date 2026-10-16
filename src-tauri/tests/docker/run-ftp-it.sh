#!/usr/bin/env bash
# Docker vsftpd 서버를 띄우고 FTP/FTPS 통합 테스트(#[ignore])를 실행합니다.
# 사용: src-tauri/tests/docker/run-ftp-it.sh [테스트 이름 필터]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
MANIFEST="$HERE/../../Cargo.toml"
PORT="${RIVET_IT_FTP_PORT:-2121}"
# 앱은 FTPS 포트가 990이면 암시적 TLS로 판단하므로 같은 번호로 연결합니다.
FTPS_PORT=990
NAME="rivet-it-ftp"
IMAGE="rivet-it-vsftpd"

cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker build -q -t "$IMAGE" "$HERE/vsftpd" >/dev/null
docker rm -f "$NAME" >/dev/null 2>&1 || true
# 패시브 데이터 포트는 같은 번호로 연결해야 클라이언트가 그대로 접속할 수 있습니다.
docker run -d --name "$NAME" \
  -p "127.0.0.1:$PORT:21" -p "127.0.0.1:$FTPS_PORT:990" \
  -p "127.0.0.1:21100-21119:21100-21119" "$IMAGE" >/dev/null

# 두 인스턴스가 연결을 받을 때까지 대기
for _ in $(seq 1 50); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$PORT" && head -c 3 <&3 | grep -q 220) 2>/dev/null \
    && (exec 3<>"/dev/tcp/127.0.0.1/$FTPS_PORT") 2>/dev/null; then
    break
  fi
  sleep 0.2
done

RIVET_IT_FTP_PORT="$PORT" RIVET_IT_BIG_MB="${RIVET_IT_BIG_MB:-64}" \
  cargo test --manifest-path "$MANIFEST" --lib -- --ignored --test-threads=1 "${@:-ftp_integration}"
