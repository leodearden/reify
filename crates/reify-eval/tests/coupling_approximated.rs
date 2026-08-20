// Integration tests for W_COUPLING_APPROXIMATED (M-WHOLE α, task #5013).
//
// Mirrors scope_coupling.rs: engine-level builder tests driven through the
// literal `reify check` entry point (engine.check()), asserting on
// DiagnosticCode rather than message substrings. The structural cluster set is
// unit-tested inside resolve_order.rs; this file covers the BT2 user-observable
// surface, the W_SCOPE_COUPLING → W_COUPLING_APPROXIMATED graduation (step-11),
// and the INV-2 surface (no clusters ⇒ no diagnostic).
//
// ## Cap independence
//
// This external test cannot read the pub(crate) `WHOLE_MODEL_CLUSTER_DIM_CAP`,
// so the over-cap fixtures use a comfortably-over-cap auto count (20 per scope,
// merged dim 40): robust as long as the cap stays below ~40. The message
// assertions check the dim the test itself built (40) and that the cap is
// named, never the cap's exact value.

use reify_core::{DiagnosticCode, ModulePath, Severity, Type};
use reify_eval::Engine;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder, gt, literal, mm,
    value_ref,
};

/// Auto cells per scope in the over-cap fixtures. Comfortably over the ~12 cap;
/// merged dim across a 2-scope cluster is `2 * OVER_CAP_AUTOS` = 40.
const OVER_CAP_AUTOS: usize = 20;

/// Expected merged auto-dimension of a 2-scope over-cap cluster.
const OVER_CAP_DIM: usize = 2 * OVER_CAP_AUTOS;

// ---------------------------------------------------------------------------
// Helpers (mirror scope_coupling.rs).
// ---------------------------------------------------------------------------

/// Build a no-solver engine — the literal `reify check` entry point.
fn no_solver_engine() -> Engine {
    Engine::new(Box::new(MockConstraintChecker::new()), None)
}

/// Add `n` auto cells (`<entity>.a0..a{n-1}`) to a template builder.
fn with_n_autos(mut b: TopologyTemplateBuilder, entity: &str, n: usize) -> TopologyTemplateBuilder {
    for i in 0..n {
        b = b.auto_param(entity, &format!("a{i}"), Type::length());
    }
    b
}

/// Count `W_COUPLING_APPROXIMATED` diagnostics in a result set.
fn count_coupling_approximated(diagnostics: &[reify_core::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CouplingApproximated))
        .count()
}

// ---------------------------------------------------------------------------
// step-9: BT2 user-observable surface + INV-2 surface.
// ---------------------------------------------------------------------------

/// (BT2) An over-cap 2-cycle {Alpha, Beta} (20 autos each ⇒ merged dim 40)
/// surfaces exactly one `W_COUPLING_APPROXIMATED` warning on `reify check`,
/// naming both scopes, the merged dim, and the cap.
#[test]
fn over_cap_cycle_emits_coupling_approximated() {
    // Alpha reads Beta.a0, Beta reads Alpha.a0 → irreducible 2-cycle. Each scope
    // owns OVER_CAP_AUTOS auto cells, so the merged cluster dim is over the cap.
    let alpha = with_n_autos(TopologyTemplateBuilder::new("Alpha"), "Alpha", OVER_CAP_AUTOS)
        .constraint("Alpha", 0, None, gt(value_ref("Beta", "a0"), literal(mm(0.0))))
        .build();
    let beta = with_n_autos(TopologyTemplateBuilder::new("Beta"), "Beta", OVER_CAP_AUTOS)
        .constraint("Beta", 0, None, gt(value_ref("Alpha", "a0"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(alpha)
        .template(beta)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.check(&module);

    let approx: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CouplingApproximated))
        .collect();
    assert_eq!(
        approx.len(),
        1,
        "over-cap cluster must surface exactly one W_COUPLING_APPROXIMATED; got {}: {:?}",
        approx.len(),
        result.diagnostics,
    );

    let d = approx[0];
    assert_eq!(
        d.severity,
        Severity::Warning,
        "W_COUPLING_APPROXIMATED must be a Warning; got {:?}",
        d.severity,
    );
    let m = &d.message;
    assert!(
        m.contains("Alpha") && m.contains("Beta"),
        "message must name both member scopes; got: {m}",
    );
    assert!(
        m.contains(&OVER_CAP_DIM.to_string()),
        "message must contain the merged auto-dimension ({OVER_CAP_DIM}); got: {m}",
    );
    assert!(
        m.contains("cap"),
        "message must name the cap; got: {m}",
    );
}

/// (within-cap) A 2-cycle with 1 auto each (merged dim 2 ≤ cap) is a
/// `MergedSolve` cluster — no `W_COUPLING_APPROXIMATED`.
#[test]
fn within_cap_cycle_emits_no_coupling_approximated() {
    let alpha = TopologyTemplateBuilder::new("Alpha")
        .auto_param("Alpha", "k", Type::length())
        .constraint("Alpha", 0, None, gt(value_ref("Beta", "m"), literal(mm(0.0))))
        .build();
    let beta = TopologyTemplateBuilder::new("Beta")
        .auto_param("Beta", "m", Type::length())
        .constraint("Beta", 0, None, gt(value_ref("Alpha", "k"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(alpha)
        .template(beta)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.check(&module);

    let count = count_coupling_approximated(&result.diagnostics);
    assert_eq!(
        count, 0,
        "within-cap cluster must emit 0 W_COUPLING_APPROXIMATED; got {}: {:?}",
        count, result.diagnostics,
    );
}

/// (INV-2 surface) An uncoupled module (each scope reads only its own auto)
/// forms no cluster and emits no `W_COUPLING_APPROXIMATED`.
#[test]
fn uncoupled_module_emits_no_coupling_approximated() {
    let x = TopologyTemplateBuilder::new("X")
        .auto_param("X", "a", Type::length())
        .constraint("X", 0, None, gt(value_ref("X", "a"), literal(mm(0.0))))
        .build();
    let y = TopologyTemplateBuilder::new("Y")
        .auto_param("Y", "b", Type::length())
        .constraint("Y", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(x)
        .template(y)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.check(&module);

    let count = count_coupling_approximated(&result.diagnostics);
    assert_eq!(
        count, 0,
        "uncoupled module must emit 0 W_COUPLING_APPROXIMATED (INV-2 surface); got {}: {:?}",
        count, result.diagnostics,
    );
}

// ---------------------------------------------------------------------------
// step-11: graduation. An over-cap SCC emits W_COUPLING_APPROXIMATED INSTEAD
// of W_SCOPE_COUPLING; within-cap SCCs keep emitting the generic warning.
// ---------------------------------------------------------------------------

/// Count `W_SCOPE_COUPLING` diagnostics in a result set.
fn count_scope_coupling(diagnostics: &[reify_core::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count()
}

/// (graduation) An over-cap SCC 2-cycle surfaces `W_COUPLING_APPROXIMATED` and
/// ZERO `W_SCOPE_COUPLING` — the generic warning is graduated away (§3.4).
#[test]
fn over_cap_cycle_graduates_away_scope_coupling() {
    let alpha = with_n_autos(TopologyTemplateBuilder::new("Alpha"), "Alpha", OVER_CAP_AUTOS)
        .constraint("Alpha", 0, None, gt(value_ref("Beta", "a0"), literal(mm(0.0))))
        .build();
    let beta = with_n_autos(TopologyTemplateBuilder::new("Beta"), "Beta", OVER_CAP_AUTOS)
        .constraint("Beta", 0, None, gt(value_ref("Alpha", "a0"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(alpha)
        .template(beta)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.check(&module);

    let approx = count_coupling_approximated(&result.diagnostics);
    let scope = count_scope_coupling(&result.diagnostics);
    assert!(
        approx >= 1,
        "over-cap SCC must surface W_COUPLING_APPROXIMATED; got {}: {:?}",
        approx, result.diagnostics,
    );
    assert_eq!(
        scope, 0,
        "over-cap SCC must graduate away W_SCOPE_COUPLING (emit it INSTEAD); got {}: {:?}",
        scope, result.diagnostics,
    );
}

/// (graduation guard) A within-cap SCC 2-cycle keeps emitting the generic
/// `W_SCOPE_COUPLING` and no `W_COUPLING_APPROXIMATED`. Until β lands, a
/// within-cap cluster is still solved bottom-up approximate, so the generic
/// warning stays accurate (this keeps scope_coupling.rs test H green).
#[test]
fn within_cap_cycle_keeps_scope_coupling() {
    let alpha = TopologyTemplateBuilder::new("Alpha")
        .auto_param("Alpha", "k", Type::length())
        .constraint("Alpha", 0, None, gt(value_ref("Beta", "m"), literal(mm(0.0))))
        .build();
    let beta = TopologyTemplateBuilder::new("Beta")
        .auto_param("Beta", "m", Type::length())
        .constraint("Beta", 0, None, gt(value_ref("Alpha", "k"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(alpha)
        .template(beta)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.check(&module);

    let scope = count_scope_coupling(&result.diagnostics);
    let approx = count_coupling_approximated(&result.diagnostics);
    assert!(
        scope >= 1,
        "within-cap SCC must keep emitting W_SCOPE_COUPLING; got {}: {:?}",
        scope, result.diagnostics,
    );
    assert_eq!(
        approx, 0,
        "within-cap SCC must NOT emit W_COUPLING_APPROXIMATED; got {}: {:?}",
        approx, result.diagnostics,
    );
}
