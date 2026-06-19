//! End-to-end integration test for the task 3599/ι thin-walled bracket example.
//!
//! `examples/shells/thin_walled_bracket.ri` (a 100 mm × 20 mm × 2 mm steel
//! cantilever plate, bare `ElasticOptions()` → shell_force default `Auto`,
//! thickness/extent = 2/100 = 0.02 < shell_threshold 0.2 → auto-classified SHELL)
//! is evaluated end-to-end through the `@optimized("solver::elastic_static")`
//! lowering.  On the shell route the engine inserts an upstream
//! `shell-extract::extract` ComputeNode feeding the `solver::elastic_static` FEA
//! trampoline, which routes assembly through the MITC3 shell kernel and populates
//! `result.shell_channels` (a `ShellStress` StructureInstance) and
//! `result.max_von_mises` (a `Pressure` scalar).
//!
//! Analytical reference: σ = 6·P·L / (b·h²) = 6·20·0.1 / (0.02·0.002²) = 1.5×10⁸ Pa.
//! Test band: [1.5e7, 1.5e9] Pa (one order of magnitude; bare-MITC3 honest band).
//!
//! PRD: docs/prds/v0_4/shell-extract-engine-bridge.md task ι (§9 Phase 7).

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;

// ── helpers ────────────────────────────────────────────────────────────────────

fn bracket_source() -> &'static str {
    include_str!("../../../examples/shells/thin_walled_bracket.ri")
}

fn bracket_engine() -> reify_eval::Engine {
    let mut engine = reify_test_support::make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);
    engine
}

fn struct_field(val: &Value, key: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&key.to_string()).cloned(),
        _ => None,
    }
}

fn scalar_si(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Value::Scalar, got {other:?}"),
    }
}

// ── test ───────────────────────────────────────────────────────────────────────

/// The thin-walled bracket fixture solves end-to-end and surfaces a `ShellStress`
/// shell_channels with `max_von_mises` within the one-OOM band around the
/// analytical Euler–Bernoulli reference σ = 1.5×10⁸ Pa.
#[test]
fn thin_walled_bracket_surfaces_shell_stress_with_in_band_max_von_mises() {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(bracket_source());
    let mut engine = bracket_engine();
    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) result.shell_channels is a ShellStress StructureInstance (not Undef).
    let result_cell = ValueCellId::new("ThinWalledBracket", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell ThinWalledBracket.result not found in eval result"));

    let shell_channels = struct_field(result_val, "shell_channels")
        .expect("ElasticResult must carry a shell_channels field");
    match &shell_channels {
        Value::StructureInstance(d) => assert_eq!(
            d.type_name.as_str(),
            "ShellStress",
            "shell_channels must be a ShellStress instance on the shell route"
        ),
        other => {
            panic!("result.shell_channels must be a ShellStress StructureInstance, got {other:?}")
        }
    }

    // (c) max_von_mises cell is a finite Pressure scalar within [1.5e7, 1.5e9] Pa.
    let mvm_cell = ValueCellId::new("ThinWalledBracket", "max_von_mises");
    let mvm_val = eval_result
        .values
        .get(&mvm_cell)
        .unwrap_or_else(|| panic!("cell ThinWalledBracket.max_von_mises not found"));

    let mvm = scalar_si(mvm_val);
    assert!(
        mvm.is_finite() && mvm > 0.0,
        "max_von_mises must be finite and > 0, got {mvm}"
    );
    assert!(
        (1.5e7..=1.5e9).contains(&mvm),
        "max_von_mises {mvm:.4e} Pa outside one-OOM band [1.5e7, 1.5e9] Pa \
         around σ=6·P·L/(b·h²)=1.5e8"
    );
}
