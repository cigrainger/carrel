#!/usr/bin/env python3
"""Validate, collect and describe a Carrel release without publishing it.

The workflow fixes a source commit before builds start. Collection accepts one
artifact per expected platform/format, then records their exact bytes alongside
an archive of that source commit. Publication is a separate maintainer action.
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


def write_metadata(version, commit, output):
    """Describe only final package bytes; signatures and tickets change hashes."""
    manifest = {
        "version": version,
        "sourceCommit": commit,
        "license": "AGPL-3.0-or-later",
        "artifacts": [
            {"file": file.name, "bytes": file.stat().st_size, "sha256": digest(file)}
            for file in sorted(output.iterdir())
        ],
    }
    (output / "release-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    checksums = "".join(f"{digest(file)}  {file.name}\n" for file in sorted(output.iterdir()))
    (output / "SHA256SUMS").write_text(checksums)


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
    write_metadata(version, commit, output)


def finalize_macos(tag, draft, signed, output, root=ROOT):
    """Replace verified draft DMGs without relabeling binaries as today's HEAD.

    Apple signing/notarization and Gatekeeper checks happen before this step.
    This platform-independent step validates the downloaded draft, retains its
    exact source/Linux bytes, and regenerates metadata for the final packages.
    """
    version = validate_tag(tag, root)
    macos_names = {
        artifact_name(version, platform, "dmg")
        for platform in FORMATS if platform.startswith("macos-")
    }
    source_name = f"Carrel-{version}-source.tar.gz"
    expected = {
        artifact_name(version, platform, suffix)
        for platform, suffixes in FORMATS.items() for suffix in suffixes
    } | {source_name}
    if {file.name for file in draft.iterdir()} != expected | {"release-manifest.json", "SHA256SUMS"}:
        raise ValueError("draft must contain the complete release and no extra files")
    for file in draft.iterdir():
        if file.is_symlink() or not file.is_file() or file.stat().st_size == 0:
            raise ValueError(f"invalid draft artifact: {file.name}")

    manifest = json.loads((draft / "release-manifest.json").read_text())
    if not isinstance(manifest, dict):
        raise ValueError("draft manifest must be a JSON object")
    commit = subprocess.check_output([
        "git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}",
    ], cwd=root, text=True).strip()
    if (manifest.get("version") != version or manifest.get("sourceCommit") != commit
            or manifest.get("license") != "AGPL-3.0-or-later"):
        raise ValueError("draft manifest does not match the release tag, version and license")
    records = manifest.get("artifacts")
    if not isinstance(records, list) or any(not isinstance(item, dict) for item in records):
        raise ValueError("draft manifest artifacts must be a list of records")
    if len(records) != len(expected) or {item["file"] for item in records} != expected:
        raise ValueError("draft manifest must describe every package and matching source exactly once")
    for item in records:
        file = draft / item["file"]
        if file.stat().st_size != item["bytes"] or digest(file) != item["sha256"]:
            raise ValueError(f"draft artifact differs from manifest: {file.name}")
    checksums = "".join(
        f"{digest(draft / name)}  {name}\n"
        for name in sorted(expected | {"release-manifest.json"})
    )
    if (draft / "SHA256SUMS").read_text() != checksums:
        raise ValueError("draft checksums do not match its packages and manifest")

    # A self-consistent manifest could still name the wrong source archive.
    with subprocess.Popen([
        "git", "archive", "--format=tar.gz", f"--prefix=carrel-{version}/", commit,
    ], cwd=root, stdout=subprocess.PIPE) as archive:
        source_hash = hashlib.file_digest(archive.stdout, "sha256").hexdigest()
        if archive.wait() != 0:
            raise ValueError("could not archive the tagged source")
    if source_hash != digest(draft / source_name):
        raise ValueError("draft source archive differs from the release tag")

    if {file.name for file in signed.iterdir()} != macos_names:
        raise ValueError("signed directory must contain exactly the two final Mac DMGs")
    for name in macos_names:
        file = signed / name
        if file.is_symlink() or not file.is_file() or file.stat().st_size == 0:
            raise ValueError(f"invalid final Mac artifact: {name}")
        if digest(file) == digest(draft / name):
            raise ValueError(f"Mac artifact has not changed since the unsigned draft: {name}")

    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError("finalization output must be empty")
    for name in expected:
        shutil.copy2((signed if name in macos_names else draft) / name, output / name)
    write_metadata(version, commit, output)


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
    finalize = commands.add_parser("finalize-macos")
    finalize.add_argument("tag")
    finalize.add_argument("draft", type=Path)
    finalize.add_argument("signed", type=Path)
    finalize.add_argument("output", type=Path)
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
        elif args.command == "finalize-macos":
            finalize_macos(args.tag, args.draft, args.signed, args.output)
        elif args.command == "notes":
            args.output.write_text(release_notes(validate_tag(args.tag)))
    except (KeyError, TypeError, ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
