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
caller chooses how to present it.

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
