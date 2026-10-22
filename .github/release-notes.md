## 설치

| 플랫폼 | 파일 |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<버전>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<버전>_x64-setup.exe` (사용자 설치) 또는 `Rivet_<버전>_x64_en-US.msi` (관리자 배포용) |

아직 코드 서명 인증서가 없어 OS가 처음 실행할 때 경고를 표시합니다.

- **macOS**: dmg를 열어 Rivet을 응용 프로그램 폴더로 옮긴 뒤 실행합니다. "확인되지 않은 개발자" 경고가 나오면
  **시스템 설정 → 개인정보 보호 및 보안**에서 **그래도 열기**를 누릅니다.
- **Windows**: SmartScreen 경고에서 **추가 정보 → 실행**을 누릅니다.

## 내려받은 파일 확인

`SHA256SUMS.txt`로 무결성을 확인하고, GitHub 빌드 출처 증명으로 이 저장소의 릴리스 워크플로가 만든 파일인지 확인할 수 있습니다.

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing      # Windows: certutil -hashfile <파일> SHA256
gh attestation verify <파일> --repo nkwoo/rivet
```

