# Updates

## Checking for a new version

- Rivet checks GitHub Releases for a new version 5 seconds after it starts. If there is one, an **Update to &lt;version&gt;** button appears in the top bar.
- To check yourself, click the version in the status bar or choose **Rivet → Check for Updates…** in the macOS menu.
- To stop checking at startup, turn off **Check at startup** in the update dialog.

Checking only downloads a version file (`latest.json`) from GitHub; nothing such as your connections is sent.

## Installing

Clicking the button shows the new version and what's changed. **Install and restart** downloads and installs it, then restarts the app.

- All connections in open tabs are closed. The dialog shows how many will be disconnected.
- Rivet only notifies you; nothing is installed until you click.
- On Windows, an installer progress window appears briefly.

## Signature check

Update files are signed with the Rivet release key and installed only after verification with the public key built into the app.
If the version inside the signature differs from the advertised version, the update is refused, so an old release cannot be passed off as a new one.

## Installing manually

You can also download a new version and install over the existing one as described in [Install](./install). Your connections and saved passwords are kept.

::: info macOS Keychain prompt
Because Rivet is not yet signed with an Apple Developer certificate, macOS may ask again for Keychain access
the first time a saved password is used after an update. Click **Always Allow**.
:::
