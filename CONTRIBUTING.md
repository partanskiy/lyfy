# Contributing to lyfy

Read [ARCHITECTURE.md](ARCHITECTURE.md) first. Its boundaries are mandatory.

## Branch policy

- `main` is the default branch and is protected without bypass actors.
- All changes reach main through pull requests.
- Merge mode is rebase only. Merge commits, squash merge and force pushes to
  main are disabled. Resolve conflicts on the topic branch before merging.
- Required checks cover architecture, formatting, clippy, tests, release build
  and every individual commit in the PR.

## Small Conventional Commits

Each commit must have one purpose and this subject format:

```text
<type>[(scope)][!]: <description>
```

Allowed types: feat, fix, refactor, docs, test, chore, perf, build, ci, style,
revert. Subjects have at most 72 characters.

No special exceptions for merge, fixup, squash, amend or release messages.
Undo changes with a new conventional `revert:` commit. Do not leave temporary
history-cleanup commits in a PR.

Each commit changes at most 300 lines and 12 files. Cargo.lock is generated and
excluded from these size limits. Renames count the actual edited lines. Binary
assets are not accepted in development commits. These are maximum limits, not
targets; keep related work smaller when possible.

Enable local checks once per clone:

```sh
git config core.hooksPath .githooks
```

Hooks validate the subject and staged size and reject direct pushes to main.
CI checks all PR commits, so local hooks are a convenience, not the protection
boundary. The GitHub ruleset in `.github/rules/main.json` records the intended
server configuration.

## Development loop

```sh
python3 scripts/check-architecture.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --locked --release
bash scripts/check-commits.sh --range origin/main..HEAD
```

Tests use temporary target roots and synthetic data. Never run fmt/apply with
`--write` against a developer's real system as a test. Core changes must remain
usable through the library API without CLI/environment configuration.

## Releases from existing commits

A release is a version tag pointing at a selected existing main commit. No
dedicated release commit, changelog commit or version-bump commit is required.
Do not add a `release:` commit type.

```sh
git fetch origin main
git tag -a v0.1.0 <selected-main-commit> -m 'lyfy v0.1.0'
git push origin v0.1.0
```

The release workflow verifies the tagged commit belongs to main, reruns checks,
builds the executable and publishes a GitHub release with an archive and
checksum. The version tag sets the built CLI version through LYFY_BUILD_VERSION.
Cargo's package version remains development metadata and does not force a
special release commit. Release tags are immutable by convention.
