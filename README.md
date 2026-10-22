# Rivet

SSH, 파일 전송, 원격 데스크톱을 하나의 창에. macOS와 Windows용 오픈소스 원격 접속 통합 클라이언트입니다.

- **SSH 터미널** — 리눅스 서버 접속, 탭/분할 창
- **SFTP / FTP / FTPS** — 로컬 ↔ 원격 2단 파일 탐색기, 전송 큐
- **RDP** — 윈도우 서버 원격 데스크톱
- **연결 프로필** — 비밀번호는 OS 보안 저장소(macOS 키체인, Windows 자격 증명 관리자)에만 저장

> 정식 릴리스 전(0.x) 단계입니다. 설정과 저장 형식이 바뀔 수 있습니다.

## 설치

[Releases](https://github.com/nkwoo/rivet/releases)에서 내려받습니다.

| 플랫폼 | 파일 |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<버전>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<버전>_x64-setup.exe` 또는 `Rivet_<버전>_x64_en-US.msi` |

아직 코드 서명 인증서가 없어 처음 실행할 때 OS가 경고를 표시합니다.

- **macOS**: "확인되지 않은 개발자" 경고가 나오면 **시스템 설정 → 개인정보 보호 및 보안**에서 **그래도 열기**를 누릅니다.
- **Windows**: SmartScreen 경고에서 **추가 정보 → 실행**을 누릅니다.

설치한 뒤에는 앱이 시작할 때 새 버전을 확인하고, 상단 막대에 알려 줍니다(업데이트 대화상자에서 끌 수 있음).
업데이트 파일은 앱에 들어 있는 공개 키로 서명을 확인한 뒤에만 설치합니다. 직접 확인하려면 macOS 메뉴 **Rivet → 업데이트 확인…** 또는
상태 표시줄의 버전을 누릅니다.

릴리스 파일은 `SHA256SUMS.txt`와 GitHub 빌드 출처 증명(`gh attestation verify <파일> --repo nkwoo/rivet`)으로 확인할 수 있습니다.

## 개발 환경

| 도구 | 버전 |
|---|---|
| Rust | `rust-toolchain.toml`로 고정 (MSRV 1.94) |
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

### SSH / SFTP / FTP 통합 테스트

Docker로 테스트용 OpenSSH 서버를 띄우고 `#[ignore]`된 통합 테스트를 실행합니다. (Docker 필요)

```bash
src-tauri/tests/docker/run-ssh-it.sh             # 전체 (대용량 전송은 64MB)
src-tauri/tests/docker/run-ssh-it.sh sftp        # 이름에 sftp가 들어간 테스트만
RIVET_IT_BIG_MB=1024 src-tauri/tests/docker/run-ssh-it.sh sftp_large   # 1GB 전송
src-tauri/tests/docker/run-ftp-it.sh             # FTP / FTPS (vsftpd, 로컬 포트 2121·990·21100-21119 사용)
```

앱으로 직접 확인할 FTP 서버는 패시브 데이터 포트(21100-21119)를 **같은 번호로** 함께 열어야 합니다.
제어 포트만 열면 로그인은 되지만 목록·전송에서 "데이터 포트에 연결할 수 없습니다" 오류가 납니다.

```bash
docker build -t rivet-it-vsftpd src-tauri/tests/docker/vsftpd
docker run -d --name rivet-dev-vsftpd \
  -p 127.0.0.1:2121:21 -p 127.0.0.1:990:990 -p 127.0.0.1:21100-21119:21100-21119 rivet-it-vsftpd
# 프로필: 호스트 127.0.0.1, 사용자 rivet / rivet-it-password
#   FTP·FTPS(AUTH TLS) → 포트 2121, 암시적 FTPS → 포트 990
```

로컬 키체인을 실제로 쓰는 테스트는 `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored keyring`으로 따로 실행합니다.

## 릴리스

`v*` 태그를 푸시하면 `.github/workflows/release.yml`이 macOS(유니버설)·Windows 설치 파일을 빌드해 Releases 초안을 만듭니다.

```bash
node scripts/version.mjs set 0.2.0   # package.json, tauri.conf.json, Cargo.toml 등 버전을 한 번에 변경
# 변경을 PR로 main에 병합한 뒤
git checkout main && git pull
git tag v0.2.0 && git push origin v0.2.0
```

- 태그와 각 파일의 버전이 다르거나, 태그 커밋이 main에 없거나, `cargo deny`·`cargo audit`·`npm audit`이 실패하면 빌드하지 않습니다.
- 버전은 `X.Y.Z`만 씁니다 (MSI가 프리릴리스 접미사를 받지 않음).
- 초안의 설치 파일을 확인한 뒤 GitHub에서 직접 게시합니다. 게시해야 `releases/latest/download/latest.json`이 바뀌어
  설치된 앱이 업데이트를 받습니다.
- 업데이트 서명 키(minisign)는 `release` 환경 시크릿(`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`)에만 있고,
  `v*` 태그 실행의 서명 작업에서만 쓰입니다. 키를 잃으면 이미 설치된 앱은 자동 업데이트를 받을 수 없으므로 따로 백업합니다.

## 보안

취약점 신고 방법은 [SECURITY.md](SECURITY.md)를 참고하세요.

## 라이선스

다음 중 하나를 선택해 사용할 수 있습니다.

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

별도로 명시하지 않는 한, 기여한 코드는 Apache-2.0 라이선스 정의에 따라 위와 같이 추가 조건 없이 듀얼 라이선스로 제공되는 것에 동의한 것으로 간주합니다.
