# Resource recovery implementation plan

Status: staged delivery plan for the resource contract accepted on 2026-11-04.
The [resource design](resource-design.md) defines the target behavior. The
implementation baseline below is main at 278484c. Each stage requires its own
reviewed commits and verification before its behavior is advertised.

## Current implementation baseline

| Capability | Current support | Required extension |
| --- | --- | --- |
| VFS ownership | Nested paths, longest prefix and per-mount selection | Arbitrary path/resource selection and recovery boundaries |
| File state | Concrete content, metadata, hardlinks and strict capture | Provider baselines, exceptions and root property coverage |
| Packages | Inventory, native/foreign observation, limited native install | Provenance, baseline omission, build sources and recovery policies |
| Git | Backend for committed lyfy snapshots | Project history, index and local work recovery |
| Remote storage | S3, replicas, age, retention and archives | External mount identity and a Yandex Disk adapter |
| Apply | Per-scope file plan, backups, journals and receipts | Dependency plans, partial receipts and privileged execution handoff |
| Common base | Immutable pin per saved host | Reviewed reconciliation of old/new bases and host exceptions |

Current capture excludes package binaries through default include paths, not by
comparing every selected file with package metadata. packages::file_scope only
excludes the configured pacman database. A broad include is not an efficient
package-aware system backup yet.

Current apply requires existing root privileges for package installation.
User-owned private repositories cannot simply become root's writable session;
store::private_dir enforces invoking-user ownership. Current receipts describe
an entire resolved scope and cannot be reused unchanged for a smaller selection.

## Stage 1 Resource descriptions and compatibility

Introduce typed resource identities, recovery descriptions, pinned references,
declared guarantees and dependencies in domain. Adapt existing file snapshots
and package inventories without assigning unproven provenance or new guarantees.
Keep concrete payloads and metadata; preserve current four-operation API boundaries.

Acceptance: old schema 1/2 fixtures still read and round-trip through supported
archives. Unknown kinds, cycles, dangling references and incompatible guarantees
fail explicitly. File-only snapshots preserve existing apply behavior. A new
schema is introduced only with complete validation and reader compatibility.

## Stage 2 Selective planning and receipts

Resolve collection, path and resource selections through routing and a typed
dependency plan. Separate selection from capture policy and preserve nested scope
ownership. Resolve and display provider boundaries and effects outside selection
before authorizing expansion. Prepare only selected connections and dependencies.

Acceptance: restoring one Pictures subtree accesses no Projects/package backend,
preserves sibling files and merges only the affected receipt entries. Selecting
a parent preserves unselected nested mounts. Required package, parent-directory
and hardlink effects are visible. No selector silently broadens restoration or
turns an unselected/missing source into a deletion.

## Stage 3 Privileged execution and identities

Preflight action privileges, destination metadata features and logical identity
mapping. Implement the input-owned one-shot handoff for verified system plans
and immutable content, with root-owned journals and receipts for system actions.
Keep the user coordinator responsible for its storage sessions and credentials.
Resolve the ownership boundary before enabling mixed home/system execution.

Acceptance: user-only restoration runs without sudo when its metadata permits;
system changes require authenticated root execution. An insufficient privilege
or unsupported attribute blocks execution before the first target mutation.
Tampered content, changed preimages and unsafe paths fail validation. The
executor never changes user repository ownership or uses root's Git identity.
Use temporary target roots and a controlled executor boundary in tests.

## Stage 4 Package baselines and file exceptions

Compare selected files with libalpm ownership and expected content/metadata.
Omit unchanged payloads through a package recovery reference; retain concrete
config overrides, metadata differences and explicit deletions. Capture changed
executables only under a declared supported policy. Implement root metadata
support and identity mapping needed for selected system properties.

Acceptance: a broad synthetic system capture stores no unchanged package binary
bytes; changed config, mode, ACL/xattr and missing files survive round trips.
Missing comparison evidence cannot be treated as an unchanged file. Applying
an override to a changed package baseline produces a reviewable reconciliation.
Package effects remain visible when selection names only one provided file.

## Stage 5 Package acquisition and builds

Record evidence for repository, AUR, local recipe and package-archive sources.
Capture recipes, auxiliary files and pinned source/build requirements without
evaluating them during save. Keep unresolved origin distinct from a known source.
Support explicit version/update policies and verified artifact acquisition.

Acceptance: temporary fixtures cover a custom repository, pinned AUR recipe,
local PKGBUILD and unverifiable source. Sources required by the selected guarantee
must remain available and verified; absence does not select a newer substitute.
Builders run unprivileged, privileged installation consumes protected verified
artifacts, and pacman retains dependency/signature/reason handling. Package
removal and transaction side effects require explicit planning. System Python
dependencies remain pacman-managed.

## Stage 6 Common state and encrypted migration

Implement three-way reconciliation of old common base, incoming common base
and existing host exceptions. Retain accepted pins and preserve observations
without silently promoting old live content into exceptions against a new base.
Choose supported encrypted routes for secrets and validate separate key recovery;
Git payload encryption requires its own encoding/compatibility change if chosen.

Acceptance: two temporary hosts receive an unrelated common update while retaining
machine-specific files. Concurrent edits to the same resource produce conflicts.
Saving observations before/after synchronization cannot discard unapplied shared
intent. Protected keys/credentials stay outside snapshots and reports. Encrypted
exports and selected restores work with explicitly supplied test identities.

## Stage 7 Git projects

Capture project refs, required objects, HEAD, index, working tree and additional
files. Classify submodules, LFS, alternates, shallow clones and linked worktrees.
Separate external object dependencies from an independent retained project copy.
Apply project state through the same selection and conflict machinery.

Acceptance: unpublished commits, staged/unstaged changes and important ignored
files survive recovery without a source-tree commit or automatic push. An
independent copy restores after its synthetic remote disappears. Missing objects
and unsupported layouts prevent completeness claims. Restoring part of a project
never resets unrelated work through an implicit whole-repository operation.

## Stage 8 External mounts and additional cloud storage

Observe mount identities and availability before walking or interpreting missing
paths. Implement explicit connection-only, independent-copy and local-materialization
policies. Add Yandex Disk through verified storage capability contracts. Preserve
Linux metadata in manifests and keep OAuth/authentication outside saved state.

Acceptance: disconnected/replaced mounts cannot publish mass deletions. A nested
cloud tree stays outside parent capture until classified. Provider fixtures cover
publication, concurrency, interruption, history and metadata restoration according
to advertised guarantees. Live-provider compatibility is reported separately.

## Stage 9 Remaining system resources and migration gates

Extend typed coverage for required users/groups, service state, storage/boot
requirements, supported generated state and consistent application data exports.
Keep each provider's execution and recovery boundary explicit. Unsupported or
deliberately excluded state stays visible in the coverage report.

Before retiring an existing maintenance setup:

- Import each host's own observations and review shared intent separately.
- Reconcile path coverage against the old declarations, including exclusions,
  secrets, local binaries, package exceptions and root-directory properties.
- Prove no-op recapture and selected restore on disposable target roots.
- Exercise a two-host common update with preserved machine differences.
- Verify required replicas, decryption/key bootstrap and selected archive recovery.
- Verify package/privilege/system recovery separately from home synchronization.

All stages retain architecture guards, library/CLI tests, formatting, clippy,
release builds and per-commit checks. Recovery tests use synthetic data and
temporary targets; real host apply, package changes and boot changes are separate
deployment actions. Successful home synchronization is not evidence that full
system recovery has been validated.
