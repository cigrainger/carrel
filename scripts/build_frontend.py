#!/usr/bin/env python3
"""Prepare required dependency notices before building the packaged webview.

Development hot reload does not need this step. Production builds must fail if
dependency licensing cannot be resolved; silently omitting notices is not a
successful package. The generated report is ignored by Git.
"""

import argparse
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "crates/carrel-app"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--notices-only", action="store_true")
    args = parser.parse_args()
    subprocess.run([
        "cargo", "about", "generate", "--workspace", "--all-features",
        "--locked", "--fail", "scripts/licenses.hbs", "--output-file",
        str(APP / "frontend/public/licenses/rust-dependencies.html"),
    ], cwd=ROOT, check=True)
    if not args.notices_only:
        subprocess.run([
            "trunk", "build", "--release", "--locked", "--config", "Trunk.toml",
            "--no-default-features", "--features", "csr",
        ], cwd=APP, check=True)


if __name__ == "__main__":
    main()
