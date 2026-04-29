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
