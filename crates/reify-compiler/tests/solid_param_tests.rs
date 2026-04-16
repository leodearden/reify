//! Tests for `param x : Solid = <geometry_call>` compilation.
//!
//! A `Solid`-typed param with a geometry-call default should be lowered as a
//! realization (like a geometry let) rather than a scalar ValueCellDecl.

use reify_compiler::TopologyTemplate;
use reify_types::Severity;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_solid_param"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:#?}",
        errors
    );
    compiled
}

/// Like `compile_no_errors` but returns the `CompiledModule` without asserting
/// that diagnostics are absent.  Used by pin-down tests that intentionally
/// inspect whatever diagnostic behavior the compiler currently exhibits, so
/// that any future change to that behavior becomes a deliberate, reviewable
/// test update rather than a silent semantic drift.
fn compile_allowing_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_solid_param"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ─── step-5: Solid-typed param must NOT emit a ValueCellDecl ─────────────────

/// After the pre-pass extension (step-4), scope registers `g` as Type::Geometry.
/// The main Param loop must also skip ValueCellDecl construction so that `g`
/// appears nowhere in `template.value_cells`.
/// Expect failure until the main-loop early-continue (step-6) is implemented.
#[test]
fn solid_param_has_no_value_cell() {
    let source = r#"structure def Widget {
    param g : Solid = cylinder(10mm, 20mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template not found");

    // The geometry param must NOT produce a scalar ValueCellDecl.
    assert!(
        !template.value_cells.iter().any(|c| c.id.member == "g"),
        "ValueCellDecl for 'g' must not exist; Solid-typed params with geometry \
         defaults should be lowered as realizations only"
    );
}

// ─── step-7: Guarded Solid-typed param must not emit a ValueCellDecl ─────────

/// A `Solid`-typed param inside a block-level `where` guard must behave the same
/// as a geometry let in a guarded block: it must NOT appear as a `ValueCellDecl`
/// in the guarded group's `members`, and must produce a `RealizationDecl` in the
/// template's top-level realizations list.
///
/// Expect failure until `guards.rs` is updated (step-8):
/// - `register_guarded_names` currently does not add Solid params to
///   `known_geometry_lets`, so the guarded-members pass treats `g` as a
///   regular scalar param and emits a `ValueCellDecl`.
/// - No realization is emitted for the guarded geometry param.
#[test]
fn guarded_solid_param_compiles_as_realization() {
    let source = r#"structure def W {
    param some_cond : Bool = true
    where some_cond {
        param g : Solid = cylinder(10mm, 20mm)
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W")
        .expect("W template not found");

    // (a) `g` must NOT appear as a ValueCellDecl in top-level value_cells.
    assert!(
        !template.value_cells.iter().any(|c| c.id.member == "g"),
        "top-level ValueCellDecl for 'g' must not exist"
    );
    // (b) `g` must NOT appear in any guarded group's members.
    for group in &template.guarded_groups {
        assert!(
            !group.members.iter().any(|m| m.id.member == "g"),
            "guarded ValueCellDecl for 'g' must not exist; Solid-typed guarded params \
             should be lowered as realizations, not scalar value cells"
        );
    }
    // (c) At least one RealizationDecl must be emitted for the guarded geometry param.
    assert!(
        !template.realizations.is_empty(),
        "expected at least one RealizationDecl for guarded `param g : Solid = cylinder(...)`, \
         got none"
    );
}

// ─── step-3: Solid-typed param should lower to a realization ─────────────────

/// `param g : Solid = cylinder(10mm, 20mm)` must:
/// (a) compile without errors,
/// (b) produce NO ValueCellDecl named `g`,
/// (c) produce exactly 1 RealizationDecl,
/// (d) register `g` as Type::Geometry (verified indirectly: the cell_type of
///     any value cell named `g` must not exist, since the param is a realization).
#[test]
fn solid_param_compiles_as_realization() {
    let source = r#"structure def Widget {
    param g : Solid = cylinder(10mm, 20mm)
}"#;
    let compiled = compile_no_errors(source);

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template not found");

    // (b) No value_cell named "g" — it should be a realization, not a scalar cell.
    let has_g_cell = template
        .value_cells
        .iter()
        .any(|c| c.id.member == "g");
    assert!(
        !has_g_cell,
        "expected no ValueCellDecl for 'g', but one was found (param should lower as realization)"
    );

    // (c) Exactly 1 RealizationDecl for the single geometry param.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 RealizationDecl for `param g : Solid = cylinder(...)`, got {}",
        template.realizations.len()
    );
}

// ─── coverage: Solid param as boolean-op operand ──────────────────────────────

/// Exercises the `geometry_lets` map-building block (task 1878 / `entity.rs:1240-1246`)
/// and the Ident-lookup block in `compile_geometry_call` (`geometry.rs:65-87`).
///
/// When a `Solid`-typed param (`g`) is used as an operand in a downstream boolean
/// op (`difference(g, other)`), the third-pass `geometry_lets` map must have
/// recorded `g`'s initializer expression so that `compile_geometry_call` can
/// resolve the `Ident("g")` and inline the underlying cylinder ops.  The result
/// is a single realization for `out` that contains the cylinder sub-op, the
/// sphere sub-op, and the difference op itself — at least 3 ops total.
///
/// Regression guard: if the `geometry_lets` map plumbing regresses (e.g. Solid
/// params are no longer inserted at `entity.rs:1240-1246`), `out` would not
/// resolve `g` and would emit a degenerate single-op or error realization.
#[test]
fn solid_param_referenced_by_downstream_boolean_op() {
    let source = r#"structure def W1 {
    param g : Solid = cylinder(10mm, 20mm)
    let other = sphere(15mm)
    let out = difference(g, other)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W1")
        .expect("W1 template not found");

    // (a) No scalar ValueCellDecls — g, other, and out are all geometry members.
    assert!(
        template.value_cells.is_empty(),
        "expected no ValueCellDecls (all three members are geometry); got: {:#?}",
        template.value_cells
    );

    // (b) Exactly 3 realizations: g, other, out.
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 RealizationDecls (g, other, out), got {}",
        template.realizations.len()
    );

    // (c) The `out` realization (index 2) must have >= 3 ops, proving that
    //     difference(g, other) inlined the resolved primitives via the
    //     geometry_lets map rather than emitting a degenerate single-op realization.
    let out_realization = &template.realizations[2];
    assert!(
        out_realization.operations.len() >= 3,
        "expected `out` realization to have >= 3 ops (cylinder + sphere + difference); \
         got {} — likely geometry_lets map plumbing regressed",
        out_realization.operations.len()
    );
}
