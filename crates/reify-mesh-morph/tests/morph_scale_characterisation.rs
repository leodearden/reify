#[path = "calibration/fixtures.rs"]
mod fixtures;

/// The `bracket` fixture must actually reach both calibration bands this
/// whole harness rests on: ~10K tets and ~100K tets, at P1 order, with a
/// usable surface-node index vector at each scale.
///
/// Cheap and always-on: pure fixture generation (integer arithmetic +
/// vertex emission), no morph and no meshing.
///
/// Bands, not exact equality. The generator's closed-form P1 element count
/// is `tets(n) = 18n^3 + 12n^2 - 6n` for n >= 2, giving 9,936 at n=8 and
/// 108,756 at n=18. The 100K band deliberately also admits n=17's 91,800
/// so an off-by-one in the chosen n cannot doom the test.
#[test]
fn bracket_fixture_reaches_the_10k_and_100k_tet_calibration_scales() {
    use reify_ir::ElementOrderTag;

    for (n, lo, hi) in [(8usize, 9_000usize, 11_000usize), (18, 90_000, 130_000)] {
        let (mesh, surface_indices) = fixtures::bracket(1.0, 0.2, 0.05, n);

        assert_eq!(
            mesh.element_order(),
            Some(ElementOrderTag::P1),
            "bracket(n={n}) must be a P1 tet mesh — the elasticity morph and the \
             gmsh arm are both P1-only"
        );

        let tets = mesh
            .tet_indices()
            .unwrap_or_else(|| panic!("bracket(n={n}) must expose tet connectivity"))
            .len()
            / 4;
        assert!(
            (lo..=hi).contains(&tets),
            "bracket(n={n}) produced {tets} tets, outside the calibration band \
             {lo}..={hi}; closed form 18n^3 + 12n^2 - 6n predicts {}",
            18 * n * n * n + 12 * n * n - 6 * n
        );

        assert!(
            !surface_indices.is_empty(),
            "bracket(n={n}) returned an empty surface-node index vector; the morph \
             arm has no Dirichlet data without it"
        );
        let n_vertices = mesh.vertices.len() / 3;
        for &i in &surface_indices {
            assert!(
                (i as usize) < n_vertices,
                "bracket(n={n}) surface index {i} is out of range for {n_vertices} vertices"
            );
        }
    }
}
