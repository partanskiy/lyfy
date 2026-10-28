# Settings reference

All names below are TOML keys and `--snake_case` flags. The corresponding
environment variable is LYFY_ followed by the uppercase name. CLI flags override
ENV, then TOML, then defaults. Launch input resolution is independent of the
library's business logic.

| Setting | Default | Meaning |
| --- | --- | --- |
| system | arch | Only the Arch Linux profile is implemented |
| root | / | Physical root of the managed filesystem tree |
| home_dir | HOME | Absolute logical home path inside the target root |
| data_dir | XDG_DATA_HOME/lyfy | Physical private content/snapshot repository |
| state_dir | XDG_STATE_HOME/lyfy | Physical private receipts, locks and journals |
| storage_format | git | Git worktree with plain files/JSON, or directory JSON/object store |
| host | /etc/hostname | Validated host identifier |
| base_host | unset | Host whose latest snapshot becomes the pinned save base |
| snapshot | latest | Input snapshot ID for apply; save always creates a new ID |
| include | See below | Managed file/directory scopes |
| exclude | See below | Prefix exclusions from capture and apply |
| ownership_roots | /usr/bin, /usr/lib, /usr/share | Trees checked for files outside pacman ownership |
| require_uki | true | Require Type #2 entries without Type #1 entries |
| check_packages | true | Capture, compare and inspect package state |
| install_packages | false | Permit supported native package restoration with apply --write |
| overwrite | false | Permit replacing live content that conflicts with the receipt |
| prune | false | Permit deleting explicitly tracked paths absent from desired state |
| max_file_bytes | 16777216 | Maximum regular-file bytes during capture/planning |
| format | text | CLI result display: text or json; not passed to the library |

`config_file` is a bootstrap selector, available only as `--config_file` and
LYFY_CONFIG_FILE. A configuration file cannot redirect its own loader.
`--write` is execution permission on an apply/fmt request, not a setting; it has
no TOML key or environment equivalent.
Interactive package execution additionally requires text display; JSON previews
remain supported. This launch constraint prevents pacman prompts from corrupting
the executable's JSON result stream.

Paths in include/exclude/ownership_roots must be absolute or start with `~/`.
They are prefixes, not globs. `~` maps to home_dir. Absolute system paths are
resolved inside root. data_dir/state_dir refer to the actual local filesystem
and are not prefixed with root. There is no shell or general ENV interpolation.

Defaults include the current XDG_CONFIG_HOME, ~/.bashrc, ~/.bash_profile,
~/.zshrc, ~/.profile, ~/.gitconfig, /etc and /usr/local/etc. Missing optional
paths are skipped. The root and home directory nodes themselves are never
restored as files, but their children can be managed using `/` or `~` scopes.

Defaults exclude ~/Projects, ~/.cache, ~/.local/state, ~/.ssh, ~/.gnupg,
/etc/shadow and shadow-, /etc/gshadow and gshadow-, /etc/ssh, /etc/ssl/private,
/etc/credstore, /etc/credstore.encrypted, /etc/pacman.d/gnupg and /etc/machine-id.
Associated pacnew/pacsave files inherit exclusion of their active path. This
list does not identify every possible secret embedded in application configs.

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
