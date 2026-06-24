//! Compiler integration tests for the η `self` intrinsic-datum projections
//! (geometric-relations η, task 4387): `self.origin` / `self.frame` /
//! `self.x|y|z` / `self.xy_plane|yz_plane|zx_plane`.
//!
//! `self` types to `Type::StructureRef(entity)`; the η projection-table
//! StructureRef arm gives each intrinsic datum its codomain. The `expr.rs`
//! `self.member` dispatch consults that table only AFTER `scope.resolve(member)`
//! misses, so a user-declared param/let/sub of the same name SHADOWS the
//! intrinsic datum (design §6, reusing the STRUCTURAL_QUERY_ACCESSORS precedent).
//!
//! RED until step-8 routes intrinsic self-datums to the projection table; today
//! an unresolved `self.origin` is rejected as `unknown member 'origin' on self`.

use reify_core::{Severity, Type};
use reify_test_support::{compile_source_with_stdlib, get_let_expr};

/// Wrap `members` in a minimal `structure S { … }` and compile with the full
/// stdlib prelude (so `Point3<Length>` / `Direction` / `Plane` / `Frame`
/// annotations resolve).
fn compile_structure(members: &str) -> reify_compiler::CompiledModule {
    let source = format!("structure S {{\n{members}\n}}");
    compile_source_with_stdlib(&source)
}

/// All `Severity::Error` diagnostics — the RED signal is the `unknown member …
/// on self` poison error before step-8.
fn errors(module: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── Intrinsic self-datums type to their codomain (clean) ─────────────────────

#[test]
fn self_origin_types_as_point() {
    let module = compile_structure("    let o = self.origin\n");
    assert_eq!(
        get_let_expr(&module, "o").result_type,
        Type::point3(Type::length()),
        "self.origin must type as Point3<Length>"
    );
    assert!(
        errors(&module).is_empty(),
        "self.origin must type-check clean; got: {:#?}",
        errors(&module)
    );
}

#[test]
fn self_frame_types_as_frame() {
    let module = compile_structure("    let fr = self.frame\n");
    assert_eq!(
        get_let_expr(&module, "fr").result_type,
        Type::Frame(3),
        "self.frame must type as Frame(3)"
    );
    assert!(errors(&module).is_empty(), "self.frame must type-check clean");
}

#[test]
fn self_xy_plane_types_as_plane() {
    let module = compile_structure("    let p = self.xy_plane\n");
    assert_eq!(
        get_let_expr(&module, "p").result_type,
        Type::Plane,
        "self.xy_plane must type as Plane"
    );
    assert!(errors(&module).is_empty(), "self.xy_plane must type-check clean");
}

#[test]
fn self_yz_plane_types_as_plane() {
    let module = compile_structure("    let q = self.yz_plane\n");
    assert_eq!(
        get_let_expr(&module, "q").result_type,
        Type::Plane,
        "self.yz_plane must type as Plane"
    );
    assert!(errors(&module).is_empty(), "self.yz_plane must type-check clean");
}

#[test]
fn self_x_types_as_direction() {
    let module = compile_structure("    let dx = self.x\n");
    assert_eq!(
        get_let_expr(&module, "dx").result_type,
        Type::Direction,
        "self.x must type as Direction"
    );
    assert!(errors(&module).is_empty(), "self.x must type-check clean");
}

// ── Shadowing: a user member of the same name wins ───────────────────────────

/// A structure with `let origin = 3mm` makes `self.origin` resolve to the user
/// `let` (a `Scalar<Length>`), NOT the intrinsic `Point3<Length>` datum. This is
/// the built-in–shadowing precedent (the self-datum arm fires only after
/// `scope.resolve` misses). A guard that must hold BOTH before and after step-8.
#[test]
fn user_member_shadows_self_origin() {
    let module = compile_structure("    let origin = 3mm\n    let probe = self.origin\n");
    assert_eq!(
        get_let_expr(&module, "probe").result_type,
        Type::length(),
        "a user-declared `origin` let must shadow the intrinsic self.origin datum"
    );
    assert!(
        errors(&module).is_empty(),
        "shadowed self.origin must type-check clean; got: {:#?}",
        errors(&module)
    );
}
