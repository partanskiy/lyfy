#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 ]]; then
  echo 'Usage: archive-source.sh VERSION OUTPUT_ARCHIVE' >&2
  exit 2
fi
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
version=$1
bash "$repo_root/scripts/check-version.sh" "$version"
output=$(realpath -m "$2")
workdir=$(mktemp -d /tmp/lyfy-source-archive.XXXXXXXX)
trap 'rm -rf "$workdir"' EXIT
source_root="$workdir/lyfy-$version"
mkdir -p "$source_root/.cargo" "$(dirname "$output")"
git -c safe.directory="$repo_root" -C "$repo_root" archive HEAD | tar -xf - -C "$source_root"
python3 "$repo_root/scripts/license-notices.py" "$source_root/THIRD_PARTY_LICENSES.txt"
(
  cd "$source_root"
  cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml
)
bash "$repo_root/scripts/collect-native-sources.sh" "$source_root/native-sources"
tar -czf "$output" -C "$workdir" "lyfy-$version"
echo "Created corresponding source archive: $output"
