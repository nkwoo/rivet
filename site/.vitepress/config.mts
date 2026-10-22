import { defineConfig, type DefaultTheme } from "vitepress";

const repo = "https://github.com/nkwoo/rivet";

function sidebar(prefix: string, t: Record<string, string>): DefaultTheme.SidebarItem[] {
  return [
    {
      text: t.start,
      items: [
        { text: t.install, link: `${prefix}/guide/install` },
        { text: t.connections, link: `${prefix}/guide/connections` },
      ],
    },
    {
      text: t.features,
      items: [
        { text: t.ssh, link: `${prefix}/guide/ssh` },
        { text: t.files, link: `${prefix}/guide/files` },
        { text: t.rdp, link: `${prefix}/guide/rdp` },
      ],
    },
    {
      text: t.more,
      items: [
        { text: t.security, link: `${prefix}/guide/security` },
        { text: t.updates, link: `${prefix}/guide/updates` },
        { text: t.troubleshooting, link: `${prefix}/guide/troubleshooting` },
      ],
    },
  ];
}

export default defineConfig({
  base: "/rivet/",
  cleanUrls: true,
  lastUpdated: false,
  appearance: "dark",
  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/rivet/rivet.svg" }],
    ["meta", { name: "referrer", content: "no-referrer" }],
  ],
  themeConfig: {
    logo: "/rivet.svg",
    siteTitle: "Rivet",
    socialLinks: [{ icon: "github", link: repo }],
    search: {
      provider: "local",
      options: {
        locales: {
          root: {
            translations: {
              button: { buttonText: "검색", buttonAriaLabel: "검색" },
              modal: {
                noResultsText: "결과가 없습니다",
                resetButtonTitle: "지우기",
                footer: { selectText: "선택", navigateText: "이동", closeText: "닫기" },
              },
            },
          },
        },
      },
    },
  },
  locales: {
    root: {
      label: "한국어",
      lang: "ko",
      title: "Rivet",
      description: "SSH, 파일 전송, 원격 데스크톱을 하나의 창에. macOS와 Windows용 오픈소스 원격 접속 클라이언트",
      themeConfig: {
        nav: [
          { text: "가이드", link: "/guide/install" },
          { text: "내려받기", link: `${repo}/releases/latest` },
        ],
        sidebar: sidebar("", {
          start: "시작하기",
          install: "설치",
          connections: "연결 만들기",
          features: "기능",
          ssh: "SSH 터미널",
          files: "파일 전송",
          rdp: "원격 데스크톱",
          more: "더 알아보기",
          security: "보안",
          updates: "업데이트",
          troubleshooting: "문제 해결",
        }),
        editLink: { pattern: `${repo}/edit/main/site/:path`, text: "이 페이지 고치기" },
        outline: { label: "이 페이지에서" },
        docFooter: { prev: "이전", next: "다음" },
        langMenuLabel: "언어",
        returnToTopLabel: "맨 위로",
        sidebarMenuLabel: "메뉴",
        darkModeSwitchLabel: "테마",
        lightModeSwitchTitle: "밝은 테마로",
        darkModeSwitchTitle: "어두운 테마로",
        notFound: { title: "페이지를 찾을 수 없습니다", quote: "주소를 확인하세요.", linkText: "처음으로" },
        footer: { message: "MIT 또는 Apache-2.0 라이선스로 배포합니다." },
      },
    },
    en: {
      label: "English",
      lang: "en",
      title: "Rivet",
      description: "SSH, file transfer and remote desktop in one window. An open-source remote access client for macOS and Windows",
      themeConfig: {
        nav: [
          { text: "Guide", link: "/en/guide/install" },
          { text: "Download", link: `${repo}/releases/latest` },
        ],
        sidebar: sidebar("/en", {
          start: "Getting started",
          install: "Install",
          connections: "Create a connection",
          features: "Features",
          ssh: "SSH terminal",
          files: "File transfer",
          rdp: "Remote desktop",
          more: "More",
          security: "Security",
          updates: "Updates",
          troubleshooting: "Troubleshooting",
        }),
        editLink: { pattern: `${repo}/edit/main/site/:path`, text: "Edit this page" },
        footer: { message: "Released under the MIT or Apache-2.0 license." },
      },
    },
  },
});
