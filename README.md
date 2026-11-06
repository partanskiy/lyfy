# lyfy

Own source is licensed under [MIT](LICENSE). Linked builds follow GPL-3.0-only;
see [distribution terms](DISTRIBUTION.md) for the approved native dependencies.

Declarative system maintenance for Arch Linux: inspect the system, capture its
concrete configuration, reconcile host state and apply supported policy fixes.

The resource recovery implementation includes selective restoration, package
sources, Git projects, external mounts and system/application state. Recovery
boundaries and deployment gates are explicit in [system recovery](docs/system-recovery.md)
and the [migration guide](docs/migration.md).

## Four commands

```sh
lyfy lint
lyfy save --host laptop
lyfy apply --host laptop
lyfy fmt
```

- `lint` runs the [Arch rule catalog](docs/rules.md). It reports incomplete and
  skipped checks explicitly. Terminals show updating stage progress and package
  counts on stderr. The final report explains findings and remedies. Export
  text, JSON, TOML, YAML, Markdown or SARIF with `--format` and `--output_file`;
  see [lint reports](docs/lint.md).
- `save` previews or captures concrete state, reports file deltas and publishes
  host metadata/content. Git and directory reuse unchanged snapshots. Nested VFS
  mounts can route to independent Git/directory/S3/Yandex Disk backends, with retention,
  replicas, age encryption and portable exports. See the [save guide](docs/save.md).
- `apply` plans selected paths and resources with explicit dependencies. `--write`
  executes authorized file, package, identity, service and generator actions,
  with conflict checks, preimages, journals and partial receipts.
- `fmt` returns supported policy fixes. `--write` currently removes only pacnew
  files identical to their active configuration; conflicting files need review.

`apply` and `fmt` are previews unless `--write` is present. Execution permission
is not a configuration value and cannot be supplied by ENV. There are no
hidden config, rules, init or package-management commands.

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
and ENV overrides. Named VFS mounts can select other backends independently.
See the [storage contract](docs/storage.md) and [save guide](docs/save.md).
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

