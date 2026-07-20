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

use reify_compiler::CompiledModule;
use reify_core::diagnostics::DiagnosticCode;
use reify_core::{Diagnostic, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only, warnings_only};

/// True when `code` is one of the diagnostic codes emitted by the struct-ctor
/// field-conformance pass (task 5302 / 4584 / 4598 / 4622 / 4444).
///
/// Filtering to this set keeps the per-fixture "exactly one diagnostic" counts
/// from being polluted by unrelated diagnostics (an incidental `W_*` warning, a
/// downstream note, etc.). All five codes already exist in `diagnostics.rs`; α
/// mints none.
fn is_ctor_conformance_code(code: Option<DiagnosticCode>) -> bool {
    matches!(
        code,
        Some(
            DiagnosticCode::ArgTypeMismatch
                | DiagnosticCode::SelectorKindMismatch
                | DiagnosticCode::TypeNotConformingToTrait
                | DiagnosticCode::TypeNotConformingToStructureRef
                | DiagnosticCode::TypeNotConformingToVector
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
