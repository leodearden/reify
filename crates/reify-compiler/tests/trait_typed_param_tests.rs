//! Trait-typed params — compilation tests for task 1874.
//!
//! Verify that `param m : Material` and related forms resolve to
//! `Type::TraitObject("Material")` across structures, ports, guards,
//! traits, and conformance checks. Uses the stdlib-enabled helpers so
//! the `Material` trait (from `stdlib/materials_mechanical.ri`) is in
//! scope.

use reify_compiler::RequirementKind;
use reify_test_support::{compile_source, compile_source_with_stdlib, parse_and_compile_with_stdlib, warnings_only};
use reify_types::{Severity, Type};

/// Structure member: `param m : Material` should resolve to `Type::TraitObject("Material")`.
#[test]
fn structure_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        structure def HasMaterial { param m : Material }
    "#;
    let module = compile_source_with_stdlib(source);

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

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "HasMaterial")
        .expect("HasMaterial template should be compiled");

    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::TraitObject("Material".to_string()),
        "param typed with trait name Material should resolve to Type::TraitObject(\"Material\")"
    );
}

/// Guarded param: `where cond { param m : Material }` inside a structure should
/// resolve the guarded member's type to `Type::TraitObject("Material")`.
#[test]
fn guarded_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        structure def GuardedMaterial {
            param active : Bool = true
            where active {
                param m : Material
            }
        }
    "#;
    let module = compile_source_with_stdlib(source);

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

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "GuardedMaterial")
        .expect("GuardedMaterial template should be compiled");

    assert_eq!(
        template.guarded_groups.len(),
        1,
        "expected 1 guarded group"
    );
    let group = &template.guarded_groups[0];

    let m_member = group
        .members
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("guarded member 'm' should exist");

    assert_eq!(
        m_member.cell_type,
        Type::TraitObject("Material".to_string()),
        "guarded param typed with trait name Material should resolve to Type::TraitObject(\"Material\")"
    );
}

/// Trait member: `trait Assembly { param m : Material }` should record the
/// member's type as `Type::TraitObject("Material")` in required_members.
#[test]
fn trait_member_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        trait Assembly { param m : Material }
    "#;
    let module = compile_source_with_stdlib(source);

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

    let assembly = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly trait should be compiled");

    let m_req = assembly
        .required_members
        .iter()
        .find(|r| r.name == "m")
        .expect("required member 'm' should exist");

    match &m_req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::TraitObject("Material".to_string()),
            "trait member typed with trait name Material should resolve to Type::TraitObject(\"Material\")"
        ),
        other => panic!("expected RequirementKind::Param, got {:?}", other),
    }
}

/// Conformance path: a structure that conforms to a trait with a trait-typed
/// member must compile without `unresolved type in conformance check: Material`.
#[test]
fn structure_conforming_to_trait_with_trait_typed_member_compiles() {
    let source = r#"
        trait HasMaterial { param material : Material }
        structure def Part : HasMaterial { param material : Material }
    "#;
    let module = compile_source_with_stdlib(source);

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

    let part = module
        .templates
        .iter()
        .find(|t| t.name == "Part")
        .expect("Part template should be compiled");

    let material_cell = part
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "material")
        .expect("value cell 'material' should exist");

    assert_eq!(
        material_cell.cell_type,
        Type::TraitObject("Material".to_string()),
        "Part.material should resolve to Type::TraitObject(\"Material\")"
    );
}

/// Flange example: after the step-14 migration, `BoltFlange.material` should
/// have type `Type::TraitObject("Material")` and the example should compile
/// without errors.
#[test]
fn flange_example_material_param_is_trait_object() {
    let source = std::fs::read_to_string(format!(
        "{}/../../examples/m5_geometry_flange.ri",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("examples/m5_geometry_flange.ri should exist");
    let module = parse_and_compile_with_stdlib(&source);

    let flange = module
        .templates
        .iter()
        .find(|t| t.name == "BoltFlange")
        .expect("BoltFlange template should be compiled");

    let material_cell = flange
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "material")
        .expect("BoltFlange.material should exist");

    assert_eq!(
        material_cell.cell_type,
        Type::TraitObject("Material".to_string()),
        "BoltFlange.material should resolve to Type::TraitObject(\"Material\") after step-14"
    );
}

/// Port member: `port p : in PortType { param m : Material }` should resolve the
/// port member's param type to `Type::TraitObject("Material")`.
#[test]
fn port_member_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        trait PortType {}
        structure def HasPortMember {
            port p : in PortType { param m : Material }
        }
    "#;
    let module = compile_source_with_stdlib(source);

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

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "HasPortMember")
        .expect("HasPortMember template should be compiled");

    let port = template
        .ports
        .iter()
        .find(|p| p.name == "p")
        .expect("port 'p' should exist");

    let m_member = port
        .members
        .iter()
        .find(|vc| vc.id.member == "p.m")
        .expect("port member 'p.m' should exist");

    assert_eq!(
        m_member.cell_type,
        Type::TraitObject("Material".to_string()),
        "port member typed with trait name Material should resolve to Type::TraitObject(\"Material\")"
    );
}

// ─── amend: reviewer_comprehensive suggestion #2 ────────────────────────────

/// Negative case: a param typed with a non-existent trait name should emit
/// the standard `unresolved type` diagnostic. This guards against a future
/// refactor accidentally making trait-name resolution a silent-pass fallback.
#[test]
fn structure_param_with_unknown_trait_name_emits_unresolved_type_diagnostic() {
    let source = r#"
        structure def Broken { param m : NoSuchTrait }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("unresolved type") && d.message.contains("NoSuchTrait")),
        "expected an `unresolved type: NoSuchTrait` diagnostic, got: {:?}",
        errors
    );
}

/// Precedence: local `type Material = Real` must win over a trait named
/// `Material` (resolution order is builtins → type params → alias registry
/// → trait names). The param should resolve to `Type::Real`, not
/// `Type::TraitObject("Material")`.
#[test]
fn alias_wins_over_trait_name_for_param_type() {
    // Both the alias and the trait share the name "Material"; the alias is
    // defined locally and should take precedence over the local trait.
    // Using `compile_source` (no stdlib) keeps the test self-contained —
    // only the names declared inline are visible.
    let source = r#"
        trait Material {}
        type Material = Real
        structure def Part { param m : Material }
    "#;
    let module = compile_source(source);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Part")
        .expect("Part template should be compiled");

    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::Real,
        "alias `type Material = Real` must win over trait `Material`; got: {:?}",
        m_cell.cell_type
    );
}

// ─── Call-site conformance tests (task 1874) ─────────────────────────────────

/// Negative test: passing a non-conforming struct to a trait-typed param must
/// produce an error containing "does not conform to trait" and the trait name.
///
/// This MUST fail on the base branch (before step-2 implementation) because no
/// call-site conformance check exists yet.
///
/// The test also asserts the co-occurring Warning-severity
/// "cannot infer return type of zero-arg function NotAMaterial" diagnostic
/// (emitted by expr.rs when a structure name is used in call position before
/// the template registry populates).
#[test]
fn sub_component_arg_for_trait_typed_param_rejects_non_conforming_struct() {
    // NotAMaterial does NOT declare `: Material` — it just has `density : Real`.
    let source = r#"
        structure def Host { param m : Material }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Top {
            sub x = Host(m: NotAMaterial())
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("Material")),
        "expected a 'does not conform to trait Material' error, got: {:?}",
        errors
    );

    let warnings = warnings_only(&module);
    assert!(
        warnings
            .iter()
            .any(|d| d.message.contains("cannot infer return type of zero-arg function")
                && d.message.contains("NotAMaterial")),
        "expected a Warning-severity 'cannot infer return type of zero-arg function NotAMaterial' diagnostic co-occurring with the conformance error, got: {:?}",
        warnings
    );
}

/// Positive test: passing a conforming struct to a trait-typed param compiles
/// without errors.
///
/// `Steel` declares `: Material` and provides `density` and `name`. The host
/// structure declares `param m : Material`. Passing `Steel()` at the call-site
/// should pass the conformance check.
#[test]
fn sub_component_arg_for_trait_typed_param_accepts_conforming_struct() {
    let source = r#"
        structure def Steel : Material {
            param density : Real = 7850.0
            param name : String = "steel"
        }
        structure def Host { param m : Material }
        structure def Top {
            sub x = Host(m: Steel())
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for conforming struct arg, got: {:?}",
        errors
    );
}

/// TraitObject-arg pass-through: a param value that is itself trait-typed (e.g. a local
/// `param p : Physical`) can be passed to a slot that requires `Material` when
/// `Physical : Material`. The arg's compiled type is `Type::TraitObject("Physical")`; the
/// check must use `trait_satisfies` to verify the refinement.
///
/// Will fail if the helper only handles `StructureRef` arg types and the `TraitObject`
/// branch falls through to the "cannot conform" error.
#[test]
fn trait_object_arg_accepted_for_trait_typed_param_via_refinement() {
    // Self-contained: no stdlib needed.
    let source = r#"
        trait Material {}
        trait Physical : Material {}
        structure def Host { param m : Material }
        structure def Top {
            param p : Physical
            sub h = Host(m: p)
        }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors: passing TraitObject(Physical) for param m : Material where Physical : Material, got: {:?}",
        errors
    );
}

/// End-to-end acceptance test: instantiate BoltFlange (from examples/m5_geometry_flange.ri)
/// with a conforming `Steel` material struct via `sub f = BoltFlange(material: Steel())`.
///
/// This is the acceptance-criterion end-to-end test for task 1874. It verifies that
/// the call-site conformance check allows `Steel : Material` to satisfy the `material : Material`
/// typed param in BoltFlange.
#[test]
fn flange_instantiated_with_conforming_material_struct_compiles() {
    let flange_source = std::fs::read_to_string(format!(
        "{}/../../examples/m5_geometry_flange.ri",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("examples/m5_geometry_flange.ri should exist");

    // Append a Steel material struct that conforms to Material (density + name),
    // and an Assembly that instantiates BoltFlange with Steel.
    // All other BoltFlange params have defaults, so only material needs supplying.
    let full_source = format!(
        r#"{}
structure def Steel : Material {{
    param density : Real = 7850.0
    param name : String = "steel"
}}
structure def Assembly {{
    sub f = BoltFlange(material: Steel())
}}
"#,
        flange_source
    );

    let module = parse_and_compile_with_stdlib(&full_source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for BoltFlange(material: Steel()) where Steel : Material, got: {:?}",
        errors
    );
}

/// Refinement test: a struct that conforms via a refinement chain is accepted.
///
/// `Physical : Material` (refinement), `Rigid : Physical` (structure declares `: Physical`).
/// The host param is `Material`. `Rigid : Physical : Material` so Rigid satisfies Material
/// transitively. This will FAIL if `satisfies_trait_bound` is not used (direct equality
/// of trait name would reject `Physical` for `Material`).
#[test]
fn sub_component_arg_conforming_via_refinement_chain_accepted() {
    // Self-contained: no stdlib needed.
    let source = r#"
        trait Material {}
        trait Physical : Material {}
        structure def Host { param m : Material }
        structure def Rigid : Physical {}
        structure def Top {
            sub x = Host(m: Rigid())
        }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors: Rigid : Physical : Material satisfies param m : Material, got: {:?}",
        errors
    );
}

/// Regression guard: passing a Real literal to a trait-typed param emits a
/// "does not conform to trait" error.
///
/// This locks in the existing behaviour of the suppression heuristic in
/// `check_trait_arg_conformance` (conformance.rs). The exact path exercised:
///
/// 1. `1.0` compiles to `CompiledExprKind::Literal(Value::Real(...))` — NOT a
///    `FunctionCall` — so `arg_call_name` is `None` (captured in the
///    `arg_call_name` extraction block in `entity.rs`).
/// 2. The promotion branch in `check_trait_arg_conformance` finds
///    `arg_call_name` is `None`, so `effective_arg_type = &Type::Real`.
/// 3. The `_` arm of `check_trait_arg_conformance` is reached.
/// 4. The suppression guard in the `_` arm requires BOTH
///    `matches!(arg_type, Type::Real)` AND `arg_call_name.is_some()`. Because
///    `arg_call_name` is `None`, the guard does NOT suppress — execution continues
///    and the diagnostic is emitted.
///
/// Would fail if the suppression heuristic were broadened to cover
/// `arg_call_name.is_none()` (i.e. suppressing even for bare Real literals).
#[test]
fn sub_component_arg_real_literal_for_trait_typed_param_emits_conformance_error() {
    let source = r#"
        structure def Host { param m : Material }
        structure def Top {
            sub x = Host(m: 1.0)
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("Material")),
        "expected a 'does not conform to trait Material' error for Real literal arg, got: {:?}",
        errors
    );
}

/// Non-empty struct-instantiation call in a sub arg value position.
///
/// `sub x = Host(m: Steel(density: 1000.0))` exercises the non-zero-arg
/// FunctionCall path in the arg_call_name capture in entity.rs.  The
/// entity.rs code is already defensively widened to `FunctionCall { function, .. }`
/// (any args) rather than an is_empty() guard, so this path should be handled
/// correctly once the parser accepts nested calls with args in sub arg positions.
///
/// **Currently ignored** because the Reify parser rejects `Steel(density: 1000.0)`
/// as an expression inside a sub arg value (follow-up task to extend the parser).
#[test]
#[ignore = "parser does not yet accept nested calls with args in sub arg positions (follow-up task)"]
fn sub_component_arg_structure_instantiation_with_args_accepted() {
    let source = r#"
        trait Material {}
        structure def Steel : Material {
            param density : Real = 7850.0
        }
        structure def Host { param m : Material }
        structure def Top {
            sub x = Host(m: Steel(density: 1000.0))
        }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors: Steel(density: 1000.0) conforms to Material, got: {:?}",
        errors
    );
}
