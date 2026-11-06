# Moving acchan from aconfmgr and chezmoi

The concrete [acchan candidate](../examples/migration/acchan.toml) and
[declaration audit](../examples/migration/acchan-coverage.tsv) were prepared from
[partanskiy/dotfiles at a24c97b](https://github.com/partanskiy/dotfiles/tree/a24c97b5302235710c2ad82c64783d7103545296).
The local chezmoi source and public main matched that revision on 2026-11-06.
The audit reads declarations without executing shell, hooks or templates and
does not decrypt or copy credential payloads. No live lyfy profile was installed
and no managed host files or packages were changed.

The source has 209 ordinary and 24 foreign AddPackage declarations. These are
declarations, not a verified current installed inventory. There are 75 CopyFile,
50 CreateLink and 3 SetFileProperty declarations. The audit also retains old
IgnorePath expressions, including dynamic helper arguments, for review. Those
expressions are not silently converted: lyfy paths are prefixes, not globs.

## Concrete coverage

| Existing responsibility | Candidate handling | Remaining recovery boundary |
| --- | --- | --- |
| Augeas-edited system configuration | Capture deployed concrete paths in system | No Augeas or old shell execution |
| System, hardware, CUPS, sudo, pacman and firewall files | Curated system includes | Review current contents and hardware applicability |
| Public accounts and group ownership | Required name hints and explicit geles resource | Passwords, login enrollment and subordinate IDs are separate bootstrap |
| Persistent system unit links | Canonical service resources and concrete drop-ins | restore_services permission; manual runtime activation |
| User unit files and links | Concrete system/config collections | Manual user-manager activation |
| Local helper and JVM/library/keyring exceptions | local and platform collections | Package acquisition and signature trust remain prerequisites |
| Root mode 0555 and boot directory properties | Metadata-only root/boot collections plus loader config | Verify boot volume identity; no disk/bootloader preparation |
| Chezmo-managed public application settings | Curated config, assistant, home-tools and desktop | Capture each host's own concrete result first |
| Pictures and Projects | Separate retained directory collections, excluded from initial selection | Bind a verified remote backend and classify every nested Git/external boundary |
| lyfy project | Nested git_project collection with target excluded | Other projects need their own explicit boundaries |
| Generated caches, runtime data and old ignored backups | Outside the curated selection, listed in audit | Rebuild through verified package/application procedures |
| SSH/GPG, rclone, cloud and application keys | Outside initial selection | Encrypted credential bootstrap and recovery drill |
| Exchange and VPN configuration/activation | Explicit system exclusions | Separate protected bootstrap; no infrastructure change |

The profile is a local migration candidate, not an independent off-device backup.
Git configuration storage is readable. Review every selected application's
embedded secrets before capture. `~/.local/state/noctalia/storage.key`, rclone
credentials, SSH/GPG material and encrypted chezmoi sources are outside it.
The .claude collection selects settings.json only; review that file for tokens.
The system collection excludes Exchange, Xray/sing-box configuration and their
activation helpers. Nftables configuration alone does not recover a working VPN.

## First host and shared intent

Use the executable built from this implementation. An older installed lyfy
binary cannot read the current schemas or profile fields. The candidate uses
separate migration data/state paths and independent collection bases, so an
initial capture does not invent a common host. Start with a reviewed subset:

```sh
target/release/lyfy --config_file examples/migration/acchan.toml save --selected_mounts config,assistant,home-tools,desktop --dry_run
target/release/lyfy --config_file examples/migration/acchan.toml save --selected_mounts config,assistant,home-tools,desktop
target/release/lyfy --config_file examples/migration/acchan.toml apply --selected_mounts config,assistant,home-tools,desktop
```

These are deployment instructions, not actions already performed. Inspect the
delta and coverage first. Save again without edits and verify reused snapshot
IDs and no file delta. Then export one selected collection, restore a selected
path into a disposable root and check metadata and unrelated siblings.
Do not enable overwrite, prune or system permissions just to clear a conflict.

The old hosts.toml registers only acchan. Import the work laptop's own observed
files, installed package state, accounts and concrete paths under its own host
ID; do not apply acchan's system profile to it. Review a shared subset, publish
it under common and use that base only for collections intended to share it.
Existing independent histories need an explicit reviewed transition; a fresh
collection can establish the desired common base. Keep machine-specific system,
hardware and boot collections independent. File-only sync can start separately
from root recovery. A common change must preserve each host's local exceptions.

## Gates before retiring the old setup

- Reconcile the audit with current deployed paths and dynamic declarations.
  Review missing files and ignored glob matches individually. The profile does
  not claim to discover every unmanaged path across the whole machine.
- Retain exact archives or pinned recipes/source inputs for foreign/custom
  packages and repository packages that must survive upstream removal. Review
  PKGBUILD and pacman-managed build dependencies; no pip-based installation.
  Test acquisition and signatures from a disposable target before root recovery.
- Configure actual S3/Yandex credentials and age recipients outside snapshots.
  Confirm required replicas, key availability and an offline encrypted restore.
  Loopback provider tests passed; no live cloud account was verified here.
- Capture both laptops independently and run a reviewed common update in both
  directions. The work laptop was not accessed during implementation.
- Verify UID/GID and ACL mapping, service activation, external volume identity,
  authentication, disk unlock, firmware, boot artifacts and rescue procedures
  separately. Static requirements and file copies do not prove a bootable host.

Temporary-root tests cover curated selection, no-op recapture, two-host common
updates, selected restore, preserved exceptions, privilege handoff, encrypted
archives, SQLite WAL and generator confinement. These software checks passed;
the deployment gates above remain unverified. See [system recovery](system-recovery.md).
