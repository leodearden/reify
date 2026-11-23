//! Tests for `ParamDefaultTypeMismatch` — param declared-type vs initializer-dimension
//! mismatch detected at the declaration site.
//!
//! A bare or negated numeric literal at a dimensioned Scalar param is rejected
//! like any other dimensionless initializer (D5 reinstated; see
//! docs/prds/v0_6/dimensioned-construction-strictness.md §0.3).

use reify_core::{Diagnostic, DiagnosticCode};
use reify_test_support::{
    assert_single_declaration_error, compile_source, compile_source_named,
    compile_source_with_prelude, compile_source_with_stdlib, errors_only,
};

/// A structure with `param drum_d : Real = rope_dia * d_ratio` where `rope_dia` has
/// declared type `Length` (Scalar[m]) must produce exactly one `ParamDefaultTypeMismatch`
/// anchored at `param drum_d`, not at a downstream consumer.
#[test]
fn top_level_param_dimension_mismatch_errors_at_declaration() {
    let source = r#"
structure T {
    param rope_dia : Length = 6mm
    param d_ratio : Real = 8.0
    param drum_d : Real = rope_dia * d_ratio
}
"#;

    let message = assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "drum_d",
    );
    assert!(
        message.contains("Real") && message.contains("Scalar[m]"),
        "message must mention 'Real' and 'Scalar[m]', got: {message:?}"
    );
}

/// A structure with valid param annotations — including Int→Real widening, undef
/// default, and correct declared dimensions — must NOT produce any
/// "declared … initializer" diagnostics.
///
/// This guard test passes throughout all steps.
#[test]
fn valid_and_widening_param_annotations_do_not_error() {
    let source = r#"
structure Valid {
    param a : Real = 8.0
    param b : Length = 6mm
    param c : Length = b * 2.0
    param n : Real = 8
    param u : Real = undef
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    // No error should carry both "declared" and "initializer" in its message.
    let false_positive = errors
        .iter()
        .find(|d| d.message.contains("declared") && d.message.contains("initializer"));
    assert!(
        false_positive.is_none(),
        "unexpected 'declared/initializer' error on valid param annotations: {:?}",
        false_positive
    );
}

/// A port-member `param d : Real = rope_dia * 2.0` where `rope_dia : Length` must
/// produce a `ParamDefaultTypeMismatch` error anchored at the port-member param
/// declaration ("`param d`"), not at a downstream consumer.
#[test]
fn port_member_param_dimension_mismatch_errors_at_declaration() {
    let source = r#"
trait T { param d : Real }
structure S {
    param rope_dia : Length = 6mm
    port p : out T { param d : Real = rope_dia * 2.0 }
}
"#;

    let message = assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "d",
    );
    assert!(
        message.contains("Real") && message.contains("Scalar[m]"),
        "expected error message to mention 'Real' and 'Scalar[m]'; got: {message:?}"
    );
}

// ─── Amendment-pass coverage tests ───────────────────────────────────────────

/// Inverts a deliberate pin of the opposite rule: a bare Int or Real literal at
/// a dimensioned Scalar param is rejected (D5 reinstated). The literal tolerance
/// was introduced by #4318; see docs/prds/v0_6/dimensioned-construction-strictness.md §0.3.
#[test]
fn param_int_and_real_literal_on_dimensioned_scalar_errors() {
    for (source, param) in [
        ("structure S { param zero_int : Length = 0 }", "zero_int"),
        ("structure S { param one_int : Length = 1 }", "one_int"),
        (
            "structure S { param half_real : Length = 0.5 }",
            "half_real",
        ),
        (
            "structure S { param large_real : Length = 70.0 }",
            "large_real",
        ),
    ] {
        assert_single_declaration_error(
            source,
            DiagnosticCode::ParamDefaultTypeMismatch,
            "param",
            param,
        );
    }
}

/// Inverts a deliberate pin of the opposite rule: a negated literal at a
/// dimensioned Scalar param is rejected like a bare one (D5 reinstated; the
/// tolerance was introduced by #4318). See docs/prds/v0_6/dimensioned-construction-strictness.md §0.3.
#[test]
fn param_negative_literal_on_dimensioned_scalar_errors() {
    for (source, param) in [
        ("structure S { param neg_real : Length = -5.0 }", "neg_real"),
        ("structure S { param neg_int : Length = -1 }", "neg_int"),
    ] {
        assert_single_declaration_error(
            source,
            DiagnosticCode::ParamDefaultTypeMismatch,
            "param",
            param,
        );
    }
}

/// A param whose declared type is a dimensioned Scalar but whose initializer
/// evaluates to a *different* dimensioned Scalar (e.g. `Length = 5kg`) MUST
/// produce `ParamDefaultTypeMismatch` — this is the primary intended catch of
/// the check.  Scalar[m] and Scalar[kg] are incompatible under `type_compatible`.
#[test]
fn param_different_scalar_dimensions_error() {
    assert_single_declaration_error(
        "structure S { param bad_mass : Length = 5kg }",
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "bad_mass",
    );
}

// ─── Step-6 regression: untyped params must NOT trigger ParamDefaultTypeMismatch ──

/// An untyped port-member param with an enum default must NOT produce a
/// `ParamDefaultTypeMismatch` diagnostic.  For an untyped param the compiler
/// assigns a `Type::Real` inference fallback (entity.rs port-member first pass),
/// NOT a user-declared type, so the declared-vs-initializer cross-check must be
/// suppressed entirely.
///
/// This is the reviewer's exact locus (enum-valued port-param override).
/// RED until step-7 gates `check_param_default_type` on `has_explicit_type`.
#[test]
fn untyped_port_member_param_with_enum_default_does_not_error() {
    let source = r#"
enum FluidType { Liquid, Gas }
trait T { param fluid_type : FluidType }
structure S {
    port p : out T { param fluid_type = FluidType.Liquid }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_positive = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_positive.is_none(),
        "untyped port-member param 'fluid_type = FluidType.Liquid' must NOT produce \
         ParamDefaultTypeMismatch (no explicit type annotation; cell_type is only a \
         Type::Real inference fallback, not a user contract); got: {:?}",
        false_positive
    );
}

/// An untyped top-level structure param with an enum default must NOT produce a
/// `ParamDefaultTypeMismatch` diagnostic.
#[test]
fn untyped_top_level_param_with_enum_default_does_not_error() {
    let source = r#"
enum Color { Red, Green }
structure S {
    param c = Color.Red
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_positive = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_positive.is_none(),
        "untyped top-level param 'c = Color.Red' must NOT produce ParamDefaultTypeMismatch \
         (no explicit type annotation; Type::Real is only an inference fallback, not a \
         user-declared type); got: {:?}",
        false_positive
    );
}

/// An untyped top-level structure param with a dimensioned-unit default must NOT
/// produce a `ParamDefaultTypeMismatch` diagnostic.
#[test]
fn untyped_top_level_param_with_dimensioned_default_does_not_error() {
    let source = r#"
structure S {
    param x = 5mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_positive = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_positive.is_none(),
        "untyped top-level param 'x = 5mm' must NOT produce ParamDefaultTypeMismatch \
         (no explicit type annotation; Type::Real is only an inference fallback, not a \
         user contract for Length); got: {:?}",
        false_positive
    );
}

/// A reciprocal-dimension initializer (`1.0 / 1m`, `Scalar[1/m]`) at a `Length`
/// param MUST produce `ParamDefaultTypeMismatch`: `type_compatible(Scalar[m],
/// Scalar[1/m])` is false.
#[test]
fn param_reciprocal_dim_mismatch_errors() {
    let source = r#"
structure S {
    // Length (Scalar[m]) ≠ reciprocal-dimension expression → error.
    param bad_dim : Length = 1.0 / 1m
}
"#;
    assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "bad_dim",
    );
}

// ─── Dimensionless compound initializer ──────────────────────────────────────

/// A compound initializer that infers dimensionless (`ratio * 2.0`) at a
/// `Length` param MUST produce `ParamDefaultTypeMismatch`:
/// `type_compatible(Scalar[m], Scalar[dimensionless])` is false.
#[test]
fn param_nonliteral_dimensionless_compound_on_dimensioned_scalar_errors() {
    let source = r#"
structure S {
    param ratio : Real   = 8.0
    param x     : Length = ratio * 2.0
}
"#;
    assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "x",
    );
}

// ─── S2 amendment: Int-arm + anti-cascade coverage ───────────────────────────

/// A param declared `Int` with a dimensioned initializer (`5kg`) MUST produce
/// `ParamDefaultTypeMismatch`: `type_compatible(Int, Scalar[kg])` is false.
#[test]
fn param_int_declared_with_dimensioned_initializer_errors() {
    let source = r#"
structure S {
    param x : Int = 5kg
}
"#;
    assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "x",
    );
}

/// A param declared `Int` with a fractional Real initializer (`0.5`) MUST produce
/// `ParamDefaultTypeMismatch`.  `type_compatible(Int, Scalar[dimensionless])` returns
/// false because the Int→dimensionless-scalar widening coercion is one-directional:
/// it allows `Int` where `Scalar[dimensionless]` is declared, NOT the reverse.
#[test]
fn param_int_declared_with_real_initializer_errors() {
    let source = r#"
structure S {
    param x : Int = 0.5
}
"#;
    assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "x",
    );
}

/// A `Real`-declared param with a reciprocal-dimension initializer (`1.0/1m`)
/// MUST produce `ParamDefaultTypeMismatch`: `type_compatible(Real, Scalar[1/m])`
/// is false. The declared=`Length` twin is `param_reciprocal_dim_mismatch_errors`.
#[test]
fn param_real_declared_with_reciprocal_dim_initializer_errors() {
    let source = r#"
structure S {
    // Real (dimensionless) declared, but 1.0/1m is a reciprocal-dimension expression.
    // 1.0/1m correctly infers as Scalar[1/m]; type_compatible(Real, Scalar[1/m]) = false.
    param bad_dim : Real = 1.0 / 1m
}
"#;
    assert_single_declaration_error(
        source,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "bad_dim",
    );
}

/// A param whose *declared type* is unresolvable (`Bogus`) produces exactly ONE
/// error: the root-cause `UnresolvedType` diagnostic.  The secondary
/// `ParamDefaultTypeMismatch` must NOT be emitted.
///
/// **Anti-cascade mechanism (PRD §9 L0, task #4645):** unknown name `Bogus`
/// resolves to `Type::Error` (poison sentinel), NOT `Type::dimensionless_scalar()`.
/// The `declared.is_error()` guard in `check_param_default_type` sees a poisoned
/// declared type and returns early — the spurious "declared type vs initializer
/// dimension" check never runs, so no secondary `ParamDefaultTypeMismatch` is
/// emitted on top of the root-cause `UnresolvedType`.
#[test]
fn param_unresolved_declared_type_anti_cascade_no_secondary_error() {
    let source = r#"
structure S {
    param p : Bogus = 5kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // There must be at least one error (the root-cause unresolved type `Bogus`).
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected an UnresolvedType error for unresolved type 'Bogus'; got: {:?}",
        errors
            .iter()
            .map(|d| (&d.message, &d.code))
            .collect::<Vec<_>>()
    );

    // NO secondary ParamDefaultTypeMismatch: the declared type is poisoned
    // (Type::Error), so the anti-cascade guard fires and suppresses the mismatch check.
    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_none(),
        "expected NO ParamDefaultTypeMismatch for 'param p : Bogus = 5kg' \
         (anti-cascade: unknown-name -> Type::Error -> guard fires -> mismatch suppressed); \
         got unexpected secondary mismatch: {:?}",
        errors
            .iter()
            .map(|d| (&d.message, &d.code))
            .collect::<Vec<_>>()
    );
}

// ─── D4-4: one authoring mistake, one diagnostic ─────────────────────────────

/// A default the declaration-site check rejects is reported once, by that check:
/// the conformance pass does not re-judge it (PRD D4-4).
#[test]
fn param_default_mismatch_is_reported_once() {
    for (source, param) in [
        ("structure S { param i2 : Int = \"no\" }", "i2"),
        ("structure S { param r2 : Real = \"nope\" }", "r2"),
        ("structure S { param m : Length = 5kg }", "m"),
        (
            "trait T { param d : Length }\n\
             structure S { port p : out T { param d : Length = 5kg } }",
            "d",
        ),
    ] {
        assert_single_declaration_error(
            source,
            DiagnosticCode::ParamDefaultTypeMismatch,
            "param",
            param,
        );
    }
}

/// Assert `source` yields exactly ONE error: a conformance `ArgTypeMismatch`.
fn assert_single_conformance_arg_type_mismatch(source: &str) {
    let module = compile_source(source);
    let errors = errors_only(&module);
    let shapes: Vec<_> = errors.iter().map(|d| (&d.message, &d.code)).collect();

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error, got: {shapes:?}"
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "got: {shapes:?}"
    );
}

/// A trait-injected default never reaches the declaration-site check, so the
/// conformance pass still judges it: the D4-4 de-dup must not silence it.
#[test]
fn trait_injected_param_default_mismatch_is_still_reported_by_conformance() {
    assert_single_conformance_arg_type_mismatch(
        "trait T { param d : Length = 5kg }\nstructure S : T { }",
    );
}

/// A bare literal at a trait-injected dimensioned default is likewise judged by
/// the conformance pass alone.
#[test]
fn trait_injected_bare_literal_param_default_is_still_reported_by_conformance() {
    assert_single_conformance_arg_type_mismatch(
        "trait T { param d : Length = 1.0 }\nstructure S : T { }",
    );
}

/// Assert `errors` holds exactly one diagnostic per `(code, param)` pair and
/// nothing else.
fn assert_one_error_per_param(errors: &[&Diagnostic], expected: &[(DiagnosticCode, &str)]) {
    let shapes: Vec<_> = errors.iter().map(|d| (&d.message, &d.code)).collect();
    assert_eq!(errors.len(), expected.len(), "got: {shapes:?}");
    for &(code, param) in expected {
        let matching = errors
            .iter()
            .filter(|d| d.code == Some(code) && d.message.contains(&format!("'{param}'")))
            .count();
        assert_eq!(
            matching, 1,
            "expected one {code:?} naming '{param}', got: {shapes:?}"
        );
    }
}

/// The D4-4 skip is per cell. A declaration-site rejection in a template does
/// not silence the conformance pass for that template's other defaults, whether
/// they are declared locally or injected by a trait.
#[test]
fn declaration_site_rejection_leaves_other_defaults_to_conformance() {
    let source = r#"
trait T { param d : Length = 1.0 }
structure S : T {
    param m : Length = 5kg
    param s : String = 42
}
"#;
    let module = compile_source(source);
    assert_one_error_per_param(
        &errors_only(&module),
        &[
            (DiagnosticCode::ParamDefaultTypeMismatch, "m"),
            (DiagnosticCode::ArgTypeMismatch, "d"),
            (DiagnosticCode::ArgTypeMismatch, "s"),
        ],
    );
}

/// A trait default injected from another file is still judged when its byte
/// offsets coincide with a local param the declaration-site check rejected.
#[test]
fn trait_injected_default_at_a_rejected_params_offsets_in_another_file_is_still_reported() {
    let consumer = "structure S : T { param m : Length = 5kg }";
    let lib_body = "pub trait T { param d : Length = 1.0 }";
    let local_member = "param m : Length = 5kg";
    let injected_member = "param d : Length = 1.0";
    assert_eq!(local_member.len(), injected_member.len());

    let local_offset = consumer.find(local_member).expect("consumer member");
    let pad = local_offset - lib_body.find(injected_member).expect("lib member");
    let lib_src = format!("{}{lib_body}", " ".repeat(pad));
    assert_eq!(lib_src.find(injected_member), Some(local_offset));

    let lib = compile_source_named(&lib_src, "lib");
    let module = compile_source_with_prelude(consumer, &[lib]);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S");
    let cell_span = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .map(|c| c.span)
    };
    let (injected, local) = (cell_span("d"), cell_span("m"));
    assert!(
        injected.is_some() && injected == local,
        "fixture no longer aligns the two spans: d={injected:?}, m={local:?}"
    );

    assert_one_error_per_param(
        &errors_only(&module),
        &[
            (DiagnosticCode::ParamDefaultTypeMismatch, "m"),
            (DiagnosticCode::ArgTypeMismatch, "d"),
        ],
    );
}

/// A rejected default of a generic is reported once, though its monomorph clone
/// carries the same cell under another template name.
#[test]
fn rejected_default_of_a_monomorphised_generic_is_reported_once() {
    let source = r#"
trait Seal {}
structure def ORingSeal : Seal { param d : Real = 10.0 }
structure def Bearing<T: Seal> { param bore : Length = 5kg }
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#;
    let module = compile_source_with_stdlib(source);

    let templates_with_bore = module
        .templates
        .iter()
        .filter(|t| t.value_cells.iter().any(|c| c.id.member == "bore"))
        .count();
    assert_eq!(
        templates_with_bore, 2,
        "expected the generic and its monomorph clone to carry 'bore'"
    );

    assert_one_error_per_param(
        &errors_only(&module),
        &[(DiagnosticCode::ParamDefaultTypeMismatch, "bore")],
    );
}

/// A pure numeric-literal-arithmetic default is neither a single literal nor a
/// dimensioned expression; at a dimensioned param it is rejected once, naming
/// the declared and found types.
#[test]
fn param_pure_literal_arithmetic_on_dimensioned_scalar_errors() {
    for (source, param, found) in [
        ("structure S { param a : Length = 2 + 3 }", "a", "`Int`"),
        (
            "structure S { param b : Length = 2.0 * 3.0 }",
            "b",
            "`Real`",
        ),
    ] {
        let message = assert_single_declaration_error(
            source,
            DiagnosticCode::ParamDefaultTypeMismatch,
            "param",
            param,
        );
        assert!(
            message.contains("`Scalar[m]`") && message.contains(found),
            "param '{param}': message must name declared `Scalar[m]` and found {found}, \
             got: {message:?}"
        );
    }
}

/// B8 no-over-rejection floor: a unit literal at a dimensioned param and a
/// dimensionless literal at `Real` / `Int` are clean.
#[test]
fn dimensioned_unit_literal_and_dimensionless_literal_defaults_are_clean() {
    let source = r#"
structure S {
    param t : Length = 5mm
    param r : Real = 1.0
    param n : Real = 8
    param i : Int = 3
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors, got: {:?}",
        errors
            .iter()
            .map(|d| (&d.message, &d.code))
            .collect::<Vec<_>>()
    );
}
