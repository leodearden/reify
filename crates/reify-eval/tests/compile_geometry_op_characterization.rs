//! Characterization / golden harness for `compile_geometry_op` (task #4673,
//! PRD `docs/prds/geometry-op-dispatch-registry.md` DD-4 / §9 L4).
//!
//! # Contract: byte-identical equivalence oracle for L5
//!
//! This suite snapshots the EXACT `Result<reify_ir::GeometryOp, String>` plus the
//! emitted `reify_core::Diagnostic`s produced by the CURRENT, unrefactored
//! `compile_geometry_op` for every [`reify_compiler::CompiledGeometryOp`] variant
//! × nested kind. The captured goldens are the equivalence oracle that gates the
//! highest-risk leaf L5 (the Axis-3 behavioral refactor of that function): any
//! behavioral drift introduced by L5 fails a golden compare here with a clear,
//! paste-ready diff. L5 MUST keep every golden in this file byte-identical green.
//!
//! # Coverage mechanism: compile-time-exhaustive `match`, NOT runtime iteration
//!
//! Per-kind coverage is enforced structurally: each `*_case(kind)` builder and
//! each `*_golden(kind)` lookup is an EXHAUSTIVE `match` with **no `_` arm**, so
//! adding a new variant to any kind enum in `reify-compiler` is a COMPILE error
//! (E0004) here until a golden case is added — a strictly stronger guarantee than
//! `strum::EnumIter` runtime iteration, and it touches zero `reify-compiler` src.
//! Each family additionally carries an `ALL_*` array + a count assertion (Modify
//! cross-checks against `reify_compiler::ModifyKind::VARIANT_COUNT`).
//!
//! # Reaching the function under test
//!
//! `compile_geometry_op` is `pub(crate)` inside the private `mod geometry_ops;`,
//! so it is reached via the cfg-gated 1:1 delegate
//! [`reify_eval::geometry_op_characterization_probe::compile_geometry_op_probe`],
//! activated by the existing self-dev-dep
//! `reify-eval = { path = ".", features = ["test-instrumentation"] }`.
//!
//! # Snapshot determinism
//!
//! Inputs are synthetic literals built via [`lit`]; `CompiledExpr::literal`
//! attaches no span, so the `{:#?}` Debug of the produced `GeometryOp`/`Err`
//! string and the `(severity, message)` diagnostic projection are byte-stable
//! across runs. Goldens were captured via a RED→GREEN bootstrap (placeholder →
//! run on current code → paste actual).

use std::collections::HashMap;

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_core::Diagnostic;
use reify_ir::{CompiledExpr, GeometryHandleId, GeometryOp, Value, ValueMap};

use reify_eval::geometry_op_characterization_probe::compile_geometry_op_probe;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build a `CompiledExpr` literal from a constant f64 (dimensionless scalar).
///
/// Mirrors the in-module `literal_f64` helper at `geometry_ops.rs` so the
/// characterization inputs match the production unit tests' representative args.
fn lit(v: f64) -> CompiledExpr {
    CompiledExpr::literal(Value::Real(v), reify_core::Type::dimensionless_scalar())
}

/// Deterministic snapshot of a `compile_geometry_op` outcome.
///
/// `{:#?}` of the `Ok(GeometryOp)`/`Err(String)` result, followed by one
/// `[diag] <Severity> <message>` line per emitted diagnostic. The diagnostics
/// are projected to `(severity, message)` — the byte-stable, user-facing content
/// — to avoid any brittleness from `DiagnosticLabel` span formatting (the
/// in-module unit tests assert against `diag.severity` + `diag.message` for the
/// same reason).
fn snapshot(res: &Result<GeometryOp, String>, diags: &[Diagnostic]) -> String {
    let mut s = format!("{res:#?}");
    for d in diags {
        s.push_str(&format!("\n[diag] {} {:?}", d.severity.as_wire_str(), d.message));
    }
    s
}

/// Drive the probe against `op` with the given `step_handles` and return the
/// deterministic snapshot string. `values`, `functions`, `meta_map`, and
/// `named_steps` are empty — the synthetic cases need none of them, matching the
/// in-module unit-test call shape.
fn run(op: &CompiledGeometryOp, step_handles: &[GeometryHandleId]) -> String {
    let values = ValueMap::new();
    let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let result = compile_geometry_op_probe(
        op,
        &values,
        step_handles,
        &[],
        &meta_map,
        &named_steps,
        &mut diagnostics,
    );
    snapshot(&result, &diagnostics)
}

/// Compare the probe's snapshot for `op` against `golden`.
///
/// Returns `None` on a byte-identical match, or `Some(<paste-ready block>)` on
/// drift. The block is delimited so a RED→GREEN golden bootstrap can copy the
/// `actual` verbatim into the corresponding `*_golden` arm.
///
/// As an inspection aid (NOT a bypass of the gate), when `REIFY_CHAR_DUMP_DIR`
/// is set the captured `actual` is also written to `<dir>/<label>.snap`. This is
/// purely a side-channel for diffing/blessing during a deliberate bootstrap; the
/// golden compare below is unaffected, so any behavioral drift still fails the
/// test. Inert when the env var is unset (the steady-state CI path).
#[must_use]
fn characterize(
    label: &str,
    op: &CompiledGeometryOp,
    step_handles: &[GeometryHandleId],
    golden: &str,
) -> Option<String> {
    let actual = run(op, step_handles);
    if let Ok(dir) = std::env::var("REIFY_CHAR_DUMP_DIR") {
        let safe: String = label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(format!("{dir}/{safe}.snap"), &actual);
    }
    if actual == golden {
        None
    } else {
        Some(format!(">>>BEGIN {label}>>>\n{actual}\n<<<END {label}<<<\n"))
    }
}

/// Join any collected drift blocks into one paste-ready panic payload (empty
/// string when every case matched its golden).
fn drift_report(blocks: &[String]) -> String {
    if blocks.is_empty() {
        String::new()
    } else {
        format!(
            "\n=== CHARACTERIZATION DRIFT — paste each block into its *_golden arm ===\n\n{}",
            blocks.join("\n")
        )
    }
}

// ---------------------------------------------------------------------------
// Primitive family (7 kinds): Box/Cylinder/Sphere/Tube/Cone/Wedge/Torus
// ---------------------------------------------------------------------------

/// Every `PrimitiveKind` variant. Count-asserted below; the exhaustive matches
/// in `primitive_case`/`primitive_golden` are the per-kind compile-time tripwire.
const ALL_PRIMITIVE: [PrimitiveKind; 7] = [
    PrimitiveKind::Box,
    PrimitiveKind::Cylinder,
    PrimitiveKind::Sphere,
    PrimitiveKind::Tube,
    PrimitiveKind::Cone,
    PrimitiveKind::Wedge,
    PrimitiveKind::Torus,
];

/// Build a representative `Primitive` op for `k`, supplying each arm's required
/// `eval_arg(...)` named args (see `geometry_ops.rs` Primitive arm). EXHAUSTIVE
/// match (no `_`): a new `PrimitiveKind` is a compile error until a case exists.
fn primitive_case(k: PrimitiveKind) -> CompiledGeometryOp {
    let args = match k {
        PrimitiveKind::Box => vec![
            ("width".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.02)),
            ("depth".to_string(), lit(0.03)),
        ],
        PrimitiveKind::Cylinder => vec![
            ("radius".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.02)),
        ],
        PrimitiveKind::Sphere => vec![("radius".to_string(), lit(0.01))],
        PrimitiveKind::Tube => vec![
            ("outer_r".to_string(), lit(0.02)),
            ("inner_r".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.03)),
        ],
        PrimitiveKind::Cone => vec![
            ("bottom_radius".to_string(), lit(0.02)),
            ("top_radius".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.03)),
        ],
        PrimitiveKind::Wedge => vec![
            ("width".to_string(), lit(0.02)),
            ("depth".to_string(), lit(0.03)),
            ("height".to_string(), lit(0.04)),
            ("top_width".to_string(), lit(0.01)),
        ],
        PrimitiveKind::Torus => vec![
            ("major_radius".to_string(), lit(0.03)),
            ("minor_radius".to_string(), lit(0.01)),
        ],
    };
    CompiledGeometryOp::Primitive { kind: k, args }
}

/// Golden snapshot per `PrimitiveKind`. EXHAUSTIVE match (no `_`): a new kind
/// without a golden is a compile error (the G2 coverage signal). Placeholders
/// (`""`) are replaced with captured actuals during the step-2 GREEN bootstrap.
fn primitive_golden(k: PrimitiveKind) -> &'static str {
    match k {
        PrimitiveKind::Box => "",
        PrimitiveKind::Cylinder => "",
        PrimitiveKind::Sphere => "",
        PrimitiveKind::Tube => "",
        PrimitiveKind::Cone => "",
        PrimitiveKind::Wedge => "",
        PrimitiveKind::Torus => "",
    }
}

#[test]
fn characterize_primitive_family() {
    assert_eq!(ALL_PRIMITIVE.len(), 7, "PrimitiveKind variant count drifted");
    let drift: Vec<String> = ALL_PRIMITIVE
        .iter()
        .filter_map(|&k| {
            characterize(&format!("primitive:{k}"), &primitive_case(k), &[], primitive_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}
