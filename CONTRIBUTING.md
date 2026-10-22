# Contributing

**English** · [한국어](CONTRIBUTING.ko.md)

## Before you start

- Report bugs and suggest features in [issues](https://github.com/nkwoo/rivet/issues); ask usage questions in [Discussions](https://github.com/nkwoo/rivet/discussions).
- Do not disclose security vulnerabilities publicly — follow [SECURITY.md](SECURITY.md).
- Changes reach `main` only through pull requests, and all CI checks below must pass.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `sec:`, `docs:`, …).
- Rust code does not use `unwrap()`/`expect()` outside tests; code handling network input propagates errors with `Result`. No `unsafe`.
- New dependencies must pass `cargo deny check`.

## Development setup

| Tool | Version |
|---|---|
| Rust | pinned by `rust-toolchain.toml` (MSRV 1.94) |
| Node.js | 22 or later |
| Platform prerequisites | [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) (macOS: Xcode CLT, Windows: MSVC build tools + WebView2) |

```bash
npm install
npm run tauri dev            # run in development
npm run tauri build          # release build
```

## Checks

The same checks as CI:

```bash
npm run typecheck
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo deny --manifest-path src-tauri/Cargo.toml check   # cargo install --locked cargo-deny
cargo audit --file src-tauri/Cargo.lock                  # cargo install --locked cargo-audit
npm audit
```

## SSH / SFTP / FTP integration tests

These start OpenSSH and vsftpd test servers in Docker and run the `#[ignore]`d integration tests (Docker required).

```bash
src-tauri/tests/docker/run-ssh-it.sh             # all (large transfer uses 64 MB)
src-tauri/tests/docker/run-ssh-it.sh sftp        # only tests whose name contains "sftp"
RIVET_IT_BIG_MB=1024 src-tauri/tests/docker/run-ssh-it.sh sftp_large   # 1 GB transfer
RIVET_IT_SSH_PORT=2233 src-tauri/tests/docker/run-ssh-it.sh            # if port 2222 is taken
src-tauri/tests/docker/run-ftp-it.sh             # FTP / FTPS (vsftpd, local ports 2121, 990, 21100-21119)
```

When you run an FTP server to try with the app, publish its passive data ports (21100-21119) **with the same numbers**.
With only the control port published you can sign in, but listing and transfers fail with "Can't connect to the server's data port".

```bash
docker build -t rivet-it-vsftpd src-tauri/tests/docker/vsftpd
docker run -d --name rivet-dev-vsftpd \
  -p 127.0.0.1:2121:21 -p 127.0.0.1:990:990 -p 127.0.0.1:21100-21119:21100-21119 rivet-it-vsftpd
# Profile: host 127.0.0.1, user rivet / rivet-it-password
#   FTP and FTPS (AUTH TLS) → port 2121, implicit FTPS → port 990
```

Tests that use the real local keychain run separately: `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored keyring`.

## Documentation site

The VitePress site in `site/` (Korean in `site/guide`, English in `site/en/guide`) is deployed to GitHub Pages when merged to `main`.
It has its own `package.json` to keep its dependencies separate from the app. When you change a feature, please update both languages.

```bash
cd site
npm ci --ignore-scripts
npm run dev      # http://localhost:5173/rivet/
npm run build
```

## Releasing (maintainers)

Pushing a `v*` tag makes `.github/workflows/release.yml` build the macOS (universal) and Windows installers and create a draft release.
Only maintainers can create `v*` tags, and they cannot be deleted or moved afterwards.

```bash
node scripts/version.mjs set 0.3.0   # bump package.json, tauri.conf.json, Cargo.toml, … together
# merge the change to main through a PR, then
git checkout main && git pull
git tag v0.3.0 && git push origin v0.3.0
```

- Nothing is built if the tag and file versions differ, the tagged commit is not on `main`, or `cargo deny`, `cargo audit` or `npm audit` fails.
- Versions are `X.Y.Z` only (MSI does not accept pre-release suffixes).
- Check the installers in the draft, then publish it on GitHub. Publishing updates `releases/latest/download/latest.json`,
  which is what installed apps check for updates.
- The update signing key (minisign) exists only as secrets of the `release` environment (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`)
  and is used only by the signing job of `v*` tag runs. If it is lost, installed apps can no longer receive updates, so keep a separate backup.
