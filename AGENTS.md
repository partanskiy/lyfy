# Project instructions

- Read ARCHITECTURE.md first and enforce its layer boundaries.
- The core is the lyfy library target; inputs own CLI, ENV and TOML configuration.
- Keep exactly four independent managers: lint, fmt, save, apply.
- Managers must never import or invoke one another; share named services.
- Do not add an outputs layer or a fifth command without an architecture discussion.
- Target Arch Linux only. Never mutate the developer's host during tests.
- Keep main protected; use topic branches and pull requests, rebase merge only.
- Use small Conventional Commits. No merge, fixup, squash, amend or release-only exceptions.
- A commit changes at most 300 lines and 12 files, excluding generated Cargo.lock.
- Releases tag existing main commits. Never require a dedicated release commit.
- All configuration fields need a snake_case flag and LYFY_UPPER_CASE environment variable.
- Preserve flag > environment > TOML > default precedence.
- Store concrete files and host overlays. Do not introduce configuration templates.
- Keep secrets out of fixtures, output, Git and documentation.
- Use ASCII punctuation in prose and comments.
- Run architecture checks, rustfmt, clippy, library and CLI tests, release build and commit validation before opening a PR.
