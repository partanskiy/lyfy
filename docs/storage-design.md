# Storage design: collections and backends

Status: implementation contract accepted by the user on 2026-10-31, with the
nested VFS routing correction below. Implemented behavior and compatibility are
documented in storage.md as features land. Examples are arbitrary paths, not
built-in collection names.

## Shared state model, independent storage

Keep one versioned semantic model for paths, concrete content, node kinds,
permissions, ownership, hosts, pinned bases, deletions and package inventory.
The model is not a mandatory local file layout. A backend encodes and persists
that model according to its storage guarantees. No intermediate Git repository
or complete local copy is required before saving to a remote backend.

Separate four concepts:

| Concept | Responsibility |
| --- | --- |
| Collection | Named set of paths, selection/exclusion rules and restore purpose |
| Backend | Named destination and its persistence capabilities |
| History policy | Keep history or publish current state only; retention of referenced data |
| Encoding | Concrete files/manifest, content objects or another backend layout |

Git is a backend with history and readable payload files. S3 is an object-store
backend, not a serializer or a Git export format. An archive export is a portable
representation of an already committed state, not a substitute for storage.
JSON/TOML/YAML used for command reports are a separate CLI presentation concern.

## Collections and routing

A collection names its paths, target backend, history policy and participation
in apply. The following is illustrative policy, not proposed configuration syntax:

| Collection | Paths | Destination | Policy |
| --- | --- | --- | --- |
| config | ~/.config, /etc | Private Git repository | Managed configuration with history |
| projects | ~/Projects | Separate Git repository or local directory | Project-specific exclusions and retention |
| pictures | ~/Pictures | S3 bucket/prefix | Streaming backup; current-only or retained snapshots |

These choices are independent: a collection can later use another backend
without changing its path semantics. Scope and size limits are per collection;
the current bounded config-file capture is not a suitable implementation for
large photo/project collections. Large content must stream with bounded memory.
Chunking, compression and encryption need separate implementation decisions.

Route arbitrary VFS mount points using the longest component-prefix match.
Nested mounts explicitly override their ancestors. Reject duplicate logical
roots and duplicate identities. Exclude each nested mount from its ancestor
before walking, even if that nested mount is not selected. This prevents a
disabled child from leaking into a parent's backend. A sibling prefix such as
~/Pictures-old is not a child of ~/Pictures.
Paths use the same validated home/system namespaces and no-follow rules as the
current core. All local backend/cache roots and runtime transaction paths are
excluded before walking any collection, preventing recursive capture. Default
project exclusions remain until explicitly migrating to a
collection policy; adding a backend never silently includes ~/Projects.

Routing and replication are different. Routing partitions paths between
collections. Replication sends the same committed collection generation to
multiple destinations. A replica has an explicit source generation and completion
state; it must not silently become an independently captured second collection.
Cache is another separate role, not a required authoritative local repository.

## History is a policy and a capability

Support both retained history and current-only state. Do not require native
backend versioning. A current-only collection still needs a generation identity
or concurrency token so readers/writers can distinguish complete states; that
does not promise recoverable history. Temporary staging and publication metadata
are needed even when only the current state is retained.

An object store can retain immutable manifests and content under distinct keys
without enabling bucket object versioning. Conversely, object versioning alone
does not define a coherent multi-file lyfy snapshot. AWS S3 Versioning is disabled
by default and is an independent bucket feature. [AWS documentation](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Versioning.html).

A backend advertises supported publication/concurrency, history, content access
and metadata guarantees. Reject unsupported policy combinations before capture.
Do not pretend that every backend is a filesystem or that every backend supports
Git-like history. Exact Rust interfaces should follow implemented operations,
not a universal CRUD/plugin framework.

## Publication and failure semantics

The proposed per-collection save contract is:

1. Resolve paths/policy and capture an expected current-generation token.
2. Observe and stream content into uncommitted storage. Verify observations;
   a scan is not a frozen view of the live filesystem.
3. Seal a complete manifest referring only to verified content.
4. Publish its generation conditionally against the expected token.
5. Report the committed generation. Retire staging/unreferenced content only
   under a separate retention/garbage-collection policy.

A failed capture never publishes a partially populated manifest as current.
Conditional publication detects concurrent writers rather than overwriting them.
Content integrity hashes remain separate from backend concurrency tokens.

AWS S3 has strong read-after-write consistency for objects, but does not provide
an atomic update across multiple keys. A design can upload content first and
conditionally update one current-manifest pointer last. If-Match/If-None-Match
provide conditional writes on supported APIs. These are capabilities to verify
for each S3-compatible provider, not assumptions for every object store.
[AWS consistency model](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Welcome.html#ConsistencyModel),
[AWS conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html).

There is no global transaction spanning Git, local storage and S3. A save run
reports committed/failed collections and any replicas still pending. A failure
in pictures must not misreport a committed config save as rolled back. An
aggregate state record may refer only to confirmed generations and reports
partial completion explicitly. Required collections must all complete for a
successful overall result; optional replicas have an explicit policy.

Retries are idempotent for a sealed generation. Resuming capture must revalidate
live observations or start a new generation, never silently substitute new bytes
under the old identity. Network effects belong to explicitly selected remote
backends; a local Git backend continues to avoid implicit fetch/push/merge.

## Hosts, bases and restoration

Collection state is scoped by host. Shared/base state has an explicit identity;
one host never overwrites another host's state through a shared latest pointer.
A reference includes enough backend/collection/host/generation identity to resolve
its data without relying on raw Git hashes as a universal version type.

Pinned bases must remain available for as long as dependent host states need
them. A current-only policy must either retain referenced generations or
materialize a self-contained state; incompatible combinations fail validation.
Retention never deletes a base/content object still reachable from another host.

Managed configuration uses the existing plan, receipt, conflict, backup and
transaction rules. A pictures backup does not automatically become desired
system configuration or participate in ordinary apply. Data restoration selects
collections explicitly and previews its own conflicts; absence from a backup
never silently means delete local photos/projects. The existing four commands
remain the operation set; this proposal adds no implicit fifth command.

Package inventory belongs to the system/config collection, not every data
collection. Cross-backend references and restore order must be validated before
writes; no backend runs scripts or templates from saved state.

## Placement in the core

Application save/apply coordinate collection work through named services. Services
own observation, publication policy, snapshot resolution and reconciliation.
Infrastructure owns backend encodings, I/O, conditional writes and streaming.
Domain owns validated collection policies, references and returned state/results.
CLI owns loading these policies and selecting operations, not backend protocols.
No outputs layer or additional package is needed for this design.

Optional export/import adapters consume the shared model and content access API.
An explicit read-only snapshot/content view must precede external exporters; the
current Saved return value is only a summary, not a complete state payload.
A one-way exporter cannot promise safe apply from an edited foreign format.
Round-trip import must validate metadata, content, deletions, host/base references
and supported semantics. A metadata-only document is not a complete backup.
Export never recaptures the live filesystem after save or changes committed state.
Authentication is an input-provided connection concern; credentials never enter
state manifests or ordinary reports.

## Implementation checkpoints

The accepted implementation uses strict capture by default. Unsupported sockets
and devices produce diagnostics; an explicit skip policy reports incomplete
capture and a nonzero result. Preserve ACL/xattr, hardlinks and special mode bits
without silently flattening them. Content access must stream large files.

Age recipients are public connection policy. Identities and S3 credentials are
input-provided connection secrets, never manifest data. Remote publication uses
conditional current-pointer updates. Replicas record the exact source generation;
failures remain visible and cannot recapture a different generation implicitly.
Retention preserves generations reachable through pinned base references.

Legacy storage_format/include/exclude remain a fallback scope. Explicit VFS
mounts take ownership before that scope is scanned. Data restore requires an
explicitly selected mount unless it was configured to participate in apply.
Export/import carry manifests, verified content and pinned bases, not reports.

Before implementing routing/backends, agree on publication capability contracts,
current-only retention with pinned bases, collection restore semantics, replica
failure policy and migration from storage_format/include/exclude. New settings
must keep snake_case flags, LYFY environment overrides and existing precedence;
exact syntax and per-collection overrides need a separate reviewed change.

Start with real Git/directory implementations and contract/round-trip/failure
tests. Add S3 only after backend publication and concurrency behavior is tested
against an explicit supported provider. Do not add unused traits, placeholder
backends, new flags or fake S3 support in this architecture-only iteration.
