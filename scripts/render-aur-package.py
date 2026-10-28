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
parser.add_argument("--local-source", action="store_true", help="use the archive's file URL for a local packaging check")
args = parser.parse_args()
if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", args.version):
    raise SystemExit("AUR requires a stable version in MAJOR.MINOR.PATCH form")
root = Path(__file__).resolve().parent.parent
package = tomllib.loads((root / "Cargo.toml").read_text())["package"]
license_id = package.get("license", "")
if not re.fullmatch(r"[A-Za-z0-9.+-]+", license_id):
    raise SystemExit("Cargo.toml must declare one SPDX license identifier before publishing")
archive = args.archive.resolve(strict=True)
url = f"https://github.com/partanskiy/lyfy/releases/download/v{args.version}/lyfy-v{args.version}-source.tar.gz"
if args.local_source:
    url = archive.as_uri()
checksum = hashlib.file_digest(archive.open("rb"), "sha256").hexdigest()
text = (root / "packaging/aur/PKGBUILD.in").read_text()
for key, value in {"VERSION": args.version, "LICENSE": license_id,
                   "SOURCE_URL": url, "SOURCE_SHA256": checksum}.items():
    text = text.replace(f"@{key}@", value)
args.output.mkdir(parents=True, exist_ok=True)
(args.output / "PKGBUILD").write_text(text)
source_info = subprocess.check_output(["makepkg", "--printsrcinfo"], cwd=args.output, text=True)
(args.output / ".SRCINFO").write_text(source_info)
print(f"Rendered lyfy {args.version} in {args.output}")
