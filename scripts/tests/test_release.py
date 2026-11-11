"""Release invariants tested without network access or real installers."""

import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("release", Path(__file__).parents[1] / "release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
SHA = "a" * 40
LOCK = '[[package]]\nname = "quickway"\nversion = "0.3.0"\n\n[[package]]\nname = "quickway-desktop"\nversion = "0.3.0"\n'
NPM_LOCK = ('{\n  "name": "quickway-desktop",\n  "version": "0.3.0",\n  "lockfileVersion": 3,\n  "packages": {\n'
            '    "": {\n      "name": "quickway-desktop",\n      "version": "0.3.0"\n    },\n'
            '    "node_modules/x": {\n      "version": "9.9.9"\n    }\n  }\n}\n')


class ReleaseTests(unittest.TestCase):
    """Verify version synchronization and fail-closed publication of all platforms."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "desktop/src-tauri").mkdir(parents=True)
        (self.root / "releases").mkdir()
        self.put("desktop/package.json", '{\n  "name": "quickway-desktop",\n  "version": "0.3.0"\n}\n')
        self.put("desktop/package-lock.json", NPM_LOCK)
        self.put("desktop/src-tauri/tauri.conf.json", '{\n  "productName": "QuickWay",\n  "version": "0.3.0"\n}\n')
        self.put("Cargo.toml", '[package]\nname = "quickway"\nversion = "0.3.0"\n\n[dependencies]\nserde = "1"\n')
        self.put("desktop/src-tauri/Cargo.toml", '[package]\nname = "quickway-desktop"\nversion = "0.3.0"\n')
        self.put("Cargo.lock", LOCK)
        self.put("desktop/src-tauri/Cargo.lock", LOCK)
        self.put("releases/v0.3.0.md", "First release.\n")

    def put(self, name: str, text: str) -> None:
        """Write a synthetic file."""
        (self.root / name).write_text(text, encoding="utf-8", newline="")

    def installers(self) -> Path:
        """Create fake build outputs for every platform, collect them, and merge the upload directories."""
        merged = self.root / "merged"
        merged.mkdir()
        for platform in release.PLATFORMS:
            for _, source, _ in release.expected_assets(platform, "0.3.0"):
                path = self.root / source
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(f"synthetic {path.name}".encode())
            release.collect(platform, SHA, self.root)
            for file in (self.root / "release-assets" / platform).iterdir():
                shutil.copy2(file, merged / file.name)
        return merged

    def test_validate_and_bump_keep_manifests_in_sync(self) -> None:
        self.assertEqual(release.validate(self.root), "0.3.0")
        release.bump("0.3.1", self.root)
        with self.assertRaisesRegex(ValueError, "release notes"):
            release.validate(self.root)
        self.put("releases/v0.3.1.md", "Next release.\n")
        self.assertEqual(release.validate(self.root), "0.3.1")
        # Dependency versions inside the npm lockfile are never touched.
        self.assertIn('"version": "9.9.9"', (self.root / "desktop/package-lock.json").read_text(encoding="utf-8"))

    def test_bump_preserves_windows_line_endings(self) -> None:
        for name in ("Cargo.lock", "desktop/src-tauri/Cargo.lock", "desktop/package-lock.json", "Cargo.toml"):
            path = self.root / name
            path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
        release.bump("0.3.1", self.root)
        self.put("releases/v0.3.1.md", "Next release.\n")
        self.assertEqual(release.validate(self.root), "0.3.1")
        self.assertNotIn(b"\n", (self.root / "Cargo.lock").read_bytes().replace(b"\r\n", b""))

    def test_mismatched_versions_fail_before_building(self) -> None:
        self.put("desktop/src-tauri/tauri.conf.json", '{\n  "version": "0.2.0"\n}\n')
        with self.assertRaisesRegex(ValueError, "must match"):
            release.validate(self.root)

    def test_stale_desktop_lock_fails_before_building(self) -> None:
        self.put("desktop/src-tauri/Cargo.lock", LOCK.replace('"quickway"\nversion = "0.3.0"', '"quickway"\nversion = "0.1.0"'))
        with self.assertRaisesRegex(ValueError, "must match"):
            release.validate(self.root)

    def test_invalid_versions_are_not_release_tags(self) -> None:
        for version in ("v0.3.0", "0.3.0-rc.1", "00.3.0", "0.3", "0.3.0\n", "$(command)"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.version_tuple(version)

    def test_complete_artifacts_generate_checksums(self) -> None:
        merged = self.installers()
        files = release.verify_assets(merged, "0.3.0", SHA)
        self.assertEqual(len(files), 5)
        checksums = (merged / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(checksums), 4)
        names = {line.split("  ")[1] for line in checksums}
        self.assertIn("QuickWay_0.3.0_x64-setup.exe", names)
        self.assertIn("QuickWay_0.3.0_aarch64.dmg", names)
        self.assertIn("QuickWay_0.3.0_x64.dmg", names)
        self.assertIn("QuickWay_0.3.0_x64-portable.exe", names)

    def test_only_desktop_packages_are_released(self) -> None:
        merged = self.installers()
        names = {path.name for path in release.verify_assets(merged, "0.3.0", SHA)}
        self.assertFalse(any(name.startswith("quickway-cli") for name in names))

    def test_missing_platform_blocks_publication(self) -> None:
        merged = self.installers()
        (merged / "build-darwin-aarch64.json").unlink()
        with self.assertRaises(FileNotFoundError):
            release.verify_assets(merged, "0.3.0", SHA)

    def test_tampered_installer_blocks_publication(self) -> None:
        merged = self.installers()
        next(merged.glob("*.exe")).write_bytes(b"different installer")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            release.verify_assets(merged, "0.3.0", SHA)

    def test_mixed_source_commits_block_publication(self) -> None:
        merged = self.installers()
        with self.assertRaisesRegex(ValueError, "same version and tested commit"):
            release.verify_assets(merged, "0.3.0", "b" * 40)

    def test_missing_build_output_blocks_collection(self) -> None:
        with self.assertRaisesRegex(ValueError, "nonempty"):
            release.collect("windows-x86_64", SHA, self.root)

    def test_published_versions_are_never_overwritten(self) -> None:
        with patch.object(release, "gh_json", return_value={"draft": False}) as api:
            self.assertFalse(release.release_plan("0.3.0", "owner/repo", SHA))
        self.assertEqual(api.call_count, 1)

    def test_new_version_is_releasable(self) -> None:
        with patch.object(release, "gh_json", side_effect=[None, {"tag_name": "v0.2.0"}, None]):
            self.assertTrue(release.release_plan("0.3.0", "owner/repo", SHA))

    def test_first_release_is_releasable(self) -> None:
        with patch.object(release, "gh_json", side_effect=[None, None, None]):
            self.assertTrue(release.release_plan("0.3.0", "owner/repo", SHA))

    def test_downgrade_blocks_publication(self) -> None:
        with patch.object(release, "gh_json", side_effect=[None, {"tag_name": "v0.3.2"}]):
            with self.assertRaisesRegex(ValueError, "higher"):
                release.release_plan("0.3.0", "owner/repo", SHA)

    def test_existing_tag_must_match_the_tested_commit(self) -> None:
        with patch.object(release, "gh_json", side_effect=[{"draft": True}, {"tag_name": "v0.2.0"},
                                                           {"object": {}}, {"sha": "b" * 40}]):
            with self.assertRaisesRegex(ValueError, "another commit"):
                release.release_plan("0.3.0", "owner/repo", SHA)

    def test_draft_for_the_same_commit_can_resume(self) -> None:
        with patch.object(release, "gh_json", side_effect=[{"draft": True}, {"tag_name": "v0.2.0"},
                                                           {"object": {}}, {"sha": SHA}]):
            self.assertTrue(release.release_plan("0.3.0", "owner/repo", SHA))


class RepositoryTests(unittest.TestCase):
    """Run the validator against the real checkout so a forgotten bump fails in pull requests."""

    def test_repository_versions_agree(self) -> None:
        self.assertRegex(release.validate(), r"^\d+\.\d+\.\d+$")


if __name__ == "__main__":
    unittest.main()
