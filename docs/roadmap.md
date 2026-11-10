# Product roadmap

Status: product direction recorded on 2026-11-10. This is a backlog, not a
claim of implemented support or a change to the accepted architecture.
The order below is proposed; there are no delivery dates yet.

## Product goal

Make checking, maintaining, saving and restoring an Arch Linux system a coherent
workflow through four operations:

- `lyfy lint`: inspect system health, findings and incomplete coverage.
- `lyfy fmt`: preview and execute supported fixes.
- `lyfy save`: preview and capture concrete system state.
- `lyfy apply`: preview and execute recovery from saved state.

The long-term goal is complete system recovery with explicit evidence of what
can be restored and which prerequisites remain. Today, saved state and passing
software tests do not prove recovery onto a bootable replacement machine.
[Recovery boundaries](system-recovery.md) describe the current guarantees.

The CLI remains a primary interface, with substantial visual polish as its own
goal. The browser UI is a visual workspace for understanding the current system:
its structure, lint findings, which parts are saved where and the state of those
saved copies. It also provides access to the four operations. The library/core
supplies system knowledge and behavior to both interfaces.

## 1. Polish the CLI visually

Intent: make the terminal interface beautiful and visually cohesive. uv and
colima are aesthetic references for presentation, animation and terminal polish.
The scope is the appearance of existing behavior and information; command
semantics, plans, error policy and result content remain the starting contract.

- [ ] Explore terminal mockups and recordings to choose a visual style before
  implementing it: spacing, alignment, emphasis, colors and status markers.
- [ ] Apply a coherent style across the four commands, their existing messages,
  progress indicators and result displays.
- [ ] Improve spinner and progress-bar appearance, animation cadence and the
  placement of existing stage labels, counters and timing information.
- [ ] Keep redraws smooth and the layout stable, with minimal flicker or jumps
  as labels, counts and path lengths change.
- [ ] Give headings, details, warnings and errors a clear visual hierarchy while
  preserving their meaning and informational content.
- [ ] Handle narrow terminals, resizing, long paths and large output gracefully.
- [ ] Keep authentication and package prompts visually clear alongside progress;
  clean up transient terminal rendering on completion, error and interrupt.
- [ ] Preserve `progress=auto|plain|never`, `NO_COLOR` and terminal detection.
  Keep progress on stderr and selected structured results on stdout, with no
  terminal escapes in redirected output or report files.

Acceptance: visually review before/after terminal recordings of existing
workflows, including long-running progress, warnings, errors, prompts, a narrow
terminal, color-disabled output and redirection. Assess layout, animation and
overall appearance directly. Keep operation behavior and structured output
unchanged. Use synthetic fixtures and disposable targets for execution scenarios.

## 2. Share operation feedback through the library API

Intent: provide typed system observations, saved-state information and operation
feedback to the interfaces without duplicating maintenance logic or interpreting
human-readable output.

The library already has typed results and a synchronous per-call observer for
inspection and capture. CLI visual work starts from those observations and
results. Extend the API where a concrete browser view needs additional data.
Keep rendering and interface-specific state in launch inputs; do not introduce
an output layer or an event bus into the library.

- [ ] Identify the public read-only data needed for the system overview,
  resource-linked lint findings, storage routing and saved-copy status.
- [ ] Distinguish configured capture destinations, confirmed saved generations,
  observed live differences and unknown or outdated observations.
- [ ] Reuse operation feedback for active work and define missing typed feedback
  only where required by a concrete interface view.
- [ ] Keep display feedback separate from private capture archive bytes.
  File payloads, credentials and privileged handoff data must not become UI
  progress messages or ordinary logs.

Acceptance: interface views consume public typed data and observations. Core
tests require no terminal or browser. Operations remain independent and share
services without invoking one another. Changes to the public contract receive
their own design review and tests.

## 3. Add a browser UI

Intent: open a beautiful, browsable view of the current Arch Linux system,
understand its condition and see how its parts are preserved. System exploration
and save-state visibility are primary workflows alongside running operations.
The UI uses the lyfy library/core as its source of data and behavior.
The first useful increment is a read-only system overview; later increments
add explicit execution.

- [ ] Design an attractive system overview with a clear visual hierarchy and
  drill-down into paths, packages, services and other supported resources.
  Show observed unmanaged areas and unknown coverage explicitly.
- [ ] Run `lint` and highlight deficiencies on their relevant system subjects.
  Connect overview indicators to findings, severity, evidence and supported
  fixes. Show incomplete coverage separately from system findings.
- [ ] Visualize which parts of the system are saved to which collections and
  storage destinations, including nested ownership and exclusions. Selecting a
  part should explain its capture scope and associated saved copies.
- [ ] Show save status: last confirmed save, snapshot/generation, destination,
  replica results, known changes since capture and failed or incomplete saves.
  Distinguish configured routing from confirmed saved data; show observation
  timestamps and unknown freshness explicitly.
- [ ] Make unsaved areas, outstanding live changes and unresolved recovery
  prerequisites easy to spot and inspect from the system overview.
- [ ] Browse saved snapshots and their resource metadata. Select paths/resources
  and compare saved and observed state for a recovery preview.
- [ ] Preview `fmt`, `save` and `apply` with visible scope, changes, conflicts,
  dependencies, exclusions and required permissions.
- [ ] Follow running operations and retain their final results when the view
  changes. Define what browser disconnect/reconnect means for ongoing work.
- [ ] Add configuration viewing and validation, then deliberate editing with
  a reviewable diff. Preserve concrete files, host overlays and the existing
  setting precedence; keep credentials out of browser-visible configuration.
- [ ] Add save publication and fmt/apply execution after preview flows are
  reviewed. Require explicit authorization for writes and additional effects,
  revalidate the plan before execution and preserve the existing privilege model.
- [ ] Make core workflows usable with a keyboard and readable without color;
  support different browser window sizes and long-running operations.

Acceptance: visually review an overview of a synthetic system containing
healthy and problematic resources, saved and unsaved paths, several storage
destinations, nested collections, stale copies and a failed save/replica.
A user can find a lint issue, inspect its subject, see whether and where it was
saved and understand the freshness and recovery limits of that copy.
An end-to-end scenario also covers saving, browsing the resulting snapshot,
selecting recovery scope and restoring onto a disposable target. The CLI and
browser agree on findings, plans, coverage and results. Reloading or reconnecting
cannot accidentally repeat a mutation.

### Architecture decisions before implementation

The [current contract](../ARCHITECTURE.md) allows exactly two workspace packages
and four operations and explicitly excludes a server, RPC protocol and event bus.
This roadmap does not authorize adding a fifth command or changing that contract.
Browser delivery needs a separate architecture discussion and reviewed change.

Decide:

- How the UI is launched, where static assets and any local HTTP adapter live,
  how their lifecycle is managed and how they are packaged for Arch Linux.
  No launch command, web framework or additional package is selected yet.
- How browser requests reach the public library API, with transport outside
  the core and without parsing CLI text or exposing internal layer modules.
- How a local browser session is authenticated, restricted to intended origins
  and granted explicit execution permission. Remote access is a separate scope.
- How authentication and privileged system work are handed off without running
  a long-lived browser-facing process as root or exposing storage credentials.
- How concurrent operations, configuration edits, stale previews and browser
  reconnects interact with repository locks and confirmed execution results.

## 4. Close recovery capability and verification gaps

Keep extending and proving the recovery model alongside interface work. The
[resource implementation follow-ups](resource-implementation.md#follow-up-work)
track package recovery closure, large-data transfers, additional Git layouts,
application adapters and provider conformance. Keep that technical backlog as
the source for those capabilities rather than duplicating its completion status.

- [ ] Prove repeatable complete-system recovery on disposable Arch Linux targets,
  including package/trust bootstrap, identities, storage/boot prerequisites and
  post-restore checks. Record exactly which scenarios and guarantees were proven.
- [ ] Exercise unavailable sources, missing decryption keys, interrupted work and
  partial restoration. Surface unresolved prerequisites in both interfaces.
- [ ] Update recovery documentation and declared guarantees only when supported
  by evidence. Keep real deployment inventories and credentials private.

## Proposed sequence

Start with visual CLI examples and before/after recordings. Design the browser
system overview and its connections between current resources, lint findings and
saved copies, then identify any missing public read-only data. Discuss browser
architecture before selecting its delivery mechanism. Build system exploration
and save-state views before enabling browser-triggered mutations. Recovery
verification can progress independently and remains necessary for complete-system
claims.
