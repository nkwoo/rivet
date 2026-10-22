<p align="center">
  <img src="site/public/rivet.svg" width="96" height="96" alt="Rivet 로고">
</p>

<h1 align="center">Rivet</h1>

<p align="center">
  SSH, 파일 전송, 원격 데스크톱을 하나의 창에.<br>
  macOS와 Windows용 오픈소스 원격 접속 클라이언트입니다.
</p>

<p align="center">
  <a href="https://github.com/nkwoo/rivet/releases/latest"><img src="https://img.shields.io/github/v/release/nkwoo/rivet" alt="최신 릴리스"></a>
  <a href="https://github.com/nkwoo/rivet/actions/workflows/ci.yml"><img src="https://github.com/nkwoo/rivet/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="#라이선스"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="라이선스: MIT OR Apache-2.0"></a>
  <a href="https://nkwoo.github.io/rivet/"><img src="https://img.shields.io/badge/docs-nkwoo.github.io%2Frivet-orange" alt="사용 설명서"></a>
</p>

<p align="center"><a href="README.md">English</a> · <b>한국어</b></p>

> [!NOTE]
> 정식 릴리스 전(0.x) 단계입니다. 설정과 저장 형식이 바뀔 수 있습니다.

## 기능

- **SSH 터미널** — 탭과 분할 창, 점프 호스트, `~/.ssh/config` 가져오기
- **SFTP / FTP / FTPS** — 로컬 ↔ 원격 2단 탐색기와 전송 큐. SSH 터미널과 같은 연결 사용
- **원격 데스크톱 (RDP)** — NLA, 텍스트 클립보드, 창에 맞춘 해상도, SSH 터널 경유 접속
- **연결 프로필** — 비밀번호는 OS 보안 저장소(macOS 키체인, Windows 자격 증명 관리자)에만 저장
- **자동 업데이트** — 서명을 확인한 업데이트를 사용자가 선택할 때만 설치

## 내려받기

| 플랫폼 | 파일 |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<버전>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<버전>_x64-setup.exe` 또는 `Rivet_<버전>_x64_en-US.msi` |

[**Releases**](https://github.com/nkwoo/rivet/releases/latest)에서 내려받습니다.
아직 코드 서명을 하지 않아 macOS와 Windows가 처음 실행할 때 경고를 표시합니다.
무엇을 누르면 되는지는 [설치 안내](https://nkwoo.github.io/rivet/guide/install)를 참고하세요.

## 사용 설명서

**[nkwoo.github.io/rivet](https://nkwoo.github.io/rivet/)**:
[설치](https://nkwoo.github.io/rivet/guide/install) ·
[연결 만들기](https://nkwoo.github.io/rivet/guide/connections) ·
[SSH](https://nkwoo.github.io/rivet/guide/ssh) ·
[파일 전송](https://nkwoo.github.io/rivet/guide/files) ·
[원격 데스크톱](https://nkwoo.github.io/rivet/guide/rdp) ·
[보안](https://nkwoo.github.io/rivet/guide/security) ·
[문제 해결](https://nkwoo.github.io/rivet/guide/troubleshooting)

## 보안

- 처음 보는 SSH 호스트 키와 신뢰할 수 없는 TLS 인증서는 지문을 보여 주고 승인을 받으며, 바뀌면 연결을 차단합니다. 검증을 건너뛰는 옵션은 없습니다.
- 비밀번호와 키 패스프레이즈는 OS 보안 저장소에만 저장하고, Rivet의 데이터베이스·설정·로그에는 남기지 않습니다.
- 대상 폴더 밖을 가리키는 원격 파일 이름은 내려받지 않습니다.
- 릴리스는 GitHub Actions에서 빌드하며 빌드 출처 증명과 `SHA256SUMS.txt`를 함께 올립니다.

취약점 신고는 [SECURITY.ko.md](SECURITY.ko.md)를 참고하세요. 공개 이슈로 올리지 마세요.

## 기여

소스 빌드, 검사와 통합 테스트, 릴리스 절차는 [CONTRIBUTING.ko.md](CONTRIBUTING.ko.md)에 있습니다.

## 라이선스

다음 중 하나를 선택해 사용할 수 있습니다.

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

별도로 명시하지 않는 한, 기여한 코드는 Apache-2.0 라이선스 정의에 따라 위와 같이 추가 조건 없이 듀얼 라이선스로 제공되는 것에 동의한 것으로 간주합니다.
