---
layout: home

hero:
  name: Rivet
  text: SSH, file transfer and remote desktop in one window
  tagline: An open-source remote access client for macOS and Windows
  image:
    src: /rivet.svg
    alt: Rivet
  actions:
    - theme: brand
      text: Install
      link: /en/guide/install
    - theme: alt
      text: GitHub
      link: https://github.com/nkwoo/rivet

features:
  - title: SSH terminal
    details: Tabs and split panes, jump hosts, ~/.ssh/config import. Unknown host keys connect only after you check the fingerprint.
    link: /en/guide/ssh
  - title: SFTP · FTP · FTPS
    details: A two-pane local/remote explorer with a transfer queue. Transfers reuse the same connection as the SSH terminal.
    link: /en/guide/files
  - title: Remote desktop (RDP)
    details: NLA authentication, certificate pinning, text clipboard sharing and resolution that follows the window. Can also connect through an SSH tunnel.
    link: /en/guide/rdp
  - title: Passwords stay in the OS vault
    details: Passwords and key passphrases are kept only in the macOS Keychain or Windows Credential Manager.
    link: /en/guide/security
---

![Rivet: terminal, file transfer and remote desktop](/screenshot1.png)

::: warning Pre-release (0.x)
Settings and storage formats may change.
:::
