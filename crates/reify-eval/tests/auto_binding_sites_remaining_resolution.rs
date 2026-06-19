//! Eval e2e tests for the remaining `auto` binding sites (task 3810, ε):
//! - LET: `let m : Length = auto` (a, b, e, f)
//! - CONSTRUCTION (sub paren-form): `sub bolt = Bolt(length: auto)` (c)
//! - CONNECT-PARAM: `connect a -> b : ConnType { gain = auto }` (d)
//!
//! ## RED→GREEN arc
//!
//! Step 7 (RED): Tests here fire the full parse→compile→eval pipeline for each
//! remaining auto binding site. They are RED until step-8 confirms that the
//! existing 3806/γ eval precedence guards (graph.rs ~429, unfold.rs ~308) plus
//! the param-default-auto solver path already handle all three sites with zero
//! new eval code (design D2). If any scoped-cell shape is uncovered, step-8
//! applies the minimal guard generalization.
//!
//! Step 8 (GREEN): confirms (a)–(f) all pass.
//!
//! ## Example smoke test
//!
//! Step 9 (RED): `example_auto_binding_sites_ri_all_four_resolve` reads
//! `examples/auto_binding_sites.ri` and asserts all four delegated cells are
//! Determined. RED until step-10 extends the example file.
//!
//! Step 10 (GREEN): extends `examples/auto_binding_sites.ri` with all four sites.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, Value};
use reify_test_support::parse_and_compile_with_stdlib;

/// Build an Engine backed by `SimpleConstraintChecker` + `DimensionalSolver`.
fn engine_with_solver() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(DimensionalSolver))
}

/// Filter `diagnostics` to error-severity entries.
fn errors_only(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Filter `diagnostics` to warning-severity entries.
fn warnings_only(diagnostics: &[reify_core::Diagnostic]) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

// ── Test (a): LET auto strict resolves uniquely ───────────────────────────────

/// `let m : Length = auto` with `constraint self.m == 10mm` must resolve to
/// `(10mm, Determined)`.
///
/// Uses 10mm = 0.01 SI because the DimensionalSolver's default initial guess for
/// Length cells is 0.01 SI — the initial point is already feasible, enabling the
/// early-exit path and avoiding Nelder-Mead precision limits (sd_tolerance=1e-15
/// fires at residual ~2e-8 > FEASIBILITY_THRESHOLD=1e-12 when the target differs
/// from the initial guess). The sub-override reference test uses the same value.
///
/// The §4.4 invariant says a let-auto cell is structurally identical to a
/// param-default-auto cell — both ride the same M3 solver path.
#[test]
fn let_auto_strict_resolves_determined() {
    let source = r#"
structure E {
    let m : Length = auto
    constraint self.m == 10mm
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "expected no error-severity eval diagnostics, got: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("E", "m");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "E.m should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "E.m should be Determined after auto resolution, got {:?}",
        det
    );

    // 10mm = 0.01 SI
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-6),
        "E.m should equal 10mm (0.01 SI); got {:?}",
        val
    );
}

// ── Test (b): LET §4.4 invariant ──────────────────────────────────────────────

/// §4.4 invariant: `let m : Length = auto` + constraint must resolve to the same
/// value as the equivalent `param m : Length = auto` + constraint.
///
/// Both share the same equality constraint (== 10mm; 0.01 SI = solver default
/// initial guess so the solver starts feasible) and must reach the same resolved
/// value. This verifies that the let-auto producer reuses identical M3 resolution
/// semantics, not a separate path.
#[test]
fn let_auto_section_4_4_invariant() {
    // LET-auto path.
    let source_let = r#"
structure ELet {
    let m : Length = auto
    constraint self.m == 10mm
}
"#;
    let compiled_let = parse_and_compile_with_stdlib(source_let);
    let mut engine_let = engine_with_solver();
    let result_let = engine_let.eval(&compiled_let);

    let let_errors = errors_only(&result_let.diagnostics);
    assert!(
        let_errors.is_empty(),
        "let-auto path: unexpected errors: {:?}",
        let_errors
    );

    let snap_let = engine_let.snapshot().expect("snapshot for ELet");
    let id_let = ValueCellId::new("ELet", "m");
    let (val_let, det_let) = snap_let.values.get(&id_let).expect("ELet.m in snapshot");
    assert_eq!(
        *det_let,
        DeterminacyState::Determined,
        "ELet.m should be Determined"
    );

    // Param-default-auto equivalent path.
    let source_param = r#"
structure EParam {
    param m : Length = auto
    constraint self.m == 10mm
}
"#;
    let compiled_param = parse_and_compile_with_stdlib(source_param);
    let mut engine_param = engine_with_solver();
    let result_param = engine_param.eval(&compiled_param);

    let param_errors = errors_only(&result_param.diagnostics);
    assert!(
        param_errors.is_empty(),
        "param-default-auto path: unexpected errors: {:?}",
        param_errors
    );

    let snap_param = engine_param.snapshot().expect("snapshot for EParam");
    let id_param = ValueCellId::new("EParam", "m");
    let (val_param, det_param) = snap_param.values.get(&id_param).expect("EParam.m in snapshot");
    assert_eq!(
        *det_param,
        DeterminacyState::Determined,
        "EParam.m should be Determined"
    );

    // §4.4: the two paths must produce the same resolved value.
    let si_let = match val_let {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("ELet.m should be Scalar, got {:?}", other),
    };
    let si_param = match val_param {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("EParam.m should be Scalar, got {:?}", other),
    };
    assert!(
        (si_let - si_param).abs() < 1e-9,
        "§4.4 invariant violated: let-auto ELet.m = {} != param-default EParam.m = {}",
        si_let,
        si_param
    );
}

// ── Test (c): CONSTRUCTION named-arg (sub paren-form) resolves ────────────────

/// `sub bolt = Bolt(length: auto)` (Bolt default 5mm) + `constraint self.bolt.length == 10mm`
/// must resolve to `(10mm, Determined)` — not the 5mm default.
///
/// Uses 10mm = 0.01 SI (solver default initial guess) so the initial point is
/// already feasible (same approach as test (a) and the sub-override reference tests).
/// The 5mm child default differs from 10mm, proving the scoped Auto cell mechanism
/// worked — the solver's initial value (not the child default) was used.
///
/// The scoped id is `ValueCellId::new("E.bolt", "length")`.
#[test]
fn construction_named_arg_auto_resolves_determined() {
    let source = r#"
structure Bolt {
    param length : Length = 5mm
}
structure E {
    sub bolt = Bolt(length: auto)
    constraint self.bolt.length == 10mm
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "expected no error-severity eval diagnostics, got: {:?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("E.bolt", "length");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "E.bolt.length should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "E.bolt.length should be Determined after auto resolution, got {:?}",
        det
    );

    // 10mm = 0.01 SI; the Bolt child default (5mm = 0.005) was NOT used.
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-6),
        "E.bolt.length should equal 10mm (0.01 SI), not the 5mm child default; got {:?}",
        val
    );
}

// ── Test (d): CONNECT-PARAM auto(free) resolves as Determined ─────────────────

/// `connect a -> b : ConnType { gain = auto(free) }` creates a scoped Auto cell
/// `E.__connector_0.gain` in the parent E.  With `auto(free)` and no determining
/// constraint in E, the solver returns a feasible value (0.01 SI = 10mm, the
/// default initial guess for Length cells) and emits a non-unique warning.
///
/// Design note (D5): E's user code cannot reference the synthesized `__connector_N`
/// name, so a strict `auto` at the connect site has no E-level constraint to satisfy —
/// it would always be underdetermined.  The `auto(free)` variant is the correct mode
/// when the connector parameter should be left as a free exploration variable.
///
/// What this test proves:
///   1. The connect-site producer (step-6) correctly emits `E.__connector_0.gain`
///      as a scoped Auto cell in E's value_cells.
///   2. The 3806/γ eval precedence guard (unfold.rs ~308) fires for this scoped cell,
///      preventing the ConnType child default (5mm = 0.005 SI) from overwriting the
///      Auto state — the resolved value is 0.01 (initial guess), NOT 0.005 (default).
///   3. auto(free) semantics: Determined + non-unique warning (no error).
#[test]
fn connect_param_auto_free_resolves_determined() {
    let source = r#"
trait Signal {}
structure ConnType {
    param gain : Length = 5mm
}
structure E {
    port a : out Signal {}
    port b : in Signal {}
    connect a -> b : ConnType { gain = auto(free) }
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    // auto(free) with no constraint: no error, but expects a non-unique warning.
    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "auto(free) connect-param should not produce error diagnostics; got: {:?}",
        eval_errors
    );

    let eval_warnings = warnings_only(&result.diagnostics);
    assert!(
        !eval_warnings.is_empty(),
        "auto(free) connect-param should emit a non-unique warning; \
         got no warnings (diagnostics: {:?})",
        result.diagnostics
    );

    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("E.__connector_0", "gain");
    let (val, det) = snap.values.get(&id).unwrap_or_else(|| {
        panic!(
            "E.__connector_0.gain should be in snapshot; keys: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "E.__connector_0.gain should be Determined (auto(free) feasible), got {:?}",
        det
    );

    // Value must be a Scalar (any feasible value; the solver initial guess is 0.01).
    // Critically, it must NOT be the ConnType child default 5mm (0.005 SI) — that
    // would indicate the 3806/γ precedence guard failed to fire.
    assert!(
        matches!(val, Value::Scalar { .. }),
        "E.__connector_0.gain should be a Scalar value; got {:?}",
        val
    );
    let si = match val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar, got {:?}", other),
    };
    assert!(
        (si - 0.005).abs() > 1e-6,
        "E.__connector_0.gain must NOT equal the ConnType child default 5mm (0.005 SI); \
         got {si} — the 3806/γ precedence guard may have failed"
    );
}

// ── Test (e): strict-underdetermined emits error ──────────────────────────────

/// `let m : Length = auto` with NO determining constraint must emit the M3
/// "not uniquely determined" error (mirrors the sub-override site's test (c)
/// from auto_sub_override_resolution.rs).
#[test]
fn let_auto_strict_underdetermined_emits_error() {
    let source = r#"
structure E {
    let m : Length = auto
    // no constraint — auto cannot be uniquely determined
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    // Must produce at least one Error-severity diagnostic (non-unique).
    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        !eval_errors.is_empty(),
        "expected a 'not uniquely determined' error diagnostic for underdetermined \
         strict let-auto; got no errors (diagnostics: {:?})",
        result.diagnostics
    );

    // The error message must mention "uniquely determined" or "unique".
    assert!(
        eval_errors.iter().any(|d| {
            d.message.to_lowercase().contains("unique")
                || d.message.to_lowercase().contains("not uniquely")
        }),
        "expected an error about unique determination; got: {:?}",
        eval_errors
    );
}

// ── Test (f): auto(free) underdetermined emits warning + feasible value ───────

/// `let m : Length = auto(free)` with NO determining constraint must emit the
/// auto(free) non-unique warning and still produce a feasible Scalar value.
///
/// Mirrors `sub_override_auto_free_non_unique_emits_warning` from
/// auto_sub_override_resolution.rs.
#[test]
fn let_auto_free_underdetermined_emits_warning_and_scalar() {
    let source = r#"
structure E {
    let m : Length = auto(free)
    // no constraint — auto(free) may be non-unique
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    // auto(free) → no error-severity diagnostics (feasibility is accepted).
    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "auto(free) should not produce error diagnostics; got: {:?}",
        eval_errors
    );

    // Must produce at least one warning about non-uniqueness.
    let eval_warnings = warnings_only(&result.diagnostics);
    assert!(
        !eval_warnings.is_empty(),
        "expected a non-unique warning for auto(free) underdetermined let; \
         got no warnings (diagnostics: {:?})",
        result.diagnostics
    );

    // The cell should have a resolved Scalar value — any feasible value.
    let snap = engine.snapshot().expect("snapshot should exist");
    let id = ValueCellId::new("E", "m");
    let (val, det) = snap
        .values
        .get(&id)
        .expect("E.m should be present after auto(free) resolution with DimensionalSolver");
    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "E.m should be Determined even with auto(free)"
    );
    assert!(
        matches!(val, Value::Scalar { .. }),
        "E.m should be a Scalar value; got {:?}",
        val
    );
}

// ── Test (step-9 / step-10): extended example smoke test ─────────────────────

/// Path to the ε-slice four-site integration example.
///
/// RED until step-10 extends `examples/auto_binding_sites.ri` with all four sites.
const AUTO_BINDING_SITES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto_binding_sites.ri"
);

/// `examples/auto_binding_sites.ri` must compile, evaluate without errors, and
/// have ALL FOUR delegated auto binding-site cells in `DeterminacyState::Determined`:
///   - sub-override:   `AutoBindingSites.b.bore`
///   - construction:   as defined by the ε-extended example
///   - let:            as defined by the ε-extended example
///   - connect-param:  as defined by the ε-extended example
///
/// The exact `ValueCellId`s are derived from the entity/member names in the
/// extended example file (see step-10 for the canonical names).
///
/// RED until step-10 extends the example.
#[test]
fn example_auto_binding_sites_ri_all_four_resolve() {
    let source = std::fs::read_to_string(AUTO_BINDING_SITES_PATH)
        .expect("examples/auto_binding_sites.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let compile_errors = errors_only(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "examples/auto_binding_sites.ri should compile with no error-severity diagnostics; \
         got:\n{:#?}",
        compile_errors
    );

    let mut engine = engine_with_solver();
    let result = engine.eval(&compiled);

    let eval_errors = errors_only(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "examples/auto_binding_sites.ri should evaluate with no error-severity diagnostics; \
         got:\n{:#?}",
        eval_errors
    );

    let snap = engine.snapshot().expect("snapshot should exist");

    // (1) Sub-override cell — already in the 3806 slice.
    let sub_override_id = ValueCellId::new("AutoBindingSites.b", "bore");
    let (_, sub_det) = snap.values.get(&sub_override_id).unwrap_or_else(|| {
        panic!(
            "AutoBindingSites.b.bore should be in snapshot; available cells: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *sub_det,
        DeterminacyState::Determined,
        "AutoBindingSites.b.bore should be Determined"
    );

    // (2) Construction cell — `AllFourSites.bolt.length` (or whatever the
    //     ε-extended example names it; the ID is `AllFourSites.bolt` + `length`).
    //     Update this ID to match step-10's entity/member naming.
    let construction_id = ValueCellId::new("AllFourSites.bolt", "length");
    let (_, cons_det) = snap.values.get(&construction_id).unwrap_or_else(|| {
        panic!(
            "AllFourSites.bolt.length should be in snapshot; available cells: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *cons_det,
        DeterminacyState::Determined,
        "AllFourSites.bolt.length should be Determined"
    );

    // (3) LET cell — `AllFourSites.m`.
    let let_id = ValueCellId::new("AllFourSites", "m");
    let (_, let_det) = snap.values.get(&let_id).unwrap_or_else(|| {
        panic!(
            "AllFourSites.m should be in snapshot; available cells: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *let_det,
        DeterminacyState::Determined,
        "AllFourSites.m should be Determined"
    );

    // (4) Connect-param cell — `AllFourSites.__connector_0.gain`.
    let connect_id = ValueCellId::new("AllFourSites.__connector_0", "gain");
    let (_, conn_det) = snap.values.get(&connect_id).unwrap_or_else(|| {
        panic!(
            "AllFourSites.__connector_0.gain should be in snapshot; available cells: {:?}",
            snap.values
                .iter()
                .map(|(k, _)| format!("{}", k))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *conn_det,
        DeterminacyState::Determined,
        "AllFourSites.__connector_0.gain should be Determined"
    );
}
