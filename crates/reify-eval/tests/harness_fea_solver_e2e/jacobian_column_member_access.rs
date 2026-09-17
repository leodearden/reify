//! Task 6102 — `joint_jacobian` returns `JacobianColumn`, and its members are
//! reachable.
//!
//! This is the user-observable signal for the Twist/JacobianColumn split. Until
//! this task, `joint_jacobian` was nominally typed as `Twist`, an EMPTY marker
//! structure, and compile-time member access is gated on a structure's DECLARED
//! params — so reading a column's parts was a hard compile error. Measured on
//! the pre-task tree, the fixture this test drives emitted exactly:
//!
//! ```text
//! error: structure 'Twist' has no member 'angular'
//! error: structure 'Twist' has no member 'linear'
//! ```
//!
//! Three assertions, in escalating strength:
//!
//! 1. the fixture compiles with zero Error-severity diagnostics (the defect
//!    above is gone);
//! 2. the column cell's STATIC type is `StructureRef("JacobianColumn")`, not
//!    `StructureRef("Twist")` — the nominal split actually happened;
//! 3. the member reads EVALUATE to the analytic column components. This last
//!    one is load-bearing: a declared-but-absent key evaluates to `Value::Undef`
//!    rather than erroring (probed via `snapshot(m, []).is_singular`), so
//!    without a value assertion the whole thing could be vacuously "green" while
//!    resolving to undef.
//!
//! The fixture is a committed `.ri` file rather than an inline string so the
//! same source stands for the PRD boundary and this test — idiom borrowed from
//! `crates/reify-compiler/tests/buckling_stdlib_compile.rs` and
//! `crates/reify-eval/tests/no_stale_undef_invariant_gate.rs`.

use reify_core::{Type, ValueCellId};
use reify_ir::Value;
use reify_test_support::{collect_errors, compile_source_with_stdlib, make_simple_engine};

/// The structure declared by the fixture; also the `ValueCellId` entity name.
const ENTITY: &str = "JacobianColumnMembers";

/// Read the committed fixture. Path resolved from `CARGO_MANIFEST_DIR` so the
/// test is location-independent.
fn fixture_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/prd-gate/fixtures/jacobian_column_members.ri");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e))
}

/// Resolved compile-time type of a `let` cell on the fixture's structure.
fn cell_type(module: &reify_compiler::CompiledModule, member: &str) -> Type {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == ENTITY)
        .unwrap_or_else(|| {
            panic!(
                "{ENTITY} template not found; available: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });
    template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| {
            panic!(
                "cell '{member}' not found on {ENTITY}; available: {:?}",
                template
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        })
        .cell_type
        .clone()
}

/// Assert a `Value::Vector` of three numeric components matches `expected`.
/// Mirrors `assert_vec3_close` in
/// `crates/reify-eval/tests/harness_fea_solver_e2e/kinematic_stdlib_smoke.rs`.
fn assert_vec3_close(actual: &Value, expected: [f64; 3], label: &str) {
    let items = match actual {
        Value::Vector(v) if v.len() == 3 => v,
        other => panic!(
            "{label}: expected a 3-component Vector, got {other:?}. \
             `Undef` here means member access resolved to an absent Map key — \
             the declaration would be vacuous."
        ),
    };
    for (i, comp) in items.iter().enumerate() {
        let v = comp
            .as_f64()
            .unwrap_or_else(|| panic!("{label}: component[{i}] not numeric: {comp:?}"));
        assert!(
            (v - expected[i]).abs() < 1e-12,
            "{label}: component[{i}] expected {}, got {v}",
            expected[i]
        );
    }
}

/// Reading `.angular` / `.linear` off a `joint_jacobian` result must compile
/// clean, type as `JacobianColumn`, and evaluate to the analytic components.
#[test]
fn jacobian_column_members_compile_and_evaluate() {
    let source = fixture_source();
    let compiled = compile_source_with_stdlib(&source);

    // (1) Compiles clean. Pre-task this failed with
    // "structure 'Twist' has no member 'angular'" / "... 'linear'".
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile with no Error-severity diagnostics; got {} error(s): {:#?}",
        errors.len(),
        errors
    );

    // (2) The column's STATIC type is the new nominal tag, not the old pun.
    let rev_col_ty = cell_type(&compiled, "rev_col");
    assert_eq!(
        rev_col_ty,
        Type::StructureRef("JacobianColumn".to_string()),
        "joint_jacobian(revolute(..)) must type as StructureRef(JacobianColumn), got {rev_col_ty:?}"
    );
    assert_ne!(
        rev_col_ty,
        Type::StructureRef("Twist".to_string()),
        "joint_jacobian must NOT type as StructureRef(Twist): a Jacobian column is \
         dpose/dq, not a spatial velocity (task 6102)"
    );
    let prism_col_ty = cell_type(&compiled, "prism_col");
    assert_eq!(
        prism_col_ty,
        Type::StructureRef("JacobianColumn".to_string()),
        "joint_jacobian(prismatic(..)) must type as StructureRef(JacobianColumn), \
         got {prism_col_ty:?}"
    );

    // (3) The member reads evaluate to the real column components — not undef.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:#?}"
    );

    let get = |name: &str| -> &Value {
        let id = ValueCellId::new(ENTITY, name);
        result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("{ENTITY}.{name} not found in eval result"))
    };

    // Revolute about +Z: pure rotation column.
    assert_vec3_close(get("rev_ang"), [0.0, 0.0, 1.0], "rev_col.angular");
    assert_vec3_close(get("rev_lin"), [0.0, 0.0, 0.0], "rev_col.linear");
    // Prismatic along +X: pure translation column.
    assert_vec3_close(get("prism_lin"), [1.0, 0.0, 0.0], "prism_col.linear");
}
