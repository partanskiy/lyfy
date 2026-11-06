# Library and launch configuration boundary

`lyfy` is a Rust library package at crates/lyfy. `lyfy-cli` is a separate package
at crates/lyfy-cli, depending on that library and producing the command `lyfy`.
A Rust application can depend directly on the library using a Cargo path dependency:

```toml
[dependencies]
lyfy = { path = "/path/to/lyfy/crates/lyfy" }
```

This does not require CLI parsing, terminal or report-format dependencies.
The library still targets Arch Linux and requires its native package libraries.
The ordinary Rust library target is linked into the caller; this does not add a
service process or a separately installed liblyfy shared-library ABI.

## What owns configuration

| Responsibility | Owner |
| --- | --- |
| Discover lyfy's application TOML through XDG | lyfy-cli |
| Read/parse application TOML, CLI arguments and ENV | lyfy-cli |
| Resolve launch defaults and flag > ENV > TOML precedence | lyfy-cli |
| Terminal progress, output format/destination and sudo policy | lyfy-cli |
| Accept typed operation parameters and validate their meaning | lyfy |
| Read pacman.conf, package databases and saved snapshots | Core infrastructure in lyfy |

`lyfy::Config` is the current public type for resolved operation parameters:
root, managed scopes, host, repository paths and maintenance policies. It does
not load an application config file, discover XDG, read ENV or select defaults
from the invoking user's environment. It is not coupled to the TOML file layout.
Library callers construct it directly. CLI callers reach it through input
resolution. Serialization traits permit conversion without choosing a file format.

`lyfy::Context` owns validated parameters and a repository session shared by the
core services. It is an in-process resource object, not a server context or a
configuration loader. A Context can execute multiple requests through the same
explicit API. The four requests remain lint, fmt, save and apply.

```rust
use lyfy::{Config, Context, Request, execute};

fn lint(options: Config) -> anyhow::Result<u8> {
    let context = Context::new(options)?;
    Ok(execute(&context, Request::Lint)?.exit_code)
}
```

The caller in this example also declares anyhow if it names that error type.
The complete standalone API fixture is in crates/lyfy/tests/api.rs.
`execute_observed` additionally accepts synchronous inspection feedback; the
caller chooses how to present it. Save takes
`Request::Save { dry_run: false, capture_only: false }`; set dry_run=true for
a read-only preview. Access holds caller-supplied connection material separately
from Config and saved state; `Context::with_access` validates absolute paths.
`Context::snapshot("default")` or a named mount returns SnapshotView with
resolved SnapshotState and verified content streams. See the complete API test
for capture/publication separation without CLI or sudo.

## Resource descriptions

`SnapshotView::resources()` returns a validated `ResourceGraph` on explicit
request. `state()` exposes the resolved current-format SnapshotState;
open_content() opens concrete objects listed in state.sizes. SNAPSHOT_SCHEMA=9
and ARCHIVE_SCHEMA=7 are the only accepted formats. Resource graph schema 1
versions descriptions independently. Prior stored/archive formats are rejected.

ResourceId identifies a subject in a collection and host. ResourceReference adds
its backend and immutable generation through SnapshotReference. Subjects cover
paths, packages, identities, services, unit definitions, mount prerequisites and
system requirements. RecoveryDescription distinguishes concrete files, package
baselines, Git projects, application images, generated outputs, explicit deletions
and the corresponding system descriptions.

Graphs retain metadata, hardlinks, tombstones, catalogs and diagnostics and
assign Recorded guarantees. This does not promote observed package classification
or an unverified external prerequisite into independent recovery. SystemState
exposes public identities, persistent enablement, source topology and coverage;
passwords and private bootstrap material are never account snapshot fields.

```rust
use lyfy::SnapshotView;

fn inspect_resources(view: &SnapshotView) -> anyhow::Result<()> {
    let graph = view.resources()?;
    for reference in graph.dependency_order()? {
        let logical = reference.identity();
        // Use the typed identity for selection without changing stored state.
        logical.validate()?;
    }
    Ok(())
}
```

`ResourceGraph::from_snapshot` also adapts a caller's resolved SnapshotState and
an explicitly matching source reference. Graph validation rejects unknown
schemas/kinds, duplicate identities, missing/cyclic dependencies, incompatible
guarantees and inconsistent filesystem relationships. Directory and hardlink
dependencies precede their consumers in deterministic order. Validation is
structural; source availability and target execution require provider checks.
The graph does not grant execution permission or add a fifth operation.

`ResourceGraph::select(&[ResourceId])` returns requested references, additional
dependencies and their execution order separately. Config.selected_paths and
selected_resources select apply work without changing capture include/exclude.

For a privileged input coordinator, Context::export_apply prepares per-scope
verified archives; with_restore_sources consumes them for apply only. Source
generation identities survive the handoff. read_receipts/with_restore_receipts
carry caller home history, while record_receipts publishes confirmed receipts
only if expected caller history still matches. Caller history never overrides
trusted system preimages. These capabilities do not authenticate or elevate;
the launch input owns that boundary and root-owned state selection.
Plans report required dependencies; an unsatisfied boundary blocks writing.
Partial execution merges affected entries into receipts and retains other state.

Package acquisition, projects, mount guards, system providers and current-format
archives share these contracts. See [the implementation record](resource-implementation.md)
and [system recovery](system-recovery.md).

The Config/Context API remains available through explicit root exports. Internal
layer paths, including the old lyfy::domain namespace, are private. See
[application/API layout](application.md) for the execution contract and import
migration. More
specific per-operation parameter types can be considered separately. There is
no third configuration package: only one launch implementation currently needs
the application file loader. Future inputs can construct typed core parameters
without inheriting CLI configuration files or terminal behavior.

## Building and checking

```sh
cargo build -p lyfy --locked
cargo test -p lyfy --locked
cargo build -p lyfy-cli --locked --release
cargo test --workspace --locked
bash scripts/check-library-deps.sh
```

The virtual workspace shares one Cargo.lock, target directory and version
metadata. Package dependency declarations remain separate. The library has no
cli feature and cannot depend on lyfy-cli or launch libraries, including through
platform-specific, renamed, build or development dependencies. CI checks the
resolved dependency graph as well as source and manifest boundaries.
