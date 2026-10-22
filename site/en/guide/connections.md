# Create a connection

## New connection

Click **New connection** in the top right and pick a protocol.

| Protocol | Default port | Use |
|---|---|---|
| SSH | 22 | Terminal. File transfer (SFTP) reuses the same connection |
| SFTP | 22 | File transfer only, no terminal |
| FTPS | 990 (implicit TLS). Other ports (usually 21) use AUTH TLS | FTP encrypted with TLS |
| FTP | 21 | Unencrypted FTP. Avoid it if you can |
| RDP | 3389 | Windows remote desktop |

Enter a name, host, port and user name, then choose how to authenticate. Groups, tags, favorites and notes help you organize the list.

### SSH authentication

- **Private key**: Point to a key file. If it has a passphrase, Rivet asks when connecting.
- **Password**: Rivet asks when connecting.
- **ssh-agent**: Uses keys from a running agent. On macOS this is `SSH_AUTH_SOCK`; on Windows, the OpenSSH Authentication Agent service (`ssh-agent`).

### Saving passwords

When Rivet asks for a password or passphrase, check **Save to the system keychain** to save it, only if sign-in succeeds.
It is stored only in the macOS Keychain or Windows Credential Manager, never in Rivet's own files.
Remove it with **Delete saved secret** in the connection editor.

## Connect

Click a connection in the sidebar. <kbd>⌘K</kbd> (Windows <kbd>Ctrl+K</kbd>) jumps to the search box.

For an SSH server you have not connected to before, Rivet shows the host key fingerprint. Click **Trust and connect** only if it
matches the fingerprint from the server administrator or from the server itself. See [Security](./security).

## Import ~/.ssh/config

**Import ~/.ssh/config** in the sidebar brings in the hosts from your OpenSSH config at once.

- Reads `Host`, `HostName`, `Port`, `User`, `IdentityFile`, `ProxyJump`, `Include` and wildcard defaults (`Host *`).
  `Match` blocks are skipped because their conditions cannot be evaluated.
- Hosts with a key file use private key authentication; others use ssh-agent.
- `ProxyJump` sets the connection with the same name as the jump host.
- Passwords and passphrases are not imported.
- Hosts that already exist with the same host, port and user are not selected by default.

## Jump hosts

Pick another SSH connection under **Jump host** in the connection editor to connect through it (OpenSSH `ProxyJump`).

- A jump host can have its own jump host, up to 4 hops.
- Each hop's host key is verified separately.
- If a jump host is deleted, connections that used it switch to a direct connection.
- RDP connections can also use a jump host to connect through an SSH tunnel. See [Remote desktop](./rdp#connect-through-an-ssh-tunnel).

## Delete several connections

Click **Select to delete** in the sidebar, pick connections and delete them. Their saved passwords are removed as well,
and open sessions are disconnected and their tabs closed.
