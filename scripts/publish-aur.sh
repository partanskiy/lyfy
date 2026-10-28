#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 ]]; then
  echo 'Usage: publish-aur.sh VERSION RENDERED_PACKAGE_DIR' >&2
  exit 2
fi
version=$1
rendered=$(cd "$2" && pwd)
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
: "${AUR_SSH_PRIVATE_KEY:?missing AUR publishing credential}"
test -s "$rendered/PKGBUILD"
test -s "$rendered/.SRCINFO"

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT
chmod 700 "$workdir"
printf '%s\n' "$AUR_SSH_PRIVATE_KEY" > "$workdir/key"
chmod 600 "$workdir/key"
unset AUR_SSH_PRIVATE_KEY
ssh-keyscan -t ed25519 aur.archlinux.org 2>/dev/null > "$workdir/known_hosts"
fingerprint=$(ssh-keygen -lf "$workdir/known_hosts" -E sha256 | awk '{print $2}')
if [[ "$fingerprint" != 'SHA256:RFzBCUItH9LZS0cKB5UE6ceAYhBD5C8GeOBip8Z11+4' ]]; then
  echo 'AUR SSH host fingerprint differs from the published value.' >&2
  exit 1
fi
export GIT_SSH_COMMAND="ssh -i $workdir/key -o IdentitiesOnly=yes -o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$workdir/known_hosts"
remote='ssh://aur@aur.archlinux.org/lyfy.git'
git clone "$remote" "$workdir/repository"
cd "$workdir/repository"
if [[ -f .SRCINFO ]]; then
  previous=$(awk '$1 == "pkgver" {print $3; exit}' .SRCINFO)
  if [[ -n "$previous" ]] && (( $(vercmp "$previous" "$version") > 0 )); then
    echo 'Refusing to replace a newer AUR release with an older version.' >&2
    exit 1
  fi
fi
git config user.name 'Ilya Partanskiy'
git config user.email 'partanskiy.im@gmail.com'
install -m644 "$rendered/PKGBUILD" PKGBUILD
install -m644 "$rendered/.SRCINFO" .SRCINFO
git add PKGBUILD .SRCINFO
if git diff --cached --quiet; then
  echo "AUR lyfy $version is already current."
  exit 0
fi
git -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -m "chore: publish lyfy $version-1"
git push origin HEAD:master
