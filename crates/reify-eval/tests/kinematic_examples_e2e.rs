//! End-to-end integration tests for the kinematic worked examples
//! (`examples/kinematic/counter_mass_balance.ri` and
//! `examples/kinematic/dock_pickup.ri`).
//!
//! Ships two `.ri` files under `examples/kinematic/` and exercises them
//! through the full `parse → compile_with_stdlib → eval` (and, for dock_pickup,
//! `engine.build` with the OCCT kernel) pipeline.
//!
//! Per docs/prds/kinematic-constraints.md task 10.

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_test_support::{collect_errors, parse_and_compile_with_stdlib};

// ── Path constants ────────────────────────────────────────────────────────────

const CMB_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kinematic/counter_mass_balance.ri"
);

// ── Cached source + compile helpers for counter_mass_balance ──────────────────

/// Read `examples/kinematic/counter_mass_balance.ri`, caching the result.
fn cmb_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(CMB_PATH)
            .unwrap_or_else(|e| panic!("{CMB_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile `counter_mass_balance.ri` with stdlib, caching the result.
fn cmb_compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(cmb_source()))
}

// ═══════════════════════════════════════════════════════════════════════════════
// counter_mass_balance tests
// ═══════════════════════════════════════════════════════════════════════════════

/// The `.ri` file exists and compiles with stdlib without any Error-severity
/// diagnostics.
#[test]
fn counter_mass_balance_compiles_clean() {
    // Reading source panics if the file doesn't exist.
    let source = cmb_source();
    assert!(!source.is_empty(), "counter_mass_balance.ri should be non-empty");

    let compiled = parse_and_compile_with_stdlib(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "counter_mass_balance.ri should compile with stdlib without errors, got: {errors:#?}"
    );
}

// Note: the `cmb_compiled()` helper is defined above but not yet used in this
// initial step.  It will be exercised by later eval/check tests added in
// subsequent steps (step-3, step-5).
#[allow(dead_code)]
fn _use_cmb_compiled() -> &'static CompiledModule {
    cmb_compiled()
}
