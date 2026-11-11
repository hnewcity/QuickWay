# Automatic releases

GitHub Actions runs the Rust tests and Clippy on Linux, Windows and macOS, the release-tool tests, version
validation, the frontend build and the browser UI tests on every pull request and push to `main`. Pull requests
also build every installer, but cannot publish a release.

When a new version reaches `main`, the workflow builds these packages from the same tested commit:

| Platform | Files |
|----------|-------|
| Windows x64 | `QuickWay_<version>_x64-setup.exe` (NSIS installer), `QuickWay_<version>_x64-portable.exe`, `quickway-cli_<version>_windows-x86_64.exe` |
| macOS Apple Silicon | `QuickWay_<version>_aarch64.dmg`, `quickway-cli_<version>_darwin-aarch64.tar.gz` |
| macOS Intel | `QuickWay_<version>_x64.dmg`, `quickway-cli_<version>_darwin-x86_64.tar.gz` |

All platforms must succeed and identify the same commit before publication. Uploads go to a draft first; the
release becomes public only after every file and `SHA256SUMS.txt` have been uploaded.

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
builds use ad-hoc signing and are not notarized. No signing secrets are needed for this workflow.

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
