#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 1 || ! "$1" =~ ^[0-9]{2}\.([1-9]|1[0-2])\.(0|[1-9][0-9]*)$ ]]; then
  echo 'Expected YY.M.PATCH: two-digit year, month 1-12 without padding, nonnegative patch without padding.' >&2
  exit 2
fi
