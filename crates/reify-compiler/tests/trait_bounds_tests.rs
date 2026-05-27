//! Trait bounds on type parameters — compilation tests.
//!
//! Tests for generic type parameters on traits and structures,
//! bound checking at instantiation, and default type parameter handling.

use reify_test_support::compile_source;
use reify_core::*;
use reify_ir::*;

// ── Step 1: generic trait stores type params ───────────────────────

#[test]
fn generic_trait_stores_type_params() {
    let source = "trait Container<T: Rigid> { param count : Int }";
    let module = compile_source(source);

    assert_eq!(module.trait_defs.len(), 1);
    let trait_def = &module.trait_defs[0];
    assert_eq!(trait_def.name, "Container");

    // Trait should have 1 type parameter
    assert_eq!(trait_def.type_params.len(), 1, "expected 1 type param");
    let tp = &trait_def.type_params[0];
    assert_eq!(tp.name, "T");

    // With one bound: Rigid
    assert_eq!(tp.bounds.len(), 1);
    assert_eq!(tp.bounds[0].trait_ref.name, "Rigid");
}

// ── Step 3: generic structure stores type params ───────────────────

#[test]
fn generic_structure_stores_type_params() {
    let source = "structure def Box<T: Rigid> { param width : Length = 10mm }";
    let module = compile_source(source);

    assert_eq!(module.templates.len(), 1);
    let template = &module.templates[0];
    assert_eq!(template.name, "Box");

    // Template should have 1 type parameter
    assert_eq!(template.type_params.len(), 1, "expected 1 type param");
    let tp = &template.type_params[0];
    assert_eq!(tp.name, "T");

    // With one bound: Rigid
    assert_eq!(tp.bounds.len(), 1);
    assert_eq!(tp.bounds[0].trait_ref.name, "Rigid");
}

// ── Step 5: type param as member type ───────────────────────────────

#[test]
fn type_param_as_member_type() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Box<T: Rigid> { param contents : T }
    "#;
    let module = compile_source(source);

    // Should compile without errors (T is a known type param, not an unknown type)
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    assert_eq!(module.templates.len(), 1);
    let template = &module.templates[0];
    assert_eq!(template.name, "Box");

    // The value cell for 'contents' should have type Type::TypeParam("T")
    let contents_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "contents")
        .expect("should have 'contents' value cell");
    assert_eq!(
        contents_cell.cell_type,
        Type::TypeParam("T".to_string()),
        "member typed with type param T should be Type::TypeParam(\"T\")"
    );
}

// ── Step 7: bound check valid type arg ──────────────────────────────

#[test]
fn bound_check_valid_type_arg() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 5kg }
        structure def Box<T: Rigid> { param width : Length = 10mm }
        structure def Assembly { sub part = Box<Bolt>() }
    "#;
    let module = compile_source(source);

    // No error diagnostics about bound violations
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for valid type arg, got: {:?}",
        errors
    );
}

// ── Step 9: bound check invalid type arg ────────────────────────────

#[test]
fn bound_check_invalid_type_arg() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Box<T: Rigid> { param width : Length = 10mm }
        structure def Widget { param x : Length = 5mm }
        structure def Assembly { sub part = Box<Widget>() }
    "#;
    let module = compile_source(source);

    // Should produce an error: Widget does not satisfy bound Rigid
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error about Widget not satisfying Rigid bound"
    );
    let error_msg = &errors[0].message;
    assert!(
        error_msg.contains("Widget") && error_msg.contains("Rigid"),
        "error should mention Widget and Rigid, got: {error_msg}"
    );
}

// ── Step 11: composite bounds both checked ──────────────────────────

#[test]
fn composite_bounds_both_checked() {
    let source = r#"
        trait A { param a : Length }
        trait B { param b : Length }
        structure def Container<T: A + B> { param width : Length = 10mm }
        structure def Full : A + B { param a : Length = 1mm  param b : Length = 2mm }
        structure def Partial : A { param a : Length = 1mm }
        structure def AsmOk { sub x = Container<Full>() }
        structure def AsmBad { sub y = Container<Partial>() }
    "#;
    let module = compile_source(source);

    // AsmOk should be fine (Full satisfies A + B)
    // AsmBad should error (Partial only satisfies A, not B)
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error about Partial not satisfying bound B"
    );
    // Should mention Partial and B
    let has_partial_b = errors
        .iter()
        .any(|e| e.message.contains("Partial") && e.message.contains("B"));
    assert!(
        has_partial_b,
        "expected error mentioning Partial and B, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    // Should NOT have errors about Full
    let has_full_error = errors.iter().any(|e| e.message.contains("Full"));
    assert!(
        !has_full_error,
        "Full satisfies A + B, should not have errors, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Step 13: default type param used when omitted ────────────────────

#[test]
fn default_type_param_used_when_omitted() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Steel : Rigid { param mass : Mass = 10kg }
        structure def Box<T: Rigid = Steel> { param width : Length = 10mm }
        structure def Assembly { sub part = Box() }
    "#;
    let module = compile_source(source);

    // Box() with no type args should use default Steel, which satisfies Rigid.
    // No errors expected.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when default type param satisfies bound, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

#[test]
fn missing_type_arg_no_default_errors() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Box<T: Rigid> { param width : Length = 10mm }
        structure def Assembly { sub part = Box() }
    "#;
    let module = compile_source(source);

    // Box() with no type args and no default on T should produce an error
    // about missing type argument.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error about missing type argument for T on Box"
    );
    let has_missing_arg = errors
        .iter()
        .any(|e| e.message.contains("missing") || e.message.contains("type argument"));
    assert!(
        has_missing_arg,
        "error should mention missing type argument, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Step 15: default type param overridden ───────────────────────────

#[test]
fn default_type_param_overridden() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Steel : Rigid { param mass : Mass = 10kg }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Box<T: Rigid = Steel> { param width : Length = 10mm }
        structure def Assembly { sub part = Box<Bolt>() }
    "#;
    let module = compile_source(source);

    // Box<Bolt>() overrides default Steel with Bolt, which also satisfies Rigid.
    // No errors expected.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when explicit type arg overrides default and satisfies bound, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Step 17: trait type params checked at conformance ────────────────

#[test]
fn trait_type_params_checked_at_conformance() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Widget { param x : Length = 5mm }
        trait Container<T: Rigid> { param count : Int }
        structure def Crate : Container<Bolt> { param count : Int = 5 }
        structure def Crate2 : Container<Widget> { param count : Int = 5 }
    "#;
    let module = compile_source(source);

    // Crate : Container<Bolt> should be fine — Bolt satisfies Rigid.
    // Crate2 : Container<Widget> should error — Widget doesn't satisfy Rigid.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error that Widget doesn't satisfy Rigid on Container's T"
    );
    let has_widget_rigid = errors
        .iter()
        .any(|e| e.message.contains("Widget") && e.message.contains("Rigid"));
    assert!(
        has_widget_rigid,
        "error should mention Widget and Rigid, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    // No error about Crate (Bolt satisfies Rigid)
    let has_bolt_error = errors
        .iter()
        .any(|e| e.message.contains("Bolt") && e.message.contains("does not satisfy"));
    assert!(
        !has_bolt_error,
        "Bolt satisfies Rigid, should not have errors, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Step 19: sub component stores type args ──────────────────────────

#[test]
fn sub_component_stores_type_args() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Box<T: Rigid> { param w : Length = 10mm }
        structure def Asm { sub part = Box<Bolt>() }
    "#;
    let module = compile_source(source);

    // No errors
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // Find Asm template
    let asm = module
        .templates
        .iter()
        .find(|t| t.name == "Asm")
        .expect("should have Asm template");
    assert_eq!(asm.sub_components.len(), 1);
    let sub = &asm.sub_components[0];
    assert_eq!(sub.name, "part");
    assert_eq!(sub.structure_name, "Box");

    // The sub component should store the resolved type args
    assert_eq!(
        sub.type_args.len(),
        1,
        "expected 1 type arg on sub component"
    );
    // Concrete structure name should be StructureRef, not TypeParam
    assert_eq!(
        sub.type_args[0],
        Type::StructureRef("Bolt".to_string()),
        "concrete structure name at instantiation site should be Type::StructureRef"
    );
}

// ── Step 21: forward reference order independence ──────────────────────

#[test]
fn forward_reference_order_independence() {
    // Structures defined in reverse dependency order:
    // Assembly references Box and Bolt, but they are defined AFTER Assembly.
    // AsmBad references Box<Widget> where Widget doesn't satisfy Rigid.
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Assembly { sub part = Box<Bolt>() }
        structure def AsmBad { sub part = Box<Widget>() }
        structure def Box<T: Rigid> { param width : Length = 10mm }
        structure def Bolt : Rigid { param mass : Mass = 5kg }
        structure def Widget { param x : Length = 5mm }
    "#;
    let module = compile_source(source);

    // Forward references must not produce false-positive bound errors:
    // Assembly's Box<Bolt> is valid, so no error there.
    // AsmBad's Box<Widget> is INVALID — Widget doesn't satisfy Rigid.
    // With correct deferred bound checking, we should get exactly one error
    // about Widget not satisfying Rigid, and NO false-positive errors
    // about Bolt.

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // Must have an error about Widget not satisfying Rigid
    let has_widget_error = errors
        .iter()
        .any(|e| e.message.contains("Widget") && e.message.contains("Rigid"));
    assert!(
        has_widget_error,
        "expected error that Widget doesn't satisfy Rigid on forward-declared Box's T, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // Must NOT have false-positive errors about Bolt
    let has_bolt_error = errors
        .iter()
        .any(|e| e.message.contains("Bolt") && e.message.contains("does not satisfy"));
    assert!(
        !has_bolt_error,
        "Bolt satisfies Rigid, should not have errors, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Step 23: supertrait chain satisfies bound ──────────────────────────

#[test]
fn supertrait_chain_satisfies_bound() {
    // ConcreteRigid refines Rigid (syntax: `: Rigid`). Bolt : ConcreteRigid.
    // Box<T: Rigid> — Bolt should transitively satisfy Rigid through the
    // refinement chain ConcreteRigid -> Rigid.
    let source = r#"
        trait Rigid { param mass : Mass }
        trait ConcreteRigid : Rigid { param density : Real }
        structure def Bolt : ConcreteRigid {
            param mass : Mass = 1kg
            param density : Real = 7800
        }
        structure def Box<T: Rigid> { param width : Length = 10mm }
        structure def Assembly { sub part = Box<Bolt>() }
    "#;
    let module = compile_source(source);

    // Bolt -> ConcreteRigid -> Rigid, so Bolt satisfies Rigid transitively.
    // No errors expected.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when supertrait chain satisfies bound, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Step 25: type param forwarded as TypeParam ──────────────────────────

#[test]
fn type_param_forwarded_as_type_param() {
    // Unresolved type variables inside generic definitions (Box<U>())
    // should remain Type::TypeParam.
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Box<T: Rigid> { param w : Length = 10mm }
        structure def Wrapper<U: Rigid> { sub inner = Box<U>() }
    "#;
    let module = compile_source(source);

    let wrapper = module
        .templates
        .iter()
        .find(|t| t.name == "Wrapper")
        .expect("should have Wrapper template");
    let sub = &wrapper.sub_components[0];
    assert_eq!(sub.type_args.len(), 1);

    // Unresolved type variable should be TypeParam, not StructureRef
    assert_eq!(
        sub.type_args[0],
        Type::TypeParam("U".to_string()),
        "unresolved type variable inside generic def should be Type::TypeParam"
    );
}

// ── Diamond refinement (B3 fix validation) ──────────────────────────

#[test]
fn diamond_refinement_satisfies_bound() {
    // D (base), B : D, C : D, A : B + C
    // Structure Obj : A. Box<T: D> with Box<Obj>() should compile.
    let source = r#"
        trait D { param d : Int }
        trait B : D { param b : Int }
        trait C : D { param c : Int }
        trait A : B + C { param a : Int }
        structure def Obj : A {
            param d : Int = 1
            param b : Int = 2
            param c : Int = 3
            param a : Int = 4
        }
        structure def Box<T: D> { param w : Length = 10mm }
        structure def Asm { sub part = Box<Obj>() }
    "#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

// ── Multiple independent type params ─────────────────────────────────

#[test]
fn multiple_independent_type_params() {
    let source = r#"
        trait A { param a : Int }
        trait B { param b : Int }
        structure def GoodA : A { param a : Int = 1 }
        structure def GoodB : B { param b : Int = 1 }
        structure def BadB { param x : Int = 1 }
        structure def Pair<T: A, U: B> { param w : Length = 10mm }
        structure def Ok { sub p = Pair<GoodA, GoodB>() }
        structure def Bad { sub p = Pair<GoodA, BadB>() }
    "#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error, got: {:?}",
        errors
    );
    let msg = &errors[0].message;
    assert!(msg.contains("BadB"), "error should mention BadB: {}", msg);
    assert!(msg.contains("B"), "error should mention trait B: {}", msg);
}

// ── Arity mismatch ──────────────────────────────────────────────────

#[test]
fn too_many_type_args_errors() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Steel : Rigid { param mass : Mass = 10kg }
        structure def Box<T: Rigid> { param w : Length = 10mm }
        structure def Asm { sub part = Box<Bolt, Steel>() }
    "#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("too many type arguments")),
        "expected arity error, got: {:?}",
        errors
    );
}

// ── Generic forwarding no false positive (B1 fix validation) ────────

#[test]
fn generic_forwarding_no_false_positive() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Box<T: Rigid> { param w : Length = 10mm }
        structure def Wrapper<U: Rigid> { sub inner = Box<U>() }
    "#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for generic forwarding, got: {:?}",
        errors
    );
}

// ── Both PendingBoundCheck paths in a single compilation ─────────────

#[test]
fn both_bound_check_paths_combined() {
    // This test exercises both deferred-checking paths in a single compilation:
    // 1. TraitConformance path: `Crate : Container<Bolt>` — trait with type params
    // 2. SubComponent path: `sub part = Box<Bolt>()` — generic structure instantiation
    // Both should succeed without errors when Bolt satisfies Rigid.
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        trait Container<T: Rigid> { param count : Int }
        structure def Box<T: Rigid> { param w : Length = 10mm }
        structure def Assembly : Container<Bolt> {
            param count : Int = 3
            sub part = Box<Bolt>()
        }
    "#;
    let module = compile_source(source);

    // Both paths should succeed — Bolt satisfies Rigid.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when both trait-conformance and sub-component \
         bound checks pass, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── SubComponent path error with forward reference ───────────────────

#[test]
fn sub_component_bound_error_with_forward_ref() {
    // Validates the SubComponent enum variant correctly resolves type_params
    // from the template registry during the post-pass, even when the generic
    // structure is defined after the structure that uses it (forward reference).
    // Widget doesn't satisfy Rigid, so Box<Widget>() must error.
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Widget { param x : Length = 5mm }
        structure def Assembly { sub part = Box<Widget>() }
        structure def Box<T: Rigid> { param width : Length = 10mm }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error about Widget not satisfying Rigid bound"
    );
    let error_msg = &errors[0].message;
    assert!(
        error_msg.contains("Widget") && error_msg.contains("Rigid"),
        "error should mention Widget and Rigid, got: {error_msg}"
    );
}
