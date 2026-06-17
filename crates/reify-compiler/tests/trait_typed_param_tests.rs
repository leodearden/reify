//! Trait-typed params — compilation tests for task 1874.
//!
//! Verify that `param m : MaterialSpec` and related forms resolve to
//! `Type::TraitObject("MaterialSpec")` across structures, ports, guards,
//! traits, and conformance checks. Uses the stdlib-enabled helpers so
//! the `MaterialSpec` trait (from `stdlib/materials_mechanical.ri`) is in
//! scope. (Task 1876 renamed the trait from `Material` to `MaterialSpec`
//! so the name `Material` could be reused for the canonical struct.)

use reify_compiler::RequirementKind;
use reify_test_support::{compile_source, compile_source_with_stdlib};
use reify_core::{DiagnosticCode, Severity, Type};

/// Structure member: `param m : MaterialSpec` should resolve to `Type::TraitObject("MaterialSpec")`.
#[test]
fn structure_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        structure def HasMaterial { param m : MaterialSpec }
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
        Type::TraitObject("MaterialSpec".to_string()),
        "param typed with trait name MaterialSpec should resolve to Type::TraitObject(\"MaterialSpec\")"
    );
}

/// Guarded param: `where cond { param m : MaterialSpec }` inside a structure should
/// resolve the guarded member's type to `Type::TraitObject("MaterialSpec")`.
#[test]
fn guarded_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        structure def GuardedMaterial {
            param active : Bool = true
            where active {
                param m : MaterialSpec
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

    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group = &template.guarded_groups[0];

    let m_member = group
        .members
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("guarded member 'm' should exist");

    assert_eq!(
        m_member.cell_type,
        Type::TraitObject("MaterialSpec".to_string()),
        "guarded param typed with trait name MaterialSpec should resolve to Type::TraitObject(\"MaterialSpec\")"
    );
}

/// Trait member: `trait Assembly { param m : MaterialSpec }` should record the
/// member's type as `Type::TraitObject("MaterialSpec")` in required_members.
#[test]
fn trait_member_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        trait Assembly { param m : MaterialSpec }
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
            Type::TraitObject("MaterialSpec".to_string()),
            "trait member typed with trait name MaterialSpec should resolve to Type::TraitObject(\"MaterialSpec\")"
        ),
        other => panic!("expected RequirementKind::Param, got {:?}", other),
    }
}

/// Conformance path: a structure that conforms to a trait with a trait-typed
/// member must compile without `unresolved type in conformance check: MaterialSpec`.
#[test]
fn structure_conforming_to_trait_with_trait_typed_member_compiles() {
    let source = r#"
        trait HasMaterial { param material : MaterialSpec }
        structure def Part : HasMaterial { param material : MaterialSpec }
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
        Type::TraitObject("MaterialSpec".to_string()),
        "Part.material should resolve to Type::TraitObject(\"MaterialSpec\")"
    );
}

/// Port member: `port p : in PortType { param m : MaterialSpec }` should resolve the
/// port member's param type to `Type::TraitObject("MaterialSpec")`.
#[test]
fn port_member_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        trait PortType {}
        structure def HasPortMember {
            port p : in PortType { param m : MaterialSpec }
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
        Type::TraitObject("MaterialSpec".to_string()),
        "port member typed with trait name MaterialSpec should resolve to Type::TraitObject(\"MaterialSpec\")"
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
/// → trait names). The param should resolve to `Type::dimensionless_scalar()`, not
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
        Type::dimensionless_scalar(),
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
/// NOTE: This source also produces a Warning-severity diagnostic
/// "cannot infer return type of zero-arg function NotAMaterial" (emitted by
/// expr.rs when a structure name is used in call position before the template
/// registry is fully populated). This warning co-occurs with the conformance
/// error and is intentional/expected — the conformance error is the one the
/// user cares about. The test filters to `Severity::Error` to remain focused.
#[test]
fn sub_component_arg_for_trait_typed_param_rejects_non_conforming_struct() {
    // NotAMaterial does NOT declare `: MaterialSpec` — it just has `density : Real`.
    let source = r#"
        structure def Host { param m : MaterialSpec }
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error, got: {:?}",
        errors
    );
}

/// Positive test: passing a conforming struct to a trait-typed param compiles
/// without errors.
///
/// `Steel` declares `: MaterialSpec` and provides `density` and `name`. The host
/// structure declares `param m : MaterialSpec`. Passing `Steel()` at the call-site
/// should pass the conformance check.
#[test]
fn sub_component_arg_for_trait_typed_param_accepts_conforming_struct() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param m : MaterialSpec }
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

// NOTE: The task-1874 test
// `flange_instantiated_with_conforming_material_struct_compiles` was removed
// by task 1876. Its premise — passing a `Steel : Material` (trait-conforming)
// struct to the flange's trait-typed `material` param — no longer holds
// after 1876 promoted `Material` from a trait to a canonical struct. The
// new end-to-end coverage for the flange lives in
// `tests/material_struct_tests.rs::boltflange_compiles_with_material_default`
// (step-9 of task 1876).

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
///    `arg_call_name` is `None`, so `effective_arg_type = &Type::dimensionless_scalar()`.
/// 3. The `_` arm of `check_trait_arg_conformance` is reached.
/// 4. The suppression guard in the `_` arm requires BOTH
///    `matches!(arg_type, Type::dimensionless_scalar())` AND `arg_call_name.is_some()`. Because
///    `arg_call_name` is `None`, the guard does NOT suppress — execution continues
///    and the diagnostic is emitted.
///
/// Would fail if the suppression heuristic were broadened to cover
/// `arg_call_name.is_none()` (i.e. suppressing even for bare Real literals).
#[test]
fn sub_component_arg_real_literal_for_trait_typed_param_emits_conformance_error() {
    let source = r#"
        structure def Host { param m : MaterialSpec }
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error for Real literal arg, got: {:?}",
        errors
    );
}

/// Acceptance for task 2039: a sub arg value may itself be a struct-instantiation
/// call with named arguments (e.g. `Host(m: Steel(density: 1000kg/m^3))`), and the
/// inner call's callee participates in trait conformance as a `StructureRef`.
/// The discriminator vs. the existing `...accepts_conforming_struct` test is
/// the inner call's named args: `Steel(density: 1000kg/m^3)` rather than `Steel()`.
/// Zero Error-severity diagnostics expected.
#[test]
fn sub_component_arg_structure_instantiation_with_args_accepted() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param m : MaterialSpec }
        structure def Top {
            sub x = Host(m: Steel(density: 1000kg/m^3))
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
        "expected no error diagnostics for nested struct-instantiation call with named args, got: {:?}",
        errors
    );
}

// ─── Option<TraitObject> conformance tests (task 2227) ───────────────────────
//
// Tests for call-site conformance checking when a param is declared as
// Option<SomeTrait>. The conformance check must recurse into the Option
// wrapper and verify the inner value conforms to the trait.

/// Negative test: passing `some(NotAMaterial())` to an `Option<MaterialSpec>` param
/// must produce a "does not conform to trait" error.
///
/// This MUST fail on the post-step-2 base because the conformance dispatcher
/// still returns silently when the param type is `Type::Option(...)` rather than
/// exactly `Type::TraitObject(...)`.
#[test]
fn option_trait_typed_param_rejects_some_with_non_conforming_struct() {
    let source = r#"
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param m : Option<MaterialSpec> }
        structure def Top {
            sub x = Host(m: some(NotAMaterial()))
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error for some(NotAMaterial()) passed to Option<MaterialSpec> param, got: {:?}",
        errors
    );
}

/// Positive test: passing `some(Steel())` (a MaterialSpec-conforming struct) to
/// an `Option<MaterialSpec>` param must compile without errors.
#[test]
fn option_trait_typed_param_accepts_some_with_conforming_struct() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param m : Option<MaterialSpec> }
        structure def Top {
            sub x = Host(m: some(Steel()))
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
        "expected no errors for some(Steel()) passed to Option<MaterialSpec> param, got: {:?}",
        errors
    );
}

/// Positive test: passing `none` to an `Option<MaterialSpec>` param must compile
/// without errors — `none` is always valid for any Option<T> param.
#[test]
fn option_trait_typed_param_accepts_none() {
    let source = r#"
        structure def Host { param m : Option<MaterialSpec> }
        structure def Top {
            sub x = Host(m: none)
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
        "expected no errors for none passed to Option<MaterialSpec> param, got: {:?}",
        errors
    );
}

// ─── List<TraitObject> conformance tests (task 2227) ─────────────────────────

/// Negative test: a list containing a non-conforming element passed to
/// `List<MaterialSpec>` must produce a "does not conform to trait" error.
#[test]
fn list_trait_typed_param_rejects_non_conforming_element() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param ms : List<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: [Steel(), NotAMaterial()])
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error for [Steel(), NotAMaterial()] passed to List<MaterialSpec> param, got: {:?}",
        errors
    );
}

/// Positive test: all elements in the list conform to the trait.
#[test]
fn list_trait_typed_param_accepts_all_conforming_elements() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param ms : List<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: [Steel(), Steel(density: 1000kg/m^3)])
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
        "expected no errors for [Steel(), Steel(density: 1000kg/m^3)] passed to List<MaterialSpec> param, got: {:?}",
        errors
    );
}

/// Positive test: an empty list is always valid for any List<T> param.
#[test]
fn list_trait_typed_param_accepts_empty_list() {
    let source = r#"
        structure def Host { param ms : List<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: [])
        }
    "#;
    let module = compile_source_with_stdlib(source);

    // Empty list may emit a Warning about "cannot infer element type" but
    // should emit no Error-severity diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for [] passed to List<MaterialSpec> param, got: {:?}",
        errors
    );
}

// ─── Set<TraitObject> conformance tests (task 2227) ──────────────────────────

/// Negative test: a set containing a non-conforming element passed to
/// `Set<MaterialSpec>` must produce a "does not conform to trait" error.
#[test]
fn set_trait_typed_param_rejects_non_conforming_element() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param ms : Set<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: set{Steel(), NotAMaterial()})
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error for set with NotAMaterial passed to Set<MaterialSpec> param, got: {:?}",
        errors
    );
}

/// Positive test: all elements in the set conform to the trait.
#[test]
fn set_trait_typed_param_accepts_all_conforming_elements() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param ms : Set<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: set{Steel(), Steel(density: 1000kg/m^3)})
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
        "expected no errors for set{{Steel(), Steel(density: 1000kg/m^3)}} passed to Set<MaterialSpec> param, got: {:?}",
        errors
    );
}

// ─── Map<K, TraitObject> conformance tests (task 2227) ───────────────────────

/// Negative test: a map entry with a non-conforming value passed to
/// `Map<String, MaterialSpec>` must produce a "does not conform to trait" error.
#[test]
fn map_trait_typed_param_rejects_non_conforming_value() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param ms : Map<String, MaterialSpec> }
        structure def Top {
            sub x = Host(ms: map{"good" => Steel(), "bad" => NotAMaterial()})
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error for map with NotAMaterial value passed to Map<String, MaterialSpec> param, got: {:?}",
        errors
    );
}

/// Positive test: all map values conform to the trait.
#[test]
fn map_trait_typed_param_accepts_all_conforming_values() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param ms : Map<String, MaterialSpec> }
        structure def Top {
            sub x = Host(ms: map{"a" => Steel(), "b" => Steel(density: 1000kg/m^3)})
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
        "expected no errors for map with all-conforming values passed to Map<String, MaterialSpec> param, got: {:?}",
        errors
    );
}

// ─── Wrapped-trait param resolution (task 2227) ──────────────────────────────
//
// Each test declares a trait and a structure whose param is wrapped in one of
// the four collection/option builtins, then asserts that the param's cell_type
// is the expected compound Type (e.g. Type::Option(Box::new(Type::TraitObject(...)))).
//
// All four tests MUST fail on the base branch with an "unresolved type" error
// because resolve_parameterized_builtin_type does not thread trait_names through
// to its inner-arg resolution, so `Option<MyTrait>` is treated as a wrapped
// unknown name rather than a wrapped trait object.

/// Verifies that `param m : Option<MyTrait>` resolves to
/// `Type::Option(Box::new(Type::TraitObject("MyTrait")))`.
#[test]
fn option_traitobject_param_resolves_to_option_traitobject() {
    let source = r#"
        trait MyTrait {}
        structure def Host { param m : Option<MyTrait> }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for Option<MyTrait> param, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Host")
        .expect("Host template should be compiled");
    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::Option(Box::new(Type::TraitObject("MyTrait".to_string()))),
        "param m : Option<MyTrait> should resolve to Type::Option(Type::TraitObject(\"MyTrait\")), got: {:?}",
        m_cell.cell_type
    );
}

/// Verifies that `param m : List<MyTrait>` resolves to
/// `Type::List(Box::new(Type::TraitObject("MyTrait")))`.
#[test]
fn list_traitobject_param_resolves_to_list_traitobject() {
    let source = r#"
        trait MyTrait {}
        structure def Host { param m : List<MyTrait> }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for List<MyTrait> param, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Host")
        .expect("Host template should be compiled");
    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::List(Box::new(Type::TraitObject("MyTrait".to_string()))),
        "param m : List<MyTrait> should resolve to Type::List(Type::TraitObject(\"MyTrait\")), got: {:?}",
        m_cell.cell_type
    );
}

/// Verifies that `param m : Set<MyTrait>` resolves to
/// `Type::Set(Box::new(Type::TraitObject("MyTrait")))`.
#[test]
fn set_traitobject_param_resolves_to_set_traitobject() {
    let source = r#"
        trait MyTrait {}
        structure def Host { param m : Set<MyTrait> }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for Set<MyTrait> param, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Host")
        .expect("Host template should be compiled");
    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::Set(Box::new(Type::TraitObject("MyTrait".to_string()))),
        "param m : Set<MyTrait> should resolve to Type::Set(Type::TraitObject(\"MyTrait\")), got: {:?}",
        m_cell.cell_type
    );
}

/// Verifies that `param m : Map<String, MyTrait>` resolves to
/// `Type::Map(Box::new(Type::String), Box::new(Type::TraitObject("MyTrait")))`.
#[test]
fn map_string_to_traitobject_param_resolves_to_map_string_traitobject() {
    let source = r#"
        trait MyTrait {}
        structure def Host { param m : Map<String, MyTrait> }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for Map<String, MyTrait> param, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Host")
        .expect("Host template should be compiled");
    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::Map(
            Box::new(Type::String),
            Box::new(Type::TraitObject("MyTrait".to_string()))
        ),
        "param m : Map<String, MyTrait> should resolve to Type::Map(Type::String, Type::TraitObject(\"MyTrait\")), got: {:?}",
        m_cell.cell_type
    );
}

// ─── Type-level fallback walker tests (task 2227) ────────────────────────────
//
// These tests exercise the walk_param_against_arg_type fallback that fires when
// the arg is not a literal (e.g. a param ValueRef) and its result_type carries
// the wrapper structure.  The dispatcher falls through from walk_param_against_arg
// (which only matches literal kinds) to walk_param_against_arg_type for ValueRef args
// whose result_type is a wrapped trait object.

/// Positive test: a param `p : Option<Physical>` where `Physical : Material` passed
/// to a slot `m : Option<Material>` should compile without errors.
///
/// `p` is a ValueRef with `result_type = Option<TraitObject("Physical")>`.
/// `walk_param_against_arg` sees `(Option<Material>, non-literal)` → falls through to
/// `walk_param_against_arg_type(Option<Material>, Option<Physical>)` → recurses to
/// `(Material, Physical)` → `emit_leaf_conformance_for_arg_type` checks
/// `trait_satisfies("Physical", "Material")` → true → passes.
#[test]
fn option_trait_typed_param_accepts_valueref_of_conforming_subtrait() {
    // Self-contained: no stdlib needed — traits are declared inline.
    let source = r#"
        trait Material {}
        trait Physical : Material {}
        structure def Host { param m : Option<Material> }
        structure def Top {
            param p : Option<Physical>
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
        "expected no errors: passing Option<Physical> (Physical : Material) for Option<Material> param, got: {:?}",
        errors
    );
}

/// Negative test: a param `p : Option<Other>` where `Other` does NOT refine `Material`
/// passed to a slot `m : Option<Material>` must produce a "does not conform to trait" error.
///
/// `p` is a ValueRef with `result_type = Option<TraitObject("Other")>`.
/// `walk_param_against_arg_type` walks to the leaf `(Material, Other)` →
/// `emit_leaf_conformance_for_arg_type` checks `trait_satisfies("Other", "Material")`
/// → false → emits diagnostic.
#[test]
fn option_trait_typed_param_rejects_valueref_of_non_conforming_trait() {
    // Self-contained: no stdlib needed — traits are declared inline.
    let source = r#"
        trait Material {}
        trait Other {}
        structure def Host { param m : Option<Material> }
        structure def Top {
            param p : Option<Other>
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
        errors
            .iter()
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("Material")),
        "expected a 'does not conform to trait Material' error for Option<Other> ValueRef passed to Option<Material> param, got: {:?}",
        errors
    );
}

// ─── Nested wrapper conformance tests (task 2227) ────────────────────────────
//
// These tests exercise recursive descent past depth 1.  For example,
// `List<Option<MaterialSpec>>` requires the walker to first unwrap List → iterate
// elements, then unwrap Option → OptionSome/OptionNone, and finally reach the
// TraitObject leaf check for MaterialSpec.

/// Negative test: a list-of-options where one element is `some(NotAMaterial())` passed
/// to `List<Option<MaterialSpec>>` must produce a "does not conform to trait" error.
///
/// Walk sequence:
/// 1. `(List<Option<MaterialSpec>>, ListLiteral([...]))` → iterate each element
/// 2. `(Option<MaterialSpec>, OptionSome(Steel()))` → `(MaterialSpec, Steel())` → pass
/// 3. `(Option<MaterialSpec>, OptionNone)` → pass immediately
/// 4. `(Option<MaterialSpec>, OptionSome(NotAMaterial()))` → `(MaterialSpec, NotAMaterial())`
///    → leaf check fails → emit diagnostic
#[test]
fn nested_wrapper_list_option_rejects_non_conforming_inner_element() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param ms : List<Option<MaterialSpec>> }
        structure def Top {
            sub x = Host(ms: [some(Steel()), none, some(NotAMaterial())])
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
                && d.message.contains("MaterialSpec")),
        "expected a 'does not conform to trait MaterialSpec' error for some(NotAMaterial()) inside List<Option<MaterialSpec>>, got: {:?}",
        errors
    );
}

/// Positive test: a list-of-options where every non-none element conforms to the trait
/// passed to `List<Option<MaterialSpec>>` must compile without errors.
#[test]
fn nested_wrapper_list_option_accepts_all_conforming_inner_elements() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param ms : List<Option<MaterialSpec>> }
        structure def Top {
            sub x = Host(ms: [some(Steel()), none, some(Steel(density: 1000kg/m^3))])
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
        "expected no errors for [some(Steel()), none, some(Steel(...))] passed to List<Option<MaterialSpec>> param, got: {:?}",
        errors
    );
}

// ─── Nested wrapper type-level walker tests (task 2279) ─────────────────────────────
//
// These tests exercise `walk_param_against_arg_type` recursing through depth-2 wrappers
// (e.g. `List<Option<T>>`).  The literal-walker tests above (task 2227) cover literal
// expansion; these tests cover the type-level fallback path used when the argument is a
// ValueRef (a param reference) rather than a list or option literal.  They pin the
// nested-wrapper recursion path that the existing depth-1 ValueRef tests do not reach.

/// Negative test: a param `p : List<Option<Inert>>` where `Inert` does NOT refine
/// `Carrier` passed to a slot `ms : List<Option<Carrier>>` must produce a
/// "does not conform to trait 'Carrier'" error.
///
/// Walk sequence (type-level walker):
/// 1. `walk_param_against_arg(List<Option<Carrier>>, ValueRef(p))` → non-literal →
///    falls through to
/// 2. `walk_param_against_arg_type(List<Option<Carrier>>, List<Option<Inert>>)` →
///    `(List, List)` arm → recurse to
/// 3. `walk_param_against_arg_type(Option<Carrier>, Option<Inert>)` →
///    `(Option, Option)` arm → recurse to
/// 4. `emit_leaf_conformance_for_arg_type(Inert, Carrier)` →
///    `trait_satisfies("Inert", "Carrier")` → false → emits diagnostic.
///
/// Uses `compile_source` (no stdlib) to avoid name conflicts with stdlib structures.
#[test]
fn nested_wrapper_type_level_list_option_rejects_valueref_of_non_conforming_trait() {
    let source = r#"
        trait Carrier {}
        trait Inert {}
        structure def Host { param ms : List<Option<Carrier>> }
        structure def Top {
            param p : List<Option<Inert>>
            sub h = Host(ms: p)
        }
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
            .any(|d| d.message.contains("does not conform to trait")
                && d.message.contains("Carrier")),
        "expected a 'does not conform to trait Carrier' error for List<Option<Inert>> ValueRef passed to List<Option<Carrier>> param, got: {:?}",
        errors
    );
}

/// Positive test: a param `p : List<Option<Rigid>>` where `Rigid : Carrier`
/// passed to a slot `ms : List<Option<Carrier>>` should compile without errors.
///
/// Walk sequence (type-level walker):
/// 1. `walk_param_against_arg(List<Option<Carrier>>, ValueRef(p))` → non-literal →
///    falls through to
/// 2. `walk_param_against_arg_type(List<Option<Carrier>>, List<Option<Rigid>>)` →
///    `(List, List)` arm → recurse to
/// 3. `walk_param_against_arg_type(Option<Carrier>, Option<Rigid>)` →
///    `(Option, Option)` arm → recurse to
/// 4. `emit_leaf_conformance_for_arg_type(Rigid, Carrier)` →
///    `trait_satisfies("Rigid", "Carrier")` → true → passes.
///
/// Uses `compile_source` (no stdlib) to avoid name conflicts with stdlib structures.
#[test]
fn nested_wrapper_type_level_list_option_accepts_valueref_of_conforming_subtrait() {
    let source = r#"
        trait Carrier {}
        trait Rigid : Carrier {}
        structure def Host { param ms : List<Option<Carrier>> }
        structure def Top {
            param p : List<Option<Rigid>>
            sub h = Host(ms: p)
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
        "expected no errors: passing List<Option<Rigid>> (Rigid : Carrier) for List<Option<Carrier>> param, got: {:?}",
        errors
    );
}

// ─── Wrapper shape-mismatch tests (task 2281) ─────────────────────────────────
//
// Tests that the compiler emits a diagnostic when the argument wrapper shape
// (Option/List/Set/Map) does not match the declared param wrapper shape.
// All four cases below exercise the `walk_param_against_arg_type` fallback `_`
// arm that currently silently drops wrapper-shape mismatches.

/// Negative test: passing `Steel()` (a bare FunctionCall, not wrapped in `some()`)
/// to an `Option<MaterialSpec>` param must produce a wrapper-shape-mismatch error.
///
/// Walk sequence:
/// 1. `walk_param_against_arg(Option<MaterialSpec>, FunctionCall{Steel})` →
///    `(Type::Option, FunctionCall)` — no literal-walker match → fallback →
/// 2. `walk_param_against_arg_type(Option<MaterialSpec>, Real)` →
///    `(Option<MaterialSpec>, Real)` — no match → fallback `_` → emits.
#[test]
fn bare_struct_call_passed_to_option_trait_param_emits_shape_mismatch() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param m : Option<MaterialSpec> }
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

    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not match")
                && d.message.contains("Option")
                && d.message.contains("Steel")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one wrapper-shape diagnostic, got {:?}",
        matching
    );
    // Pin that the anti-cascade guard in walk_param_against_arg_type prevents
    // secondary TypeNotConformingToTrait diagnostics from piling on top of the
    // wrapper-shape mismatch. If this count exceeds 1, the guard is not working.
    assert_eq!(
        errors
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
            .count(),
        1,
        "expected exactly one TypeNotConformingToTrait diagnostic total (anti-cascade guard), got {:?}",
        errors
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
            .collect::<Vec<_>>()
    );
}

/// Negative test: passing `[Steel()]` (a list literal) to an `Option<MaterialSpec>`
/// param must produce a wrapper-shape-mismatch error.
///
/// Walk sequence:
/// 1. `walk_param_against_arg(Option<MaterialSpec>, ListLiteral([Steel()]))` →
///    `(Type::Option, ListLiteral)` — no literal-walker match → fallback →
/// 2. `walk_param_against_arg_type(Option<MaterialSpec>, List<…>)` →
///    `(Option, List)` — no match → fallback `_` → emits.
#[test]
fn list_literal_passed_to_option_trait_param_emits_shape_mismatch() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param m : Option<MaterialSpec> }
        structure def Top {
            sub x = Host(m: [Steel()])
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not match")
                && d.message.contains("Option")
                && d.message.contains("List")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one wrapper-shape diagnostic, got {:?}",
        matching
    );
}

/// Negative test: passing `map{\"k\" => Steel()}` (a map literal) to a
/// `List<MaterialSpec>` param must produce a wrapper-shape-mismatch error.
///
/// Walk sequence:
/// 1. `walk_param_against_arg(List<MaterialSpec>, MapLiteral({\"k\" => Steel()}))` →
///    `(Type::List, MapLiteral)` — no literal-walker match → fallback →
/// 2. `walk_param_against_arg_type(List<MaterialSpec>, Map<String, …>)` →
///    `(List, Map)` — no match → fallback `_` → emits.
#[test]
fn map_literal_passed_to_list_trait_param_emits_shape_mismatch() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param ms : List<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: map{"k" => Steel()})
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not match")
                && d.message.contains("List")
                && d.message.contains("Map")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one wrapper-shape diagnostic, got {:?}",
        matching
    );
}

/// Negative test: a `param p : List<Material>` ValueRef passed to an
/// `Option<Material>` slot must produce a wrapper-shape-mismatch error.
///
/// Walk sequence:
/// 1. `walk_param_against_arg(Option<Material>, ValueRef(p))` →
///    `(Type::Option, ValueRef)` — no literal-walker match → fallback →
/// 2. `walk_param_against_arg_type(Option<Material>, List<Material>)` →
///    `(Option, List)` — no match → fallback `_` → emits.
///
/// Uses `compile_source` (no stdlib) — inline trait only.
#[test]
fn valueref_of_list_passed_to_option_slot_emits_shape_mismatch() {
    let source = r#"
        trait Material {}
        structure def Host { param m : Option<Material> }
        structure def Top {
            param p : List<Material>
            sub h = Host(m: p)
        }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not match")
                && d.message.contains("Option")
                && d.message.contains("List<Material>")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one wrapper-shape diagnostic, got {:?}",
        matching
    );
}

/// Negative test: a `param p : List<M>` ValueRef passed to a `Set<M>` slot must
/// produce a wrapper-shape-mismatch error. Exercises the `Type::Set(_)` arm of
/// the wrapper-match guard in `walk_param_against_arg_type`'s fallback.
///
/// Walk sequence:
/// 1. `walk_param_against_arg(Set<M>, ValueRef(p))` →
///    `(Type::Set, ValueRef)` — no literal-walker match → fallback →
/// 2. `walk_param_against_arg_type(Set<M>, List<M>)` →
///    `(Set, List)` — no match → fallback `_` → emits.
#[test]
fn valueref_of_list_passed_to_set_trait_param_emits_shape_mismatch() {
    let source = r#"
        trait M {}
        structure def Host { param ms : Set<M> }
        structure def Top {
            param p : List<M>
            sub h = Host(ms: p)
        }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not match")
                && d.message.contains("Set")
                && d.message.contains("List<M>")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one wrapper-shape diagnostic, got {:?}",
        matching
    );
}

/// Negative test: a `param p : List<M>` ValueRef passed to a `Map<String, M>`
/// slot must produce a wrapper-shape-mismatch error. Exercises the
/// `Type::Map(_, _)` arm of the wrapper-match guard in
/// `walk_param_against_arg_type`'s fallback.
///
/// Walk sequence:
/// 1. `walk_param_against_arg(Map<String, M>, ValueRef(p))` →
///    `(Type::Map, ValueRef)` — no literal-walker match → fallback →
/// 2. `walk_param_against_arg_type(Map<String, M>, List<M>)` →
///    `(Map, List)` — no match → fallback `_` → emits.
#[test]
fn valueref_of_list_passed_to_map_trait_param_emits_shape_mismatch() {
    let source = r#"
        trait M {}
        structure def Host { param ms : Map<String, M> }
        structure def Top {
            param p : List<M>
            sub h = Host(ms: p)
        }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not match")
                && d.message.contains("Map<")
                && d.message.contains("List<M>")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one wrapper-shape diagnostic, got {:?}",
        matching
    );
}

// ─── amend (task 2282): Map-key + Option<List<T>> wrapper conformance coverage ───
//
// Two coverage gaps locked in here:
//   Gap #1 — Map KEY non-conformance was untested (walker at conformance/mod.rs:219-224
//             recurses into both key_p and val_p; only val_p had regression coverage).
//   Gap #2 — Option<List<MaterialSpec>> was untested (only List<Option<MaterialSpec>>
//             existed); inverting the wrapper order exercises the same arms in a
//             different traversal sequence.
// Both walker arms are already implemented; these are pure regression-coverage tests.

/// Negative test (gap #1): a map literal with a non-conforming KEY passed to
/// `Map<MaterialSpec, String>` must produce a `TypeNotConformingToTrait`
/// diagnostic naming the param. Mirrors `map_trait_typed_param_rejects_non_conforming_value`
/// (line 763) but flipped to the key position. Locks in the walker's `key_p`
/// recursion at `conformance/mod.rs:219-224` against arm-ordering regressions.
#[test]
fn map_trait_typed_param_rejects_non_conforming_key() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param ms : Map<MaterialSpec, String> }
        structure def Top {
            sub x = Host(ms: map{Steel() => "good", NotAMaterial() => "bad"})
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.iter().any(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not conform to trait")
                && d.message.contains("MaterialSpec")
                && d.message.contains("param 'ms'")
        }),
        "expected a TypeNotConformingToTrait error naming param 'ms' for NotAMaterial at the key position of Map<MaterialSpec, String>, got: {:?}",
        errors
    );
}

/// Negative test (gap #2): a non-conforming inner element inside an
/// `Option<List<MaterialSpec>>` arg must produce a `TypeNotConformingToTrait`
/// diagnostic. Mirrors `nested_wrapper_list_option_rejects_non_conforming_inner_element`
/// (line 1090) with the wrapper order inverted. Locks in the walker's
/// Option-then-List recursion at `conformance/mod.rs:201-211`.
///
/// Walk sequence:
/// 1. `(Type::Option(List<MaterialSpec>), OptionSome(ListLiteral([Steel(), NotAMaterial()])))` → recurse on inner
/// 2. `(Type::List(MaterialSpec), ListLiteral([Steel(), NotAMaterial()]))` → recurse on each elem
/// 3. `(Type::TraitObject("MaterialSpec"), FunctionCall("NotAMaterial"))` → leaf check fails → emit
#[test]
fn option_list_trait_typed_param_rejects_non_conforming_inner_element() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def NotAMaterial { param density : Real = 1.0 }
        structure def Host { param ms : Option<List<MaterialSpec>> }
        structure def Top {
            sub x = Host(ms: some([Steel(), NotAMaterial()]))
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.iter().any(|d| {
            d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
                && d.message.contains("does not conform to trait")
                && d.message.contains("MaterialSpec")
                && d.message.contains("param 'ms'")
        }),
        "expected a TypeNotConformingToTrait error naming param 'ms' for NotAMaterial inside Option<List<MaterialSpec>>, got: {:?}",
        errors
    );
}

/// Positive test (gap #2 companion): an `Option<List<MaterialSpec>>` arg whose
/// inner list elements all conform must compile without errors. Pairs with
/// `option_list_trait_typed_param_rejects_non_conforming_inner_element` to lock
/// in the false-positive direction — a regression that emitted spurious
/// `TypeNotConformingToTrait` for `some([Steel(), Steel(...)])` would be caught
/// here. Mirrors `nested_wrapper_list_option_accepts_all_conforming_inner_elements`
/// (line 1124) with the wrapper order inverted.
#[test]
fn option_list_trait_typed_param_accepts_all_conforming_inner_elements() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name : String = "steel"
        }
        structure def Host { param ms : Option<List<MaterialSpec>> }
        structure def Top {
            sub x = Host(ms: some([Steel(), Steel(density: 1000kg/m^3)]))
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
        "expected no errors for some([Steel(), Steel(density: 1000kg/m^3)]) passed to Option<List<MaterialSpec>> param, got: {:?}",
        errors
    );
}

// ─── Parameterized builtins as trait member types (task 2908 esc-2908-87) ────
//
// `param x : Option<Pressure>` (and `List<T>`, `Set<T>`, `Map<K, V>`) was
// rejected as "unresolved type" when declared inside a trait body even though
// it worked inside a structure. Root cause: traits.rs::compile_trait routed
// member type resolution through the simple-name `resolve_type_with_aliases`
// and never consulted `type_args`, while structures use the full
// `resolve_type_expr_with_aliases` that handles parameterized builtins.
// These tests pin parity between trait-member and structure-member resolvers.

/// `param x : Option<Pressure>` in a trait body must resolve to
/// `Type::Option(Type::Scalar { dimension: PRESSURE })` with no diagnostics.
/// Direct repro of the FEA `ElasticMaterial.yield_stress` shape.
#[test]
fn trait_param_option_pressure_resolves_to_option_scalar() {
    let source = r#"
        trait ElasticLike {
            param yield_stress : Option<Pressure>
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
        "expected no errors for `param yield_stress : Option<Pressure>` in trait body, got: {:?}",
        errors
    );

    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElasticLike")
        .expect("ElasticLike trait should be compiled");

    let req = trait_def
        .required_members
        .iter()
        .find(|r| r.name == "yield_stress")
        .expect("yield_stress should be a required trait member");

    match &req.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(
                ty,
                &Type::Option(Box::new(Type::Scalar {
                    dimension: reify_core::DimensionVector::PRESSURE,
                })),
                "yield_stress trait member should resolve to Type::Option(Pressure-Scalar), got: {:?}",
                ty
            );
        }
        other => panic!("expected Param requirement kind, got: {:?}", other),
    }
}

/// `param ms : List<MaterialSpec>` in a trait body must resolve through the
/// parameterized-builtin path the same way it does in a structure body.
/// Companion to `trait_param_option_pressure_resolves_to_option_scalar`
/// covering the List<TraitObject> shape.
#[test]
fn trait_param_list_traitobject_resolves_through_parameterized_path() {
    let source = r#"
        trait HasMaterials {
            param ms : List<MaterialSpec>
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
        "expected no errors for `param ms : List<MaterialSpec>` in trait body, got: {:?}",
        errors
    );

    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "HasMaterials")
        .expect("HasMaterials trait should be compiled");

    let req = trait_def
        .required_members
        .iter()
        .find(|r| r.name == "ms")
        .expect("ms should be a required trait member");

    match &req.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(
                ty,
                &Type::List(Box::new(Type::TraitObject("MaterialSpec".to_string()))),
                "ms trait member should resolve to Type::List(Type::TraitObject(\"MaterialSpec\")), got: {:?}",
                ty
            );
        }
        other => panic!("expected Param requirement kind, got: {:?}", other),
    }
}

// ─── Parameterized builtins as conforming-structure member types (task 2908) ─
//
// Companion to the `trait_param_option_pressure_resolves_to_option_scalar` /
// `trait_param_list_traitobject_resolves_through_parameterized_path` pair that
// pinned the trait-side fix. The trait-side resolver and the conformance-side
// resolver each carry their own type-expression lookup; the previous fix only
// patched the trait side. When a structure declares `: SomeTrait` AND uses a
// parameterized builtin (`Option<Pressure>`, `List<TraitObject>`, ...) in one
// of its own member annotations, the conformance pass independently re-resolves
// that annotation to compare it against the trait's required-member type.
// `conformance/checker.rs::check_phase_resolve_structure_members` previously
// routed this re-resolution through the simple-name `resolve_type_with_aliases`
// and never consulted `type_args`, so any structure-side parameterized builtin
// was rejected as "unresolved type in conformance check" — even when the same
// shape resolved cleanly on the structure-compile path through `entity.rs`.
// These tests pin parity between the conformance-side and trait-side resolvers.

/// A conforming structure whose member declares `Option<Pressure>` must compile
/// with no errors. Direct repro of the FEA `Steel_AISI_1045 : ElasticMaterial
/// { param yield_stress : Option<Pressure> = some(310MPa) }` shape.
#[test]
fn structure_conforming_with_option_pressure_param_resolves_in_conformance_check() {
    let source = r#"
        trait HasYield {
            param yield_stress : Option<Pressure>
        }
        structure def MyMaterial : HasYield {
            param yield_stress : Option<Pressure> = some(250MPa)
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
        "expected no errors for `param yield_stress : Option<Pressure>` on a structure \
         conforming to a trait, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "MyMaterial")
        .expect("MyMaterial template should be compiled");

    assert!(
        template.trait_bounds.contains(&"HasYield".to_string()),
        "MyMaterial should carry 'HasYield' trait bound, got: {:?}",
        template.trait_bounds
    );
}

/// Companion test: a conforming structure whose member declares
/// `List<MaterialSpec>` must compile with no errors. Pins parity for the
/// `List<TraitObject>` shape on the conformance-side resolver.
#[test]
fn structure_conforming_with_list_traitobject_param_resolves_in_conformance_check() {
    let source = r#"
        trait HasMaterials {
            param ms : List<MaterialSpec>
        }
        structure def MyHolder : HasMaterials {
            param ms : List<MaterialSpec> = []
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
        "expected no errors for `param ms : List<MaterialSpec>` on a structure \
         conforming to a trait, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "MyHolder")
        .expect("MyHolder template should be compiled");

    assert!(
        template.trait_bounds.contains(&"HasMaterials".to_string()),
        "MyHolder should carry 'HasMaterials' trait bound, got: {:?}",
        template.trait_bounds
    );
}
