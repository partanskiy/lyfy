#!/usr/bin/env python3
"""Collect exact dependency license/notice texts from the locked Cargo sources."""
import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("output", type=Path)
args = parser.parse_args()
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--format-version", "1", "--frozen", "--all-features",
    "--filter-platform", "x86_64-unknown-linux-gnu",
]))
sections = ["Third-party notices for lyfy (x86_64 Linux).\nGenerated from Cargo.lock.\n"]
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["name"] == "lyfy" and package["source"] is None:
        continue
    root = Path(package["manifest_path"]).parent
    notices = sorted(path for path in root.iterdir() if path.is_file()
                     and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")))
    # alpm's published crates declare GPL-3.0 but omit the workspace LICENSE.
    # Arch's pacman-managed licenses package provides the canonical full text.
    if not notices and package.get("license") in {"GPL-3.0", "GPL-3.0-only"}:
        canonical = Path("/usr/share/licenses/spdx/GPL-3.0-only.txt")
        if canonical.is_file():
            notices = [canonical]
    if not notices:
        raise SystemExit(f"Missing license texts: {package['name']} {package['version']}")
    sections.append(f"\n{package['name']} {package['version']}\nSPDX: {package.get('license') or 'see license text'}\n")
    sections.append("Authors: " + ", ".join(package.get("authors", [])) + "\n")
    for path in notices:
        sections.append(f"\n{path.name}\n\n{path.read_text()}\n")
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text("".join(sections))
print(f"Wrote dependency notices to {args.output}")
