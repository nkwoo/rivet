## Install

| Platform | File |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<version>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<version>_x64-setup.exe` (per-user install) or `Rivet_<version>_x64_en-US.msi` (managed deployment) |

Rivet is not code-signed yet, so the OS shows a warning on first launch.

- **macOS**: Open the dmg and move Rivet to Applications. If you see an "unidentified developer" warning,
  click **Open Anyway** in **System Settings → Privacy & Security**.
- **Windows**: In the SmartScreen warning, click **More info → Run anyway**.
  On Windows 11 PCs where **Smart App Control** is set to **On**, unsigned apps are blocked with no way to bypass.
  Most PCs have it off or in evaluation mode; check under **Windows Security → App & browser control → Smart App Control settings**.

Already running Rivet 0.2.0 or later? It will offer this update automatically.
See the [user guide](https://nkwoo.github.io/rivet/en/guide/install) for details.

## Verify downloads

Check integrity with `SHA256SUMS.txt`, and use the GitHub build provenance attestation to confirm a file was built by this repository's release workflow.

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing      # Windows: certutil -hashfile <file> SHA256
gh attestation verify <file> --repo nkwoo/rivet
```

<details>
<summary><b>한국어 안내 (Korean)</b></summary>

## 설치

| 플랫폼 | 파일 |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<버전>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<버전>_x64-setup.exe` (사용자 설치) 또는 `Rivet_<버전>_x64_en-US.msi` (관리자 배포용) |

아직 코드 서명 인증서가 없어 OS가 처음 실행할 때 경고를 표시합니다.

- **macOS**: dmg를 열어 Rivet을 응용 프로그램 폴더로 옮긴 뒤 실행합니다. "확인되지 않은 개발자" 경고가 나오면
  **시스템 설정 → 개인정보 보호 및 보안**에서 **그래도 열기**를 누릅니다.
- **Windows**: SmartScreen 경고에서 **추가 정보 → 실행**을 누릅니다.
  Windows 11에서 **스마트 앱 컨트롤**이 **켜짐**으로 설정된 PC에서는 서명되지 않은 앱이 우회 버튼 없이 차단됩니다.
  대부분의 PC는 꺼짐 또는 평가 모드라 해당하지 않으며, **Windows 보안 → 앱 및 브라우저 컨트롤 → 스마트 앱 컨트롤 설정**에서 확인할 수 있습니다.

Rivet 0.2.0 이상을 쓰고 있다면 앱이 이 업데이트를 자동으로 알려 줍니다.
자세한 내용은 [사용 설명서](https://nkwoo.github.io/rivet/guide/install)를 참고하세요.

## 내려받은 파일 확인

`SHA256SUMS.txt`로 무결성을 확인하고, GitHub 빌드 출처 증명으로 이 저장소의 릴리스 워크플로가 만든 파일인지 확인할 수 있습니다.

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing      # Windows: certutil -hashfile <파일> SHA256
gh attestation verify <파일> --repo nkwoo/rivet
```

</details>
