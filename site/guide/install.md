# 설치

[GitHub Releases](https://github.com/nkwoo/rivet/releases/latest)에서 운영체제에 맞는 파일을 내려받습니다.

| 플랫폼 | 파일 |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<버전>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<버전>_x64-setup.exe` (사용자 설치) 또는 `Rivet_<버전>_x64_en-US.msi` (관리자 배포용) |

## macOS

1. dmg를 열고 **Rivet**을 **응용 프로그램** 폴더로 끌어 놓습니다.
2. Rivet을 실행합니다.

아직 Apple 개발자 인증서로 서명·공증하지 않아서 처음 실행할 때 "확인되지 않은 개발자" 경고가 나옵니다.
**시스템 설정 → 개인정보 보호 및 보안**에서 Rivet 항목의 **그래도 열기**를 누르면 됩니다. 한 번만 하면 됩니다.

## Windows

1. `setup.exe`를 실행합니다. 관리자 권한 없이 현재 사용자에게 설치됩니다.
2. **Windows의 PC 보호** (SmartScreen) 경고가 나오면 **추가 정보 → 실행**을 누릅니다.

Rivet은 아직 Windows 코드 서명을 하지 않아서 이 경고가 나옵니다.

::: warning 스마트 앱 컨트롤
Windows 11에서 **스마트 앱 컨트롤**이 **켜짐**으로 설정된 PC에서는 서명되지 않은 앱이 우회 버튼 없이 차단됩니다.
대부분의 PC는 꺼짐 또는 평가 모드라 해당하지 않습니다.
**Windows 보안 → 앱 및 브라우저 컨트롤 → 스마트 앱 컨트롤 설정**에서 상태를 확인할 수 있습니다. 코드 서명은 준비 중입니다.
:::

msi는 그룹 정책이나 배포 도구로 여러 PC에 설치할 때 씁니다.

## 내려받은 파일 확인

릴리스마다 `SHA256SUMS.txt`와 GitHub 빌드 출처 증명이 함께 올라갑니다.
출처 증명으로 파일이 이 저장소의 릴리스 워크플로에서 만들어졌는지 확인할 수 있습니다.

```bash
# 체크섬 (macOS)
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
# 체크섬 (Windows PowerShell)
certutil -hashfile Rivet_<버전>_x64-setup.exe SHA256

# 빌드 출처 증명 (GitHub CLI)
gh attestation verify Rivet_<버전>_universal.dmg --repo nkwoo/rivet
```

## 저장 위치

| 내용 | 위치 |
|---|---|
| 연결 목록, 저장한 호스트 키·인증서 | macOS `~/Library/Application Support/dev.rivet.client/rivet.db`<br>Windows `%APPDATA%\dev.rivet.client\rivet.db` |
| 비밀번호, 키 패스프레이즈 | macOS 키체인 / Windows 자격 증명 관리자 (저장을 선택한 경우에만) |

## 제거

- **macOS**: 응용 프로그램 폴더에서 Rivet을 휴지통으로 옮깁니다. 연결 목록까지 지우려면 위 폴더도 삭제합니다.
- **Windows**: **설정 → 앱 → 설치된 앱**에서 Rivet을 제거합니다.

키체인에 저장한 비밀번호는 앱을 지우기 전에 각 연결의 **저장된 비밀 값 삭제**로 지우거나, 연결을 삭제하면 함께 지워집니다.
