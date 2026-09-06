//! Struct-constructor field-conformance tests (task 5302, struct-ctor-conformance α).
//!
//! Task 5302 generalizes task 4584's struct-ctor conformance chokepoint from its
//! original 4-family allowlist (`List<TraitObject>` / `StructureRef` / `Vector` /
//! `Selector`) to ALL concrete field types, at **Warning** severity behind a single
//! module const (`CTOR_FIELD_CONFORMANCE_SEVERITY`; δ later flips it to Error).
//!
//! These are inline-source integration tests (NOT on-disk `.ri` fixtures under
//! `examples/`, which would be swept by `examples_smoke.rs`). Each `const SOURCE`
//! begins with a `module test.<name>` decl so the compiler does not emit the
//! `W_MODULE_DECL_MISSING` warning — that keeps the per-fixture "exactly one
//! diagnostic" counts exact (the double-emission pin in row 4 / C2(ii) relies on
//! this).
//!
//! Convention (mirrors `param_binding_selector_coercion_tests.rs` /
//! `vec3_type_tests.rs`): inline `const SOURCE` + assertions on the *filtered*
//! diagnostics' code / severity / message. The [`ctor_conformance_diags`] /
//! [`ctor_conformance_warnings`] helpers below filter to just the ctor-conformance
//! diagnostic codes so unrelated diagnostics never pollute the counts.
//!
//! No new diagnostic codes are minted in α; no reify-core change.
//!
//! Task 5303 (struct-ctor-conformance ε) mints exactly two: `CtorUnknownField`
//! and `CtorArity`, for the two lenient `__arg{i}` sites in the
//! `StructureInstanceCtor` by-name binder (PRD §7 rows 11/12 — an unknown named
//! argument, and an over-arity positional argument). Both are emitted from
//! `crates/reify-compiler/src/expr.rs` at the SAME
//! `CTOR_FIELD_CONFORMANCE_SEVERITY` knob as the α surface, so they are part of
//! the ctor-conformance code set below and δ flips them with everything else.

mod common;

use reify_compiler::CompiledModule;
use reify_core::diagnostics::DiagnosticCode;
use reify_core::{
    BASE_UNIT_SYMBOLS, Diagnostic, DimensionVector, NAMED_DIMENSIONS, Severity, SourceSpan,
};
use reify_test_support::{compile_source_with_stdlib, errors_only, warnings_only};

/// True when `code` is one of the diagnostic codes emitted by the struct-ctor
/// field-conformance surface (task 5302 / 5303 / 4584 / 4598 / 4622 / 4444).
///
/// Filtering to this set keeps the per-fixture "exactly one diagnostic" counts
/// from being polluted by unrelated diagnostics (an incidental `W_*` warning, a
/// downstream note, etc.). The first five codes already existed in
/// `diagnostics.rs`; α minted none. ε (task 5303) adds the two structural codes
/// `CtorUnknownField` / `CtorArity` — they belong here because they are emitted
/// at the same `CTOR_FIELD_CONFORMANCE_SEVERITY` knob and δ flips them together
/// with the α type codes, so the ε probes' "exactly N" counts must see them.
fn is_ctor_conformance_code(code: Option<DiagnosticCode>) -> bool {
    matches!(
        code,
        Some(
            DiagnosticCode::ArgTypeMismatch
                | DiagnosticCode::SelectorKindMismatch
                | DiagnosticCode::TypeNotConformingToTrait
                | DiagnosticCode::TypeNotConformingToStructureRef
                | DiagnosticCode::TypeNotConformingToVector
                | DiagnosticCode::CtorUnknownField
                | DiagnosticCode::CtorArity
        )
    )
}

/// All ctor-conformance diagnostics in `module`, of any severity.
///
/// Used by "exactly N diagnostics" / "zero diagnostics" assertions so an
/// incidental unrelated diagnostic does not throw off the count.
fn ctor_conformance_diags(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

/// Ctor-conformance diagnostics in `module` restricted to `Severity::Warning`.
///
/// At α the whole ctor-conformance surface emits at Warning (the knob default),
/// so most probe fixtures assert against this. Intersecting the code filter with
/// [`warnings_only`] guards against a fixture that trips an unrelated warning.
#[allow(dead_code)]
fn ctor_conformance_warnings(module: &CompiledModule) -> Vec<&Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

/// Ctor-conformance diagnostics in `module` restricted to `Severity::Error`.
///
/// Reserved for the (few) sites that must stay Error even at α — currently only
/// the fn-call conformance path, which these ctor fixtures do not exercise; kept
/// for symmetry with [`ctor_conformance_warnings`] and future δ-flip tests.
#[allow(dead_code)]
fn ctor_conformance_errors(module: &CompiledModule) -> Vec<&Diagnostic> {
    errors_only(module)
        .into_iter()
        .filter(|d| is_ctor_conformance_code(d.code))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-1 probes: core general-leaf + implicit-Some + allowlist-flip behaviors.
//
// RED on `main`: rows 2/4/8/13 are silent (or a wrong-code wrapper-shape Error for
// rows 5/8) before the step-2 core mechanism lands. Rows 3/7 and boundary rows
// 8/9 are legality guards that must stay clean before AND after.
// ─────────────────────────────────────────────────────────────────────────────

// ── row 2: value-cell String param given Int → one ArgTypeMismatch Warning ──
const SOURCE_ROW2_VALUE_CELL_STRING: &str = r#"module test.row2
structure def Widget { param label : String }
structure def Root {
    let x = Widget(label: 42)
}
"#;

#[test]
fn row2_value_cell_string_param_given_int_warns_arg_type_mismatch() {
    let module = compile_source_with_stdlib(SOURCE_ROW2_VALUE_CELL_STRING);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "value-cell String←Int must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "α: ctor field conformance is Warning-severity, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "expected ArgTypeMismatch, got: {:?}",
        diags[0].code
    );
    for needle in ["label", "String", "Int"] {
        assert!(
            diags[0].message.contains(needle),
            "message must name {needle:?}, got: {:?}",
            diags[0].message
        );
    }
}

// ── row 4: sub `=` String param given Int → SAME code/msg, exactly one total ──
// (context independence + C2(ii) double-emission pin: the sub `=` RHS is not a
//  StructureInstanceCtor expr, so only the PendingBoundCheck path emits.)
const SOURCE_ROW4_SUB_STRING: &str = r#"module test.row4
structure def Widget { param label : String }
structure def Root {
    sub p = Widget(label: 42)
}
"#;

#[test]
fn row4_sub_string_param_given_int_warns_arg_type_mismatch_exactly_once() {
    let module = compile_source_with_stdlib(SOURCE_ROW4_SUB_STRING);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "sub-path String←Int must emit EXACTLY ONE ctor-conformance diagnostic \
         (C2(ii) double-emission pin), got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "sub context must produce the same code as the value-cell context, got: {:?}",
        diags[0].code
    );
    for needle in ["label", "String", "Int"] {
        assert!(
            diags[0].message.contains(needle),
            "message must name {needle:?}, got: {:?}",
            diags[0].message
        );
    }
}

// ── row 3: value-cell FaceSelector → Option<FaceSelector> (implicit-Some) → 0 ──
const SOURCE_ROW3_VALUE_CELL_OPTION_SELECTOR: &str = r#"module test.row3
structure def PressureLoad { param face : Option<FaceSelector> }
structure def Root {
    let b = box(10mm, 10mm, 10mm)
    let pl = PressureLoad(face: faces_by_normal(b, [0, 0, 1], 1deg))
}
"#;

#[test]
fn row3_value_cell_option_selector_implicit_some_is_clean() {
    let module = compile_source_with_stdlib(SOURCE_ROW3_VALUE_CELL_OPTION_SELECTOR);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "implicit-Some FaceSelector→Option<FaceSelector> must be clean, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must not produce compile errors, got: {:?}",
        errors_only(&module)
    );
}

// ── row 5: sub FaceSelector → Option<FaceSelector> → 0 (wrapper-shape Error on main) ──
const SOURCE_ROW5_SUB_OPTION_SELECTOR: &str = r#"module test.row5
structure def PressureLoad { param face : Option<FaceSelector> }
structure def Root {
    let b = box(10mm, 10mm, 10mm)
    sub p = PressureLoad(face: faces_by_normal(b, [0, 0, 1], 1deg))
}
"#;

#[test]
fn row5_sub_option_selector_implicit_some_is_clean() {
    let module = compile_source_with_stdlib(SOURCE_ROW5_SUB_OPTION_SELECTOR);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "implicit-Some FaceSelector→Option<FaceSelector> (sub context) must be clean; \
         this was a wrapper-shape Error on main (the live hole α closes), got: {diags:#?}"
    );
}

// ── row 8: sub Int → Option<FaceSelector> → one ArgTypeMismatch Warning ──
// (re-coded from the misleading wrapper-shape TypeNotConformingToTrait Error on main)
const SOURCE_ROW8_SUB_OPTION_SELECTOR_INT: &str = r#"module test.row8
structure def PressureLoad { param face : Option<FaceSelector> }
structure def Root {
    sub p = PressureLoad(face: 42)
}
"#;

#[test]
fn row8_sub_option_selector_given_int_warns_arg_type_mismatch() {
    let module = compile_source_with_stdlib(SOURCE_ROW8_SUB_OPTION_SELECTOR_INT);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "Option<FaceSelector>←Int must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "re-coded from the misleading wrapper-shape TypeNotConformingToTrait, got: {:?}",
        diags[0].code
    );
}

// ── §7 row 7: value-cell Int → Real (dimensionless) param → 0 (C1.2 Int→Real) ──
const SOURCE_B7_REAL_PARAM_INT: &str = r#"module test.b7
structure def Gadget { param mag : Real }
structure def Root {
    let x = Gadget(mag: 1)
}
"#;

#[test]
fn boundary7_real_param_given_int_is_clean() {
    let module = compile_source_with_stdlib(SOURCE_B7_REAL_PARAM_INT);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "Int→dimensionless Real is compatible (C1.2); must be clean, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must not produce compile errors, got: {:?}",
        errors_only(&module)
    );
}

// ── §7 row 8: value-cell empty list → List<Geometry> param → 0 (C1.10 TypeParam skip) ──
const SOURCE_B8_EMPTY_LIST_GEOMETRY: &str = r#"module test.b8
structure def Holder { param items : List<Geometry> }
structure def Root {
    let x = Holder(items: [])
}
"#;

#[test]
fn boundary8_empty_list_geometry_is_clean() {
    let module = compile_source_with_stdlib(SOURCE_B8_EMPTY_LIST_GEOMETRY);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "empty-collection arg has TypeParam element type → skipped (C1.10); must be clean, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must not produce compile errors, got: {:?}",
        errors_only(&module)
    );
}

// ── §7 row 9: value-cell bare trait param → 0 (D6 bare-TraitObject exemption) ──
// A NON-conforming arg is used deliberately: it proves the exemption (if bare
// TraitObject were checked here, this would fire TypeNotConformingToTrait).
const SOURCE_B9_BARE_TRAIT_EXEMPT: &str = r#"module test.b9
structure def NotAMaterial { param density : Real = 1.0 }
structure def Host { param m : MaterialSpec }
structure def Root {
    let x = Host(m: NotAMaterial())
}
"#;

#[test]
fn boundary9_bare_trait_param_value_cell_is_exempt() {
    let module = compile_source_with_stdlib(SOURCE_B9_BARE_TRAIT_EXEMPT);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "D6: bare TraitObject params are exempt from value-cell ctor conformance and must \
         stay clean even with a non-conforming arg, got: {diags:#?}"
    );
}

// ── §7 row 13: value-cell Option<trait> non-conforming → one TypeNotConformingToTrait Warning ──
const SOURCE_B13_OPTION_TRAIT_NONCONFORMING: &str = r#"module test.b13
structure def NotAMaterial { param density : Real = 1.0 }
structure def Holder { param mat : Option<MaterialSpec> }
structure def Root {
    let x = Holder(mat: NotAMaterial())
}
"#;

#[test]
fn boundary13_option_trait_param_nonconforming_warns_trait_conformance() {
    let module = compile_source_with_stdlib(SOURCE_B13_OPTION_TRAIT_NONCONFORMING);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "Option<MaterialSpec>←non-conforming must emit exactly one ctor-conformance diagnostic, \
         got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::TypeNotConformingToTrait),
        "expected TypeNotConformingToTrait, got: {:?}",
        diags[0].code
    );
    assert!(
        diags[0].message.contains("MaterialSpec"),
        "message must name the required trait, got: {:?}",
        diags[0].message
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-3 probes: selector diagnostic refinement (D2).
//
// After step-2 the whole selector surface fires as Warnings, but every selector
// mismatch is still coded `ArgTypeMismatch` (the single `emit_selector_mismatch`
// helper). Step-4 refines it: a `Selector(j)→Selector(k)` KIND mismatch must
// carry `SelectorKindMismatch` (4371 BT1 uniformity), while a non-selector arg
// (bare String) stays `ArgTypeMismatch` (disallow-string over-tag guard, 4581).
//
// RED on this branch: row 6's `SelectorKindMismatch` code assertion fails
// (currently `ArgTypeMismatch`). Row 9 is the over-tag guard and is already green
// after step-2.
// ─────────────────────────────────────────────────────────────────────────────

// ── row 6: value-cell EdgeSelector → FaceSelector param → one SelectorKindMismatch ──
// `edges(b)` is typed `Selector(Edge)`; `param loc : FaceSelector` is
// `Selector(Face)`. A kind mismatch must be coded SelectorKindMismatch and name
// BOTH the expected (FaceSelector) and found (EdgeSelector) kinds.
const SOURCE_ROW6_VALUE_CELL_WRONG_SELECTOR_KIND: &str = r#"module test.row6
structure def Holder { param loc : FaceSelector }
structure def Root {
    let b = box(10mm, 10mm, 10mm)
    let h = Holder(loc: edges(b))
}
"#;

#[test]
fn row6_value_cell_wrong_selector_kind_warns_selector_kind_mismatch() {
    let module = compile_source_with_stdlib(SOURCE_ROW6_VALUE_CELL_WRONG_SELECTOR_KIND);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "wrong-kind Selector→Selector param must emit exactly one ctor-conformance diagnostic, \
         got: {diags:#?}"
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "α: ctor field conformance is Warning-severity, got: {:?}",
        diags[0]
    );
    // RED until step-4: `emit_selector_mismatch` currently tags every selector
    // mismatch `ArgTypeMismatch`; a `Selector(j)→Selector(k)` KIND mismatch must
    // carry `SelectorKindMismatch` (D2 / 4371 BT1 uniformity).
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::SelectorKindMismatch),
        "wrong-kind selector must carry SelectorKindMismatch, got: {:?}",
        diags[0].code
    );
    // Message names BOTH the expected (FaceSelector) and found (EdgeSelector)
    // kinds — the Display strings for `SelectorKind::Face` / `SelectorKind::Edge`
    // (reify-core/src/ty.rs). Already satisfied by the current wording; the code
    // assertion above is the RED one.
    for needle in ["FaceSelector", "EdgeSelector"] {
        assert!(
            diags[0].message.contains(needle),
            "message must name {needle:?}, got: {:?}",
            diags[0].message
        );
    }
}

// ── row 9: value-cell bare String literal → FaceSelector param → one ArgTypeMismatch ──
// Over-tag guard (mirrors 4581): a bare String is NOT a selector, so this stays
// ArgTypeMismatch even after step-4 re-codes the Selector(j)→Selector(k) case.
const SOURCE_ROW9_VALUE_CELL_STRING_TO_SELECTOR: &str = r#"module test.row9
structure def Holder { param loc : FaceSelector }
structure def Root {
    let h = Holder(loc: "x_max")
}
"#;

#[test]
fn row9_value_cell_string_to_selector_param_warns_arg_type_mismatch() {
    let module = compile_source_with_stdlib(SOURCE_ROW9_VALUE_CELL_STRING_TO_SELECTOR);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "bare String→FaceSelector must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    // A bare String literal is not a selector — disallow-string stays
    // ArgTypeMismatch; only a Selector(j)→Selector(k) kind mismatch is re-coded to
    // SelectorKindMismatch in step-4 (over-tag guard, 4581).
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "String→Selector must stay ArgTypeMismatch (not SelectorKindMismatch), got: {:?}",
        diags[0].code
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-5 probes: pose-vs-set hint (D2, task 4833 consumer).
//
// PRD §4 D2: a selector-typed param given a coordinate-pose arg (`Frame` /
// `Transform` / `Point`) keeps `DiagnosticCode::ArgTypeMismatch` but carries a
// fixed hint substring — the deterministic string task 4833's fixtures assert
// on. It is a *message variant*, not a new failure class or code.
//
// Fixture note — why a `param : Frame` ValueRef and NOT `frame3(...)`:
// the plan's row-1 sketch used `frame3(...)`, but the Reify compiler does NOT
// special-case `frame3`/`point3`/`orient_identity` in its function-call
// return-type ladder (units.rs result_type tables / expr.rs NoUserFunctions),
// so `frame3(point3(0mm,0mm,0mm), orient_identity())` infers via the first-arg
// fallback to `Scalar[m]` (Length) — NOT a pose type — despite the LSP's
// advertised `-> Frame` signature (verified empirically, 2026-07-20). A
// `Scalar[m]` arg would exercise the generic concrete-leaf mismatch, never the
// pose branch. To obtain a genuine pose-typed arg deterministically we declare
// a param with an explicit resolvable pose type and pass it as a ValueRef:
// `param p : Frame` → `Type::Frame(3)`, `param p : Transform3` →
// `Type::Transform(3)` (type_resolution.rs:576/589). The plan explicitly grants
// this latitude ("whichever concrete pose type it yields; the hint applies to
// Frame/Transform/Point uniformly"). Frame AND Transform variants below force
// the step-6 detection set to cover both pose kinds.
//
// RED on this branch: after step-4 the Option-unwrap arm already reaches the
// selector leaf and `emit_selector_mismatch` emits `ArgTypeMismatch` at Warning,
// but WITHOUT the pose hint substring — so the `contains(POSE_HINT)` assertion
// is the RED one. Count / code / severity are already green.
// ─────────────────────────────────────────────────────────────────────────────

/// The verbatim pose-vs-set hint substring (PRD §4 D2; task 4833's fixtures
/// assert on exactly this string). Kept as one const so the RED test and the
/// step-6 impl cannot drift.
const POSE_HINT: &str = "a coordinate pose is not a region target; select a face/edge/vertex instead";

// ── row 1: value-cell Option<FaceSelector> param given a Frame pose → hint ──
const SOURCE_ROW1_OPTION_SELECTOR_POSE_FRAME: &str = r#"module test.row1
structure def PressureLoad { param face : Option<FaceSelector> }
structure def Root {
    param pose : Frame
    let pl = PressureLoad(face: pose)
}
"#;

#[test]
fn row1_value_cell_option_selector_given_pose_frame_warns_with_pose_hint() {
    let module = compile_source_with_stdlib(SOURCE_ROW1_OPTION_SELECTOR_POSE_FRAME);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "pose→Option<FaceSelector> must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "α: ctor field conformance is Warning-severity, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "pose-vs-set stays ArgTypeMismatch (a message variant, not a new code), got: {:?}",
        diags[0].code
    );
    // RED until step-6: the pose branch must append the fixed hint substring.
    assert!(
        diags[0].message.contains(POSE_HINT),
        "pose arg at a selector field must carry the pose-vs-set hint {POSE_HINT:?}, got: {:?}",
        diags[0].message
    );
}

// ── bare selector param (FaceSelector) given a Frame pose → hint ──
const SOURCE_BARE_SELECTOR_POSE_FRAME: &str = r#"module test.row1b
structure def Holder { param loc : FaceSelector }
structure def Root {
    param pose : Frame
    let h = Holder(loc: pose)
}
"#;

#[test]
fn bare_selector_param_given_pose_frame_warns_with_pose_hint() {
    let module = compile_source_with_stdlib(SOURCE_BARE_SELECTOR_POSE_FRAME);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "pose→FaceSelector must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "pose-vs-set stays ArgTypeMismatch, got: {:?}",
        diags[0].code
    );
    // RED until step-6.
    assert!(
        diags[0].message.contains(POSE_HINT),
        "pose arg at a bare selector field must carry the pose-vs-set hint {POSE_HINT:?}, got: {:?}",
        diags[0].message
    );
}

// ── bare selector param (FaceSelector) given a Transform3 pose → hint ──
// Uniformity: the hint applies to every pose kind, so the step-6 detection set
// must include Transform, not only Frame.
const SOURCE_BARE_SELECTOR_POSE_TRANSFORM: &str = r#"module test.row1c
structure def Holder { param loc : FaceSelector }
structure def Root {
    param pose : Transform3
    let h = Holder(loc: pose)
}
"#;

#[test]
fn bare_selector_param_given_pose_transform_warns_with_pose_hint() {
    let module = compile_source_with_stdlib(SOURCE_BARE_SELECTOR_POSE_TRANSFORM);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "Transform pose→FaceSelector must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "pose-vs-set stays ArgTypeMismatch, got: {:?}",
        diags[0].code
    );
    // RED until step-6 (and requires Transform in the detection set).
    assert!(
        diags[0].message.contains(POSE_HINT),
        "Transform pose arg at a selector field must carry the pose-vs-set hint {POSE_HINT:?}, got: {:?}",
        diags[0].message
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-7 probes: param-default extension (D8, §7 row 10).
//
// A struct `param`'s DEFAULT expression is checked by
// `check_param_default_conformance` (conformance/mod.rs) — a distinct ctor-
// conformance entry from the call-site arg walker. On main it covers only
// `StructureRef` and `Geometry` param defaults (a `_ => continue` skips every
// other cell type), so a `String` param with an `Int` default is SILENT. Step-8
// routes every concrete param default through the shared walker at ctx.severity,
// so a String←Int default warns `ArgTypeMismatch` (Warning at α) — the same code
// and message shape the call-site probes assert (context independence: a bad
// default and a bad explicit arg diagnose identically).
//
// A lone `structure def` (no instantiation) is enough: the check runs over every
// template unconditionally (see `solid_param_with_non_geometry_default_rejected`
// in solid_param_tests.rs, which pins `structure def W3 { param g : Solid = 42 }`
// with no call site).
//
// RED on this branch: the String-default warning assertion fails (silent before
// step-8). The valid-default guard (Int=3, Real=1) is clean before AND after.
// ─────────────────────────────────────────────────────────────────────────────

// ── §7 row 10: param default String ← Int literal → one ArgTypeMismatch Warning ──
const SOURCE_PARAM_DEFAULT_STRING_GIVEN_INT: &str = r#"module test.pd_string
structure def LabelHolder { param label : String = 42 }
"#;

#[test]
fn param_default_string_given_int_warns_arg_type_mismatch() {
    let module = compile_source_with_stdlib(SOURCE_PARAM_DEFAULT_STRING_GIVEN_INT);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "String param with an Int default must emit exactly one ctor-conformance \
         diagnostic (param-default path, D8), got: {diags:#?}"
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "α: param-default conformance is knob-governed (Warning), got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "concrete param-default mismatch must carry ArgTypeMismatch (same code as the \
         call-site leaf), got: {:?}",
        diags[0].code
    );
    for needle in ["label", "String", "Int"] {
        assert!(
            diags[0].message.contains(needle),
            "message must name {needle:?}, got: {:?}",
            diags[0].message
        );
    }
}

// ── valid param defaults (Int←Int, dimensionless Real←Int) → 0 (guard) ──
// `param n : Int = 3` is trivially compatible; `param mag : Real = 1` is the
// C1.2 Int→dimensionless-Real coercion — both must stay clean before AND after
// step-8 so the extension does not over-warn on legal defaults.
const SOURCE_PARAM_DEFAULT_VALID: &str = r#"module test.pd_valid
structure def Config {
    param n : Int = 3
    param mag : Real = 1
}
"#;

#[test]
fn param_default_valid_int_and_real_is_clean() {
    let module = compile_source_with_stdlib(SOURCE_PARAM_DEFAULT_VALID);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "valid Int←Int and dimensionless Real←Int param defaults must be clean, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must not produce compile errors, got: {:?}",
        errors_only(&module)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-9 probes: per-context coverage (Q4/D10), span anchoring (Q1), and the
// examples/fea_pressure_smoke.ri implicit-Some regression.
//
// (a) Per-context coverage: a `String`←`Int` ctor mismatch (`Widget(label: 42)`
//     with `param label : String`) must warn `ArgTypeMismatch` in EVERY parsing
//     context that routes through the two ctor-conformance entries — the fields
//     `for_each_template_root_expr` enumerates plus the free-fn / assoc-fn body
//     loops (entities_phase.rs). Syntax per context confirmed against the
//     tree-sitter grammar and passing tests/examples (task 5302 syntax survey):
//       - constraint keyword is `constraint` (there is NO `require`);
//       - a "realization" is a geometry-op call bound in a value cell (there is
//         NO `realize`/`realization` keyword) — the ctor rides an op argument;
//       - guarded groups use `where <cond> { … }` (NOT `when`/`if`);
//       - a forall body is `forall v in <coll>: constraint <expr>`.
//     The "colon-form arg-bearing sub" (`sub p : Widget(label: 42)`) is
//     deliberately ABSENT: no colon-form `sub` arm admits `(args)` in the
//     grammar, so it does not parse — only the `=` instantiation form (row-4
//     context) carries ctor args.
//
// (b) Span anchoring: the emitted diagnostic's label span must anchor at the
//     offending ctor call-site, NOT `SourceSpan::empty`. The free-fn body is
//     walked with a `SourceSpan::empty(0)` representative span today, so the
//     label span is empty on this branch — RED until step-10 threads the
//     StructureInstanceCtor node's own call-site span (task 4089 `.span`).
//
// (c) Real-stdlib regression: examples/fea_pressure_smoke.ri exercises
//     `PressureLoad(face: face(body, "x_max"))` where `face : Option<FaceSelector>`
//     and `face(...)` is `Selector(Face)` — the implicit-Some legality this task
//     opens. It must compile with ZERO ctor-conformance diagnostics.
// ─────────────────────────────────────────────────────────────────────────────

// ── (a) per-context coverage sources: `Widget(label: 42)` (String←Int) ──

const SRC_CTX_VALUE_CELL: &str = r#"module test.ctx_value_cell
structure def Widget { param label : String }
structure def Root {
    let x = Widget(label: 42)
}
"#;

const SRC_CTX_SUB: &str = r#"module test.ctx_sub
structure def Widget { param label : String }
structure def Root {
    sub p = Widget(label: 42)
}
"#;

// A constraint expr is a full `_expression`; `==` makes it Bool-shaped. Two ctor
// calls (one per operand) → this context legitimately hosts >1 diagnostic, hence
// the `.any()` coverage predicate below rather than an exactly-one count.
const SRC_CTX_CONSTRAINT: &str = r#"module test.ctx_constraint
structure def Widget { param label : String }
structure def Root {
    constraint Widget(label: 42) == Widget(label: 42)
}
"#;

// "Realization" = a geometry-op call bound in a value cell; the ctor rides a
// positional op argument. `box` also rejects the non-Length arg, but that is a
// distinct (non-ctor-conformance) diagnostic filtered out by the code predicate.
const SRC_CTX_REALIZATION: &str = r#"module test.ctx_realization
structure def Widget { param label : String }
structure def Root {
    let g = box(Widget(label: 42), 10mm, 10mm)
}
"#;

const SRC_CTX_PORT_MEMBER_DEFAULT: &str = r#"module test.ctx_port
structure def Widget { param label : String }
trait P {}
structure def Root {
    port x : P {
        param m : Widget = Widget(label: 42)
    }
}
"#;

const SRC_CTX_GUARDED_GROUP: &str = r#"module test.ctx_guard
structure def Widget { param label : String }
structure def Root {
    where true {
        let m = Widget(label: 42)
    }
}
"#;

const SRC_CTX_FORALL_BODY: &str = r#"module test.ctx_forall
structure def Widget { param label : String }
structure def Root {
    forall v in [1, 2, 3]: constraint Widget(label: 42) == Widget(label: 42)
}
"#;

const SRC_CTX_FREE_FN_BODY: &str = r#"module test.ctx_free_fn
structure def Widget { param label : String }
fn make() -> Widget { Widget(label: 42) }
"#;

// Associated-function body — DOCUMENTED as not-currently-routing (per step-9's
// "documenting any form that does not parse/route"). A `Widget(label: 42)` ctor
// placed in an assoc-fn body PARSES in all three forms — a trait DEFAULT fn body
// realized on a conforming structure, a structure's own trait-fn override, and a
// bare structure-body fn — but none emits a ctor-conformance diagnostic in this
// task's α surface (empirically zero diagnostics in every form). The existing
// `assoc_fn_body_bad_arg_emits_type_not_conforming_to_trait` fixture proves the
// assoc-fn body IS walked for a `self`-dependent fn-CALL trait-conformance check
// (`couple(self)`); a self-independent nested StructureInstanceCtor in that same
// body is not re-checked by the ctor entry here. This is a coverage observation,
// not a regression (nothing about assoc-fn bodies changed in α), and is left as a
// documented gap rather than asserted, so the sweep below stays green and honest.
// A `module test.ctx_assoc_fn` form: `trait Mk { fn make(self) -> Widget {
// Widget(label: 42) } }  structure def Root : Mk { param seed : Int = 0 }`.

/// True when `module` carries at least one ctor-conformance `ArgTypeMismatch`
/// Warning whose message names `label` / `String` / `Int` — the signature of the
/// `Widget(label: 42)` String←Int mismatch. `.any()` (not exactly-one) because a
/// context may legitimately host more than one ctor call (`constraint A == B`) or
/// draw an incidental non-ctor-conformance diagnostic (already filtered).
fn has_string_int_arg_type_mismatch_warning(module: &CompiledModule) -> bool {
    ctor_conformance_diags(module).iter().any(|d| {
        d.severity == Severity::Warning
            && d.code == Some(DiagnosticCode::ArgTypeMismatch)
            && ["label", "String", "Int"].iter().all(|n| d.message.contains(n))
    })
}

/// (a) D10 per-context coverage sweep. A String←Int ctor mismatch must warn
/// `ArgTypeMismatch` wherever a `StructureInstanceCtor` can be routed through the
/// two ctor-conformance entries. Collects ALL non-firing contexts before
/// asserting so one run reports the full picture.
#[test]
fn per_context_string_int_ctor_mismatch_warns_everywhere() {
    let cases: &[(&str, &str)] = &[
        ("value-cell let", SRC_CTX_VALUE_CELL),
        ("sub `=`", SRC_CTX_SUB),
        ("constraint expr", SRC_CTX_CONSTRAINT),
        ("realization arg", SRC_CTX_REALIZATION),
        ("port member default", SRC_CTX_PORT_MEMBER_DEFAULT),
        ("guarded-group member", SRC_CTX_GUARDED_GROUP),
        ("forall body", SRC_CTX_FORALL_BODY),
        ("free-fn body", SRC_CTX_FREE_FN_BODY),
        // "assoc-fn body" is documented above as not-currently-routing (parses in
        // three forms, emits zero ctor-conformance diagnostics), so it is a
        // documented gap rather than an asserted row.
    ];
    let mut missing: Vec<(&str, String)> = Vec::new();
    for &(label, source) in cases {
        let module = compile_source_with_stdlib(source);
        if !has_string_int_arg_type_mismatch_warning(&module) {
            missing.push((label, format!("{:#?}", module.diagnostics)));
        }
    }
    assert!(
        missing.is_empty(),
        "per-context coverage: these contexts did NOT emit the expected String←Int \
         ArgTypeMismatch Warning:\n{}",
        missing
            .iter()
            .map(|(l, d)| format!("  [{l}] diagnostics:\n{d}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── (b) span anchoring: free-fn body → empty representative span today ──

const SRC_SPAN_FN_BODY: &str = r#"module test.span_fn
structure def Widget { param label : String }
fn make() -> Widget { Widget(label: 42) }
"#;

/// (b) Q1 span anchoring. The free-fn body routes through
/// `check_expr_struct_ctor_args` with a `SourceSpan::empty(0)` representative
/// span, so on this branch the emitted label span is EMPTY. Step-10 threads the
/// `StructureInstanceCtor` node's own call-site span (task 4089 `.span`) so the
/// label anchors at the offending `Widget(label: 42)` call — RED until then.
#[test]
fn ctor_conformance_label_span_anchors_at_ctor_call_site() {
    let module = compile_source_with_stdlib(SRC_SPAN_FN_BODY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "free-fn body String←Int must emit exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert!(
        !diags[0].labels.is_empty(),
        "ctor-conformance diagnostic must carry a label span, got: {:?}",
        diags[0]
    );
    let span: SourceSpan = diags[0].labels[0].span;
    // RED until step-10: today the label span is `SourceSpan::empty(0)`.
    assert!(
        !span.is_empty(),
        "label span must be NON-empty (anchored at the ctor call site) rather than \
         SourceSpan::empty, got: {span:?}"
    );
    let sliced = &SRC_SPAN_FN_BODY[span.start as usize..span.end as usize];
    assert!(
        sliced.starts_with("Widget") && sliced.contains("label: 42"),
        "label span must anchor at the offending `Widget(label: 42)` ctor call, got slice {sliced:?}"
    );
}

// ── (c) real-stdlib implicit-Some regression ──

/// (c) examples/fea_pressure_smoke.ri exercises the live implicit-Some legality
/// this task opens: `PressureLoad(face: face(body, "x_max"))` where
/// `face : Option<FaceSelector>` and `face(...)` yields `Selector(Face)`. It must
/// compile with ZERO ctor-conformance diagnostics AND no compile errors.
#[test]
fn fea_pressure_smoke_example_has_no_ctor_conformance_diagnostics() {
    const FEA_SMOKE: &str = include_str!("../../../examples/fea_pressure_smoke.ri");
    let module = compile_source_with_stdlib(FEA_SMOKE);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "examples/fea_pressure_smoke.ri must compile with ZERO ctor-conformance diagnostics \
         (real-stdlib implicit-Some legality: PressureLoad.face is Option<FaceSelector> given a \
         FaceSelector via face(body, \"x_max\")), got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fea_pressure_smoke.ri must compile without errors, got: {:?}",
        errors_only(&module)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-11 probes: per-family false-positive fences + α-value-floor guards.
//
// The general concrete-leaf arm shipped with a NEGATIVE skip list (`!matches!(
// param_type, Type::Geometry | Type::TypeParam(_))`), i.e. every present and
// future `Type` variant was opted IN to raw `type_compatible` unless someone
// remembered to carve it out. `type_compatible` is a call-site coercion
// predicate that assumes both sides carry genuinely inferred types; it is not
// valid against the expression compiler's placeholder/erased result_types, nor
// against literal shapes for which no coercion rule exists. Five families were
// therefore false-rejected across 15+ shipped examples.
//
// Step-12 inverted the gate to a positive, explicitly-vetted allowlist. Task
// 5465 then PROMOTED four of those five families back in — not by re-broadening
// the gate, but by giving each family a rule that is valid against the
// placeholder/erased types the expression compiler actually produces: dedicated
// shape-based arms for Point / Field / Matrix / Tensor, and an
// `enum_payload_compatible` short-circuit for Enum / Applied.
//
// The groups below are the fence around that promotion:
//
//   (b) PROMOTED families — each contributes a PAIR: a clean fixture that must
//       stay at ZERO ctor-conformance diagnostics (the false positive that
//       caused the family's original exclusion), and at least one value floor
//       that must emit exactly one Warning (proving the family is genuinely
//       checked and not merely re-excluded under a new name). Each family also
//       carries a wrapper-composition probe so a leaf arm cannot be added in a
//       position the List/Option recursion never reaches.
//
//       Accepts that no inline `.ri` fixture can reach — the `Point`/`Matrix`
//       ARITY rules, the `Matrix` nominal self-accept, the `Field` arm's lambda
//       accept, and the enum-erasure REVERSE pairing — are pinned in
//       `conformance/mod.rs`'s own `mod tests`, which constructs the `Type`s
//       directly. Each site here points at its counterpart so the split stays
//       navigable.
//
//       One family — dimensioned `Scalar` — is deliberately still HELD, with a
//       clean-only probe and a doc comment naming its owner. It is the fifth
//       family, and its absence from group (b)'s pair pattern is the signal.
//
//   (c) α-VALUE-FLOOR guards — the families that were vetted at α must still
//       emit exactly one Warning. GREEN before and after both step-12 and 5465.
//       Their presence is what keeps each change a re-shaping rather than a
//       revert.
// ─────────────────────────────────────────────────────────────────────────────

// ── (b) excluded family: Point ← numeric-fallback placeholder ────────────────
//
// `point3(0m, 0m, 0m)` is a `CompiledExprKind::FunctionCall` whose result_type
// is the expression compiler's numeric fallback `Scalar[m]`, NOT `Type::Point`.
// `type_compatible` has a Point-vs-Point arm but no Point-vs-Scalar arm, so the
// general leaf arm false-rejects. This is the same placeholder class the
// pre-existing `Type::Geometry` carve-out and `promote_function_call_to_structure_ref`
// exist for. Shape taken from examples/anisotropic_bar.ri and the five
// examples/tensegrity_*.ri files.
const SRC_FAMILY_POINT: &str = r#"module test.family_point
structure def Anchor { param origin : Point3<Length> }
structure def Root {
    let a = Anchor(origin: point3(0m, 0m, 0m))
}
"#;

/// Clean fixture for the promoted `Point` family.
///
/// `point3` is a stdlib EVAL-BUILTIN (`crates/reify-stdlib/src/geometry.rs:942`)
/// with no `.ri` signature, so it carries no declared return type at compile
/// time and the call compiles to a `CompiledExprKind::FunctionCall` typed
/// `Scalar[m]` — the expression compiler's numeric fallback — never
/// `Type::Point`. The dedicated `Point` arm tolerates scalar-like args as
/// exactly that placeholder.
///
/// Shape from `examples/anisotropic_bar.ri:82` (`origin: point3(0m, 0m, 0m)`)
/// and `examples/dynamics/pendulum_idyn.ri:29` (`com:`).
#[test]
fn point_param_given_placeholder_function_call_stays_clean() {
    let module = compile_source_with_stdlib(SRC_FAMILY_POINT);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a Point3<Length> param given `point3(0m, 0m, 0m)` must emit ZERO ctor-conformance \
         diagnostics — the arg's result_type is the numeric-fallback placeholder Scalar[m], \
         not Type::Point, so `type_compatible` cannot judge it. Got: {diags:#?}"
    );
}

const SRC_LIST_OF_POINT_PLACEHOLDERS: &str = r#"module test.list_point
structure def Truss { param nodes : List<Point3<Length>> }
structure def Root {
    let t = Truss(nodes: [point3(0m, 0m, 0m), point3(1m, 0m, 0m), point3(0m, 1m, 0m)])
}
"#;

/// Wrapper composition on the clean side: the placeholder tolerance must be
/// reached PER ELEMENT through the walker's `ListLiteral` recursion.
///
/// Shape from `examples/tensegrity_pavilion.ri:53-58`, where the `point3(…)`
/// calls sit inside a list literal.
#[test]
fn list_of_point_param_given_placeholder_calls_stays_clean() {
    let module = compile_source_with_stdlib(SRC_LIST_OF_POINT_PLACEHOLDERS);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a List<Point3<Length>> param given a list literal of `point3(…)` calls must emit ZERO \
         ctor-conformance diagnostics — each element is the same numeric-fallback placeholder. \
         Got: {diags:#?}"
    );
}

const SRC_POINT_GIVEN_STRING: &str = r#"module test.point_string
structure def Anchor { param origin : Point3<Length> }
structure def Root {
    let a = Anchor(origin: "origin")
}
"#;

/// Value floor for the promoted `Point` family: a `String` is not point-shaped
/// and is not the numeric placeholder, so it must warn.
///
/// This is the probe that fences the placeholder tolerance: it is narrow
/// (`Int | Scalar` only), NOT `type_compat.rs::is_scalar_like_leaf`, which also
/// admits `Bool`/`String`/`Enum`/`StructureRef`/`TraitObject`/`Geometry`.
#[test]
fn point_param_given_string_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_POINT_GIVEN_STRING,
        "origin",
        "Point3<Length> ← String",
    );
}

// The `Point` arm's ARITY rule ("a `Point2` value is not a valid substitute for
// a `Point3` param", mirroring the `Type::Vector` arm) is NOT pinned here.
// `resolve_parameterized_builtin_type` recognises `Point3` only
// (crates/reify-compiler/src/type_resolution.rs:3192) — there is no `Point2`
// surface spelling — so no inline `.ri` fixture can produce a
// `Type::Point { n: 2, .. }` arg. It is pinned instead by
// `point_param_rejects_wrong_arity_point_arg` in `conformance/mod.rs`'s own
// `mod tests`, which constructs the `Type` directly, alongside
// `point_param_accepts_dimensionless_point_arg` for the loose-quantity leg.
// `vector_param_rejects_wrong_arity_vector_arg` sits there for the same reason.

const SRC_OPTION_POINT_GIVEN_STRING: &str = r#"module test.option_point_string
structure def Anchor { param origin : Option<Point3<Length>> }
structure def Root {
    let a = Anchor(origin: "origin")
}
"#;

/// Wrapper composition: the Option-unwrap arm (implicit-`Some`) must reach the
/// new `Point` arm, not fall through to the wrapper-shape catch-all.
#[test]
fn option_wrapped_point_param_given_string_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_OPTION_POINT_GIVEN_STRING,
        "origin",
        "Option<Point3<Length>> ← String",
    );
}

// ── (b) excluded family: Matrix ← nested list literal ────────────────────────
//
// A nested list literal is the idiomatic spelling of a Matrix3x3, but it
// compiles to `List<List<Real>>` and `type_compatible` has no
// `List<List<Real>>` → Matrix arm. Unlike the Point family this is NOT a
// placeholder artifact — it is a genuinely missing coercion rule, so it cannot
// be fixed by placeholder detection alone. Shape from the MassProperties ctor
// in every examples/dynamics/*_idyn.ri.
const SRC_FAMILY_MATRIX: &str = r#"module test.family_matrix
structure def Body { param inertia : Matrix<3, 3, MomentOfInertia> }
structure def Root {
    let b = Body(inertia: [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]])
}
"#;

/// Clean fixture for the promoted `Matrix` family.
///
/// A nested list literal is the IDIOMATIC spelling of a `Matrix3x3` — it is what
/// the `MassProperties` ctor in `examples/dynamics/pendulum_idyn.ri:29` writes —
/// and it compiles to `List<List<Real>>`. `List<List<Real>>` carries no element
/// counts in its type, so the declared m/n arity is structurally unverifiable at
/// this leaf; the dedicated `Matrix`/`Tensor` arm therefore accepts `Type::List`
/// and deliberately applies NO arity check (see the arm comment for the
/// asymmetry with the `Point`/`Vector` arms).
#[test]
fn matrix_param_given_nested_list_literal_stays_clean() {
    let module = compile_source_with_stdlib(SRC_FAMILY_MATRIX);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a Matrix<3,3,_> param given a nested list literal must emit ZERO ctor-conformance \
         diagnostics — the literal compiles to List<List<Real>> and no List→Matrix coercion \
         rule exists in type_compat.rs. Got: {diags:#?}"
    );
}

const SRC_MATRIX_GIVEN_STRING: &str = r#"module test.matrix_string
structure def Body { param inertia : Matrix<3, 3, MomentOfInertia> }
structure def Root {
    let b = Body(inertia: "heavy")
}
"#;

/// Value floor for the promoted `Matrix` family.
#[test]
fn matrix_param_given_string_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_MATRIX_GIVEN_STRING,
        "inertia",
        "Matrix<3,3,MomentOfInertia> ← String",
    );
}

const SRC_MATRIX_GIVEN_STRING_LIST: &str = r#"module test.matrix_string_list
structure def Body { param inertia : Matrix<3, 3, MomentOfInertia> }
structure def Root {
    let b = Body(inertia: ["a", "b"])
}
"#;

/// Value floor on the `Matrix`/`Tensor` arm's `Type::List` ACCEPT itself.
///
/// The arm accepts `Type::List` because a nested list literal is the idiomatic
/// `Matrix3x3` spelling — but "is a list" is a weaker claim than "is a matrix
/// literal". A `Matrix`-typed param never pairs with the literal walker's
/// `(Type::List(param), ListLiteral)` arm (the param is `Matrix`, not `List`),
/// so `["a", "b"]` reaches this leaf as a bare `List<String>` type. An
/// unconstrained `Type::List(_)` accept therefore left the just-promoted family
/// silent on exactly the shape the accept was written to admit.
///
/// `list_bottoms_out_numeric` closes it by peeling `List` recursively and
/// requiring a numeric/tensor bottom — the same narrowness the `Point` arm's
/// placeholder tolerance already had.
#[test]
fn matrix_param_given_string_list_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_MATRIX_GIVEN_STRING_LIST,
        "inertia",
        "Matrix<3,3,MomentOfInertia> ← List<String>",
    );
}

const SRC_LIST_OF_MATRIX_GIVEN_STRING_ELEMENT: &str = r#"module test.list_matrix_string
structure def Body { param inertias : List<Matrix<3, 3, MomentOfInertia>> }
structure def Root {
    let b = Body(inertias: ["a"])
}
"#;

/// Wrapper composition for the `Matrix`/`Tensor` family: the arm must be
/// reachable THROUGH the walker's `List` recursion, not only at the top level.
///
/// The `Point` and `Field` families each have one of these; `Matrix`/`Tensor`
/// did not, so nothing pinned that a `List<Matrix<…>>` param's elements are
/// judged at all.
#[test]
fn list_of_matrix_param_given_string_element_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_LIST_OF_MATRIX_GIVEN_STRING_ELEMENT,
        "inertias",
        "List<Matrix<3,3,MomentOfInertia>> ← [String]",
    );
}

const SRC_TENSOR_GIVEN_STRING: &str = r#"module test.tensor_string
structure def Body { param stress : Tensor<2, 3, Pressure> }
structure def Root {
    let b = Body(stress: "t")
}
"#;

/// Value floor for the promoted `Tensor` family (which the task title groups
/// with `Matrix`, and which the combined arm handles).
///
/// `Tensor<rank, n, Q>` is the surface spelling `resolve_parameterized_builtin_type`
/// accepts (`type_resolution.rs:3220`, three type args).
#[test]
fn tensor_param_given_string_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_TENSOR_GIVEN_STRING,
        "stress",
        "Tensor<2,3,Pressure> ← String",
    );
}

const SRC_TENSOR_RANK1_GIVEN_VECTOR: &str = r#"module test.tensor_vector
structure def Body { param axis : Tensor<1, 3, Length> }
structure def Root {
    let b = Body(axis: vec3(0m, 0m, 1m))
}
"#;

/// Regression fence: the new arm must not reject an ALREADY-legal conversion.
///
/// `implicitly_converts_to` Rules 1a/1b (`type_compat.rs:83-108`) make
/// `Vector<N, Q>` and `Tensor<1, N, Q>` interconvertible, so a `Vector3`-typed
/// arg at a rank-1 `Tensor` param is legal today and must stay silent.
#[test]
fn tensor_param_given_vector_stays_clean() {
    let module = compile_source_with_stdlib(SRC_TENSOR_RANK1_GIVEN_VECTOR);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a Tensor<1,3,Length> param given a vec3 arg must emit ZERO ctor-conformance \
         diagnostics — implicitly_converts_to Rules 1a/1b already make Vector<N,Q> and \
         Tensor<1,N,Q> interconvertible. Got: {diags:#?}"
    );
}

// The `Matrix`/`Tensor` arm's NOMINAL SELF-ACCEPT (a genuinely `Type::Matrix`-
// typed arg at a `Matrix` param, and with it the deliberate ABSENCE of an m/n
// arity check) is pinned by `matrix_param_accepts_matrix_arg_without_arity_check`
// in `conformance/mod.rs`'s own `mod tests`, alongside the Point-arity probes
// and for the same reason: no inline `.ri` fixture reliably yields a
// `Type::Matrix`-typed arg, so the `Type` is constructed directly. The probes
// above cover only the LOOSE accepts (nested list literal, `Vector` →
// `Tensor<1,…>`).

// ── (b) excluded family: Field ← erased Field<Real, Real> ────────────────────
//
// An analytical `field def` erases both slots to the numeric fallback, so its
// result_type is `Field<Real, Real>` whatever the declared domain/codomain.
// Shape from examples/fea_shell_channels.ri, which alone emitted six such
// warnings (top / mid / bottom / displacement / stress / frame).
const SRC_FAMILY_FIELD: &str = r#"module test.family_field
field def scalar_field : Real -> Real { source = analytical { |x| 100.0 } }
structure def Holder { param mode_shape : Field<Point3<Length>, Vector3<Length>> }
structure def Root {
    let h = Holder(mode_shape: scalar_field)
}
"#;

/// Clean fixture for the promoted `Field` family.
///
/// BOTH slots of a `Field` erase to the expression compiler's numeric fallback:
/// an analytical `field def` carries `result_type = Field<Real, Real>` however
/// its domain/codomain were declared. Comparing the declaration against that
/// placeholder would be comparing a declaration against a hole, which is exactly
/// why the dedicated arm added for this family is SHAPE-based and is not routed
/// through `type_compatible` (same posture as the adjacent `Type::Vector` arm).
///
/// Before task 5465 this passed via blanket family exclusion; it now passes
/// because the `Type::Field` arm accepts any `Type::Field { .. }` regardless of
/// domain/codomain. This is the same 6-warning shape
/// `examples/fea_shell_channels.ri` exhibited (top / mid / bottom /
/// displacement / stress / frame).
#[test]
fn field_param_given_erased_analytical_field_stays_clean() {
    let module = compile_source_with_stdlib(SRC_FAMILY_FIELD);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a Field<Point3<Length>, Vector3<Length>> param given an analytical field whose \
         result_type erases to Field<Real, Real> must emit ZERO ctor-conformance diagnostics. \
         Got: {diags:#?}"
    );
}

const SRC_FIELD_GIVEN_STRING: &str = r#"module test.field_string
structure def Holder { param mode_shape : Field<Point3<Length>, Vector3<Length>> }
structure def Root {
    let h = Holder(mode_shape: "shape")
}
"#;

/// Value floor for the promoted `Field` family: a `String` at a `Field` slot is
/// not field-shaped by any reading and must warn.
///
/// The arm's OTHER accept — a lambda, i.e. a `Type::Function` arg — is pinned by
/// `field_param_accepts_function_arg` in `conformance/mod.rs`'s own `mod tests`.
#[test]
fn field_param_given_string_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_FIELD_GIVEN_STRING,
        "mode_shape",
        "Field<Point3<Length>, Vector3<Length>> ← String",
    );
}

const SRC_LIST_OF_FIELD_GIVEN_STRING_ELEMENT: &str = r#"module test.list_field_string
structure def Holder { param modes : List<Field<Point3<Length>, Vector3<Length>>> }
structure def Root {
    let h = Holder(modes: ["a"])
}
"#;

/// Wrapper composition: the new `Field` leaf arm must be reachable THROUGH the
/// walker's `ListLiteral` recursion, not only at the top level.
///
/// Without this probe a leaf arm could be added in a position the wrapper
/// recursion never reaches and the top-level probe above would not notice.
#[test]
fn list_of_field_param_given_string_element_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_LIST_OF_FIELD_GIVEN_STRING_ELEMENT,
        "modes",
        "List<Field<…>> ← [String]",
    );
}

// ── promoted family: generic enum / `Applied` (task 5465, family 4) ──────────
//
// Under enum erasure a constructed variant's result_type is always the bare
// `Type::Enum(name)`, never the applied form — which is precisely why
// `enum_payload_compatible` exists in type_compat.rs (its own doc notes that a
// naive `type_compatible` "would spuriously fail") and why variant_construct.rs
// already guards with it. Shape from examples/m6_generic_enum.ri and the
// reify-cli result_match_bore_{ok,err}.ri / result_prelude_pinned_mismatch.ri
// fixtures.
const SRC_FAMILY_GENERIC_ENUM: &str = r#"module test.family_generic_enum
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
structure def Root {
    param r : Result<Length, String> = Ok { value: 12mm }
}
"#;

/// Clean fixture for the promoted enum family.
///
/// The declared param type is the applied form `Type::Applied { name: "Result",
/// args: [Length, String] }`, but the supplied arg's `result_type` under D1 /
/// F-Mono erasure is the BARE `Type::Enum("Result")` — the applied form is never
/// persisted on a value (`type_compat.rs::enum_payload_compatible` doc, §7.1:
/// "resolved args live only in the per-site substitution map"). Raw
/// `type_compatible` therefore has nothing to match on and would spuriously
/// fail.
///
/// Before task 5465 this passed for the WRONG reason — the whole enum family was
/// absent from `general_leaf_param_family_is_validated`, so every enum-typed
/// param fell through silently, including genuine mismatches. It now passes
/// because the general concrete-leaf arm short-circuits on
/// `enum_payload_compatible`, exactly as `variant_construct.rs:325` already
/// does — a targeted erasure tolerance, not a blanket family bypass (probe
/// `enum_param_given_wrong_enum_warns_arg_type_mismatch` below is the fence).
#[test]
fn generic_enum_param_given_erased_variant_stays_clean() {
    let module = compile_source_with_stdlib(SRC_FAMILY_GENERIC_ENUM);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a `Result<Length, String>` param defaulted to `Ok {{ value: 12mm }}` must emit ZERO \
         ctor-conformance diagnostics — enum erasure gives the constructed variant the bare \
         result_type Enum(\"Result\"), never the applied form. Got: {diags:#?}"
    );
}

const SRC_GENERIC_ENUM_GIVEN_STRING: &str = r#"module test.generic_enum_string
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
structure def Root {
    param r : Result<Length, String> = "nope"
}
"#;

/// Value floor for the promoted `Applied`-enum family: a `String` default at a
/// `Result<Length, String>` param is a genuine mismatch and must warn.
///
/// This exercises the param-DEFAULT entry (`check_param_default_conformance`),
/// which is the shape the reify-cli `result_match_bore_ok.ri:10` fixture uses —
/// not the call-site ctor entry. Without it, promoting the family would only be
/// pinned on its tolerance half.
#[test]
fn generic_enum_param_given_string_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_GENERIC_ENUM_GIVEN_STRING,
        "r",
        "Result<Length, String> ← String",
    );
}

// NOTE ON NAMING: the two enums are deliberately spelled `Hue` / `Outline`
// rather than the obvious `Color` / `Shape`. The prelude declares
// `structure def Color` (crates/reify-compiler/stdlib/materials_appearance.ri:17),
// so a local `enum Color` annotation resolves to the prelude's
// `Type::StructureRef("Color")` instead of `Type::Enum("Color")` and the probe
// would exercise the StructureRef arm, not the enum family.
const SRC_ENUM_CROSS_ENUM_MISMATCH: &str = r#"module test.enum_cross_mismatch
enum Hue {
    Red { level: Real },
    Blue { level: Real },
}
enum Outline {
    Round { radius: Length },
    Square { side: Length },
}
structure def Root {
    param c : Hue = Round { radius: 5mm }
}
"#;

/// Fence on the `enum_payload_compatible` short-circuit: it tolerates ONLY the
/// erasure gap (same base name), and still rejects a genuine cross-enum
/// mismatch.
///
/// A `Hue` param defaulted to a constructed `Outline::Round { .. }` variant
/// gives declared `Type::Enum("Hue")` vs supplied `Type::Enum("Outline")` —
/// different base names, so `enum_payload_compatible` returns false and the
/// general concrete-leaf arm's `type_compatible` gate rejects. Without this
/// probe the short-circuit could be widened into a blanket enum bypass without
/// any test noticing.
#[test]
fn enum_param_given_wrong_enum_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(SRC_ENUM_CROSS_ENUM_MISMATCH, "c", "Hue ← Outline");
}

// The REVERSE erasure pairing — a param declared as the BARE `Type::Enum(n)`
// supplied an APPLIED `Type::Applied { name: n, .. }` arg — is pinned by
// `enum_param_accepts_applied_enum_arg_of_same_base` (plus its cross-base and
// applied-generic-STRUCTURE fences) in `conformance/mod.rs`'s own `mod tests`.
// It lives there because which surface spelling survives inference on the arg
// side is not something an inline fixture can pin reliably, and because that
// pairing is reachable at `Severity::Error` through the fn-call entry.



// ── (b) HELD family: dimensioned Scalar ← dimensionless Real ─────────────────
//
// Migrated by task 5758 (β): the args are now dimensioned unit literals, the
// fix form the ruling settled on. Shape from
// examples/trajectory/{printer_print_envelope,tots_optimal_ptp}.ri.
const SRC_FAMILY_DIMENSIONED_SCALAR: &str = r#"module test.family_dim_scalar
structure def Limit {
    param velocity_limit : Scalar<Velocity>
    param acceleration_limit : Scalar<Acceleration>
}
structure def Root {
    let l = Limit(velocity_limit: 300mm/s, acceleration_limit: 5000mm/s^2)
}
"#;

/// A dimensioned unit literal at a dimensioned `Scalar<Q>` ctor field slot emits
/// ZERO ctor-conformance diagnostics — i.e. β's fix form is ACCEPTED.
///
/// **The ruling landed.** This probe was formerly
/// `excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent`, held
/// pending a language-semantics ruling on whether a dimensionless arg is legal
/// at a dimensioned slot. Leo ruled (esc-5758-2, and esc-5758-4 for the
/// printer's deferred pair), recorded in PRD
/// `docs/prds/v0_6/dimensioned-construction-strictness.md` §6.3 / §11 β: fix the
/// call sites. Task 5758 migrated this fixture along with the rest of the
/// corpus, so the probe now pins fix-form ACCEPTANCE rather than the held
/// dimensionless-at-dimensioned question. See the PRD for the contract table;
/// it is not restated here.
///
/// **What this probe pins, precisely.** Zero *ctor-conformance* diagnostics —
/// i.e. exclusion from ONE pass. It does NOT assert that the program is
/// well-typed, and it must not be read as one.
///
/// **The bare-arg negative pin is γ's.** With this fixture migrated, nothing
/// here asserts what happens to a BARE arg at a dimensioned slot. γ introduces
/// that as one of its I1-I8 value floors (PRD §7.1) rather than inverting this
/// probe in place.
#[test]
fn family_dimensioned_scalar_given_unit_literal_arg_is_silent() {
    let module = compile_source_with_stdlib(SRC_FAMILY_DIMENSIONED_SCALAR);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "dimensioned Scalar params given dimensioned unit-literal args must emit ZERO \
         ctor-conformance diagnostics — this is the fix form task 5758 (β) migrated the \
         corpus to. Got: {diags:#?}"
    );
}

// ── (c) α-value-floor guards: the RETAINED families must still warn ──────────

/// Assert `source` emits exactly one ctor-conformance diagnostic, and that it is
/// a `Warning`-severity `ArgTypeMismatch` naming `param_name`.
///
/// Shared by the four value-floor guards below so each stays a one-liner and the
/// four cases cannot drift apart in what they check.
fn assert_single_arg_type_mismatch_warning(source: &str, param_name: &str, label: &str) {
    assert_single_arg_type_mismatch_warning_in(
        &compile_source_with_stdlib(source),
        param_name,
        label,
    );
}

/// `&CompiledModule`-taking half of [`assert_single_arg_type_mismatch_warning`],
/// for a probe that has ALREADY compiled its fixture — e.g. to run the
/// non-vacuity guard `vec3_dimensionless_at_dimensioned_vector_param_stays_clean`
/// documents — so the guard and the assertion share ONE compile of the source
/// plus the whole stdlib instead of doing it twice.
///
/// Returns the diagnostics it filtered, so a caller adding further assertions
/// (the quantity sibling below) does not re-run [`ctor_conformance_diags`] over
/// the module. Matches `assert_quantity_slot_conflict` (`conformance/mod.rs`),
/// its fn-call twin, which returns its `Vec<Diagnostic>` for the same reason.
fn assert_single_arg_type_mismatch_warning_in<'a>(
    module: &'a CompiledModule,
    param_name: &str,
    label: &str,
) -> Vec<&'a Diagnostic> {
    let diags = ctor_conformance_diags(module);
    assert_eq!(
        diags.len(),
        1,
        "{label}: expected exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "{label}: α ctor field conformance is Warning-severity, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{label}: expected ArgTypeMismatch, got: {:?}",
        diags[0].code
    );
    assert!(
        diags[0].message.contains(param_name),
        "{label}: message must name the offending param {param_name:?}, got: {:?}",
        diags[0].message
    );
    diags
}

/// Quantity-rule sibling of [`assert_single_arg_type_mismatch_warning_in`]: the
/// same four checks, PLUS the two fragments that discriminate the quantity-slot
/// emitter from the whole-type `emit_arg_type_mismatch`.
///
/// Without those fragments a quantity fixture is VACUOUS against a family/arity
/// regression. Count, `Severity::Warning`, `ArgTypeMismatch` and "the message
/// names the param" are all satisfied by the whole-type emitter too, so if a
/// shape arm's own family/arity check started rejecting the arg the pre-existing
/// `emit_arg_type_mismatch` would fire instead — same code, same severity, still
/// naming the param — and the fixture would stay GREEN while the rule it exists
/// to pin no longer fired at all. These `.ri` fixtures are the ONLY tests that
/// reach the rule from real source (the `conformance/mod.rs` probes construct the
/// `Type` directly and bypass the `math_fn_result_type` chain), so that is
/// exactly where a masked regression costs the most.
///
/// Arguments run PARAM-then-ARG, matching `assert_quantity_slot_conflict`
/// (`conformance/mod.rs`), which applies these same two fragments on the fn-call
/// (Error) leg. This is its ctor (Warning) twin.
fn assert_single_quantity_conflict_warning_in(
    module: &CompiledModule,
    param_name: &str,
    expected_param_quantity: &str,
    expected_arg_quantity: &str,
    label: &str,
) {
    let diags = assert_single_arg_type_mismatch_warning_in(module, param_name, label);
    let has_quantity = format!("has quantity '{expected_arg_quantity}'");
    let requires_quantity = format!("requires quantity '{expected_param_quantity}'");
    assert!(
        diags[0].message.contains(&has_quantity) && diags[0].message.contains(&requires_quantity),
        "{label}: the diagnostic must come from the QUANTITY-slot rule, naming the ARG's slot \
         after `has quantity` ({has_quantity:?}) and the PARAM's after `requires quantity` \
         ({requires_quantity:?}) — without this the whole-type emit_arg_type_mismatch would \
         satisfy every other assertion here and the fixture would pass while the rule never \
         fired. Got: {:?}",
        diags[0].message
    );
}

const SRC_FLOOR_STRING: &str = r#"module test.floor_string
structure def W { param label : String }
structure def Root { let a = W(label: 42) }
"#;

const SRC_FLOOR_BOOL: &str = r#"module test.floor_bool
structure def W { param flag : Bool }
structure def Root { let a = W(flag: "yes") }
"#;

const SRC_FLOOR_INT: &str = r#"module test.floor_int
structure def W { param n : Int }
structure def Root { let a = W(n: "seven") }
"#;

const SRC_FLOOR_REAL: &str = r#"module test.floor_real
structure def W { param mag : Real }
structure def Root { let a = W(mag: "big") }
"#;

/// Value floor: `String` stays validated. Deliberately a standalone fn rather
/// than a reliance on the step-1 row-2 probe — this group is the regression
/// fence around the step-12 narrowing itself, so it must fail loudly and by name
/// if the allowlist ever loses a family.
#[test]
fn value_floor_string_param_given_int_still_warns() {
    assert_single_arg_type_mismatch_warning(SRC_FLOOR_STRING, "label", "String ← Int");
}

/// Value floor: `Bool` stays validated.
#[test]
fn value_floor_bool_param_given_string_still_warns() {
    assert_single_arg_type_mismatch_warning(SRC_FLOOR_BOOL, "flag", "Bool ← String");
}

/// Value floor: `Int` stays validated.
#[test]
fn value_floor_int_param_given_string_still_warns() {
    assert_single_arg_type_mismatch_warning(SRC_FLOOR_INT, "n", "Int ← String");
}

/// Value floor: dimensionless `Scalar` (spelled `Real`) stays validated. Note
/// this is the DIMENSIONLESS half of the Scalar family only — the dimensioned
/// half is excluded above, and the two must not be conflated.
#[test]
fn value_floor_dimensionless_real_param_given_string_still_warns() {
    assert_single_arg_type_mismatch_warning(SRC_FLOOR_REAL, "mag", "Real ← String");
}

// ═════════════════════════════════════════════════════════════════════════════
// γ (task 5627): value floors for the PROMOTED dimensioned-`Scalar` family
// ═════════════════════════════════════════════════════════════════════════════
//
// γ promotes dimensioned `Type::Scalar` into
// `general_leaf_param_family_is_validated`, so a dimensioned ctor field slot is
// now judged under STRICT `DimensionVector` equality. The contract table is in
// `docs/prds/v0_6/dimensioned-construction-strictness.md` §7.1 (invariants
// I1-I8) and §7.4 (the named B1-B4 signals); it is deliberately NOT restated
// here — these are its executable floors, keyed by invariant id.
//
// Two halves, and the split is load-bearing:
//
// * the REJECTION floors (I2/I3/I4, and the B1/B2 author-side shape) are RED
//   before the promotion — the family is excluded, so every one of them is
//   silent today;
// * the ACCEPTANCE floors (I1/I6/I8/A3/B3) pass BEFORE and AFTER. They are the
//   FALSE-POSITIVE floor: their whole value is that the promotion must not
//   disturb them, which is why they are written before it rather than after.
//
// Every assertion is on `DiagnosticCode` IDENTITY plus `Severity::Warning`,
// never on message prose beyond the param name (D4-8 / INV-SF-6, tasks
// 2255/3416 precedent). There is NO exit-code assertion: γ is pre-δ, so
// `CTOR_FIELD_CONFORMANCE_SEVERITY` is still `Warning` and γ does not touch it.

/// Assert `source` emits ZERO ctor-conformance diagnostics.
///
/// The acceptance-floor counterpart of
/// [`assert_single_arg_type_mismatch_warning`]. Scoped to the ctor-conformance
/// code set only — a fixture may still emit unrelated diagnostics (an
/// `auto`-resolution warning, an unresolved-name Error) without weakening the
/// claim, which is exactly what makes the I6 and A3 floors expressible.
fn assert_no_ctor_conformance_diags(source: &str, label: &str) {
    let module = compile_source_with_stdlib(source);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "{label}: expected ZERO ctor-conformance diagnostics, got: {diags:#?}"
    );
}

// ── REJECTION floors (RED before the promotion) ──────────────────────────────

const SRC_G_I2_CROSS_DIMENSION: &str = r#"module test.g_i2_cross_dimension
structure def W { param p : Scalar<Pressure> }
structure def Root { let a = W(p: 200mm) }
"#;

const SRC_G_I3_BARE_REAL: &str = r#"module test.g_i3_bare_real
structure def W { param p : Scalar<Velocity> }
structure def Root { let a = W(p: 300.0) }
"#;

const SRC_G_I3_BARE_INT: &str = r#"module test.g_i3_bare_int
structure def W { param p : Scalar<Velocity> }
structure def Root { let a = W(p: 300) }
"#;

const SRC_G_I4_STRING: &str = r#"module test.g_i4_string
structure def W { param d : Scalar<Density> }
structure def Root { let a = W(d: "heavy") }
"#;

const SRC_G_I4_BOOL: &str = r#"module test.g_i4_bool
structure def W { param d : Scalar<Density> }
structure def Root { let a = W(d: true) }
"#;

/// I2 — cross-dimension: a `Length` literal at a `Scalar<Pressure>` slot is
/// rejected. This is the strict-`DimensionVector`-equality half of the ruling:
/// both sides are dimensioned scalars, so nothing but the dimension vectors
/// themselves distinguishes them.
#[test]
fn g_i2_cross_dimension_arg_at_dimensioned_slot_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I2_CROSS_DIMENSION,
        "p",
        "I2: Scalar<Pressure> ← Length literal",
    );
}

/// I3 — a BARE dimensionless `Real` at a dimensioned slot is rejected.
///
/// This is the negative pin β's `family_dimensioned_scalar_given_unit_literal_arg_is_silent`
/// deliberately left to γ (PRD §11 γ): with that probe's fixture migrated to
/// unit literals, nothing else asserts what a bare arg does here.
#[test]
fn g_i3_bare_real_arg_at_dimensioned_slot_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I3_BARE_REAL,
        "p",
        "I3: Scalar<Velocity> ← bare Real",
    );
}

/// I3 — the `Int` spelling of the same bare arg is rejected too.
///
/// Also the fence on the D4-5 `ScalarParam` accept: `is_numeric_placeholder_leaf`
/// matches `Int`, so an over-wide fence would make THIS case silent. It must
/// keep failing-then-passing, never become silent.
#[test]
fn g_i3_bare_int_arg_at_dimensioned_slot_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I3_BARE_INT,
        "p",
        "I3: Scalar<Velocity> ← bare Int",
    );
}

/// I4 — a `String` at a dimensioned slot is rejected (family-level mismatch).
#[test]
fn g_i4_string_arg_at_dimensioned_slot_warns() {
    assert_single_arg_type_mismatch_warning(SRC_G_I4_STRING, "d", "I4: Scalar<Density> ← String");
}

/// I4 — a `Bool` at a dimensioned slot is rejected.
#[test]
fn g_i4_bool_arg_at_dimensioned_slot_warns() {
    assert_single_arg_type_mismatch_warning(SRC_G_I4_BOOL, "d", "I4: Scalar<Density> ← Bool");
}

const SRC_G_B1_B2_AUTHOR_SIDE: &str = r#"module test.g_b1_b2
structure def Steel {
    param youngs_modulus : Pressure
    param density : Density
}
structure def Root {
    let s = Steel(youngs_modulus: 200mm, density: "heavy")
}
"#;

/// PRD §7.4 B1/B2 — the combined author-side shape, and the PRD's own named
/// signal for this promotion: ONE structure whose two dimensioned params are
/// both supplied wrongly emits TWO independent `ArgTypeMismatch` warnings, one
/// per site, not one aggregate and not a cascade.
///
/// The identical file is the §6.1 before-image: it emits NOTHING today.
#[test]
fn g_b1_b2_two_wrong_dimensioned_args_warn_once_each() {
    let module = compile_source_with_stdlib(SRC_G_B1_B2_AUTHOR_SIDE);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        2,
        "B1/B2: two wrongly-supplied dimensioned params must emit exactly two \
         ctor-conformance diagnostics, got: {diags:#?}"
    );
    for d in &diags {
        assert_eq!(
            d.severity,
            Severity::Warning,
            "B1/B2: γ ctor field conformance is Warning-severity, got: {d:?}"
        );
        assert_eq!(
            d.code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "B1/B2: expected ArgTypeMismatch, got: {:?}",
            d.code
        );
    }
    for param in ["youngs_modulus", "density"] {
        assert!(
            diags.iter().any(|d| d.message.contains(param)),
            "B1/B2: no diagnostic names param {param:?}; got: {diags:#?}"
        );
    }
}

// ── ACCEPTANCE floors (green BEFORE and AFTER the promotion) ─────────────────

const SRC_G_I1_CLEAN_DIMENSIONED_ARGS: &str = r#"module test.g_i1_clean
structure def Bundle {
    param velocity : Scalar<Velocity>
    param half_span : Length
    param footprint : Area
    param accel : Acceleration
    param forwarded : Length
}
structure def Root {
    param span : Length = 200mm
    let b = Bundle(
        velocity: 300mm/s,
        half_span: span / 2.0,
        footprint: 100mm * 1mm,
        accel: 5.0 * STANDARD_GRAVITY(),
        forwarded: span
    )
}
"#;

/// I1 (§6.6, the highest-risk acceptance floor) — `Scalar<Q> ← Scalar<Q>` is
/// silent across ALL THREE arg shapes together: a unit literal, three
/// arithmetic-DERIVED values (division by a `Real`, a `Length × Length` product
/// promoted to `Area`, and a scaled stdlib constant), and a plain reference to a
/// dimensioned param.
///
/// A failure here is an arg-side INFERENCE bug, not a corpus bug — the strict
/// equality this task installs only holds up if the arg side actually carries
/// the dimension it should. Keeping all four shapes in ONE fixture is
/// deliberate: it is the shape the whole shipped corpus is written in, so a
/// regression in any of them is a corpus-wide regression.
#[test]
fn g_i1_matching_dimensioned_args_are_silent() {
    assert_no_ctor_conformance_diags(
        SRC_G_I1_CLEAN_DIMENSIONED_ARGS,
        "I1: Scalar<Q> ← Scalar<Q> across literal / derived / ref shapes",
    );
}

const SRC_G_I6_ERROR_ARG: &str = r#"module test.g_i6_error_arg
structure def W { param p : Scalar<Velocity> }
structure def Root { let a = W(p: no_such_symbol_anywhere) }
"#;

/// I6 — anti-cascade: an arg whose `result_type` is `Type::Error` emits NO
/// ctor-conformance diagnostic at a dimensioned slot, so the promotion cannot
/// pile a spurious dimension complaint on top of an already-reported
/// root cause. Delivered for free by `reject_if_incompatible`'s
/// `arg_type_is_unverifiable` guard; pinned here so a later refactor cannot
/// route around it.
///
/// The fixture DOES emit an unresolved-name Error; that is the root cause and
/// is not a ctor-conformance code, so it is correctly outside this assertion.
#[test]
fn g_i6_error_typed_arg_at_dimensioned_slot_is_silent() {
    assert_no_ctor_conformance_diags(SRC_G_I6_ERROR_ARG, "I6: Scalar<Velocity> ← Type::Error");
}

const SRC_G_I8_REAL_GIVEN_INT: &str = r#"module test.g_i8_real_int
structure def W { param mag : Real }
structure def Root { let a = W(mag: 7) }
"#;

/// I8 — non-regression on the DIMENSIONLESS half task 5465 already promoted:
/// `Real ← Int` stays silent.
///
/// `type_compatible`'s `Int`→`Scalar` widening (`type_compat.rs:232-237`) is
/// gated on the PARAM side being dimensionless, which is simultaneously why
/// this survives and why I3 (`Scalar<Velocity> ← Int`) rejects. The two are the
/// same code path read from opposite sides, so they are pinned as a pair.
#[test]
fn g_i8_dimensionless_real_given_int_stays_silent() {
    assert_no_ctor_conformance_diags(SRC_G_I8_REAL_GIVEN_INT, "I8: Real ← Int");
}

const SRC_G_A3_AUTO_AND_UNDEF_DEFAULTS: &str = r#"module test.g_a3_defaults
structure def W {
    param strict_slot : Length = auto
    param free_slot : Scalar<Velocity> = auto(free)
    param undef_slot : Scalar<Density> = undef
}
structure def Root { sub w = W() }
"#;

const SRC_G_A3_AUTO_AND_UNDEF_ARGS: &str = r#"module test.g_a3_args
structure def W {
    param strict_slot : Length
    param free_slot : Scalar<Velocity>
    param undef_slot : Scalar<Density>
}
structure def Root {
    sub w = W(strict_slot: auto, free_slot: auto(free), undef_slot: undef)
}
"#;

/// A3 (addendum) — `auto`, `auto(free)` and `undef` at a DIMENSIONED `Scalar`
/// slot are SILENT, at the param-DEFAULT entry into the walker.
///
/// Load-bearing for the corpus gate rather than merely nice to have: the PRD
/// never states this and §6.5 item 1 records that this gate was never measured.
/// The shipped corpus carries 96 such default sites, so if the promotion fired
/// on any of them the gate would go red corpus-wide. (Measured green under the
/// promotion at planning time; pinned here so a future change cannot silently
/// break it.)
#[test]
fn g_a3_auto_and_undef_param_defaults_at_dimensioned_slots_are_silent() {
    assert_no_ctor_conformance_diags(
        SRC_G_A3_AUTO_AND_UNDEF_DEFAULTS,
        "A3: auto / auto(free) / undef at dimensioned param DEFAULTS",
    );
}

/// A3 — the same three placeholders at the ctor-ARG entry.
#[test]
fn g_a3_auto_and_undef_ctor_args_at_dimensioned_slots_are_silent() {
    assert_no_ctor_conformance_diags(
        SRC_G_A3_AUTO_AND_UNDEF_ARGS,
        "A3: auto / auto(free) / undef at dimensioned ctor ARGS",
    );
}

const SRC_G_B3_DENSITY_LITERAL: &str = r#"module test.g_b3_density
structure def W { param d : Density }
structure def Root { let a = W(d: 7850kg/m^3) }
"#;

/// PRD §7.4 B3 — the migrated fix form for a COMPOUND dimension: a
/// `7850kg/m^3` literal at a `Density` slot is silent. This is also the literal
/// γ's own migration hint offers for `Density` (step-4), so the accepted fix
/// form and the suggested fix form are pinned to be the same thing.
#[test]
fn g_b3_compound_unit_literal_at_density_slot_is_silent() {
    assert_no_ctor_conformance_diags(SRC_G_B3_DENSITY_LITERAL, "B3: Density ← 7850kg/m^3");
}

// ═════════════════════════════════════════════════════════════════════════════
// γ (task 5627): the D4-6 / I7 migration hint
// ═════════════════════════════════════════════════════════════════════════════
//
// A rejection at a DIMENSIONED `Scalar` slot carries a hint naming the expected
// dimension and an example literal, mirroring `ArgRejection::message`'s shape in
// `crates/reify-eval/src/arg_acceptance.rs` so the compile-time and runtime
// diagnostics read consistently.
//
// Assertions are on the STABLE, semantic parts only — the fixed clause prefix
// and the dimension's own `canonical_name()`, DERIVED by calling it rather than
// hard-coded, so they track the `NAMED_DIMENSIONS` registry instead of pinning a
// literal string.

/// The invariant part of the migration-hint clause.
const HINT_CLAUSE_PREFIX: &str = "pass a dimensioned ";

/// Separator introducing the example literal, which is then backtick-delimited.
const HINT_EXAMPLE_INTRO: &str = "such as `";

/// The `DimensionVector` that reify's `<type_name>` annotation resolves to.
///
/// Looked up in `NAMED_DIMENSIONS` — the same registry both the name→dimension
/// resolution and `canonical_name`'s dimension→name scan use — rather than via a
/// `DimensionVector::<CONST>`, so an alias row (`Momentum` → the `Impulse`
/// vector) resolves exactly as the compiler resolves it.
fn dimension_named(type_name: &str) -> DimensionVector {
    NAMED_DIMENSIONS
        .iter()
        .find(|(_, name)| *name == type_name)
        .unwrap_or_else(|| panic!("no NAMED_DIMENSIONS row is named {type_name:?}"))
        .0
}

/// The single ctor-conformance diagnostic message `source` emits.
fn sole_ctor_conformance_message(source: &str, label: &str) -> String {
    let module = compile_source_with_stdlib(source);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "{label}: expected exactly one ctor-conformance diagnostic, got: {diags:#?}"
    );
    diags[0].message.clone()
}

/// Extract the example literal from a hint's ``such as `…` `` tail.
///
/// `None` when the hint carries no example — the deliberate escape hatch for a
/// dimension whose exponents have no clean literal spelling.
fn example_literal_from_hint(message: &str) -> Option<String> {
    let after = message.split(HINT_EXAMPLE_INTRO).nth(1)?;
    let end = after.find('`')?;
    Some(after[..end].to_owned())
}

/// Whether `dim` has no clean unit-literal spelling, and so is legitimately
/// exempt from offering an example.
///
/// Exactly three reasons, each independently derivable from the vector and the
/// stdlib's own unit table — this predicate does NOT consult the compiler's
/// derivation, so it is a real oracle rather than a restatement of it:
///
/// * a FRACTIONAL exponent (`den() != 1`, e.g. `FractureToughness`'s Pa·m^0.5)
///   cannot be written as a unit literal at all;
/// * an entirely NEGATIVE exponent set (e.g. `Frequency`, s⁻¹) leaves no
///   numerator term to anchor the literal. Measured: `param x : Frequency = 1/s`
///   fails with `unresolved name: s` — reify reads the bare `1` as a number and
///   `/s` as a division, not as a quantity literal;
/// * a slot whose base-unit symbol `stdlib/units.ri` does not declare as a unit.
///   [`BASE_UNIT_SYMBOLS`] is the DISPLAY table; being renderable is weaker than
///   being parseable. Measured: `param x : Voltage = 1m^2*kg/s^3/A` fails with
///   `unknown unit: A`.
///
/// [`SPELLABLE_BASE_UNITS`] is this test's own reading of the stdlib, kept
/// deliberately separate from the compiler's. Drift in EITHER direction is
/// caught: if the stdlib gains a unit the compiler still declines, the exemption
/// assert fires; if the compiler offers a literal the stdlib cannot parse, the
/// round-trip half fails.
///
/// Anything else declining to offer an example is a derivation bug, and the rot
/// guard says so by name.
fn has_no_clean_unit_literal(dim: &DimensionVector) -> bool {
    dim.0.iter().enumerate().any(|(slot, r)| {
        r.num() != 0 && (r.den() != 1 || !SPELLABLE_BASE_UNITS.contains(&BASE_UNIT_SYMBOLS[slot]))
    }) || !dim.0.iter().any(|r| r.num() > 0)
}

/// The base-unit symbols `crates/reify-compiler/stdlib/units.ri` declares as
/// units, and which can therefore appear in a unit literal.
///
/// `A`, `mol`, `cd` and `sr` are deliberately absent: they name
/// `DimensionVector` slots but the stdlib declares no unit for them.
const SPELLABLE_BASE_UNITS: [&str; 6] = ["m", "kg", "s", "K", "rad", "USD"];

/// Assert `source`'s sole rejection carries the hint AND names the dimension
/// `type_name` resolves to, using that dimension's own `canonical_name()`.
fn assert_hint_names_dimension(source: &str, type_name: &str, label: &str) {
    let message = sole_ctor_conformance_message(source, label);
    assert!(
        message.contains(HINT_CLAUSE_PREFIX),
        "{label}: rejection at a dimensioned slot must carry the migration hint \
         ({HINT_CLAUSE_PREFIX:?}); got: {message:?}"
    );
    let expected = dimension_named(type_name)
        .canonical_name()
        .unwrap_or_else(|| panic!("{label}: {type_name:?} has no canonical_name()"));
    assert!(
        message.contains(expected),
        "{label}: hint must name the expected dimension {expected:?}; got: {message:?}"
    );
}

/// Assert `source`'s sole rejection carries NO migration hint.
fn assert_no_hint(source: &str, label: &str) {
    let message = sole_ctor_conformance_message(source, label);
    assert!(
        !message.contains(HINT_CLAUSE_PREFIX),
        "{label}: this family must keep its message byte-identical — no migration \
         hint; got: {message:?}"
    );
}

/// I7 — every rejection at a dimensioned slot carries the hint, and the hint
/// names that slot's dimension.
///
/// One test over all five of the step-1 rejection fixtures rather than five
/// one-liners: I7 is a property of the SLOT, not of the arg that missed it, so
/// the arg shape (cross-dimension literal / bare Real / bare Int / String /
/// Bool) must not change the outcome — which is only visible when they are
/// asserted together.
#[test]
fn g_i7_rejections_at_dimensioned_slots_carry_the_migration_hint() {
    for (source, type_name, label) in [
        (SRC_G_I2_CROSS_DIMENSION, "Pressure", "I7: I2 cross-dimension"),
        (SRC_G_I3_BARE_REAL, "Velocity", "I7: I3 bare Real"),
        (SRC_G_I3_BARE_INT, "Velocity", "I7: I3 bare Int"),
        (SRC_G_I4_STRING, "Density", "I7: I4 String"),
        (SRC_G_I4_BOOL, "Density", "I7: I4 Bool"),
    ] {
        assert_hint_names_dimension(source, type_name, label);
    }
}

/// Hint SCOPE fence — the four families task 5465 already promoted keep their
/// messages byte-identical.
///
/// `emit_arg_type_mismatch` is shared, so an unconditional append would produce
/// nonsense ("pass a dimensioned Bool literal") and would silently change the
/// user-visible wording of four already-shipped diagnostics γ has no mandate to
/// touch. This is what keeps the hint strictly ADDITIVE to the family γ
/// promotes, and it is why the four `value_floor_*_still_warns` guards above
/// remain meaningful as untouched regression fences.
#[test]
fn g_i7_hint_is_absent_for_the_already_promoted_families() {
    assert_no_hint(SRC_FLOOR_STRING, "hint scope: String ← Int");
    assert_no_hint(SRC_FLOOR_BOOL, "hint scope: Bool ← String");
    assert_no_hint(SRC_FLOOR_INT, "hint scope: Int ← String");
    assert_no_hint(SRC_FLOOR_REAL, "hint scope: dimensionless Real ← String");
}

/// ROT GUARD (D4-6) — for EVERY row of `NAMED_DIMENSIONS`, the example literal
/// the hint offers must round-trip: parse as a reify unit literal and resolve
/// back to exactly the dimension it was derived from.
///
/// This is what makes "derived from the registry so it cannot rot" STRUCTURAL
/// rather than merely asserted. A new `NAMED_DIMENSIONS` row whose derived
/// example does not parse fails the build; so does a derivation that composes a
/// literal reify's compound-unit grammar does not accept.
///
/// The literal is read back out of the real diagnostic, and round-tripped
/// through `common::stdlib_param_si_value` — the same oracle
/// `compound_unit_resolution_tests.rs` uses, which drives the compiler's actual
/// unit-resolution path. Nothing here re-implements the derivation, so the test
/// cannot agree with a broken implementation by construction.
///
/// Rows with no clean literal spelling (see [`has_no_clean_unit_literal`]) are
/// exempt BY OFFERING NO EXAMPLE. The exemption is asserted to be exercised —
/// as an absent example, never as a broken string — so the escape hatch stays
/// visible rather than becoming a silent catch-all.
#[test]
fn g_migration_hint_example_round_trips_for_every_named_dimension() {
    let mut exempt: Vec<&str> = Vec::new();

    for (index, (dim, type_name)) in NAMED_DIMENSIONS.iter().enumerate() {
        let source = format!(
            "module test.rot_{index}\n\
             structure def W {{ param p : {type_name} }}\n\
             structure def Root {{ let a = W(p: \"not a quantity\") }}\n"
        );
        let label = format!("rot guard: {type_name}");
        let message = sole_ctor_conformance_message(&source, &label);

        assert!(
            message.contains(HINT_CLAUSE_PREFIX),
            "{label}: every NAMED_DIMENSIONS row is dimensioned, so its rejection must \
             carry the hint; got: {message:?}"
        );

        let Some(example) = example_literal_from_hint(&message) else {
            assert!(
                has_no_clean_unit_literal(dim),
                "{label}: only a dimension with no clean unit-literal spelling may decline \
                 to offer an example, but {dim} has one; got: {message:?}"
            );
            exempt.push(type_name);
            continue;
        };

        let (_si_value, resolved) = common::stdlib_param_si_value(type_name, &example);
        assert_eq!(
            resolved, *dim,
            "{label}: example literal `{example}` resolves to {resolved}, not to the \
             dimension it was derived from ({dim})"
        );
    }

    assert!(
        !exempt.is_empty(),
        "no NAMED_DIMENSIONS row exercised the fractional-exponent exemption — either the \
         registry lost its fractional dimensions (then delete the escape hatch) or the \
         derivation stopped declining them (then it is inventing literals that cannot \
         parse)"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// γ (task 5627): the D4-5 `ScalarParam` fence
// ═════════════════════════════════════════════════════════════════════════════
//
// `Type::ScalarParam(Q)` is the UNRESOLVED-DIMENSION placeholder a dim-kinded
// generic produces (`fn fwd<Q: Dimension>(x: Scalar<Q>)`, resolved in
// `type_resolution.rs`; displayed `Scalar<Q>`). It is a scalar-FAMILY leaf whose
// dimension alone is open — not an unknown type — so at a `Scalar` slot there is
// no dimension to compare and the real conformance check belongs to
// instantiation, not to this walker. γ's promotion makes such an arg newly
// REJECTED, which is a false positive; D4-5 fences it back out.
//
// Every fixture below is the SAME shape modulo the forwarded arg's declared
// type, so the only thing that can vary the outcome is the arg type itself.
// That is what makes the narrowness fences bite: an over-wide fence reached for
// via `is_numeric_placeholder_leaf` (which also matches `Int` and any concrete
// `Scalar { .. }`) leaves the RED pair green while breaking these by name.

const SRC_G_I5_DIMENSIONED_GIVEN_SCALARPARAM: &str = r#"module test.g_i5_scalarparam
structure def W { param len : Scalar<Length> }
fn fwd<Q: Dimension>(x: Scalar<Q>) -> W { W(len: x) }
"#;

const SRC_G_A2_REAL_GIVEN_SCALARPARAM: &str = r#"module test.g_a2_real_scalarparam
structure def W { param mag : Real }
fn fwd<Q: Dimension>(x: Scalar<Q>) -> W { W(mag: x) }
"#;

const SRC_G_I5_STRING_GIVEN_SCALARPARAM: &str = r#"module test.g_i5_string_scalarparam
structure def W { param label : String }
fn fwd<Q: Dimension>(x: Scalar<Q>) -> W { W(label: x) }
"#;

const SRC_G_I5_BOOL_GIVEN_SCALARPARAM: &str = r#"module test.g_i5_bool_scalarparam
structure def W { param flag : Bool }
fn fwd<Q: Dimension>(x: Scalar<Q>) -> W { W(flag: x) }
"#;

const SRC_G_I5_DIMENSIONED_GIVEN_CROSS_DIMENSION: &str = r#"module test.g_i5_cross_dimension
structure def W { param len : Scalar<Length> }
fn fwd(m: Scalar<Mass>) -> W { W(len: m) }
"#;

const SRC_G_I5_DIMENSIONED_GIVEN_INT: &str = r#"module test.g_i5_int
structure def W { param len : Scalar<Length> }
fn fwd(n: Int) -> W { W(len: n) }
"#;

/// I5 / PRD §7.4 B4 — a `Type::ScalarParam(_)` arg at a DIMENSIONED `Scalar`
/// slot is SILENT.
///
/// A dim-kinded generic forwarding its own `Scalar<Q>` param into a concrete
/// `Scalar<Length>` ctor field has no dimension to compare: `Q` is bound at
/// instantiation, and whether the binding conforms is decided there. Judging the
/// UNINSTANTIATED body under strict `DimensionVector` equality can only ever
/// reject, so every such site would be a false positive.
///
/// RED both before γ's promotion is fenced and — importantly — after step-2
/// ALONE: the promotion is precisely what makes this newly fire, so this floor
/// is not a pre-existing gap γ inherited but one γ itself opens and must close.
#[test]
fn g_i5_scalarparam_arg_at_dimensioned_slot_is_silent() {
    assert_no_ctor_conformance_diags(
        SRC_G_I5_DIMENSIONED_GIVEN_SCALARPARAM,
        "I5: Scalar<Length> ← ScalarParam(Q)",
    );
}

/// A2 — the DECLARED, INTENDED side effect on the dimensionless half.
///
/// Pinned here rather than discovered later. Before this fence,
/// `param mag : Real` given a `Scalar<Q>` arg emitted
/// `argument 'mag' has type 'Scalar<Q>' but param 'mag' requires type 'Real'` —
/// a diagnostic belonging to task 5465's already-shipped DIMENSIONLESS family,
/// not to the family γ promotes. Because the fence lands on the SHARED
/// general-leaf arm, it necessarily silences that too.
///
/// That is a real behaviour change to a shipped diagnostic, and it is
/// INTENDED: the argument for silence is identical in both halves — `Q` is
/// unbound, so there is nothing to compare, and the uninstantiated body can
/// only ever be rejected. Splitting the fence to preserve the dimensionless
/// warning would mean asserting that `Scalar<Q>` is definitely-not-`Real`
/// while simultaneously accepting it as maybe-`Scalar<Length>`, which is
/// incoherent. Recording the post-state here makes it a decision on the record
/// rather than a silent regression.
#[test]
fn g_a2_dimensionless_real_given_scalarparam_becomes_silent() {
    assert_no_ctor_conformance_diags(SRC_G_A2_REAL_GIVEN_SCALARPARAM, "A2: Real ← ScalarParam(Q)");
}

/// FENCE — the accept is not a blanket: `String ← ScalarParam(Q)` STILL fires.
///
/// D4-5's own requirement. This half is already correct today and must survive:
/// it is the reason the fix is a narrow PER-ARM guard rather than a widening of
/// `arg_type_is_unverifiable`, which would silence `String ← Scalar<Q>` at every
/// arm at once — the exact outcome that predicate's doc comment already forbids.
#[test]
fn g_i5_string_slot_given_scalarparam_still_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I5_STRING_GIVEN_SCALARPARAM,
        "label",
        "I5 fence: String ← ScalarParam(Q)",
    );
}

/// FENCE — `Bool ← ScalarParam(Q)` STILL fires, for the same reason.
#[test]
fn g_i5_bool_slot_given_scalarparam_still_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I5_BOOL_GIVEN_SCALARPARAM,
        "flag",
        "I5 fence: Bool ← ScalarParam(Q)",
    );
}

/// NARROWNESS FENCE vs I2 — a CONCRETE cross-dimension arg is not a
/// placeholder, so `Scalar<Length> ← Scalar<Mass>` STILL fires.
///
/// Deliberately restated here, adjacent to the fence and in the fence's own
/// fn-forwarding shape, even though `g_i2_cross_dimension_arg_at_dimensioned_slot_warns`
/// covers the invariant: `is_numeric_placeholder_leaf` matches any concrete
/// `Scalar { .. }`, so reaching for it as the arg-side accept would make THIS
/// case silent. Failing by a name that says `i5` points at the fence rather than
/// at the promotion.
#[test]
fn g_i5_dimensioned_slot_given_concrete_cross_dimension_still_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I5_DIMENSIONED_GIVEN_CROSS_DIMENSION,
        "len",
        "I5 fence: Scalar<Length> ← Scalar<Mass>",
    );
}

/// NARROWNESS FENCE vs I3 — `Scalar<Length> ← Int` STILL fires.
///
/// The other half of `is_numeric_placeholder_leaf`'s membership set, and the
/// other rejection γ exists to produce. Same restatement rationale as above.
#[test]
fn g_i5_dimensioned_slot_given_int_still_warns() {
    assert_single_arg_type_mismatch_warning(
        SRC_G_I5_DIMENSIONED_GIVEN_INT,
        "len",
        "I5 fence: Scalar<Length> ← Int",
    );
}

// ===========================================================================
// Quantity-slot dimension semantics (task 5766) — `Vector` family, `.ri` level
// ===========================================================================
//
// Task 5766 rules the quantity slot of `Vector`/`Point`/`Matrix`/`Tensor` to be
// dimension-checked under **dimensionless-tolerant strict equality**: reject iff
// BOTH sides name a concrete dimension and they disagree. `Field` is HELD LOOSE
// by decision (it has no quantity slot — see `field_hold_*` below and the
// normative block in `crates/reify-core/src/ty.rs`).
//
// The four tests below pin the whole boundary of the `Vector` half in one place:
// one REJECT leg (the tightening) and three legs that must stay GREEN both
// before and after it.

const SRC_VEC3_CROSS_DIMENSION: &str = r#"module test.vec3_cross_dimension
structure def Joint { param axis : Vector3<Length> }
structure def Root {
    let j = Joint(axis: vec3(1kg, 0kg, 0kg))
}
"#;

/// THE TIGHTENING (task 5766, `Vector` family): a `vec3` whose components carry
/// a dimension that DISAGREES with the param's quantity slot is now rejected.
///
/// `vec3(1kg, 0kg, 0kg)` compiles to `Vector3<Scalar[kg]>`; the param declares
/// `Vector3<Scalar[m]>`. Both sides name a concrete — and different — dimension,
/// so this is the one case decidable from the type alone with no risk of
/// comparing a declaration against a hole.
///
/// Routed through `emit_arg_type_mismatch` (→ `ArgTypeMismatch`), NOT
/// `emit_vector_mismatch` (→ `TypeNotConformingToVector`), which keeps that
/// bespoke code owning ARITY failures only — see
/// `vector_string_still_rejected_family_before_quantity` below.
#[test]
fn vec3_cross_dimension_at_dimensioned_vector_param_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_VEC3_CROSS_DIMENSION,
        "axis",
        "Vector3<Length> ← Vector3<Mass>",
    );
}

const SRC_VEC3_DIMENSIONLESS: &str = r#"module test.vec3_dimensionless
structure def Joint { param axis : Vector3<Length> }
structure def Root {
    let j = Joint(axis: vec3(0, 0, 1))
}
"#;

/// FENCE (a) — the DIMENSIONLESS leg the ruling deliberately keeps loose.
///
/// This is the reason the rule is dimensionless-TOLERANT rather than strict
/// `DimensionVector` equality: `examples/dynamics/pendulum_idyn.ri:32` spells a
/// joint axis `vec3(0, 1, 0)` into `Revolute.axis : Vec3<Length>`, and the ty.rs
/// "Point / Vector quantity-slot convention" records that
/// `Value::Vector::infer_type()` may yield a dimensionless (or `Int`) quantity.
/// Green BEFORE and AFTER the tightening; its unit-level sibling is
/// `vector_param_accepts_dimensionless_vector_arg` in `conformance/mod.rs`.
#[test]
fn vec3_dimensionless_at_dimensioned_vector_param_stays_clean() {
    let module = compile_source_with_stdlib(SRC_VEC3_DIMENSIONLESS);
    // Non-vacuity guard: `ctor_conformance_diags` filters to the five
    // ctor-conformance codes, so a compile-layer error in the fixture would leave
    // it empty and pass this fence for the wrong reason.
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a dimensionless vec3(0, 0, 1) at a Vector3<Length> param must stay SILENT — the \
         quantity rule is dimensionless-tolerant by decision (task 5766), because the arg \
         side is systematically erased at this arm and pendulum_idyn.ri:32 relies on this \
         spelling. Got: {diags:#?}"
    );
}

const SRC_VEC3_MATCHING_DIMENSION: &str = r#"module test.vec3_matching_dimension
structure def Joint { param axis : Vector3<Length> }
structure def Root {
    let j = Joint(axis: vec3(0mm, 0mm, 1mm))
}
"#;

/// FENCE (b) — dimensions AGREE, so silent. `stdlib/fdm.ri:112`'s shape.
///
/// Guards against a tightening that compares something other than the dimension
/// (e.g. the rendered unit or the `Type` by structural equality): `mm` and the
/// param's `Length` are the same `DimensionVector`, and `DimensionVector`'s
/// derived `PartialEq` is what the rule uses.
#[test]
fn vec3_matching_dimension_at_dimensioned_vector_param_stays_clean() {
    let module = compile_source_with_stdlib(SRC_VEC3_MATCHING_DIMENSION);
    // Non-vacuity guard — see `vec3_dimensionless_at_dimensioned_vector_param_stays_clean`.
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "vec3(0mm, 0mm, 1mm) at a Vector3<Length> param must stay SILENT — the dimensions \
         AGREE (mm and Length are the same DimensionVector). Got: {diags:#?}"
    );
}

const SRC_VEC3_DIMENSIONED_AT_DIMENSIONLESS: &str = r#"module test.vec3_dimensioned_at_dimensionless
structure def Frame { param dir : Vector3<Dimensionless> }
structure def Root {
    let f = Frame(dir: vec3(1m, 0m, 0m))
}
"#;

/// THE PARAM-SIDE RULING (task 6159), `.ri`/ctor seam: a concretely-dimensioned
/// `vec3` at a `Vector3<Dimensionless>` param is REJECTED.
///
/// The ctor-path (Warning) twin of `conformance/mod.rs`'s
/// `dimensionless_quantity_param_rejects_dimensioned_vector_arg` (fn-call path,
/// Error). Routing through [`assert_single_quantity_conflict_warning_in`] pins
/// code + Warning severity + param name for this cell from birth — so the rule's
/// severity split cannot drift silently at the new leg — PLUS the two quantity
/// fragments, without which none of the other four checks could tell this
/// emitter from the whole-type `emit_arg_type_mismatch`.
///
/// This INVERTS task 5766's symmetric tolerance on the param side only. Its
/// enabling premise is task 5848's landed ruling that direction/axis fields are
/// `Vec3<Dimensionless>` — `stdlib/constitutive.ri`'s `x_axis`/`y_axis`/`z_axis`,
/// `stdlib/kinematic.ri`'s `axis`, `stdlib/fea_multi_case.ri`'s `direction` —
/// which makes that spelling an assertion of unit-lessness rather than the
/// grammar workaround `constitutive.ri` once called it. Basis and asymmetry:
/// `crates/reify-core/src/ty.rs`.
#[test]
fn vec3_dimensioned_at_dimensionless_vector_param_warns_arg_type_mismatch() {
    // Non-vacuity guard — see `vec3_dimensionless_at_dimensioned_vector_param_stays_clean`.
    // Without it a fixture that failed to resolve `Vector3<Dimensionless>` would
    // emit zero ctor-conformance diagnostics and read as a RULE failure.
    let module = compile_source_with_stdlib(SRC_VEC3_DIMENSIONED_AT_DIMENSIONLESS);
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    // The `_in` variant so the guard above and the assertion share that one
    // compile, as the two sibling fences directly above do.
    assert_single_quantity_conflict_warning_in(
        &module,
        "dir",
        "Real",
        "Scalar[m]",
        "Vector3<Dimensionless> ← Vector3<Length>",
    );
}

const SRC_VEC3_DIMENSIONED_OFF_FIRST_AT_DIMENSIONLESS: &str = r#"module test.vec3_dimensioned_off_first_at_dimensionless
structure def Frame { param dir : Vector3<Dimensionless> }
structure def Root {
    let f = Frame(dir: vec3(0, 1m, 0))
}
"#;

/// THE ACCEPTED RESIDUAL of the param-side ruling, pinned as an accepted state
/// rather than left unnoticed: move the dimensioned component OFF index `[0]`
/// and the very same rejection goes SILENT.
///
/// The one-token-different twin of
/// [`vec3_dimensioned_at_dimensionless_vector_param_warns_arg_type_mismatch`]
/// directly above — same `Frame`, same `Vector3<Dimensionless>` param, same
/// three components, only the ORDER differs. That twin is therefore this
/// fixture's non-vacuity proof and no separate one is needed: the param
/// spelling demonstrably resolves and rejects, so silence HERE can come only
/// from `math_fn_result_type`'s collapsed `"vec3" | "vec2" | "point3" |
/// "point2"` arm (`crates/reify-compiler/src/math_signatures.rs`) taking the
/// whole vector's quantity from component `[0]` alone.
///
/// Worth pinning because the param-side tightening made this false negative
/// USER-VISIBLE where it had been cosmetic: before the tightening no
/// `Dimensionless` param could reject at all, so component order changed
/// nothing. The two directions are now recorded together, which is what stops a
/// one-sided change to task 5889's inference from moving one and not the other
/// without a test noticing.
///
/// Task 5889 owns that inference (its scope covers this inline arm alongside
/// `list_shape` / `matrix_shape`). When it lands, this fixture and the
/// `matrix` sibling
/// [`matrix_builtin_dimensioned_cell_at_dimensionless_matrix_param_warns_arg_type_mismatch`]
/// must be re-read as a PAIR in that same commit, because they move in opposite
/// directions and which way depends on the fix chosen: degrading a
/// heterogeneous literal to `Type::dimensionless_scalar()` flips the `matrix`
/// sibling to CLEAN and leaves this one clean, whereas comparing EVERY
/// component flips this one to a Warning and leaves the sibling warning.
/// Neither is allowed to move silently.
#[test]
fn vec3_dimensioned_off_first_component_at_dimensionless_vector_param_stays_clean() {
    let module = compile_source_with_stdlib(SRC_VEC3_DIMENSIONED_OFF_FIRST_AT_DIMENSIONLESS);
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "vec3(0, 1m, 0) at a Vector3<Dimensionless> param stays SILENT today. This is a \
         KNOWN false negative owned by task 5889, NOT a design choice: the arg's quantity \
         slot is taken from component [0] alone, so moving the `1m` to component [0] \
         rejects (the twin directly above). If this now fires, 5889 (or an equivalent \
         change) has landed — retarget BOTH this fixture and the matrix sibling together. \
         Got: {diags:#?}"
    );
}

const SRC_POINT3_DIMENSIONED_AT_DIMENSIONLESS: &str = r#"module test.point3_dimensioned_at_dimensionless
structure def Origin { param origin : Point3<Dimensionless> }
structure def Root {
    let o = Origin(origin: point3(1m, 0m, 0m))
}
"#;

const SRC_POINT3_DIMENSIONED_AT_REAL: &str = r#"module test.point3_dimensioned_at_real
structure def Origin { param origin : Point3<Real> }
structure def Root {
    let o = Origin(origin: point3(1m, 0m, 0m))
}
"#;

/// THE PARAM-SIDE RULING (task 6159) at the `Point` arm, `.ri`/ctor seam — the
/// third arm's twin of
/// `vec3_dimensioned_at_dimensionless_vector_param_warns_arg_type_mismatch`
/// (`Vector`) and
/// `matrix_builtin_dimensioned_cell_at_dimensionless_matrix_param_warns_arg_type_mismatch`
/// (`Matrix`/`Tensor`).
///
/// `crates/reify-core/src/ty.rs` asserts a MEASURED end-to-end result for exactly
/// this cell — `point3(1m, 0m, 0m)` at `Point3<Dimensionless>` emits ONE
/// `ArgTypeMismatch` requiring quantity `Real` — and until this fixture nothing
/// pinned it: the `Point` arm had only the direct-`Type` probe
/// `dimensionless_quantity_point_param_rejects_dimensioned_point_arg`
/// (`conformance/mod.rs`), which constructs the `Type::Point` itself and so
/// BYPASSES the whole inference chain. That chain is what makes the claim true:
/// task 5344 (`3c4ee5e9ac`) claimed `point3` / `point2` into the math
/// construction family, so `math_fn_result_type`'s collapsed
/// `"vec3" | "vec2" | "point3" | "point2"` arm now returns a real
/// `Type::Point { n, quantity }` with the quantity taken from the FIRST argument.
/// The same reasoning the `Matrix` fixture states about `matrix_shape` applies
/// here, one arm over.
///
/// It also retires, by demonstration, the premise that "no `.ri` source can
/// produce a dimensioned `Type::Point` arg" — expired since 5344 landed, still
/// written at the sites task 6436 owns.
///
/// **Scope fence.** This pins the cell task 6159 itself ruled and measured.
/// Converting the pre-existing `Point`-arm probes (task 5465's) to `.ri`
/// fixtures, and reconciling the stale erasure rationales around them, stays
/// task 6436's.
#[test]
fn point3_dimensioned_at_dimensionless_point_param_warns_arg_type_mismatch() {
    // Non-vacuity guard — see `vec3_dimensionless_at_dimensioned_vector_param_stays_clean`.
    // Load-bearing twice over: a `Point3<Dimensionless>` that failed to resolve,
    // or a `point3(…)` call that failed to compile, would emit zero
    // ctor-conformance diagnostics and read as a RULE failure.
    let module = compile_source_with_stdlib(SRC_POINT3_DIMENSIONED_AT_DIMENSIONLESS);
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    // The `_in` variant so the guard above and the assertion share that one
    // compile of the source plus the whole stdlib.
    assert_single_quantity_conflict_warning_in(
        &module,
        "origin",
        "Real",
        "Scalar[m]",
        "Point3<Dimensionless> ← Point3<Length> (from component [0] alone)",
    );
}

/// `Real` is the SAME CELL as `Dimensionless` — the same fixture, spelled the
/// other way, end to end.
///
/// `crates/reify-core/src/ty.rs` rules the two exact synonyms at every route into
/// a quantity slot and measures this cell "identically at `Point3<Real>`, the
/// `fdm_slice.ri` spelling". That is a claim about `resolve_type_name` and the
/// dimension-EXPRESSION route in `type_resolution.rs`, not about the conformance
/// rule, so only a second fixture can hold it: were the `Real` spelling ever to
/// resolve to something other than `Type::Scalar { dimension: DIMENSIONLESS }`,
/// the sibling above would stay green while
/// `stdlib/fdm_slice.ri:43`'s `List<Point3<Real>>` silently left the ruling.
#[test]
fn point3_dimensioned_at_real_point_param_warns_arg_type_mismatch() {
    let module = compile_source_with_stdlib(SRC_POINT3_DIMENSIONED_AT_REAL);
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    assert_single_quantity_conflict_warning_in(
        &module,
        "origin",
        "Real",
        "Scalar[m]",
        "Point3<Real> ← Point3<Length> — Real and Dimensionless are the same cell",
    );
}

const SRC_VECTOR_GIVEN_STRING: &str = r#"module test.vector_string
structure def Joint { param axis : Vector3<Length> }
structure def Root {
    let j = Joint(axis: "z")
}
"#;

/// FENCE (c) — the unknown-ness fence's REJECT leg: FAMILY is decided BEFORE
/// quantity, so a `String` at a `Vector3<Length>` param is still rejected.
///
/// Pins the ordering the ruling depends on. The quantity check is applied only
/// AFTER the existing family/arity check passes and only to args that actually
/// carry a quantity slot, so it can neither silence this rejection nor be
/// reached by it. The code stays `TypeNotConformingToVector` — the `Vector`
/// arm's bespoke family/arity code — which is what leaves `ArgTypeMismatch`
/// free to mean "quantity conflict" at this arm.
#[test]
fn vector_string_still_rejected_family_before_quantity() {
    let module = compile_source_with_stdlib(SRC_VECTOR_GIVEN_STRING);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "a String at a Vector3<Length> param must still be rejected — family is decided \
         before the quantity slot is consulted. Got: {diags:#?}"
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::TypeNotConformingToVector),
        "the Vector arm's FAMILY/ARITY rejection keeps its bespoke code; only a QUANTITY \
         conflict routes to ArgTypeMismatch (task 5766). Got: {:?}",
        diags[0].code
    );
}

// ===========================================================================
// Quantity-slot dimension semantics (task 5766) — `Matrix`/`Tensor` family
// ===========================================================================

const SRC_MATRIX_CROSS_DIMENSION: &str = r#"module test.matrix_cross_dimension
structure def Body { param inertia : Matrix<3, 3, MomentOfInertia> }
structure def Root {
    let b = Body(inertia: matrix([[1mm, 0mm, 0mm], [0mm, 1mm, 0mm], [0mm, 0mm, 1mm]]))
}
"#;

/// THE TIGHTENING (task 5766, `Matrix`/`Tensor` family), on the task's own named
/// example type.
///
/// `math_fn_result_type("matrix", …)` takes the quantity from the first element,
/// so `matrix([[1mm, …], …])` compiles to `Tensor2x3<Scalar[m]>` while the param
/// declares `Matrix3x3<Scalar[m^2·kg]>`. Both sides name a concrete — and
/// different — dimension, so the conflict is decidable from the type alone.
///
/// Also the empirical proof that `MomentOfInertia` in a `Matrix<3,3,…>` slot
/// resolves through `resolve_type_expr_with_aliases` all the way to
/// `Type::Scalar { dimension }` and not to an alias or `Applied` form — if it
/// did not, [`param_quantity_slot_dimension`] would need an alias-resolution step and
/// this fixture would be silent.
///
/// Note the arg is a `Type::Tensor` and the param a `Type::Matrix`: Rule 3
/// (`type_compat.rs`) already makes that conversion legal, so the FAMILY check
/// passes and only the quantity slot separates them.
#[test]
fn matrix_builtin_cross_dimension_at_inertia_param_warns_arg_type_mismatch() {
    assert_single_arg_type_mismatch_warning(
        SRC_MATRIX_CROSS_DIMENSION,
        "inertia",
        "Matrix<3,3,MomentOfInertia> ← Tensor2x3<Length>",
    );
}

// FENCES (a) and (b) for this family are ALREADY PINNED above and must stay
// green through the tightening; they are cited here rather than cloned, because
// re-asserting an identical fixture in two places is the lockstep duplication
// that rots (house rule G7):
//
//   (a) `matrix_param_given_nested_list_literal_stays_clean` — a
//       `Matrix<3,3,MomentOfInertia>` param fed `[[0.0, …], …]`. `Type::List`
//       carries NO quantity slot, so there is nothing to compare and the arm
//       must stay silent. This is the spelling all 12 corpus
//       `MassProperties.inertia` sites use (`examples/dynamics/*_idyn.ri`), and
//       it is the reason strict equality is unavailable for this family.
//   (b) `tensor_param_given_vector_stays_clean` — a `Tensor<1,3,Length>` param
//       fed `vec3(0m, 0m, 1m)`: dimensions AGREE, so silent.

const SRC_MATRIX_DIMENSIONED_CELL_AT_DIMENSIONLESS: &str = r#"module test.matrix_dimensioned_at_dimensionless
structure def Jacobian { param jac : Matrix<3, 3, Dimensionless> }
structure def Root {
    let a = Jacobian(jac: matrix([[1m, 0, 0], [0, 0, 0], [0, 0, 0]]))
}
"#;

/// THE PARAM-SIDE RULING (task 6159), `.ri`/ctor seam at the `Matrix`/`Tensor`
/// arm — and the end-to-end pin for `matrix(…)` → `matrix_shape` → the rule.
///
/// `crates/reify-core/src/ty.rs` asserts a consequence specific to this arm: a
/// heterogeneous `matrix(…)` at a `Matrix<M, N, Dimensionless>` param can now be
/// rejected on cell `[0][0]` alone, where before only a DIMENSIONED param slot
/// could trip it. Nothing pinned that CHAIN. Its two neighbours each cover a
/// different half and neither covers this one:
///
///   * `dimensionless_quantity_matrix_param_rejects_dimensioned_tensor_arg`
///     (`conformance/mod.rs`) builds a `Type::tensor(2, 3, Length)` directly, so
///     it exercises the STRICT param-side predicate but bypasses `matrix_shape`
///     entirely;
///   * `matrix_builtin_cross_dimension_at_inertia_param_warns_arg_type_mismatch`
///     (directly above) routes through `matrix_shape` but is concrete×concrete,
///     i.e. already green under task 5766's SYMMETRIC rule — it cannot tell the
///     two param-side predicates apart.
///
/// Without this fixture a regression in either `matrix_shape` or this arm's
/// routing would leave the ty.rs claim documented and every test green.
///
/// The literal is deliberately HETEROGENEOUS — cell `[0][0]` is `1m` and every
/// other cell is dimensionless — so the rejection rests on the first-cell
/// inference ALONE, which is exactly the instance ty.rs names. That inference
/// weakness is owned by task 5889: if it lands the preferred fix (detect
/// heterogeneous cells, degrade the inferred quantity to
/// `Type::dimensionless_scalar()`), this fixture flips to CLEAN and must be
/// retargeted at a HOMOGENEOUS dimensioned literal in the same commit.
#[test]
fn matrix_builtin_dimensioned_cell_at_dimensionless_matrix_param_warns_arg_type_mismatch() {
    // Non-vacuity guard — see `vec3_dimensionless_at_dimensioned_vector_param_stays_clean`.
    // Load-bearing twice over here: a `Matrix<3, 3, Dimensionless>` that failed
    // to resolve, or a heterogeneous `matrix(…)` literal that failed to compile,
    // would emit zero ctor-conformance diagnostics and read as a RULE failure.
    let module = compile_source_with_stdlib(SRC_MATRIX_DIMENSIONED_CELL_AT_DIMENSIONLESS);
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    // The `_in` variant so the guard above and the assertion share that one
    // compile of the source plus the whole stdlib.
    assert_single_quantity_conflict_warning_in(
        &module,
        "jac",
        "Real",
        "Scalar[m]",
        "Matrix<3,3,Dimensionless> ← Tensor2x3<Length> (from cell [0][0] alone)",
    );
}

const SRC_TENSOR_MATCHING_DIMENSION: &str = r#"module test.tensor_matching_dimension
structure def Surface {
    param moi : Tensor<2, 3, MomentOfInertia>
}
structure def Root {
    let s = Surface(moi: matrix([
        [1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m]
    ]))
}
"#;

/// FENCE (c) — a MATCHING concrete dimension at a `Tensor` param stays clean.
///
/// The build form is lifted verbatim from `examples/type_hygiene/type_hygiene_surface.ri:26`'s
/// own `HasInertia.moi` default, so this pins the tightening against the one
/// non-`Length` `Tensor` spelling that actually exists in the corpus. Green both
/// before and after the tightening; distinguishes "compares the dimension" from
/// "rejects any dimensioned `matrix(…)` arg".
#[test]
fn tensor_matching_dimension_at_moi_param_stays_clean() {
    let module = compile_source_with_stdlib(SRC_TENSOR_MATCHING_DIMENSION);
    // Non-vacuity guard, load-bearing HERE above all: this is the only new fixture
    // with no rejecting sibling to prove its param/arg spelling still reaches the
    // walker, so an unresolvable `MomentOfInertia`, a `Tensor<2,3,Q>` resolution
    // regression, or a change in how `1.0 * 1kg * 1m * 1m` types would otherwise
    // leave the fence passing vacuously.
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a Tensor<2,3,MomentOfInertia> param fed a matrix(…) whose elements carry the SAME \
         dimension (kg·m²) must stay SILENT — the quantity rule compares DimensionVectors, \
         it does not reject dimensioned args. Got: {diags:#?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// ε step-1 probes: unknown named argument (PRD §7 row 11 — E_CTOR_UNKNOWN_FIELD)
//
// The `StructureInstanceCtor` by-name binder in `crates/reify-compiler/src/expr.rs`
// appends a named argument whose name matches no `Param` cell as `__arg{i}` and
// says nothing at all. ε emits `DiagnosticCode::CtorUnknownField` there, at the
// `CTOR_FIELD_CONFORMANCE_SEVERITY` knob (Warning at ε, Error at δ).
//
// RED on the pre-step-2 tree for (a)/(b)/(c) — all three fixtures are silent
// today. (d)/(e)/(f) are legality guards and must be green BOTH before and after.
// ═════════════════════════════════════════════════════════════════════════════

const SRC_UNKNOWN_FIELD: &str = r#"module test.unknown_field
structure def Widget11 { param label : String }
structure def Root {
    let x = Widget11(labl: "x")
}
"#;

/// (a) Row 11 in a value-cell context: a typo'd field name must produce exactly
/// one `CtorUnknownField`, at Warning, naming both the offending field and the
/// constructor — and must NOT affect the exit code at ε (`errors_only` empty;
/// δ is what flips these to Error).
#[test]
fn unknown_named_argument_emits_ctor_unknown_field_warning() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_FIELD);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "a single unknown named argument must emit exactly one ctor-conformance \
         diagnostic, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::CtorUnknownField),
        "unknown named argument must carry the CtorUnknownField code, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "ε emits at the CTOR_FIELD_CONFORMANCE_SEVERITY knob (Warning); a hard-coded \
         severity here would silently survive δ's one-const flip. Got: {:?}",
        diags[0]
    );
    assert!(
        diags[0].message.starts_with("E_CTOR_UNKNOWN_FIELD: "),
        "the mnemonic must be a message PREFIX — `reify check` renders \
         `{{severity}}: {{message}}` and never prints the DiagnosticCode, so without it \
         the ε signal is invisible at the CLI. Got: {:?}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("labl"),
        "message must name the offending field, got: {:?}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("Widget11"),
        "message must name the constructor, got: {:?}",
        diags[0].message
    );
    assert!(
        errors_only(&module).is_empty(),
        "ε keeps exit code 0 — the unknown-field tightening is Warning-only until δ. \
         Got errors: {:?}",
        errors_only(&module)
    );
}

/// (b) Span anchoring (PRD §6 C3): the label must sit at the OFFENDING ARGUMENT's
/// own span, not at the whole `Widget11(...)` call. α's step-10 moved the
/// ctor-conformance labels off a representative span onto the real call-site
/// span; ε goes one level finer because the actionable token is the argument.
#[test]
fn unknown_named_argument_label_anchors_at_the_offending_argument() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_FIELD);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got: {diags:#?}");
    assert!(
        !diags[0].labels.is_empty(),
        "CtorUnknownField must carry a label span, got: {:?}",
        diags[0]
    );
    let span: SourceSpan = diags[0].labels[0].span;
    assert!(
        !span.is_empty(),
        "label span must be NON-empty (a SourceSpan::empty renders no caret), got: {span:?}"
    );
    let sliced = &SRC_UNKNOWN_FIELD[span.start as usize..span.end as usize];
    assert!(
        !sliced.contains("Widget11("),
        "label must anchor at the offending ARGUMENT, not at the whole ctor call \
         (PRD §6 C3), got slice {sliced:?}"
    );
    assert!(
        sliced.contains("\"x\""),
        "label span must cover the offending `labl: \"x\"` argument, got slice {sliced:?}"
    );
}

const SRC_UNKNOWN_FIELD_TWICE: &str = r#"module test.unknown_field_twice
structure def Widget11 { param label : String }
structure def Root {
    let x = Widget11(labl: "x", lable2: "y")
}
"#;

/// (c) Multiplicity: an unknown field name is a PER-ARGUMENT fact, so two typo'd
/// names produce two diagnostics — each needs its own span to be actionable.
/// This is the C2(ii) "at most one diagnostic per (arg, fact)" pin for this code;
/// contrast with `CtorArity`, which is one-per-CALL.
#[test]
fn two_unknown_named_arguments_emit_one_diagnostic_each() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_FIELD_TWICE);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        2,
        "two unknown named arguments must emit exactly two diagnostics (one per \
         offending arg), got: {diags:#?}"
    );
    assert!(
        diags
            .iter()
            .all(|d| d.code == Some(DiagnosticCode::CtorUnknownField)),
        "both diagnostics must be CtorUnknownField, got: {diags:#?}"
    );
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("labl'")),
        "one diagnostic must name `labl`, got: {messages:#?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("lable2")),
        "one diagnostic must name `lable2`, got: {messages:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "ε keeps exit code 0, got errors: {:?}",
        errors_only(&module)
    );
}

const SRC_KNOWN_FIELD: &str = r#"module test.known_field
structure def Widget11 { param label : String }
structure def Root {
    let x = Widget11(label: "x")
}
"#;

/// (d) Legality guard: a correctly-spelled named argument stays silent. Green
/// both before and after step-2 — this is what proves the tightening keys on
/// "no such parameter" and not merely on "the call uses named arguments".
#[test]
fn correct_named_argument_emits_no_ctor_conformance_diagnostic() {
    let module = compile_source_with_stdlib(SRC_KNOWN_FIELD);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a correctly-named argument must stay silent, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
}

const SRC_DUPLICATE_KNOWN_FIELD: &str = r#"module test.duplicate_known_field
structure def Widget11 { param label : String }
structure def Root {
    let x = Widget11(label: "a", label: "b")
}
"#;

/// (e) The sibling duplicate-named-arg diagnostic — emitted three lines away in
/// the same binder — must be untouched by ε, and a duplicate of a KNOWN parameter
/// must NOT be reclassified as an unknown field. That diagnostic is code-less
/// today (it predates the ctor-conformance code set), so it does not appear in
/// `ctor_conformance_diags` at all; it is matched by message text here.
#[test]
fn duplicate_known_named_argument_is_not_an_unknown_field() {
    let module = compile_source_with_stdlib(SRC_DUPLICATE_KNOWN_FIELD);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a duplicate of a KNOWN parameter is not an unknown field — ε must emit no \
         ctor-conformance diagnostic here, got: {diags:#?}"
    );
    let dupes: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate named argument"))
        .collect();
    assert_eq!(
        dupes.len(),
        1,
        "the pre-existing duplicate-named-argument diagnostic must still fire exactly \
         once, got: {:#?}",
        module.diagnostics
    );
    assert_eq!(
        dupes[0].severity,
        Severity::Error,
        "the duplicate-named-arg precedent is an Error and is outside the ε knob, \
         got: {:?}",
        dupes[0]
    );
}

const SRC_SUB_UNKNOWN_FIELD: &str = r#"module test.sub_unknown_field
structure def Widget11 { param label : String }
structure def Root {
    sub p = Widget11(labl: "x")
}
"#;

/// (f) SCOPE BOUNDARY, pinned deliberately rather than left implicit.
///
/// The `sub p = Ctor(...)` binding form does NOT route through the
/// `StructureInstanceCtor` by-name binder in `expr.rs` — its RHS is handled by
/// the `PendingBoundCheck` path instead — so neither `__arg{i}` fallback is
/// reachable from it and ε cannot see this call at all. Measured on the base of
/// this branch: even the long-standing duplicate-named-arg Error does not fire
/// through `sub =`, which is the same seam.
///
/// ε therefore covers expression-position constructors only. Closing the sub-path
/// hole means adding an unknown-field check to the `PendingBoundCheck` walker — a
/// different mechanism in a different file, with its own double-emission risk
/// (PRD §10 Q4) — so it is a follow-up (#6191), not part of this diff. Note the
/// over-arity half is unreachable there regardless: `sub p = W("a", "b")` does not
/// parse. This test exists so
/// that boundary is a recorded, asserted fact: if a later change DOES make the sub
/// path reach the binder, this test fails and forces the double-emission question
/// to be answered rather than silently regressed into.
#[test]
fn sub_binding_rhs_is_outside_the_epsilon_binder_seam() {
    let module = compile_source_with_stdlib(SRC_SUB_UNKNOWN_FIELD);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "the `sub =` RHS does not route through the StructureInstanceCtor binder, so ε \
         emits nothing here (documented scope boundary, not a silent miss). If this \
         fires, the seam moved — re-answer the PRD §10 Q4 double-emission question \
         before widening. Got: {diags:#?}"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// ε step-3 probes: over-arity positional argument (PRD §7 row 12 — E_CTOR_ARITY)
//
// The sibling leniency to row 11: a positional argument with no param slot left
// is appended as `__arg{call_idx}` with no diagnostic. ε emits
// `DiagnosticCode::CtorArity` there, at the same knob.
//
// Multiplicity differs from `CtorUnknownField` on purpose: arity is a CALL-level
// fact (`W("a","b","c")` against a 1-param def is ONE mistake), so exactly one
// diagnostic per call site, anchored at the FIRST surplus argument. That matches
// every existing arity diagnostic in the repo — `arg_check.rs` emits one per
// call, never one per surplus arg.
//
// RED on the pre-step-4 tree for (a)-(e); (f)-(h) are legality guards, green
// both before and after.
// ═════════════════════════════════════════════════════════════════════════════

const SRC_OVER_ARITY: &str = r#"module test.over_arity
structure def Widget12 { param label : String }
structure def Root {
    let x = Widget12("a", "b")
}
"#;

/// (a) Row 12: one surplus positional argument must produce exactly one
/// `CtorArity`, at Warning, naming BOTH arity facts (expected 1, got 2), with
/// exit-code neutrality preserved at ε.
#[test]
fn over_arity_positional_argument_emits_ctor_arity_warning() {
    let module = compile_source_with_stdlib(SRC_OVER_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "an over-arity ctor call must emit exactly one ctor-conformance diagnostic, \
         got: {diags:#?}"
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::CtorArity),
        "over-arity must carry the CtorArity code, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "ε emits at the CTOR_FIELD_CONFORMANCE_SEVERITY knob (Warning), got: {:?}",
        diags[0]
    );
    let msg = &diags[0].message;
    assert!(
        msg.starts_with("E_CTOR_ARITY: "),
        "the mnemonic must be a message PREFIX so the ε signal is visible at the CLI, \
         got: {msg:?}"
    );
    assert!(
        msg.contains("Widget12"),
        "message must name the constructor, got: {msg:?}"
    );
    assert!(
        msg.contains("at most 1 argument"),
        "message must name the EXPECTED arity, in arg_check.rs's centralised wording \
         (singular noun when the count is 1), got: {msg:?}"
    );
    assert!(
        msg.contains("got 2"),
        "message must name the ACTUAL arity, got: {msg:?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "ε keeps exit code 0, got errors: {:?}",
        errors_only(&module)
    );
}

/// (b) Span anchoring + centralised label text: the label sits at the offending
/// surplus argument (`"b"`), not at the whole call, and reads exactly
/// `"wrong number of arguments"` — the wording `arg_check.rs` centralises so
/// every arg-count diagnostic in the codebase looks identical.
#[test]
fn over_arity_label_anchors_at_the_first_surplus_argument() {
    let module = compile_source_with_stdlib(SRC_OVER_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(diags.len(), 1, "expected one diagnostic, got: {diags:#?}");
    assert!(
        !diags[0].labels.is_empty(),
        "CtorArity must carry a label span, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].labels[0].message, "wrong number of arguments",
        "label text is centralised in arg_check.rs so all arg-count diagnostics read \
         identically, got: {:?}",
        diags[0].labels[0]
    );
    let span: SourceSpan = diags[0].labels[0].span;
    assert!(!span.is_empty(), "label span must be NON-empty, got: {span:?}");
    let sliced = &SRC_OVER_ARITY[span.start as usize..span.end as usize];
    assert!(
        !sliced.contains("Widget12("),
        "label must anchor at the offending argument, not the whole ctor call, got \
         slice {sliced:?}"
    );
    assert!(
        sliced.contains('b') && !sliced.contains('a'),
        "label span must cover the SURPLUS second argument `\"b\"`, not the first, got \
         slice {sliced:?}"
    );
}

const SRC_OVER_ARITY_BY_TWO: &str = r#"module test.over_arity_by_two
structure def Widget12 { param label : String }
structure def Root {
    let x = Widget12("a", "b", "c")
}
"#;

/// (c) Once-per-call: TWO surplus arguments still produce exactly ONE `CtorArity`,
/// anchored at the FIRST surplus one. Pins that the implementation does not emit
/// per surplus arg — which would triple β's survey count for a single author
/// mistake and make δ's Error stage hostile.
#[test]
fn two_surplus_positional_arguments_still_emit_exactly_one_ctor_arity() {
    let module = compile_source_with_stdlib(SRC_OVER_ARITY_BY_TWO);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "arity is a CALL-level fact: two surplus args are still one mistake and must \
         emit exactly one diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorArity));
    let msg = &diags[0].message;
    assert!(
        msg.contains("at most 1 argument") && msg.contains("got 3"),
        "message must report expected 1 / got 3, got: {msg:?}"
    );
    let span: SourceSpan = diags[0].labels[0].span;
    let sliced = &SRC_OVER_ARITY_BY_TWO[span.start as usize..span.end as usize];
    assert!(
        sliced.contains('b') && !sliced.contains('c'),
        "the single diagnostic must anchor at the FIRST surplus argument (`\"b\"`), got \
         slice {sliced:?}"
    );
}

const SRC_ZERO_PARAM_OVER_ARITY: &str = r#"module test.zero_param_over_arity
structure def W0 { let k = 1 }
structure def Root {
    let x = W0("a")
}
"#;

/// (d) Zero-param structure: any positional argument is surplus. Pins the
/// plural-noun branch of the centralised wording (`0 arguments`, not
/// `0 argument`) and that `nparams == 0` is not special-cased into silence.
#[test]
fn positional_argument_to_zero_param_structure_emits_ctor_arity() {
    let module = compile_source_with_stdlib(SRC_ZERO_PARAM_OVER_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "a positional arg to a param-less structure must emit exactly one CtorArity, \
         got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorArity));
    let msg = &diags[0].message;
    assert!(
        msg.contains("at most 0 arguments") && msg.contains("got 1"),
        "message must report expected 0 / got 1, with the PLURAL noun for a zero \
         expected count (arg_check.rs keys the noun on the expected count), got: {msg:?}"
    );
}

const SRC_MIXED_NAMED_THEN_POSITIONAL: &str = r#"module test.mixed_named_positional
structure def Widget12 { param label : String }
structure def Root {
    let x = Widget12(label: "a", "b")
}
"#;

/// (e) Mixed named + positional: the named argument consumes the only slot, so
/// the trailing positional has nowhere to bind and is surplus. Pins that arity is
/// computed against REMAINING slots (the binder's two-pass named-then-positional
/// order), not against a naive `args.len() > nparams` on positionals alone.
#[test]
fn positional_argument_after_named_fills_the_slot_emits_ctor_arity() {
    let module = compile_source_with_stdlib(SRC_MIXED_NAMED_THEN_POSITIONAL);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "the named arg consumed the only slot, so the positional is surplus — exactly \
         one CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorArity));
    assert!(
        diags[0].message.contains("got 2"),
        "the reported actual count is the whole call's arg count, got: {:?}",
        diags[0].message
    );
}

const SRC_EXACT_ARITY: &str = r#"module test.exact_arity
structure def Widget12 { param label : String }
structure def Root {
    let x = Widget12("a")
}
"#;

/// (f) Legality guard: an exact-arity positional call stays silent.
#[test]
fn exact_arity_positional_call_emits_no_ctor_conformance_diagnostic() {
    let module = compile_source_with_stdlib(SRC_EXACT_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "an exact-arity call must stay silent, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
}

const SRC_UNDER_ARITY_WITH_DEFAULT: &str = r#"module test.under_arity_default
structure def W2 {
    param a : String
    param b : String = "d"
}
structure def Root {
    let x = W2("x")
}
"#;

/// (g) Legality guard, the load-bearing one: UNDER-arity is legal when the
/// uncovered params carry defaults. ε tightens the surplus direction only; the
/// binder's `defaults` computation must be untouched. A naive
/// `args.len() != nparams` check would break every defaulted structure in the
/// corpus, so this fixture is what stops the tightening from over-reaching.
#[test]
fn under_arity_covered_by_defaults_emits_no_ctor_conformance_diagnostic() {
    let module = compile_source_with_stdlib(SRC_UNDER_ARITY_WITH_DEFAULT);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "uncovered params with defaults are legal — ε must not diagnose under-arity, \
         got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
}

const SRC_ZERO_ARG_CALL: &str = r#"module test.zero_arg_call
structure def W0 { let k = 1 }
structure def Root {
    let x = W0()
}
"#;

/// (h) Legality guard: a zero-arg call to a param-less structure stays silent —
/// the empty-args edge of the surplus check.
#[test]
fn zero_argument_call_emits_no_ctor_conformance_diagnostic() {
    let module = compile_source_with_stdlib(SRC_ZERO_ARG_CALL);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a zero-arg call must stay silent, got: {diags:#?}"
    );
    assert!(
        errors_only(&module).is_empty(),
        "fixture must compile cleanly, got: {:?}",
        errors_only(&module)
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// ε step-5 probes: cross-context reach, no-cross-talk, and preservation pins.
//
// Steps 2/4 each verified their own emit site in a value-cell context. These pin
// the INTERACTIONS those steps leave unverified: that the binder is reached from
// the other expression positions, that ε's two structural checks neither swallow
// nor double α's type check when all three faults ride one call, that the type
// anti-cascade carve-out holds, and that ε remains behaviour-preserving.
// ═════════════════════════════════════════════════════════════════════════════

const SRC_UNKNOWN_FIELD_FN_BODY: &str = r#"module test.unknown_field_fn
structure def Widget11 { param label : String }
fn make() -> Widget11 { Widget11(labl: "x") }
"#;

/// (a) Free-fn body reach: the by-name binder is reached from a `fn` body, not
/// only from a structure member. Without this, step-2 would only prove the
/// value-cell path.
#[test]
fn unknown_named_argument_in_fn_body_emits_ctor_unknown_field() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_FIELD_FN_BODY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "a free-fn body ctor must reach the binder and emit exactly one diagnostic, \
         got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorUnknownField));
    assert_eq!(diags[0].severity, Severity::Warning);
}

const SRC_UNKNOWN_FIELD_NESTED: &str = r#"module test.unknown_field_nested
structure def Widget11 { param label : String }
structure def O { param inner : Widget11 }
structure def Root {
    let x = O(inner: Widget11(labl: "x"))
}
"#;

/// (b) Nested-ctor-argument reach, and attribution. The INNER call's typo must be
/// diagnosed exactly once; the OUTER call is well-formed and must contribute
/// nothing — a nested ctor is a legal argument, so an implementation that walked
/// the outer call's args as if they were unknown fields would double-report here.
#[test]
fn unknown_named_argument_in_nested_ctor_argument_is_attributed_to_the_inner_call() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_FIELD_NESTED);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "only the INNER call is malformed — the well-formed outer `O(inner: ...)` call \
         must contribute nothing, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorUnknownField));
    assert!(
        diags[0].message.contains("Widget11") && !diags[0].message.contains("'O'"),
        "the diagnostic must be attributed to the inner `Widget11` call, not the outer \
         `O` call, got: {:?}",
        diags[0].message
    );
}

const SRC_OVER_ARITY_FN_BODY: &str = r#"module test.over_arity_fn
structure def Widget12 { param label : String }
fn make2() -> Widget12 { Widget12("a", "b") }
"#;

/// (c) Free-fn body reach for the arity site, the sibling of (a).
#[test]
fn over_arity_in_fn_body_emits_ctor_arity() {
    let module = compile_source_with_stdlib(SRC_OVER_ARITY_FN_BODY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "a free-fn body over-arity ctor must emit exactly one diagnostic, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorArity));
    assert_eq!(diags[0].severity, Severity::Warning);
}

const SRC_ALL_THREE_FAULTS: &str = r#"module test.all_three_faults
structure def Widget13 { param label : String }
structure def Root {
    let x = Widget13(label: 42, labl: "x", "extra")
}
"#;

/// (d) NO CROSS-TALK — the central pin of this step. One call carrying all three
/// faults at once (wrong TYPE on a bound param, an unknown FIELD name, and a
/// surplus positional) must yield exactly three diagnostics: one each of
/// `ArgTypeMismatch` (α's type walker), `CtorUnknownField` and `CtorArity`
/// (ε's structural checks).
///
/// This is the C2(ii) at-most-one-per-(arg, fact) pin: no duplicates (ε must not
/// double-count an argument it already reported), no suppression (ε's structural
/// checks must not swallow α's type check, nor each other), and all three at the
/// shared knob severity so δ moves them together. It is also what makes β's
/// corpus survey able to bucket the three fault classes separately.
#[test]
fn all_three_ctor_faults_on_one_call_emit_exactly_one_diagnostic_each() {
    let module = compile_source_with_stdlib(SRC_ALL_THREE_FAULTS);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        3,
        "expected exactly three ctor-conformance diagnostics (one per fact), got: {diags:#?}"
    );
    let codes: Vec<Option<DiagnosticCode>> = diags.iter().map(|d| d.code).collect();
    for expected in [
        DiagnosticCode::ArgTypeMismatch,
        DiagnosticCode::CtorUnknownField,
        DiagnosticCode::CtorArity,
    ] {
        assert_eq!(
            codes.iter().filter(|c| **c == Some(expected)).count(),
            1,
            "expected exactly one {expected:?}, got codes {codes:#?} from {diags:#?}"
        );
    }
    assert!(
        diags.iter().all(|d| d.severity == Severity::Warning),
        "all three must sit at the shared CTOR_FIELD_CONFORMANCE_SEVERITY knob so δ \
         moves them together, got: {diags:#?}"
    );
    let type_msg = diags
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        type_msg.contains("label"),
        "α's type diagnostic must still name the bound param `label` — ε must not \
         perturb it, got: {type_msg:?}"
    );
    let unknown_msg = diags
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CtorUnknownField))
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        unknown_msg.contains("labl'"),
        "the unknown-field diagnostic must name `labl`, got: {unknown_msg:?}"
    );
    let arity_msg = diags
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CtorArity))
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        arity_msg.contains("at most 1 argument") && arity_msg.contains("got 3"),
        "the arity diagnostic must report expected 1 / got 3, got: {arity_msg:?}"
    );
}

const SRC_UNKNOWN_NAME_THEN_POSITIONAL: &str = r#"module test.unknown_name_then_positional
structure def Widget14 { param label : String }
structure def Root {
    let x = Widget14(labl: "x", "a")
}
"#;

/// (e) SLOT ACCOUNTING, half one: an UNKNOWN named argument does NOT consume a
/// param slot, so a following positional binds into the slot the typo'd name
/// failed to claim. `Widget14(labl: "x", "a")` against a 1-param def is therefore
/// exactly one `CtorUnknownField` and ZERO `CtorArity` — the call is not
/// over-arity, because pass 1 left slot 0 free for `"a"` to take.
///
/// (d) above and `positional_argument_after_named_fills_the_slot_emits_ctor_arity`
/// both use a VALID named argument, which DOES consume its slot, so without this
/// fixture the unknown-name-skips-the-slot rule is unpinned. Making an unknown
/// name consume a slot anyway is a plausible "be more helpful" refactor — and
/// exactly the seam #6191 works in — which would silently add a spurious second
/// diagnostic here with the rest of the suite still green.
#[test]
fn unknown_named_argument_does_not_consume_a_param_slot() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_NAME_THEN_POSITIONAL);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "the unknown name must not claim slot 0, leaving it for the positional — \
         exactly one CtorUnknownField and no CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorUnknownField));
    assert_eq!(diags[0].severity, Severity::Warning);
    assert_eq!(
        diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::CtorArity))
            .count(),
        0,
        "one positional for one free slot is exact arity — an unknown NAME must not \
         additionally be reported as surplus, got: {diags:#?}"
    );
}

const SRC_UNKNOWN_NAME_AND_SURPLUS: &str = r#"module test.unknown_name_and_surplus
structure def Widget15 { param label : String }
structure def Root {
    let x = Widget15(labl: "x", "a", "b")
}
"#;

/// (f) SLOT ACCOUNTING, half two: add one more positional to (e) and the call
/// genuinely IS over-arity — `"a"` takes slot 0, `"b"` has nowhere to go — so both
/// ε codes fire, exactly one each.
///
/// The load-bearing pin is the `got` COUNT. It is `args.len()`, so it counts the
/// unknown NAMED argument too: the message reads `expects at most 1 argument,
/// got 3`, not `got 2`, even though the fact being reported concerns surplus
/// POSITIONALS. That is deliberate — it reports the call's actual argument count,
/// matching `arg_check.rs`'s builtin-arity wording, rather than a positionals-only
/// count the author could not match against their own source. Recorded as a
/// decision because (e)'s "does not consume a slot" and this "is nevertheless
/// counted in `got`" are separate questions: a refactor that unified them would
/// flip exactly one of these two fixtures.
#[test]
fn unknown_named_argument_is_still_counted_in_the_arity_got_total() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_NAME_AND_SURPLUS);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        2,
        "expected exactly one CtorUnknownField and one CtorArity, got: {diags:#?}"
    );
    for expected in [DiagnosticCode::CtorUnknownField, DiagnosticCode::CtorArity] {
        assert_eq!(
            diags.iter().filter(|d| d.code == Some(expected)).count(),
            1,
            "expected exactly one {expected:?}, got: {diags:#?}"
        );
    }
    let arity_msg = diags
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CtorArity))
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        arity_msg.contains("at most 1 argument") && arity_msg.contains("got 3"),
        "`got` is args.len(), so the unknown named argument is counted in the total; \
         expected `at most 1 argument` / `got 3`, got: {arity_msg:?}"
    );
}

const SRC_UNKNOWN_FIELD_POISONED_ARG: &str = r#"module test.unknown_field_poisoned
structure def Widget11 { param label : String }
structure def Root {
    let x = Widget11(labl: no_such_name_anywhere)
}
"#;

/// (g) ANTI-CASCADE CARVE-OUT. The unknown-named arg here is itself an erroring
/// expression (an unresolved name → poison). The `CtorUnknownField` must STILL
/// fire exactly once.
///
/// PRD §6 C2(i)'s type anti-cascade exists so a type walker never stacks a second
/// TYPE complaint on an upstream type error. An unknown field NAME is not a type
/// fact — it is decidable with no reference to the argument's type — and
/// suppressing it here would hide the more actionable diagnostic (the typo)
/// behind a downstream error the typo often caused. Pinned explicitly so a later
/// refactor cannot quietly fold these emit sites under the walker's skip rules.
#[test]
fn unknown_named_argument_still_fires_when_its_value_is_poisoned() {
    let module = compile_source_with_stdlib(SRC_UNKNOWN_FIELD_POISONED_ARG);
    // Non-vacuity guard, load-bearing here: if the argument stopped erroring, this
    // fixture would no longer exercise the anti-cascade path at all and would pass
    // for the wrong reason. Measured: `error: unresolved name: no_such_name_anywhere`.
    assert!(
        !errors_only(&module).is_empty(),
        "fixture must genuinely poison the argument, else the carve-out is untested"
    );
    let unknown: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CtorUnknownField))
        .collect();
    assert_eq!(
        unknown.len(),
        1,
        "an unknown field NAME is type-independent, so a poisoned argument value must \
         not suppress it (nor duplicate it), got all diagnostics: {:#?}",
        module.diagnostics
    );
    assert_eq!(unknown[0].severity, Severity::Warning);
}

/// (h) BEHAVIOUR PRESERVATION. ε adds diagnostics only: every ε fixture whose
/// faults are purely ctor-conformance ones still compiles to a module with NO
/// errors, i.e. `reify check` keeps exit 0 until δ flips the knob. The poisoned
/// fixture from (e) is deliberately excluded — its unresolved name is a genuine
/// pre-existing Error unrelated to ε.
#[test]
fn epsilon_fixtures_remain_error_free_and_exit_code_neutral() {
    let cases: &[(&str, &str)] = &[
        ("unknown field", SRC_UNKNOWN_FIELD),
        ("unknown field x2", SRC_UNKNOWN_FIELD_TWICE),
        ("unknown field in fn body", SRC_UNKNOWN_FIELD_FN_BODY),
        ("unknown field nested", SRC_UNKNOWN_FIELD_NESTED),
        ("over-arity", SRC_OVER_ARITY),
        ("over-arity by two", SRC_OVER_ARITY_BY_TWO),
        ("over-arity in fn body", SRC_OVER_ARITY_FN_BODY),
        ("zero-param over-arity", SRC_ZERO_PARAM_OVER_ARITY),
        ("mixed named + positional", SRC_MIXED_NAMED_THEN_POSITIONAL),
        ("all three faults", SRC_ALL_THREE_FAULTS),
        ("unknown + positional", SRC_UNKNOWN_NAME_THEN_POSITIONAL),
        ("unknown + surplus", SRC_UNKNOWN_NAME_AND_SURPLUS),
    ];
    let mut offenders: Vec<String> = Vec::new();
    for &(label, source) in cases {
        let module = compile_source_with_stdlib(source);
        let errors = errors_only(&module);
        if !errors.is_empty() {
            offenders.push(format!("  [{label}] errors: {errors:#?}"));
        }
        // Non-vacuity: each of these fixtures must actually be reaching a ctor
        // emit site, else "no errors" would pass for the wrong reason.
        assert!(
            !ctor_conformance_diags(&module).is_empty(),
            "[{label}] fixture must still trip a ctor-conformance diagnostic — a silent \
             fixture would make the error-free assertion vacuous"
        );
    }
    assert!(
        offenders.is_empty(),
        "ε is a WARNING stage: these fixtures must still compile error-free (exit 0), \
         and only δ's one-const flip may turn them into errors:\n{}",
        offenders.join("\n")
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Step-10 probes (REVIEW REMEDIATION): an `auto`-declared param IS a declared
// param, so a named argument targeting it is not an unknown field.
//
// The binder's `params` vec (`expr.rs`, `StructureInstanceCtor` arm) filters
// `template.value_cells` to `ValueCellKind::Param` ONLY, but `param x : T = auto`
// compiles to `ValueCellKind::Auto { free }` (`entity.rs`,
// `build_param_value_cell_decl`). An auto-declared param is therefore invisible
// to the ε unknown-field emit site, and its own name reads back as "unknown" —
// a message that asserts the opposite of the source. Two independent sites
// already treat `Param | Auto { .. }` as the externally-settable member set
// (`connect.rs`, `traits.rs`), so this binder is the outlier.
//
// Not hypothetical: ~20 corpus structures declare auto params
// (`examples/auto_binding_sites.ri`, `examples/integration_corner_cases.ri`,
// `examples/m11_annotations.ri`, …). The corpus gate is green today only
// because none of them is constructed by name in expression position — and
// step-6 wired that gate to this very code, so the first example that does
// fails it; δ then hard-rejects the same valid source.
//
// RED today for (a)/(b) (each measured `count=1` against the 89-green
// baseline). (c)/(d) are guards, green BOTH before and after step-11: (c) pins
// that the fix must not blanket-suppress the code on any template that merely
// HAS an `Auto` cell, (d) that the no-auto path is untouched.
// ═════════════════════════════════════════════════════════════════════════════

const SRC_AUTO_PARAM_NAMED_ARG: &str = r#"module test.auto_param_named_arg
structure def WidgetAuto {
    param a : Real = auto
    param b : Real
}
structure def Root {
    let x = WidgetAuto(a: 1.0, b: 2.0)
}
"#;

/// (a) A named argument for a strict-`auto` param must emit ZERO
/// ctor-conformance diagnostics. `WidgetAuto` declares `a`; today the binder
/// cannot see it and reports `E_CTOR_UNKNOWN_FIELD: … 'WidgetAuto' has no
/// parameter with that name`, which is false about the source above.
#[test]
fn named_argument_for_an_auto_param_is_not_an_unknown_field() {
    let module = compile_source_with_stdlib(SRC_AUTO_PARAM_NAMED_ARG);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "`param a : Real = auto` is a DECLARED param — naming it in a ctor call must \
         emit no ctor-conformance diagnostic. The binder's `params` vec filters to \
         `ValueCellKind::Param` only, so an `Auto` cell reads back as unknown. Got: \
         {diags:#?}"
    );
}

const SRC_AUTO_FREE_PARAM_NAMED_ARG: &str = r#"module test.auto_free_param_named_arg
structure def WidgetAutoFree {
    param a : Real = auto(free)
    param b : Real
}
structure def Root {
    let x = WidgetAutoFree(a: 1.0, b: 2.0)
}
"#;

/// (b) The same for the `auto(free)` spelling. Both spellings lower to
/// `ValueCellKind::Auto { free }` — strict is `free: false`, `auto(free)` is
/// `free: true` — so this pins that the fix matches `Auto { .. }` and not one
/// `free` polarity, which would leave the other half of the defect live.
#[test]
fn named_argument_for_an_auto_free_param_is_not_an_unknown_field() {
    let module = compile_source_with_stdlib(SRC_AUTO_FREE_PARAM_NAMED_ARG);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "`auto(free)` and strict `auto` both lower to `ValueCellKind::Auto {{ free }}`; \
         the declared-param predicate must match on the VARIANT, not on `free`. Got: \
         {diags:#?}"
    );
}

const SRC_AUTO_PARAM_GENUINE_UNKNOWN: &str = r#"module test.auto_param_genuine_unknown
structure def WidgetAutoTypo {
    param a : Real = auto
    param b : Real
}
structure def Root {
    let x = WidgetAutoTypo(zz: 1.0, b: 2.0)
}
"#;

/// (c) OVER-CORRECTION GUARD — green before AND after. A genuinely unknown name
/// on a template that ALSO declares an auto param must still emit exactly one
/// `CtorUnknownField`, naming `zz`. Suppressing the code whenever an `Auto` cell
/// exists anywhere on the template would pass (a)/(b) while silently deleting
/// ε's detection power on ~20 corpus structures.
#[test]
fn genuine_unknown_name_still_fires_on_a_template_with_an_auto_param() {
    let module = compile_source_with_stdlib(SRC_AUTO_PARAM_GENUINE_UNKNOWN);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "a genuinely unknown name must still emit exactly one diagnostic even when the \
         template declares an auto param, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::CtorUnknownField),
        "expected CtorUnknownField, got: {:?}",
        diags[0]
    );
    assert!(
        diags[0].message.contains("zz"),
        "the diagnostic must name the offending `zz`, not the auto param `a`, got: {:?}",
        diags[0].message
    );
    assert!(
        !diags[0].message.contains("'a'"),
        "the auto-declared param `a` must not be reported as unknown, got: {:?}",
        diags[0].message
    );
}

/// (d) NO-DRIFT GUARD — green before AND after. The no-auto path is untouched by
/// the remediation: widening the DIAGNOSTIC's predicate to the declared-param set
/// must be purely additive, never a rewrite of the base behaviour. Table-driven
/// over the two pre-existing no-auto fixtures so a step-11 over-reach that
/// loosened the base predicate is caught here rather than 80 probes downstream.
#[test]
fn no_auto_unknown_field_behaviour_is_unchanged_by_the_declared_param_view() {
    let cases: &[(&str, &str, usize)] = &[
        ("typo'd name", SRC_UNKNOWN_FIELD, 1),
        ("correct name", SRC_KNOWN_FIELD, 0),
    ];
    for &(label, source, expected) in cases {
        let module = compile_source_with_stdlib(source);
        let diags = ctor_conformance_diags(&module);
        assert_eq!(
            diags.len(),
            expected,
            "[{label}] a template with no `Auto` cell must be wholly unaffected by the \
             declared-param view (there `declared == params`), got: {diags:#?}"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Step-12 probes (REVIEW REMEDIATION, second half): the over-arity CEILING is
// the DECLARED param count, not the count of positionally-bindable slots.
//
// The step-4 emit site reports `nparams` (= `params.len()`, `Param`-only), so on
// a template declaring an auto param it both fires spuriously and states a count
// the source contradicts.
//
// RED today for (a)/(b)/(c). (a)/(b) currently measure `count=1` where zero is
// owed; (c) currently reads `"at most 1 argument"` where the template declares
// two — so (c) is what forbids "just suppress CtorArity when an `Auto` cell is
// present" as a fix: the ceiling must RISE, not vanish. (d) is a no-drift guard,
// green before AND after.
//
// SCOPE, so (a) is not misread as a hole: `WidgetAutoArity(1.0, 2.0)` binds
// `1.0` to `b` (the first `Param`-kind cell, i.e. the SECOND declared param) and
// drops `2.0` into a garbage `__arg1` member. That is a REAL defect — but a
// BINDING one, not an arity one, and ε cannot state a true fact about it without
// changing the IR (forbidden: ε is diagnostics-only). It is owned by #6705.
// Probe (a) asserts SILENCE, not correctness.
// ═════════════════════════════════════════════════════════════════════════════

const SRC_AUTO_PARAM_EXACT_ARITY: &str = r#"module test.auto_param_exact_arity
structure def WidgetAutoArity {
    param a : Real = auto
    param b : Real
}
structure def Root {
    let x = WidgetAutoArity(1.0, 2.0)
}
"#;

/// (a) Two positional args against a template declaring TWO params (one of them
/// `auto`) is not over-arity, so no ctor-conformance diagnostic is owed. Today
/// the ceiling is the `Param`-only slot count (1), so this fires with
/// `"expects at most 1 argument, got 2"` — the opposite of what the source says.
///
/// Asserts silence, NOT binding correctness: the call still mis-binds (see the
/// scope note above), which is #6705's to fix.
#[test]
fn arity_within_the_declared_param_count_is_not_over_arity() {
    let module = compile_source_with_stdlib(SRC_AUTO_PARAM_EXACT_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "`WidgetAutoArity` declares TWO params (`a` is `auto`, still declared), so a \
         2-arg call is within the declared arity and must be silent. The ceiling must \
         count DECLARED params, not positionally-bindable slots. Got: {diags:#?}"
    );
}

const SRC_AUTO_FREE_PARAM_EXACT_ARITY: &str = r#"module test.auto_free_param_exact_arity
structure def WidgetAutoFreeArity {
    param a : Real = auto(free)
    param b : Real
}
structure def Root {
    let x = WidgetAutoFreeArity(1.0, 2.0)
}
"#;

/// (b) The same for `auto(free)` — both spellings lower to
/// `ValueCellKind::Auto { free }`, so the ceiling must key off the VARIANT.
#[test]
fn arity_ceiling_counts_auto_free_params_too() {
    let module = compile_source_with_stdlib(SRC_AUTO_FREE_PARAM_EXACT_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "an `auto(free)` param is declared just as a strict `auto` one is; the ceiling \
         must not depend on the `free` flag. Got: {diags:#?}"
    );
}

const SRC_AUTO_PARAM_GENUINE_OVER_ARITY: &str = r#"module test.auto_param_over_arity
structure def WidgetAutoSurplus {
    param a : Real = auto
    param b : Real
}
structure def Root {
    let x = WidgetAutoSurplus(1.0, 2.0, 3.0)
}
"#;

/// (c) DETECTION-POWER GUARD, RED today. A genuine over-arity on the SAME
/// auto-param template must still emit exactly one `CtorArity`, now reporting the
/// DECLARED count with the noun keyed off it: `"expects at most 2 arguments,
/// got 3"`. Today it wrongly reads `"at most 1 argument"`.
///
/// This is what forbids fixing (a)/(b) by suppressing the code whenever the
/// template holds an `Auto` cell: that would pass (a)/(b) while deleting ε's
/// arity detection on ~20 corpus structures.
#[test]
fn genuine_over_arity_on_an_auto_param_template_reports_the_declared_ceiling() {
    let module = compile_source_with_stdlib(SRC_AUTO_PARAM_GENUINE_OVER_ARITY);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "three args against two declared params is still over-arity — exactly one \
         CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].code,
        Some(DiagnosticCode::CtorArity),
        "expected CtorArity, got: {:?}",
        diags[0]
    );
    assert_eq!(
        diags[0].message,
        "E_CTOR_ARITY: WidgetAutoSurplus() expects at most 2 arguments, got 3",
        "the ceiling, the reported count and the PLURAL noun must all key off the \
         DECLARED param count (2), not the `Param`-only slot count (1)"
    );
}

/// (d) NO-DRIFT GUARD — green before AND after. On a template with no `Auto`
/// cell `declared_count == nparams`, and a non-empty surplus set already implies
/// `args.len() > nparams`, so the new ceiling and the new `args.len() >
/// declared_count` conjunct are both no-ops there. Table-driven over the
/// pre-existing no-auto arity fixtures — message, count and span all unchanged.
#[test]
fn no_auto_over_arity_messages_and_spans_are_unchanged_by_the_declared_ceiling() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "one surplus",
            SRC_OVER_ARITY,
            "E_CTOR_ARITY: Widget12() expects at most 1 argument, got 2",
        ),
        (
            "two surplus",
            SRC_OVER_ARITY_BY_TWO,
            "E_CTOR_ARITY: Widget12() expects at most 1 argument, got 3",
        ),
        (
            "zero-param template",
            SRC_ZERO_PARAM_OVER_ARITY,
            "E_CTOR_ARITY: W0() expects at most 0 arguments, got 1",
        ),
        (
            "named then positional",
            SRC_MIXED_NAMED_THEN_POSITIONAL,
            "E_CTOR_ARITY: Widget12() expects at most 1 argument, got 2",
        ),
    ];
    for &(label, source, expected) in cases {
        let module = compile_source_with_stdlib(source);
        let diags = ctor_conformance_diags(&module);
        assert_eq!(
            diags.len(),
            1,
            "[{label}] must still emit exactly one ctor-conformance diagnostic, got: \
             {diags:#?}"
        );
        assert_eq!(
            diags[0].message, expected,
            "[{label}] a template with no `Auto` cell must be wholly unaffected by the \
             declared-param ceiling"
        );
    }
    // Span: unchanged anchoring at the FIRST surplus argument.
    let module = compile_source_with_stdlib(SRC_OVER_ARITY);
    let span: SourceSpan = ctor_conformance_diags(&module)[0].labels[0].span;
    let sliced = &SRC_OVER_ARITY[span.start as usize..span.end as usize];
    assert!(
        sliced.contains('b') && !sliced.contains('a'),
        "the label must still anchor at the surplus `\"b\"` — the ceiling change must \
         not move `extra_positional_idxs`, got slice {sliced:?}"
    );
}

// ---------------------------------------------------------------------------
// AUTO-LET vs AUTO-PARAM (esc-5303-9 remediation)
//
// `let m : T = auto` inside a structure lowers to the SAME
// `ValueCellKind::Auto { free }` cell as `param m : T = auto`
// (`entity.rs`, the auto-let branch) — an auto LET is not a constructor
// parameter, so a view that counts every `Auto` cell as a declared param
// inflates the declared set and both ε diagnostics misfire.
//
// The two diagnostics therefore take DIFFERENT views, each safe in its own
// direction (`expr.rs`, views (a) and (b) at the ctor binder):
//   * `CtorUnknownField` suppresses on the WIDE `Param | Auto{..}` set, so
//     over-inclusion can only cost a diagnostic, never state a falsehood.
//   * `CtorArity` prints a NUMBER, so it counts `Param` cells plus only those
//     `Auto` cells whose `visibility` is `Public` — a param defaults to
//     `Public`, a plain let to `Private`.
// ---------------------------------------------------------------------------

const SRC_AUTO_LET_ONLY_POSITIONAL: &str = r#"module test.auto_let_only_positional
structure def WLet {
    let m : Length = auto
}
structure def Root {
    let x = WLet(1.0)
}
"#;

/// (a) A structure whose ONLY `Auto` cell is an auto LET declares ZERO params,
/// so a positional argument is surplus and the ceiling is `0`.
///
/// Regression pin for esc-5303-9 case (1): counting every `Auto` cell as a
/// declared param made this call silent, and the surplus argument landed in a
/// garbage `__arg0` member with no diagnostic at all. The pre-existing
/// zero-param probe uses a NON-auto `let k = 1`, which never reaches the `Auto`
/// branch, so the hole was invisible to it.
#[test]
fn positional_argument_to_an_auto_let_only_structure_emits_ctor_arity() {
    let module = compile_source_with_stdlib(SRC_AUTO_LET_ONLY_POSITIONAL);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "an auto LET declares no param, so the positional is surplus — exactly one \
         CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorArity));
    assert_eq!(
        diags[0].message, "E_CTOR_ARITY: WLet() expects at most 0 arguments, got 1",
        "the ceiling must be the DECLARED param count (0), not the `Auto` cell count"
    );
}

const SRC_AUTO_LET_PLUS_PARAM_SURPLUS: &str = r#"module test.auto_let_plus_param_surplus
structure def WLetParam {
    let m : Length = auto
    param b : Real
}
structure def Root {
    let x = WLetParam(1.0, 2.0, 3.0)
}
"#;

/// (b) An auto LET beside a real param must not inflate the reported ceiling.
/// `WLetParam` declares ONE param, so the message must say `at most 1 argument`.
///
/// Regression pin for esc-5303-9 case (2): counting the auto let made the
/// message read `expects at most 2 arguments` — a ceiling the source
/// contradicts, i.e. the same false-message defect class the declared view was
/// introduced to fix, inverted.
#[test]
fn an_auto_let_does_not_inflate_the_ctor_arity_ceiling() {
    let module = compile_source_with_stdlib(SRC_AUTO_LET_PLUS_PARAM_SURPLUS);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "exactly one CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(diags[0].code, Some(DiagnosticCode::CtorArity));
    assert_eq!(
        diags[0].message,
        "E_CTOR_ARITY: WLetParam() expects at most 1 argument, got 3",
        "the ceiling must count the PARAM only — the auto let is not a declared param"
    );
}

const SRC_AUTO_PARAM_BESIDE_AUTO_LET: &str = r#"module test.auto_param_beside_auto_let
structure def WMixed {
    let m : Length = auto
    param a : Length = auto
    param b : Real
}
structure def Root {
    let x = WMixed(1.0, 2.0, 3.0)
}
"#;

/// (c) BOTH DIRECTIONS AT ONCE: an auto PARAM still counts toward the ceiling
/// while an auto LET still does not. `WMixed` declares two params (`a`, `b`), so
/// the ceiling is `2` — not `1` (dropping the auto param, the defect the
/// declared view fixed) and not `3` (counting the auto let, the defect
/// esc-5303-9 reported).
#[test]
fn the_arity_ceiling_counts_auto_params_but_not_auto_lets() {
    let module = compile_source_with_stdlib(SRC_AUTO_PARAM_BESIDE_AUTO_LET);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "exactly one CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].message, "E_CTOR_ARITY: WMixed() expects at most 2 arguments, got 3",
        "the ceiling must be PARAM count (auto param + plain param = 2)"
    );
}

const SRC_NAMED_ARG_FOR_AUTO_LET: &str = r#"module test.named_arg_for_auto_let
structure def WLetNamed {
    let m : Length = auto
    param b : Real
}
structure def Root {
    let x = WLetNamed(m: 1.0, b: 2.0)
}
"#;

/// (d) LENIENCY, DELIBERATELY PINNED — a named argument targeting an auto LET is
/// silently accepted (pushed to `__arg{i}`), NOT reported as an unknown field.
///
/// `CtorUnknownField` suppresses on the WIDE view precisely so it can never
/// assert a falsehood: today's IR cannot tell `let m : T = auto` from
/// `priv param m : T = auto`, so firing here would risk telling an author their
/// structure "has no parameter with that name" when it visibly does. Silence is
/// leniency, not a false claim, and the residual `Auto`-slot binding gap it
/// leaves is owned by #6705.
///
/// This test pins the CURRENT behaviour so a future change to it is a deliberate
/// decision rather than an accident.
#[test]
fn named_argument_for_an_auto_let_is_leniently_accepted() {
    let module = compile_source_with_stdlib(SRC_NAMED_ARG_FOR_AUTO_LET);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "a named argument naming an `Auto` cell must stay silent — `CtorUnknownField` \
         may not claim a name the author visibly wrote is unknown, got: {diags:#?}"
    );
}

const SRC_PRIV_AUTO_PARAM_POSITIONAL: &str = r#"module test.priv_auto_param_positional
structure def WPrivAuto {
    priv param x : Length = auto
}
structure def Root {
    let y = WPrivAuto(1.0)
}
"#;

/// (e) KNOWN RESIDUAL, pinned so it is visible rather than hidden.
///
/// `priv param x : T = auto` lowers to a `Private` `Auto` cell — byte-identical
/// to a plain auto let — so the arity ceiling reads it as a let and reports `0`
/// rather than `1`. No predicate over today's IR can be right for both shapes,
/// and this reading is the one that is correct for every shape in `examples/*.ri`
/// (which contains auto lets and zero `priv param … = auto`) and the one that
/// matches `priv`'s own meaning: a private member is not part of the
/// constructor's externally-settable surface. The durable fix carries the origin
/// explicitly in the IR (see the `CtorArity` doc comment); it is an ~86-site
/// change across six crates and is tracked by its own follow-up task.
///
/// The companion direction is already covered by (d): the WIDE
/// `CtorUnknownField` view means `WPrivAuto(x: …)` stays silent, so the residual
/// is confined to the arity NUMBER and never produces a false "no parameter with
/// that name".
#[test]
fn priv_auto_param_is_read_as_an_auto_let_by_the_arity_ceiling() {
    let module = compile_source_with_stdlib(SRC_PRIV_AUTO_PARAM_POSITIONAL);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "exactly one CtorArity expected, got: {diags:#?}"
    );
    assert_eq!(
        diags[0].message,
        "E_CTOR_ARITY: WPrivAuto() expects at most 0 arguments, got 1",
        "documented residual: a `Private` `Auto` cell is read as an auto let, so the \
         ceiling understates by one. Changing this assertion means the IR gained an \
         origin discriminator — update the `CtorArity` doc comment with it."
    );
}

const SRC_PRIV_AUTO_PARAM_NAMED: &str = r#"module test.priv_auto_param_named
structure def WPrivAutoNamed {
    priv param x : Length = auto
}
structure def Root {
    let y = WPrivAutoNamed(x: 1.0mm)
}
"#;

/// (f) The residual in (e) must NOT leak into `CtorUnknownField`: a named
/// argument for a `priv param … = auto` names a parameter the author visibly
/// wrote, and the WIDE view keeps it silent.
#[test]
fn named_argument_for_a_priv_auto_param_is_not_an_unknown_field() {
    let module = compile_source_with_stdlib(SRC_PRIV_AUTO_PARAM_NAMED);
    let diags = ctor_conformance_diags(&module);
    assert!(
        diags.is_empty(),
        "the wide `CtorUnknownField` view must cover every `Auto` cell regardless of \
         visibility, got: {diags:#?}"
    );
}

// ---------------------------------------------------------------------------
// NON-PARAM MEMBERS ARE NOT CTOR-SETTABLE (esc-5303-10, suggestion A)
//
// The suite exercised the `Param` and `Auto` sides of the suppressing predicate
// heavily, but never the NEGATIVE side: a named argument whose name matches a
// member the structure genuinely has, but which is not externally settable.
// That is the same predicate axis that produced the esc-5303-9 blocking defect,
// so it is guarded explicitly here rather than left implied.
//
// The rule is keyed on the CELL KIND, not on visibility or on member existence:
// only `Param` and `Auto { .. }` cells suppress. A `Let` cell does not, at any
// visibility, and a `sub` is not a value cell at all.
// ---------------------------------------------------------------------------

const SRC_NAMED_ARG_FOR_PLAIN_LET: &str = r#"module test.named_arg_plain_let
structure def WLetMember {
    param p : Real
    let k = 1
}
structure def Root {
    let x = WLetMember(k: 2.0)
}
"#;

const SRC_NAMED_ARG_FOR_AUX_LET: &str = r#"module test.named_arg_aux_let
structure def WAuxLet {
    param p : Real
    aux let k = 1
}
structure def Root {
    let x = WAuxLet(k: 2.0)
}
"#;

const SRC_NAMED_ARG_FOR_PUB_LET: &str = r#"module test.named_arg_pub_let
structure def WPubLet {
    param p : Real
    pub let k = 1
}
structure def Root {
    let x = WPubLet(k: 2.0)
}
"#;

const SRC_NAMED_ARG_FOR_GEOMETRY_LET: &str = r#"module test.named_arg_geometry_let
structure def WGeomLet {
    param p : Real
    let g = box(1mm, 1mm, 1mm)
}
structure def Root {
    let x = WGeomLet(g: 2.0)
}
"#;

const SRC_NAMED_ARG_FOR_SUB: &str = r#"module test.named_arg_sub
structure def SubInner { param q : Real }
structure def WSubMember {
    param p : Real
    sub inner = SubInner(q: 1.0)
}
structure def Root {
    let x = WSubMember(inner: 2.0)
}
"#;

/// A named argument naming a NON-settable member is an unknown field.
///
/// Table-driven across every internal-member shape the language admits: a plain
/// `let`, an `aux let`, a `pub let`, a geometry-typed `let`, and a `sub`. All
/// five must report `CtorUnknownField` — a constructor sets parameters, and an
/// internal member is not one, however visible it is.
///
/// `pub let` is the load-bearing row. The esc-5303-9 arity fix keys on
/// `visibility`, but ONLY for `Auto` cells; a `Let` cell is excluded by KIND, so
/// making it `Public` must not smuggle it into the settable set. Without this row
/// a future simplification that hoists the visibility test out of the `Auto` arm
/// would pass the suite while silently making every `pub let` ctor-settable.
///
/// `priv let` is deliberately absent: the compiler rejects it upstream with
/// `E_PRIV_REDUNDANT` ("'let' bindings are already private to the structure
/// body"), so it is not a reachable shape and a probe for it would be asserting
/// on a compile error, not on this predicate.
#[test]
fn named_argument_for_a_non_param_member_is_an_unknown_field() {
    let cases: &[(&str, &str, &str, &str)] = &[
        ("plain let", SRC_NAMED_ARG_FOR_PLAIN_LET, "WLetMember", "k"),
        ("aux let", SRC_NAMED_ARG_FOR_AUX_LET, "WAuxLet", "k"),
        ("pub let", SRC_NAMED_ARG_FOR_PUB_LET, "WPubLet", "k"),
        (
            "geometry let",
            SRC_NAMED_ARG_FOR_GEOMETRY_LET,
            "WGeomLet",
            "g",
        ),
        ("sub component", SRC_NAMED_ARG_FOR_SUB, "WSubMember", "inner"),
    ];
    for &(label, source, ctor, field) in cases {
        let module = compile_source_with_stdlib(source);
        let diags = ctor_conformance_diags(&module);
        assert_eq!(
            diags.len(),
            1,
            "[{label}] a named arg for a non-settable member must emit exactly one \
             ctor-conformance diagnostic, got: {diags:#?}"
        );
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::CtorUnknownField),
            "[{label}] must be CtorUnknownField, not CtorArity"
        );
        assert_eq!(
            diags[0].message,
            format!(
                "E_CTOR_UNKNOWN_FIELD: unknown named argument '{field}' in call to \
                 '{ctor}'; '{ctor}' has no parameter with that name"
            ),
            "[{label}] the message must name the offending field and the ctor"
        );
    }
}

const SRC_REPEATED_UNKNOWN_NAME: &str = r#"module test.repeated_unknown_name
structure def WRepeat { param p : Real }
structure def Root {
    let x = WRepeat(labl: 1.0, labl: 2.0)
}
"#;

/// MULTIPLICITY BOUNDARY — the SAME unknown name supplied twice yields TWO
/// `CtorUnknownField` diagnostics, one per argument.
///
/// This is the deliberate consequence of the two guards having disjoint
/// domains: the pre-existing duplicate-named-arg Error runs only inside the
/// KNOWN-param branch (it fires when a param slot is already bound), so it never
/// sees a repeated unknown name. `CtorUnknownField` is per-argument by design
/// (PRD §6 C2(ii) — each typo needs its own span to be actionable), and two
/// arguments are two typos to fix.
///
/// Pinned because it is the natural place for a future "collapse duplicate
/// diagnostics" change to alter behaviour silently, and because it fixes the
/// contrast with the known-name case asserted below.
#[test]
fn the_same_unknown_named_argument_twice_emits_two_diagnostics() {
    let module = compile_source_with_stdlib(SRC_REPEATED_UNKNOWN_NAME);
    let diags = ctor_conformance_diags(&module);
    assert_eq!(
        diags.len(),
        2,
        "one CtorUnknownField per offending argument, not one per distinct name, \
         got: {diags:#?}"
    );
    for d in &diags {
        assert_eq!(d.code, Some(DiagnosticCode::CtorUnknownField));
        assert!(
            d.message.contains("unknown named argument 'labl'"),
            "each diagnostic names the repeated field, got: {:?}",
            d.message
        );
    }
    // The two diagnostics must anchor at DIFFERENT spans — that is the whole
    // reason the multiplicity is per-argument rather than per-name.
    assert_ne!(
        diags[0].labels[0].span.start, diags[1].labels[0].span.start,
        "the two diagnostics must anchor at the two distinct arguments"
    );
}

const SRC_REPEATED_KNOWN_NAME: &str = r#"module test.repeated_known_name
structure def WKnown { param p : Real }
structure def Root {
    let x = WKnown(p: 1.0, p: 2.0)
}
"#;

/// CONTRAST — a repeated KNOWN name stays the pre-existing duplicate-named-arg
/// Error and produces NO `CtorUnknownField`.
///
/// Together with the test above this pins the domain split: the duplicate guard
/// owns known names, `CtorUnknownField` owns unknown ones, and neither doubles
/// up on the other's case. Note the duplicate diagnostic carries no
/// `DiagnosticCode` at all, so it is invisible to `ctor_conformance_diags` —
/// asserted here against the raw diagnostic list.
#[test]
fn a_repeated_known_named_argument_is_a_duplicate_not_an_unknown_field() {
    let module = compile_source_with_stdlib(SRC_REPEATED_KNOWN_NAME);
    assert!(
        ctor_conformance_diags(&module).is_empty(),
        "a duplicate of a KNOWN param is not an unknown field"
    );
    let dupes: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate named argument"))
        .collect();
    assert_eq!(
        dupes.len(),
        1,
        "the pre-existing duplicate-named-arg guard must still fire exactly once, \
         got: {:#?}",
        module.diagnostics
    );
    assert_eq!(
        dupes[0].severity,
        Severity::Error,
        "the duplicate guard is a hard Error and is NOT behind the ctor-conformance \
         knob — it must not move with the δ flip"
    );
}
