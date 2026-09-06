//! End-to-end test for kinematic singularity surfacing through `snapshot()`/
//! `sweep()` (task 3580 — GR-039 / cluster C-37).
//!
//! Drives a rank-deficient closed-chain mechanism through the full
//! `parse → compile_with_stdlib → eval` pipeline and asserts BOTH halves of
//! the leaf observable: the Snapshot Map's `is_singular = Bool(true)` flag
//! (baked by `reify-stdlib::snapshot::make_snapshot`) AND a typed
//! `KinematicSingularity` Warning in `EvalResult.diagnostics` (emitted by
//! `engine_eval::detect_kinematic_singularity`).
//!
//! Mirrors `kinematic_sweep_closed_chain.rs` (closed-chain mechanism source
//! pattern) and `forward_kinematics_e2e.rs` (direct `bind()`+`snapshot()`
//! source pattern).
//!
//! See docs/prds/v0_2/kinematic-constraints.md, §"Singularity, over/under-constraint diagnostics".

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{Value, ValueMap};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

/// Rank-deficient closed-chain mechanism: FOUR bodies, THREE prismatic-X
/// joints. `solidA` anchors the closing joint `j_x` directly to world
/// (spanning-tree edge); `solidB` anchors `j_a` to world and `solidC` stacks
/// `j_b` on top of `j_a`, giving the closing side a TWO-DEEP walk; `solidD`
/// re-anchors the SAME `j_x` on top of `j_b` (closing edge). The resulting
/// paths are `path_a = [world, j_x]` and `path_b = [world, j_a, j_b]`.
///
/// Free variables come from `chain_b` ONLY —
/// `loop_closure::extract_loop_closure_chains` resolves every `chain_a`
/// joint and iterates none of them. Binding/sweeping `j_x` therefore leaves
/// `j_a` and `j_b` as the two free variables, and both are prismatic on the
/// SAME +X axis, so their finite-difference Jacobian columns are identical →
/// rank-1 `JᵀJ` → `NewtonOutcome::Singular`. This is the `.ri`-source analog
/// of `snapshot_bakes_is_singular_true_for_rank_deficient_closed_chain` in
/// `reify-stdlib::snapshot`'s co-located unit tests.
///
/// **Task 7186 defect A.** The rank deficiency used to come from the closing
/// joint being appended to `path_b` as well: `chain_b` was `[j_b, j_x]`, two
/// unbound +X prismatics with identical columns. With the closing joint
/// composed exactly once (`path_a` only), that `chain_b` collapses to a
/// single free joint and the fixture stops exhibiting the condition it
/// exists to pin. The two identical free columns are therefore re-homed onto
/// a genuine two-deep closing-side walk rather than recovered from a
/// double-counted joint. The PROPERTY under test is unchanged: a genuinely
/// rank-deficient `chain_b` must surface `is_singular` plus the diagnostic.
///
/// `j_x`/`j_a`/`j_b` use three DIFFERENT ranges (rather than three identical
/// `prismatic(vec3(1,0,0), 0mm .. 1000mm)` calls) for two reasons:
///   1. **Distinct `Value`s.** Identical-range joints would be byte-identical
///      `Value::Map`s that alias in `joint_parents`, collapsing the intended
///      2-free-joint/1-loop-closure topology into a spurious second closure
///      (see the doc comment on the unit test named above).
///   2. **Non-zero closure residual at the Newton starting guess.** With the
///      closing joint on `path_a` alone, the residual reduces to
///      `bound_or_swept(j_x) − (midpoint(j_a) + midpoint(j_b))`. The free
///      side starts at `1050mm + 1250mm = 2300mm`, while `j_x` stays within
///      `0..1000mm` (both the static 500mm bind and the full sweep range), so
///      the two sides never collide — a collision would let Newton converge
///      trivially at iteration 0 without ever inverting the rank-deficient
///      Jacobian (see the offset-0.1-vs-0.0 note on that same unit test).
///
/// Both a static `snapshot()` cell (`snap`, bound at `j_x = 500mm`) and a
/// `sweep()` cell (`snaps`, driving `j_x` over its full range) are computed
/// from the same mechanism so both surfacing paths share one fixture.
const SINGULAR_SOURCE: &str = r#"
structure def Kinematic {
    let j_x = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 2100mm)
    let j_b = prismatic(vec3(1, 0, 0), 0mm .. 2500mm)

    let m0 = mechanism()
    let m1 = body(m0, "solidA", j_x)
    let m2 = body(m1, "solidB", j_a)
    let m3 = body(m2, "solidC", j_b, j_a)
    let m4 = body(m3, "solidD", j_x, j_b)

    let bind_x = bind(j_x, 500mm)
    let snap = snapshot(m4, [bind_x])

    let snaps = sweep(m4, j_x, 0mm .. 1000mm, 5)
}
"#;

/// Well-posed closed-chain regression: `kinematic_sweep_closed_chain.rs`'s
/// proven single-free-DOF 2-prismatic-X pattern. Must produce NEITHER a
/// `KinematicSingularity` diagnostic NOR any Error-severity diagnostic.
const NON_SINGULAR_SOURCE: &str = r#"
structure def Kinematic {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(1, 0, 0), 0mm .. 2000mm)
    let m0  = mechanism()
    let m1  = body(m0, "solid_a", j_a)
    let m2  = body(m1, "solid_b", j_b)
    let m3  = body(m2, "solid_c", j_b, j_a)

    let snaps = sweep(m3, j_a, 0mm .. 1000mm, 11)
}
"#;

/// `snapshot()` case: a rank-deficient closed-chain snapshot must carry
/// `is_singular = Bool(true)` on the Snapshot Map AND a `KinematicSingularity`
/// Warning must appear in `EvalResult.diagnostics`.
#[test]
fn snapshot_singularity_surfaces_is_singular_and_diagnostic_e2e() {
    let compiled = parse_and_compile_with_stdlib(SINGULAR_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let v = &result.values;
    let snap = get_value(v, "snap");
    let smap = match snap {
        Value::Map(m) => m,
        other => panic!("snap should be a Snapshot Map, got {other:?}"),
    };
    assert_eq!(
        smap.get(&Value::String("is_singular".to_string())),
        Some(&Value::Bool(true)),
        "rank-deficient closed-chain snapshot must carry is_singular=true"
    );

    let has_singularity_warning = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Warning && d.code == Some(DiagnosticCode::KinematicSingularity)
    });
    assert!(
        has_singularity_warning,
        "expected a KinematicSingularity Warning, got: {:?}",
        result.diagnostics
    );
}

/// `sweep()` case: `snaps` is a `List<Snapshot>` — the engine detector must
/// recurse into the List to find the singular snapshots and still emit a
/// `KinematicSingularity` Warning for the `snaps` cell.
#[test]
fn sweep_singularity_surfaces_diagnostic_through_list_e2e() {
    let compiled = parse_and_compile_with_stdlib(SINGULAR_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let v = &result.values;
    let snaps = get_value(v, "snaps");
    let snaps_list = match snaps {
        Value::List(l) => l,
        other => panic!("snaps should be a List, got {other:?}"),
    };
    assert_eq!(
        snaps_list.len(),
        5,
        "sweep should produce exactly 5 snapshots"
    );

    for (i, snap) in snaps_list.iter().enumerate() {
        let smap = match snap {
            Value::Map(m) => m,
            other => panic!("snaps[{i}] should be a Map, got {other:?}"),
        };
        assert_eq!(
            smap.get(&Value::String("is_singular".to_string())),
            Some(&Value::Bool(true)),
            "snaps[{i}] rank-deficient closed-chain snapshot must carry is_singular=true"
        );
    }

    let has_singularity_warning = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Warning && d.code == Some(DiagnosticCode::KinematicSingularity)
    });
    assert!(
        has_singularity_warning,
        "expected a KinematicSingularity Warning for the swept List<Snapshot> cell, got: {:?}",
        result.diagnostics
    );
}

/// Non-singular regression: a well-posed closed chain must produce NEITHER a
/// `KinematicSingularity` diagnostic NOR any Error-severity diagnostic.
#[test]
fn well_posed_closed_chain_has_no_singularity_diagnostic_e2e() {
    let compiled = parse_and_compile_with_stdlib(NON_SINGULAR_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let has_singularity_warning = result
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::KinematicSingularity));
    assert!(
        !has_singularity_warning,
        "well-posed closed chain must not report KinematicSingularity, got: {:?}",
        result.diagnostics
    );
}
