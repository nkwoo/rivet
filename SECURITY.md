# Security Policy

**English** · [한국어](SECURITY.ko.md)

## Supported versions

Before 1.0 (0.x), only the latest release receives security fixes.

## Reporting a vulnerability

**Please do not report security vulnerabilities through public issues.**
Use **Security → [Report a vulnerability](https://github.com/nkwoo/rivet/security/advisories/new)** (GitHub Private Vulnerability Reporting) in this repository.

Helpful details to include:

- Affected version and platform (macOS / Windows)
- Steps to reproduce or a proof of concept
- Expected impact (for example host key verification bypass, credential exposure, path traversal)

You will get a first response within 7 days, along with a plan for the fix.
Once a fixed release is out, we will coordinate public disclosure with you.

## Scope

Reports in these areas are especially welcome:

- Bypassing SSH host key or RDP/FTPS certificate verification
- Exposure of passwords or key passphrases (logs, database, frontend state)
- Path traversal through remote file names
- FTP command injection, resource exhaustion from malicious server responses
- Bypassing Tauri capabilities, remote code execution from the webview
- Tampering with updates or release artifacts

Vulnerabilities in third-party libraries (russh, suppaftp, IronRDP, Tauri, etc.) should also be reported to those projects.
