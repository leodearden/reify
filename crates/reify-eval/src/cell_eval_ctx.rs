//! Required-args `cell_eval_ctx` free-function constructor (INV-EVAL-2).
//!
//! PRD: `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.5, §8 — task β of
//! the eval cell-commit substrate.
//!
//! `reify_expr::EvalContext` is an optional-capability builder: omitting
//! `.with_determinacy` / `.with_runtime_diagnostics` / `.with_containment`
//! does not fail to compile — it silently degrades evaluation instead
//! (`DeterminacyPredicate` cells resolve to `Value::Undef`, runtime warnings
//! are dropped, and `sample(restrict(field, region), point)` is forced to
//! `Value::Undef`). This module's `cell_eval_ctx` free function makes those
//! three capabilities REQUIRED parameters (plain `&'a T`, not `Option`), so
//! omitting a load-bearing capability at a call site is a compile error
//! rather than a silent behavior change.
