//! Trait conformance compilation tests.
//!
//! Tests for compiling trait declarations, conformance checking,
//! default merging, and composition conflict detection.

use reify_compiler::*;
use reify_types::*;

// Helper to create a minimal CompiledTrait with no required members.
fn empty_trait(name: &str) -> CompiledTrait {
    CompiledTrait {
        name: name.to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![],
        defaults: vec![],
        content_hash: ContentHash::of_str(name),
    }
}

// Helper to create a span for tests.
fn test_span() -> SourceSpan {
    SourceSpan { start: 0, end: 0 }
}

/// step-1: empty trait → no errors.
#[test]
fn conformance_empty_trait_no_errors() {
    let trait_def = empty_trait("Empty");
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

/// step-3: missing required param → MissingParam error.
#[test]
fn conformance_missing_param_error() {
    let trait_def = CompiledTrait {
        name: "HasWidth".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "width".to_string(),
            kind: RequirementKind::Param(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("HasWidth"),
    };
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingParam { name, expected_type } => {
            assert_eq!(name, "width");
            assert_eq!(
                *expected_type,
                Type::Scalar { dimension: DimensionVector::LENGTH }
            );
        }
        other => panic!("expected MissingParam, got: {:?}", other),
    }
}

/// step-5: param with wrong type → TypeMismatch error.
#[test]
fn conformance_type_mismatch_error() {
    let trait_def = CompiledTrait {
        name: "Weighted".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "mass".to_string(),
            kind: RequirementKind::Param(Type::Scalar {
                dimension: DimensionVector::MASS,
            }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("Weighted"),
    };
    // Provide 'mass' but with Length instead of Mass.
    let mut structure_members = std::collections::HashMap::new();
    structure_members.insert(
        "mass".to_string(),
        Type::Scalar { dimension: DimensionVector::LENGTH },
    );
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::TypeMismatch { name, expected_type, actual_type } => {
            assert_eq!(name, "mass");
            assert_eq!(*expected_type, Type::Scalar { dimension: DimensionVector::MASS });
            assert_eq!(*actual_type, Type::Scalar { dimension: DimensionVector::LENGTH });
        }
        other => panic!("expected TypeMismatch, got: {:?}", other),
    }
}

/// step-7: satisfied param → no errors (happy path).
#[test]
fn conformance_satisfied_param_no_errors() {
    let trait_def = CompiledTrait {
        name: "HasWidth".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "width".to_string(),
            kind: RequirementKind::Param(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("HasWidth"),
    };
    let mut structure_members = std::collections::HashMap::new();
    structure_members.insert(
        "width".to_string(),
        Type::Scalar { dimension: DimensionVector::LENGTH },
    );
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

/// step-8: multiple requirements — one satisfied, one mismatched, one missing.
#[test]
fn conformance_multiple_requirements_mixed() {
    let trait_def = CompiledTrait {
        name: "Complex".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![
            TraitRequirement {
                name: "width".to_string(),
                kind: RequirementKind::Param(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
                span: test_span(),
            },
            TraitRequirement {
                name: "mass".to_string(),
                kind: RequirementKind::Param(Type::Scalar {
                    dimension: DimensionVector::MASS,
                }),
                span: test_span(),
            },
            TraitRequirement {
                name: "name".to_string(),
                kind: RequirementKind::Param(Type::String),
                span: test_span(),
            },
        ],
        defaults: vec![],
        content_hash: ContentHash::of_str("Complex"),
    };
    // 'width' correct, 'mass' wrong type (Length not Mass), 'name' missing.
    let mut structure_members = std::collections::HashMap::new();
    structure_members.insert(
        "width".to_string(),
        Type::Scalar { dimension: DimensionVector::LENGTH },
    );
    structure_members.insert(
        "mass".to_string(),
        Type::Scalar { dimension: DimensionVector::LENGTH },
    );
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert_eq!(errors.len(), 2, "expected 2 errors, got: {:?}", errors);

    let has_type_mismatch = errors.iter().any(|e| matches!(
        e,
        ConformanceError::TypeMismatch { name, .. } if name == "mass"
    ));
    let has_missing_param = errors.iter().any(|e| matches!(
        e,
        ConformanceError::MissingParam { name, .. } if name == "name"
    ));
    assert!(has_type_mismatch, "expected TypeMismatch for 'mass', errors: {:?}", errors);
    assert!(has_missing_param, "expected MissingParam for 'name', errors: {:?}", errors);
}

/// step-9: Let requirement missing → MissingLet error.
#[test]
fn conformance_let_requirement_checked() {
    let trait_def = CompiledTrait {
        name: "HasArea".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "area".to_string(),
            kind: RequirementKind::Let(Type::Real),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("HasArea"),
    };
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingLet { name, expected_type } => {
            assert_eq!(name, "area");
            assert_eq!(*expected_type, Type::Real);
        }
        other => panic!("expected MissingLet, got: {:?}", other),
    }
}

/// step-11: exact dimensional type equality — Scalar{LENGTH} ≠ Scalar{MASS}.
#[test]
fn conformance_exact_type_equality_dimensions() {
    let trait_def = CompiledTrait {
        name: "HasLength".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "length".to_string(),
            kind: RequirementKind::Param(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("HasLength"),
    };

    // Wrong dimension → TypeMismatch.
    {
        let mut structure_members = std::collections::HashMap::new();
        structure_members.insert(
            "length".to_string(),
            Type::Scalar { dimension: DimensionVector::MASS },
        );
        let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
        assert_eq!(errors.len(), 1, "expected 1 error for wrong dimension, got: {:?}", errors);
        assert!(
            matches!(&errors[0], ConformanceError::TypeMismatch { name, .. } if name == "length"),
            "expected TypeMismatch, got: {:?}",
            errors
        );
    }

    // Correct dimension → no errors.
    {
        let mut structure_members = std::collections::HashMap::new();
        structure_members.insert(
            "length".to_string(),
            Type::Scalar { dimension: DimensionVector::LENGTH },
        );
        let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
        assert!(errors.is_empty(), "expected no errors for correct dimension, got: {:?}", errors);
    }
}

/// Helper: parse source and compile, returning the CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: parse source and compile, returning first template + diagnostics.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected 1 template");
    (template, module.diagnostics)
}

/// Step 1: Compile a trait declaration produces CompiledTrait in CompiledModule.trait_defs.
#[test]
fn compile_trait_produces_compiled_trait() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}
"#;

    let module = compile_module(source);

    // Should have 1 trait def
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait def");
    let trait_def = &module.trait_defs[0];

    // Name should be "Fastener"
    assert_eq!(trait_def.name, "Fastener");

    // Should have 1 required member named "thread_pitch"
    assert_eq!(trait_def.required_members.len(), 1, "expected 1 required member");
    let req = &trait_def.required_members[0];
    assert_eq!(req.name, "thread_pitch");

    // Requirement kind should be Param with type Scalar{LENGTH}
    match &req.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(*ty, Type::Scalar { dimension: DimensionVector::LENGTH });
        }
        other => panic!("expected RequirementKind::Param, got {:?}", other),
    }
}

/// Step 3: Simple conformance — structure satisfies trait requirement.
#[test]
fn simple_conformance_no_errors() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}

structure def Bolt : Fastener {
    param thread_pitch : Length = 20mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// Step 15: Diamond inheritance — requirement from C reachable via both A and B.
#[test]
fn diamond_inheritance_deduplication() {
    let source = r#"
trait C {
    param x : Length
}

trait A : C {
}

trait B : C {
}

structure def X : A + B {
    param x : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// Step 5: Missing member — error diagnostic about missing required member.
#[test]
fn missing_member_error() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}

structure def Bolt : Fastener {
    param length : Length = 10mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected error diagnostic for missing member");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("missing required member") && error_msg.contains("thread_pitch"),
        "error should mention 'missing required member' and 'thread_pitch', got: {}",
        error_msg
    );
}

/// Step 7: Type mismatch — member has wrong type.
#[test]
fn type_mismatch_error() {
    let source = r#"
trait Weighted {
    param mass : Mass
}

structure def S : Weighted {
    param mass : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected error diagnostic for type mismatch");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("type mismatch"),
        "error should mention 'type mismatch', got: {}",
        error_msg
    );
}

/// Step 9: Default merging — trait provides default, structure doesn't override.
#[test]
fn default_merging_injects_value_cell() {
    let source = r#"
trait HasSize {
    param size : Length = 10mm
}

structure def S : HasSize {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The template should contain a value cell for 'size' injected from the trait default.
    let size_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "size");
    assert!(
        size_cell.is_some(),
        "expected 'size' value cell from trait default, got cells: {:?}",
        template.value_cells.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    let size_cell = size_cell.unwrap();
    assert_eq!(size_cell.kind, ValueCellKind::Param);
    assert_eq!(
        size_cell.cell_type,
        Type::Scalar { dimension: DimensionVector::LENGTH }
    );
    assert!(size_cell.default_expr.is_some(), "expected default expression for 'size'");
}

/// Step 11: Default override — structure provides its own value, no error, only one cell.
#[test]
fn default_override_uses_structure_value() {
    let source = r#"
trait HasSize {
    param size : Length = 10mm
}

structure def S : HasSize {
    param size : Length = 20mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Only one 'size' value cell should exist (the structure's, not the trait default).
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(
        size_cells.len(),
        1,
        "expected exactly 1 'size' value cell, got {}",
        size_cells.len()
    );
}

/// Step 13: Multiple trait bounds — structure satisfies both traits.
#[test]
fn multiple_trait_bounds_satisfied() {
    let source = r#"
trait A {
    param a : Length
}

trait B {
    param b : Length
}

structure def X : A + B {
    param a : Length = 1mm
    param b : Length = 2mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors for multi-trait: {:?}", errors);
}

/// Step 17: Composition conflict — same name, different types across traits.
#[test]
fn composition_conflict_error() {
    let source = r#"
trait A {
    param size : Length
}

trait B {
    param size : Mass
}

structure def X : A + B {
    param size : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected error for conflicting requirements");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        error_msg
    );
}

/// Step 19: Deep trait chain — C→B→A, structure must satisfy all.
#[test]
fn deep_trait_chain() {
    let source = r#"
trait A {
    param x : Length
}

trait B : A {
    param y : Length
}

trait C : B {
    param z : Length
}

structure def S : C {
    param x : Length = 1mm
    param y : Length = 2mm
    param z : Length = 3mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors for deep chain: {:?}", errors);
}

/// Step 21: Constraint from trait — default constraint is injected.
#[test]
fn constraint_from_trait_injected() {
    let source = r#"
trait Safe {
    param x : Length
    constraint x > 0mm
}

structure def S : Safe {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The constraint from the trait should be injected
    assert!(
        !template.constraints.is_empty(),
        "expected at least 1 constraint from trait default"
    );
}

/// Step 23: Duplicate default injection — two distinct traits with same-named default param.
/// Currently `collect_all_requirements` pushes defaults unconditionally, producing TWO
/// ValueCellDecl entries for 'size'. Test asserts exactly one 'size' value cell exists.
#[test]
fn duplicate_default_injection_deduped() {
    let source = r#"
trait A {
    param size : Length = 10mm
}

trait B {
    param size : Length = 5mm
}

structure def X : A + B {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected (same name + same type → dedup, not conflict).
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly one 'size' value cell should exist (not two).
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(
        size_cells.len(),
        1,
        "expected exactly 1 'size' value cell after dedup, got {}",
        size_cells.len()
    );
}

/// Step 25a: Default conflict across traits with different types.
/// Two traits provide defaults for 'size' with different types → conflict diagnostic.
#[test]
fn default_conflict_different_types() {
    let source = r#"
trait A {
    param size : Length = 10mm
}

trait B {
    param size : Mass = 5kg
}

structure def X : A + B {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected conflict diagnostic");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting") && error_msg.contains("size"),
        "error should mention 'conflicting' and 'size', got: {}",
        error_msg
    );
}

/// Step 25b: Default conflict resolution — structure overrides the conflicting default.
/// When the structure provides its own member, the conflict is moot — no diagnostic.
#[test]
fn default_conflict_resolved_by_override() {
    let source = r#"
trait A {
    param size : Length = 10mm
}

trait B {
    param size : Mass = 5kg
}

structure def Y : A + B {
    param size : Length = 7mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics — the structure provides 'size', resolving the conflict.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors when structure overrides: {:?}", errors);

    // Only one 'size' value cell.
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(size_cells.len(), 1, "expected exactly 1 'size' value cell");
}

/// Step 27a: Unlabeled constraint defaults from two traits — both injected.
/// Since labeled constraints are not yet supported in the grammar (label is always None),
/// unlabeled constraints from distinct traits are both injected (no dedup for unnamed).
#[test]
fn unlabeled_constraint_defaults_from_two_traits() {
    let source = r#"
trait A {
    param x : Length
    constraint x > 0mm
}

trait B {
    param x : Length
    constraint x > 0mm
}

structure def X : A + B {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Both unlabeled constraints are injected (unnamed defaults always push).
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints from two traits, got {}",
        template.constraints.len()
    );
}

/// Step 27b: Structure provides its own constraint — trait constraints still injected
/// (since all are unlabeled and there's no label-based override).
#[test]
fn structure_constraint_with_trait_constraints() {
    let source = r#"
trait A {
    param x : Length
    constraint x > 0mm
}

structure def X : A {
    param x : Length = 5mm
    constraint x > 1mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Structure's constraint + trait's unlabeled constraint = at least 2.
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints (structure + trait), got {}",
        template.constraints.len()
    );
}

/// Step 21b: Trait with constraint and param — both injected correctly.
#[test]
fn trait_constraint_and_param_both_injected() {
    let source = r#"
trait Safe {
    param x : Length = 5mm
    constraint x > 0mm
}

structure def S : Safe {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Both param default and constraint should be injected.
    let has_x = template.value_cells.iter().any(|vc| vc.id.member == "x");
    assert!(has_x, "expected value cell 'x' from trait default");

    assert!(
        !template.constraints.is_empty(),
        "expected constraint from trait default"
    );
}

// ── Port conformance unit tests ─────────────────────────────────────────────

fn make_port_trait(
    trait_name: &str,
    port_name: &str,
    type_name: &str,
    direction: reify_types::PortDirection,
) -> CompiledTrait {
    CompiledTrait {
        name: trait_name.to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: port_name.to_string(),
            kind: RequirementKind::Port {
                type_name: type_name.to_string(),
                direction,
            },
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str(trait_name),
    }
}

/// step-1: MissingPort — trait requires port 'input : Signal in', structure has no ports → MissingPort.
#[test]
fn conformance_missing_port_error() {
    let trait_def = make_port_trait("HasInput", "input", "Signal", reify_types::PortDirection::In);
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingPort { name, expected_type, expected_direction } => {
            assert_eq!(name, "input");
            assert_eq!(expected_type, "Signal");
            assert_eq!(*expected_direction, reify_types::PortDirection::In);
        }
        other => panic!("expected MissingPort, got: {:?}", other),
    }
}

/// step-3: PortTypeMismatch — trait requires port 'mount : MountInterface', structure has 'mount : OtherInterface'.
#[test]
fn conformance_port_type_mismatch_error() {
    let trait_def =
        make_port_trait("HasMount", "mount", "MountInterface", reify_types::PortDirection::In);
    let ports = vec![PortInfo {
        name: "mount".to_string(),
        type_name: "OtherInterface".to_string(),
        direction: reify_types::PortDirection::In,
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &ports, &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::PortTypeMismatch { name, expected_type, actual_type } => {
            assert_eq!(name, "mount");
            assert_eq!(expected_type, "MountInterface");
            assert_eq!(actual_type, "OtherInterface");
        }
        other => panic!("expected PortTypeMismatch, got: {:?}", other),
    }
}

/// step-5: PortDirectionMismatch — trait requires port 'output : Signal out', structure has 'output : Signal in'.
#[test]
fn conformance_port_direction_mismatch_error() {
    let trait_def =
        make_port_trait("HasOutput", "output", "Signal", reify_types::PortDirection::Out);
    let ports = vec![PortInfo {
        name: "output".to_string(),
        type_name: "Signal".to_string(),
        direction: reify_types::PortDirection::In,
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &ports, &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::PortDirectionMismatch {
            name,
            expected_direction,
            actual_direction,
        } => {
            assert_eq!(name, "output");
            assert_eq!(*expected_direction, reify_types::PortDirection::Out);
            assert_eq!(*actual_direction, reify_types::PortDirection::In);
        }
        other => panic!("expected PortDirectionMismatch, got: {:?}", other),
    }
}

/// step-7: port fully satisfied — trait requires 'input : Signal in', structure has matching port → no errors.
#[test]
fn conformance_port_fully_satisfied() {
    let trait_def = make_port_trait("HasInput", "input", "Signal", reify_types::PortDirection::In);
    let ports = vec![PortInfo {
        name: "input".to_string(),
        type_name: "Signal".to_string(),
        direction: reify_types::PortDirection::In,
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &ports, &[]);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

// ── Sub conformance unit tests ───────────────────────────────────────────────

fn make_sub_trait(trait_name: &str, sub_name: &str, required_trait: &str) -> CompiledTrait {
    CompiledTrait {
        name: trait_name.to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: sub_name.to_string(),
            kind: RequirementKind::Sub(required_trait.to_string()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str(trait_name),
    }
}

/// step-8: MissingSub — trait requires sub 'hole = ScrewHole()', structure has no subs → MissingSub.
#[test]
fn conformance_missing_sub_error() {
    // "ScrewHole" is the required structure name (as compile_trait would store it).
    let trait_def = make_sub_trait("HasHole", "hole", "ScrewHole");
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingSub { name, expected_structure } => {
            assert_eq!(name, "hole");
            assert_eq!(expected_structure, "ScrewHole");
        }
        other => panic!("expected MissingSub, got: {:?}", other),
    }
}

/// step-10: SubStructureMismatch — trait requires sub 'mount = MountBracket()',
/// structure has sub 'mount' with structure_name 'Bracket' (wrong structure).
#[test]
fn conformance_sub_structure_mismatch_error() {
    let trait_def = make_sub_trait("HasMount", "mount", "MountBracket");
    let subs = vec![SubInfo {
        name: "mount".to_string(),
        structure_name: "Bracket".to_string(), // wrong structure name
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &subs);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::SubStructureMismatch { name, expected_structure, actual_structure } => {
            assert_eq!(name, "mount");
            assert_eq!(expected_structure, "MountBracket");
            assert_eq!(actual_structure, "Bracket");
        }
        other => panic!("expected SubStructureMismatch, got: {:?}", other),
    }
}

/// step-12: sub fully satisfied — trait requires sub 'hole = ScrewHole()',
/// structure has sub 'hole' with structure_name 'ScrewHole' → no errors.
#[test]
fn conformance_sub_fully_satisfied() {
    let trait_def = make_sub_trait("HasHole", "hole", "ScrewHole");
    let subs = vec![SubInfo {
        name: "hole".to_string(),
        structure_name: "ScrewHole".to_string(), // matches required structure name
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &subs);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

/// step-20: multiple port requirements — one satisfied, one missing → exactly one error.
#[test]
fn conformance_multiple_port_requirements_one_missing() {
    let trait_def = CompiledTrait {
        name: "HasTwoPorts".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![
            TraitRequirement {
                name: "input".to_string(),
                kind: RequirementKind::Port {
                    type_name: "Signal".to_string(),
                    direction: reify_types::PortDirection::In,
                },
                span: test_span(),
            },
            TraitRequirement {
                name: "output".to_string(),
                kind: RequirementKind::Port {
                    type_name: "Signal".to_string(),
                    direction: reify_types::PortDirection::Out,
                },
                span: test_span(),
            },
        ],
        defaults: vec![],
        content_hash: ContentHash::of_str("HasTwoPorts"),
    };
    // Only provide 'input', not 'output'.
    let ports = vec![PortInfo {
        name: "input".to_string(),
        type_name: "Signal".to_string(),
        direction: reify_types::PortDirection::In,
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &ports, &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    assert!(
        matches!(&errors[0], ConformanceError::MissingPort { name, .. } if name == "output"),
        "expected MissingPort for 'output', got: {:?}",
        errors
    );
}

// ── compile_trait Port handling (step-13) ───────────────────────────────────

/// step-13: compile_trait handles MemberDecl::Port — parse a trait with
/// 'port input : in Signal', verify compiled trait has RequirementKind::Port
/// with correct type_name="Signal" and direction=In.
#[test]
fn compile_trait_handles_port_member() {
    let source = r#"
trait HasInput {
    port input : in Signal
}
"#;
    let module = compile_module(source);
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait def");
    let trait_def = &module.trait_defs[0];
    assert_eq!(trait_def.name, "HasInput");
    assert_eq!(
        trait_def.required_members.len(),
        1,
        "expected 1 required member (port), got: {:?}",
        trait_def.required_members
    );
    let req = &trait_def.required_members[0];
    assert_eq!(req.name, "input");
    match &req.kind {
        RequirementKind::Port { type_name, direction } => {
            assert_eq!(type_name, "Signal");
            assert_eq!(*direction, reify_types::PortDirection::In);
        }
        other => panic!("expected RequirementKind::Port, got: {:?}", other),
    }
}

/// step-13b: compile_trait handles port with direction Out.
#[test]
fn compile_trait_handles_port_member_out() {
    let source = r#"
trait HasOutput {
    port output : out Data
}
"#;
    let module = compile_module(source);
    let trait_def = &module.trait_defs[0];
    let req = &trait_def.required_members[0];
    assert_eq!(req.name, "output");
    match &req.kind {
        RequirementKind::Port { type_name, direction } => {
            assert_eq!(type_name, "Data");
            assert_eq!(*direction, reify_types::PortDirection::Out);
        }
        other => panic!("expected RequirementKind::Port, got: {:?}", other),
    }
}

// ── Integration tests: port conformance ─────────────────────────────────────

/// step-15: structure with matching port satisfies trait port requirement → no errors.
#[test]
fn integration_port_satisfied_no_errors() {
    let source = r#"
trait HasInput {
    port input : in Signal
}

structure def Receiver : HasInput {
    port input : in Signal
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// step-16: structure missing required port → error diagnostic mentioning missing port.
#[test]
fn integration_port_missing_error() {
    let source = r#"
trait HasInput {
    port input : in Signal
}

structure def Transmitter : HasInput {
    param x : Length = 1mm
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(!errors.is_empty(), "expected error for missing port");
    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("input"),
        "error should mention 'input', got: {}",
        error_msg
    );
}

// ── Integration tests: sub conformance ──────────────────────────────────────

/// step-18: structure with sub satisfying trait sub requirement → no errors.
#[test]
fn integration_sub_satisfied_no_errors() {
    let source = r#"
trait HasFastener {
    sub bolt = BoltSet()
}

structure def BoltSet {
    param count : Int = 4
}

structure def Assembly : HasFastener {
    sub bolt = BoltSet()
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// step-19: structure missing required sub → error diagnostic mentioning missing sub.
#[test]
fn integration_sub_missing_error() {
    let source = r#"
trait HasFastener {
    sub bolt = BoltSet()
}

structure def BoltSet {
    param count : Int = 4
}

structure def BadAssembly : HasFastener {
    param x : Length = 1mm
}
"#;
    let (_, diagnostics) = compile_first_template(source);
    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(!errors.is_empty(), "expected error for missing sub");
    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("bolt"),
        "error should mention 'bolt', got: {}",
        error_msg
    );
}

// ── Review fix: Sub semantic contract (steps 21-24) ─────────────────────────

/// step-21: expose the semantic mismatch — RequirementKind::Sub stores a structure
/// name (as compile_trait produces), but the current implementation checks
/// trait_bounds instead of structure_name. A SubInfo with structure_name == the
/// required name but empty trait_bounds should produce no errors after the fix.
///
/// Currently FAILS: returns SubTraitNotSatisfied because trait_bounds doesn't
/// contain "BoltSet". After step-22 fix, returns no errors.
#[test]
fn conformance_sub_structure_name_match_no_error() {
    // make_sub_trait passes "BoltSet" as the second argument to RequirementKind::Sub —
    // exactly what compile_trait() would do for `sub bolt = BoltSet()`.
    let trait_def = make_sub_trait("HasBolt", "bolt", "BoltSet");
    let subs = vec![SubInfo {
        name: "bolt".to_string(),
        structure_name: "BoltSet".to_string(),
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, &trait_def, &[], &subs);
    // Should be satisfied: sub.structure_name == RequirementKind::Sub value "BoltSet".
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

/// step-23: verify compile_trait → check_trait_conformance agreement.
/// Parse a trait with 'sub bolt = BoltSet()', compile it via compile_module,
/// extract RequirementKind::Sub and verify it contains "BoltSet" (structure name),
/// then pass to check_trait_conformance with SubInfo { name: "bolt", structure_name: "BoltSet" }
/// → no errors. Proves both code paths agree on structure-name semantics.
#[test]
fn compile_trait_sub_check_conformance_agreement() {
    let source = r#"
trait HasBolt {
    sub bolt = BoltSet()
}
"#;
    let module = compile_module(source);
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait def");
    let trait_def = &module.trait_defs[0];
    assert_eq!(trait_def.name, "HasBolt");
    assert_eq!(
        trait_def.required_members.len(),
        1,
        "expected 1 required member (sub), got: {:?}",
        trait_def.required_members
    );
    let req = &trait_def.required_members[0];
    assert_eq!(req.name, "bolt");
    // Verify compile_trait stores structure name "BoltSet", not a trait name.
    match &req.kind {
        RequirementKind::Sub(structure_name) => {
            assert_eq!(structure_name, "BoltSet", "Sub should store structure name");
        }
        other => panic!("expected RequirementKind::Sub, got: {:?}", other),
    }
    // Now verify check_trait_conformance agrees: SubInfo with structure_name="BoltSet" → no errors.
    let subs = vec![SubInfo {
        name: "bolt".to_string(),
        structure_name: "BoltSet".to_string(),
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, trait_def, &[], &subs);
    assert!(
        errors.is_empty(),
        "compile_trait + check_trait_conformance should agree — no errors, got: {:?}",
        errors
    );
}

/// step-24: structure-name mismatch via the compile_trait path.
/// Compile a trait with 'sub bolt = BoltSet()', check_trait_conformance with
/// SubInfo { name: "bolt", structure_name: "WrongStructure" }
/// → ConformanceError::SubStructureMismatch { expected_structure: "BoltSet", actual_structure: "WrongStructure" }.
#[test]
fn compile_trait_sub_structure_mismatch() {
    let source = r#"
trait HasBolt {
    sub bolt = BoltSet()
}
"#;
    let module = compile_module(source);
    let trait_def = &module.trait_defs[0];
    // Sub with wrong structure name.
    let subs = vec![SubInfo {
        name: "bolt".to_string(),
        structure_name: "WrongStructure".to_string(),
    }];
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors = check_trait_conformance(&structure_members, trait_def, &[], &subs);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::SubStructureMismatch { name, expected_structure, actual_structure } => {
            assert_eq!(name, "bolt");
            assert_eq!(expected_structure, "BoltSet");
            assert_eq!(actual_structure, "WrongStructure");
        }
        other => panic!("expected SubStructureMismatch, got: {:?}", other),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Task 186 — Trait conformance: refinement chains
// ──────────────────────────────────────────────────────────────────────────────

/// step-1 (task-186): chain with no refinements, structure missing param → MissingParam.
#[test]
fn chain_single_trait_no_refinement() {
    let trait_def = CompiledTrait {
        name: "HasWidth".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "width".to_string(),
            kind: RequirementKind::Param(Type::Scalar { dimension: DimensionVector::LENGTH }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("HasWidthChain"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("HasWidth".to_string(), &trait_def);
    let structure_members: std::collections::HashMap<String, Type> =
        std::collections::HashMap::new();
    let errors =
        check_trait_conformance_chain(&structure_members, "HasWidth", &registry, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error, got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingParam { name, .. } => assert_eq!(name, "width"),
        other => panic!("expected MissingParam, got: {:?}", other),
    }
}

/// step-3 (task-186): two-level chain — trait A refines B; B requires `width`, A requires `height`.
/// Structure provides `height` but not `width` → MissingParam for `width`.
/// Verifies the chain walker visits parent traits' requirements.
#[test]
fn chain_two_level_walks_parent() {
    // Trait B: requires `width : Length`.
    let trait_b = CompiledTrait {
        name: "B".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "width".to_string(),
            kind: RequirementKind::Param(Type::Scalar { dimension: DimensionVector::LENGTH }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("B"),
    };
    // Trait A: refines B, requires `height : Length`.
    let trait_a = CompiledTrait {
        name: "A".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["B".to_string()],
        required_members: vec![TraitRequirement {
            name: "height".to_string(),
            kind: RequirementKind::Param(Type::Scalar { dimension: DimensionVector::LENGTH }),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("A"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("A".to_string(), &trait_a);
    registry.insert("B".to_string(), &trait_b);
    // Structure has `height` but not `width`.
    let mut members = std::collections::HashMap::new();
    members.insert("height".to_string(), Type::Scalar { dimension: DimensionVector::LENGTH });
    let errors = check_trait_conformance_chain(&members, "A", &registry, &[], &[]);
    assert_eq!(errors.len(), 1, "expected 1 error (missing 'width'), got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingParam { name, .. } => assert_eq!(name, "width"),
        other => panic!("expected MissingParam for 'width', got: {:?}", other),
    }
}

/// step-5 (task-186): three-level chain C→B→A all satisfied.
/// C requires `x`, B:C requires `y`, A:B requires `z`.
/// Structure provides all three → no errors.
#[test]
fn chain_three_level_all_satisfied() {
    let length = || Type::Scalar { dimension: DimensionVector::LENGTH };
    let trait_c = CompiledTrait {
        name: "C".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("C"),
    };
    let trait_b = CompiledTrait {
        name: "B".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string()],
        required_members: vec![TraitRequirement {
            name: "y".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("B"),
    };
    let trait_a = CompiledTrait {
        name: "A".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["B".to_string()],
        required_members: vec![TraitRequirement {
            name: "z".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("A"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("A".to_string(), &trait_a);
    registry.insert("B".to_string(), &trait_b);
    registry.insert("C".to_string(), &trait_c);
    let mut members = std::collections::HashMap::new();
    members.insert("x".to_string(), length());
    members.insert("y".to_string(), length());
    members.insert("z".to_string(), length());
    let errors = check_trait_conformance_chain(&members, "A", &registry, &[], &[]);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

/// step-7 (task-186): deep ancestor missing — A:B:C, C requires `base : Length`.
/// Structure provides A's and B's members but not `base` → exactly 1 MissingParam.
#[test]
fn chain_missing_deep_ancestor_member() {
    let length = || Type::Scalar { dimension: DimensionVector::LENGTH };
    let trait_c = CompiledTrait {
        name: "C".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "base".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("C2"),
    };
    let trait_b = CompiledTrait {
        name: "B".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string()],
        required_members: vec![TraitRequirement {
            name: "b_param".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("B2"),
    };
    let trait_a = CompiledTrait {
        name: "A".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["B".to_string()],
        required_members: vec![TraitRequirement {
            name: "a_param".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("A2"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("A".to_string(), &trait_a);
    registry.insert("B".to_string(), &trait_b);
    registry.insert("C".to_string(), &trait_c);
    // Provide A and B's members but NOT `base` from C.
    let mut members = std::collections::HashMap::new();
    members.insert("a_param".to_string(), length());
    members.insert("b_param".to_string(), length());
    let errors = check_trait_conformance_chain(&members, "A", &registry, &[], &[]);
    assert_eq!(errors.len(), 1, "expected exactly 1 error for 'base', got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingParam { name, .. } => assert_eq!(name, "base"),
        other => panic!("expected MissingParam for 'base', got: {:?}", other),
    }
}

/// step-9 (task-186): diamond dedup with check_trait_conformance_multi — C requires `x`.
/// A:C, B:C. Call multi fn with [A, B]; structure has `x` → no errors, C not walked twice.
#[test]
fn chain_diamond_dedup_satisfied() {
    let length = || Type::Scalar { dimension: DimensionVector::LENGTH };
    let trait_c = CompiledTrait {
        name: "C".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("C_diamond"),
    };
    let trait_a = CompiledTrait {
        name: "A".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string()],
        required_members: vec![],
        defaults: vec![],
        content_hash: ContentHash::of_str("A_diamond"),
    };
    let trait_b = CompiledTrait {
        name: "B".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string()],
        required_members: vec![],
        defaults: vec![],
        content_hash: ContentHash::of_str("B_diamond"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("A".to_string(), &trait_a);
    registry.insert("B".to_string(), &trait_b);
    registry.insert("C".to_string(), &trait_c);
    let mut members = std::collections::HashMap::new();
    members.insert("x".to_string(), length());
    let errors = check_trait_conformance_multi(&members, &["A", "B"], &registry, &[], &[]);
    assert!(errors.is_empty(), "expected no errors (diamond dedup satisfied), got: {:?}", errors);
}

/// step-11 (task-186): diamond missing exactly one error.
/// Same diamond (A:C, B:C, C requires `x`), structure lacks `x` → exactly 1 MissingParam.
/// Dedup ensures C is only walked once, so only 1 error (not 2).
#[test]
fn chain_diamond_missing_exactly_one_error() {
    let length = || Type::Scalar { dimension: DimensionVector::LENGTH };
    let trait_c = CompiledTrait {
        name: "C".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("C_diamond2"),
    };
    let trait_a = CompiledTrait {
        name: "A".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string()],
        required_members: vec![],
        defaults: vec![],
        content_hash: ContentHash::of_str("A_diamond2"),
    };
    let trait_b = CompiledTrait {
        name: "B".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string()],
        required_members: vec![],
        defaults: vec![],
        content_hash: ContentHash::of_str("B_diamond2"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("A".to_string(), &trait_a);
    registry.insert("B".to_string(), &trait_b);
    registry.insert("C".to_string(), &trait_c);
    // Structure does NOT provide `x`.
    let members: std::collections::HashMap<String, Type> = std::collections::HashMap::new();
    let errors = check_trait_conformance_multi(&members, &["A", "B"], &registry, &[], &[]);
    assert_eq!(errors.len(), 1, "expected exactly 1 MissingParam (not 2), got: {:?}", errors);
    match &errors[0] {
        ConformanceError::MissingParam { name, .. } => assert_eq!(name, "x"),
        other => panic!("expected MissingParam for 'x', got: {:?}", other),
    }
}

/// step-13 (task-186): conflicting requirements — C requires `x : Length`, D requires `x : Mass`.
/// Trait A refines both C and D → expect ConflictingRequirement { name: "x", .. }.
/// Fails until ConflictingRequirement variant is added.
#[test]
fn chain_conflicting_requirements() {
    let length = || Type::Scalar { dimension: DimensionVector::LENGTH };
    let mass = || Type::Scalar { dimension: DimensionVector::MASS };
    let trait_c = CompiledTrait {
        name: "C".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Param(length()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("C_conflict"),
    };
    let trait_d = CompiledTrait {
        name: "D".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec![],
        required_members: vec![TraitRequirement {
            name: "x".to_string(),
            kind: RequirementKind::Param(mass()),
            span: test_span(),
        }],
        defaults: vec![],
        content_hash: ContentHash::of_str("D_conflict"),
    };
    // Trait A refines both C and D (conflicting on `x`).
    let trait_a = CompiledTrait {
        name: "A".to_string(),
        is_pub: true,
        type_params: vec![],
        refinements: vec!["C".to_string(), "D".to_string()],
        required_members: vec![],
        defaults: vec![],
        content_hash: ContentHash::of_str("A_conflict"),
    };
    let mut registry = std::collections::HashMap::new();
    registry.insert("A".to_string(), &trait_a);
    registry.insert("C".to_string(), &trait_c);
    registry.insert("D".to_string(), &trait_d);
    // Structure provides `x` as Length (matches C, not D).
    let mut members = std::collections::HashMap::new();
    members.insert("x".to_string(), length());
    let errors = check_trait_conformance_chain(&members, "A", &registry, &[], &[]);
    // Should have at least 1 ConflictingRequirement error.
    let conflict = errors.iter().find(|e| matches!(e, ConformanceError::ConflictingRequirement { name, .. } if name == "x"));
    assert!(conflict.is_some(), "expected ConflictingRequirement for 'x', got: {:?}", errors);
    match conflict.unwrap() {
        ConformanceError::ConflictingRequirement { name, type_a, type_b } => {
            assert_eq!(name, "x");
            // type_a is from the first parent (C: Length), type_b from the conflicting (D: Mass).
            let types = [type_a.clone(), type_b.clone()];
            assert!(
                types.contains(&length()) && types.contains(&mass()),
                "expected one Length and one Mass, got: {:?} and {:?}",
                type_a,
                type_b
            );
        }
        other => panic!("expected ConflictingRequirement, got: {:?}", other),
    }
}
