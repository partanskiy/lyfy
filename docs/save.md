# Saving and restoring state

`lyfy save` captures selected VFS scopes and publishes concrete host state.
Start with a preview:

```sh
lyfy save --dry_run
lyfy save --progress plain
lyfy save --format json
```

Preview reads and verifies files but creates no repository, state directory,
lock, export or remote object. Temporary private streams may be used. The report
lists additions, modifications and deletions against the previous effective
state. Modified entries name changed fields without printing file contents.
`changes` is this delta, independent of the persisted overlay over base_host.
An unchanged save reuses its snapshot in both Git and directory storage.
`changed` also covers inventory, base and schema changes; `created` reports a
new publication. Multi-scope reports have totals and individual `mounts` results.

## Protected reads

Interactive saves of the live system request sudo automatically when selected
scopes include system paths. Use `--elevate=never` to keep existing privileges,
or `--elevate` to require protected capture explicitly. Scripts and alternative
roots do not prompt automatically. The sudo policy has no ENV/TOML setting.

The elevated process only reads the target and writes a private temporary
capture channel. The original caller verifies that channel and publishes each
scope with their own credentials and repository ownership. It never publishes
through root's Git configuration or cloud credentials. Imported archives do not
need capture elevation. Tests exercise the channel without authenticating sudo.

## Arbitrary nested VFS mounts

The legacy include/exclude/storage_format settings remain the default scope.
Named mounts override that scope and route arbitrary logical paths to named
backends. The longest component-prefix match owns each path. A child is excluded
from its parent even when that child is not selected. `/data-old` is not inside
`/data`. Duplicate roots and mount/backend names are errors.

For example, add these tables to config.toml, replacing physical destinations:

```toml
[[backends]]
name = "config"
kind = "git"
path = "/home/user/.local/share/lyfy-config"

[[backends]]
name = "local"
kind = "directory"
path = "/home/user/Backups/lyfy"

[[backends]]
name = "remote"
kind = "s3"
[backends.s3]
bucket = "example-private-lyfy"
prefix = "personal"
endpoint = "https://s3.eu-central-1.amazonaws.com"
region = "eu-central-1"
profile = "backup"

[[mounts]]
name = "system"
path = "/etc"
backend = "config"
apply = true
packages = true
replicas = ["local"]
replicas_required = true

[[mounts]]
name = "projects"
path = "~/Projects"
backend = "local"
history = "retained"
retain = 10
exclude = ["~/Projects/cache"]

[[mounts]]
name = "pictures"
path = "~/Pictures"
backend = "remote"
history = "current_only"

[[mounts]]
name = "raw"
path = "~/Pictures/Raw"
backend = "local"
history = "current_only"
```

Local backends store each mount under `path/mounts/<name>`; a Git mount has its
own repository. S3 uses `prefix/mounts/<name>/` without an authoritative local
repository. All backend/state roots and active connection files are excluded
before walking. Explicit mounts override ancestor/default exclusions, such as
the default ~/Projects exclusion; mount.exclude adds exclusions for that mount.
Virtual/runtime trees and connection-file protection cannot be overridden.
Mount exclusions are absolute or home-relative prefixes, not globs or paths
relative to the mount. max_file_bytes can be overridden per mount.

```sh
lyfy save --selected_mounts system,pictures --dry_run
lyfy save --selected_mounts projects
lyfy apply --selected_mounts projects
lyfy apply --selected_mounts projects --write
```

Ordinary apply selects mounts with `apply = true` plus the default scope.
Other data restoration requires explicit collection/path/resource selection. Restore previews,
conflict checks, overwrite and prune retain their existing meanings. An absent
backup path never silently deletes unknown local data. Package inventory belongs
to the single mount with packages=true, or to the default scope when none owns
it. check_packages remains the global opt-out. Package file classification follows
the global opt-in across selected file collections, independently of where the
single package inventory is stored.

## Package files

With check_packages enabled, save compares selected regular-file hashes and sizes
with libalpm mtree evidence. Unchanged package payloads are represented by exact
package/version/hash references. Actual metadata is retained, including local
mode, ownership and ACL/xattr changes. Changed contents remain concrete payloads
with their original package baseline; missing package files become explicit
deletions. Missing comparison evidence retains concrete bytes. Hardlink groups
also retain concrete objects to preserve their topology.

`package_file_policy=capture` explicitly permits concrete changed executables.
`reject_executables` blocks changes to non-backup package executables. Capture
does not run package recipes or invoke package transactions.

Apply exposes package dependencies even when a path selects just one provided
file. A baseline must match the installed package version; omitted content must
also be present and match its recorded hash. Missing/different providers block
all selected writes. Metadata overrides can reuse verified target bytes. Concrete
overrides still require a matching package baseline, so a package upgrade cannot
silently change the context of a saved configuration exception. An explicitly
selected package inventory with install_packages=true can repair providers
before file restoration. Its effects remain visible outside the selected path.

### Package acquisition

Save records a unique matching repository candidate with version, architecture,
digest, servers and signature policy. This is evidence of an available source,
not proof of the original installation method. Ambiguous and unavailable sources
stay unresolved. Explicit package_sources override candidates:

```toml
[[package_sources]]
package = "example"
kind = "archive"
path = "/srv/packages/example-1-1-x86_64.pkg.tar.zst"
retain = true
signature = "/srv/packages/example-1-1-x86_64.pkg.tar.zst.sig"
signature_required = true
```

local_recipe uses a concrete recipe directory; aur_recipe uses a local AUR clone
with an explicit immutable revision and matching origin. Recipes, auxiliary files,
declared sources and build requirements are recorded without running PKGBUILD.
Each sources entry supplies name, path and/or credential-free locator, sha256 and
size. Local inputs are retained. .SRCINFO contributes declared build requirements;
it is text evidence, not a proof that the recipe has no other dependencies.
Helper caches are not searched automatically. Set package_sources explicitly.

The default package_version_policy=locked refuses missing exact artifacts rather
than substituting a newer version. current explicitly permits pacman -Syu for
repository packages, but keeps archive/recipe pins exact. Upgraded package file
baselines need separate reconciliation. remove_packages is the explicit removal
list; extras are retained otherwise. Package transactions require the live /
target and root. They are separate from file rollback and journal their actions,
verified artifact digests, completion and resulting inventory.

With apply --write and install_packages=true the CLI first previews through the
root executor. The caller acquires exact archives or, with build_packages=true,
builds frozen recipes unprivileged. makepkg does not install dependencies; missing
requirements must already be satisfied through pacman. Root copies and rechecks
prepared archives and signatures into private staging before pacman sees them.
Signature validation uses a disposable public-key view and never imports keys
into the target keyring. All selected file preimages are backed up before package
actions; file plans are rebuilt afterwards before concrete overrides are written.

Retained recipes/artifacts are included in complete exports and replicas. Package
resources remain Recorded: a recipe alone does not prove dependency closure,
network independence or bit-for-bit reproducibility. This implementation has
synthetic build/signature tests; live host package deployment is a separate gate.

mounts/backends are TOML arrays of tables. Their snake_case flags and ENV
overrides accept a complete JSON array. selected_mounts accepts comma-separated
names, including `default`. Lists replace the lower-precedence list.

## Apply privileges

Apply uses `--elevate=auto|never|required`, like save. Auto requests sudo for
protected actions on the running system when stdin and stderr are terminals.
User-owned file restoration runs directly when its permissions, groups and
attributes permit. Required explicitly requests sudo; never keeps current
privileges. `--write` remains a separate, explicit execution permission.

The caller reads selected sources with its own credentials and prepares private,
verified archives. The one-shot executor imports these into disposable storage
and plans against the target again. It never publishes to caller repositories.
System journals and trusted receipts use `/var/lib/lyfy/apply/<state-profile-hash>`;
the caller updates its receipt only from confirmed execution results, checking
that its history has not changed concurrently. Caller receipts can provide home
history; they cannot authorize replacement of system preimages.

Before target writes, every selected scope must pass planning and destination
metadata checks. A write preflight creates and removes private empty staging
objects on the destination filesystem to test ownership, modes and attributes.
Preview does not perform these staging writes. Immutable/append-only targets,
unsupported metadata and cross-filesystem hardlinks block execution. Runtime
errors stop later scopes; completed actions retain their journals and backups.
There is no automatic rollback or crash resume.

`identity_mappings` explicitly maps source UID/GID values to existing target
names, including named ACL entries. Home ownership maps to the destination home
owner. Ordinary CLI reports show attribute names and byte counts, omitting their
values; the private archive and executor channel preserve those values.

## Partial apply

`selected_paths` selects comma-separated logical prefixes for apply. Each prefix
activates its most specific owning collection, even when apply=false. Nested
collections remain unselected unless named explicitly. Selection intersects any
selected_mounts restriction and never changes capture include/exclude policy.

```sh
lyfy apply --selected_paths '~/Pictures/2025'
lyfy apply --selected_paths '~/Pictures/2025' --write
lyfy apply --selected_resources '[{"collection":"default","host":"laptop","subject":{"kind":"path","key":"home/.config/app"}}]'
```

Resource identities select exact paths or installed package names in the owning
collection and host. Unknown/unmanaged selectors fail. Path-only restoration
does not query or install unrelated packages. Selected package-provided files
explicitly require verification of their provider. Flags, LYFY_SELECTED_PATHS and
LYFY_SELECTED_RESOURCES replace lower-priority lists. An empty path flag clears
the list; resource flags/ENV accept a complete JSON array. Selectors are apply-only.

Plans list required directories and hardlinks outside the selection. Existing
dependencies are guarded against concurrent changes; an unsatisfied dependency
blocks writing until the selection is explicitly expanded. Partial receipts
retain all unaffected entries, including tracked deletions. overwrite and prune
remain explicit. SnapshotView selection does not restrict the underlying snapshot.

## History and replicas

History defaults to retained. retain=0 keeps all history; a positive value keeps
that many recent snapshots per host, every current host state and every pinned
base reachable from them. current_only requires retain=0 and keeps current
states plus pinned dependencies. Git preserves its native history; current_only
and retention pruning are rejected for Git destinations, including replicas.
An existing host retains its accepted base when base_host is omitted. An explicit
base_host selects that host's latest generation for three-way reconciliation:
old common, incoming common and saved host exceptions. Save then merges changed
live observations separately. Unchanged live files cannot cancel an unapplied
common update. Overlapping edits block publication and keep the previous pin;
review and resolve the conflicting desired state before saving again. There is
no automatic text merge. File deletions and package inventory use the same rule.
Git generations are scoped to changes under the particular host, so saving one
host cannot advance another host's base accidentally.

Replicas copy the exact committed source generation and record its identity.
They materialize pinned bases, so restoration does not depend on the source
backend remaining online. Retrying an unchanged state does not create duplicate
replica snapshots. A required replica failure returns 2 while still reporting
the successful primary snapshot. replicas_required=false makes failures visible
without failing an otherwise successful primary save. Each scope and replica
reports its own outcome; there is no transaction across backends.

## Encryption and connection material

Directory and S3 backends support native age recipients. Install age using
pacman, create an identity file and obtain its public recipient with age-keygen.
Set the backend's recipients list to the public recipient, and configure the
absolute identity_files paths for reading existing encrypted state. Git keeps
readable payloads and rejects encryption; use directory/S3 for that policy.

```sh
install -d -m 700 ~/.config/lyfy/keys
age-keygen -o ~/.config/lyfy/keys/backup.agekey
age-keygen -y ~/.config/lyfy/keys/backup.agekey
```

Private keys never enter manifests or ordinary reports. Active identity files,
their canonical targets and loaded AWS credential files are protected from
capture, including sudo capture. Additional physical files can be protected
with protected_paths.
The default lyfy/keys configuration directory is excluded even before an
identity is configured, so generating a key does not add it to the next save.

Encryption covers payloads, manifests and current pointers;
host/object names, hashes and stored sizes remain visible. An encoding marker
prevents silently changing encryption/recipient policy in an existing backend.
Use export/import into a new destination when changing that policy.
Other application configs may contain embedded secrets; review their selection
and choose an encrypted destination when needed.

S3 credentials come from AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY and optional
AWS_SESSION_TOKEN, or the selected profile in AWS_SHARED_CREDENTIALS_FILE
(default ~/.aws/credentials). An explicit backend profile selects the file
profile; otherwise AWS_PROFILE/default is used after ENV credentials. IAM/SSO
credential discovery and credential_process are not implemented. Credentials
never enter manifests, reports or command arguments.

S3 requires HTTPS, ListObjectsV2, ETags, conditional PUT and conditional DELETE.
The collection writer lock probes conditional support before capture. Current
pointers publish with an expected ETag; lost responses are checked before an
outcome is reported. A crashed writer can leave writer.lock; confirm the writer
has stopped and recover that lock explicitly through the S3 provider. Never
delete another live writer's lock. Tests validate signed requests against an
isolated HTTP implementation; deployment still needs provider-specific checks.
Single uploaded payloads are limited to 5 GiB, including encryption overhead;
multipart upload is not implemented. Bucket versioning/lifecycle is independent
of lyfy retention. Failed attempts can leave unreferenced content until retention
cleanup. Local Git never implicitly fetches, pushes or merges.

## Complete exports and imports

Select exactly one scope for export/import. An export carries the resolved tree,
metadata, known deletions, inventory and verified bytes, materializing pinned
bases. It reads committed state and never recaptures live files. Existing export
destinations are preserved; files publish atomically with mode 0600. Encrypted
sources produce an age-encrypted archive. The filename does not select encoding.

```sh
lyfy save --selected_mounts system --export_file /absolute/system.lyfy
lyfy save --selected_mounts system --import_file /absolute/system.lyfy
lyfy apply --selected_mounts system --import_file /absolute/system.lyfy
lyfy apply --selected_mounts system --import_file /absolute/system.lyfy --write
```

Save/import publishes into the selected backend. Apply/import uses a private
temporary source without publishing to that backend. Import validates schema,
scope, paths, hardlinks, metadata and every content hash before target writes.
Truncation, extra content and unsupported declarations fail. Home namespaces
map to the destination home_dir; numeric system IDs and named ACL IDs remain
numeric. Archives provide an explicit migration path between Git/directory/S3.

## Capture completeness and compatibility

Regular files stream with bounded buffers. max_file_bytes=0 is unlimited locally;
set a positive cap when desired. Unix modes including special bits, ownership,
file/directory ACLs and xattr, hardlinks within a scope and symlinks are preserved.
Hardlink groups extending outside a scope are diagnosed instead of flattened;
include the complete group in one scope to preserve its topology.
Sockets, devices, unsupported symlink attributes and non-UTF-8 names are explicit
capture diagnostics. A scan is verified again but cannot freeze the filesystem;
pause concurrent editors and package transactions for a coherent capture.

capture_errors="strict" blocks publication when any path is incomplete.
Explicit "skip" records diagnostics, preserves previously saved unreadable paths
and returns 2. Incomplete snapshots are refused by apply. A failed export,
retention or required replica after publication reports the committed ID and
the follow-up failure, rather than claiming rollback.

File-only snapshots use schema 2; package baselines and namespace root metadata
use schema 3, acquisition catalogs schema 4, and based hosts with separate
observation fingerprints schema 5. Prior schemas remain readable. A first save after upgrade
may create a schema migration snapshot with zero file changes. Older binaries
cannot read the new metadata. The Rust API uses Save { dry_run, capture_only };
ordinary callers set capture_only=false. SnapshotView exposes resolved metadata
and verified read streams. capture_only/CaptureArchive are the private read-only
transport used by launch inputs, with no publication or authentication in core.
