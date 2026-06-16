//! Type-arg un-drop, arity, and bound diagnostics — task 4603 γ.
//!
//! Tests for:
//! (a) `Type::Applied` produced when a structure name has non-empty type args
//!     (`resolve_type_expr_with_aliases_kinded` un-drop, step-2).
//! (b) `DiagnosticCode::TypeArgArity` on too-many-args / non-generic-given-args
//!     (phase_pending_bound_checks walk, step-6).
//! (c) `DiagnosticCode::TypeArgBound` on bound-violating type arg
//!     (phase_pending_bound_checks walk, step-8).

use reify_core::{diagnostics::DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

// ─── Shared fixture source ───────────────────────────────────────────────────
//
// `HasMotion` trait, two conforming structures (`Prismatic`, `Revolute`), one
// non-conforming (`NotMotion`), a generic `Coupling<P: HasMotion>`, and a
// non-generic `Foo`. All tests compile extensions of this scaffold.
fn base_source() -> &'static str {
    r#"
        trait HasMotion {}
        structure def Prismatic : HasMotion {}
        structure def Revolute : HasMotion {}
        structure def NotMotion {}
        structure def Coupling<P: HasMotion> { param p : P }
        structure def Foo {}
    "#
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1 — RED: un-drop Applied (step-2 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// `param c : Coupling<Prismatic>` must resolve `c` to
/// `Type::Applied { name: "Coupling", args: [Type::StructureRef("Prismatic")] }`.
///
/// RED until step-2: today `type_args` are silently dropped → `StructureRef("Coupling")`.
#[test]
fn applied_type_coupling_prismatic_cell_type() {
    let source = format!(
        "{}\nstructure def UseP {{ param c : Coupling<Prismatic> }}",
        base_source()
    );
    let module = compile_source(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for valid Coupling<Prismatic>; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseP")
        .expect("UseP template must exist");

    let c_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("UseP must have a value cell named 'c'");

    let expected = Type::Applied {
        name: "Coupling".to_string(),
        args: vec![Type::StructureRef("Prismatic".to_string())],
    };
    assert_eq!(
        c_cell.cell_type, expected,
        "Coupling<Prismatic> must resolve to Applied{{\"Coupling\", [StructureRef(\"Prismatic\")]}}, got {:?}",
        c_cell.cell_type
    );
}

/// `param c : Coupling<Revolute>` must resolve to
/// `Type::Applied { name: "Coupling", args: [Type::StructureRef("Revolute")] }`.
///
/// RED until step-2.
#[test]
fn applied_type_coupling_revolute_cell_type() {
    let source = format!(
        "{}\nstructure def UseR {{ param c : Coupling<Revolute> }}",
        base_source()
    );
    let module = compile_source(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for valid Coupling<Revolute>; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseR")
        .expect("UseR template must exist");

    let c_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("UseR must have a value cell named 'c'");

    let expected = Type::Applied {
        name: "Coupling".to_string(),
        args: vec![Type::StructureRef("Revolute".to_string())],
    };
    assert_eq!(
        c_cell.cell_type, expected,
        "Coupling<Revolute> must resolve to Applied{{\"Coupling\", [StructureRef(\"Revolute\")]}}, got {:?}",
        c_cell.cell_type
    );
}

/// `Coupling<Prismatic>` and `Coupling<Revolute>` must be DISTINCT types.
///
/// RED until step-2: today both are `StructureRef("Coupling")` — equal.
#[test]
fn applied_types_with_different_args_are_distinct() {
    let source = format!(
        "{}\nstructure def UseP {{ param c : Coupling<Prismatic> }}\nstructure def UseR {{ param c : Coupling<Revolute> }}",
        base_source()
    );
    let module = compile_source(&source);

    let template_p = module
        .templates
        .iter()
        .find(|t| t.name == "UseP")
        .expect("UseP must exist");
    let template_r = module
        .templates
        .iter()
        .find(|t| t.name == "UseR")
        .expect("UseR must exist");

    let p_cell = template_p.value_cells.iter().find(|vc| vc.id.member == "c").unwrap();
    let r_cell = template_r.value_cells.iter().find(|vc| vc.id.member == "c").unwrap();

    assert_ne!(
        p_cell.cell_type, r_cell.cell_type,
        "Coupling<Prismatic> and Coupling<Revolute> must be distinct types; \
         both resolved to: {:?}",
        p_cell.cell_type
    );
}

/// Empty-args invariant: `param d : Coupling` (no type args) must still resolve
/// to `Type::StructureRef("Coupling")`, not `Type::Applied`.
///
/// This must stay GREEN through step-2 (empty-args → fallthrough → StructureRef).
#[test]
fn bare_structure_ref_unchanged_when_no_type_args() {
    let source = format!(
        "{}\nstructure def UseD {{ param d : Coupling }}",
        base_source()
    );
    let module = compile_source(&source);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseD")
        .expect("UseD must exist");

    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("UseD must have a value cell named 'd'");

    assert_eq!(
        d_cell.cell_type,
        Type::StructureRef("Coupling".to_string()),
        "bare `Coupling` (no type args) must resolve to StructureRef(\"Coupling\"), got {:?}",
        d_cell.cell_type
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 5 — RED: arity diagnostics (step-6 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// Arity-too-many: `Coupling<Prismatic, Revolute>` supplies 2 args to a
/// 1-param generic — must emit exactly one `TypeArgArity` diagnostic.
///
/// RED until step-6: no arity check on the member-annotation path.
#[test]
fn arity_too_many_args_emits_type_arg_arity() {
    let source = format!(
        "{}\nstructure def Bad {{ param c : Coupling<Prismatic, Revolute> }}",
        base_source()
    );
    let module = compile_source(&source);

    let arity_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgArity))
        .collect();

    assert_eq!(
        arity_errors.len(),
        1,
        "Coupling<Prismatic, Revolute> (2 args vs 1 param) must emit exactly one \
         TypeArgArity diagnostic; got: {:?}",
        arity_errors
    );
}

/// Arity-on-non-generic: `Foo<Prismatic>` supplies 1 arg to a non-generic
/// structure (0 params) — must emit exactly one `TypeArgArity` diagnostic.
///
/// RED until step-6.
#[test]
fn arity_args_on_non_generic_structure_emits_type_arg_arity() {
    let source = format!(
        "{}\nstructure def Bad2 {{ param c : Foo<Prismatic> }}",
        base_source()
    );
    let module = compile_source(&source);

    let arity_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgArity))
        .collect();

    assert_eq!(
        arity_errors.len(),
        1,
        "Foo<Prismatic> (1 arg vs 0 params) must emit exactly one TypeArgArity diagnostic; \
         got: {:?}",
        arity_errors
    );
}

/// Correct arity: `Coupling<Prismatic>` (1 arg, 1 param) must NOT emit
/// `TypeArgArity` — only valid-arity uses are error-free on the arity check.
///
/// Must stay GREEN through step-6.
#[test]
fn correct_arity_emits_no_type_arg_arity() {
    let source = format!(
        "{}\nstructure def Ok {{ param c : Coupling<Prismatic> }}",
        base_source()
    );
    let module = compile_source(&source);

    let arity_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgArity))
        .collect();

    assert!(
        arity_errors.is_empty(),
        "Coupling<Prismatic> (correct 1 arg, 1 param) must emit NO TypeArgArity; \
         got: {:?}",
        arity_errors
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 7 — RED: bound diagnostics (step-8 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bound-violation: `Coupling<NotMotion>` passes arity (1 arg, 1 param) but
/// violates the `P: HasMotion` bound — must emit exactly one `TypeArgBound`
/// diagnostic and NO `TypeArgArity`.
///
/// RED until step-8: no bound check on the member-annotation path.
#[test]
fn bound_violation_emits_type_arg_bound() {
    let source = format!(
        "{}\nstructure def Bad {{ param c : Coupling<NotMotion> }}",
        base_source()
    );
    let module = compile_source(&source);

    // Must have NO arity error — arity is correct (1 arg, 1 param).
    let arity_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgArity))
        .collect();
    assert!(
        arity_errors.is_empty(),
        "Coupling<NotMotion> (correct arity) must emit NO TypeArgArity; got: {:?}",
        arity_errors
    );

    // Must have exactly one bound-violation diagnostic.
    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgBound))
        .collect();
    assert_eq!(
        bound_errors.len(),
        1,
        "Coupling<NotMotion> (NotMotion does not satisfy HasMotion) must emit exactly one \
         TypeArgBound diagnostic; got: {:?}",
        bound_errors
    );
}

/// Bound-satisfied: `Coupling<Prismatic>` conforms to `P: HasMotion` — must
/// emit NO `TypeArgBound` diagnostic.
///
/// Must stay GREEN through step-8.
#[test]
fn bound_satisfied_emits_no_type_arg_bound() {
    let source = format!(
        "{}\nstructure def Ok2 {{ param c : Coupling<Prismatic> }}",
        base_source()
    );
    let module = compile_source(&source);

    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeArgBound))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "Coupling<Prismatic> (Prismatic satisfies HasMotion) must emit NO TypeArgBound; \
         got: {:?}",
        bound_errors
    );
}
