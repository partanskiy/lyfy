#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
  echo 'Usage: check-arch-package.sh VERSION WORKDIR' >&2
  exit 2
fi
version=$1
workdir=$2
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ $(uname -m) == x86_64 ]]
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p "$workdir"
workdir=$(cd "$workdir" && pwd)
archive="$workdir/lyfy-v$version-source.tar.gz"
git -c safe.directory="$repo_root" -C "$repo_root" archive --format=tar --prefix="lyfy-$version/" HEAD | gzip -n > "$archive"
python3 "$repo_root/scripts/render-aur-package.py" "$version" "$archive" "$workdir/package" --local-source
(
  cd "$workdir/package"
  makepkg --noconfirm --cleanbuild
)
package="$workdir/package/lyfy-$version-1-x86_64.pkg.tar.zst"
test -f "$package"
mkdir -p "$workdir/extracted"
bsdtar -xf "$package" -C "$workdir/extracted"
test "$("$workdir/extracted/usr/bin/lyfy" --version)" = "lyfy $version"
test -s "$workdir/extracted/usr/share/licenses/lyfy/LICENSE"
test -s "$workdir/extracted/usr/share/licenses/lyfy/THIRD_PARTY_LICENSES.txt"
bsdtar -tf "$package" | while IFS= read -r path; do
  case "$path" in
    .PKGINFO|.BUILDINFO|.MTREE|usr/|usr/bin/|usr/bin/lyfy|usr/share/|usr/share/doc/*|usr/share/licenses/*) ;;
    *) echo "Unexpected installed path: $path" >&2; exit 1 ;;
  esac
done
echo "Arch package checked: $package"
