#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
graph=$(mktemp)
trap 'rm -f "$graph"' EXIT
cd "$repo_root"
cargo tree --locked --all-features --target all -p lyfy --prefix none > "$graph"
# url uses percent-encoding for S3 object paths; it is a shared pure codec.
if grep -E '^(lyfy-cli|clap|toml|config|dotenv|dotenvy|indicatif|console|serde_yaml_ng) ' "$graph"; then
  echo 'Library dependency graph contains launch or presentation packages.' >&2
  exit 1
fi
echo 'Library dependency graph contains no launch packages.'
