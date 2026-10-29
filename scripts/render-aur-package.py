#!/usr/bin/env python3
"""Render the source-only x86_64 AUR recipe with a verified archive checksum."""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import tomllib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("version")
parser.add_argument("archive", type=Path)
parser.add_argument("output", type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
version_check = subprocess.run(["bash", str(root / "scripts/check-version.sh"), args.version])
if version_check.returncode:
    raise SystemExit(version_check.returncode)
workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
cli_package = tomllib.loads((root / "crates/lyfy-cli/Cargo.toml").read_text())["package"]
license_id = workspace.get("package", {}).get("license", "")
binary_license = cli_package.get("metadata", {}).get("lyfy", {}).get("binary-license", "")
if not all(re.fullmatch(r"[A-Za-z0-9.+-]+", value) for value in (license_id, binary_license)):
    raise SystemExit("Workspace and CLI manifests must declare SPDX license identifiers before publishing")
archive = args.archive.resolve(strict=True)
with archive.open("rb") as source:
    checksum = hashlib.file_digest(source, "sha256").hexdigest()
text = (root / "packaging/aur/PKGBUILD.in").read_text()
for key, value in {"VERSION": args.version, "LICENSE": license_id, "BINARY_LICENSE": binary_license,
                   "SOURCE_SHA256": checksum}.items():
    text = text.replace(f"@{key}@", value)
args.output.mkdir(parents=True, exist_ok=True)
(args.output / "PKGBUILD").write_text(text)
source_info = subprocess.check_output(["makepkg", "--printsrcinfo"], cwd=args.output, text=True)
(args.output / ".SRCINFO").write_text(source_info)
print(f"Rendered lyfy {args.version} in {args.output}")
