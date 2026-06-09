//! Tests for monomorphization of resolved generic sub-components in the
//! `phase_auto_type_param_resolution` pass (task 4431, M-013 α).
//!
//! For each resolved `auto:` use-site, the compiler synthesizes a per-(generic,
//! resolved-type-args) MONOMORPH `TopologyTemplate`, substitutes
//! `Type::TypeParam(T)→Type::StructureRef(c)` into the clone's cells and body
//! expressions, strips its `type_params`, and rewrites the originating
//! `SubComponentDecl.structure_name` to the monomorph name.

use reify_core::{Severity, Type};
use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};

/// Keystone test: a single `auto:` use-site produces a monomorph template.
///
/// Invariant 1 (partial coverage — top-level value_cells only until step-8):
///   No value cell reachable from the resolved sub-component carries `Type::TypeParam`.
///
/// RED until step-2 (no monomorph template is created before the implementation).
#[test]
fn single_auto_use_site_produces_monomorph() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // Zero error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "expected no error diagnostics, got: {:?}",
        errors
    );

    // The monomorph template must exist.
    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$GasketSeal")
        .expect("expected monomorph template 'Bearing$GasketSeal' in compiled.templates");

    // It must have no type parameters (it is a concrete instance).
    assert!(
        monomorph.type_params.is_empty(),
        "monomorph 'Bearing$GasketSeal' must have no type_params, got: {:?}",
        monomorph.type_params
    );

    // The 'seal' value cell must have cell_type == StructureRef("GasketSeal").
    let seal_cell = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "seal")
        .expect("expected 'seal' value cell in 'Bearing$GasketSeal'");
    assert_eq!(
        seal_cell.cell_type,
        Type::StructureRef("GasketSeal".to_string()),
        "'seal' cell_type must be StructureRef(\"GasketSeal\"), got: {:?}",
        seal_cell.cell_type
    );

    // Assembly's sub 'b' must reference the monomorph.
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("expected 'Assembly' template");
    let sub_b = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("expected sub 'b' in 'Assembly'");
    assert_eq!(
        sub_b.structure_name,
        "Bearing$GasketSeal",
        "sub 'b' must reference the monomorph 'Bearing$GasketSeal', got: {:?}",
        sub_b.structure_name
    );
    assert_eq!(
        sub_b.type_args.first(),
        Some(&Type::StructureRef("GasketSeal".to_string())),
        "sub 'b' type_args[0] must be StructureRef(\"GasketSeal\"), got: {:?}",
        sub_b.type_args
    );
}

// ─── step-3: dedup + determinism + multi-param position-order ─────────────────

/// Identical `auto:` instantiations at different sub-sites deduplicate to ONE
/// monomorph template; distinct instantiations produce separate monomorphs.
///
/// RED until step-4 (without dedup, g1 and g2 each push their own
/// "Bearing$GasketSeal" clone, giving two entries instead of one).
#[test]
fn identical_instantiations_dedupe_distinct_do_not() {
    let source = r#"
        trait Seal {}
        trait Gasket : Seal {}
        trait ORing : Seal {}
        structure def GasketSeal : Gasket {}
        structure def ORingSeal : ORing {}
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly {
            sub g1 = Bearing<auto: Gasket>()
            sub g2 = Bearing<auto: Gasket>()
            sub o  = Bearing<auto: ORing>()
        }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // Zero error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 0, "expected no error diagnostics, got: {:?}", errors);

    // EXACTLY ONE Bearing$GasketSeal template (g1, g2 deduplicate).
    let gasket_monomorphs: Vec<&str> = compiled
        .templates
        .iter()
        .filter(|t| t.name == "Bearing$GasketSeal")
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        gasket_monomorphs.len(),
        1,
        "g1 and g2 must deduplicate to EXACTLY ONE 'Bearing$GasketSeal', got: {:?}",
        gasket_monomorphs
    );

    // EXACTLY ONE Bearing$ORingSeal template.
    let oring_monomorphs: Vec<&str> = compiled
        .templates
        .iter()
        .filter(|t| t.name == "Bearing$ORingSeal")
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        oring_monomorphs.len(),
        1,
        "expected EXACTLY ONE 'Bearing$ORingSeal' template, got: {:?}",
        oring_monomorphs
    );

    // Prove that sharing the template is WRONG without monomorphization: the two
    // monomorphs' 'seal' cells must each carry the correct StructureRef.
    let gasket_mono = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$GasketSeal")
        .unwrap();
    let gasket_seal_cell = gasket_mono
        .value_cells
        .iter()
        .find(|c| c.id.member == "seal")
        .expect("expected 'seal' cell in Bearing$GasketSeal");
    assert_eq!(
        gasket_seal_cell.cell_type,
        Type::StructureRef("GasketSeal".to_string()),
        "Bearing$GasketSeal 'seal' cell_type must be StructureRef(GasketSeal)"
    );

    let oring_mono = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$ORingSeal")
        .unwrap();
    let oring_seal_cell = oring_mono
        .value_cells
        .iter()
        .find(|c| c.id.member == "seal")
        .expect("expected 'seal' cell in Bearing$ORingSeal");
    assert_eq!(
        oring_seal_cell.cell_type,
        Type::StructureRef("ORingSeal".to_string()),
        "Bearing$ORingSeal 'seal' cell_type must be StructureRef(ORingSeal)"
    );

    // g1 and g2 both point at the shared monomorph; o points at the ORing one.
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("expected 'Assembly' template");
    let sub_g1 = assembly.sub_components.iter().find(|s| s.name == "g1").expect("sub g1");
    let sub_g2 = assembly.sub_components.iter().find(|s| s.name == "g2").expect("sub g2");
    let sub_o  = assembly.sub_components.iter().find(|s| s.name == "o").expect("sub o");
    assert_eq!(sub_g1.structure_name, "Bearing$GasketSeal", "g1 must reference Bearing$GasketSeal");
    assert_eq!(sub_g2.structure_name, "Bearing$GasketSeal", "g2 must reference Bearing$GasketSeal");
    assert_eq!(sub_o.structure_name,  "Bearing$ORingSeal",  "o must reference Bearing$ORingSeal");
}

/// Invariant 3: the mono name is a pure function of (generic, ordered candidates).
/// Compiling the same source twice must produce identical sets of `$`-named templates.
///
/// GREEN from step-2 onwards (the mangle is deterministic by construction).
#[test]
fn mono_name_deterministic_across_compiles() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal {}
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled1 = compile_source_with_stdlib(source);
    let compiled2 = compile_source_with_stdlib(source);

    let names1: std::collections::BTreeSet<&str> = compiled1
        .templates
        .iter()
        .filter(|t| t.name.contains('$'))
        .map(|t| t.name.as_str())
        .collect();
    let names2: std::collections::BTreeSet<&str> = compiled2
        .templates
        .iter()
        .filter(|t| t.name.contains('$'))
        .map(|t| t.name.as_str())
        .collect();

    assert_eq!(
        names1, names2,
        "two compiles of identical source must produce identical monomorph name sets"
    );
    assert!(
        names1.contains("Bearing$GasketSeal"),
        "expected 'Bearing$GasketSeal' in monomorph name set, got: {:?}",
        names1
    );
}

/// Multi-param: candidates are joined in type-param POSITION order (not source order).
/// `Pair<X: A, Y: B>` with `FooA : A` and `BarB : B` must produce `Pair$FooA$BarB`.
///
/// GREEN from step-2 onwards (candidates_by_position is sorted before mangle).
#[test]
fn multi_param_monomorph_uses_position_order() {
    let source = r#"
        trait A {}
        trait B {}
        structure def FooA : A {}
        structure def BarB : B {}
        structure def Pair<X: A, Y: B> { param x : X  param y : Y }
        structure def Asm { sub p = Pair<auto: A, auto: B>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 0, "expected no error diagnostics, got: {:?}", errors);

    assert!(
        compiled.templates.iter().any(|t| t.name == "Pair$FooA$BarB"),
        "expected monomorph 'Pair$FooA$BarB' in templates, got: {:?}",
        compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

// ─── step-5: expr result_type substitution ────────────────────────────────────

/// Monomorph body expressions must have NO node whose `result_type` is
/// `Type::TypeParam(_)`. This covers value-cell `default_expr`s and
/// constraint exprs.
///
/// Fixture adds `let seal_ref = seal` which produces a `ValueRef(seal)` node
/// with `result_type == Type::TypeParam("T")` in the generic template; after
/// monomorphization it must be `Type::StructureRef("GasketSeal")`.
///
/// RED until step-6 adds `substitute_expr_result_types`.
#[test]
fn monomorph_body_exprs_have_no_typeparam_result_type() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal {}
        structure def Bearing<T: Seal> {
            param seal : T
            let seal_ref = seal
        }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 0, "expected no error diagnostics, got: {:?}", errors);

    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$GasketSeal")
        .expect("expected 'Bearing$GasketSeal' monomorph");

    // Walk every value-cell default_expr and every constraint expr: none may
    // carry result_type == TypeParam(_).
    let mut typeparam_result_types: Vec<String> = Vec::new();
    for cell in &monomorph.value_cells {
        if let Some(expr) = &cell.default_expr {
            expr.walk(&mut |node| {
                if matches!(&node.result_type, Type::TypeParam(_)) {
                    typeparam_result_types.push(format!(
                        "value_cell '{}' expr node result_type={:?}",
                        cell.id.member, node.result_type
                    ));
                }
            });
        }
    }
    for (i, constraint) in monomorph.constraints.iter().enumerate() {
        constraint.expr.walk(&mut |node| {
            if matches!(&node.result_type, Type::TypeParam(_)) {
                typeparam_result_types.push(format!(
                    "constraint[{}] expr node result_type={:?}",
                    i, node.result_type
                ));
            }
        });
    }

    assert!(
        typeparam_result_types.is_empty(),
        "Bearing$GasketSeal monomorph must have no TypeParam result_type nodes in exprs, found: {:?}",
        typeparam_result_types
    );

    // Specific check: seal_ref's default_expr root node result_type is
    // StructureRef("GasketSeal") after substitution.
    let seal_ref_cell = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "seal_ref")
        .expect("expected 'seal_ref' value cell in Bearing$GasketSeal");
    assert_eq!(
        seal_ref_cell.cell_type,
        Type::StructureRef("GasketSeal".to_string()),
        "'seal_ref' cell_type must be StructureRef(GasketSeal)"
    );
    let seal_ref_expr = seal_ref_cell
        .default_expr
        .as_ref()
        .expect("'seal_ref' let must have a default_expr");
    assert_eq!(
        seal_ref_expr.result_type,
        Type::StructureRef("GasketSeal".to_string()),
        "'seal_ref' default_expr root result_type must be StructureRef(GasketSeal), got: {:?}",
        seal_ref_expr.result_type
    );
}

// ─── step-7: guarded-group + port cell_type coverage (invariant 1 full sweep) ─

/// Full representability smoke — invariant 1 over guarded-group members.
///
/// Fixture: a generic whose guarded branch (`if use_premium`) carries a
/// value-cell typed `T`.  After monomorphization, that cell must have
/// `cell_type == StructureRef("GasketSeal")`, NOT `TypeParam("T")`.
///
/// PRIMARY assertion: for EVERY concrete template (`type_params.is_empty()`),
/// no value cell across `value_cells`, `guarded_groups[*].members`,
/// `guarded_groups[*].else_members`, or `ports[*].members` carries
/// `Type::TypeParam(_)`.
///
/// SECONDARY positive check: every cell in the `Bearing$GasketSeal` monomorph
/// passes `reify_eval::is_representable_cell_type` (safe because all cells are
/// StructureRef post-resolution — no Union/Keyed caveat applies to the
/// monomorph's own cells).
///
/// RED until step-8 extends the clone substitution to guarded-group members.
#[test]
fn resolved_subcomponent_has_no_typeparam_cell() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal {}
        structure def Bearing<T: Seal> {
            param use_premium : Bool = true
            where use_premium { param seal : T }
        }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // Zero error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "expected no error diagnostics, got: {:?}",
        errors
    );

    // The monomorph must exist.
    assert!(
        compiled.templates.iter().any(|t| t.name == "Bearing$GasketSeal"),
        "expected 'Bearing$GasketSeal' in templates"
    );

    // PRIMARY: for every CONCRETE template, no cell in any collection may carry
    // `Type::TypeParam(_)`.  We deliberately use `matches!` instead of
    // `is_representable_cell_type` here to avoid the Union/Keyed false-positive
    // caveat — stdlib concrete templates may legitimately carry Union cells that
    // pre-date this invariant; TypeParam is the only variant that α must fix.
    let mut typeparam_cells: Vec<String> = Vec::new();
    for tmpl in &compiled.templates {
        if !tmpl.type_params.is_empty() {
            // Abstract generic — TypeParam cells are intentionally present.
            continue;
        }
        // value_cells
        for cell in &tmpl.value_cells {
            if matches!(&cell.cell_type, Type::TypeParam(_)) {
                typeparam_cells.push(format!(
                    "template '{}' value_cells cell '{}': {:?}",
                    tmpl.name, cell.id.member, cell.cell_type
                ));
            }
        }
        // guarded_groups[*].members + .else_members
        for (gi, group) in tmpl.guarded_groups.iter().enumerate() {
            for cell in &group.members {
                if matches!(&cell.cell_type, Type::TypeParam(_)) {
                    typeparam_cells.push(format!(
                        "template '{}' guarded_groups[{}].members cell '{}': {:?}",
                        tmpl.name, gi, cell.id.member, cell.cell_type
                    ));
                }
            }
            for cell in &group.else_members {
                if matches!(&cell.cell_type, Type::TypeParam(_)) {
                    typeparam_cells.push(format!(
                        "template '{}' guarded_groups[{}].else_members cell '{}': {:?}",
                        tmpl.name, gi, cell.id.member, cell.cell_type
                    ));
                }
            }
        }
        // ports[*].members
        for (pi, port) in tmpl.ports.iter().enumerate() {
            for cell in &port.members {
                if matches!(&cell.cell_type, Type::TypeParam(_)) {
                    typeparam_cells.push(format!(
                        "template '{}' ports[{}].members cell '{}': {:?}",
                        tmpl.name, pi, cell.id.member, cell.cell_type
                    ));
                }
            }
        }
    }
    assert!(
        typeparam_cells.is_empty(),
        "invariant-1 violation: TypeParam cell_types found in concrete templates: {:?}",
        typeparam_cells
    );

    // SECONDARY: the monomorph's own cells must all pass is_representable_cell_type.
    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$GasketSeal")
        .unwrap();
    let mut non_representable: Vec<String> = Vec::new();
    for cell in &monomorph.value_cells {
        if !reify_eval::is_representable_cell_type(&cell.cell_type) {
            non_representable.push(format!("value_cells '{}': {:?}", cell.id.member, cell.cell_type));
        }
    }
    for (gi, group) in monomorph.guarded_groups.iter().enumerate() {
        for cell in &group.members {
            if !reify_eval::is_representable_cell_type(&cell.cell_type) {
                non_representable.push(format!(
                    "guarded_groups[{}].members '{}': {:?}",
                    gi, cell.id.member, cell.cell_type
                ));
            }
        }
        for cell in &group.else_members {
            if !reify_eval::is_representable_cell_type(&cell.cell_type) {
                non_representable.push(format!(
                    "guarded_groups[{}].else_members '{}': {:?}",
                    gi, cell.id.member, cell.cell_type
                ));
            }
        }
    }
    assert!(
        non_representable.is_empty(),
        "Bearing$GasketSeal monomorph has non-representable cell types: {:?}",
        non_representable
    );
}

// ─── regression lock ──────────────────────────────────────────────────────────

/// Regression lock (invariant 2): a module with no `auto:` use-sites produces
/// zero monomorph templates and leaves `ctx.templates` unchanged.
///
/// The empty-queue early-return at `auto_type_param_phase.rs:83` guarantees this.
/// This test pins that invariant so a future refactor cannot accidentally
/// introduce monomorphs for non-`auto:` modules.
#[test]
fn no_auto_module_produces_zero_monomorphs() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing { param d : Real = 10.0 }
        structure def Assembly { sub b = Bearing() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    // No template name should contain '$' (the monomorph name separator).
    let monomorphs: Vec<&str> = compiled
        .templates
        .iter()
        .filter(|t| t.name.contains('$'))
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        monomorphs.is_empty(),
        "no-auto: module must produce zero monomorph templates (none with '$' in name), got: {:?}",
        monomorphs
    );
}
