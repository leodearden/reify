//! End-to-end datum-projection member-access type-checking (geometric-relations β).
//!
//! Compiles small structures whose `let`s project members off datum-typed params
//! (`param a : Axis`, `param f : Frame`, …) and asserts that:
//!   * valid projections type-check with NO error diagnostics and the projected
//!     cell carries the right codomain type (Axis.dir → Direction, etc.), and
//!   * invalid projections are rejected with the correct `DiagnosticCode`
//!     (`DatumProjectionUnavailable` for `point.dir`, `DatumProjectionAmbiguous`
//!     for the bare `frame.dir`).
//!
//! RED until step-10 wires datum projections into the `MemberAccess` arm of
//! `compile_expr_guarded`: today every `.member` on a datum receiver falls
//! through to the generic "member access not yet supported" error (code = None),
//! so the Resolved assertions fail (the cell type is `Error`, and an error
//! diagnostic is present) and the Unavailable/Ambiguous code assertions fail
//! (no datum-projection code is attached).

use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

/// The `default_expr.result_type` of the value cell named `member` — i.e. the
/// computed type of the projection initializer expression.
fn projection_type<'a>(m: &'a CompiledModule, member: &str) -> &'a Type {
    let cell = m.templates[0]
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("no value cell '{member}'"));
    &cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{member}' has no default expr"))
        .result_type
}

/// Assert the compile produced no `Severity::Error` diagnostics.
fn assert_no_errors(m: &CompiledModule, ctx: &str) {
    let errs: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "{ctx}: expected no error diagnostics, got {errs:#?}"
    );
}

/// True iff some error diagnostic carries the given datum-projection code.
fn has_error_code(m: &CompiledModule, code: DiagnosticCode) -> bool {
    m.diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

// ── (a) Axis.dir → Direction ──────────────────────────────────────────────
#[test]
fn axis_dir_projects_to_direction() {
    let m = compile_source("structure S { param a : Axis  let d : Direction = a.dir }");
    assert_no_errors(&m, "axis.dir");
    assert_eq!(
        projection_type(&m, "d"),
        &Type::Direction,
        "a.dir should have type Direction"
    );
}

// ── (b) Axis.origin → Point3<Length> ──────────────────────────────────────
#[test]
fn axis_origin_projects_to_point3_length() {
    let m = compile_source("structure S { param a : Axis  let o = a.origin }");
    assert_no_errors(&m, "axis.origin");
    assert_eq!(
        projection_type(&m, "o"),
        &Type::point3(Type::length()),
        "a.origin should have type Point3<Length>"
    );
}

// ── (c) Frame.xy_plane → Plane; Frame.z → Direction ───────────────────────
#[test]
fn frame_projections_resolve() {
    let m = compile_source(
        "structure S { param f : Frame  let pl = f.xy_plane  let fz : Direction = f.z }",
    );
    assert_no_errors(&m, "frame.xy_plane / frame.z");
    assert_eq!(
        projection_type(&m, "pl"),
        &Type::Plane,
        "f.xy_plane should have type Plane"
    );
    assert_eq!(
        projection_type(&m, "fz"),
        &Type::Direction,
        "f.z should have type Direction"
    );
}

// ── (d) Point.dir → Unavailable ───────────────────────────────────────────
#[test]
fn point_dir_is_unavailable() {
    let m = compile_source("structure S { param p : Point3<Length>  let bad = p.dir }");
    assert!(
        has_error_code(&m, DiagnosticCode::DatumProjectionUnavailable),
        "point.dir should be rejected with DatumProjectionUnavailable; got {:#?}",
        m.diagnostics
    );
}

// ── (d2) Plane.dir → Unavailable, redirects to .normal ────────────────────
#[test]
fn plane_dir_is_unavailable_and_redirects_to_normal() {
    let m = compile_source("structure S { param pl : Plane  let bad = pl.dir }");
    assert!(
        has_error_code(&m, DiagnosticCode::DatumProjectionUnavailable),
        "plane.dir should be rejected with DatumProjectionUnavailable; got {:#?}",
        m.diagnostics
    );
    let msg = m
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DatumProjectionUnavailable))
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        msg.contains("use .normal"),
        "plane.dir message should redirect to .normal; got {msg:?}"
    );
}

// ── (e) Frame.dir → Ambiguous (suggest .x/.y/.z) ──────────────────────────
#[test]
fn frame_dir_is_ambiguous() {
    let m = compile_source("structure S { param f : Frame  let amb = f.dir }");
    assert!(
        has_error_code(&m, DiagnosticCode::DatumProjectionAmbiguous),
        "frame.dir should be rejected with DatumProjectionAmbiguous; got {:#?}",
        m.diagnostics
    );
    let amb_msg = m
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DatumProjectionAmbiguous))
        .map(|d| d.message.clone())
        .unwrap_or_default();
    assert!(
        amb_msg.contains(".x") && amb_msg.contains(".y") && amb_msg.contains(".z"),
        "ambiguous message should suggest .x/.y/.z; got {amb_msg:?}"
    );
}
