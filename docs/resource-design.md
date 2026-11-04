# System resources and selective restoration

Status: accepted direction on 2026-11-04. This contract extends the
[storage design](storage-design.md) with resource ownership, recovery sources
and selective execution. It defines implementation requirements; current
commands and settings remain documented in save.md and configuration.md.

The objective is complete accounting of persistent system state with efficient
recovery. Every selected resource has a recovery description or an explicit
exclusion and reason. A reference to an external source carries an availability
dependency. An independent backup retains everything required by its declared
recovery boundary. Neither claim implies an atomic disk image.

## Scope and recovery source and storage

Keep these decisions independent:

| Concept | Responsibility |
| --- | --- |
| Scope | Logical VFS paths, selection, nested ownership and exclusions |
| Recovery provider | How to recreate a resource and verify the result |
| Backend | Where descriptions and required content are persisted |
| Policy | Recovery guarantee, encryption, history and required replicas |

Providers cover concrete files, packages, Git projects, external mounts and
supported generated/application state. Use typed resource descriptions and
explicit implementations within the existing layers. This does not introduce
a command registry, arbitrary execution hooks or a general plugin framework.

| Example | Recovery description | Stored content |
| --- | --- | --- |
| /usr/bin/ssh | Package openssh and its acquisition policy | Package metadata; unchanged payload omitted |
| A modified packaged config | Package baseline plus a concrete override | Override bytes and metadata differences |
| ~/Pictures | Concrete files and filesystem metadata | S3 or another selected backend |
| ~/Projects | Ordinary files with nested resource boundaries | Selected backend content |
| ~/Projects/demo | Git project plus local work | Pinned Git state and required local content |
| ~/Cloud | External connection and mount identity | Connection description; independent copy when selected |
| Runtime or reproducible cache | Explicit exclusion or supported generation | Reason or generation inputs |

Git used to store lyfy snapshots and Git used to restore a project are separate
roles. A project remote does not become a lyfy snapshot repository implicitly.
Yandex Disk needs its own backend capabilities; it is not an S3 endpoint.

The longest component-prefix scope owns a path. A nested scope stays outside
its parent even when unselected. A resource has one baseline recovery provider;
concrete overrides and metadata changes supplement that baseline explicitly.
Replicas preserve one confirmed semantic generation across destinations.

## Resource identity and dependencies

A resource has a stable identity scoped by collection and host, a typed recovery
description, observed metadata, content references and dependencies. References
pin the backend, collection, host and immutable generation needed for recovery.
Paths retain the existing home/system namespaces and destination home mapping.

The dependency graph expresses facts such as a package providing a file,
a concrete override requiring that package, a mount exposing a tree, or a
generated artifact requiring configuration and tools. Validate references,
content hashes, cycles and provider support before execution. Loading saved
state never dispatches arbitrary commands supplied by a manifest.

Recovery reports distinguish recorded observations, recovery through pinned
external sources, and independent retained copies. Unknown provenance or an
unavailable dependency cannot satisfy a stronger guarantee. Strict capture
blocks publication when required resources fail the declared guarantee.
Explicit exclusions and supported generation rules remain visible in coverage.
Existing explicit skip behavior remains incomplete and returns a nonzero result.

Only verified content is reused. Stream new content and transfer absent objects
without duplicating package payloads or parent/child scope content. Chunking and
compression require separate implementation; they are not current guarantees.
An aggregate manifest references confirmed resource generations and records
partial failure. Independent backend commits do not create a global transaction.

## Packages and concrete exceptions

Use libalpm ownership and expected metadata to compare selected paths against
the installed package baseline. A matching filename alone does not establish
that its content or metadata is unchanged. Missing comparison evidence produces
a coverage issue or requires an explicitly supported fallback.

Unchanged packaged payloads are represented by their package recovery source.
Preserve modified configs as concrete files, metadata-only changes as explicit
differences, and deliberate missing package files as tombstones. Modified
packaged executables need an explicit override policy; they cannot disappear
from capture because their names belong to a package.

Restore the package baseline before applying its concrete exceptions. A changed
package version requires reconciliation against its new baseline; do not blindly
overwrite changed upstream configuration. Package transactions and their hooks
have effects beyond individual paths and their own recovery semantics.

Acquisition descriptions cover these cases:

| Source | Required provenance |
| --- | --- |
| Configured repository | Repository identity, package, architecture, version policy and signature policy |
| AUR recipe | pkgbase, recipe revision, auxiliary files, resolved sources and build requirements |
| Local PKGBUILD | Concrete recipe, patches, source identities and build requirements |
| Package archive | Verifiable locator, identity and digest |
| Unknown | Explicit unresolved recovery description |

Native/foreign classification is observation, not proof of installation origin.
Repository candidates and helper caches may provide evidence; ambiguity remains
explicit. Save reads and records evidence without evaluating a PKGBUILD.
Recipe execution is a declared package action after review of its pinned inputs.

The default omits installed executable bytes. Exact recovery requires available
immutable packages or sufficient pinned sources and build inputs; a recipe name
or version string alone cannot guarantee it. Current-version and locked recovery
are explicit policies. Bit-for-bit build reproducibility requires verification.
Never silently substitute a newer recipe or artifact for a locked dependency.

Build recipes run without root, following [makepkg](https://man.archlinux.org/man/makepkg.8.en).
Installation uses pacman with root privileges,
dependency/conflict handling, signature policy and preserved installation reasons.
Privileged installation consumes verified artifacts protected against replacement
after validation. Package removal is an explicit planned action. Source and build
policies retain pacman management of system Python and its dependencies.

## Git projects and local work

Record repository identity, credential-free remote references, HEAD, relevant
refs and required object closure. Account for unpublished commits, the index,
unstaged changes, untracked files, submodules, LFS and required local settings.
Ignoring a file in Git does not classify it as disposable backup data.

An external recovery source pins and verifies the required Git objects and
records its availability dependency. An independent backup retains those objects
and required local data in selected storage. A [Git bundle](https://git-scm.com/docs/git-bundle)
alone does not preserve the working tree or index. Shallow repositories, alternates and linked worktrees
need explicit closure handling before a completeness claim.

Capture does not commit local work into the project's branch or publish it
implicitly. Restoration recreates the recorded repository and local work while
checking existing target changes. Credential material stays in connection inputs
or explicitly protected secret storage, outside ordinary manifests and reports.

## External mounts and generated state

Distinguish restoring a connection, independently backing up remote data, and
materializing remote data locally. Discover mount boundaries before walking.
An unknown boundary needs classification; parent capture cannot silently descend
into a different mounted source. Verify source identity and availability during
observation and before publication. A missing mount is not an empty source and
must not publish deletions. Credentials remain separate from mount descriptions.

Backends retain Linux metadata in the shared model regardless of remote filesystem
features. Validate destination metadata support before restoration. Publication,
concurrency and history capabilities are checked for each provider, including
Yandex Disk. Unsupported publication guarantees cannot be inferred from a
filesystem-shaped interface.

Generated resources declare their inputs and supported regeneration steps.
Persistent application data requires a consistent capture method; live databases
need a supported export/checkpoint or equivalent protocol. Include identities,
service enablement, storage/mount configuration and boot requirements in system
coverage. Disk preparation and boot recovery need explicit execution plans.

## Selection and execution

Apply selects collection names, logical path prefixes or declared resource
identities from one resolved generation. Data scopes participate explicitly or
through configured defaults. A selection is distinct from changing capture policy.
The proposed path/resource selectors require implementation and are not CLI syntax.

1. Resolve the selection, pinned source generations and required dependencies.
2. Report every effect outside the requested boundary and require explicit
   execution permission for expansion. A package or repository can be a larger
   recovery unit than a selected file. Never silently install packages for a
   Pictures restore or reset unrelated project files.
3. Preflight the full selected plan: content access, metadata support, identity
   mapping, privileges, conflicts, hardlink constraints and target preconditions.
4. Execute supported actions in dependency order, revalidating live preconditions.
   Journal completed actions and preserve recovery data on failure.
5. Record verified results for the affected resources only. Merge partial receipts
   with unaffected state; unselected scopes never become deletions or lose receipts.

Existing overwrite/prune safeguards remain explicit. Hardlinks that require
resources outside the selection need visible dependency handling; unsupported
cross-filesystem links cannot be silently flattened. Providers that cannot
restore a smaller unit must expose that boundary before execution.

## Privileges and host identity

Privilege requirements belong to actions. Ordinary restoration into the invoking
user's home can run unprivileged when all required metadata is representable.
System writes, arbitrary ownership and package installation require root.
Ordinary file owners can set modes and groups within Linux's
[chmod](https://www.man7.org/linux/man-pages/man2/chmod.2.html) and
[chown](https://www.man7.org/linux/man-pages/man2/chown.2.html) restrictions.
Insufficient privileges or filesystem features block the selected plan before
its first target mutation. Protected planning reads can also require privileges.

The input owns authentication and any one-shot privileged execution handoff.
The library accepts resolved identities and execution authority; it never invokes
sudo. Preserve the existing four commands, package count and layer boundaries.
The user coordinator owns remote access and its repositories. A privileged
executor validates an immutable selected plan and content, keeps system journals
in trusted root-owned state, and does not write to user repositories or evaluate
user Git hooks. No persistent service or additional public command is needed.

Retain symbolic home ownership and add explicit mapping for required named system
identities, including ACL subjects. Resolve collisions and missing users/groups
before writes. Root execution alone cannot make source UID/GID values portable
between hosts. Root-directory metadata needs explicit support for complete
system property coverage; current capture skips root/home nodes themselves.

## Shared desired state and migration

Save records observations without promoting every host difference into shared
intent. Common settings have an independently versioned desired generation.
Updating a host from an old base to a new base reconciles both bases and existing
host exceptions, preserves unrelated exceptions, and reports overlapping changes.
Old live content must not silently override an incoming shared update through
an ordinary save. Hosts pin accepted bases until that reconciliation completes.

Retain existing schema readers and complete export/import. Introduce a new schema
only with validated resource descriptions, pinned dependencies and recovery
semantics. Unknown resource kinds fail explicitly. Existing settings preserve
their precedence and fallback behavior; new selectors/settings need matching
library, snake_case CLI and environment contracts.

Connection credentials and active decryption identities remain outside capture.
Secret payloads require compatible encrypted storage, with key recovery arranged
separately. Current Git payload encryption remains unsupported; this contract
does not make a public state repository suitable for plaintext system snapshots.

Migration compares each host's own live state, verifies declared coverage and
round trips through temporary targets, then exercises reviewed selective home
deployment. Full system restoration needs separate package/privilege/boot
validation before replacing an existing system maintenance workflow.

The [implementation plan](resource-implementation.md) defines delivery stages
and acceptance criteria for each recovery capability and migration gate.
