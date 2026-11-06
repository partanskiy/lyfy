# Settings reference

All names below are TOML keys and `--snake_case` flags. The corresponding
environment variable is LYFY_ followed by the uppercase name. CLI flags override
ENV, then TOML, then defaults. Launch input resolution is independent of the
library's business logic. `lyfy-cli` loads this application configuration;
`lyfy::Config` contains resolved typed parameters and validates their meaning.
See [the library boundary](library.md).

| Setting | Default | Meaning |
| --- | --- | --- |
| system | arch | Only the Arch Linux profile is implemented |
| root | / | Physical root of the managed filesystem tree |
| home_dir | HOME | Absolute logical home path inside the target root |
| data_dir | XDG_DATA_HOME/lyfy | Physical private content/snapshot repository |
| state_dir | XDG_STATE_HOME/lyfy | Physical private receipts, locks and journals |
| storage_format | git | Git worktree with plain files/JSON, or directory JSON/object store |
| host | /etc/hostname | Validated host identifier |
| base_host | unset | Reconcile with this host's latest base; omission retains an existing accepted pin |
| snapshot | latest | Input snapshot ID for apply; save always creates a new ID |
| include | See below | Managed file/directory scopes |
| exclude | See below | Prefix exclusions from capture and apply |
| ownership_roots | /usr/bin, /usr/lib, /usr/share | Trees checked for files outside pacman ownership |
| require_uki | true | Require Type #2 entries without Type #1 entries |
| check_packages | true | Capture, compare and inspect package state |
| check_system | true | Capture required public identity hints and the owning collection's system catalog |
| system_resources | [] | Typed user/group/service/generated/application/excluded declarations |
| install_identities | false | Permit explicit account creation and additive group memberships |
| restore_services | false | Restore declared persistent systemd links without starting services |
| regenerate | false | Permit verified execution of a declared generated-state producer |
| package_file_policy | capture | Store changed package files; reject_executables instead blocks modified executables |
| install_packages | false | Permit planned package transactions with apply --write |
| package_sources | [] | Explicit archive/local_recipe/aur_recipe inputs; JSON array or TOML tables |
| package_version_policy | locked | locked verifies saved versions; current permits a full repository upgrade |
| build_packages | false | Permit unprivileged makepkg execution of retained recipes |
| remove_packages | [] | Explicit comma-separated package names to remove; extras are retained by default |
| overwrite | false | Permit replacing live content that conflicts with the receipt |
| prune | false | Permit explicit saved deletions; omitted archive entries never imply deletion |
| max_file_bytes | 0 | Maximum regular-file bytes during capture/planning; 0 is unlimited |
| capture_errors | strict | strict blocks incomplete saves; explicit skip records diagnostics and returns 2 |
| mounts | [] | Named VFS paths, backend, history, restore, git_project and replica policies |
| backends | [] | Named Git/directory/S3/Yandex Disk destinations and public age recipients |
| selected_mounts | [] | Comma-separated mount names; empty selects normal operation scopes |
| selected_paths | [] | Apply-only prefixes within their owning collections; preserve nested scopes |
| selected_resources | [] | Apply-only JSON list of collection/host/subject resource identities |
| identity_mappings | [] | Explicit user/group source IDs mapped to existing target names |
| export_file | unset | Complete archive destination for save; select one scope |
| import_file | unset | Verified archive source for save/apply; select one scope |
| identity_files | [] | Absolute age identity paths; input-owned connection material |
| protected_paths | [] | Extra physical connection paths always excluded from capture; input-owned |
| disabled_rules | [] | Validated rule IDs excluded from inspection |
| fail_on | warning | Fail a complete scan at info, warning or error |
| format | text | Lint: text, json, toml, yaml, markdown or sarif; CLI only |
| output_file | unset | New private lint report file; - selects stdout; CLI only |
| progress | auto | auto, plain or never; CLI only |

`config_file` is a bootstrap selector, available only as `--config_file` and
LYFY_CONFIG_FILE. A configuration file cannot redirect its own loader.
`--write` is execution permission on an apply/fmt request, not a setting; it has
no TOML key or environment equivalent.
See [lint reports](lint.md) for progress, formats, private files and exit codes.
Markdown/SARIF and output_file are lint-only. Other commands support text, JSON,
TOML and YAML. Interactive package execution requires text display; JSON previews
remain supported. This launch constraint prevents pacman prompts from corrupting
the executable's JSON result stream.

Paths in include/exclude/ownership_roots must be absolute or start with `~/`.
They are prefixes, not globs. `~` maps to home_dir. Absolute system paths are
resolved inside root. data_dir/state_dir refer to the actual local filesystem
and are not prefixed with root. There is no shell or general ENV interpolation.

Defaults include the current XDG_CONFIG_HOME, ~/.bashrc, ~/.bash_profile,
~/.zshrc, ~/.profile, ~/.gitconfig, /etc and /usr/local/etc. Missing optional
paths are skipped. Explicit `/` or `~` scopes also capture their directory
metadata, represented as `system/` and `home/`. Apply can restore those properties
but never deletes or replaces a namespace root with another node type.

Defaults exclude ~/Projects, ~/.cache, ~/.local/state, ~/.ssh, ~/.gnupg, ~/.aws,
~/.config/lyfy/keys and the corresponding current XDG_CONFIG_HOME/lyfy/keys,
/etc/shadow and shadow-, /etc/gshadow and gshadow-, /etc/ssh, /etc/ssl/private,
/etc/credstore, /etc/credstore.encrypted, /etc/pacman.d/gnupg and /etc/machine-id.
Associated pacnew/pacsave files inherit exclusion of their active path. This
list does not identify every possible secret embedded in application configs.
Active identity/credential files and configured protected_paths are always
excluded, including when an explicit mount overrides a default exclusion.
See [the save guide](save.md) for nested mount routing, complete JSON list
overrides, encryption, S3 authentication and history/replica policies.

Virtual/runtime/cache trees /proc, /sys, /dev, /run, /tmp, /var/tmp, /var/cache
and the live /var/lib/pacman database are always excluded from file state.
The data and state repositories themselves are also always excluded to prevent
recursive capture. Pacman state is represented through package inventory.

XDG_CONFIG_DIRS is searched in order, after the user's configuration file.
The first readable file wins as a whole; files are not merged across directories.
An explicitly selected missing or unreadable file fails. Relative XDG paths
are ignored, and empty/unset base variables use their standard HOME defaults.

App directories are created with mode 0700 and files with mode 0600. Existing
XDG parent directories are not chmodded. An existing app repository/state
directory must already be private and owned by the invoking user. A root
deployment needs explicitly chosen trusted data/state/home paths; changing the
invoking user must not accidentally select another user's defaults.

`apply --elevate=auto|never|required` controls the input-owned one-shot root
executor. It has no ENV/TOML equivalent. The caller resolves storage and exports
selected verified archives; root keeps system receipts/journals below
`/var/lib/lyfy/apply/<state-profile-hash>`. User-only apply runs directly when its
ownership and metadata permit. See [apply privileges](save.md#apply-privileges).

`identity_mappings` accepts TOML tables or a complete JSON array via flag/ENV:
`[{"kind":"user","source":1000,"target":"workuser"}]`. Kinds are user/group.
Mappings apply to numeric system ownership and named POSIX ACL entries. Home
ownership stays symbolic. Only needed names are looked up in the target
`/etc/passwd` or `/etc/group`. Saved public names supply automatic mappings;
explicit identity_mappings override them. A missing account blocks dependent
writes until an explicitly selected identity plan can create it. Raw passwd,
group, shadow, gshadow, subuid/subgid files and their standard backups are always
outside ordinary file restoration.

## Collection capture and system policy

Mount.include is an optional list of absolute or ~/ paths below its VFS point.
An empty list includes the entire subtree. Ancestor directories are retained,
while unlisted siblings stay outside capture and apply. Mount.exclude adds
prefix exclusions. Neither list is a glob. A properties_only mount retains one
existing directory's metadata and excludes descendants; it is suitable for /.
It cannot be a Git project or declare descendant includes.

Mount.system selects the single explicit owner of the system inventory; otherwise
the default collection owns it. Needed numeric owner/ACL hints can accompany
file-only collections. Mount.packages independently selects the package owner.
Mount.base_host overrides the global base host; an empty string explicitly makes
the collection independent. Removing an existing accepted base requires a new
collection rather than silently changing its history.

system_resources accepts a complete JSON array or equivalent TOML tables:

```toml
[[system_resources]]
kind = "user"
name = "backup"

[[system_resources]]
kind = "service"
name = "backup@daily.service"

[[system_resources]]
kind = "generated"
path = "/etc/ld.so.cache"
producer = "ldconfig"

[[system_resources]]
kind = "application"
path = "~/.local/share/example/state.sqlite"
format = "sqlite"
sensitive = true

[[system_resources]]
kind = "excluded"
path = "~/.local/share/example/cache"
reason = "rebuildable application cache"
```

Sensitive SQLite exports require an age-encrypted named backend. Explicit
sensitive=false is for reviewed nonsecret databases. ldconfig is the supported
closed generator and requires Landlock ABI >= 3 plus a writable target /tmp.
No arbitrary shell hooks or SQL scripts are accepted. See
[system recovery](system-recovery.md) for execution and coverage boundaries.
