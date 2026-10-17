"""Boundary tests for the source/version/artifact contract of the release job."""

import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).parents[1] / "release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "source"
        app = self.root / "crates/carrel-app"
        app.mkdir(parents=True)
        (app / "Cargo.toml").write_text('[package]\nversion = "0.1.0-alpha.1"\n')
        (app / "tauri.conf.json").write_text('{"version":"0.1.0-alpha.1"}')
        (self.root / "CHANGELOG.md").write_text("# Changes\n\n## 0.1.0-alpha.1\n\nTrial.\n\n## older\n\nOld.\n")
        self.artifacts = Path(self.temporary.name) / "artifacts"
        self.artifacts.mkdir()
        self.output = Path(self.temporary.name) / "out"

    def matrix(self):
        for platform, suffixes in release.FORMATS.items():
            folder = self.artifacts / platform
            folder.mkdir()
            for suffix in suffixes:
                (folder / release.artifact_name("0.1.0-alpha.1", platform, suffix)).write_bytes(platform.encode())

    def test_tag_and_changelog_are_bound_to_app_version(self):
        self.assertEqual(release.validate_tag("v0.1.0-alpha.1", self.root), "0.1.0-alpha.1")
        self.assertEqual(release.release_notes("0.1.0-alpha.1", self.root), "Trial.\n")
        for tag in ["", "v0.1.0", "main", "v0.1.0-alpha.1\nmalicious=output", "v0.1.0-alpha.1;echo bad"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.validate_tag(tag, self.root)
        (self.root / "crates/carrel-app/tauri.conf.json").write_text('{"version":"0.2.0"}')
        with self.assertRaises(ValueError):
            release.project_version(self.root)

    def test_missing_or_empty_release_notes_are_rejected(self):
        for text in ["# No release", "## 0.1.0-alpha.1\n\n## older\nOld"]:
            (self.root / "CHANGELOG.md").write_text(text)
            with self.assertRaises(ValueError):
                release.validate_tag("v0.1.0-alpha.1", self.root)

    def test_package_requires_one_nonempty_artifact_per_format(self):
        bundle = Path(self.temporary.name) / "bundles"
        bundle.mkdir()
        with self.assertRaises(ValueError):
            release.package("macos-arm64", bundle, self.output, self.root)
        first = bundle / "native.dmg"
        first.touch()
        with self.assertRaises(ValueError):
            release.package("macos-arm64", bundle, self.output, self.root)
        first.write_bytes(b"native bytes")
        release.package("macos-arm64", bundle, self.output, self.root)
        expected = self.output / "Carrel-0.1.0-alpha.1-macos-arm64.dmg"
        self.assertEqual(expected.read_bytes(), b"native bytes")
        (bundle / "old.dmg").write_bytes(b"stale")
        with self.assertRaises(ValueError):
            release.package("macos-arm64", bundle, self.output, self.root)

    def test_assembly_rejects_missing_duplicate_and_unknown_assets(self):
        with self.assertRaises(ValueError):
            release.assemble("v0.1.0-alpha.1", self.artifacts, self.output, self.root)
        self.matrix()
        first = next(self.artifacts.rglob("*.dmg"))
        duplicate = self.artifacts / first.name
        duplicate.write_bytes(first.read_bytes())
        with self.assertRaises(ValueError):
            release.assemble("v0.1.0-alpha.1", self.artifacts, self.output, self.root)
        duplicate.unlink()
        (self.artifacts / "unexpected.txt").write_text("wrong workflow")
        with self.assertRaises(ValueError):
            release.assemble("v0.1.0-alpha.1", self.artifacts, self.output, self.root)

    def test_assembly_checks_every_byte_and_archives_the_exact_source_commit(self):
        self.matrix()
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        subprocess.run(["git", "add", "CHANGELOG.md", "crates"], cwd=self.root, check=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "commit", "--quiet", "-m", "fixture"], cwd=self.root, check=True)
        release.assemble("v0.1.0-alpha.1", self.artifacts, self.output, self.root)
        manifest = json.loads((self.output / "release-manifest.json").read_text())
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        self.assertEqual(manifest["sourceCommit"], commit)
        for artifact in manifest["artifacts"]:
            data = (self.output / artifact["file"]).read_bytes()
            self.assertEqual(artifact["sha256"], hashlib.sha256(data).hexdigest())
            self.assertEqual(artifact["bytes"], len(data))
        for line in (self.output / "SHA256SUMS").read_text().splitlines():
            checksum, name = line.split("  ", 1)
            self.assertEqual(checksum, release.digest(self.output / name))
        self.assertEqual(len((self.output / "SHA256SUMS").read_text().splitlines()), 6)
        with tarfile.open(self.output / "Carrel-0.1.0-alpha.1-source.tar.gz") as archive:
            self.assertIn("carrel-0.1.0-alpha.1/CHANGELOG.md", archive.getnames())
        with self.assertRaises(ValueError):
            release.assemble("v0.1.0-alpha.1", self.artifacts, self.output, self.root)


if __name__ == "__main__":
    unittest.main()
