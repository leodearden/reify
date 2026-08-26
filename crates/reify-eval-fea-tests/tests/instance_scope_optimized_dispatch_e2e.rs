//! Task #6662, at FEA scale: an uninstantiated-arg sub's FEA-derived value
//! cells at INSTANCE scope must equal the template's solved values.
//!
//! The `Int`-valued fixtures in `crates/reify-eval/tests/compute_dispatch_registry.rs`
//! pin the mechanism; this file pins the thing the task is actually about. A
//! real `solve_elastic_static` call returns an `ElasticResult` whose
//! `displacement` is a Sampled `Field`, and the whole downstream chain
//! (`max(...)` → a `Stiffness` → a parent-level read → a constraint) is what
//! silently went indeterminate when the instance cell held the inline-fallback
//! `ElasticResult()` shell instead of the solved value.
//!
//! This is the shape the defect was found in — `prj/printer_v01/printer.ri`'s
//! `GantryFea` / `sub gantry_fea = GantryFea()` pair, shrunk to a fast solve.
//! That file still carries the pinned-literal workaround (and the comment
//! naming this defect) at the time of writing; removing it is deliberately
//! out of this task's scope.
//!
//! Engine construction is copied verbatim from
//! `solve_elastic_static_e2e.rs` — `make_simple_engine()` +
//! `reify_eval::compute_targets::register_compute_fns(&mut engine)` — so this
//! e2e cannot drift from how every other FEA e2e in this crate builds its
//! engine.

use reify_core::{Severity, ValueCellId};
use reify_ir::{Satisfaction, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Cantilever beam whose FEA-derived cells are read through a `sub`.
///
/// `Asm` instantiates `Beam` with NO constructor overrides, so the instance's
/// inputs are value-identical to the template's and the two scopes must agree.
///
/// Kept deliberately small and P1 (bare `ElasticOptions()`) — the assertions
/// below are existence / determinacy / cross-scope-equality only, never a
/// numeric tolerance, so there is nothing here that a coarser mesh can break.
fn instance_scope_fea_source() -> &'static str {
    r#"
        structure Beam {
            param span : Length = 1000mm
            param w    : Length = 100mm
            param h    : Length = 100mm
            param load : Real   = 1000.0

            let material = Steel_AISI_1045()
            let tip      = PointLoad(point: "tip", force: load)
            let mount    = FixedSupport(target: "root")

            let r_static = solve_elastic_static(
                material, span, w, h, [tip], [mount], ElasticOptions()
            )
            let defl     = max(r_static.displacement)
            let k        : Stiffness = load * 1N / defl
        }

        structure Asm {
            sub beam = Beam()
            let k_from_sub = self.beam.k
            constraint self.beam.k > 1N / 1m
        }
    "#
}

/// Extract a named field from an `ElasticResult` value, handling both the
/// `StructureInstance` shape the engine builds and the `Map` fallback — same
/// helper shape as `solve_elastic_static_e2e.rs`.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// The task's literal acceptance criterion: an uninstantiated-arg sub's
/// FEA-derived value cells at instance scope equal the template's solved
/// values, and the whole downstream chain stays determinate.
#[test]
fn instance_scope_fea_cells_equal_template_solved_values() {
    let compiled = parse_and_compile_with_stdlib(instance_scope_fea_source());

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    // `check` gives us both the values and the constraint satisfactions in one
    // pass — `EvalResult` carries no constraint results.
    let result = engine.check(&compiled);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the solve, got: {:?}",
        errors
    );

    let template_r = ValueCellId::new("Beam", "r_static");
    let instance_r = ValueCellId::new("Asm.beam", "r_static");

    // ── (1) The instance cell is a POPULATED ElasticResult, not the shell ────
    //
    // The inline-fallback body is a bare `{ ElasticResult() }` ctor, whose
    // Field-typed params are all Undef. Asserting `displacement` is non-Undef
    // is what distinguishes a real solve from that sentinel; asserting on the
    // field's EXISTENCE rather than on a number keeps this from becoming a
    // numeric-tolerance pin.
    let instance_val = result
        .values
        .get(&instance_r)
        .unwrap_or_else(|| panic!("cell Asm.beam.r_static not found in check result"));
    assert!(
        matches!(instance_val, Value::StructureInstance(_) | Value::Map(_)),
        "Asm.beam.r_static must be an ElasticResult value, got: {:?}",
        instance_val
    );
    let instance_disp = extract_field(instance_val, "displacement").unwrap_or_else(|| {
        panic!(
            "Asm.beam.r_static has no `displacement` field: {:?}",
            instance_val
        )
    });
    assert_ne!(
        instance_disp,
        Value::Undef,
        "Asm.beam.r_static.displacement is Undef — the instance cell holds the \
         inline-fallback `ElasticResult()` shell (every Field-typed param Undef) \
         rather than the solved value"
    );

    // ── (2) Cross-scope equality — the acceptance wording ────────────────────
    //
    // Exact by construction under this task's design (the same `Value` is
    // cloned from the template cell), so no epsilon is needed or wanted.
    let template_val = result
        .values
        .get(&template_r)
        .unwrap_or_else(|| panic!("cell Beam.r_static not found in check result"));
    assert_eq!(
        instance_val, template_val,
        "Asm.beam.r_static must EQUAL the template's Beam.r_static — an \
         uninstantiated-arg sub inherits the template's solved value verbatim"
    );

    // ── (3) The downstream chain is determinate ─────────────────────────────
    //
    // `defl = max(r_static.displacement)` and `k = load * 1N / defl` are
    // ordinary lets over the instance map. With the shell in place they went
    // Undef; this is the chain the task reports as undef.
    for member in ["defl", "k"] {
        let cell = ValueCellId::new("Asm.beam", member);
        let val = result
            .values
            .get(&cell)
            .unwrap_or_else(|| panic!("cell Asm.beam.{} not found in check result", member));
        assert_ne!(
            *val,
            Value::Undef,
            "Asm.beam.{} must be determinate — it is derived from the sub's \
             FEA result, which is exactly the chain that went Undef when the \
             instance cell held the sentinel shell",
            member
        );
        assert_eq!(
            result.values.get(&ValueCellId::new("Beam", member)),
            Some(val),
            "Asm.beam.{} must equal the template's Beam.{}",
            member,
            member
        );
    }

    // ── (4) A parent-level read of a sub's FEA-derived cell ─────────────────
    //
    // `self.beam.k` lowers onto the `"Asm.beam"` instance cell, so this is the
    // CONSEQUENCE clause of the task: the parent could not see the sub's
    // FEA-derived value at all.
    let k_from_sub = result
        .values
        .get(&ValueCellId::new("Asm", "k_from_sub"))
        .unwrap_or_else(|| panic!("cell Asm.k_from_sub not found in check result"));
    assert_ne!(
        *k_from_sub,
        Value::Undef,
        "Asm.k_from_sub must be determinate — a parent-level `self.beam.k` read \
         of a sub's FEA-derived cell"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("Beam", "k")),
        Some(k_from_sub),
        "Asm.k_from_sub must equal the template's Beam.k"
    );

    // ── (5) The constraint is EVALUATED, not skipped as indeterminate ───────
    //
    // `Satisfaction::Indeterminate` is precisely the "undef inputs" outcome
    // the sentinel shell produced. Asserting `Satisfied` (not merely
    // "not Indeterminate") also pins that the value that reached the
    // constraint is the real, positive stiffness.
    assert!(
        !result.constraint_results.is_empty(),
        "expected the `constraint self.beam.k > 1N / 1m` to appear in \
         constraint_results, got none"
    );
    for entry in &result.constraint_results {
        assert_ne!(
            entry.satisfaction,
            Satisfaction::Indeterminate,
            "constraint {:?} is Indeterminate — it reads a sub's FEA-derived \
             cell, which is undef exactly when the instance cell holds the \
             sentinel shell",
            entry.id
        );
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} must be Satisfied: a 1 m steel cantilever under \
             1 kN has a stiffness far above 1 N/m",
            entry.id
        );
    }
}

/// #6662 / S8 item 1: an instance-scope `@optimized` cell that a SOLVER-visible
/// constraint depends on is not clobbered by the cost loop or reverted by a
/// reeval-cone pass.
///
/// The `@optimized`-cell exclusion sets in `engine_eval.rs`
/// (`is_optimized_userfn_cell` → `build_dependent_cells`, and the reeval-cone
/// exclusions) all iterate `template.value_cells`, so they are TEMPLATE-keyed.
/// Now that an instance cell can hold a real dispatched value, the concern is
/// that such a cell is not in those exclusion sets and could be overwritten
/// with `Undef` by the solver's cost loop, or re-evaluated back to the
/// body-inline sentinel by a reeval pass.
///
/// The first e2e in this file cannot surface that: it has no `auto`, so the
/// solver never engages and `build_dependent_cells` never runs. Here `budget`
/// is an `auto` whose constraint reads `self.beam.k` — an FEA-derived INSTANCE
/// cell — which is exactly what pulls the instance cell into the solver's
/// dependent set.
///
/// Deliberately asserts NON-CLOBBERING, not the auto's resolved value: the
/// point is that the dispatched value survives the solver pass, which holds
/// regardless of what the solver decides for `budget`.
#[test]
fn solver_visible_instance_scope_fea_cell_is_not_clobbered() {
    let source = r#"
        structure BeamAuto {
            param span : Length = 1000mm
            param w    : Length = 100mm
            param h    : Length = 100mm
            param load : Real   = 1000.0

            let material = Steel_AISI_1045()
            let tip      = PointLoad(point: "tip", force: load)
            let mount    = FixedSupport(target: "root")

            let r_static = solve_elastic_static(
                material, span, w, h, [tip], [mount], ElasticOptions()
            )
            let defl     = max(r_static.displacement)
            let k        : Stiffness = load * 1N / defl
        }

        structure AsmAuto {
            sub beam = BeamAuto()
            param budget : Stiffness = auto
            constraint budget <= self.beam.k
            constraint budget >= 1N / 1m
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let result = engine.check(&compiled);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics with a solver-visible instance-scope \
         FEA cell, got: {:?}",
        errors
    );

    // The instance cell still holds the solved ElasticResult after the solver
    // pass — not Undef (cost-loop clobber) and not the sentinel shell
    // (reeval-cone revert).
    let instance_val = result
        .values
        .get(&ValueCellId::new("AsmAuto.beam", "r_static"))
        .unwrap_or_else(|| panic!("cell AsmAuto.beam.r_static not found in check result"));
    let instance_disp = extract_field(instance_val, "displacement").unwrap_or_else(|| {
        panic!(
            "AsmAuto.beam.r_static has no `displacement` field: {:?}",
            instance_val
        )
    });
    assert_ne!(
        instance_disp,
        Value::Undef,
        "AsmAuto.beam.r_static.displacement is Undef after the solver pass — the \
         instance cell was clobbered by the cost loop or reverted to the \
         body-inline sentinel by a reeval-cone pass"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("BeamAuto", "r_static")),
        Some(instance_val),
        "AsmAuto.beam.r_static must still equal the template's BeamAuto.r_static \
         after constraint solving"
    );

    // The derived stiffness the constraint actually reads stays determinate.
    let instance_k = result
        .values
        .get(&ValueCellId::new("AsmAuto.beam", "k"))
        .unwrap_or_else(|| panic!("cell AsmAuto.beam.k not found in check result"));
    assert_ne!(
        *instance_k,
        Value::Undef,
        "AsmAuto.beam.k must stay determinate through the solver pass — it is \
         the cell the auto's constraint reads"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("BeamAuto", "k")),
        Some(instance_k),
        "AsmAuto.beam.k must still equal the template's BeamAuto.k after solving"
    );

    // The constraints that read the instance cell were evaluated, not skipped.
    assert!(
        !result.constraint_results.is_empty(),
        "expected the budget constraints in constraint_results, got none"
    );
}
