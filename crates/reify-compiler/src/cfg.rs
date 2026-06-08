//! Cfg predicate evaluator for conditional-compilation (`#cfg(...)`) pragmas.
//!
//! This module provides two public items:
//! - [`CfgSet`] — the active compile-time configuration (flags, target, kv pairs).
//! - [`cfg_satisfied`] — a pure predicate: given a `#cfg(...)` pragma and an
//!   active `CfgSet`, returns `true` iff all args of the pragma are satisfied.

use std::collections::{BTreeMap, BTreeSet};

/// The active compile-time configuration used to evaluate `#cfg(...)` pragmas.
///
/// All three fields are optional/empty by default (see [`Default`]).
/// - `target`: the platform target string (e.g. `"linux"`, `"wasm"`). Populated
///   by `--cfg target=<value>` in the driver (Task δ).
/// - `flags`: boolean feature flags (e.g. `--cfg linux`). A flag is either
///   present or absent.
/// - `kv`: arbitrary key→value pairs for non-`target` keys (e.g. `feature="x"`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CfgSet {
    pub target: Option<String>,
    pub flags: BTreeSet<String>,
    pub kv: BTreeMap<String, String>,
}

/// Returns `true` iff every arg in `pragma` is satisfied under `active`.
///
/// **Contract:**
/// - Pure function — no diagnostics emitted, no side effects.
/// - Name-agnostic — caller (Task γ) guarantees `pragma.name == "cfg"`.
/// - Multiple args are ANDed: all args must be satisfied.
/// - An empty arg list is vacuously `true` (`[].iter().all(...)` is true by
///   definition). Note: `#cfg()` is diagnosed by Task α; β is indifferent.
///
/// **Satisfiable arg shapes** (PRD §4 D-1):
/// - `Bare(Ident(flag))` — true iff `flag ∈ active.flags`.
/// - `KeyValue { key: "target", value: String(v) }` — true iff
///   `active.target == Some(v)`. Reads **only** `CfgSet.target`.
/// - `KeyValue { key, value: String(v) }` (key ≠ "target") — true iff
///   `active.kv.get(key) == Some(v)`. Reads **only** `CfgSet.kv`.
///
/// All other shapes are unsatisfiable (term evaluates to `false`) in v1.
pub fn cfg_satisfied(pragma: &reify_ast::Pragma, active: &CfgSet) -> bool {
    let _ = (pragma, active);
    false
}

/// Evaluates a single pragma arg against `active`.
///
/// **Satisfiable shapes** (v1):
/// - `Bare(Ident(flag))` → `active.flags.contains(flag)`
/// - `KeyValue { key: "target", value: String(v) }` → `active.target == Some(v)`
/// - `KeyValue { key, value: String(v) }` (key ≠ "target") → `active.kv[key] == v`
///
/// **Unsatisfiable** (term = `false`) for all other shapes:
/// KeyValue with Number/Bool/Quantity/Ident value; Bare with Number/String/Bool/Quantity value.
fn arg_satisfied(_arg: &reify_ast::PragmaArg, _active: &CfgSet) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ast::{Pragma, PragmaArg, PragmaValue};
    use reify_core::SourceSpan;

    /// Build a `#cfg(args...)` pragma for tests.
    fn cfg_pragma(args: Vec<PragmaArg>) -> Pragma {
        Pragma {
            name: "cfg".into(),
            args,
            span: SourceSpan::empty(0),
        }
    }
}
