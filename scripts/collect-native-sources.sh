#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 1 ]]; then
  echo 'Usage: collect-native-sources.sh OUTPUT_DIR' >&2
  exit 2
fi
output_dir=$(realpath -m "$1")
mkdir -p "$output_dir"
read -r package_name package_version < <(/usr/bin/pacman -Q pacman)
[[ "$package_name" == pacman && "$package_version" =~ ^[a-zA-Z0-9.:+-]+$ ]]
arch_version=${package_version#*:}
upstream_version=${arch_version%-*}
if [[ "$upstream_version" =~ \.r[0-9]+\.g([0-9a-f]+)$ ]]; then
  source_ref=${BASH_REMATCH[1]}
else
  source_ref="v$upstream_version"
fi
workdir=$(mktemp -d /tmp/lyfy-native-source.XXXXXXXX)
trap 'rm -rf "$workdir"' EXIT
git clone --quiet --filter=blob:none --no-checkout \
  https://gitlab.archlinux.org/pacman/pacman.git "$workdir/upstream"
git clone --quiet --depth 1 --branch "$arch_version" \
  https://gitlab.archlinux.org/archlinux/packaging/packages/pacman.git "$workdir/packaging"
commit=$(git -C "$workdir/upstream" rev-parse "$source_ref^{commit}")
git -C "$workdir/upstream" archive --prefix="pacman-$upstream_version/" "$commit" | \
  gzip -n > "$output_dir/pacman-$upstream_version.tar.gz"
git -C "$workdir/packaging" archive --prefix=arch-packaging/ HEAD | \
  gzip -n > "$output_dir/arch-pacman-packaging.tar.gz"
printf 'pacman=%s\nlibalpm=%s\nlibarchive=%s\npacman_upstream_commit=%s\n' \
  "$package_version" "$(pkg-config --modversion libalpm)" \
  "$(pkg-config --modversion libarchive)" "$commit" > "$output_dir/VERSIONS"
