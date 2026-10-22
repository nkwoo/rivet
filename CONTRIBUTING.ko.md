# 기여 안내

[English](CONTRIBUTING.md) · **한국어**

## 시작하기 전에

- 버그와 기능 제안은 [이슈](https://github.com/nkwoo/rivet/issues), 사용 질문은 [Discussions](https://github.com/nkwoo/rivet/discussions)에 올려 주세요.
- 보안 취약점은 공개하지 말고 [SECURITY.ko.md](SECURITY.ko.md)의 방법으로 신고해 주세요.
- main에는 PR로만 병합합니다. 아래 CI 검사가 모두 통과해야 합니다.
- 커밋 메시지는 [Conventional Commits](https://www.conventionalcommits.org/ko/) 형식(`feat:`, `fix:`, `sec:`, `docs:` 등)을 따릅니다.
- Rust 코드는 테스트 밖에서 `unwrap()`/`expect()`를 쓰지 않고, 네트워크 입력은 `Result`로 에러를 전파합니다. `unsafe`는 쓰지 않습니다.
- 새 의존성은 `cargo deny check`를 통과해야 합니다.

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

## 검사

CI와 같은 검사입니다.

```bash
npm run typecheck
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo deny --manifest-path src-tauri/Cargo.toml check   # cargo install --locked cargo-deny
cargo audit --file src-tauri/Cargo.lock                  # cargo install --locked cargo-audit
npm audit
```

## SSH / SFTP / FTP 통합 테스트

Docker로 테스트용 OpenSSH·vsftpd 서버를 띄우고 `#[ignore]`된 통합 테스트를 실행합니다. (Docker 필요)

```bash
src-tauri/tests/docker/run-ssh-it.sh             # 전체 (대용량 전송은 64MB)
src-tauri/tests/docker/run-ssh-it.sh sftp        # 이름에 sftp가 들어간 테스트만
RIVET_IT_BIG_MB=1024 src-tauri/tests/docker/run-ssh-it.sh sftp_large   # 1GB 전송
RIVET_IT_SSH_PORT=2233 src-tauri/tests/docker/run-ssh-it.sh            # 2222 포트가 사용 중일 때
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

## 문서 사이트

`site/`의 VitePress 사이트(한국어 `site/guide`, 영어 `site/en/guide`)는 main에 병합되면 GitHub Pages로 배포됩니다.
앱과 의존성을 나누기 위해 `site/`에 별도 `package.json`이 있습니다. 기능을 바꾸면 두 언어의 문서를 함께 고쳐 주세요.

```bash
cd site
npm ci --ignore-scripts
npm run dev      # http://localhost:5173/rivet/
npm run build
```

## 릴리스 (관리자)

`v*` 태그를 푸시하면 `.github/workflows/release.yml`이 macOS(유니버설)·Windows 설치 파일을 빌드해 Releases 초안을 만듭니다.
`v*` 태그는 관리자만 만들 수 있고, 만든 뒤에는 지우거나 옮길 수 없습니다.

```bash
node scripts/version.mjs set 0.3.0   # package.json, tauri.conf.json, Cargo.toml 등 버전을 한 번에 변경
# 변경을 PR로 main에 병합한 뒤
git checkout main && git pull
git tag v0.3.0 && git push origin v0.3.0
```

- 태그와 각 파일의 버전이 다르거나, 태그 커밋이 main에 없거나, `cargo deny`·`cargo audit`·`npm audit`이 실패하면 빌드하지 않습니다.
- 버전은 `X.Y.Z`만 씁니다 (MSI가 프리릴리스 접미사를 받지 않음).
- 초안의 설치 파일을 확인한 뒤 GitHub에서 직접 게시합니다. 게시해야 `releases/latest/download/latest.json`이 바뀌어
  설치된 앱이 업데이트를 받습니다.
- 업데이트 서명 키(minisign)는 `release` 환경 시크릿(`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`)에만 있고,
  `v*` 태그 실행의 서명 작업에서만 쓰입니다. 키를 잃으면 이미 설치된 앱은 자동 업데이트를 받을 수 없으므로 따로 백업합니다.
