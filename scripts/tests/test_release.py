"""Boundary tests for the source/version/artifact contract of the release job."""

import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
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

    def prepare_finalization(self):
        self.matrix()
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        subprocess.run(["git", "add", "CHANGELOG.md", "crates"], cwd=self.root, check=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "commit", "--quiet", "-m", "fixture"], cwd=self.root, check=True)
        subprocess.run(["git", "tag", "v0.1.0-alpha.1"], cwd=self.root, check=True)
        release.assemble("v0.1.0-alpha.1", self.artifacts, self.output, self.root)
        self.signed = Path(self.temporary.name) / "signed"
        self.signed.mkdir()
        for file in self.output.glob("*.dmg"):
            (self.signed / file.name).write_bytes(file.read_bytes() + b" signed and stapled")
        self.final = Path(self.temporary.name) / "final"

    def finalize(self):
        release.finalize_macos("v0.1.0-alpha.1", self.output, self.signed, self.final, self.root)

    def test_finalization_preserves_tagged_source_and_linux_when_head_has_advanced(self):
        self.prepare_finalization()
        baseline = {file.name: file.read_bytes() for file in self.output.iterdir()}
        tagged_commit = json.loads(baseline["release-manifest.json"])["sourceCommit"]
        (self.root / "later.txt").write_text("This is not the source of the tested binaries.")
        subprocess.run(["git", "add", "later.txt"], cwd=self.root, check=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid", "commit", "--quiet", "-m", "later"], cwd=self.root, check=True)
        self.finalize()
        manifest = json.loads((self.final / "release-manifest.json").read_text())
        self.assertEqual(manifest["sourceCommit"], tagged_commit)
        self.assertEqual({file.name for file in self.final.iterdir()}, baseline.keys())
        for record in manifest["artifacts"]:
            file = self.final / record["file"]
            self.assertEqual(file.stat().st_size, record["bytes"])
            self.assertEqual(release.digest(file), record["sha256"])
            expected = (self.signed / file.name).read_bytes() if file.suffix == ".dmg" else baseline[file.name]
            self.assertEqual(file.read_bytes(), expected)
        for line in (self.final / "SHA256SUMS").read_text().splitlines():
            checksum, name = line.split("  ", 1)
            self.assertEqual(release.digest(self.final / name), checksum)
        self.assertEqual({file.name: file.read_bytes() for file in self.output.iterdir()}, baseline)

    def test_finalization_rejects_corrupt_downloads_before_creating_output(self):
        self.prepare_finalization()
        for name in ["Carrel-0.1.0-alpha.1-linux-x86_64.deb", "SHA256SUMS"]:
            with self.subTest(name=name):
                file = self.output / name
                original = file.read_bytes()
                file.write_bytes(original + b"corrupted")
                with self.assertRaises(ValueError):
                    self.finalize()
                self.assertFalse(self.final.exists())
                file.write_bytes(original)

    def test_finalization_rejects_wrong_or_incomplete_manifest(self):
        self.prepare_finalization()
        file = self.output / "release-manifest.json"
        original = file.read_text()
        variants = [
            {"version": "0.2.0"}, {"sourceCommit": "0" * 40}, {"license": "MIT"},
            {"artifacts": []}, {"artifacts": None}, {"artifacts": [42]},
        ]
        records = json.loads(original)["artifacts"]
        variants.append({"artifacts": records[:-1] + [records[0]]})
        variants.append({"artifacts": records + [records[0]]})
        for changes in variants:
            with self.subTest(changes=changes):
                manifest = json.loads(original) | changes
                file.write_text(json.dumps(manifest))
                with self.assertRaises(ValueError):
                    self.finalize()
                self.assertFalse(self.final.exists())
        file.write_text("[]")
        with self.assertRaisesRegex(ValueError, "JSON object"):
            self.finalize()

    def test_finalization_rejects_self_consistent_but_wrong_source_archive(self):
        self.prepare_finalization()
        manifest = json.loads((self.output / "release-manifest.json").read_text())
        (self.output / "Carrel-0.1.0-alpha.1-source.tar.gz").write_bytes(b"a different source tree")
        (self.output / "release-manifest.json").unlink()
        (self.output / "SHA256SUMS").unlink()
        release.write_metadata("0.1.0-alpha.1", manifest["sourceCommit"], self.output)
        with self.assertRaisesRegex(ValueError, "source archive differs"):
            self.finalize()
        self.assertFalse(self.final.exists())

    def test_finalization_requires_exact_nonempty_regular_changed_mac_packages(self):
        self.prepare_finalization()
        first = next(self.signed.glob("*.dmg"))
        original = first.read_bytes()
        for kind in ["missing", "extra", "empty", "symlink", "directory", "unchanged"]:
            with self.subTest(kind=kind):
                if kind == "missing":
                    first.unlink()
                elif kind == "extra":
                    (self.signed / "unexpected.txt").write_text("stale")
                elif kind == "empty":
                    first.write_bytes(b"")
                elif kind == "symlink":
                    first.unlink()
                    first.symlink_to(self.output / first.name)
                elif kind == "directory":
                    first.unlink()
                    first.mkdir()
                else:
                    shutil.copy2(self.output / first.name, first)
                with self.assertRaises(ValueError):
                    self.finalize()
                self.assertFalse(self.final.exists())
                if first.is_dir():
                    first.rmdir()
                elif first.exists() or first.is_symlink():
                    first.unlink()
                first.write_bytes(original)
                (self.signed / "unexpected.txt").unlink(missing_ok=True)

    def test_finalization_refuses_to_overwrite_an_existing_output(self):
        self.prepare_finalization()
        self.final.mkdir()
        sentinel = self.final / "keep.txt"
        sentinel.write_text("retain me")
        with self.assertRaisesRegex(ValueError, "output must be empty"):
            self.finalize()
        self.assertEqual(sentinel.read_text(), "retain me")


if __name__ == "__main__":
    unittest.main()
