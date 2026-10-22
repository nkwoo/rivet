# Troubleshooting

## Connection blocked because the host key changed

The server sent a host key different from the one saved in Rivet. The server may have been reinstalled or its key replaced, but someone could also be intercepting the connection.

1. Ask the server administrator whether the key was changed.
2. If it was, remove the server's entry from the shield icon (**Saved host keys and certificates**) in the top bar.
3. Reconnect, check the new fingerprint and approve it.

Do the same to remove a pin when an RDP or FTPS certificate changes.

## Sign-in fails

- If Rivet asks for the password again, check the password. If a saved password is wrong, use **Delete saved secret** in the connection editor and enter it again.
- Private key: make sure the public key is in the server's `~/.ssh/authorized_keys` and the key file path is correct.
- ssh-agent: check that the agent has keys with `ssh-add -l`. On Windows, the **OpenSSH Authentication Agent** service must be running.
- With a jump host, "jump host authentication failed" means the jump host's own sign-in failed.

## Keychain access fails

When macOS asks for Keychain access, click **Allow** or **Always Allow**. If you deny it, you need to enter the password every time.
macOS may ask again after an update (see [Updates](./updates#installing-manually)).

## FTP listing or transfers fail

"Can't connect to the server's data port" means the FTP passive data connection is blocked.

- Make sure the passive port range is open on the server firewall or router.
- With Docker or port forwarding, publish the passive ports **with the same numbers**. Publishing only the control port (21) lets you sign in, but listing and transfers fail.
- Prefer SFTP when possible; it uses a single port.

## RDP is slow

- Check fps and input latency with **Stats** in the toolbar.
- Turning off **High-resolution (Retina)** or lowering the resolution reduces bandwidth.
- Rivet does not use the H.264 codec yet, so constantly changing full-screen content such as video can be slower than the official client.

## Windows blocks the app

See the SmartScreen and Smart App Control notes in [Install · Windows](./install#windows).

## Other problems

Open a [GitHub issue](https://github.com/nkwoo/rivet/issues) with your OS, Rivet version (status bar) and steps to reproduce.
Remove sensitive details such as passwords and server addresses. Report security problems as described in [Security](./security#reporting-vulnerabilities).
