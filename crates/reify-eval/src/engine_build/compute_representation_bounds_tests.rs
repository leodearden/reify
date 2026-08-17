//! Unit tests for the C-BOUND bound pre-pass `compute_representation_bounds`
//! (task β, #6167 — PRD `docs/prds/v0_6/precision-nominal-representation-guarantee.md` §5).
//!
//! Fixtures are real compiled IR built with `reify_test_support::parse_and_compile`
//! (the no-stdlib variant, mirroring reify-cli's
//! `module_has_representation_within_detects_assertion_vs_plain` — `mm` is a
//! built-in length unit). No kernel, no tessellation, no measurement: every case
//! is compiled IR plus a plain `BTreeMap`, so nothing here needs OCCT and no
//! wall-clock bound is asserted.

use std::collections::BTreeMap;

use reify_test_support::{parse_and_compile, parse_and_compile_with_stdlib};

use super::compute_representation_bounds;

/// Case (a): the table keys on the SUBJECT's declared struct name.
///
/// `Checker` declares `RepresentationWithin(subject, 1mm)` where
/// `param subject : MyGeom`, so the key is `"MyGeom"` — the subject's declared
/// type from `arg0.result_type`, NOT the declaring template `"Checker"`. That is
/// the single easiest thing to get backwards, so the negative half is asserted
/// explicitly.
#[test]
fn bounds_single_direct_constraint_keys_on_subject_struct_name() {
    let module = parse_and_compile(
        r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1mm)
}
"#,
    );
    let bounds = compute_representation_bounds(&module);

    assert_eq!(
        bounds,
        BTreeMap::from([("MyGeom".to_string(), 1e-3)]),
        "the table is keyed on the SUBJECT struct name with the declared bound in SI metres"
    );
    assert!(
        !bounds.contains_key("Checker"),
        "the DECLARING template name must never be a key — the key comes from \
         arg0.result_type (StructureRef), not from the template that declares the constraint"
    );
}

/// Case (b): duplicates on the same subject min-fold (tighter satisfies looser);
/// distinct subjects get distinct keys.
///
/// `CheckerA` bounds `MyGeom` at 1mm and `CheckerB` bounds the same `MyGeom` at
/// 0.2mm → the tighter 2e-4 wins. `CheckerC` bounds a different geometry
/// structure `Other` at 5mm → its own key. Same partial order as
/// `combine_demanded_tolerance` and `tolerance_scope::merge_with_min`.
#[test]
fn bounds_min_fold_duplicates_and_separate_keys_per_struct() {
    let module = parse_and_compile(
        r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Other {
    param y : Real = 2.0
}

structure CheckerA {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1mm)
}

structure CheckerB {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 0.2mm)
}

structure CheckerC {
    param subject : Other
    constraint RepresentationWithin(subject, 5mm)
}
"#,
    );
    let bounds = compute_representation_bounds(&module);

    assert_eq!(
        bounds,
        BTreeMap::from([("MyGeom".to_string(), 2e-4), ("Other".to_string(), 5e-3)]),
        "duplicate bounds on one subject min-fold to the tightest (2e-4, not 1e-3); \
         a second subject structure gets its own key"
    );
}

/// Case (c): emptiness IS F's scoping predicate.
///
/// `bounds.is_empty()` must mean "the module declares no bound", i.e. exactly
/// `!module_has_representation_within(module)` (reify-cli main.rs). Both
/// directions are pinned here: a plain module yields an empty table, and case
/// (a)'s module yields a non-empty one.
///
/// This matters because a module that routes into the kernel-backed check path
/// while carrying an empty bound table would, under δ (#6168), measure nothing —
/// silently degrading a real Satisfied/Violated verdict to Indeterminate.
#[test]
fn bounds_empty_for_module_with_no_representation_within() {
    let plain = parse_and_compile(
        r#"
structure Plain {
    param x : Real = 1.0
    constraint x > 0.0
}
"#,
    );
    assert!(
        compute_representation_bounds(&plain).is_empty(),
        "a module with no RepresentationWithin must yield an EMPTY table — \
         emptiness is F's scoping predicate"
    );

    let assertion = parse_and_compile(
        r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1mm)
}
"#,
    );
    assert!(
        !compute_representation_bounds(&assertion).is_empty(),
        "the other direction: a module that DOES declare a bound must yield a \
         non-empty table"
    );
}

/// Case (d): shapes that fail the shared matcher's gates contribute no key and
/// do not panic (silent-skip posture, inherited from
/// `match_representation_within_shape`).
///
/// Two gate failures are covered:
/// * Gate 3 — `arg0.result_type` is `Scalar`, not `StructureRef`
///   (`RepresentationWithin(x, 1mm)` where `param x : Real`).
/// * Gate 4a — the tolerance literal is not a LENGTH
///   (`RepresentationWithin(subject, 1kg)`).
///
/// Both source forms do compile under `parse_and_compile` (verified while
/// authoring this step), so the skip happens in the matcher rather than in the
/// compiler. That keeps the table in lockstep with reify-cli's routing gate,
/// which delegates to the same matcher and would likewise report `false`.
#[test]
fn bounds_silently_skip_gate_failures() {
    let non_struct_subject = parse_and_compile(
        r#"
structure BadSubject {
    param x : Real = 1.0
    constraint RepresentationWithin(x, 1mm)
}
"#,
    );
    assert!(
        compute_representation_bounds(&non_struct_subject).is_empty(),
        "Gate 3 failure (arg0.result_type is Scalar, not StructureRef) → no key, no panic"
    );

    let non_length_tolerance = parse_and_compile(
        r#"
structure MyGeom {
    param x : Real = 1.0
}

structure BadTol {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1kg)
}
"#,
    );
    assert!(
        compute_representation_bounds(&non_length_tolerance).is_empty(),
        "Gate 4a failure (tolerance literal is not a LENGTH) → no key, no panic"
    );
}

/// Case (e): the stdlib-resolved IR variant.
///
/// `match_representation_within_shape` accepts two IR variants —
/// `UserFunctionCall` (synthetic/test-built expressions) and
/// `FunctionCall { function: ResolvedFunction { name: "RepresentationWithin", .. } }`
/// (compiler-resolved stdlib calls). This case compiles case (a)'s source with
/// `parse_and_compile_with_stdlib` so the stdlib-resolved path is pinned
/// explicitly.
///
/// Measured while authoring this step (task β pre-1 probe): in this repo state
/// BOTH `parse_and_compile` and `parse_and_compile_with_stdlib` already lower
/// `RepresentationWithin` to
/// `FunctionCall { ResolvedFunction { qualified_name: "std::RepresentationWithin" } }`
/// — the plain helper does NOT produce `UserFunctionCall` for this call. So this
/// case is not the *contrast* to the other cases it was designed as; it is a
/// second, independent pin on the stdlib-resolution path (which could diverge
/// from the plain path in future). The `UserFunctionCall` variant is covered by
/// tolerance_combine's own recognizer tests, which build that IR shape directly.
#[test]
fn bounds_recognize_stdlib_resolved_function_call_variant() {
    let module = parse_and_compile_with_stdlib(
        r#"
structure MyGeom {
    param x : Real = 1.0
}

structure Checker {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1mm)
}
"#,
    );
    assert_eq!(
        compute_representation_bounds(&module),
        BTreeMap::from([("MyGeom".to_string(), 1e-3)]),
        "the stdlib-resolved FunctionCall{{ResolvedFunction}} variant must be \
         recognised exactly like the plain-compiled one"
    );
}
