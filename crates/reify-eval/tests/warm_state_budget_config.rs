//! Integration tests for the warm-state budget config wiring (task #3572 step-5/6).
//!
//! Pins the full Manifest→warm_state_budget_bytes→Engine pipeline:
//! - `[warm_state]\nbudget_bytes = 4096` in a manifest ⇒ engine warm pool has budget 4096.
//! - Empty manifest (no `[warm_state]`) ⇒ engine warm pool budget equals what
//!   `WarmStatePool::from_config_or_env(None)` would produce (env or default),
//!   avoiding hard-coding the DEFAULT_BUDGET_BYTES so an env-var override in CI
//!   doesn't break the test.
//! - `Engine::set_warm_state_budget(Some(4096))` directly sets the pool budget to 4096.
//!
//! Mirrors `crates/reify-runtime/tests/node_overrides_config.rs` pattern.

use reify_constraints::SimpleConstraintChecker;
use reify_config::Manifest;
use reify_eval::Engine;
use reify_eval::warm_pool::{DEFAULT_BUDGET_BYTES, WarmStatePool};

/// Manifest with `[warm_state]\nbudget_bytes = 4096` must wire budget 4096 into
/// the engine's warm-state pool via `with_registered_kernels_and_manifest`.
#[test]
fn manifest_warm_state_budget_wires_into_engine() {
    let manifest = Manifest::from_toml_str("[warm_state]\nbudget_bytes = 4096\n")
        .expect("manifest with [warm_state] must parse");

    let (engine, _diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        Some(&manifest),
    );

    assert_eq!(
        engine.warm_pool().budget_bytes(),
        Some(4096),
        "engine warm_pool budget must be Some(4096) from manifest [warm_state].budget_bytes"
    );
}

/// Empty manifest (no `[warm_state]`) must leave the engine warm pool at the
/// env-var-or-default budget, NOT a hardcoded value.
///
/// Compares against `WarmStatePool::from_config_or_env(None)` to avoid coupling
/// the test to a specific budget when `REIFY_WARM_STATE_BUDGET_BYTES` is set in CI.
#[test]
fn empty_manifest_leaves_warm_pool_at_env_or_default() {
    let manifest = Manifest::from_toml_str("").expect("empty manifest must parse");

    let (engine, _diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        Some(&manifest),
    );

    let expected = WarmStatePool::from_config_or_env(None).budget_bytes();
    assert_eq!(
        engine.warm_pool().budget_bytes(),
        expected,
        "engine warm_pool budget with empty manifest must equal from_config_or_env(None)"
    );
}

/// `Engine::set_warm_state_budget(Some(4096))` must directly update the warm pool budget.
///
/// Mirrors the `set_persistent_cache_dir` post-construction setter pattern: the
/// budget is re-evaluated from env+config each time this setter is called.
#[test]
fn set_warm_state_budget_updates_pool_budget() {
    let (mut engine, _diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        None,
    );

    engine.set_warm_state_budget(Some(4096));

    assert_eq!(
        engine.warm_pool().budget_bytes(),
        Some(4096),
        "set_warm_state_budget(Some(4096)) must set warm_pool budget to Some(4096)"
    );
}

/// `Engine::set_warm_state_budget(None)` must use the env-var or default budget.
#[test]
fn set_warm_state_budget_none_uses_env_or_default() {
    let (mut engine, _diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        None,
    );
    // Force a non-default budget first so we can see the None case restore it.
    engine.set_warm_state_budget(Some(999_999));
    assert_eq!(engine.warm_pool().budget_bytes(), Some(999_999));

    // Now reset to None → must fall back to from_config_or_env(None).
    engine.set_warm_state_budget(None);
    let expected = WarmStatePool::from_config_or_env(None).budget_bytes();
    assert_eq!(
        engine.warm_pool().budget_bytes(),
        expected,
        "set_warm_state_budget(None) must restore to env/default budget"
    );
}
