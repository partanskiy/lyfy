# System recovery boundaries

The resource implementation has nine completed software stages. A successful
save confirms the selected capture and publications. It does not certify a
bootable replacement machine. `coverage` distinguishes accounted state, explicit
exclusions and unresolved recovery requirements; resource guarantees remain
`recorded` until the declared recovery closure has been proven separately.

## Public identities and privileges

File collections retain the public account names needed for numeric ownership
and named ACL entries. The system inventory owner can additionally request user
and group resources. Existing target names map source IDs to local IDs; explicit
`identity_mappings` take precedence. Home ownership maps to the destination home.

Missing accounts block dependent writes. Explicit identity selection plus
`install_identities=true` permits creation at free IDs and additive supplementary
memberships. Groups precede users, and users precede dependent file/package work.
Creation is supported only by the root executor on the running root. Alternate
roots support account previews. Numeric collisions and contradictory selected
account plans block execution. Existing IDs, home directories and shells are
preserved; memberships are never removed automatically.

New users have locked passwords and no generated home. Raw passwd, group,
shadow, gshadow and subordinate-ID databases, including standard backups, are
outside ordinary file capture/restoration. Passwords, authentication enrollment,
account aging, GECOS and subordinate-ID allocations need a separate bootstrap.
Public account actions have private journals without raw authentication data.

User-only apply can run without root when the target ownership, groups and
metadata allow it. System ownership, special permissions, package transactions
and account creation require root. The caller exports verified sources with its
own credentials; a one-shot root process replans and keeps trusted system
receipts. All selected plans and preimages are prepared before mutation.
Failures leave journals and preimages, not an automatic global rollback.

## Persistent service state

Declare canonical system unit names as `kind="service"`. The provider records
enabled, disabled or masked state, persistent links and the pinned definition.
Instance names can use a template definition. Aliased/linked definitions require
review and a canonical declaration. A definition must already match, belong to
a selected package acquisition plan, or be supplied by the selected file plan.

Changes require `restore_services=true`; removals additionally require `prune`.
Path-only selection of managed unit links cannot bypass service permissions,
including through the privileged execution archive. Select the corresponding
service resource as well. Partial service restoration preserves other units.
No service is started or stopped, and daemon-reload is a separate activation
step. User units remain ordinary concrete files/links with manual activation.

## Application and generated state

Declare SQLite databases as `kind="application", format="sqlite"`. A native
sqlite3 backup creates a consistent database image, including committed WAL
state. Existing targets are restored through SQLite so open WAL connections
remain coherent. Declared WAL/SHM/journal companions are outside ordinary file
capture. Unclassified database sidecars are diagnosed in strict mode.

Exports default to sensitive and require an age-encrypted named backend.
`sensitive=false` is an explicit decision for reviewed nonsecret data. Key files
stay outside snapshots. The fixed adapter accepts no SQL scripts or hooks.
SQLite exports require sqlite3, a regular database with one hardlink and a safe
parent. The adapter runs as its owner. Root-owned databases additionally need
protected parents. PostgreSQL, LevelDB and other live stores need a supported
export strategy or an explicit exclusion; copying their live files is not a
consistent application backup.

The closed generated-state provider currently supports `/etc/ld.so.cache` with
`producer="ldconfig"`. Save verifies that regeneration matches the observed
bytes. Apply requires `regenerate=true`, runs after ordinary files and requires
the exact recorded output. Landlock ABI >= 3 confines generator writes to private
output storage, including ldconfig's auxiliary-cache attempts. The target needs
a writable `/tmp`. There are no arbitrary generator commands.

## Files, packages and external storage

Regular files and symlinks preserve nanosecond mtime, modes, ownership and
supported ACL/xattr; hardlinks retain topology within one collection. Directory
mtime, access/change/birth times and inode numbers are not recovery promises.
Socket/device and unsupported metadata diagnostics block strict publication.
Explicit `capture_errors="skip"` returns 2 and yields unappliable incomplete
state. Include/exclude policies and nested collection boundaries are retained in
portable archives. Only explicit tombstones authorize tracked deletion.

Unchanged package payloads use verified package baselines rather than copied
binaries; modified files remain concrete exceptions. Repository candidates do
not imply future availability. Archive/recipe inputs and their hashes are
retained; opt-in unprivileged builds need pacman-managed build requirements.
Foreign packages require explicit provenance. Automatic AUR discovery and a
complete independently recoverable build/dependency closure are not claimed.

External mounts carry identity and topology requirements. Connection-only,
independent-copy and local-materialization policies differ explicitly. A
disconnected or replaced source cannot become a deletion. Storage/firmware/boot
requirements describe prerequisites; lyfy does not partition, unlock disks,
install a bootloader, rebuild every boot artifact or reboot the target.

See [settings](configuration.md), [save/apply](save.md),
[storage guarantees](storage.md) and the [dotfiles migration](dotfiles-migration.md).
