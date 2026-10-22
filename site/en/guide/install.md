# Install

Download the file for your OS from [GitHub Releases](https://github.com/nkwoo/rivet/releases/latest).

| Platform | File |
|---|---|
| macOS (Apple Silicon, Intel) | `Rivet_<version>_universal.dmg` |
| Windows 10/11 x64 | `Rivet_<version>_x64-setup.exe` (per-user install) or `Rivet_<version>_x64_en-US.msi` (for managed deployment) |

## macOS

1. Open the dmg and drag **Rivet** into **Applications**.
2. Launch Rivet.

Rivet is not yet signed and notarized with an Apple Developer certificate, so the first launch shows an "unidentified developer" warning.
Open **System Settings → Privacy & Security** and click **Open Anyway** next to Rivet. You only need to do this once.

## Windows

1. Run `setup.exe`. It installs for the current user without administrator rights.
2. If **Windows protected your PC** (SmartScreen) appears, click **More info → Run anyway**.

You see this warning because Rivet is not yet code-signed for Windows.

::: warning Smart App Control
On Windows 11 PCs where **Smart App Control** is set to **On**, unsigned apps are blocked with no way to bypass.
Most PCs have it off or in evaluation mode and are not affected.
You can check it under **Windows Security → App & browser control → Smart App Control settings**. Code signing is in progress.
:::

Use the msi when installing on many PCs through Group Policy or a deployment tool.

## Verify downloads

Each release includes `SHA256SUMS.txt` and GitHub build provenance attestations.
The attestation proves a file was built by this repository's release workflow.

```bash
# Checksums (macOS)
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
# Checksums (Windows PowerShell)
certutil -hashfile Rivet_<version>_x64-setup.exe SHA256

# Build provenance (GitHub CLI)
gh attestation verify Rivet_<version>_universal.dmg --repo nkwoo/rivet
```

## Where data is stored

| Data | Location |
|---|---|
| Connections, trusted host keys and certificates | macOS `~/Library/Application Support/dev.rivet.client/rivet.db`<br>Windows `%APPDATA%\dev.rivet.client\rivet.db` |
| Passwords, key passphrases | macOS Keychain / Windows Credential Manager (only when you choose to save) |

## Uninstall

- **macOS**: Move Rivet from Applications to the Trash. Delete the folder above as well to remove your connections.
- **Windows**: Uninstall Rivet from **Settings → Apps → Installed apps**.

Saved passwords are removed when you delete a connection, or with **Delete saved secret** in the connection editor. Do this before uninstalling.
