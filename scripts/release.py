#!/usr/bin/env python3
"""Validate, collect and describe a Carrel release without publishing it.

The workflow fixes a source commit before builds start. Collection accepts one
artifact per expected platform/format, then records their exact bytes alongside
an archive of that source commit. GitHub publication is a separate workflow step.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
FORMATS = {
    "macos-arm64": ("dmg",),
    "macos-x86_64": ("dmg",),
    "linux-x86_64": ("deb", "AppImage"),
}


def project_version(root=ROOT):
    """Require the native package and bundle to describe the same version."""
    config = json.loads((root / "crates/carrel-app/tauri.conf.json").read_text())
    manifest = tomllib.loads((root / "crates/carrel-app/Cargo.toml").read_text())
    version = config["version"]
    if version != manifest["package"]["version"]:
        raise ValueError("Cargo and Tauri app versions differ")
    return version


def validate_tag(tag, root=ROOT):
    """Accept only the exact version, never arbitrary shell/ref input."""
    version = project_version(root)
    if tag != f"v{version}":
        raise ValueError(f"expected tag v{version}, received {tag!r}")
    release_notes(version, root)
    return version


def release_notes(version, root=ROOT):
    """Take the current version's notes, excluding other release history."""
    changelog = (root / "CHANGELOG.md").read_text()
    heading = f"## {version}\n"
    if heading not in changelog:
        raise ValueError(f"CHANGELOG.md has no {version} entry")
    notes = changelog.split(heading, 1)[1].split("\n## ", 1)[0].strip()
    if not notes:
        raise ValueError("release notes are empty")
    return notes + "\n"


def artifact_name(version, platform, suffix):
    return f"Carrel-{version}-{platform}.{suffix}"


def package(platform, bundle_dir, output, root=ROOT):
    """Normalize native bundler names, refusing ambiguous or empty outputs."""
    version = project_version(root)
    selected = []
    for suffix in FORMATS[platform]:
        candidates = list(bundle_dir.rglob(f"*.{suffix}"))
        if len(candidates) != 1:
            raise ValueError(f"expected one {suffix} for {platform}, found {len(candidates)}")
        source = candidates[0]
        if source.is_symlink() or not source.is_file() or source.stat().st_size == 0:
            raise ValueError(f"invalid release artifact: {source.name}")
        selected.append((source, artifact_name(version, platform, suffix)))
    output.mkdir(parents=True, exist_ok=True)
    for source, name in selected:
        shutil.copy2(source, output / name)


def digest(path):
    """Hash large native packages without reading them all into memory."""
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def assemble(tag, artifacts, output, root=ROOT):
    """Require the complete matrix, add matching source, and generate checksums."""
    version = validate_tag(tag, root)
    expected = {
        artifact_name(version, platform, suffix)
        for platform, suffixes in FORMATS.items()
        for suffix in suffixes
    }
    selected = {}
    for source in artifacts.rglob("*"):
        if source.is_dir():
            continue
        if source.name not in expected or source.name in selected:
            raise ValueError(f"unexpected or duplicate artifact: {source.name}")
        if source.is_symlink() or not source.is_file() or source.stat().st_size == 0:
            raise ValueError(f"invalid release artifact: {source.name}")
        selected[source.name] = source
    if selected.keys() != expected:
        raise ValueError(f"missing artifacts: {', '.join(sorted(expected - selected.keys()))}")
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError("assembly output must be empty")
    for name, source in selected.items():
        shutil.copy2(source, output / name)

    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    source_name = f"Carrel-{version}-source.tar.gz"
    subprocess.run([
        "git", "archive", "--format=tar.gz", f"--prefix=carrel-{version}/",
        "-o", str((output / source_name).resolve()), commit,
    ], cwd=root, check=True)
    files = sorted(output.iterdir())
    manifest = {
        "version": version,
        "sourceCommit": commit,
        "license": "AGPL-3.0-or-later",
        "artifacts": [
            {"file": file.name, "bytes": file.stat().st_size, "sha256": digest(file)}
            for file in files
        ],
    }
    (output / "release-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    checksums = "".join(f"{digest(file)}  {file.name}\n" for file in sorted(output.iterdir()))
    (output / "SHA256SUMS").write_text(checksums)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    validate.add_argument("tag")
    collect = commands.add_parser("package")
    collect.add_argument("platform", choices=FORMATS)
    collect.add_argument("bundle_dir", type=Path)
    collect.add_argument("output", type=Path)
    merge = commands.add_parser("assemble")
    merge.add_argument("tag")
    merge.add_argument("artifacts", type=Path)
    merge.add_argument("output", type=Path)
    notes = commands.add_parser("notes")
    notes.add_argument("tag")
    notes.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "validate":
            print(validate_tag(args.tag))
        elif args.command == "package":
            package(args.platform, args.bundle_dir, args.output)
        elif args.command == "assemble":
            assemble(args.tag, args.artifacts, args.output)
        elif args.command == "notes":
            args.output.write_text(release_notes(validate_tag(args.tag)))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
