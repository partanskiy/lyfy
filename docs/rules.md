# Arch Linux rule catalog

The executable implements these definitions from `src/domain/diagnostics.rs`.
lint and fmt share inspection services; neither command calls the other.

| Rule | Severity | Check | Automatic fmt |
| --- | --- | --- | --- |
| arch.os_release | error | Target os-release has ID=arch | No |
| arch.pacnew | error | Regular .pacnew files in /etc | Only an identical copy of its active file |
| arch.pacsave | warning | Regular .pacsave files in /etc | No |
| arch.database_lock | warning | Pacman db.lck exists | No |
| arch.database | error | libalpm dependency/conflict and duplicate file-owner checks | No |
| arch.package_files | error | Installed paths from libalpm are missing | No |
| arch.package_metadata | warning | mtree type, permissions, ownership, size, mtime, link or SHA-256 differs | No |
| arch.unowned_file | warning | File/symlink absent from local pacman ownership inventory | No |
| arch.orphans | warning | Dependency packages with no required-by or optional-for users | No |
| arch.kernel_reboot | warning | Running kernel has no installed modules/version/vmlinuz | No |
| arch.uki | error | bootctl reports Type #2 UKIs and no Type #1 entries | No |
| systemd.failed_units | error | System systemd units are failed | No |
| scan.incomplete | error | Tool, schema or other IO/traversal failure prevented a requested check | No |
| scan.access_denied | warning | Aggregated inaccessible paths/checks leave partial scan coverage | No |

Package metadata changes can be intentional, especially configured backup
files. Unowned files and orphan packages also need review, not blind deletion.
Ownership checks list at most 100 paths and report the total count. Do not
interpret that limit as a complete path listing.

Permission denials do not imply broken system configuration. Inspection keeps
walking accessible paths and aggregates denied paths/checks into one coverage
finding, listing at most 100 entries and the total count. Other failures remain
`scan.incomplete` errors. Both leave a nonzero lint result. For protected paths,
interactive `lyfy lint` requests sudo automatically for root = /; the child
retains resolved user settings. Use `--elevate=never` to disable the request,
or bare `--elevate` to request it for a noninteractive launch or another root.
Auto mode requires terminal stdin and stderr. Root access cannot bypass every
access policy or missing mount.

Lint reports each check's start, finish and duration on stderr. Package checks
call libalpm directly and expose completed/total package progress. Text findings
appear as each package completes. JSON stays one final document on stdout.
Package metadata findings include typed evidence with package, path, changed
fields and whether a file is a backup configuration. Path lists stay concrete
filesystem paths. Each package lists at most 100 changed or missing files and
reports its full count.

Package checks use the target pacman.conf, recursive Include/globs, DBPath,
NoExtract and configured sync repositories. NoExtract omissions are respected.
The package database must already exist with a version marker: libalpm can
otherwise initialize it, so lyfy refuses the open before that can happen.
Missing mtree metadata is explicitly skipped; malformed mtree is an incomplete
check. Full content checks stream SHA-256 with bounded buffers and can take
minutes. The 120-second deadline applies to external tool queries, not native
file inspection.

A user namespace can shift or hide ownership IDs. In that case UID/GID checks
are explicitly incomplete, rather than reporting every affected file as having
changed ownership. Other metadata and content checks continue.

The UKI check consumes bootctl JSON. It does not infer UKI validity from a file
extension, verify Secure Boot signatures, confirm the running loader is
systemd-boot or guarantee a new image will boot. This policy is stricter than
Arch Linux requirements and can be disabled with require_uki = false.

When root differs from /, runtime-dependent checks are skipped explicitly:
the running kernel, EFI/bootctl and systemd. File scans,
os-release and the target pacman ownership database remain usable. Package
queries never silently inspect the host instead of the configured target root.

fmt only fixes cases with one deterministic, implemented action. The identical
pacnew rule checks both regular files, guards the active file's observed state,
backs up the candidate and removes it without merging or replacing active
configuration. Other findings remain visible and produce exit code 1.

References: [pacman manual](https://man.archlinux.org/man/pacman.8.en),
[bootctl manual](https://man.archlinux.org/man/bootctl.1.en),
[UKI specification](https://uapi-group.org/specifications/specs/unified_kernel_image/),
[Boot Loader Specification](https://uapi-group.org/specifications/specs/boot_loader_specification/).
