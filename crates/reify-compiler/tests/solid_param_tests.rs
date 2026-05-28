//! Tests for `param x : Solid = <geometry_call>` compilation.
//!
//! After GHR-γ: a `Solid`-typed param with a geometry-call default is lowered
//! BOTH as a `ValueCellDecl{cell_type: Type::Geometry}` AND as a `RealizationDecl`.

use reify_compiler::{BooleanOp, CompiledGeometryOp, PrimitiveKind, ValueCellKind};
use reify_core::{RealizationNodeId, Severity, Type};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_solid_param"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let compiled = parse_and_compile(source);
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

// ─── GHR-γ: Solid-typed param MUST emit a ValueCellDecl ──────────────────────

/// After GHR-γ (bypass retired), `param g : Solid = cylinder(10mm, 20mm)`
/// must produce a `ValueCellDecl{cell_type: Type::Geometry, kind: Param}`
/// AND a `RealizationDecl` (both paths are now active in parallel).
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

    // After GHR-γ: exactly one ValueCellDecl for 'g' with Type::Geometry.
    let g_cells: Vec<_> = template.value_cells.iter().filter(|c| c.id.member == "g").collect();
    assert_eq!(
        g_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl for 'g' (GHR-γ: bypass retired); got: {:#?}",
        g_cells
    );
    assert_eq!(
        g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for 'g'"
    );
    assert_eq!(
        g_cells[0].kind,
        ValueCellKind::Param,
        "expected kind=ValueCellKind::Param for 'g'"
    );
}

// ─── GHR-γ: Guarded Solid-typed param emits both ValueCellDecl + RealizationDecl

/// After GHR-γ (bypass retired), a `Solid`-typed param inside a block-level
/// `where` guard MUST appear as a `ValueCellDecl` in the guarded group's
/// `members` (with `cell_type == Type::Geometry`) AND produce a `RealizationDecl`
/// in the template's top-level realizations list.
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

    // (a) `g` must NOT appear as a top-level ValueCellDecl — it is guarded.
    assert!(
        !template.value_cells.iter().any(|c| c.id.member == "g"),
        "top-level ValueCellDecl for 'g' must not exist (it belongs in guarded_groups)"
    );
    // (b) After GHR-γ: `g` MUST appear in exactly one guarded group's members
    //     with cell_type == Type::Geometry and kind == Param.
    let guarded_g_cells: Vec<_> = template
        .guarded_groups
        .iter()
        .flat_map(|grp| grp.members.iter())
        .filter(|c| c.id.member == "g")
        .collect();
    assert_eq!(
        guarded_g_cells.len(),
        1,
        "expected exactly 1 guarded ValueCellDecl for 'g' (GHR-γ: bypass retired); got: {:#?}",
        guarded_g_cells
    );
    assert_eq!(
        guarded_g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for guarded 'g'"
    );
    // (c) At least one RealizationDecl must still be emitted for the guarded geometry param.
    assert!(
        !template.realizations.is_empty(),
        "expected at least one RealizationDecl for guarded `param g : Solid = cylinder(...)`, \
         got none"
    );
}

// ─── GHR-γ: Solid-typed param lowers to BOTH a ValueCellDecl and a RealizationDecl

/// After GHR-γ (bypass retired), `param g : Solid = cylinder(10mm, 20mm)` must:
/// (a) compile without errors,
/// (b) produce exactly one `ValueCellDecl` named `g` with `cell_type == Type::Geometry`,
/// (c) produce exactly 1 RealizationDecl (realization path unchanged).
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

    // (b) After GHR-γ: exactly one ValueCellDecl named "g" with Type::Geometry.
    let g_cells: Vec<_> = template.value_cells.iter().filter(|c| c.id.member == "g").collect();
    assert_eq!(
        g_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl for 'g' (GHR-γ: bypass retired); got: {:#?}",
        g_cells
    );
    assert_eq!(
        g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for 'g'"
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

    // (a) After GHR-γ: `g` (Solid param) has exactly 1 ValueCellDecl with Type::Geometry.
    //     `other` and `out` are geometry lets — no ValueCellDecl for them.
    let g_cells: Vec<_> = template.value_cells.iter().filter(|c| c.id.member == "g").collect();
    assert_eq!(
        g_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl for 'g' (GHR-γ: bypass retired); got: {:#?}",
        g_cells
    );
    assert_eq!(
        g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for 'g'"
    );
    assert_eq!(
        template.value_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl total (only 'g' is a Solid param; other/out are lets); \
         got: {:#?}",
        template.value_cells
    );

    // (b) Exactly 3 realizations: g, other, out.
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 RealizationDecls (g, other, out), got {}",
        template.realizations.len()
    );

    // (c) The `out` realization must have exactly 3 ops (cylinder + sphere + Difference),
    //     proving that difference(g, other) inlined the resolved primitives via the
    //     geometry_lets map rather than emitting a degenerate single-op realization.
    //     Lowering is deterministic; == 3 catches silent regressions that duplicate ops.
    //     If future lowering legitimately adds helper ops (e.g., transforms or bounds),
    //     update this count intentionally — that should be a deliberate, reviewable change.
    let out_realization = &template.realizations[2]; // realizations emitted in source order: [0]=g, [1]=other, [2]=out

    // Ordering invariant: realizations[2] must carry source-order index 2 (the `out` node,
    // assigned by realization_index in entity.rs).  If emission order ever changes, this
    // assertion surfaces the root cause (an ordering shift) rather than producing a
    // misleading "expected 3 ops" failure.
    assert_eq!(
        out_realization.id,
        RealizationNodeId::new("W1", 2),
        "expected realizations[2] to be W1#realization[2] (source-order `out` node); \
         emission order may have changed — update the [0]=g,[1]=other,[2]=out comment \
         and this assertion accordingly"
    );
    assert_eq!(
        out_realization.operations.len(),
        3,
        "expected `out` realization to have exactly 3 ops (cylinder + sphere + Difference); \
         got {} — likely geometry_lets map plumbing regressed",
        out_realization.operations.len()
    );

    // Also verify op kinds to tighten the regression anchor: at least one
    // Primitive (Cylinder or Sphere) op and one Boolean(Difference) op must be
    // present.  This rules out a fallback that happens to emit 3 unrelated ops.
    let has_primitive = out_realization.operations.iter().any(|compiled_op| {
        matches!(
            compiled_op,
            CompiledGeometryOp::Primitive { kind, .. }
                if *kind == PrimitiveKind::Cylinder || *kind == PrimitiveKind::Sphere
        )
    });
    assert!(
        has_primitive,
        "expected at least one Primitive (Cylinder or Sphere) op in `out` realization; \
         got: {:#?} — geometry_lets map may not be inlining the Solid param operand",
        out_realization.operations
    );

    let has_difference = out_realization.operations.iter().any(|compiled_op| {
        matches!(
            compiled_op,
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                ..
            }
        )
    });
    assert!(
        has_difference,
        "expected at least one Boolean(Difference) op in `out` realization; \
         got: {:#?} — the difference call may not have been compiled correctly",
        out_realization.operations
    );
}

// ─── coverage: Solid param default as Ident alias ─────────────────────────────

/// Exercises the Ident branch of `is_geometry_let` at `geometry.rs:16` (task 1878).
///
/// When a `Solid`-typed param (`g`) has as its default an Ident (`a`) that names
/// a geometry let defined earlier in the same structure, the ordered first-pass
/// (`entity.rs:305-350`) must insert `a` into `known_geometry_lets` before `g`
/// is visited, allowing `is_geometry_let(Ident("a"), ..., known_geometry_lets)`
/// to return `true` via the Ident match arm.  Both `a` and `g` are then
/// recorded in the `geometry_lets` map (third pass) and lowered as realizations.
///
/// Regression guard: if the Ident branch of `is_geometry_let` regresses, `g`
/// will not be added to `known_geometry_lets` and will not be lowered as a
/// realization — either a ValueCellDecl will appear for `g` or the realization
/// count will drop to 1.
#[test]
fn solid_param_default_aliasing_geometry_let_is_realization() {
    let source = r#"structure def W2 {
    let a = cylinder(10mm, 20mm)
    param g : Solid = a
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W2")
        .expect("W2 template not found");

    // (a) After GHR-γ: `g` (Solid param with geometry-ident default) has exactly 1 ValueCellDecl.
    //     `a` is a geometry let — no ValueCellDecl for it.
    let g_cells: Vec<_> = template.value_cells.iter().filter(|c| c.id.member == "g").collect();
    assert_eq!(
        g_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl for 'g' (GHR-γ: bypass retired); got: {:#?}",
        g_cells
    );
    assert_eq!(
        g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for 'g'"
    );
    assert_eq!(
        template.value_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl total (only 'g' is a Solid param; 'a' is a geometry let); \
         got: {:#?}",
        template.value_cells
    );

    // (b) Exactly 2 realizations: one for `a`, one for `g`.
    assert_eq!(
        template.realizations.len(),
        2,
        "expected 2 RealizationDecls (a and g), got {}",
        template.realizations.len()
    );

    // (c) Both realizations carry at least one compiled geometry op.
    for (i, realization) in template.realizations.iter().enumerate() {
        assert!(
            !realization.operations.is_empty(),
            "realization at index {} has no operations — likely Ident-alias branch regressed",
            i
        );
    }

    // (d) At least one realization must contain a Cylinder primitive, confirming
    //     that the Ident branch of is_geometry_let resolved `a`'s cylinder ops
    //     into `g` rather than emitting an unrelated placeholder op.
    let has_cylinder = template.realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Cylinder,
                    ..
                }
            )
        })
    });
    assert!(
        has_cylinder,
        "expected at least one realization to contain a Cylinder primitive — \
         Ident-alias branch of is_geometry_let may not be inlining a's cylinder ops into g; \
         got: {:#?}",
        template.realizations
    );
}

// ─── step-1: Nested-guard regression ─────────────────────────────────────────

/// Two-level `where` guard nesting: `where a { where b { param g : Solid = cylinder(...) } }`.
///
/// `register_guarded_names` in guards.rs already recurses correctly, so `g` ends
/// up in `known_geometry_lets`. However, both the `geometry_lets` lookup-table
/// builder and the realization-emission loop in entity.rs iterate only ONE level
/// deep, so `g` is silently skipped and no `RealizationDecl` is produced.
///
/// Regression: prior to the fix in this change, assertion (d) failed because
/// entity.rs only recursed one level into `GuardedGroupDecl`, so the nested
/// `g` was registered in `known_geometry_lets` but never made it into
/// `geometry_lets` and never produced a `RealizationDecl`.
#[test]
fn nested_guarded_solid_param_compiles_as_realization() {
    let source = r#"structure def W {
    param a : Bool = true
    param b : Bool = true
    where a {
        where b {
            param g : Solid = cylinder(10mm, 20mm)
        }
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W")
        .expect("W template not found");

    // (a) No error diagnostics — already checked by compile_no_errors.

    // (b) `g` must NOT appear as a top-level ValueCellDecl — it is nested-guarded.
    assert!(
        !template.value_cells.iter().any(|c| c.id.member == "g"),
        "top-level ValueCellDecl for 'g' must not exist (it belongs in guarded_groups)"
    );

    // (c) After GHR-γ: `g` MUST appear in some guarded group's members as a
    //     ValueCellDecl with cell_type == Type::Geometry.
    let guarded_g_cells: Vec<_> = template
        .guarded_groups
        .iter()
        .flat_map(|grp| grp.members.iter())
        .filter(|c| c.id.member == "g")
        .collect();
    assert_eq!(
        guarded_g_cells.len(),
        1,
        "expected exactly 1 guarded ValueCellDecl for 'g' (GHR-γ: bypass retired); got: {:#?}",
        guarded_g_cells
    );
    assert_eq!(
        guarded_g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for nested-guarded 'g'"
    );

    // (d) Exactly one RealizationDecl must be emitted for the nested geometry
    // param. Using `== 1` (not `>= 1`) prevents a future double-emit regression
    // if the recursive walk mistakenly visits the same guard twice.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 RealizationDecl for nested `param g : Solid = cylinder(...)`, \
         got {} — nested GuardedGroup recursion in entity.rs is broken",
        template.realizations.len()
    );
}

// ─── step-3: Intermediate: nested guard emits exactly one realization ────────

/// Simpler variant using `where true { where true { ... } }` (no bool param
/// overhead). Asserts exactly-one `RealizationDecl` for the nested geometry
/// param so a regression either drops the realization (recursion broken) or
/// double-emits it (recursive walk visits both sides of the same guard).
///
/// Note: bare boolean-literal guard conditions (`where true`) are a supported
/// and stable syntactic form in the Reify compiler — they are handled by the
/// same guard-condition lowering path as param-reference guards and are not
/// subject to future tightening.
#[test]
fn nested_guarded_solid_param_emits_exactly_one_realization() {
    let source = r#"structure def X {
    where true {
        where true {
            param g : Solid = cylinder(1mm, 1mm)
        }
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "X")
        .expect("X template not found");

    // Exactly one RealizationDecl must be emitted — one geometry param, one
    // realization. Using `== 1` (not `>= 1`) prevents a future double-emit
    // if the recursive walk mistakenly visits both members and else_members
    // of the same guard at the same nesting level.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 RealizationDecl for nested guarded \
         `param g : Solid = cylinder(...)`"
    );
}

// ─── coverage: else_members recursion at depth ≥2 ────────────────────────────

/// Regression guard for the `else_members` branch of the recursive walkers in
/// `collect_geometry_exprs` and `emit_guarded_geometry_realizations`.
///
/// A Solid-typed param inside a nested `else { ... }` block (`where a { where b
/// { } else { param g : Solid = cylinder(...) } }`) exercises the
/// `g.else_members` recursive call at depth 2. A regression that dropped
/// `else_members` recursion would leave `g` unregistered in `geometry_lets` and
/// emit no `RealizationDecl`, causing assertion (b) to fail.
#[test]
fn nested_guarded_solid_param_in_else_branch_compiles_as_realization() {
    let source = r#"structure def W5 {
    param a : Bool = true
    param b : Bool = true
    where a {
        where b {
        } else {
            param g : Solid = cylinder(10mm, 20mm)
        }
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W5")
        .expect("W5 template not found");

    // (a) `g` must NOT appear as a top-level ValueCellDecl — it is nested-guarded.
    assert!(
        !template.value_cells.iter().any(|c| c.id.member == "g"),
        "top-level ValueCellDecl for 'g' must not exist (it belongs in guarded_groups)"
    );
    // After GHR-γ: `g` MUST appear in some guarded group's members or else_members
    //     as a ValueCellDecl with cell_type == Type::Geometry.
    let guarded_g_cells: Vec<_> = template
        .guarded_groups
        .iter()
        .flat_map(|grp| grp.members.iter().chain(grp.else_members.iter()))
        .filter(|c| c.id.member == "g")
        .collect();
    assert_eq!(
        guarded_g_cells.len(),
        1,
        "expected exactly 1 guarded ValueCellDecl for 'g' in members or else_members (GHR-γ); \
         got: {:#?}",
        guarded_g_cells
    );
    assert_eq!(
        guarded_g_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for else-branch nested-guarded 'g'"
    );

    // (b) Exactly one RealizationDecl must be emitted for `g` (from the else
    // branch). Using `== 1` guards against both recursion failure (0) and
    // double-emit regressions (≥2).
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 RealizationDecl for `param g : Solid = cylinder(...)` \
         declared inside a nested else_members branch, got {} — else_members recursion may be broken",
        template.realizations.len()
    );
}

// ─── coverage: Solid param with non-geometry default (pin-down) ───────────────

/// PIN-DOWN REGRESSION LOCK (task 1878).
///
/// Documents and locks the currently-observed *silent-accept* behavior when a
/// `Solid`-typed param is given a non-geometry default (`42`).
///
/// Currently-observed behavior (verified via probe harness before planning):
///   - The compiler emits **no Error-severity diagnostics** — the mismatch is
///     silently accepted.
///   - `is_geometry_let(42, ...)` returns `false`, so the param is NOT inserted
///     into `known_geometry_lets` and NOT added to the `geometry_lets` map.
///   - As a result **no RealizationDecl** is emitted for `g`.
///   - The param falls through to the `ValueCellDecl` path with
///     `cell_type = Type::Geometry` and `kind = ValueCellKind::Param`.
///
/// KNOWN QUIRKY: this is not necessarily correct behavior — a future change
/// (e.g. diagnosing `Solid = <non-geometry>` as an error) would be desirable.
/// Any such change MUST update this test intentionally so that the regression
/// guard remains accurate rather than becoming stale.
#[test]
fn solid_param_with_non_geometry_default_silently_accepts() {
    let source = r#"structure def W3 {
    param g : Solid = 42
}"#;
    // pin-down: parse+compile without asserting absence of diagnostics — the test
    // intentionally inspects whatever diagnostic behavior the compiler currently
    // exhibits so any future change becomes a deliberate, reviewable test update.
    let compiled = parse_and_compile(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W3")
        .expect("W3 template not found");

    // (a) Currently no Error-severity diagnostics are emitted — the compiler
    //     silently accepts the type mismatch.  If this changes (e.g. a Warning
    //     or Error is added), update this assertion intentionally.
    let error_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "pin-down: expected no Error-severity diagnostics for `param g : Solid = 42`, \
         got: {:#?}\n\
         If the compiler now diagnoses this mismatch, update this test to reflect the new behavior.",
        error_diags
    );

    // (b) No realization is emitted — `g` is not inserted into geometry_lets
    //     because is_geometry_let returns false for a literal integer.
    assert!(
        template.realizations.is_empty(),
        "pin-down: expected no RealizationDecls for `param g : Solid = 42`, got: {:#?}\n\
         If the compiler now lowers this as a realization, update this test.",
        template.realizations
    );

    // (c) Exactly one ValueCellDecl named `g` with cell_type=Geometry and kind=Param.
    let g_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "g")
        .expect(
            "pin-down: expected a ValueCellDecl named 'g' for `param g : Solid = 42`; \
             none found — update this test if the lowering strategy changed",
        );
    assert_eq!(
        g_cell.cell_type,
        Type::Geometry,
        "pin-down: expected cell_type=Type::Geometry for `param g : Solid = 42`, got {:?}",
        g_cell.cell_type
    );
    assert_eq!(
        g_cell.kind,
        ValueCellKind::Param,
        "pin-down: expected kind=ValueCellKind::Param for `param g : Solid = 42`, got {:?}",
        g_cell.kind
    );
}

// ─── GHR-γ step-1: ValueCellDecl MUST exist for Solid params ─────────────────
// These RED tests assert that after the bypass retirement, Solid-typed params
// with geometry-call defaults produce a `ValueCellDecl{cell_type: Type::Geometry,
// kind: ValueCellKind::Param}` in addition to the existing RealizationDecl.
// They FAIL until step-2 deletes the `is_solid_geometry_param → continue` bypass.

/// After GHR-γ, `param body : Solid = box(10mm, 20mm, 30mm)` must produce:
/// (a) exactly one `ValueCellDecl` with `id.member == "body"`, `cell_type ==
///     Type::Geometry`, `kind == ValueCellKind::Param`, `default_expr.is_some()`.
/// (b) exactly one `RealizationDecl` (parallel realization-op chain unchanged).
///
/// Currently FAILS because entity.rs:1045's `is_solid_geometry_param(…) → continue`
/// skip drops the ValueCellDecl.
#[test]
fn solid_param_creates_geometry_value_cell() {
    let source = r#"structure def Widget {
    param body : Solid = box(10mm, 20mm, 30mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template not found");

    // (a) Exactly one ValueCellDecl named "body" with Type::Geometry + Param kind.
    let body_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|c| c.id.member == "body")
        .collect();
    assert_eq!(
        body_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl for 'body'; got: {:#?}",
        body_cells
    );
    let cell = body_cells[0];
    assert_eq!(
        cell.cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for 'body', got {:?}",
        cell.cell_type
    );
    assert_eq!(
        cell.kind,
        ValueCellKind::Param,
        "expected kind=ValueCellKind::Param for 'body', got {:?}",
        cell.kind
    );
    assert!(
        cell.default_expr.is_some(),
        "expected default_expr.is_some() for 'body' (box call as default)"
    );

    // (b) Exactly one RealizationDecl — realization path is orthogonal and unchanged.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 RealizationDecl for `param body : Solid = box(...)`, got {}",
        template.realizations.len()
    );
}

/// Guarded-block variant: `where some_cond { param body : Solid = box(...) }` must
/// produce a `ValueCellDecl` in the guarded group's `members` AND a `RealizationDecl`
/// in the template's top-level realizations list.
///
/// Currently FAILS because guards.rs:381's `is_solid_geometry_param(…) → continue`
/// skips the ValueCellDecl in the guarded-members compilation pass.
#[test]
fn guarded_solid_param_creates_geometry_value_cell() {
    let source = r#"structure def W {
    param some_cond : Bool = true
    where some_cond {
        param body : Solid = box(10mm, 20mm, 30mm)
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W")
        .expect("W template not found");

    // (a) "body" must appear as a ValueCellDecl in exactly one guarded group's members.
    let guarded_body_cells: Vec<_> = template
        .guarded_groups
        .iter()
        .flat_map(|g| g.members.iter())
        .filter(|c| c.id.member == "body")
        .collect();
    assert_eq!(
        guarded_body_cells.len(),
        1,
        "expected exactly 1 guarded ValueCellDecl for 'body'; got: {:#?}",
        guarded_body_cells
    );
    let cell = guarded_body_cells[0];
    assert_eq!(
        cell.cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for guarded 'body', got {:?}",
        cell.cell_type
    );
    assert_eq!(
        cell.kind,
        ValueCellKind::Param,
        "expected kind=ValueCellKind::Param for guarded 'body', got {:?}",
        cell.kind
    );

    // (b) At least one RealizationDecl must still be emitted.
    assert!(
        !template.realizations.is_empty(),
        "expected at least one RealizationDecl for guarded `param body : Solid = box(...)`, got none"
    );
}
