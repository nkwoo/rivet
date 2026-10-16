# Rivet

SSH, 파일 전송, 원격 데스크톱을 하나의 창에. macOS와 Windows용 오픈소스 원격 접속 통합 클라이언트입니다.

- **SSH 터미널** — 리눅스 서버 접속, 탭/분할 창
- **SFTP / FTP / FTPS** — 로컬 ↔ 원격 2단 파일 탐색기, 전송 큐
- **RDP** — 윈도우 서버 원격 데스크톱
- **연결 프로필** — 비밀번호는 OS 보안 저장소(macOS 키체인, Windows 자격 증명 관리자)에만 저장

> 현재 개발 초기 단계입니다. SSH 터미널과 연결 프로필이 동작하며, 파일 전송과 RDP는 개발 중입니다.

## 개발 환경

| 도구 | 버전 |
|---|---|
| Rust | `rust-toolchain.toml`로 고정 (MSRV 1.90) |
| Node.js | 22 이상 |
| 플랫폼 요구사항 | [Tauri 2 사전 준비](https://v2.tauri.app/start/prerequisites/) (macOS: Xcode CLT, Windows: MSVC 빌드 도구 + WebView2) |

```bash
npm install
npm run tauri dev            # 개발 실행
npm run tauri build          # 배포 빌드
```

### 검사

```bash
npm run typecheck
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo deny --manifest-path src-tauri/Cargo.toml check   # cargo install --locked cargo-deny
cargo audit --file src-tauri/Cargo.lock                  # cargo install --locked cargo-audit
npm audit
```

### SSH / SFTP 통합 테스트

Docker로 테스트용 OpenSSH 서버를 띄우고 `#[ignore]`된 통합 테스트를 실행합니다. (Docker 필요)

```bash
src-tauri/tests/docker/run-ssh-it.sh             # 전체 (대용량 전송은 64MB)
src-tauri/tests/docker/run-ssh-it.sh sftp        # 이름에 sftp가 들어간 테스트만
RIVET_IT_BIG_MB=1024 src-tauri/tests/docker/run-ssh-it.sh sftp_large   # 1GB 전송
```

로컬 키체인을 실제로 쓰는 테스트는 `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored keyring`으로 따로 실행합니다.

## 보안

취약점 신고 방법은 [SECURITY.md](SECURITY.md)를 참고하세요.

## 라이선스

다음 중 하나를 선택해 사용할 수 있습니다.

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

별도로 명시하지 않는 한, 기여한 코드는 Apache-2.0 라이선스 정의에 따라 위와 같이 추가 조건 없이 듀얼 라이선스로 제공되는 것에 동의한 것으로 간주합니다.
