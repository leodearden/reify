//! Field declaration compilation tests.
//!
//! Tests for compiling `field def` declarations into CompiledField entries.

use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};
use reify_core::{DiagnosticCode, FIELD_ENTITY_PREFIX, ValueCellId};

// ── Step 13: compile analytical field ──────────────────────────────────

#[test]
fn compile_field_analytical() {
    let module =
        compile_source("field def temp : Point3 -> Scalar { source = analytical { |p| 1.0m } }");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.fields.len(), 1, "expected 1 compiled field");

    let field = &module.fields[0];
    assert_eq!(field.name, "temp");
    assert!(!field.is_pub);

    // Domain and codomain types should be resolved
    // Point3 is not a built-in type, so it resolves to StructureRef
    assert_eq!(format!("{}", field.domain_type), "Point3");
    // Scalar resolves to Type::length() which displays as "Scalar[m]"
    assert_eq!(format!("{}", field.codomain_type), "Scalar[m]");

    // Source should be analytical with a compiled lambda expression
    match &field.source {
        reify_compiler::CompiledFieldSource::Analytical { expr } => {
            // The expression should be a lambda
            assert!(
                matches!(expr.kind, reify_ir::CompiledExprKind::Lambda { .. }),
                "expected Lambda expression in analytical source, got: {:?}",
                expr.kind
            );
        }
        other => panic!("expected Analytical source, got: {:?}", other),
    }
}

// ── Task 2341 step-5/8b: well-formed sampled field config compiles clean ────

#[test]
fn compile_field_sampled_with_well_formed_config_compiles_clean() {
    // Pins the v0.2 behavior of `compile_field`'s Sampled arm: when all five
    // required keys (`grid`, `bounds`, `spacing`, `interpolation`, `data`)
    // are present and each value is a clean-compiling expression, no
    // `FieldSampledV02` deferral diagnostic is emitted and the compiled
    // config Vec carries one `(String, CompiledExpr)` entry per key.
    //
    // Eval-time parsing of the values into a runtime `SampledField` is
    // pinned by separate tests in `crates/reify-eval/tests/field_eval_tests.rs`.
    //
    // `bbox`/`point3` are stdlib builtins, so this test uses
    // `compile_source_with_stdlib` to make those names resolve. (Steps 9/10
    // wire the eval-time parsing that consumes the BoundingBox/Point shapes.)
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }"#,
    );

    // Zero `FieldSampledV02` errors — the v0.1 deferral has been replaced.
    let v02_errs: Vec<_> = errors_only(&module)
        .into_iter()
        .filter(|d| d.code == Some(DiagnosticCode::FieldSampledV02))
        .collect();
    assert!(
        v02_errs.is_empty(),
        "expected zero FieldSampledV02 errors after v0.2 implementation, got: {:?}",
        v02_errs.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // No other compile errors should appear for this well-formed source.
    let all_errs = errors_only(&module);
    assert!(
        all_errs.is_empty(),
        "expected no errors for well-formed sampled field, got: {:?}",
        all_errs.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    assert_eq!(module.fields.len(), 1, "expected 1 compiled field");

    let field = &module.fields[0];
    assert_eq!(field.name, "f");

    // Source should be Sampled with five config entries — each compiled to a
    // CompiledExpr — so engine_eval can later evaluate them and parse the
    // results into a runtime `SampledField`.
    match &field.source {
        reify_compiler::CompiledFieldSource::Sampled { config } => {
            assert_eq!(
                config.len(),
                5,
                "expected 5 compiled config entries (grid, bounds, spacing, interpolation, data), got: {:?}",
                config.iter().map(|(k, _)| k).collect::<Vec<_>>()
            );
            let keys: Vec<&str> = config.iter().map(|(k, _)| k.as_str()).collect();
            for required in ["grid", "bounds", "spacing", "interpolation", "data"] {
                assert!(
                    keys.contains(&required),
                    "expected `{}` key in compiled config, got: {:?}",
                    required,
                    keys
                );
            }
        }
        other => panic!("expected Sampled source, got: {:?}", other),
    }
}

// ── Task 2341 step-7/8b: sampled field config validation negative paths ─────

#[test]
fn compile_field_sampled_rejects_missing_data_key() {
    // Pins the v0.2 behavior of `compile_field`'s Sampled arm: when one of the
    // five required keys is absent, exactly one error per missing key is
    // emitted. This source provides every required key except `data`, so we
    // expect exactly one error whose message mentions `data`.
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" } }"#,
    );

    // Only count errors that reference the missing `data` key — there should
    // be exactly one such diagnostic. We deliberately match on the message
    // substring rather than a dedicated DiagnosticCode because the missing-key
    // condition is a generic shape-validation error, not a user-facing
    // diagnostic-code variant.
    let data_errs: Vec<_> = errors_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("'data'") || d.message.contains("`data`"))
        .collect();
    assert_eq!(
        data_errs.len(),
        1,
        "expected exactly one error mentioning the missing `data` key, got {}: {:?}",
        data_errs.len(),
        data_errs.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(
        data_errs[0].message.contains("missing") || data_errs[0].message.contains("required"),
        "expected the error message to indicate `data` is missing/required, got: {}",
        data_errs[0].message
    );
    // No cascade: the missing-key error is the only error emitted (pins that
    // a future regression introducing an unrelated diagnostic would not slip
    // past the substring-filter above).
    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected exactly one total error, got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_field_sampled_rejects_missing_bounds_key() {
    // Pins step-8b's expansion of REQUIRED_KEYS to include `bounds`. Source
    // omits `bounds` but provides every other required key, so we expect
    // exactly one error whose message mentions `bounds`.
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }"#,
    );

    let bounds_errs: Vec<_> = errors_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("'bounds'") || d.message.contains("`bounds`"))
        .collect();
    assert_eq!(
        bounds_errs.len(),
        1,
        "expected exactly one error mentioning the missing `bounds` key, got {}: {:?}",
        bounds_errs.len(),
        bounds_errs.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(
        bounds_errs[0].message.contains("missing") || bounds_errs[0].message.contains("required"),
        "expected the error message to indicate `bounds` is missing/required, got: {}",
        bounds_errs[0].message
    );
    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected exactly one total error, got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_field_sampled_rejects_missing_spacing_key() {
    // Pins step-8b's expansion of REQUIRED_KEYS to include `spacing`. Source
    // omits `spacing` but provides every other required key, so we expect
    // exactly one error whose message mentions `spacing`.
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) interpolation = "Linear" data = [0.0, 1.0, 2.0] } }"#,
    );

    let spacing_errs: Vec<_> = errors_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("'spacing'") || d.message.contains("`spacing`"))
        .collect();
    assert_eq!(
        spacing_errs.len(),
        1,
        "expected exactly one error mentioning the missing `spacing` key, got {}: {:?}",
        spacing_errs.len(),
        spacing_errs.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(
        spacing_errs[0].message.contains("missing") || spacing_errs[0].message.contains("required"),
        "expected the error message to indicate `spacing` is missing/required, got: {}",
        spacing_errs[0].message
    );
    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected exactly one total error, got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_field_sampled_rejects_unknown_key() {
    // Pins the v0.2 behavior: keys outside the closed set
    // {grid, bounds, spacing, interpolation, data} produce an error
    // mentioning both `unknown` and the offending key name. The five required
    // keys are still present in the source so the missing-key check doesn't
    // fire and confuse the diagnostic count.
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] resolution = 100 } }"#,
    );

    let unknown_errs: Vec<_> = errors_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("unknown") && d.message.contains("resolution"))
        .collect();
    assert_eq!(
        unknown_errs.len(),
        1,
        "expected exactly one 'unknown' error mentioning `resolution`, got {}: {:?}",
        unknown_errs.len(),
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected exactly one total error, got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn compile_field_sampled_unknown_key_with_broken_value_does_not_cascade() {
    // Pins the no-cascade design (functions.rs:327-330): when an unknown
    // sampled-config key is encountered the entry is dropped WITHOUT calling
    // `compile_expr` on its value.  A future refactor that accidentally
    // compiles the dropped value would surface an extra "unresolved name"
    // error from `nonexistent_func()`, breaking this test.  The five
    // required keys are present so missing-key errors do not fire.
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] bogus_key = nonexistent_func() } }"#,
    );

    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected only the unknown-key error (no cascade from compiling the dropped value), got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        errors_only(&module)[0].message.contains("unknown")
            && errors_only(&module)[0].message.contains("bogus_key"),
        "expected the single error to be the unknown-key diagnostic for `bogus_key`, got: {}",
        errors_only(&module)[0].message
    );
}

#[test]
fn compile_field_sampled_duplicate_key_with_broken_value_does_not_cascade() {
    // Sister test to the unknown-key no-cascade pin: a duplicate key whose
    // value is a deliberately broken expression should not surface the
    // unresolved-name error (functions.rs:343-344 drops without compiling).
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" grid = nonexistent_func() bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }"#,
    );

    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected only the duplicate-key error (no cascade from compiling the dropped value), got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        errors_only(&module)[0].message.contains("duplicate")
            && errors_only(&module)[0].message.contains("grid"),
        "expected the single error to be the duplicate-key diagnostic for `grid`, got: {}",
        errors_only(&module)[0].message
    );
}

#[test]
fn compile_field_sampled_rejects_duplicate_grid_key() {
    // Pins the v0.2 behavior: duplicate keys (e.g. two `grid = ...` lines)
    // produce a duplicate-key error referencing the offending key. The four
    // other required keys are present so missing-key errors do not fire.
    let module = compile_source_with_stdlib(
        r#"field def f : Real -> Real { source = sampled { grid = "RegularGrid1" grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0, 2.0] } }"#,
    );

    let dup_errs: Vec<_> = errors_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("grid"))
        .collect();
    assert_eq!(
        dup_errs.len(),
        1,
        "expected exactly one 'duplicate' error mentioning `grid`, got {}: {:?}",
        dup_errs.len(),
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        errors_only(&module).len(),
        1,
        "expected exactly one total error, got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Step 17: compose type check valid ───────────────────────────────

#[test]
fn compile_field_compose_type_check_valid() {
    // Field<Point3, Scalar> composed with Field<Scalar, Scalar> is valid:
    // codomain of first (Scalar) matches domain of second (Scalar).
    // Result should be Field<Point3, Scalar>.
    let module = compile_source(
        r#"
field def f1 : Point3 -> Scalar { source = analytical { |p| 1.0m } }
field def f2 : Scalar -> Scalar { source = analytical { |x| 1.0m } }
field def composed : Point3 -> Scalar { source = composed { |p| f2(f1(p)) } }
"#,
    );
    // Should compile without type errors (warnings for StructureRef types are OK)
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.fields.len(), 3, "expected 3 compiled fields");

    let composed = &module.fields[2];
    assert_eq!(composed.name, "composed");
    assert_eq!(format!("{}", composed.domain_type), "Point3");
    assert_eq!(format!("{}", composed.codomain_type), "Scalar[m]");

    match &composed.source {
        reify_compiler::CompiledFieldSource::Composed { expr } => {
            // Should have compiled the composition lambda
            assert!(
                matches!(expr.kind, reify_ir::CompiledExprKind::Lambda { .. }),
                "expected Lambda expression in composed source, got: {:?}",
                expr.kind
            );
        }
        other => panic!("expected Composed source, got: {:?}", other),
    }
}

// ── Step 19: compose type mismatch ──────────────────────────────────

#[test]
fn compile_field_compose_type_mismatch() {
    // Field<Point3, Vector3> composed with Field<Scalar, Scalar> is INVALID:
    // codomain of first (Vector3) != domain of second (Scalar).
    // Should produce a type error diagnostic.
    let module = compile_source(
        r#"
field def f1 : Point3 -> Vector3 { source = analytical { |p| p } }
field def f2 : Scalar -> Scalar { source = analytical { |x| x } }
field def bad_compose : Point3 -> Scalar { source = composed { |p| f2(f1(p)) } }
"#,
    );
    // Should have at least one diagnostic about field composition type mismatch
    assert!(
        !module.diagnostics.is_empty(),
        "expected a type mismatch diagnostic for mismatched field composition"
    );
    let has_mismatch_error = module.diagnostics.iter().any(|d| {
        d.message.contains("mismatch")
            || d.message.contains("compose")
            || d.message.contains("field")
    });
    assert!(
        has_mismatch_error,
        "expected field composition type mismatch diagnostic, got: {:?}",
        module.diagnostics
    );
}

// ── Step 29: compose type check nested in match ─────────────────────────

#[test]
fn compose_type_check_nested_in_match() {
    // Field composition mismatch nested inside a match arm body.
    // The current walk_field_composition misses Match variants;
    // after rewriting to use CompiledExpr::walk, it will be caught.
    let module = compile_source(
        r#"
enum Mode { A B }

field def f1 : Point3 -> Vector3 { source = analytical { |p| p } }
field def f2 : Scalar -> Scalar { source = analytical { |x| x } }
field def bad_nested : Point3 -> Scalar {
    source = composed { |p| match Mode.A { A => f2(f1(p)) B => f2(f1(p)) } }
}
"#,
    );
    // Should detect the type mismatch even though it's inside a match arm
    let has_mismatch_error = module.diagnostics.iter().any(|d| {
        d.message.contains("mismatch")
            || d.message.contains("compose")
            || d.message.contains("field")
    });
    assert!(
        has_mismatch_error,
        "expected field composition type mismatch diagnostic inside match arm, got: {:?}",
        module.diagnostics
    );
}

// ── Step 33: duplicate field names ───────────────────────────────────────

#[test]
fn compile_duplicate_field_names() {
    let module = compile_source(
        r#"
field def temp : Point3 -> Scalar { source = analytical { |p| p } }
field def temp : Scalar -> Scalar { source = analytical { |x| x } }
"#,
    );
    // Should emit a diagnostic about duplicate entity definition (covers field-vs-field collision
    // now that fields participate in the unified entity namespace per spec §4.2.1).
    let has_dup_error = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("duplicate entity definition") && d.message.contains("temp"));
    assert!(
        has_dup_error,
        "expected 'duplicate entity definition' diagnostic for 'temp', got: {:?}",
        module.diagnostics
    );
    // Should only compile the first field (duplicate skipped)
    assert_eq!(
        module.fields.len(),
        1,
        "expected only 1 compiled field (duplicate should be skipped)"
    );
}

// ── Step 2336: analytical field codomain type-check ─────────────────────────

#[test]
fn compile_field_analytical_codomain_dimension_mismatch_emits_diagnostic() {
    // Body returns Real (param x has default Real type), codomain declared as Scalar[m].
    // implicitly_converts_to(Real, Scalar[LENGTH]) is false → FieldCodomainMismatch.
    let module =
        compile_source("field def temp : Real -> Scalar { source = analytical { |x| x } }");

    let has_mismatch = module.diagnostics.iter().any(|d| {
        d.severity == reify_core::Severity::Error
            && d.code == Some(DiagnosticCode::FieldCodomainMismatch)
    });
    assert!(
        has_mismatch,
        "expected DiagnosticCode::FieldCodomainMismatch error for codomain mismatch, got: {:?}",
        module.diagnostics
    );

    // The diagnostic message should use the canonical phrasing, naming both sides.
    // Checking the full phrase rather than bare type names avoids false positives
    // from substrings like "Vector<Real>", "Scalar<Temperature>", etc.
    //
    // `Scalar` in the source resolves to Type::length() (type_resolution.rs:374-376),
    // which Displays as `Scalar[m]` (ty.rs:296-302 + dimension.rs:308-327: the LENGTH
    // basis dimension emits "m"). Pinning the exact `Scalar[m]` rendering ensures a
    // future change to `Scalar`'s default dimension (e.g. switching to dimensionless
    // or to a different SI base) causes this assertion to fail loudly rather than
    // silently passing with a changed rendering.
    let mismatch_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FieldCodomainMismatch))
        .unwrap();
    assert!(
        mismatch_diag
            .message
            .contains("declared codomain `Scalar[m]`"),
        "expected message to contain 'declared codomain `Scalar[m]`', got: {}",
        mismatch_diag.message
    );
    assert!(
        mismatch_diag
            .message
            .contains("lambda body produces `Real`"),
        "expected message to contain 'lambda body produces `Real`', got: {}",
        mismatch_diag.message
    );
}

// ── Step 2336: positive-path guard — matching codomain does not emit mismatch ─

#[test]
fn compile_field_analytical_matching_codomain_does_not_emit_mismatch() {
    // Body returns Real (2.5 * x + 1.0), codomain declared as Real — types match.
    // No FieldCodomainMismatch diagnostic should be emitted.
    let module = compile_source(
        "field def linear : Real -> Real { source = analytical { |x| 2.5 * x + 1.0 } }",
    );

    let has_mismatch = module
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::FieldCodomainMismatch));
    assert!(
        !has_mismatch,
        "expected NO FieldCodomainMismatch for Real->Real field with Real body, got: {:?}",
        module.diagnostics
    );
}

// ── Task 2414 step-1: pin Int→Real widening arm in field_codomain_compatible ──

#[test]
fn compile_field_analytical_int_body_widens_to_real_codomain() {
    // Body literal `1` is typed as Type::Int (expr.rs:257-258): whole-number
    // literals without a unit suffix always produce Int, not Real.
    // Codomain is Real (Type::Real).
    //
    // implicitly_converts_to(Int, Real) returns false (type_compat.rs:52-169:
    // identity check fails because Int != Real; none of rules 1a/1b/2a/2b/2c/3
    // match the Int→Real direction; the default arm returns false).
    //
    // The dedicated `(Type::Int, Type::Real)` arm at functions.rs:170-171 is
    // the *only* thing that keeps this source valid. Removing that arm would
    // cause field_codomain_compatible to return false and emit
    // DiagnosticCode::FieldCodomainMismatch, making this test fail.
    let module = compile_source("field def f : Real -> Real { source = analytical { |x| 1 } }");

    let has_mismatch = module
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::FieldCodomainMismatch));
    assert!(
        !has_mismatch,
        "expected NO FieldCodomainMismatch for Real->Real field with Int literal body \
         (Int→Real widening must hold), got: {:?}",
        module.diagnostics
    );

    // Discriminate against upstream parser/compiler regressions: a totally broken
    // compilation (e.g. parse failure) would also produce no FieldCodomainMismatch
    // diagnostic but would emit other error diagnostics and yield zero compiled fields.
    assert!(
        errors_only(&module).is_empty(),
        "expected no error diagnostics for valid Int→Real widening, got: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.fields.len(), 1, "expected exactly 1 compiled field");
}

// ── Task 2343 step-3: composed lambda captures referenced fields ────────────
//
// After `phase_augment_composed_captures` runs, a composed field's lambda's
// `captures` Vec must contain the `__field.<name>` cell for each *other* field
// it references inside the body — surfacing field-to-field deps to the
// existing `Lambda { captures, .. }` arm of `collect_value_refs_inner`.
// Analytical fields are unaffected (no field-to-field references possible
// without composed semantics).

#[test]
fn compile_field_composed_lambda_captures_referenced_fields() {
    let module = compile_source(
        r#"
field def f1 : Real -> Real { source = analytical { |p| p } }
field def f2 : Real -> Real { source = analytical { |x| x } }
field def f3 : Real -> Real { source = composed { |p| f2(f1(p)) } }
"#,
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.fields.len(), 3, "expected 3 compiled fields");

    let f3 = module
        .fields
        .iter()
        .find(|f| f.name == "f3")
        .expect("expected field 'f3' in compiled module");

    let captures = match &f3.source {
        reify_compiler::CompiledFieldSource::Composed { expr } => match &expr.kind {
            reify_ir::CompiledExprKind::Lambda { captures, .. } => captures.clone(),
            other => panic!(
                "expected composed source to wrap a Lambda expr, got: {:?}",
                other
            ),
        },
        other => panic!("expected Composed source for 'f3', got: {:?}", other),
    };

    let f1_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f1");
    let f2_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f2");
    assert!(
        captures.contains(&f1_cell),
        "f3 lambda captures should contain __field.f1, got: {:?}",
        captures
    );
    assert!(
        captures.contains(&f2_cell),
        "f3 lambda captures should contain __field.f2, got: {:?}",
        captures
    );

    // Pin the no-cross-talk contract: f1's analytical lambda must not have
    // any field captures (it does not reference any field — and even if it
    // did, only composed fields go through the augmentation pass).
    let f1 = module.fields.iter().find(|f| f.name == "f1").unwrap();
    if let reify_compiler::CompiledFieldSource::Analytical { expr } = &f1.source
        && let reify_ir::CompiledExprKind::Lambda { captures, .. } = &expr.kind
    {
        for cap in captures {
            assert_ne!(
                cap.entity, FIELD_ENTITY_PREFIX,
                "f1 (analytical) should not capture any __field.* cells, got: {:?}",
                cap
            );
        }
    } else {
        panic!("expected f1 to be Analytical Lambda");
    }
}

// ── Step 2344: imported field emits v0.2 deferral diagnostic ────────────────

#[test]
fn compile_field_imported_emits_v02_deferral_diagnostic() {
    let module = compile_source(
        r#"field def data : Point3 -> Scalar { source = imported { path = "data.vtu" format = OpenVDB grid = "voxel" } }"#,
    );

    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected at least one error for imported field source, got: {:?}",
        module.diagnostics
    );

    let has_code_and_msg = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::FieldImportedV02)
            && d.message.contains("v0.2")
            && d.message.contains("imported")
    });
    assert!(
        has_code_and_msg,
        "expected DiagnosticCode::FieldImportedV02 with message containing 'v0.2' and 'imported', got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FieldImportedV02))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}
