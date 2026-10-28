# lyfy

Licensed under [MIT](LICENSE).

Declarative system maintenance for Arch Linux: inspect the system, capture its
concrete configuration, reconcile host state and apply supported policy fixes.

The project is an initial working implementation. It does not yet replace a
complete AUR helper or provide a universal automatic system repair engine.
The architecture and current boundaries are explicit below.

## Architecture

One Cargo package provides a library named `lyfy` and an executable named
`lyfy`. CLI arguments, ENV, XDG discovery and TOML belong to the binary input.
The library accepts resolved settings and exactly four operation requests.

The four independent managers share named services for inspection, capture,
snapshots, reconciliation and transactions. Managers never call each other.
There is no output layer. See the mandatory [architecture contract](ARCHITECTURE.md)
and [contribution rules](CONTRIBUTING.md).

## Build

```sh
cargo build --locked --release
target/release/lyfy --help
```

Installed packages include `man 1 lyfy` for commands and `man 5 lyfy.conf` for
configuration, environment variables and XDG paths. Static manual sources live
in [docs/man](docs/man). The dotted manual name follows the configuration-page
convention; the actual configuration filename is `config.toml`.

Library without CLI parsing dependencies:

```sh
cargo build --lib --no-default-features --locked
cargo test --lib --test api --no-default-features --locked
```

The supported target system is Arch Linux. Tests run against temporary Linux
directory trees and do not change the developer's real system. No daemon or
Docker service is needed.

Current releases support x86_64 (amd64) only and are distributed as the AUR
source package `lyfy`. See [RELEASING.md](RELEASING.md) for release automation,
package checks and credential handling.
Versions use calendar numbering like synchrogit: `YY.M.PATCH`, for example
`26.10.0`. Month and patch have no leading zeros; PATCH starts at 0 each month
and increments for every release. Compatibility changes appear in release notes.

## Four commands

```sh
lyfy lint
lyfy save --host laptop
lyfy apply --host laptop
lyfy fmt
```

- `lint` runs the [Arch rule catalog](docs/rules.md). It reports incomplete and
  skipped checks explicitly. Text findings appear as checks run; check progress
  and durations go to stderr. Terminals also show the active check every two
  seconds. `--format json` keeps a single final JSON report on stdout.
- `save` captures concrete state and commits the host's files and metadata in
  the configured Git repository. An unchanged state creates no empty commit.
- `apply` returns a file plan and package comparison. `--write` executes the file
  plan, with conflict checks, preimage backups and a transaction journal.
- `fmt` returns supported policy fixes. `--write` currently removes only pacnew
  files identical to their active configuration; conflicting files need review.

`apply` and `fmt` are previews unless `--write` is present. Execution permission
is not a configuration value and cannot be supplied by ENV. There are no
hidden config, rules, init or package-management commands.

Interactive `lyfy lint` requests sudo automatically when scanning the running
system (`root = /`) as a non-root user, with both stdin and stderr connected
to terminals. This is the default `--elevate=auto` launch policy.
Use `--elevate=never` to retain current privileges. Bare `--elevate` (equivalent
to `--elevate=required`) requests sudo even for another root or a noninteractive
launch. Auto mode does not prompt in scripts or for alternative roots.

The input invokes `/usr/bin/sudo`, which owns authentication, then runs only
read-only lint with the original resolved user settings, including HOME/XDG
paths and flag overrides. The child explicitly disables further elevation.
The policy has no ENV/config equivalent. Sudo must be installed and authorize
the request; refusal or failure stops that launch. When scanning without sudo,
inaccessible paths become one `scan.access_denied` warning and accessible
checks continue; partial coverage returns exit code 1. Running `sudo lyfy lint`
directly is possible but may use root's HOME/XDG defaults instead of yours.

Exit codes: 0 means the requested operation succeeded, 1 means findings or
conflicts remain, 2 means an operational/configuration error. An apply preview
with planned changes but no conflicts succeeds. A fmt preview with pending
fixes returns 1.

## XDG configuration

Configuration lookup takes the first readable file:

1. `$XDG_CONFIG_HOME/lyfy/config.toml`, default `~/.config/lyfy/config.toml`.
2. Each `$XDG_CONFIG_DIRS/lyfy/config.toml`, default `/etc/xdg/lyfy/config.toml`.

An explicit `--config_file` or `LYFY_CONFIG_FILE` selects one required file.
Unknown fields and malformed configuration are errors. Relative XDG base
variables are ignored according to the [XDG specification](https://specifications.freedesktop.org/basedir/latest/).

Every setting has a snake_case flag and a `LYFY_UPPER_CASE` environment variable.
Precedence is flag > ENV > TOML > default. Lists replace the lower-priority
list. Boolean overrides take an explicit true/false value.

```sh
LYFY_HOST=desktop lyfy save --host laptop
lyfy lint --require_uki false
lyfy save --include '/etc,~/.config,~/.bashrc'
lyfy apply --format json
```

See [examples/config.toml](examples/config.toml) and the [complete settings reference](docs/configuration.md).
No shell expansion occurs; `~` is supported only in managed scope lists.

Persistent content lives in `$XDG_DATA_HOME/lyfy` (default `~/.local/share/lyfy`).
By default this is a Git worktree; `storage_format = "directory"` selects the
alternative immutable JSON/content-object store. Both path and format have flag
and ENV overrides. See the [storage contract](docs/storage.md).
Receipts, locks and recovery journals live in `$XDG_STATE_HOME/lyfy` (default
`~/.local/state/lyfy`). Newly created application directories are private.
No cache or runtime directory is created when it is not needed.

## Hosts without templates

```sh
lyfy save --host common
# Edit real files on the laptop.
lyfy save --host laptop --base_host common
# Inspect or deploy that concrete laptop state.
lyfy apply --host laptop
```

A host snapshot stores only differences from its optional base. It pins the
base's immutable snapshot ID; saving common later does not rewrite existing
host snapshots. Saving edited files updates the selected host, not the base.
Home paths map to the destination `home_dir`, including a different username.
Home ownership maps to the destination home directory's owner and group.

Share the state repository using normal Git clone/fetch/push. Use a full clone
so pinned base revisions remain available. lyfy commits locally and never
implicitly pushes, pulls, merges or rewrites history. State synchronization
conflicts are resolved using normal Git workflows. Real system snapshots belong
in a separate private repository, not this public source repository.

## Apply and packages

An apply compares desired content, live content and the last applied receipt
for the same host/root/home. A first deployment conflicts with different existing
files. Review the preview before intentionally choosing `--overwrite true`.
Known deletions additionally require `--prune true`; directory deletion is
nonrecursive and fails if unknown contents remain. Untracked files are retained.

Snapshots record all installed package names, versions, explicit/dependency
reasons and native/foreign classification. Offline inventories have unknown
classification. Foreign does not imply AUR.

Native restoration is a separate opt-in:

```sh
lyfy apply --install_packages true
lyfy apply --install_packages true --write
```

Package execution requires existing root privileges; apply does not invoke sudo.
It installs missing native packages using `pacman -Syu --needed`, preserves
pacman's confirmation, then restores affected installation reasons. It refuses
missing foreign packages. It does not remove extra packages or downgrade to
recorded versions. The plan reports version/reason drift and extra explicit
packages. After a package transaction the file plan is rebuilt and checked again.

## Scope and current limits

Default capture includes XDG user configuration, common shell/Git dotfiles,
`/etc` and `/usr/local/etc`. `~/Projects`, caches, runtime trees, pacman database
files and common identity/secret locations are excluded. Additional paths can
be managed through include/exclude lists. This is a configuration/state tool,
not a disk image or a package-cache backup.

Regular files, directories, symlinks, normal Unix modes and ownership are
supported. Hardlinks, extended attributes/ACLs on regular files/directories,
special permission bits, special files and non-UTF-8 names fail capture instead
of being silently flattened. Unsupported symlink attributes are not preserved.
Read errors and oversized files abort save; there is no partial-success snapshot.

Target access uses descriptor-relative operations and refuses symlink parents.
Individual replacements are atomic. A multi-file operation is not one atomic
filesystem transaction: failures leave a private recovery journal and original
file objects in `state_dir/transactions/<id>`. Automatic rollback, crash resume,
filesystem snapshots and restoration of package transactions are not implemented.
Pause concurrent editors/package transactions while capturing or applying state;
lyfy cannot freeze the whole system.

UKI is a strict lyfy policy, not an Arch Linux requirement. Boot migration,
reboots, divergent pacnew merges, removal of unowned files/orphans and AUR builds
require deliberate handling and are not automatic fixes in this version.
