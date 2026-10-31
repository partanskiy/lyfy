//! Arch Linux maintenance logic, independently reusable as a Rust library.
//!
//! [`Config`] contains resolved operation parameters, not an application config
//! loader. Inputs own file discovery, parsing, ENV, defaults and presentation.
//! [`Context`] owns validated parameters and the repository session.
//!
//! ```no_run
//! use lyfy::{Config, Context, Request, execute};
//!
//! fn lint(options: Config) -> anyhow::Result<u8> {
//!     let context = Context::new(options)?;
//!     Ok(execute(&context, Request::Lint)?.exit_code)
//! }
//! ```
//! Internal operations are not public entry points:
//!
//! ```compile_fail
//! use lyfy::application::save::run;
//! ```
//!
//! Domain layout is internal; use the explicit root types such as [`Config`]:
//!
//! ```compile_fail
//! use lyfy::domain::config::Config;
//! ```
//!
//! API implementation paths are private; use [`execute`] at the crate root:
//!
//! ```compile_fail
//! use lyfy::api::execute;
//! ```
mod api;
mod application;
mod domain;
mod infrastructure;
mod services;

pub use api::{Context, Data, Outcome, Request, execute, execute_observed};
pub use domain::checks::{CheckId, CheckResult, CheckStatus};
pub use domain::config::Config;
pub use domain::diagnostics::{Diagnostic, DiagnosticCode, Finding, Report};
pub use domain::observation::Observation;
pub use domain::package_files::{FileDifference, PackageEvidence};
pub use domain::packages::{Package, PackagePlan};
pub use domain::plan::{Change, Guard, Plan};
pub use domain::results::{
    Applied, CaptureDiagnostic, Formatted, SaveChangeKind, Saved, SavedChange,
};
pub use domain::rules::{Category, FixAction, FixKind, RuleId, RuleSpec, Severity, SubjectKind};
pub use domain::state::{Content, Node, Tree};
