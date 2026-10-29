# Arch Linux rules and check lifecycle

`crates/lyfy/src/domain/rules.rs` is the single catalog. `RuleId` is a closed enum, generated
from that catalog. Each entry defines a stable serialized ID, semantic revision,
check owner, severity, category, subject kind, explanation, remedy and supported
fix kind. Services emit typed IDs. Strings cannot introduce an unregistered rule.

| Rule | Severity | Category | Observation | Automatic fmt |
| --- | --- | --- | --- | --- |
| arch.os_release | error | integrity | Target os-release does not have ID=arch | No |
| arch.pacnew | error | maintenance | Regular .pacnew file in /etc | Only an identical copy of its active file |
| arch.pacsave | warning | maintenance | Regular .pacsave file in /etc | No |
| arch.database_lock | warning | observation | Pacman db.lck exists | No |
| arch.missing_dependency | error | integrity | libalpm reports an unsatisfied dependency | No |
| arch.package_conflict | error | integrity | libalpm reports conflicting installed packages | No |
| arch.file_owners | error | integrity | Several packages own the same non-directory path | No |
| arch.package_files | error | integrity | Installed package paths are missing, respecting NoExtract | No |
| arch.package_metadata | warning | observation | mtree type, mode, owner, size, mtime, link or SHA-256 differs | No |
| arch.unowned_file | warning | observation | File/symlink in ownership_roots has no package owner | No |
| arch.orphans | info | maintenance | Dependency has no required-by or optional-for users | No |
| arch.kernel_reboot | warning | maintenance | Running release has no installed modules/version/vmlinuz | No |
| arch.uki | error | policy | bootctl does not report Type #2 UKIs exclusively | No |
| systemd.failed_units | error | integrity | System systemd units have failed | No |

An observation is not automatically damage. Backup configurations can be edited
intentionally; a database lock can belong to an active transaction. Unowned files
and orphan packages need review. UKI is a lyfy policy, not an Arch requirement.
The current metadata rule groups typed differences per package; it does not
claim that every modification is corruption. No rule deletes these files blindly.

## Findings and coverage

`Report` has separate `findings`, `diagnostics` and `checks`. A finding records
rule revision, severity, subjects, explanation, optional typed action, fixed state
and typed package evidence. The historical `paths` field contains subjects of
`RuleSpec.subject_kind`: only File subjects are filesystem paths. Packages,
units, boot-entry IDs and kernel versions must not be interpreted as paths.
Each package lists at most 100 missing/changed files, with the full count in its
detail. Unowned paths have the same limit.

Coverage diagnostics are `incomplete` or `access_denied`, associated with a
check. Permission denials are deduplicated and aggregated per check, with up to
100 locations and the full count. Accessible paths continue to be scanned.
Missing tools, unsupported data and malformed records also prevent a complete
scan. These are not system rule violations.

Check IDs and labels live in `crates/lyfy/src/domain/checks.rs`. Inspection records one of:

- `complete`: observations collected, possibly with findings.
- `incomplete`: some requested observations failed.
- `disabled`: configuration or disabled_rules excluded the check.
- `not_applicable`: for example runtime checks with an alternative root.
- `blocked`: a prerequisite did not complete.

A check records duration and finding count. The package database opens once;
its consistency, ownership, file and orphan checks share that read-only handle.
Checks containing only disabled rules do not run. Disabling the metadata rule
also avoids reading mtree or hashing package content for that rule.

The default fail_on threshold is warning. Complete scans return 1 for outstanding
findings at/above the threshold and 0 otherwise. Incomplete coverage always
returns 2, including when other findings exist. Info observations are visible
without failing the default policy. Skips deliberately selected by configuration
are not incomplete coverage. See [report formats](lint.md).

## Adding or updating a rule

1. Add a typed entry to the catalog. Choose its existing check owner or add a
   CheckId with an explicit name and execution path. A check groups shared facts;
   do not add a trait/implementation pair or repeat a filesystem walk per rule.
2. Emit the typed ID from that inspection capability. Run verifies that the active
   check owns it, applies disabled_rules and publishes synchronous observations.
   Missing facts produce diagnostics, never fabricated successful results.
3. Add positive and negative fixtures, failure/coverage cases, and a document
   entry. Native checks use synthetic local databases, never the developer's host.
4. Preserve existing ID meaning. Increment that entry's revision for semantic
   changes. Replacing or removing an ID requires a documented migration below.
5. For automation, add a closed FixKind/FixAction variant and exhaustively handle
   it in remediation. Validate its rule association, scope and preimages; use
   transactions and verify postconditions before marking the finding fixed.
   Test changed input, excluded scope, conflict and failed postconditions.

Catalog and lifecycle tests enforce ID uniqueness, real check owners, action
compatibility, disabled checks, coverage and exit semantics. The library-only API
suite runs without any input/terminal libraries. lint and fmt share services and
never invoke one another. There is no plugin registry or implicit command.

## Native observation boundaries

Package checks use target pacman.conf, recursive Include/globs, DBPath,
NoExtract and configured sync repositories. A local database and version marker
must already exist; inspection refuses to let libalpm initialize them. Missing
mtree for a package with payload makes metadata coverage incomplete. An empty
package file list is valid. Malformed mtree is incomplete. SHA-256 streams with
bounded buffers; the 120-second deadline applies to external queries, not hashing.

Shifted/incomplete user-namespace ownership mappings leave UID/GID coverage
incomplete, while other fields continue. Runtime checks (kernel, EFI, systemd)
apply only to root = /. Package queries never substitute the host for a target.
The kernel rule detects absence of the running image, not all possible pending
kernel updates. UKI inspection uses bootctl JSON; it does not verify signatures,
confirm the active bootloader or guarantee the next image will boot.

## Report schema 2 migration

- `arch.database` is retired, replaced by `arch.missing_dependency`,
  `arch.package_conflict` and `arch.file_owners`.
- `scan.incomplete` and `scan.access_denied` are retired as rule IDs. Inspect
  `diagnostics[].code` and check status instead. Partial coverage now exits 2.
- `arch.orphans` is informational; use fail_on = "info" to fail on it.
- `fixable` is replaced by an optional typed `fix`; `fixed` requires verified
  execution. Findings and exports carry rule revisions and schema_version = 2.

References: [pacman](https://man.archlinux.org/man/pacman.8.en),
[bootctl](https://man.archlinux.org/man/bootctl.1.en),
[UKI specification](https://uapi-group.org/specifications/specs/unified_kernel_image/),
[Boot Loader Specification](https://uapi-group.org/specifications/specs/boot_loader_specification/).
