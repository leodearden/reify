//! End-to-end eval tests for the free-function `generate(n, |i| expr)` combinator
//! (task 3994 / structural-query ζ, PRD §5.9 / §2.3).
//!
//! `generate(n, |i| expr)` applies the lambda to indices `0..n-1` in order and
//! collects the results into a `List`.  Model: parse → `reify_compiler::compile`
//! → `Engine::eval` → assert `result.values`, mirroring `structural_query_eval.rs`.

use reify_core::{DiagnosticCode, ModulePath, Severity, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

/// Parse + compile + eval `source`, asserting no parse/compile Error diagnostics,
/// and return the evaluated result.
fn eval_source(source: &str) -> EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ─── step-7: eval core ───

/// `generate(4, |i| i)` evaluates to `[Int(0), Int(1), Int(2), Int(3)]` — the
/// lambda is applied to indices 0..3 in order.
///
/// RED today: there is no free-fn `generate` arm in eval_expr's FunctionCall
/// dispatch, so it falls through to `reify_stdlib::eval_builtin` (which has no
/// `generate` builtin) → `Value::Undef`. The eval dispatch (step-8) makes this GREEN.
#[test]
fn generate_positive_count_yields_index_list() {
    let result = eval_source(
        r#"
        structure S {
            let a = generate(4, |i| i)
        }
    "#,
    );
    let a = result.values.get(&ValueCellId::new("S", "a"));
    assert_eq!(
        a,
        Some(&Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ])),
        "generate(4, |i| i) should be [0,1,2,3]; got: {:?}",
        a,
    );
}

/// `generate(0, |i| i)` evaluates to the empty list `[]`.
#[test]
fn generate_zero_count_yields_empty_list() {
    let result = eval_source(
        r#"
        structure S {
            let b = generate(0, |i| i)
        }
    "#,
    );
    let b = result.values.get(&ValueCellId::new("S", "b"));
    assert_eq!(
        b,
        Some(&Value::List(vec![])),
        "generate(0, |i| i) should be the empty list; got: {:?}",
        b,
    );
}

/// `generate(3, |i| i * 1mm)` evaluates to a 3-element list of `Length`s
/// `[0mm, 1mm, 2mm]` (in SI metres: 0.0, 0.001, 0.002) — proving a non-Int body
/// type flows through and length is preserved.
#[test]
fn generate_length_body_yields_list_of_lengths() {
    let result = eval_source(
        r#"
        structure S {
            let c = generate(3, |i| i * 1mm)
        }
    "#,
    );
    match result.values.get(&ValueCellId::new("S", "c")) {
        Some(Value::List(items)) => {
            assert_eq!(items.len(), 3, "expected 3 elements; got: {:?}", items);
            for (idx, item) in items.iter().enumerate() {
                match item {
                    Value::Scalar { si_value, .. } => {
                        let expected = idx as f64 * 0.001; // idx mm in SI metres
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "element {} should be {} m (= {}mm); got si_value {}",
                            idx,
                            expected,
                            idx,
                            si_value,
                        );
                    }
                    other => panic!("element {} should be a Length scalar; got {:?}", idx, other),
                }
            }
        }
        other => panic!("S.c should be a List of 3 lengths; got: {:?}", other),
    }
}

// ─── step-9: negative-count named diagnostic ───

/// `generate(-1, |i| i)` leaves the cell `Undef` AND pushes a `Severity::Error`
/// eval diagnostic carrying `DiagnosticCode::GenerateNegativeCount` — a negative
/// count is a runtime contract failure (PRD §2.3).
///
/// The negative literal `-1` types as `Int` (UnOp::Neg over Int), so it PASSES
/// the compile-time `ExpectedArg::Int` count check (step-6) and reaches eval.
///
/// RED today: `DiagnosticCode::GenerateNegativeCount` does not exist (minted in
/// step-10) so this file does not compile; and nothing is emitted for `n < 0`
/// (eval_generate_dispatch currently yields the empty list for a negative range).
/// The n<0 branch (step-10) makes this GREEN.
#[test]
fn generate_negative_count_emits_named_diagnostic() {
    let result = eval_source(
        r#"
        structure S {
            let d = generate(-1, |i| i)
        }
    "#,
    );
    let d = result.values.get(&ValueCellId::new("S", "d"));
    assert_eq!(
        d,
        Some(&Value::Undef),
        "generate(-1, |i| i) should leave the cell Undef; got: {:?}",
        d,
    );
    let has_named = result.diagnostics.iter().any(|diag| {
        diag.severity == Severity::Error
            && diag.code == Some(DiagnosticCode::GenerateNegativeCount)
    });
    assert!(
        has_named,
        "expected a Severity::Error GenerateNegativeCount diagnostic; got: {:?}",
        result.diagnostics,
    );
}

// ─── step-11: example golden (structured) ───

/// Extract the `(x, y, z)` SI-metre components of a `point3` `Value::Point`.
fn point3_xyz(v: &Value) -> (f64, f64, f64) {
    match v {
        Value::Point(comps) if comps.len() == 3 => {
            let f = |c: &Value| c.as_f64().expect("point component should be numeric");
            (f(&comps[0]), f(&comps[1]), f(&comps[2]))
        }
        other => panic!("expected a 3-component point3; got: {:?}", other),
    }
}

/// The `examples/generate_bolt_circle.ri` golden: `generate(bolt_count = 4, …)`
/// places 4 point3s at exact quarter-turns on a 50 mm circle, plus `empty =
/// generate(0, |i| i)`.
///
/// `n == Int(4)` and `empty == []` are the PRIMARY (FP-free) signals; the
/// coordinates are checked within a 1e-6 mm tolerance (basis: f64 trig error at
/// exact k·π/2 ≤ ~1e-15 absolute → ~5e-19 m at r = 50mm, ~9 orders below the
/// tolerance — NOT exact equality, which the ~6e-17 cos(π/2) residue would break).
///
/// RED today: `examples/generate_bolt_circle.ri` does not exist (step-12 creates it).
#[test]
fn generate_bolt_circle_example_golden() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/generate_bolt_circle.ri"
    );
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("example {} should exist (step-12): {}", path, e));
    let result = eval_source(&source);

    // (a) PRIMARY: count(positions) == Int(4), exact (FP-free).
    let n_id = ValueCellId::new("BoltCircle", "n");
    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::Int(4)),
        "BoltCircle.n should be Int(4); got: {:?}",
        result.values.get(&n_id),
    );

    // (b) generate(0, |i| i) → [] (exact).
    let empty_id = ValueCellId::new("BoltCircle", "empty");
    assert_eq!(
        result.values.get(&empty_id),
        Some(&Value::List(vec![])),
        "BoltCircle.empty should be the empty list; got: {:?}",
        result.values.get(&empty_id),
    );

    // (c) positions: 4 point3s at golden quarter-turns (within 1e-9 m = 1e-6 mm).
    const TOL_M: f64 = 1e-9; // 1e-6 mm in SI metres
    let r = 0.05; // 50 mm in SI metres
    let golden = [
        (r, 0.0, 0.0),
        (0.0, r, 0.0),
        (-r, 0.0, 0.0),
        (0.0, -r, 0.0),
    ];
    match result.values.get(&ValueCellId::new("BoltCircle", "positions")) {
        Some(Value::List(items)) => {
            assert_eq!(items.len(), 4, "expected 4 positions; got: {:?}", items);
            for (idx, (item, (gx, gy, gz))) in items.iter().zip(golden.iter()).enumerate() {
                let (x, y, z) = point3_xyz(item);
                assert!(
                    (x - gx).abs() < TOL_M && (y - gy).abs() < TOL_M && (z - gz).abs() < TOL_M,
                    "position {} should be ({}, {}, {}) m; got ({}, {}, {})",
                    idx,
                    gx,
                    gy,
                    gz,
                    x,
                    y,
                    z,
                );
            }
        }
        other => panic!("BoltCircle.positions should be a List of 4 point3s; got: {:?}", other),
    }
}

// ─── task #5385: generate() over a GEOMETRY body ───

/// Assert `cell` is a `Value::List` of exactly `len` geometry handles, none
/// `Undef`, with pairwise-distinct `realization_ref`s, and return them.
fn assert_geometry_handle_list(
    result: &EvalResult,
    entity: &str,
    cell: &str,
    len: usize,
) -> Vec<reify_core::identity::RealizationNodeId> {
    let value = result
        .values
        .get(&ValueCellId::new(entity, cell))
        .unwrap_or_else(|| panic!("no value cell `{entity}.{cell}`"));
    let Value::List(items) = value else {
        panic!("`{entity}.{cell}` should be a List; got: {value:?}");
    };
    assert_eq!(
        items.len(),
        len,
        "`{entity}.{cell}` should have {len} elements; got: {items:?}",
    );
    let mut refs = Vec::new();
    for (k, item) in items.iter().enumerate() {
        assert!(
            !item.is_undef(),
            "`{entity}.{cell}[{k}]` is undef — the silent-undef repro is back: {items:?}",
        );
        match item {
            Value::GeometryHandle {
                realization_ref, ..
            } => refs.push(realization_ref.clone()),
            other => panic!("`{entity}.{cell}[{k}]` should be a GeometryHandle; got: {other:?}"),
        }
    }
    let distinct: std::collections::HashSet<_> = refs.iter().collect();
    assert_eq!(
        distinct.len(),
        len,
        "the elements must be SEPARATE realizations, not clones of one: {refs:?}",
    );
    refs
}

/// The headline acceptance: `generate(n, |i| <geometry>)` yields an evaluable
/// `List<Geometry>` — n distinct geometry handles, no undefs.
#[test]
fn generate_over_geometry_yields_evaluable_geometry_list() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
        }
    "#,
    );
    assert_geometry_handle_list(&result, "S", "holes", 3);
}

/// The list-literal form yields the same evaluable `List<Geometry>`.
#[test]
fn geometry_list_literal_yields_evaluable_geometry_list() {
    let result = eval_source(
        r#"
        structure S {
            let parts = [cylinder(5mm, 20mm), box(1mm, 1mm, 1mm)]
        }
    "#,
    );
    assert_geometry_handle_list(&result, "S", "parts", 2);
}

/// The probe-captured repro itself.
///
/// Probed 2026-07-24: `P.holes = [undef, undef, undef, undef]`, no diagnostic,
/// `reify check` green. That is the user-observable defect this task closes.
///
/// Source INLINED rather than read from
/// `docs/prds/v0_6/fixtures/silent_undef_generate_geometry.ri` (review
/// esc-5385-4). Two reasons, both about build coupling:
///
/// 1. `docs/prds/**/fixtures/` is the DEPRECATED fixture tier being emptied by
///    task #6431 (`tests/prd-gate/README.md` → "Where fixtures live"). A
///    `read_to_string` of a path on that tier hard-panics the moment the move
///    lands.
/// 2. The escalation that makes a Rust-read `.ri` fixture count as source
///    (`_RUST_COUPLED_RI_FIXTURES`, `scripts/verify.sh`) covers only
///    `tests/prd-gate/fixtures/`. A compiled test reading from `docs/` is
///    therefore coupled to a file the docs fast-path scopes to no-heavy-checks
///    — a docs-only edit could break this test with the gate none the wiser.
///
/// The `.ri` file remains the PRD's documenting artifact; this const is the
/// pinned signal. Keeping the two in step is manual — the alternative (move the
/// fixture to `tests/prd-gate/fixtures/` and register its basename in
/// `_RUST_COUPLED_RI_FIXTURES`) spans `docs/`, `tests/prd-gate/` and the verify
/// pipeline, so it is filed rather than done here.
const PRD_FIXTURE_SILENT_UNDEF_GENERATE_GEOMETRY: &str = r#"module silent_undef_generate_geometry

structure def P {
    let holes = generate(4, |i| cylinder(5mm, 20mm))
}"#;

/// ELEMENT ORDER: item `k` of the assembled list must be the element the
/// compiler unrolled at `list_binding.index == k` — not merely *some*
/// permutation of the three realizations (review esc-5385-4).
///
/// `assert_geometry_handle_list` checks length, non-undef and pairwise
/// distinctness, all of which a reversed or re-keyed regroup would still
/// satisfy. This test resolves the expectation from the COMPILED module rather
/// than assuming realization ids happen to ascend with index, so it also
/// discriminates a regroup that keyed on realization id instead of
/// `GeometryListBinding::index`.
///
/// Note `upstream_values_hash` is NOT a usable per-element discriminator here:
/// all three elements read the same upstream cells, so all three hashes are
/// equal even though the elements are distinct realizations. `realization_ref`
/// is the only symbolic discriminator, which is why the oracle is built from
/// `list_binding`.
#[test]
fn geometry_list_element_k_is_the_realization_unrolled_at_index_k() {
    let source = r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
        }
    "#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {errors:?}");

    // Oracle: the realization the COMPILER assigned to each index of `holes`.
    let mut expected: Vec<Option<reify_core::identity::RealizationNodeId>> = vec![None; 3];
    for template in &compiled.templates {
        for realization in &template.realizations {
            if let Some(binding) = &realization.list_binding
                && binding.list_name == "holes"
            {
                assert_eq!(binding.len, 3, "fixture drift: holes should have len 3");
                expected[binding.index] = Some(realization.id.clone());
            }
        }
    }
    let expected: Vec<_> = expected
        .into_iter()
        .enumerate()
        .map(|(k, r)| r.unwrap_or_else(|| panic!("compiler emitted no element at index {k}")))
        .collect();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    let actual = assert_geometry_handle_list(&result, "S", "holes", 3);

    assert_eq!(
        actual, expected,
        "list element k must be backed by the realization the compiler unrolled \
         at list_binding.index == k, in that order"
    );
}

#[test]
fn prd_fixture_silent_undef_generate_geometry_no_longer_undefs() {
    let result = eval_source(PRD_FIXTURE_SILENT_UNDEF_GENERATE_GEOMETRY);
    assert_geometry_handle_list(&result, "P", "holes", 4);
}

/// The list's LENGTH must be determinate too. Shipping resolved elements while
/// `.count` stayed `Undef` would trade one silent undef for another — a fresh
/// instance of exactly the failure class this task exists to kill.
///
/// RED today (and still RED after list assembly alone): `holes.count` is
/// compiled as a plain member access and evaluated in the main value-cell
/// pass, which runs BEFORE geometry-handle hydration — so it reads the
/// pre-hydration list and `count`'s `any(is_undef)` guard collapses it to
/// `Undef`.
#[test]
fn geometry_list_count_is_determinate() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let n = holes.count
        }
    "#,
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S", "n")),
        Some(&Value::Int(3)),
        "`holes.count` must be Int(3), not Undef; got: {:?}",
        result.values.get(&ValueCellId::new("S", "n")),
    );
}

/// Same for the list-literal form.
#[test]
fn geometry_list_literal_count_is_determinate() {
    let result = eval_source(
        r#"
        structure S {
            let parts = [cylinder(5mm, 20mm), box(1mm, 1mm, 1mm)]
            let n = parts.count
        }
    "#,
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S", "n")),
        Some(&Value::Int(2)),
        "`parts.count` must be Int(2); got: {:?}",
        result.values.get(&ValueCellId::new("S", "n")),
    );
}

/// An EMPTY geometry list has a determinate count of zero — the empty case
/// must not fall back into undef either.
#[test]
fn empty_geometry_list_count_is_zero() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(0, |i| cylinder(5mm, 20mm))
            let n = holes.count
        }
    "#,
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S", "n")),
        Some(&Value::Int(0)),
        "`generate(0, …).count` must be Int(0); got: {:?}",
        result.values.get(&ValueCellId::new("S", "n")),
    );
}

/// Assert `cell` is a non-`Undef` `Value::GeometryHandle`, returning its ref.
fn assert_geometry_handle(
    result: &EvalResult,
    entity: &str,
    cell: &str,
) -> reify_core::identity::RealizationNodeId {
    let value = result
        .values
        .get(&ValueCellId::new(entity, cell))
        .unwrap_or_else(|| panic!("no value cell `{entity}.{cell}`"));
    match value {
        Value::GeometryHandle {
            realization_ref, ..
        } => realization_ref.clone(),
        other => panic!("`{entity}.{cell}` should be a GeometryHandle; got: {other:?}"),
    }
}

/// The acceptance criterion end to end: `union_all` over a `List<Geometry>`
/// evaluates to a real geometry handle, distinct from every element's.
#[test]
fn union_all_over_a_geometry_list_evaluates_to_a_handle() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let combined = union_all(holes)
        }
    "#,
    );
    let element_refs = assert_geometry_handle_list(&result, "S", "holes", 3);
    let combined = assert_geometry_handle(&result, "S", "combined");
    assert!(
        !element_refs.contains(&combined),
        "`combined` must be its OWN realization, not an alias of an element: \
         {combined:?} in {element_refs:?}",
    );
}

/// The same for a direct list literal, and for `intersection_all`.
#[test]
fn boolean_all_over_a_direct_list_literal_evaluates_to_a_handle() {
    let result = eval_source(
        r#"
        structure S {
            let u = union_all([cylinder(5mm, 20mm), cylinder(6mm, 20mm)])
            let x = intersection_all([cylinder(5mm, 20mm), cylinder(6mm, 20mm)])
        }
    "#,
    );
    let u = assert_geometry_handle(&result, "S", "u");
    let x = assert_geometry_handle(&result, "S", "x");
    assert_ne!(u, x, "the two folds are distinct realizations");
}

/// The full motivating idiom from the task's REPRO: the index is substituted
/// through ARITHMETIC inside the geometry constructor's arguments, not just
/// into a constant body. Four distinct handles, no undef anywhere.
#[test]
fn generate_with_index_arithmetic_in_geometry_args_evaluates() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(4, |i| translate(cylinder(5mm, 20mm), 50mm * cos(i * 90deg), 50mm * sin(i * 90deg), 0mm))
        }
    "#,
    );
    assert_geometry_handle_list(&result, "S", "holes", 4);
}

// ─── task #5385 (review esc-5385-3): shadowing must defeat the .count fold ───

/// A LAMBDA PARAM that shadows a geometry-list let must NOT inherit that let's
/// statically-folded length.
///
/// `geometry_list_elements` is populated once in entity.rs pass 1 and then inherited
/// verbatim by the cloned lambda scope, which registers its binder in `names`
/// only. Keying the `.count` fold on the raw identifier text therefore folded the
/// INNER `holes` to the OUTER list's length.
///
/// RED before the shadow guard: `ns` evaluated to `List([Int(3), Int(3)])` — the
/// outer list's length, silently, with zero diagnostics, regardless of what the
/// lambda actually received. That is a silently-wrong compile-time CONSTANT,
/// exactly the class this task exists to eliminate.
///
/// Asserted as a CONCRETE SHAPE, not merely as "no `Int(3)` anywhere" (review
/// esc-5385-7). A bare negative is satisfied by the silent-undef outcome this
/// task exists to eliminate, and by `List([])` — so it could not tell "the shadow
/// guard works" from "the lambda body produced nothing at all". Pinning
/// `[Undef, Undef]` exactly makes BOTH a regression to the folded `Int(3)` and a
/// regression to a degenerate/empty list fail.
///
/// `Undef` is today's contract, not an aspiration: once the fold is correctly
/// suppressed the body is a genuine `MethodCall` on an `Int`-typed binder, and
/// `.count` on a non-collection evaluates to `Undef`. Making THAT loud is the
/// eval-side provenance question, task #5402 — so if #5402 turns these into a
/// diagnostic, this expectation is the thing it must update, deliberately.
#[test]
fn lambda_param_shadowing_a_geometry_list_let_does_not_inherit_its_count() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let ns = generate(2, |holes| holes.count)
        }
    "#,
    );
    let ns = result.values.get(&ValueCellId::new("S", "ns"));
    let Some(Value::List(elems)) = ns else {
        panic!("`ns` must evaluate to a List; got {ns:?}");
    };
    assert_eq!(
        elems.len(),
        2,
        "the lambda still runs once per index — a shorter list means the body \
         degenerated rather than being un-folded; got {ns:?}",
    );
    assert!(
        !elems.iter().any(|e| *e == Value::Int(3)),
        "the lambda's `holes.count` must NOT fold to the OUTER geometry list's \
         length 3 — the param shadows it; got {ns:?}",
    );
    assert_eq!(
        elems,
        &vec![Value::Undef, Value::Undef],
        "…and each element must be exactly `Undef` — `.count` on the Int-typed \
         binder the lambda really received (see the doc comment: #5402 owns \
         making this loud); got {ns:?}",
    );
}

/// The same for a QUANTIFIER VARIABLE, which `expr.rs` binds through a second,
/// independent `scope.clone()` that likewise leaves `geometry_list_elements` intact.
///
/// RED before the shadow guard: the predicate's `holes.count` folded to
/// `Literal(Int(3))`, so `holes.count == 2` was decided at COMPILE time and `ok`
/// evaluated to `Bool(false)` — a wrong answer, silently, with no diagnostic and
/// without the quantifier variable ever being consulted.
#[test]
fn quantifier_variable_shadowing_a_geometry_list_let_does_not_inherit_its_count() {
    let parsed = reify_syntax::parse(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let xs = [1, 2]
            let ok = forall holes in xs: holes.count == 2
        }
    "#,
        ModulePath::single("test"),
    );
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);

    // Assert against the COMPILED predicate rather than the evaluated Bool: the
    // fold is a compile-time rewrite, so the tree is where it is unambiguously
    // present or absent. (`Bool(false)` vs `Undef` would also discriminate today,
    // but only incidentally.)
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    let ok_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "ok")
        .expect("value cell 'ok'");
    // Matched STRUCTURALLY rather than against a `Debug` rendering (review
    // esc-5385-6): a rendering probe gates a negative assertion on the exact
    // spelling of derived `Debug` output, so a field rename would make it pass
    // vacuously. `forall holes in xs: holes.count == 2` compiles to
    // `Quantifier{ predicate: BinOp{ left: <holes.count>, right: Literal(2) } }`,
    // and the fold — had it fired — would have replaced `left` in place.
    let ok_expr = ok_cell.default_expr.as_ref().expect("default_expr");
    let reify_ir::CompiledExprKind::Quantifier { predicate, .. } = &ok_expr.kind else {
        panic!("expected `ok` to compile to a Quantifier; got {ok_expr:#?}");
    };
    let reify_ir::CompiledExprKind::BinOp { left, .. } = &predicate.kind else {
        panic!("expected the predicate to be a comparison; got {predicate:#?}");
    };
    let reify_ir::CompiledExprKind::MethodCall { object, method, .. } = &left.kind else {
        panic!(
            "the quantifier variable's `holes.count` must survive as a MethodCall \
             on the bound variable, not be folded away; got {left:#?}"
        );
    };
    assert_eq!(method, "count", "the surviving MethodCall must be `.count`");
    let reify_ir::CompiledExprKind::ValueRef(receiver) = &object.kind else {
        panic!("`.count`'s receiver must be a ValueRef; got {object:#?}");
    };
    assert_ne!(
        *receiver,
        ValueCellId::new("S", "holes"),
        "the predicate must not reference the OUTER geometry-list cell — that is \
         its length leaking past the shadowing binder",
    );
}

/// S2 (review esc-5385-3): pin the VALUE of an empty geometry list, not just
/// its folded `.count`.
///
/// `GeometryListCellAccumulator::into_entries` used to claim it emitted the
/// empty list itself. It cannot: `generate(0, …)` emits zero
/// `RealizationDecl`s, so `declare()` is never called and the key never enters
/// `declared`. The cell therefore keeps whatever the ordinary value-cell pass
/// computed — which IS `List([])`, but by a different route than the doc
/// described. This test pins the end state so the two routes cannot silently
/// disagree if either changes.
#[test]
fn empty_geometry_list_evaluates_to_the_empty_list() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(0, |i| cylinder(5mm, 20mm))
        }
    "#,
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("S", "holes")),
        Some(&Value::List(vec![])),
        "`generate(0, …)` must evaluate to the empty list, not Undef and not \
         absent; got: {:?}",
        result.values.get(&ValueCellId::new("S", "holes")),
    );
}

/// S6 (review esc-5385-3): the exactly-one-element fold, end to end.
///
/// This is the case that justifies bypassing `union_all`'s `>= 2` arity gate —
/// "a list that expanded to exactly ONE element folds to that element with zero
/// Boolean ops". Nothing exercised it at either level before.
#[test]
fn union_all_over_a_one_element_geometry_list_evaluates_to_a_handle() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(1, |i| cylinder(5mm, 20mm))
            let combined = union_all(holes)
        }
    "#,
    );
    let element_refs = assert_geometry_handle_list(&result, "S", "holes", 1);
    let combined = assert_geometry_handle(&result, "S", "combined");
    assert!(
        !element_refs.contains(&combined),
        "`combined` must be its OWN realization even for a one-element fold: \
         {combined:?} in {element_refs:?}",
    );
}

/// BOUNDARY (review esc-5385-3): a non-`.count` read of a geometry-list let is
/// resolved by the FULL pipeline but not by `Engine::eval` on its own.
///
/// Only `.count` (constant-folded in expr.rs) and the `union_all` /
/// `intersection_all` folds (expanded in geometry_boolean.rs) read a
/// geometry-list let at COMPILE time. Every other read — indexing here —
/// compiles to an ordinary value-cell expression, and those evaluate BEFORE
/// `post_process_geometry_handle_cells` regroups the sibling handles into the
/// list cell. `Engine::eval` does not then recompute dependents, so `first`
/// keeps the `Undef` it read from the pre-hydration placeholder even though
/// `holes` itself ends up correct.
///
/// This is NOT a user-visible silent-undef: the CLI's build/tessellate pass
/// does recompute, and `reify eval` on this exact source prints
/// `S.first = <Geometry: S#realization[0]>` (verified against the debug
/// binary). The gap is confined to the single-pass entry point.
///
/// Pinned rather than fixed: closing it means recomputing value cells after
/// hydration, an evaluation-ordering change well outside a task scoped to
/// making `generate` yield a `List<Geometry>` and `union_all` accept one. The
/// assertion is deliberately paired — `holes` correct AND `first` undef — so
/// the test states where the seam actually is instead of reading as "geometry
/// lists are not indexable".
#[test]
fn indexing_a_geometry_list_reads_the_pre_hydration_placeholder() {
    let result = eval_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let first = holes[0]
        }
    "#,
    );
    // The list cell itself IS hydrated — this half must never regress.
    assert_geometry_handle_list(&result, "S", "holes", 3);

    let first = result.values.get(&ValueCellId::new("S", "first"));
    assert_eq!(
        first,
        Some(&Value::Undef),
        "under `Engine::eval` alone an indexed read is the pre-hydration \
         placeholder; if this now resolves, the recompute landed and this test \
         should assert the handle instead: {first:?}",
    );
}

