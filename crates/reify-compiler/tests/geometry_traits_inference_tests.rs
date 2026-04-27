//! Tests for `geometry_traits_inference` — the per-op trait propagation table
//! and call-site `Bounded`/`Connected`/`Convex` conformance check, implementing
//! PRD `docs/prds/geometry-traits.md` tasks 2 and 3.
//!
//! Scope: the value type `InferredTraits`, the per-`PrimitiveKind` lookup, the
//! pure propagation helpers (`combine_*`), the op-array walk
//! (`infer_traits_for_op`), the `CompiledExpr` walk (`infer_traits_for_expr`),
//! the diagnostic-shape helper, and end-to-end positive call-site behaviour
//! (`Foo(g: box(...))` with `param g : Bounded` produces no
//! `GeometryUnbounded`).
//!
//! End-to-end negative ("Unbounded source rejected at a `Bounded` slot") is
//! deferred until a `half_space` / `extrude_infinite` primitive lands — see
//! the `TODO(geometry-traits-task-4-or-later)` block in
//! `geometry_traits_inference.rs`.
//!
//! The trait-decl behaviour (refinements, `required_members`, defaults) is
//! kept in the sibling file `geometry_traits_tests.rs`; this file is reserved
//! for the inference pipeline.

use reify_compiler::geometry_traits_inference::{GeometryTrait, InferredTraits};

// ─── InferredTraits value type — flag math + has() accessor ─────────────────

#[test]
fn inferred_traits_all_has_all_three_flags_set() {
    let t = InferredTraits::all();
    assert!(t.bounded, "InferredTraits::all() must set bounded");
    assert!(t.connected, "InferredTraits::all() must set connected");
    assert!(t.convex, "InferredTraits::all() must set convex");
}

#[test]
fn inferred_traits_none_has_no_flags_set() {
    let t = InferredTraits::none();
    assert!(!t.bounded, "InferredTraits::none() must clear bounded");
    assert!(!t.connected, "InferredTraits::none() must clear connected");
    assert!(!t.convex, "InferredTraits::none() must clear convex");
}

#[test]
fn inferred_traits_bounded_only_has_only_bounded_set() {
    let t = InferredTraits::bounded_only();
    assert!(t.bounded, "InferredTraits::bounded_only() must set bounded");
    assert!(
        !t.connected,
        "InferredTraits::bounded_only() must clear connected"
    );
    assert!(
        !t.convex,
        "InferredTraits::bounded_only() must clear convex"
    );
}

#[test]
fn inferred_traits_bounded_connected_has_bounded_and_connected_set() {
    let t = InferredTraits::bounded_connected();
    assert!(
        t.bounded,
        "InferredTraits::bounded_connected() must set bounded"
    );
    assert!(
        t.connected,
        "InferredTraits::bounded_connected() must set connected"
    );
    assert!(
        !t.convex,
        "InferredTraits::bounded_connected() must clear convex"
    );
}

#[test]
fn inferred_traits_has_returns_corresponding_flag() {
    let all = InferredTraits::all();
    assert!(all.has(GeometryTrait::Bounded));
    assert!(all.has(GeometryTrait::Connected));
    assert!(all.has(GeometryTrait::Convex));

    let none = InferredTraits::none();
    assert!(!none.has(GeometryTrait::Bounded));
    assert!(!none.has(GeometryTrait::Connected));
    assert!(!none.has(GeometryTrait::Convex));

    let b_only = InferredTraits::bounded_only();
    assert!(b_only.has(GeometryTrait::Bounded));
    assert!(!b_only.has(GeometryTrait::Connected));
    assert!(!b_only.has(GeometryTrait::Convex));
}
