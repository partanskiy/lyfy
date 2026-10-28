#!/usr/bin/env bash
set -euo pipefail

check_message() {
    local subject
    IFS= read -r subject < "$1" || true
    local pattern='^(feat|fix|refactor|docs|test|chore|perf|build|ci|style|revert)(\([a-z0-9._/-]+\))?!?: [^[:space:]].*$'
    if [[ ! "$subject" =~ $pattern ]] || (( ${#subject} > 72 )); then
        echo 'Expected a Conventional Commit subject of at most 72 characters.' >&2
        return 1
    fi
}

check_size() {
    local added removed path lines=0 files=0
    while IFS=$'\t' read -r added removed path; do
        [[ -z "$path" || "$path" == Cargo.lock ]] && continue
        if [[ "$added" == - || "$removed" == - ]]; then
            echo "Binary changes need a separate distribution mechanism: $path" >&2
            return 1
        fi
        lines=$((lines + added + removed))
        files=$((files + 1))
    done
    if (( lines > 300 || files > 12 )); then
        echo "Commit is too large: $lines changed lines, $files files (limits: 300, 12)." >&2
        return 1
    fi
}

case "${1:-}" in
    --message) check_message "$2" ;;
    --staged) check_size < <(git diff --cached --numstat -M) ;;
    --range)
        temp=$(mktemp)
        trap 'rm -f "$temp"' EXIT
        while IFS= read -r sha; do
            parents=$(git show -s --format=%P "$sha")
            if [[ "$parents" == *' '* ]]; then
                echo "Merge commit is forbidden: $sha" >&2
                exit 1
            fi
            git show -s --format=%s "$sha" > "$temp"
            check_message "$temp"
            check_size < <(git diff-tree --root --no-commit-id --numstat -r -M "$sha")
        done < <(git rev-list --reverse "$2")
        ;;
    *) echo 'Usage: check-commits.sh --message FILE | --staged | --range BASE..HEAD' >&2; exit 2 ;;
esac
