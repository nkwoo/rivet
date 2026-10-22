# Security

Rivet has no option to skip checking a server's identity. Instead it checks the first time and blocks when something changes.

## SSH host keys

- For a server you have not connected to before, Rivet shows the key type and SHA-256 fingerprint and asks for approval. Compare it with `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` on the server.
- Approved keys are saved in Rivet's known_hosts.
- If the server sends a different key, Rivet **blocks the connection** and shows both the saved and received fingerprints.
  If the change is legitimate, for example after reinstalling the server, confirm with the administrator, remove the old entry in **Review saved keys** and reconnect.
- Each jump host hop is verified separately.

## RDP and FTPS certificates

- Certificates trusted by your OS connect as usual.
- Untrusted certificates (self-signed, including most default Windows RDP certificates) are pinned after you check their SHA-256 fingerprint on the first connection (TOFU).
- A certificate that differs from the pinned one blocks the connection. Review and remove pins from the shield icon (**Saved host keys and certificates**) in the top bar.

## Passwords and passphrases

- Saved only when you choose to, in the macOS Keychain / Windows Credential Manager.
- Never written to the connection database, settings files or logs.
- Saved only after sign-in succeeds, so wrong passwords are never kept.

## File transfer

- Remote file names pointing outside the target folder (`../`, absolute paths) or reserved on Windows are not downloaded.
- Paths, user names and passwords containing line breaks (CR/LF) are never sent in FTP commands.
- Server listings and responses have size limits.
- Plain FTP keeps showing a warning.

## The app

- The webview only shows the app's built-in pages and never loads remote pages.
- Updates are installed only after their signature is verified with the public key built into the app. See [Updates](./updates).
- Release files are built on GitHub Actions with build provenance attestations. See [Verify downloads](./install#verify-downloads).

## Reporting vulnerabilities

Please do not open public issues for security vulnerabilities. Report them through **Security → Report a vulnerability** in the GitHub repository.
See [SECURITY.md](https://github.com/nkwoo/rivet/blob/main/SECURITY.md) for details.
