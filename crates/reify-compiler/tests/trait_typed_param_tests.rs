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
use reify_types::{Severity, Type};

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
            param density : Real = 7850.0
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
/// call with named arguments (e.g. `Host(m: Steel(density: 1000.0))`), and the
/// inner call's callee participates in trait conformance as a `StructureRef`.
/// The discriminator vs. the existing `...accepts_conforming_struct` test is
/// the inner call's named args: `Steel(density: 1000.0)` rather than `Steel()`.
/// Zero Error-severity diagnostics expected.
#[test]
fn sub_component_arg_structure_instantiation_with_args_accepted() {
    let source = r#"
        structure def Steel : MaterialSpec {
            param density : Real = 7850.0
            param name : String = "steel"
        }
        structure def Host { param m : MaterialSpec }
        structure def Top {
            sub x = Host(m: Steel(density: 1000.0))
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
            param density : Real = 7850.0
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
            param density : Real = 7850.0
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
            param density : Real = 7850.0
            param name : String = "steel"
        }
        structure def Host { param ms : List<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: [Steel(), Steel(density: 1000.0)])
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
        "expected no errors for [Steel(), Steel(density: 1000.0)] passed to List<MaterialSpec> param, got: {:?}",
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
            param density : Real = 7850.0
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
            param density : Real = 7850.0
            param name : String = "steel"
        }
        structure def Host { param ms : Set<MaterialSpec> }
        structure def Top {
            sub x = Host(ms: set{Steel(), Steel(density: 1000.0)})
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
        "expected no errors for set{{Steel(), Steel(density: 1000.0)}} passed to Set<MaterialSpec> param, got: {:?}",
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
            param density : Real = 7850.0
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
            param density : Real = 7850.0
            param name : String = "steel"
        }
        structure def Host { param ms : Map<String, MaterialSpec> }
        structure def Top {
            sub x = Host(ms: map{"a" => Steel(), "b" => Steel(density: 1000.0)})
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
