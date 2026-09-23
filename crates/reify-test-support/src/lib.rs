// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

pub mod builders;
pub mod ctor_conformance;
pub mod ctor_conformance_debt;
pub mod fixtures;
pub mod git_env;
pub mod helpers;
pub mod ignore_hygiene;
pub mod kernel_assertions;
pub mod lsp_fixtures;
pub mod mocks;
pub mod orphan_audit;
pub mod specialization_fixtures;
pub mod temp_dirs;
pub mod tolerance_fixtures;
pub mod tracing_support;
pub mod value_decompose;
pub mod values;

pub use builders::*;
pub use ctor_conformance::*;
// Deliberately NOT `pub use ctor_conformance_debt::*;`, for the same reason as
// `git_env` below: `CTOR_CONFORMANCE_MIGRATION_DEBT`, `debt_entry_matches` and
// `param_name_from_ctor_diagnostic` are generic enough names that hoisting them
// into a crate root which many test files glob-import would turn a future
// same-named item in any other glob-exported module into an E0659 ambiguity at
// every such use site. Its readers all spell the module path.
pub use fixtures::*;
// Deliberately NOT `pub use git_env::*;`. `sanitize` and `REPO_REDIRECT_VARS`
// are generic enough names that hoisting them into a crate root which many
// test files glob-import (`use reify_test_support::*;`) would turn a future
// same-named item in any other glob-exported module into an E0659 ambiguity at
// every such use site. Both real consumers already spell the module path —
// `reify_audit::git_env` re-exports from `reify_test_support::git_env`, and
// `orphan_audit` uses `crate::git_env::sanitize` — and a repo-wide grep finds
// no user of the crate-root path, so `pub mod git_env;` above is the whole
// surface.
pub use helpers::*;
pub use lsp_fixtures::*;
pub use mocks::*;
pub use orphan_audit::*;
pub use temp_dirs::*;
pub use tolerance_fixtures::*;
pub use tracing_support::*;
pub use value_decompose::*;
pub use values::*;
