//! Name-membership registry for the closed v0.1 stdlib geometry-conformance
//! marker trait set.
//!
//! This module hosts [`GEOMETRY_MARKER_TRAITS`] (the canonical seven names) and
//! the [`is_geometry_marker_trait`] predicate that queries it.  The predicate is
//! **name detection, not inference** — see task 2321 §1 for the rationale.
//!
//! Per-op trait propagation (inference) lives next door in
//! [`crate::geometry_traits_inference`].

/// The closed v0.1 set of stdlib geometry-conformance marker trait names.
///
/// These are the seven pure marker traits declared in
/// `crates/reify-compiler/stdlib/geometry_traits.ri`; the set is fixed by
/// the stdlib's `§3.10 trait-decl surface`. When a structure explicitly
/// declares one of these as a trait bound, the compiler emits a
/// `W_TRAIT_USER_ASSERTED` warning (see `DiagnosticCode::TraitUserAsserted`).
///
/// Order is stable — matches the `EXPECTED_GEOMETRY_TRAITS` fixture in
/// `crates/reify-test-support/src/fixtures.rs` so parametric tests can
/// iterate both in the same order. Case-sensitive: Reify trait names are
/// PascalCase by convention.
pub const GEOMETRY_MARKER_TRAITS: &[&str] = &[
    "Bounded",
    "Closed",
    "Manifold",
    "Orientable",
    "Convex",
    "Connected",
    "Watertight",
];

/// Returns `true` iff `name` is one of the seven stdlib geometry-conformance
/// marker trait names (case-sensitive).
///
/// This is the detection predicate used by the `entity.rs` trait_bound
/// iteration to decide whether to emit a `W_TRAIT_USER_ASSERTED` warning.
/// Detection is name-based (not qualified-trait-resolution-based) — see
/// task 2321's design decision §1 for the rationale.
pub fn is_geometry_marker_trait(name: &str) -> bool {
    GEOMETRY_MARKER_TRAITS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::{is_geometry_marker_trait, GEOMETRY_MARKER_TRAITS};

    /// `GEOMETRY_MARKER_TRAITS` must agree with the shared test-fixture
    /// `EXPECTED_GEOMETRY_TRAITS` and `is_geometry_marker_trait` must accept
    /// every name in that fixture.  Driving the assertion off
    /// `EXPECTED_GEOMETRY_TRAITS` rather than a third inline copy means any
    /// divergence between the two independently-maintained lists surfaces here
    /// rather than silently passing.
    #[test]
    fn is_geometry_marker_trait_recognises_each_of_the_seven_stdlib_names() {
        let expected = reify_test_support::EXPECTED_GEOMETRY_TRAITS;
        assert_eq!(
            GEOMETRY_MARKER_TRAITS.len(),
            expected.len(),
            "GEOMETRY_MARKER_TRAITS length mismatch against EXPECTED_GEOMETRY_TRAITS: {:?}",
            GEOMETRY_MARKER_TRAITS
        );
        for name in expected {
            assert!(
                is_geometry_marker_trait(name),
                "expected is_geometry_marker_trait({name:?}) == true, but got false"
            );
        }
    }

    /// Non-marker names — including lowercase variants — must return `false`.
    /// Case-sensitivity is by design: Reify trait names are PascalCase.
    #[test]
    fn is_geometry_marker_trait_rejects_non_marker_names() {
        let non_markers = ["Container", "Material", "Elastic", "watertight", ""];
        for name in &non_markers {
            assert!(
                !is_geometry_marker_trait(name),
                "expected is_geometry_marker_trait({name:?}) == false, but got true"
            );
        }
    }
}
