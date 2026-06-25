//! Integration tests for the `[warm_state]` table in `reify.toml`.
//!
//! Pins the public contract for task #3572:
//!
//! - A `budget_bytes = N` override (N > 0) round-trips through serde and is
//!   returned by `Manifest::warm_state_budget_bytes()`.
//! - An absent `[warm_state]` section returns `None`.
//! - A declared-but-empty `[warm_state]` section returns `None`.
//! - `budget_bytes = 0` is rejected at parse time with a typed
//!   `ManifestError::InvalidWarmStateBudget(0)` (a zero-byte budget is meaningless).
//! - Unknown keys inside `[warm_state]` surface as `ManifestError::Parse(_)`
//!   (strict schema; mirrors the `[auto_type_params]` convention).
//!
//! Mirror of `crates/reify-config/tests/auto_type_params_config.rs`.

use reify_config::{Manifest, ManifestError};

/// A `[warm_state]` table with `budget_bytes = 4096` overrides the default
/// of None. Pins the serde wiring (`WarmStateRaw` is read into `WarmStateConfig`)
/// and that the parsed value flows through to the public accessor.
#[test]
fn budget_bytes_round_trips() {
    let manifest = Manifest::from_toml_str("[warm_state]\nbudget_bytes = 4096\n")
        .expect("manifest with warm_state section must parse");
    assert_eq!(
        manifest.warm_state_budget_bytes(),
        Some(4096),
        "parsed budget_bytes must be Some(4096)"
    );
}

/// When a manifest has no `[warm_state]` section, `warm_state_budget_bytes()` returns
/// `None` — the engine falls back to the env-var or default.
#[test]
fn absent_section_returns_none() {
    let manifest = Manifest::from_toml_str("").expect("empty manifest must parse to defaults");
    assert_eq!(
        manifest.warm_state_budget_bytes(),
        None,
        "absent [warm_state] section must return None (fall through to env/default)"
    );
}

/// A declared-but-empty `[warm_state]` section (no keys set) must also return `None`
/// from `warm_state_budget_bytes()` — `budget_bytes` is optional with no default value.
#[test]
fn empty_section_returns_none() {
    let manifest = Manifest::from_toml_str("[warm_state]\n")
        .expect("empty [warm_state] section must parse (all fields are optional)");
    assert_eq!(
        manifest.warm_state_budget_bytes(),
        None,
        "empty [warm_state] section must return None (budget_bytes is optional)"
    );
}

/// `budget_bytes = 0` is semantically meaningless: a zero-byte budget would
/// prevent any state from being cached. Pin the typed
/// `ManifestError::InvalidWarmStateBudget(0)` rejection at parse time so
/// misconfiguration cannot ship as a silent no-op.
#[test]
fn zero_budget_bytes_rejected_with_typed_error() {
    let err = Manifest::from_toml_str("[warm_state]\nbudget_bytes = 0\n")
        .expect_err("budget_bytes = 0 must be rejected");
    let rendered = format!("{}", err);
    match err {
        ManifestError::InvalidWarmStateBudget(value) => {
            assert_eq!(value, 0, "value must carry the offending value");
            assert!(
                rendered.contains("warm_state.budget_bytes must be > 0"),
                "Display must contain diagnostic substring; got {:?}",
                rendered
            );
        }
        other => panic!(
            "expected ManifestError::InvalidWarmStateBudget(0), got {:?}",
            other
        ),
    }
}

/// Unknown keys inside `[warm_state]` are surfaced as `ManifestError::Parse(_)`
/// rather than silently dropped. Pins the strict-schema convention shared with
/// `[auto_type_params]` and `[kernels.<id>]`.
#[test]
fn unknown_field_in_warm_state_rejected() {
    let err = Manifest::from_toml_str("[warm_state]\nbudgett = 1\n")
        .expect_err("unknown field in warm_state must be rejected");
    match err {
        ManifestError::Parse(_) => {}
        other => panic!("expected ManifestError::Parse(_), got {:?}", other),
    }
}
