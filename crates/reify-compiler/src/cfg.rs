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

impl CfgSet {
    /// The host-default active configuration: `target` is the compiling host's
    /// platform string (`std::env::consts::OS` — e.g. `"linux"`, `"macos"`,
    /// `"windows"`), with empty `flags` and `kv`.
    ///
    /// Per PRD §4 D-2, when no `--cfg target=` override is supplied the active
    /// target defaults to the host platform so `#cfg(target = "...")` import
    /// gating still fires. Shared by the `reify check` CLI driver (Task δ) and
    /// the GUI dirty-buffer compile path (which has no cfg selector in v1).
    pub fn host_default() -> Self {
        CfgSet {
            target: Some(std::env::consts::OS.to_string()),
            ..Default::default()
        }
    }
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
        PragmaArg::KeyValue {
            key,
            value: PragmaValue::String(v),
        } if key == "target" => active.target.as_deref() == Some(v.as_str()),
        PragmaArg::KeyValue {
            key,
            value: PragmaValue::String(v),
        } => active.kv.get(key).map(String::as_str) == Some(v.as_str()),
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

    // -------------------------------------------------------------------------
    // Step 5: general (non-target) KeyValue → CfgSet.kv, strict separation
    // -------------------------------------------------------------------------

    fn kv_set(pairs: &[(&str, &str)]) -> CfgSet {
        CfgSet {
            kv: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn feature_kv_present_is_true() {
        let p = cfg_pragma(vec![kv_string("feature", "x")]);
        assert!(cfg_satisfied(&p, &kv_set(&[("feature", "x")])));
    }

    #[test]
    fn feature_kv_wrong_value() {
        let p = cfg_pragma(vec![kv_string("feature", "x")]);
        assert!(!cfg_satisfied(&p, &kv_set(&[("feature", "y")])));
    }

    #[test]
    fn feature_kv_absent_key() {
        let p = cfg_pragma(vec![kv_string("feature", "x")]);
        assert!(!cfg_satisfied(&p, &kv_set(&[])));
    }

    #[test]
    fn target_key_does_not_read_kv() {
        // target="linux" with CfgSet{target:None, kv:{target:"linux"}} → false
        // The target arg must NOT fall through to kv lookup.
        let p = cfg_pragma(vec![kv_string("target", "linux")]);
        let active = CfgSet {
            target: None,
            kv: [("target".into(), "linux".into())].into_iter().collect(),
            ..Default::default()
        };
        assert!(!cfg_satisfied(&p, &active));
    }

    #[test]
    fn non_target_key_does_not_read_target() {
        // feature="x" with CfgSet{target:Some("x"), kv:{}} → false
        // A non-target key must NOT read active.target.
        let p = cfg_pragma(vec![kv_string("feature", "x")]);
        let active = CfgSet {
            target: Some("x".into()),
            ..Default::default()
        };
        assert!(!cfg_satisfied(&p, &active));
    }

    #[test]
    fn empty_string_value_matches_empty_string_in_kv() {
        // feature="" against kv:{feature:""} → true (empty string is a valid match)
        let p = cfg_pragma(vec![kv_string("feature", "")]);
        assert!(cfg_satisfied(&p, &kv_set(&[("feature", "")])));
    }

    #[test]
    fn empty_string_value_absent_key_is_false() {
        // feature="" with kv:{} (absent key) → false (absent ≠ empty string)
        let p = cfg_pragma(vec![kv_string("feature", "")]);
        assert!(!cfg_satisfied(&p, &kv_set(&[])));
    }

    #[test]
    fn superset_active_does_not_cause_false_positive() {
        // Extra unrelated flags/kv entries in active set must not cause a false positive.
        // #cfg(linux) should be satisfied when flags⊃{linux, extra_flag} and kv has extras too.
        let p = cfg_pragma(vec![bare_ident("linux")]);
        let active = CfgSet {
            flags: ["linux".into(), "extra_flag".into()].into_iter().collect(),
            kv: [("unrelated".into(), "value".into())].into_iter().collect(),
            target: Some("wasm".into()),
        };
        assert!(cfg_satisfied(&p, &active));
    }

    // -------------------------------------------------------------------------
    // Step 7: degenerate/unsatisfiable shapes → false; mixed AND sanity
    // -------------------------------------------------------------------------

    #[test]
    fn kv_number_value_is_false() {
        // target = 42 (KeyValue with Number value) → unsatisfiable
        let p = cfg_pragma(vec![PragmaArg::KeyValue {
            key: "target".into(),
            value: PragmaValue::Number(42.0),
        }]);
        let active = CfgSet {
            target: Some("42".into()),
            ..Default::default()
        };
        assert!(!cfg_satisfied(&p, &active));
    }

    #[test]
    fn kv_bool_value_is_false() {
        // feature = true (KeyValue with Bool value) → unsatisfiable
        let p = cfg_pragma(vec![PragmaArg::KeyValue {
            key: "feature".into(),
            value: PragmaValue::Bool(true),
        }]);
        let active = kv_set(&[("feature", "true")]);
        assert!(!cfg_satisfied(&p, &active));
    }

    #[test]
    fn kv_quantity_value_is_false() {
        // k = 1m (KeyValue with Quantity value) → unsatisfiable
        let p = cfg_pragma(vec![PragmaArg::KeyValue {
            key: "k".into(),
            value: PragmaValue::Quantity {
                value: 1.0,
                unit: "m".into(),
            },
        }]);
        assert!(!cfg_satisfied(&p, &CfgSet::default()));
    }

    #[test]
    fn kv_ident_value_is_false() {
        // k = ident (KeyValue with Ident value) → unsatisfiable
        let p = cfg_pragma(vec![PragmaArg::KeyValue {
            key: "k".into(),
            value: PragmaValue::Ident("ident".into()),
        }]);
        assert!(!cfg_satisfied(&p, &CfgSet::default()));
    }

    #[test]
    fn bare_number_is_false() {
        let p = cfg_pragma(vec![PragmaArg::Bare(PragmaValue::Number(1.0))]);
        assert!(!cfg_satisfied(&p, &CfgSet::default()));
    }

    #[test]
    fn bare_string_is_false() {
        let p = cfg_pragma(vec![PragmaArg::Bare(PragmaValue::String("linux".into()))]);
        assert!(!cfg_satisfied(&p, &flags(&["linux"])));
    }

    #[test]
    fn bare_bool_is_false() {
        let p = cfg_pragma(vec![PragmaArg::Bare(PragmaValue::Bool(true))]);
        assert!(!cfg_satisfied(&p, &CfgSet::default()));
    }

    #[test]
    fn bare_quantity_is_false() {
        let p = cfg_pragma(vec![PragmaArg::Bare(PragmaValue::Quantity {
            value: 1.0,
            unit: "m".into(),
        })]);
        assert!(!cfg_satisfied(&p, &CfgSet::default()));
    }

    #[test]
    fn mixed_and_both_satisfied() {
        // #cfg(linux, target = "wasm") with flags:{linux}, target:Some("wasm") → true
        let p = cfg_pragma(vec![bare_ident("linux"), kv_string("target", "wasm")]);
        let active = CfgSet {
            flags: ["linux".into()].into_iter().collect(),
            target: Some("wasm".into()),
            ..Default::default()
        };
        assert!(cfg_satisfied(&p, &active));
    }

    #[test]
    fn mixed_and_flag_missing() {
        // #cfg(linux, target = "wasm") with flags:{}, target:Some("wasm") → false
        let p = cfg_pragma(vec![bare_ident("linux"), kv_string("target", "wasm")]);
        let active = CfgSet {
            target: Some("wasm".into()),
            ..Default::default()
        };
        assert!(!cfg_satisfied(&p, &active));
    }

    #[test]
    fn mixed_and_target_wrong() {
        // #cfg(linux, target = "wasm") with flags:{linux}, target:Some("linux") → false
        let p = cfg_pragma(vec![bare_ident("linux"), kv_string("target", "wasm")]);
        let active = CfgSet {
            flags: ["linux".into()].into_iter().collect(),
            target: Some("linux".into()),
            ..Default::default()
        };
        assert!(!cfg_satisfied(&p, &active));
    }

    // -------------------------------------------------------------------------
    // Task δ: CfgSet::host_default() — target defaults to the compiling host
    // platform (PRD §4 D-2); flags and kv start empty.
    // -------------------------------------------------------------------------

    #[test]
    fn host_default_target_is_host_os() {
        let cfg = CfgSet::host_default();
        assert_eq!(cfg.target, Some(std::env::consts::OS.to_string()));
        assert!(cfg.flags.is_empty(), "flags should start empty");
        assert!(cfg.kv.is_empty(), "kv should start empty");
    }
}
