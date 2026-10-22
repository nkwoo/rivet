# 보안 정책

[English](SECURITY.md) · **한국어**

## 지원 버전

정식 릴리스 전(0.x)에는 최신 릴리스만 보안 수정을 받습니다.

## 취약점 신고

보안 취약점은 **공개 이슈로 올리지 마세요.**
이 저장소의 **Security → [Report a vulnerability](https://github.com/nkwoo/rivet/security/advisories/new)**(GitHub Private Vulnerability Reporting)로 신고해 주세요.

신고에 포함해 주시면 좋은 정보:

- 영향받는 버전과 플랫폼(macOS / Windows)
- 재현 절차 또는 PoC
- 예상되는 영향 (예: 호스트 키 검증 우회, 자격 증명 노출, 경로 조작)

접수 후 7일 이내에 1차 답변을 드리고, 수정 일정을 공유합니다. 수정 릴리스가 배포된 뒤 신고자와 협의해 공개합니다.

## 범위

특히 다음 영역의 신고를 환영합니다.

- SSH 호스트 키 / RDP·FTPS 인증서 검증 우회
- 비밀번호, 키 패스프레이즈 노출 (로그, DB, 프론트엔드 상태)
- 원격 파일명을 통한 경로 조작(path traversal)
- FTP 명령 주입, 악성 서버 응답으로 인한 리소스 고갈
- Tauri 권한(capabilities) 우회, 웹뷰에서의 원격 코드 실행
- 업데이트나 릴리스 파일 변조

서드파티 라이브러리(russh, suppaftp, IronRDP, Tauri 등) 자체의 취약점은 해당 프로젝트에도 함께 신고해 주세요.
