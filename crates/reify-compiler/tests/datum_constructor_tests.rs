//! Compiler typing tests for the η construction-datum constructors
//! (geometric-relations η, task 4387): `midplane` / `axis_through` /
//! `plane_through` / arity-2 `offset` / `frame_at`.
//!
//! Compiles real `.ri` snippets against the stdlib and asserts each constructor
//! call types to its datum codomain (Plane / Axis / Frame(3)), and that the
//! arity-2 `offset(Plane, Length)` construction-datum form types to `Plane` and
//! draws NO relation-policing diagnostic (it is the construction-datum `offset`,
//! NOT γ's arity-3 `offset(Plane, Plane, Length)` relation).
//!
//! Operands are param-typed datum literals (Plane / Point3<Length> / Direction)
//! so this file is independent of the `self.*` intrinsic-datum projections
//! (steps 7–8). RED until step-4 registers the datum-constructor name family in
//! `units.rs`, wires it into the `expr.rs` `NoUserFunctions` ladder, and
//! arity-gates `offset`.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::{compile_source_with_stdlib, get_let_expr};

/// Wrap `members` in a minimal `structure S { … }` and compile with the full
/// stdlib prelude (so dimensioned literals like `5mm` resolve). The
/// datum-constructor builtins are compiler-internal (the `NoUserFunctions`
/// arm), not stdlib `.ri` definitions.
fn compile_structure(members: &str) -> reify_compiler::CompiledModule {
    let source = format!("structure S {{\n{members}\n}}");
    compile_source_with_stdlib(&source)
}

// ── Construction-datum constructors type to their datum codomain ─────────────

/// `midplane(Plane, Plane) -> Plane`.
#[test]
fn midplane_types_as_plane() {
    let module = compile_structure(
        "    param pa : Plane\n    param pb : Plane\n    let m = midplane(pa, pb)\n",
    );
    assert_eq!(
        get_let_expr(&module, "m").result_type,
        Type::Plane,
        "midplane(Plane, Plane) must type as Plane"
    );
}

/// `axis_through(Point, Point) -> Axis`.
///
/// RED: until registered, `axis_through` falls to the first-arg fallback and
/// `ax` mis-types as `Point3<Length>`.
#[test]
fn axis_through_points_types_as_axis() {
    let module =
        compile_structure("    param o : Point3<Length>\n    let ax = axis_through(o, o)\n");
    assert_eq!(
        get_let_expr(&module, "ax").result_type,
        Type::Axis,
        "axis_through(Point, Point) must type as Axis"
    );
}

/// `plane_through(Point, Point, Point) -> Plane`.
///
/// RED: first-arg fallback would mis-type `pl` as `Point3<Length>`.
#[test]
fn plane_through_points_types_as_plane() {
    let module =
        compile_structure("    param o : Point3<Length>\n    let pl = plane_through(o, o, o)\n");
    assert_eq!(
        get_let_expr(&module, "pl").result_type,
        Type::Plane,
        "plane_through(Point, Point, Point) must type as Plane"
    );
}

/// `frame_at(Point, Direction, Direction) -> Frame(3)`.
///
/// RED: first-arg fallback would mis-type `f` as `Point3<Length>`.
#[test]
fn frame_at_types_as_frame() {
    let module = compile_structure(
        "    param o : Point3<Length>\n    param dx : Direction\n    \
         param dz : Direction\n    let f = frame_at(o, dx, dz)\n",
    );
    assert_eq!(
        get_let_expr(&module, "f").result_type,
        Type::Frame(3),
        "frame_at(Point, Direction, Direction) must type as Frame(3)"
    );
}

// ── offset arity overload: arity-2 is the construction datum ─────────────────

/// `offset(Plane, Length) -> Plane` — the arity-2 construction datum, NOT γ's
/// arity-3 `offset(Plane, Plane, Length)` relation.
///
/// RED: `offset` is in `RELATION_FN_NAMES`, so `relation_fn_result_type`
/// currently returns `Some(Relation)` at any arity → `s` mis-types as
/// `Relation`.
#[test]
fn offset_plane_length_types_as_plane() {
    let module = compile_structure("    param pa : Plane\n    let s = offset(pa, 5mm)\n");
    assert_eq!(
        get_let_expr(&module, "s").result_type,
        Type::Plane,
        "offset(Plane, Length) must type as Plane (construction datum, not the offset/3 relation)"
    );
}

/// The arity-2 construction-datum `offset(Plane, Length)` draws NO
/// relation-policing diagnostic. Today `relation_operand_datum("offset", …)`
/// returns `Some(Plane)` regardless of arity, so the `Length` metric in slot 1
/// trips a spurious `DatumProjectionUnavailable`.
///
/// RED until step-4 arity-gates `offset` so the relation policing fires only at
/// arity 3.
#[test]
fn offset_plane_length_draws_no_relation_policing() {
    let module = compile_structure("    param pa : Plane\n    let s = offset(pa, 5mm)\n");
    let policing: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && matches!(
                    d.code,
                    Some(DiagnosticCode::DatumProjectionUnavailable)
                        | Some(DiagnosticCode::DatumProjectionAmbiguous)
                        | Some(DiagnosticCode::ArgTypeMismatch)
                )
        })
        .collect();
    assert!(
        policing.is_empty(),
        "arity-2 offset(Plane, Length) must draw no relation-policing diagnostic; got: {:#?}",
        policing
    );
}

// ── REGRESSION: arity-3 offset is still the relation ─────────────────────────

/// The arity-3 `offset(Plane, Plane, Length)` DRIVE form is unchanged: it stays
/// `Type::Relation`. A boundary guard that must hold both before and after the
/// arity-gate.
#[test]
fn offset_arity3_still_types_as_relation() {
    let module = compile_structure(
        "    param pa : Plane\n    param pb : Plane\n    let r = offset(pa, pb, 5mm)\n",
    );
    assert_eq!(
        get_let_expr(&module, "r").result_type,
        Type::Relation,
        "offset(Plane, Plane, Length) must stay Type::Relation (arity-3 relation form)"
    );
}
