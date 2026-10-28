#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
for version in 26.1.0 26.10.0 26.10.1 26.12.42; do
  bash "$repo_root/scripts/check-version.sh" "$version"
done
for version in 0.1.2 2026.10.0 6.10.0 26.0.0 26.13.0 26.01.0 26.10.01 26.10.-1 v26.10.0 26.10.0-rc.1 ''; do
  if bash "$repo_root/scripts/check-version.sh" "$version" 2>/dev/null; then
    echo "Accepted invalid release version: $version" >&2
    exit 1
  fi
done
if command -v vercmp >/dev/null; then
  test "$(vercmp 26.10.0 0.1.2)" -gt 0
  test "$(vercmp 26.10.1 26.10.0)" -gt 0
  test "$(vercmp 26.11.0 26.10.42)" -gt 0
  test "$(vercmp 27.1.0 26.12.42)" -gt 0
  echo 'Pacman upgrade ordering passed.'
fi
echo 'Calendar version validation passed.'
