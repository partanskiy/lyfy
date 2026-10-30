# State repository contract

The data_dir setting selects where state is stored. storage_format selects
`git` (default) or `directory`. Both can be overridden by snake_case flags and
LYFY_DATA_DIR / LYFY_STORAGE_FORMAT. These are core settings resolved by the
launch input, not extra commands.

## Git is the primary format

```text
<data_dir>/
  .git/
  hosts/
    common/
      manifest.json
      files/
        home/.config/editor/config.toml
        system/etc/example.conf
    laptop/
      manifest.json
      files/
        system/etc/example.conf
```

Payloads are ordinary files with their actual bytes. The manifest records the
host, pinned common base revision, node kinds, permissions, ownership, deletions
and package inventory. File contents are not embedded in JSON or hidden behind
hash filenames. Symlinks and directories are declarations in the manifest;
checking out the state repository does not create their target-system links.

Home ownership is symbolic and resolves from the destination home directory.
System ownership uses numeric uid/gid values. Git's executable bit does not
substitute for the saved target permissions.

save captures the selected host and makes a local Conventional Commit on the
current attached branch. It creates no commit for unchanged state, stages only
the owned host tree and refuses pre-existing tracked, staged, untracked or ignored
working-tree changes. It does not switch branches or push. Failed capture never
advances the Git branch. A failed Git commit can leave generated changes staged
for review/recovery; resolve that state before repeating save.

A snapshot ID is a full Git commit ID. apply reads committed state at HEAD or
the selected `--snapshot` revision. Uncommitted edits in the source repository
are not silently deployed. A host's base stores an exact commit ID, so ordinary
full clones contain all required state. Shallow clones or rewriting/pruning
history can remove pinned base commits and are not supported by that invariant.

Existing payload files may be edited with an ordinary editor and committed with
ordinary Git. apply then reads those new bytes without requiring content hash
updates or rendering templates. Adding/removing declarations also requires an
appropriate manifest entry. A payload tree inconsistent with its manifest fails
validation instead of guessing metadata or ignoring files.

For example, edit a real deployed file and capture it:

```sh
lyfy save --host laptop --data_dir /absolute/path/to/private-state
git -C /absolute/path/to/private-state log --oneline
git -C /absolute/path/to/private-state diff HEAD~1 HEAD -- hosts/laptop/files
```

Or edit a committed desired file in that repository, commit the change, inspect
`lyfy apply`, and execute the reviewed result with `lyfy apply --write`.

## Repository setup and sharing

An absent Git repository is initialized on first save, inside the configured
private data_dir. Git user.name and user.email must already be configured for
the invoking user or locally in the state repository. For an explicit setup:

```sh
install -d -m 700 /absolute/path/to/private-state
git -C /absolute/path/to/private-state init --initial-branch=main
git -C /absolute/path/to/private-state config user.name 'Your Name'
git -C /absolute/path/to/private-state config user.email 'you@example.org'
lyfy save --host common --data_dir /absolute/path/to/private-state
```

The path must be the actual worktree root. Linked worktrees and gitdir pointer
files are not currently supported. After cloning, make the repository directory
private before using mutating lyfy operations. The application does not chmod
existing XDG parent directories or assume Git preserves Unix target metadata.

Configure remotes, fetch, merge/rebase and push using Git explicitly. This lets
the state repository follow its own branch/PR policy and avoids hidden network
activity in save/apply. Keep system state and its history private: ordinary
configuration files may contain embedded secrets. Encryption is not implemented.

Git operations disable hooks, fsmonitor and signing prompts in this adapter.
Repository-local info/attributes forces raw bytes for hosts/**, preventing line
ending conversion, ident expansion and clean filters from modifying snapshots.
Snapshot creation never executes deployment scripts from the repository.

## Directory format

The directory backend retains immutable snapshots for local workflows:

```text
<data_dir>/objects/<sha256>
<data_dir>/hosts/<host>/HEAD
<data_dir>/hosts/<host>/snapshots/<sha256>.json
```

It uses the same domain model and host overlays as Git, with SHA-256 content
objects and snapshot manifests. It is not a recommended layout to wrap in Git
for readable configuration diffs. Choose the Git backend for that use case.
Automatic conversion between existing backend layouts is not implemented;
choose an empty/new data_dir when changing storage_format.

Both backends share the same application operations and services. The persistence adapter
owns encoding and history mechanics. Multi-file target backups and receipts
remain in state_dir and are not committed as desired system state.


## Planned storage evolution

storage_format currently chooses Git or directory layout/backend. The CLI
format setting changes the operation report, not the saved state representation.
The [collection/backend design](storage-design.md) proposes independent path
routing, history and backend policies for a later iteration, including object
storage. It does not introduce implemented S3 support or new runtime settings.
