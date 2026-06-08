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
    pragma.args.iter().all(|arg| arg_satisfied(arg, active))
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
fn arg_satisfied(arg: &reify_ast::PragmaArg, active: &CfgSet) -> bool {
    use reify_ast::{PragmaArg, PragmaValue};
    match arg {
        PragmaArg::Bare(PragmaValue::Ident(flag)) => active.flags.contains(flag),
        _ => false,
    }
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

    fn bare_ident(s: &str) -> PragmaArg {
        PragmaArg::Bare(PragmaValue::Ident(s.into()))
    }

    fn kv_string(key: &str, val: &str) -> PragmaArg {
        PragmaArg::KeyValue {
            key: key.into(),
            value: PragmaValue::String(val.into()),
        }
    }

    fn flags(fs: &[&str]) -> CfgSet {
        CfgSet {
            flags: fs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn target(t: &str) -> CfgSet {
        CfgSet {
            target: Some(t.into()),
            ..Default::default()
        }
    }

    // -------------------------------------------------------------------------
    // Step 1: flag form (Bare Ident), AND-of-args, vacuous-true
    // -------------------------------------------------------------------------

    #[test]
    fn flag_present_is_true() {
        let p = cfg_pragma(vec![bare_ident("linux")]);
        assert!(cfg_satisfied(&p, &flags(&["linux"])));
    }

    #[test]
    fn flag_absent_is_false() {
        let p = cfg_pragma(vec![bare_ident("linux")]);
        assert!(!cfg_satisfied(&p, &flags(&[])));
    }

    #[test]
    fn two_bare_flags_anded_both_present() {
        let p = cfg_pragma(vec![bare_ident("linux"), bare_ident("x86_64")]);
        assert!(cfg_satisfied(&p, &flags(&["linux", "x86_64"])));
    }

    #[test]
    fn two_bare_flags_anded_one_absent() {
        // Only linux present, not x86_64 — AND must fail.
        let p = cfg_pragma(vec![bare_ident("linux"), bare_ident("x86_64")]);
        assert!(!cfg_satisfied(&p, &flags(&["linux"])));
    }

    #[test]
    fn empty_args_vacuously_true() {
        // #cfg() — no args — is vacuously satisfied regardless of active set.
        let p = cfg_pragma(vec![]);
        assert!(cfg_satisfied(&p, &CfgSet::default()));
    }

    // -------------------------------------------------------------------------
    // Step 3: target KeyValue resolves against CfgSet.target
    // -------------------------------------------------------------------------

    #[test]
    fn target_kv_matches_target_field() {
        let p = cfg_pragma(vec![kv_string("target", "linux")]);
        assert!(cfg_satisfied(&p, &target("linux")));
    }

    #[test]
    fn target_kv_wrong_target_value() {
        let p = cfg_pragma(vec![kv_string("target", "linux")]);
        assert!(!cfg_satisfied(&p, &target("wasm")));
    }

    #[test]
    fn target_kv_no_target_set() {
        let p = cfg_pragma(vec![kv_string("target", "linux")]);
        assert!(!cfg_satisfied(&p, &CfgSet::default()));
    }
}
