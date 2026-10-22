<p align="center">
  <img src="site/public/rivet.svg" width="96" height="96" alt="Rivet logo">
</p>

<h1 align="center">Rivet</h1>

<p align="center">
  SSH, file transfer and remote desktop in one window.<br>
  An open-source remote access client for macOS and Windows.
</p>

<p align="center">
  <a href="https://github.com/nkwoo/rivet/releases/latest"><img src="https://img.shields.io/github/v/release/nkwoo/rivet" alt="Latest release"></a>
  <a href="https://github.com/nkwoo/rivet/actions/workflows/ci.yml"><img src="https://github.com/nkwoo/rivet/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0"></a>
  <a href="https://nkwoo.github.io/rivet/en/"><img src="https://img.shields.io/badge/docs-nkwoo.github.io%2Frivet-orange" alt="Documentation"></a>
</p>

<p align="center"><b>English</b> · <a href="README.ko.md">한국어</a></p>

> [!NOTE]
> Rivet is pre-release (0.x). Settings and storage formats may change.

## Features

- **SSH terminal** — tabs and split panes, jump hosts, `~/.ssh/config` import
- **SFTP / FTP / FTPS** — two-pane local ↔ remote explorer with a transfer queue, sharing the SSH terminal's connection
- **Remote desktop (RDP)** — NLA, text clipboard, resolution that follows the window, optional SSH tunnel
- **Connection profiles** — passwords live only in the OS vault (macOS Keychain, Windows Credential Manager)
- **Automatic updates** — signed updates, installed only when you choose to

## Download

| Platform | File |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<version>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<version>_x64-setup.exe` or `Rivet_<version>_x64_en-US.msi` |

Get them from [**Releases**](https://github.com/nkwoo/rivet/releases/latest).
Rivet is not code-signed yet, so macOS and Windows show a warning on first launch —
see the [install guide](https://nkwoo.github.io/rivet/en/guide/install) for what to click.

## Documentation

The user guide is at **[nkwoo.github.io/rivet](https://nkwoo.github.io/rivet/en/)**:
[install](https://nkwoo.github.io/rivet/en/guide/install) ·
[connections](https://nkwoo.github.io/rivet/en/guide/connections) ·
[SSH](https://nkwoo.github.io/rivet/en/guide/ssh) ·
[file transfer](https://nkwoo.github.io/rivet/en/guide/files) ·
[remote desktop](https://nkwoo.github.io/rivet/en/guide/rdp) ·
[security](https://nkwoo.github.io/rivet/en/guide/security) ·
[troubleshooting](https://nkwoo.github.io/rivet/en/guide/troubleshooting)

## Security

- Unknown SSH host keys and untrusted TLS certificates are shown with their fingerprint before you connect; changed ones are blocked. There is no option to skip verification.
- Passwords and key passphrases are stored only in the OS vault, never in Rivet's database, settings or logs.
- Remote file names that point outside the target folder are refused.
- Releases are built on GitHub Actions with build provenance attestations and `SHA256SUMS.txt`.

To report a vulnerability, see [SECURITY.md](SECURITY.md). Please do not open a public issue.

## Contributing

Building from source, running the checks and integration tests, and the release process are described in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
