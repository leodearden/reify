//! Tests for monomorphization of resolved generic sub-components in the
//! `phase_auto_type_param_resolution` pass (task 4431, M-013 α).
//!
//! For each resolved `auto:` use-site, the compiler synthesizes a per-(generic,
//! resolved-type-args) MONOMORPH `TopologyTemplate`, substitutes
//! `Type::TypeParam(T)→Type::StructureRef(c)` into the clone's cells and body
//! expressions, strips its `type_params`, and rewrites the originating
//! `SubComponentDecl.structure_name` to the monomorph name.

use reify_config::Manifest;
use reify_core::{DiagnosticCode, ModulePath, Severity, Type};
use reify_test_support::{check_source_with_stdlib, compile_source_with_stdlib};

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
        sub_b.structure_name, "Bearing$GasketSeal",
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
    assert_eq!(
        errors.len(),
        0,
        "expected no error diagnostics, got: {:?}",
        errors
    );

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
    let sub_g1 = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "g1")
        .expect("sub g1");
    let sub_g2 = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "g2")
        .expect("sub g2");
    let sub_o = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "o")
        .expect("sub o");
    assert_eq!(
        sub_g1.structure_name, "Bearing$GasketSeal",
        "g1 must reference Bearing$GasketSeal"
    );
    assert_eq!(
        sub_g2.structure_name, "Bearing$GasketSeal",
        "g2 must reference Bearing$GasketSeal"
    );
    assert_eq!(
        sub_o.structure_name, "Bearing$ORingSeal",
        "o must reference Bearing$ORingSeal"
    );
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
    assert_eq!(
        errors.len(),
        0,
        "expected no error diagnostics, got: {:?}",
        errors
    );

    assert!(
        compiled
            .templates
            .iter()
            .any(|t| t.name == "Pair$FooA$BarB"),
        "expected monomorph 'Pair$FooA$BarB' in templates, got: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
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
    assert_eq!(
        errors.len(),
        0,
        "expected no error diagnostics, got: {:?}",
        errors
    );

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
        compiled
            .templates
            .iter()
            .any(|t| t.name == "Bearing$GasketSeal"),
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
            non_representable.push(format!(
                "value_cells '{}': {:?}",
                cell.id.member, cell.cell_type
            ));
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

// ─── amendment: sub_components.type_args coverage ────────────────────────────

/// α coverage: sub_components[*].type_args are substituted so that nested
/// generic instantiations like `sub inner = Inner<T>()` become
/// `Inner<StructureRef(GasketSeal)>` in the monomorph.
///
/// This pins the substitution introduced by the amendment pass for M-013 α.
/// Other collections (realizations, connections, objective, match_arm_groups,
/// forall_templates, assoc_fns) are documented as known α gaps in
/// auto_type_param_phase.rs and are NOT asserted here.
#[test]
fn monomorph_sub_component_type_args_are_substituted() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal {}
        structure def Inner<T: Seal> { param t : T }
        structure def Outer<T: Seal> { sub inner = Inner<T>() }
        structure def Assembly { sub o = Outer<auto: Seal>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

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

    let outer_mono = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer$GasketSeal")
        .expect("expected 'Outer$GasketSeal' monomorph");

    // The sub 'inner' in Outer$GasketSeal must reference Inner with
    // StructureRef("GasketSeal") in type_args — not TypeParam("T").
    let sub_inner = outer_mono
        .sub_components
        .iter()
        .find(|s| s.name == "inner")
        .expect("expected sub 'inner' in 'Outer$GasketSeal'");
    assert_eq!(
        sub_inner.type_args.first(),
        Some(&Type::StructureRef("GasketSeal".to_string())),
        "Outer$GasketSeal sub 'inner' type_args[0] must be StructureRef(GasketSeal), got: {:?}",
        sub_inner.type_args
    );

    // No TypeParam should remain in any sub_component type_args of the monomorph.
    let residual_typeparams: Vec<_> = outer_mono
        .sub_components
        .iter()
        .flat_map(|sub| {
            sub.type_args
                .iter()
                .filter(|t| matches!(t, Type::TypeParam(_)))
                .map(|t| format!("sub '{}' type_args: {:?}", sub.name, t))
        })
        .collect();
    assert!(
        residual_typeparams.is_empty(),
        "Outer$GasketSeal must have no TypeParam in sub_component type_args, found: {:?}",
        residual_typeparams
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

    let compiled = compile_source_with_stdlib(source);

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

/// Non-constructible candidate: a resolved candidate with a required (non-defaulted)
/// param emits `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE` and does NOT
/// synthesize a default for the type-param cell (leaves `default_expr = None`).
///
/// Fixture: `RequiredSeal : Seal { param thickness : Length }` — NO default on
/// `thickness`.  A single `RequiredSeal` candidate guarantees deterministic
/// resolution (1 feasible → Selected) under the stub checker.
///
/// RED until step-6 wires the constructibility guard into the monomorph-build
/// pass and emits the diagnostic.  After step-4 (constructible path only), the
/// NOT_CONSTRUCTIBLE case is silently skipped — no diagnostic is emitted, so
/// this test fails on the diagnostic-count assertion.
#[test]
fn non_constructible_candidate_emits_diagnostic_and_leaves_no_default() {
    let source = r#"
        trait Seal {}

        // Single candidate with a REQUIRED (no-default) param — non-constructible.
        structure def RequiredSeal : Seal {
            param thickness : Length
        }

        structure def Bearing<T: Seal> {
            param seal : T
        }

        structure def Asm {
            sub b = Bearing<auto(free): Seal>()
        }
    "#;

    let compiled = reify_test_support::compile_source_with_stdlib(source);

    // ── Assertion 1: exactly ONE Error diagnostic, code = AutoTypeParamCandidateNotConstructible ──
    let not_constructible_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::AutoTypeParamCandidateNotConstructible)
        })
        .collect();
    assert_eq!(
        not_constructible_errors.len(),
        1,
        "expected exactly 1 AutoTypeParamCandidateNotConstructible error, got {} diagnostics \
         with that code.  Full diagnostics: {:#?}",
        not_constructible_errors.len(),
        compiled.diagnostics
    );

    // The diagnostic message should name the missing param `thickness`.
    let diag = &not_constructible_errors[0];
    assert!(
        diag.message.contains("thickness"),
        "diagnostic message must name the missing param 'thickness', got: {:?}",
        diag.message
    );

    // ── Assertion 2: the monomorph's `seal` cell has default_expr = None ──
    //
    // No partial-Undef StructureInstance should be synthesized for a non-constructible
    // candidate (design decision 2 in the plan).
    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$RequiredSeal")
        .expect(
            "monomorph 'Bearing$RequiredSeal' must still be created even when non-constructible \
             (the synthesis guard fires after the clone is built)",
        );
    let seal_cell = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "seal")
        .expect("expected 'seal' value cell in 'Bearing$RequiredSeal'");
    assert!(
        seal_cell.default_expr.is_none(),
        "non-constructible candidate: 'seal' cell must have default_expr = None \
         (no partial-Undef StructureInstance synthesized), got: {:?}",
        seal_cell.default_expr
    );
}

/// Once-per-monomorph diagnostic emission: two use-sites resolving to the SAME
/// non-constructible candidate must produce EXACTLY ONE diagnostic, not two.
///
/// The δ synthesis loop runs inside `if created_monomorphs.insert(mono_name)`,
/// so it fires only on the FIRST use-site for each (generic, candidates) pair.
/// The second use-site reuses the already-built monomorph and skips synthesis.
/// This test pins that contract so a future refactor cannot accidentally move
/// synthesis outside the dedup branch and start emitting N duplicate diagnostics.
#[test]
fn non_constructible_two_use_sites_emits_one_diagnostic() {
    // Two sub-components resolving to the same non-constructible candidate.
    // Both `b1` and `b2` are `Bearing<auto(free): Seal>()` → Bearing$RequiredSeal.
    // The monomorph is deduplicated (created_monomorphs.insert returns false on the
    // second insertion), so the NotConstructible diagnostic fires exactly once.
    let source = r#"
        trait Seal {}

        structure def RequiredSeal : Seal {
            param thickness : Length
        }

        structure def Bearing<T: Seal> {
            param seal : T
        }

        structure def Asm {
            sub b1 = Bearing<auto(free): Seal>()
            sub b2 = Bearing<auto(free): Seal>()
        }
    "#;

    let compiled = reify_test_support::compile_source_with_stdlib(source);

    // Both use-sites resolve to Bearing$RequiredSeal — ONE monomorph, ONE diagnostic.
    let not_constructible_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::AutoTypeParamCandidateNotConstructible)
        })
        .collect();
    assert_eq!(
        not_constructible_errors.len(),
        1,
        "two use-sites resolving to the same non-constructible candidate must emit \
         EXACTLY 1 AutoTypeParamCandidateNotConstructible diagnostic (once per \
         monomorph, not once per use-site).  Full diagnostics: {:#?}",
        compiled.diagnostics
    );

    // There must be exactly ONE Bearing$RequiredSeal template (dedup held).
    let monomorphs: Vec<&str> = compiled
        .templates
        .iter()
        .filter(|t| t.name == "Bearing$RequiredSeal")
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        monomorphs.len(),
        1,
        "expected exactly one 'Bearing$RequiredSeal' monomorph template, got: {:?}",
        monomorphs
    );
}

// ─── task 6854: tighten partial-coverage guard beyond !sigma.is_empty() ──────

/// Depth-bound BFS-fallback partial resolution: `max_depth=1` forces a 2-param
/// `auto:` use-site into the v0.1 BFS fallback, which halts on U's
/// `NoCandidate` (no `Gasket` implementor exists) after already selecting T.
/// `resolve_auto_type_params_with_backtracking`'s joint-recheck only runs when
/// `outcome.substitution.len() == params.len()` (auto_type_param.rs:1586), so
/// this PARTIAL substitution (`{T: SealA}`, U unresolved) sails through
/// unchanged — today's `!sigma.is_empty()` guard in `auto_type_param_phase.rs`
/// still synthesizes a "Widget$SealA" monomorph with `type_params` cleared
/// while its `slot_u` cell keeps `Type::TypeParam("U")`.
///
/// RED today (#6854): the broken monomorph is synthesized and passes the old
/// guard. GREEN once the guard requires full `target.type_params` coverage.
#[test]
fn depth_bound_partial_resolution_synthesizes_no_monomorph() {
    let source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<auto: Seal, auto: Gasket>() }
    "#;

    let cfg = Manifest::from_toml_str("[auto_type_params]\nmax_depth = 1\n")
        .expect("valid manifest")
        .auto_type_params()
        .clone();

    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile_with_stdlib_with_config(&parsed, &cfg);

    // (1) No partial monomorph is synthesized.
    assert!(
        !compiled.templates.iter().any(|t| t.name == "Widget$SealA"),
        "partial resolution (T selected, U: NoCandidate) must NOT synthesize \
         'Widget$SealA'; got templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    // (2) WidgetAssembly's sub 'w' must still reference the generic template.
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "WidgetAssembly")
        .expect("expected 'WidgetAssembly' template");
    let sub_w = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "w")
        .expect("expected sub 'w' in 'WidgetAssembly'");
    assert_eq!(
        sub_w.structure_name, "Widget",
        "sub 'w' must still reference the generic 'Widget' template on partial \
         resolution, got: {:?}",
        sub_w.structure_name
    );

    // (3) General invariant: no '$'-named, type-params-empty template retains
    // a top-level TypeParam value cell.
    let leaks: Vec<String> = compiled
        .templates
        .iter()
        .filter(|t| t.name.contains('$') && t.type_params.is_empty())
        .flat_map(|t| {
            t.value_cells
                .iter()
                .filter(|c| matches!(&c.cell_type, Type::TypeParam(_)))
                .map(move |c| {
                    format!(
                        "template '{}' cell '{}': {:?}",
                        t.name, c.id.member, c.cell_type
                    )
                })
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "invariant violation: a '$'-named template with empty type_params \
         retains a TypeParam value cell: {:?}",
        leaks
    );

    // (4) The resolver's own diagnostic still fires — the fix must not
    // suppress it.
    let no_candidate_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate))
        .collect();
    assert!(
        !no_candidate_errors.is_empty(),
        "expected AutoTypeParamNoCandidate to still fire on U's zero-candidate \
         pool; got diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// Cross-product-cap BFS-fallback partial resolution: `max_cross_product_size=1`
/// with T (1 candidate) x U (2 candidates, `GasketA`/`GasketB`) = cross-product
/// size 2 > 1 forces the same BFS fallback. BFS selects T (`SealA`, sole
/// candidate) then hits U's ≥2-feasible-candidates `Ambiguous` outcome (strict
/// `auto:`, not `auto(free):`) and halts — again a PARTIAL substitution
/// (`{T: SealA}` only) that today's `!sigma.is_empty()` guard still turns into
/// a "Widget$SealA" monomorph with a leaked `Type::TypeParam("U")` slot_u cell.
///
/// RED today (#6854); GREEN once the guard requires full `target.type_params`
/// coverage.
#[test]
fn cross_product_cap_partial_resolution_synthesizes_no_monomorph() {
    let source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def GasketB : Gasket { param g : Real = 1.5 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<auto: Seal, auto: Gasket>() }
    "#;

    let cfg = Manifest::from_toml_str("[auto_type_params]\nmax_cross_product_size = 1\n")
        .expect("valid manifest")
        .auto_type_params()
        .clone();

    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile_with_stdlib_with_config(&parsed, &cfg);

    // (1) No partial monomorph is synthesized.
    assert!(
        !compiled.templates.iter().any(|t| t.name == "Widget$SealA"),
        "partial resolution (T selected, U: Ambiguous) must NOT synthesize \
         'Widget$SealA'; got templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    // (2) WidgetAssembly's sub 'w' must still reference the generic template.
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "WidgetAssembly")
        .expect("expected 'WidgetAssembly' template");
    let sub_w = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "w")
        .expect("expected sub 'w' in 'WidgetAssembly'");
    assert_eq!(
        sub_w.structure_name, "Widget",
        "sub 'w' must still reference the generic 'Widget' template on partial \
         resolution, got: {:?}",
        sub_w.structure_name
    );

    // (3) General invariant: no '$'-named, type-params-empty template retains
    // a top-level TypeParam value cell.
    let leaks: Vec<String> = compiled
        .templates
        .iter()
        .filter(|t| t.name.contains('$') && t.type_params.is_empty())
        .flat_map(|t| {
            t.value_cells
                .iter()
                .filter(|c| matches!(&c.cell_type, Type::TypeParam(_)))
                .map(move |c| {
                    format!(
                        "template '{}' cell '{}': {:?}",
                        t.name, c.id.member, c.cell_type
                    )
                })
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "invariant violation: a '$'-named template with empty type_params \
         retains a TypeParam value cell: {:?}",
        leaks
    );

    // (4) The resolver's own diagnostic still fires — the fix must not
    // suppress it.
    let ambiguous_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamAmbiguous))
        .collect();
    assert!(
        !ambiguous_errors.is_empty(),
        "expected AutoTypeParamAmbiguous to still fire on U's 2-feasible-candidate \
         pool; got diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// Mixed explicit + auto type-args (shape C, #6854): `Widget<SealA, auto: Gasket>()`
/// gives T a fully-explicit type-arg at the call site — `entity.rs` resolves it
/// directly to `Type::StructureRef("SealA")` in the sub's `type_args` WITHOUT ever
/// recording an `AutoClause` for T (only `Auto` type-args become clauses). So
/// `params.len() == 1` (U only) and `sigma.len() == 1` — step-2's
/// `sigma.len() == params.len()` check PASSES even though T, a declared type
/// parameter of `Widget`, was never substituted into the monomorph clone's own
/// `slot_t` cell.
///
/// Review round 2 (#6854) established that SKIPPING synthesis here — step-4's
/// behaviour — is unsafe rather than a safe degradation: the generic `Widget`
/// template is only harmless while nothing points at it, and
/// `assert_value_cell_types_representable` walks the hydrated graph, not
/// `compiled.templates`, so leaving `sub w` on the generic drags its
/// `Type::TypeParam` cells straight into a hydration-time panic (see the
/// eval-level sibling test below). An explicitly-supplied type-arg is not
/// unbound — it is already resolved, just not by this resolver — so the
/// correct fix SEEDS `sigma`/`candidates_by_position` from the sub's
/// already-resolved explicit type-arg, reaching full `target.type_params`
/// coverage and synthesizing a correct `Widget$SealA$GasketA` monomorph.
///
/// RED against HEAD (step-4's skip-on-partial guard, no seeding yet):
/// `Widget$SealA$GasketA` does not exist and `sub w` still references the
/// generic `Widget`. GREEN once step-6 lands the seeding.
#[test]
fn mixed_explicit_and_auto_type_args_synthesize_full_monomorph() {
    let source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<SealA, auto: Gasket>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // (1) The seeded monomorph IS synthesized, with BOTH slots substituted —
    // slot_t from the seeded explicit arg, slot_u from the resolver.
    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget$SealA$GasketA")
        .unwrap_or_else(|| {
            panic!(
                "expected seeded monomorph 'Widget$SealA$GasketA' in compiled.templates, \
                 got: {:?}",
                compiled
                    .templates
                    .iter()
                    .map(|t| &t.name)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        monomorph.type_params.is_empty(),
        "monomorph 'Widget$SealA$GasketA' must have no type_params, got: {:?}",
        monomorph.type_params
    );
    let slot_t = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "slot_t")
        .expect("expected 'slot_t' value cell in 'Widget$SealA$GasketA'");
    assert_eq!(
        slot_t.cell_type,
        Type::StructureRef("SealA".to_string()),
        "'slot_t' cell_type must be StructureRef(\"SealA\") — seeded from the \
         explicit type-arg, got: {:?}",
        slot_t.cell_type
    );
    let slot_u = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "slot_u")
        .expect("expected 'slot_u' value cell in 'Widget$SealA$GasketA'");
    assert_eq!(
        slot_u.cell_type,
        Type::StructureRef("GasketA".to_string()),
        "'slot_u' cell_type must be StructureRef(\"GasketA\"), got: {:?}",
        slot_u.cell_type
    );

    // (2) General invariant: no '$'-named, type-params-empty template retains
    // a top-level TypeParam value cell.
    let leaks: Vec<String> = compiled
        .templates
        .iter()
        .filter(|t| t.name.contains('$') && t.type_params.is_empty())
        .flat_map(|t| {
            t.value_cells
                .iter()
                .filter(|c| matches!(&c.cell_type, Type::TypeParam(_)))
                .map(move |c| {
                    format!(
                        "template '{}' cell '{}': {:?}",
                        t.name, c.id.member, c.cell_type
                    )
                })
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "invariant violation: a '$'-named template with empty type_params \
         retains a TypeParam value cell: {:?}",
        leaks
    );

    // (3) WidgetAssembly's sub 'w' now references the seeded monomorph, and
    // its type_args are unchanged by seeding (the explicit T slot was never
    // rewritten — it already held its final StructureRef).
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "WidgetAssembly")
        .expect("expected 'WidgetAssembly' template");
    let sub_w = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "w")
        .expect("expected sub 'w' in 'WidgetAssembly'");
    assert_eq!(
        sub_w.structure_name, "Widget$SealA$GasketA",
        "sub 'w' must reference the seeded monomorph once full coverage is \
         reached, got: {:?}",
        sub_w.structure_name
    );
    assert_eq!(
        sub_w.type_args,
        vec![
            Type::StructureRef("SealA".to_string()),
            Type::StructureRef("GasketA".to_string()),
        ],
        "sub 'w' type_args must remain [StructureRef(SealA), StructureRef(GasketA)] \
         — unchanged by seeding, got: {:?}",
        sub_w.type_args
    );

    // (4) Zero Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "expected zero error diagnostics for this seeded mixed shape, got: {:?}",
        errors
    );
}

/// Companion to the test above, and the EXACT shape review round 2 used to
/// demonstrate the step-4 regression: T is PHANTOM w.r.t. top-level value
/// cells — `Widget` declares no `slot_t` at all, only `slot_u : U`. Measured
/// on this worktree: this source compiled AND evaluated cleanly (0
/// diagnostics) under the ORIGINAL pre-#6854 `!sigma.is_empty()` guard, and
/// PANICS under step-4's skip-on-partial guard at
/// `crates/reify-eval/src/engine_eval.rs:210`
/// (`unrepresentable cell_type: value cell 'Widget.slot_u' has cell_type
/// TypeParam("U")`) — because skipping synthesis leaves `sub w` pointing at
/// the generic `Widget` template, which is exactly what drags its
/// `Type::TypeParam` cell into the hydrated graph. See the eval-level sibling
/// test below for the assertion that actually catches this.
///
/// RED against HEAD for the same reason as the non-phantom sibling: no
/// `Widget$SealA$GasketA` monomorph is synthesized. GREEN once step-6 lands.
#[test]
fn mixed_explicit_and_auto_type_args_phantom_param_synthesize_full_monomorph() {
    let source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<SealA, auto: Gasket>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget$SealA$GasketA")
        .unwrap_or_else(|| {
            panic!(
                "expected seeded monomorph 'Widget$SealA$GasketA' in compiled.templates \
                 (phantom T), got: {:?}",
                compiled
                    .templates
                    .iter()
                    .map(|t| &t.name)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        monomorph.type_params.is_empty(),
        "monomorph 'Widget$SealA$GasketA' must have no type_params, got: {:?}",
        monomorph.type_params
    );
    let slot_u = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "slot_u")
        .expect("expected 'slot_u' value cell in 'Widget$SealA$GasketA'");
    assert_eq!(
        slot_u.cell_type,
        Type::StructureRef("GasketA".to_string()),
        "'slot_u' cell_type must be StructureRef(\"GasketA\"), got: {:?}",
        slot_u.cell_type
    );

    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "WidgetAssembly")
        .expect("expected 'WidgetAssembly' template");
    let sub_w = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "w")
        .expect("expected sub 'w' in 'WidgetAssembly'");
    assert_eq!(
        sub_w.structure_name, "Widget$SealA$GasketA",
        "sub 'w' must reference the seeded monomorph even though T is phantom \
         w.r.t. top-level value cells, got: {:?}",
        sub_w.structure_name
    );
}

/// The assertion steps 1-4 were missing, and the one that would have caught
/// the regression before review round 2: `compile_source_with_stdlib` alone
/// cannot see the shape-C defect, because the leaked `Type::TypeParam` sits on
/// the GENERIC `Widget` template — always present in `compiled.templates` and
/// harmless until a sub points at it — while
/// `assert_value_cell_types_representable` runs over the HYDRATED GRAPH, not
/// over `compiled.templates`. Only `check_source_with_stdlib` (compile +
/// evaluate) can distinguish "sound compile, sound eval" from "sound compile,
/// panics at hydration".
///
/// RED against HEAD: both the non-phantom and phantom sources panic inside
/// `check_source_with_stdlib` at `engine_eval.rs:210`
/// (`unrepresentable cell_type ... TypeParam(...) post-compilation`), because
/// step-4's guard skips synthesis and leaves `sub w` on the generic `Widget`
/// template. GREEN once step-6 seeds `sigma` so a correct monomorph is
/// synthesized and `sub w` points at it instead.
#[test]
fn mixed_explicit_and_auto_type_args_evaluate_without_typeparam_leak() {
    let non_phantom_source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<SealA, auto: Gasket>() }
    "#;
    let phantom_source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<SealA, auto: Gasket>() }
    "#;

    for (label, source) in [
        ("non-phantom", non_phantom_source),
        ("phantom", phantom_source),
    ] {
        let result = check_source_with_stdlib(source);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(
            errors.len(),
            0,
            "[{label}] expected zero error diagnostics evaluating the mixed \
             explicit+auto shape, got: {:?}",
            errors
        );
    }
}

/// Residual un-seedable skip path (#6854): the seeding loop requires a
/// concrete `Type::StructureRef` at the explicit position, but a use-site
/// nested inside another generic can supply an ENCLOSING generic's own
/// `Type::TypeParam` instead. In
/// `structure def Outer<X: Seal> { sub w = Widget<X, auto: Gasket>() }`, `X`
/// is `Outer`'s own declared type parameter, not a concrete structure — so
/// `sub.type_args[0]` is `Type::TypeParam("X")`, which the seeding loop's
/// `if let Some(Type::StructureRef(name)) = ...` pattern does not match.
/// `T` is therefore never added to `sigma`, coverage stays partial, and
/// synthesis is (still, correctly) skipped — but that skip is currently
/// SILENT: zero diagnostics, and `check_source_with_stdlib` panics at
/// hydration once something instantiates `Outer` concretely.
///
/// This is a PRE-EXISTING gap on main, not a #6854 regression: nothing
/// monomorphizes `Outer` itself (`Top`'s use-site `Outer<SealA>()` carries no
/// `auto:` clause, so it never enters this phase at all), so the original
/// `!sigma.is_empty()` guard panics on this source too. Closing it properly
/// means general monomorphization of explicitly-instantiated generics, which
/// is out of scope here — this test's contract is narrower: make the
/// compiler LOUD rather than silent about the gap.
///
/// RED after step-6: this source compiles with zero Error diagnostics today.
/// GREEN once step-8 adds the residual-skip diagnostic.
#[test]
fn unseedable_explicit_type_arg_emits_diagnostic_rather_than_silent_leak() {
    let source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def Outer<X: Seal> { sub w = Widget<X, auto: Gasket>() }
        structure def Top { sub o = Outer<SealA>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // (1) Exactly one Error diagnostic, naming the unbound type parameter
    // ('T'), the target ('Widget'), and the owner sub-component ('w') —
    // enough for a user to locate the use-site. Substring checks are
    // quote-delimited so they cannot false-positive on stray letters
    // elsewhere in the prose (e.g. the word "would").
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error diagnostic for the un-seedable explicit \
         type-arg, got: {:?}",
        compiled.diagnostics
    );
    let message = &errors[0].message;
    assert!(
        message.contains("\"T\""),
        "diagnostic message must name the unbound type parameter 'T', got: {message:?}"
    );
    assert!(
        message.contains("'Widget'"),
        "diagnostic message must name the target 'Widget', got: {message:?}"
    );
    assert!(
        message.contains("'w'"),
        "diagnostic message must name the owner sub-component 'w', got: {message:?}"
    );

    // (2) No monomorph template is synthesized — partial coverage still
    // skips synthesis; this step changes only whether the skip is reported.
    assert!(
        !compiled
            .templates
            .iter()
            .any(|t| t.name.starts_with("Widget$")),
        "un-seedable partial coverage must NOT synthesize a 'Widget$...' \
         monomorph; got templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    // (3) Outer's sub 'w' still references the generic 'Widget' template.
    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("expected 'Outer' template");
    let sub_w = outer
        .sub_components
        .iter()
        .find(|s| s.name == "w")
        .expect("expected sub 'w' in 'Outer'");
    assert_eq!(
        sub_w.structure_name, "Widget",
        "sub 'w' must still reference the generic 'Widget' template, got: {:?}",
        sub_w.structure_name
    );
}

/// Companion assertion pinning the diagnostic-gate design (reviewer detail
/// (b)): the residual-skip diagnostic must NOT double-report on shapes A/B
/// (step-1), which already carry the resolver's own
/// `AutoTypeParamNoCandidate` / `AutoTypeParamAmbiguous` error. The gate is
/// `params.iter().all(|p| sigma.contains_key(&p.name))` — "the resolver
/// bound everything it was asked to bind" — which shapes A/B fail (their
/// sole `auto:`-clause param was never bound), so they must never reach the
/// new diagnostic. A `diagnostics.len()`-based gate (snapshot before the
/// resolver call, scan the tail for new errors) would be fragile to the
/// severity the resolver assigns each halt reason; this assertion is what
/// would catch that fragility slipping through.
#[test]
fn shape_a_and_shape_b_partial_resolution_emit_no_additional_diagnostic() {
    let depth_bound_source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<auto: Seal, auto: Gasket>() }
    "#;
    let depth_cfg = Manifest::from_toml_str("[auto_type_params]\nmax_depth = 1\n")
        .expect("valid manifest")
        .auto_type_params()
        .clone();
    let depth_parsed =
        reify_compiler::parse_with_stdlib(depth_bound_source, ModulePath::single("test"));
    let depth_compiled = reify_compiler::compile_with_stdlib_with_config(&depth_parsed, &depth_cfg);
    let depth_errors: Vec<_> = depth_compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !depth_errors.is_empty()
            && depth_errors
                .iter()
                .all(|d| d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate)),
        "shape A (depth-bound) must emit only its own AutoTypeParamNoCandidate \
         error(s) and no additional residual-skip diagnostic, got: {:?}",
        depth_errors
    );

    let cross_product_source = r#"
        trait Seal {}
        trait Gasket {}
        structure def SealA : Seal { param d : Real = 2.0 }
        structure def GasketA : Gasket { param g : Real = 1.0 }
        structure def GasketB : Gasket { param g : Real = 1.5 }
        structure def Widget<T: Seal, U: Gasket> { param slot_t : T  param slot_u : U }
        structure def WidgetAssembly { sub w = Widget<auto: Seal, auto: Gasket>() }
    "#;
    let cross_product_cfg =
        Manifest::from_toml_str("[auto_type_params]\nmax_cross_product_size = 1\n")
            .expect("valid manifest")
            .auto_type_params()
            .clone();
    let cross_product_parsed =
        reify_compiler::parse_with_stdlib(cross_product_source, ModulePath::single("test"));
    let cross_product_compiled =
        reify_compiler::compile_with_stdlib_with_config(&cross_product_parsed, &cross_product_cfg);
    let cross_product_errors: Vec<_> = cross_product_compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !cross_product_errors.is_empty()
            && cross_product_errors
                .iter()
                .all(|d| d.code == Some(DiagnosticCode::AutoTypeParamAmbiguous)),
        "shape B (cross-product cap) must emit only its own AutoTypeParamAmbiguous \
         error(s) and no additional residual-skip diagnostic, got: {:?}",
        cross_product_errors
    );
}
