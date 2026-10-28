#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
  echo 'Usage: check-arch-package.sh VERSION WORKDIR' >&2
  exit 2
fi
version=$1
workdir=$2
[[ $(uname -m) == x86_64 ]]
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
bash "$repo_root/scripts/check-version.sh" "$version"
for page in "$repo_root/docs/man/lyfy.1" "$repo_root/docs/man/lyfy.conf.5"; do
  groff -Tascii -man -z -ww "$page"
done
mkdir -p "$workdir"
workdir=$(cd "$workdir" && pwd)
archive="$workdir/lyfy-v$version-source.tar.gz"
(
  cd "$repo_root"
  cargo fetch --locked --target x86_64-unknown-linux-gnu
)
bash "$repo_root/scripts/archive-source.sh" "$version" "$archive"
python3 "$repo_root/scripts/render-aur-package.py" "$version" "$archive" "$workdir/package"
if grep -Eq 'python[0-9]*|scripts/license-notices' "$workdir/package/PKGBUILD"; then
  echo 'The AUR recipe must not execute Python or depend on it.' >&2
  exit 1
fi
install -m644 "$archive" "$workdir/package/lyfy-$version.tar.gz"
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
for page in man1/lyfy.1 man5/lyfy.conf.5; do
  test -s "$workdir/extracted/usr/share/man/$page" || test -s "$workdir/extracted/usr/share/man/$page.gz"
done
bsdtar -tf "$package" | while IFS= read -r path; do
  case "$path" in
    .PKGINFO|.BUILDINFO|.MTREE|usr/|usr/bin/|usr/bin/lyfy|usr/share/|usr/share/doc/*|usr/share/licenses/*|usr/share/man/*) ;;
    *) echo "Unexpected installed path: $path" >&2; exit 1 ;;
  esac
done
echo "Arch package checked: $package"
