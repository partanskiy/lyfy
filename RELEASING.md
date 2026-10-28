# Releases and AUR

The current release target is Arch Linux x86_64 (amd64). AUR package `lyfy`
builds from source. There are no arm64 builds, other distribution packages or
`lyfy-bin` package in this release policy.

The maintainer agent may merge green PRs by rebase, tag selected main commits
and publish releases without requesting another deployment approval. Main
protection, required checks and small Conventional Commits remain mandatory.

## Choose a commit

Versions follow synchrogit's calendar scheme, `YY.M.PATCH`, with no leading
zero in the month or patch. For example, `26.10.0` is the first October 2026
release and `26.10.1` is the next release that month. Start PATCH at 0 for a new
month and increment it for each published release in that month, including
feature releases. The version communicates release timing; compatibility
changes must be explained in release notes. The first calendar release is
`26.10.0`; previous `0.1.x` releases remain historical and immutable.

All source and packaging changes go through a PR. After required checks pass,
merge by rebase. Select an existing resulting main commit. A dedicated version
bump, changelog or release commit is not required.

```sh
git fetch origin main
git tag -a v26.10.0 <selected-main-commit> -m 'lyfy v26.10.0'
git push origin v26.10.0
```

Only stable calendar tags currently publish. `scripts/check-version.sh` is the
shared validator for the workflow and packaging/publication tools; it rejects
padded months, invalid months, prereleases and legacy SemVer numbering.
Release tags and published
assets must not be rewritten. The version tag controls LYFY_BUILD_VERSION for
both the downloadable binary and AUR builds; Cargo.toml's development version
does not have to match the tag.

## What the workflow does

1. Verify the tag points to a commit reachable from main and the build runs on
   x86_64. Rerun architecture, formatting, clippy, CLI and library tests.
2. Build `x86_64-unknown-linux-gnu`. Collect exact license/notice texts from
   locked Cargo dependency sources, without installing Python packages.
3. Publish a GitHub release with a binary/documentation/license archive, a
   source archive and SHA256SUMS. Both archives include the already generated
   dependency license notices as a plain text file.
4. Download and verify the source archive in an Arch Linux build environment.
   Render the actual PKGBUILD and .SRCINFO, then run makepkg as an unprivileged
   user against that published source and its fixed checksum.
5. Keep the tested PKGBUILD and .SRCINFO as workflow artifacts for local AUR
   publication, or publish automatically if explicitly configured below.

Arch packaging is also a required PR check. It builds and inspects an actual
package without installing it on the developer's laptop. The package includes
only /usr/bin/lyfy, man pages, documentation, an example configuration and licenses. It
does not write user configuration, install a service or run a system mutation.

The source recipe fetches locked Rust dependencies in prepare(), then builds
and tests offline with --frozen. Runtime dependencies are managed by pacman.
The AUR recipe does not require or invoke Python. Notice generation and recipe
rendering happen in the maintainer's release/check pipeline before makepkg;
the package only installs the resulting license text. Neither the PKGBUILD
nor release scripts use pip, pipx or equivalent installers.

## Local authenticated AUR publication

After a successful release workflow, download its `lyfy-aur-<version>` artifact
and publish the tested recipe with the local AUR identity:

```sh
AUR_SSH_KEY_FILE=/absolute/path/to/aur-key \
  bash scripts/publish-aur.sh 26.10.0 /absolute/path/to/rendered-package
```

The private key stays on the local host; SSH uses it to authenticate directly
to AUR. The script checks the AUR server's published ED25519 fingerprint,
preserves AUR history, rejects version downgrades and uses a Conventional
Commit. AUR requires its package Git branch to be named master; the upstream
source repository remains main with rebase-only PRs.

No GitHub credential upload is needed for this local path. The owner explicitly
authorized using the existing AUR credential in partanskiy/lyfy Actions for
automatic publication. That path uses the AUR_SSH_PRIVATE_KEY Actions secret
and repository variable AUR_AUTOPUBLISH=true. Never print or commit the secret.
If the variable is disabled, the workflow retains the tested recipe for local
publication instead.

## Local packaging check

For the maintainer's packaging check, install Arch base-devel, cargo, python,
git, groff and runtime dependencies. Python is used by the check tooling, not
by the AUR build:

```sh
bash scripts/check-arch-package.sh 26.10.0 /tmp/lyfy-package-check
```

The helper adds pre-generated notices to the current committed source archive
and renders the same variable-based recipe used in AUR. It preloads the local
source archive into makepkg's cache instead of replacing the production URL.
It renders the manuals, builds the package, extracts it in the work directory
and verifies its version, man pages, license notices and installed paths.
It does not invoke sudo or pacman -U.

Sources: [Rust package guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines),
[PKGBUILD reference](https://man.archlinux.org/man/PKGBUILD.5.en.html),
[published AUR SSH fingerprints](https://aur.archlinux.org/).
