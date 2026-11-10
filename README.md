# lyfy

lyfy saves and restores configuration and selected data on Arch Linux. Use it
to keep several machines in sync, recover a project or restore part of a system.
It records the files you actually use, together with the metadata and recovery
sources needed to explain how they can be restored.

A collection can be any filesystem path. For example, configuration can live in
Git, pictures in S3 and a project inside another collection can retain its own
Git history and local changes. Verified unchanged package files are represented
by package references; modified files remain concrete overrides.

## Four commands

| Command | What it does |
| --- | --- |
| `lyfy lint` | Inspect the system and report configuration, package and boot-policy findings |
| `lyfy save` | Capture selected files, metadata and supported resources |
| `lyfy apply` | Preview restoration from saved state |
| `lyfy fmt` | Preview supported fixes for inspection findings |

`apply` and `fmt` change the target only with `--write`. Save can be previewed
with `--dry_run`. Conflicting files, tracked deletions and package/account actions
have additional explicit permissions.

## Start with a small configuration

Create `~/.config/lyfy/config.toml` and select the paths you want to manage. This
example uses fictional application names; replace them with your own paths:

```toml
host = "laptop"
include = ["~/.config/editor", "~/.config/terminal"]
check_packages = false
```

The default backend is a local Git repository in `~/.local/share/lyfy`. Missing
optional paths are skipped. Review what will be saved, then capture it:

```sh
lyfy save --dry_run
lyfy save
```

Preview restoration before applying it:

```sh
lyfy apply
lyfy apply --write
```

Restore just one selected path when needed:

```sh
lyfy apply --selected_paths '~/.config/editor'
lyfy apply --selected_paths '~/.config/editor' --write
```

Keep saved machine state and real deployment profiles in private storage.
Application configuration can contain credentials even when its filename looks
ordinary. The [migration guide](docs/migration.md) explains how to inventory an
existing setup, capture each host and verify restoration before replacing it.

## Choose how state is stored and shared

Named collections route arbitrary paths to Git, directory, S3 or Yandex Disk
backends. Nested collections own their paths independently. History retention,
replicas, age encryption and portable archives are configurable per destination
and collection. See the [save and restore guide](docs/save.md) for TOML examples.

Hosts can share a pinned common base while retaining their own concrete
configuration. Updating the common state is reconciled with each host's observed
files and exceptions. Git synchronization uses ordinary clone/fetch/push;
lyfy never pushes or merges a repository implicitly.

Supported resources include files with Linux metadata, package sources and
exceptions, independent Git projects, external mount identity, public account
mappings, persistent systemd links, SQLite exports and verified ldconfig output.
The [system recovery guide](docs/system-recovery.md) describes each boundary.

## Installation and current scope

lyfy targets Arch Linux x86_64 and is available as the AUR package
[lyfy](https://aur.archlinux.org/packages/lyfy). To build from source, use the
pacman-managed Rust toolchain, pkgconf, pacman/libalpm and libarchive:

```sh
cargo build -p lyfy-cli --locked --release
target/release/lyfy --help
```

Capture is strict by default: unsupported objects and incomplete reads block
publication. User files can be restored without root when their metadata allows
it; system ownership, accounts and package changes require root. Lyfy preserves
preimages and journals, but does not provide automatic rollback, a filesystem
freeze or disk/bootloader preparation. Read the recovery guide before relying
on a snapshot for complete system recovery.

The project is still evolving. Only current snapshot/archive formats are
supported; previous versions have no compatibility guarantee.

## Documentation

- [Product roadmap: CLI visual polish, browser system overview and recovery](docs/roadmap.md)
- [Settings reference](docs/configuration.md) and `man 5 lyfy.conf`
- [Save, selective apply, backends and encryption](docs/save.md)
- [Migration with synthetic configuration examples](docs/migration.md)
- [Inspection findings and reports](docs/lint.md)
- [Recovery boundaries](docs/system-recovery.md) and [storage guarantees](docs/storage.md)
- [Rust library API](docs/library.md), [architecture](ARCHITECTURE.md) and [contributing](CONTRIBUTING.md)

Own source is licensed under [MIT](LICENSE). Linked builds follow GPL-3.0-only;
see [distribution terms](DISTRIBUTION.md) for native dependency licensing.
