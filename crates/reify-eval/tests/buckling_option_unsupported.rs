//! RED test: W_BucklingOptionUnsupported diagnostics from `solve_buckling_trampoline` (task 4149).
//!
//! Verifies that the trampoline emits a `W_BucklingOptionUnsupported` Warning for each
//! non-default value of the declared-but-not-yet-honored BucklingOptions params:
//!   - `mode`       default: `"shift_invert"` — any other string triggers the warning
//!   - `sigma`      default: `0.0`             — any non-zero value triggers the warning
//!   - `auto_dense` default: `true`            — `false` triggers the warning
//!
//! # Design
//!
//! Reuses the tiny-geometry harness from `buckling_element_order_dispatch.rs`
//! (lz=1mm, lx=ly=10mm, n_modes=1, empty loads/supports) which runs in debug CI
//! without a release gate (~486 DOFs P1).
//!
//! Each test calls `solve_buckling_trampoline`, destructures `ComputeOutcome::Completed`,
//! and filters `diagnostics` for `d.code == Some(DiagnosticCode::BucklingOptionUnsupported)`.
//!
//! # RED status (step-3)
//!
//! The trampoline returns `diagnostics: vec![]` unconditionally, so assertions
//! (b)–(e) fail. GREEN after step-4 implements `buckling_unsupported_option_diagnostics`.

use reify_core::{DiagnosticCode, DimensionVector, Severity};
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_steel_material() -> Value {
    let fields: PersistentMap<String, Value> = [
        (
            "youngs_modulus".to_string(),
            Value::Scalar {
                si_value: 205.0e9,
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("poisson_ratio".to_string(), Value::Real(0.3)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ElasticMaterial".to_string(),
        version: 1,
        fields,
    }))
}

fn make_scalar_length(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::LENGTH,
    }
}

/// Build a BucklingOptions StructureInstance with the given field overrides.
///
/// Always includes `n_modes: 1`, `tol: 1e-4`, `max_iters: 500`.
/// The `extra_fields` slice can add/override `mode`, `sigma`, `auto_dense` (or
/// `element_order`). Fields are applied in order; later entries win on collision.
fn make_buckling_options(extra_fields: &[(&str, Value)]) -> Value {
    let mut map: Vec<(String, Value)> = vec![
        ("n_modes".to_string(), Value::Int(1)),
        ("tol".to_string(), Value::Real(1e-4)),
        ("max_iters".to_string(), Value::Int(500)),
    ];
    for (name, val) in extra_fields {
        map.push((name.to_string(), val.clone()));
    }
    let fields: PersistentMap<String, Value> = map.into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingOptions".to_string(),
        version: 1,
        fields,
    }))
}

/// Run the trampoline on the tiny geometry (lz=1mm, lx=ly=10mm) with the given
/// BucklingOptions value.  Empty loads list triggers the 1.0 N sentinel default.
fn run_trampoline_with_opts(opts: Value) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;

    let value_inputs = vec![
        make_steel_material(),
        make_scalar_length(0.001), // length (lz) = 1 mm
        make_scalar_length(0.01),  // width  (lx) = 10 mm
        make_scalar_length(0.01),  // height (ly) = 10 mm
        Value::List(vec![]),       // loads  — empty → default 1.0 N sentinel
        Value::List(vec![]),       // supports — pin-pin (empty → trampoline default)
        opts,
    ];

    reify_eval::compute_targets::buckling::solve_buckling_trampoline(
        &value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// Extract the `W_BucklingOptionUnsupported` diagnostics from a completed outcome.
fn extract_unsupported_diags(outcome: ComputeOutcome) -> Vec<reify_core::Diagnostic> {
    let diagnostics = match outcome {
        ComputeOutcome::Completed { diagnostics, .. } => diagnostics,
        other => panic!("expected ComputeOutcome::Completed, got: {:?}", other),
    };
    diagnostics
        .into_iter()
        .filter(|d| d.code == Some(DiagnosticCode::BucklingOptionUnsupported))
        .collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// (a) All-default / absent options → zero W_BucklingOptionUnsupported diagnostics.
///
/// The three params (mode/sigma/auto_dense) are absent from the map; the trampoline
/// must not emit any unsupported-option warning for the happy-path baseline.
#[test]
fn buckling_option_unsupported_defaults_no_diagnostic() {
    let opts = make_buckling_options(&[]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert!(
        diags.is_empty(),
        "expected zero W_BucklingOptionUnsupported for default options, got: {:?}",
        diags
    );
}

/// (b) `mode: "bogus"` (non-default, out-of-allowlist string) → exactly one Warning
/// whose message contains "mode".
#[test]
fn buckling_option_unsupported_mode_bogus_emits_one_warning() {
    let opts = make_buckling_options(&[("mode", Value::String("bogus".to_string()))]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 W_BucklingOptionUnsupported for mode:bogus, got: {:?}",
        diags
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "expected Warning severity, got: {:?}",
        diags[0].severity
    );
    assert!(
        diags[0].message.contains("mode"),
        "expected diagnostic message to contain 'mode', got: {:?}",
        diags[0].message
    );
}

/// (b2) `mode: "dense"` (non-default, IN allowlist but still silently dropped) →
/// exactly one Warning whose message contains "mode".
///
/// This is the honest case: even a valid-sounding mode string has no effect today,
/// so the user deserves to know.
#[test]
fn buckling_option_unsupported_mode_dense_emits_one_warning() {
    let opts = make_buckling_options(&[("mode", Value::String("dense".to_string()))]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 W_BucklingOptionUnsupported for mode:dense, got: {:?}",
        diags
    );
    assert!(
        diags[0].message.contains("mode"),
        "expected message to contain 'mode', got: {:?}",
        diags[0].message
    );
}

/// (c) `sigma: 1.5` (non-zero `Value::Real`) → exactly one Warning whose message
/// contains "sigma".
#[test]
fn buckling_option_unsupported_sigma_nonzero_emits_one_warning() {
    let opts = make_buckling_options(&[("sigma", Value::Real(1.5))]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 W_BucklingOptionUnsupported for sigma:1.5, got: {:?}",
        diags
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("sigma"),
        "expected message to contain 'sigma', got: {:?}",
        diags[0].message
    );
}

/// (c2) `sigma: Value::Int(2)` — integer literal for a `Real` field — → exactly one
/// Warning whose message contains "sigma".
///
/// The DSL declares `sigma` as `Real`, but the eval pipeline may materialise an
/// integer literal like `sigma: 2` as `Value::Int`.  The helper must catch this so
/// a non-default integer sigma cannot silently bypass the warning.
#[test]
fn buckling_option_unsupported_sigma_int_nonzero_emits_one_warning() {
    let opts = make_buckling_options(&[("sigma", Value::Int(2))]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 W_BucklingOptionUnsupported for sigma:Int(2), got: {:?}",
        diags
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("sigma"),
        "expected message to contain 'sigma', got: {:?}",
        diags[0].message
    );
}

/// (d) `auto_dense: false` (non-default Bool) → exactly one Warning whose message
/// contains "auto_dense".
#[test]
fn buckling_option_unsupported_auto_dense_false_emits_one_warning() {
    let opts = make_buckling_options(&[("auto_dense", Value::Bool(false))]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 W_BucklingOptionUnsupported for auto_dense:false, got: {:?}",
        diags
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        diags[0].message.contains("auto_dense"),
        "expected message to contain 'auto_dense', got: {:?}",
        diags[0].message
    );
}

/// (e) All three non-default → exactly three W_BucklingOptionUnsupported Warnings.
///
/// One per unsupported param; no deduplication.
#[test]
fn buckling_option_unsupported_all_three_nondefault_emits_three_warnings() {
    let opts = make_buckling_options(&[
        ("mode", Value::String("dense".to_string())),
        ("sigma", Value::Real(-1.0)),
        ("auto_dense", Value::Bool(false)),
    ]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert_eq!(
        diags.len(),
        3,
        "expected exactly 3 W_BucklingOptionUnsupported (one per non-default param), got: {:?}",
        diags
    );
    for d in &diags {
        assert_eq!(
            d.severity,
            Severity::Warning,
            "all unsupported-option diagnostics must be Warning, got: {:?}",
            d
        );
    }
}

/// (f) Default-valued explicit fields → zero W_BucklingOptionUnsupported diagnostics.
///
/// Supplying the default explicitly must NOT trigger warnings — only non-default
/// values fire the diagnostic.
#[test]
fn buckling_option_unsupported_explicit_defaults_no_diagnostic() {
    let opts = make_buckling_options(&[
        ("mode", Value::String("shift_invert".to_string())),
        ("sigma", Value::Real(0.0)),
        ("auto_dense", Value::Bool(true)),
    ]);
    let diags = extract_unsupported_diags(run_trampoline_with_opts(opts));
    assert!(
        diags.is_empty(),
        "expected zero W_BucklingOptionUnsupported for explicit default values, got: {:?}",
        diags
    );
}
