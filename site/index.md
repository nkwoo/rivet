---
layout: home

hero:
  name: Rivet
  text: SSH, 파일 전송, 원격 데스크톱을 하나의 창에
  tagline: macOS와 Windows용 오픈소스 원격 접속 클라이언트
  image:
    src: /rivet.svg
    alt: Rivet
  actions:
    - theme: brand
      text: 설치하기
      link: /guide/install
    - theme: alt
      text: GitHub
      link: https://github.com/nkwoo/rivet

features:
  - title: SSH 터미널
    details: 탭과 분할 창, 점프 호스트, ~/.ssh/config 가져오기. 처음 보는 호스트 키는 지문을 확인한 뒤에만 연결합니다.
    link: /guide/ssh
  - title: SFTP · FTP · FTPS
    details: 로컬과 원격을 나란히 놓은 2단 탐색기와 전송 큐. SSH 터미널과 같은 연결로 파일을 주고받습니다.
    link: /guide/files
  - title: 원격 데스크톱 (RDP)
    details: NLA 인증, 인증서 고정, 텍스트 클립보드 공유, 창에 맞춘 해상도. SSH 터널을 거쳐 접속할 수도 있습니다.
    link: /guide/rdp
  - title: 비밀번호는 OS 보안 저장소에만
    details: 비밀번호와 키 패스프레이즈는 macOS 키체인 또는 Windows 자격 증명 관리자에만 보관합니다.
    link: /guide/security
---

::: warning 정식 릴리스 전(0.x)
설정과 저장 형식이 바뀔 수 있습니다.
:::
