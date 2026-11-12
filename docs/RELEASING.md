# Automatic releases

GitHub Actions runs the Rust tests and Clippy, the release-tool tests, version validation, the frontend build and
the browser UI tests (Chromium on Windows, WebKit on macOS) on every pull request and push to `main`. Pull requests
also build every installer, but cannot publish a release.

When a new version reaches `main`, the workflow builds these packages from the same tested commit:

| Platform | Files |
|----------|-------|
| Windows x64 | `QuickWay_<version>_x64-setup.exe` (NSIS installer) and its `.sig`, `QuickWay_<version>_x64-portable.exe` |
| macOS Apple Silicon | `QuickWay_<version>_aarch64.dmg`, `QuickWay_<version>_aarch64.app.tar.gz` and its `.sig` |
| macOS Intel | `QuickWay_<version>_x64.dmg`, `QuickWay_<version>_x64.app.tar.gz` and its `.sig` |

Only the desktop app is released; the command-line proxy is built from source (`cargo build --release`).

All platforms must succeed and identify the same commit before publication. Uploads go to a draft first; the
release becomes public only after every file, `SHA256SUMS.txt` and `latest.json` have been uploaded.

## In-app updates

Installed copies check `releases/latest/download/latest.json` at launch and once a day. The manifest names each
platform's update package (the NSIS installer on Windows, `.app.tar.gz` on macOS) and its signature. QuickWay
downloads the package, verifies it against the public key in `desktop/src-tauri/tauri.conf.json`
(`plugins.updater.pubkey`), stops the local proxy, installs and restarts. The portable Windows exe and an app run
from the mounted DMG cannot replace themselves; they show a link to the release page instead. A copy older than the
first version with the updater must be updated by hand once.

Update packages are signed in CI with two repository secrets, `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The build stops if either the secret or the public key is missing. Pull
requests from forks receive no secrets, so their build jobs fail; verification still runs.

The private key and its password are kept outside the repository (the maintainer's copy is in `~/.tauri/`). Back
them up: if they are lost, already installed copies cannot accept any new update and must be reinstalled by hand
after a new key is configured. To replace the key:

1. `cd desktop && npm run tauri signer generate -- -w ~/.tauri/quickway.key`
2. Put the contents of `~/.tauri/quickway.key.pub` in `plugins.updater.pubkey`.
3. `gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/quickway.key` and set the password the same way.

Local builds (`npm run tauri build`, `scripts/Build-Windows.ps1`) do not create update packages and need no key;
CI adds them with `--config src-tauri/tauri.updater.conf.json`.

## Publish the next version

1. Run `python scripts/release.py bump 0.3.1` to update the npm, Cargo and Tauri manifests and lockfiles together.
2. Write the release notes in `releases/v0.3.1.md`.
3. Commit the changes and push or merge them to `main`.
4. Follow the **CI and release** run in the repository's Actions tab.

No manual tag, artifact upload or personal access token is required. The workflow uses GitHub's built-in token,
with write permission restricted to the publication job. A commit that keeps the already published version runs
verification without publishing again. To recover a failed run, use **Re-run failed jobs**; a full rerun or
**Run workflow** on `main` can also resume an existing draft for the same commit. If a draft tag exists and the
source commit changed, use a new version rather than moving the old tag.

Only stable `major.minor.patch` versions are published, and a new version must be higher than the latest release.

## Distribution scope

This publishes downloadable installers on GitHub Releases. Windows installers are not Authenticode-signed; macOS
builds use ad-hoc signing and are not notarized. The only secrets are the update-signing key described above.

The workflow pins action revisions and the Rust toolchain (`rust-toolchain.toml`); npm and Cargo lockfiles are
committed. Never add account credentials or local application configuration to the repository or release assets.

## Local checks

```bash
python scripts/release.py validate
python -m unittest discover -s scripts/tests -v
cargo test --locked && cargo clippy --all-targets --locked -- -D warnings
cargo test --locked --manifest-path desktop/src-tauri/Cargo.toml
cd desktop && npm ci && npm run build && npm run test:ui
```

On Windows, `scripts/Build-Windows.ps1` runs the same checks and builds the installer into `release/`.
