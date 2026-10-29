# Lint presentation and report files

The library returns inspection data and synchronous events. Only the CLI detects
terminals, renders progress and encodes/writes report files. Library users can
observe checks directly without selecting a report format.

```sh
lyfy lint
lyfy lint --format json
lyfy lint --format toml --output_file laptop.toml
lyfy lint --format yaml --output_file laptop.yaml
lyfy lint --format markdown --output_file laptop.md
lyfy lint --format sarif --output_file laptop.sarif
lyfy lint --format text --output_file laptop.txt
```

The format is explicit, independent of the filename extension. Without output_file,
the report goes to stdout. The value `-` selects stdout, overriding a configured
file. output_file is a local physical path, independent of the inspected root.
Its parent must exist. Files are created privately with mode 0600 and published
atomically without replacing any existing file, directory or symlink. Choose a
new filename to preserve older reports. With a file destination stdout is empty;
stderr confirms the saved path and scan exit code.

The elevated worker always returns JSON through a caller-owned temporary
channel. The original process renders and publishes the selected format, so
automatic sudo does not create root-owned report files. No extra command is
introduced for this internal re-execution. Calling `sudo lyfy` directly changes
the invoking user and therefore the owner in the usual way.

## Progress and human output

`progress = "auto"` uses an updating terminal display when stderr is a terminal
and TERM is set to a value other than dumb. Each stage has a spinner and elapsed
time. Package inspection switches to a bar with actual completed/total package
counts. The display does not invent an overall percentage for unknown work.
Only ASCII spinner/bar characters are used. NO_COLOR disables colors.

Redirected stderr and TERM=dumb use plain stage start/completion messages and
periodic package counts. There is no repeating "still checking" message.
`--progress plain` explicitly selects that form; `--progress never` hides scan
feedback. Progress never enters stdout or report files. After scanning, the
progress display clears and the human report groups findings by severity,
explaining their category, observed state, subjects, action and fix availability.
Coverage diagnostics and deliberately skipped checks have separate sections.

## Machine formats

JSON, TOML and YAML share report schema 2, including tool/version, rule catalog,
summary, target/host, effective policy, check outcomes, findings and coverage
diagnostics. Rule metadata includes stable ID, revision, severity, category,
subject kind, explanation, remedy and supported fix kind. Package evidence names
the package, path, changed fields and backup-configuration status. Markdown and
text represent the same findings, evidence and coverage for readers.

SARIF uses version 2.1.0. The driver includes the catalog; results reference rule
IDs/indices and use error/warning/note levels. Files have percent-encoded absolute
artifact URIs; packages, unit names and other subjects use logical locations.
No source line is invented for a whole-file/system observation. Stable
fingerprints include host, rule and subjects. Coverage diagnostics appear under
invocation.toolExecutionNotifications; executionSuccessful is false for an
incomplete scan. Ordinary findings do not mean the scanner failed to execute.

SARIF is an export format, not automatic upload. GitHub Code Scanning can only
show appropriate repository file locations; arbitrary /etc paths and system
subjects are not repository source lines. Valid system SARIF is still useful to
other consumers. See [GitHub SARIF support](https://docs.github.com/en/code-security/reference/code-scanning/sarif-files/sarif-support).

## Policy and exit codes

```toml
fail_on = "warning"
disabled_rules = ["arch.orphans"]
format = "text"
progress = "auto"
# output_file = "laptop.txt"
```

Each setting has its snake_case flag and LYFY_UPPER_CASE environment variable,
with flag > ENV > TOML > default precedence. disabled_rules replaces the whole
list; IDs are validated. An empty flag/ENV value clears the disabled list. fail_on accepts info, warning or error and defaults to
warning. It changes failure policy without hiding lower-severity findings.
Disabling all rules in a check skips that check explicitly.

- 0: complete scan with no outstanding finding at/above fail_on.
- 1: complete scan with findings at/above fail_on.
- 2: incomplete coverage or an operational/configuration/export failure.

An incomplete report is still exported when scanning can continue. Deliberate
configuration skips do not cause exit 2. Info findings such as orphan packages
are visible without failing the default policy. [Rules](rules.md) documents
implementation, limitations and migration from schema 1.
