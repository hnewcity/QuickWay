"""Validate, package, and publish a release from one tested Git commit."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parent.parent
DESKTOP = "desktop"
TAURI = f"{DESKTOP}/src-tauri"
PLATFORMS = {
    "windows-x86_64": "x86_64-pc-windows-msvc",
    "darwin-aarch64": "aarch64-apple-darwin",
    "darwin-x86_64": "x86_64-apple-darwin",
}


def read_json(path: Path) -> dict:
    """Read a UTF-8 JSON document."""
    return json.loads(path.read_text(encoding="utf-8"))


def read_text(path: Path) -> str:
    """Read UTF-8 text without translating line endings, so edits keep CRLF or LF files intact."""
    with path.open(encoding="utf-8", newline="") as stream:
        return stream.read()


def write_text(path: Path, text: str) -> None:
    """Write UTF-8 text exactly as given."""
    with path.open("w", encoding="utf-8", newline="") as stream:
        stream.write(text)


def version_tuple(version: str) -> tuple[int, int, int]:
    """Accept stable semantic versions only; prereleases require a separate channel."""
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        raise ValueError("Release version must be a stable semantic version, such as 0.3.0")
    return tuple(int(part) for part in version.split("."))


def lock_version(root: Path, name: str, package: str) -> str:
    """Return the version recorded for one crate in a Cargo lockfile."""
    lock = tomllib.loads(read_text(root / name))
    return next(item["version"] for item in lock["package"] if item["name"] == package)


def validate(root: Path = ROOT) -> str:
    """Require every package manifest and the release notes to describe the same version."""
    version = read_json(root / DESKTOP / "package.json")["version"]
    version_tuple(version)
    npm_lock = read_json(root / DESKTOP / "package-lock.json")
    versions = [
        npm_lock["version"], npm_lock["packages"][""]["version"],
        tomllib.loads(read_text(root / "Cargo.toml"))["package"]["version"],
        lock_version(root, "Cargo.lock", "quickway"),
        tomllib.loads(read_text(root / TAURI / "Cargo.toml"))["package"]["version"],
        lock_version(root, f"{TAURI}/Cargo.lock", "quickway-desktop"),
        lock_version(root, f"{TAURI}/Cargo.lock", "quickway"),
        read_json(root / TAURI / "tauri.conf.json")["version"],
    ]
    if any(value != version for value in versions):
        raise ValueError("Package, npm lock, Cargo, Cargo lock, and Tauri versions must match")
    notes = root / "releases" / f"v{version}.md"
    if not notes.is_file() or not notes.read_text(encoding="utf-8").strip():
        raise ValueError(f"Write release notes in releases/v{version}.md before publishing")
    return version


def replace_once(root: Path, name: str, pattern: str, version: str) -> None:
    """Replace the single version string matched by a pattern with two capture groups around it."""
    path = root / name
    text, count = re.subn(pattern, lambda match: match[1] + version + match[2], read_text(path), count=1)
    if count != 1:
        raise ValueError(f"Cannot update package version in {name}")
    write_text(path, text)


def bump(version: str, root: Path = ROOT) -> None:
    """Update all package versions without creating a tag or publishing anything."""
    version_tuple(version)
    top = r'(?m)^(  "version": ")[^"]+(")'
    replace_once(root, f"{DESKTOP}/package.json", top, version)
    replace_once(root, f"{TAURI}/tauri.conf.json", top, version)
    replace_once(root, f"{DESKTOP}/package-lock.json", top, version)
    replace_once(root, f"{DESKTOP}/package-lock.json",
                 r'(?m)(^    "": \{\r?\n(?:.*\r?\n)*?      "version": ")[^"]+(")', version)
    replace_once(root, "Cargo.toml", r'(?m)^(version = ")[^"]+(")', version)
    replace_once(root, f"{TAURI}/Cargo.toml", r'(?m)^(version = ")[^"]+(")', version)
    for lock, package in (("Cargo.lock", "quickway"), (f"{TAURI}/Cargo.lock", "quickway"),
                          (f"{TAURI}/Cargo.lock", "quickway-desktop")):
        replace_once(root, lock, rf'(?m)(^name = "{package}"\r?\nversion = ")[^"]+(")', version)
    print(f"Version set to {version}. Write releases/v{version}.md and push to main to publish.")


def gh_json(endpoint: str) -> dict | None:
    """Read GitHub metadata, distinguishing absent releases from authorization failures."""
    result = subprocess.run(["gh", "api", endpoint], capture_output=True, text=True, check=False)
    if result.returncode:
        if "HTTP 404" in result.stderr:
            return None
        raise RuntimeError(f"GitHub metadata request failed: {endpoint}")
    return json.loads(result.stdout)


def release_plan(version: str, repository: str, sha: str) -> bool:
    """Publish new versions and resume drafts, never overwrite an already published release."""
    release = gh_json(f"repos/{repository}/releases/tags/v{version}")
    if release and not release["draft"]:
        return False
    latest = gh_json(f"repos/{repository}/releases/latest")
    if latest and version_tuple(version) <= version_tuple(latest["tag_name"].removeprefix("v")):
        raise ValueError("A new release must have a version higher than the latest published release")
    tagged = gh_json(f"repos/{repository}/git/ref/tags/v{version}")
    if tagged:
        commit = gh_json(f"repos/{repository}/commits/v{version}")
        if not commit or commit["sha"] != sha:
            raise ValueError("The release tag already points to another commit; bump the version")
    return True


def plan() -> None:
    """Export the release decision for GitHub Actions; PRs always exercise packaging."""
    version = validate()
    publish = os.environ["GITHUB_REF"] == "refs/heads/main" and os.environ["GITHUB_EVENT_NAME"] != "pull_request"
    if publish:
        publish = release_plan(version, os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"])
    build = publish or os.environ["GITHUB_EVENT_NAME"] == "pull_request"
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"version={version}\nshould_build={str(build).lower()}\nshould_publish={str(publish).lower()}\n")
    print(f"Version {version}: build={build}, publish={publish}")


def expected_assets(platform: str, version: str) -> list[tuple[str, str, str]]:
    """List (kind, build output relative to the repository, release asset name) for one platform."""
    desktop_release = f"{TAURI}/target/{PLATFORMS[platform]}/release"
    if platform.startswith("windows"):
        return [
            ("installer", f"{desktop_release}/bundle/nsis/QuickWay_{version}_x64-setup.exe",
             f"QuickWay_{version}_x64-setup.exe"),
            ("portable", f"{desktop_release}/quickway-desktop.exe", f"QuickWay_{version}_x64-portable.exe"),
        ]
    arch = "aarch64" if platform == "darwin-aarch64" else "x64"
    return [
        ("installer", f"{desktop_release}/bundle/dmg/QuickWay_{version}_{arch}.dmg", f"QuickWay_{version}_{arch}.dmg"),
    ]


def digest(path: Path) -> str:
    """Hash an installer without reading it all into memory."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def collect(platform: str, sha: str, root: Path = ROOT) -> None:
    """Collect the platform's desktop packages with source-commit and checksum metadata."""
    version = validate(root)
    destination = root / "release-assets" / platform
    destination.mkdir(parents=True, exist_ok=True)
    files = []
    for kind, source_name, name in expected_assets(platform, version):
        source = root / source_name
        if not source.is_file() or not source.stat().st_size:
            raise ValueError(f"Expected a nonempty {kind} build output at {source_name}")
        target = destination / name
        shutil.copy2(source, target)
        files.append({"name": name, "sha256": digest(target), "size": target.stat().st_size})
    metadata = {"version": version, "commit": sha, "platform": platform, "target": PLATFORMS[platform], "files": files}
    (destination / f"build-{platform}.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")


def verify_assets(directory: Path, version: str, sha: str) -> list[Path]:
    """Reject missing platforms, mixed commits, tampered assets, and duplicate filenames."""
    files: dict[str, Path] = {}
    for platform, target in PLATFORMS.items():
        metadata = read_json(directory / f"build-{platform}.json")
        if (metadata["version"], metadata["commit"], metadata["platform"], metadata["target"]) != (version, sha, platform, target):
            raise ValueError("Release artifacts must all come from the same version and tested commit")
        expected = sorted(name for _, _, name in expected_assets(platform, version))
        if sorted(item["name"] for item in metadata["files"]) != expected:
            raise ValueError(f"Incomplete installer set for {platform}")
        for item in metadata["files"]:
            name = item["name"]
            if Path(name).name != name or name in files:
                raise ValueError("Invalid or duplicate release asset filename")
            path = directory / name
            if not path.is_file() or not item["size"] or path.stat().st_size != item["size"] or digest(path) != item["sha256"]:
                raise ValueError("Release asset size or SHA-256 does not match its build metadata")
            files[name] = path
    checksum_file = directory / "SHA256SUMS.txt"
    checksum_file.write_text("".join(f"{digest(path)}  {name}\n" for name, path in sorted(files.items())), encoding="utf-8")
    return [*files.values(), checksum_file]


def publish(directory: Path) -> None:
    """Upload a complete release as a draft, then make it public after every upload succeeds."""
    version = validate()
    repository, sha = os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"]
    assets = verify_assets(directory, version, sha)
    if not release_plan(version, repository, sha):
        print("This version is already published; leaving it unchanged.")
        return
    tag = f"v{version}"
    release = gh_json(f"repos/{repository}/releases/tags/{tag}")
    common = ["--repo", repository]
    if not release:
        subprocess.run(["gh", "release", "create", tag, *common, "--target", sha,
                        "--title", f"QuickWay {tag}", "--notes-file", str(ROOT / "releases" / f"{tag}.md"),
                        "--draft"], check=True)
    subprocess.run(["gh", "release", "upload", tag, *common, *map(str, assets), "--clobber"], check=True)
    subprocess.run(["gh", "release", "edit", tag, *common, "--draft=false", "--latest"], check=True)


def main() -> None:
    """Run the release helper from the repository root or CI."""
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("validate")
    commands.add_parser("plan")
    commands.add_parser("bump").add_argument("version")
    commands.add_parser("collect").add_argument("platform", choices=PLATFORMS)
    commands.add_parser("publish").add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "validate":
        print(f"Validated release v{validate()}")
    elif args.command == "bump":
        bump(args.version)
    elif args.command == "plan":
        plan()
    elif args.command == "collect":
        collect(args.platform, os.environ["GITHUB_SHA"])
    elif args.command == "publish":
        publish(args.directory)


if __name__ == "__main__":
    main()
