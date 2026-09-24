//! Mesh-level Neumann load: a force resultant spread over a boundary patch.
//!
//! [`apply_patch_resultant`] turns a total force `F` on a node set into the
//! consistent P1 nodal load of the uniform traction `F / A` over the patch's
//! free (boundary) faces, `A` being their total area. Being a traction rather
//! than a per-node split, its resultant and line of action (the patch's area
//! centroid) do not depend on how the patch is triangulated — which is what an
//! adaptive loop needs when it re-derives the patch after every remesh.
//! [`free_faces_within`] is the face enumerator it is built on.
//!
//! Accumulates additively like the element-level primitives in
//! [`super::neumann`] (see the `boundary` module doc).

#[cfg(test)]
mod tests {
    use super::*;

    const F: [f64; 3] = [0.0, 0.0, -1000.0];

    /// Structured hex grid over the tick arrays, each hex split into the six
    /// tets sharing its c0→c6 diagonal — the node indexing and tets of the
    /// synthetic cantilever mesh in reify-eval's `solve_cantilever_fea`.
    /// Uneven ticks give a graded mesh.
    fn graded_box_p1_mesh(xs: &[f64], ys: &[f64], zs: &[f64]) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let (nx1, ny1) = (xs.len(), ys.len());
        let node_idx = |ix: usize, iy: usize, iz: usize| iz * ny1 * nx1 + iy * nx1 + ix;
        let mut coords = Vec::with_capacity(xs.len() * ys.len() * zs.len());
        for &z in zs {
            for &y in ys {
                for &x in xs {
                    coords.push([x, y, z]);
                }
            }
        }
        let mut tets = Vec::new();
        for hz in 0..zs.len() - 1 {
            for hy in 0..ys.len() - 1 {
                for hx in 0..xs.len() - 1 {
                    let c = [
                        node_idx(hx, hy, hz),
                        node_idx(hx + 1, hy, hz),
                        node_idx(hx + 1, hy + 1, hz),
                        node_idx(hx, hy + 1, hz),
                        node_idx(hx, hy, hz + 1),
                        node_idx(hx + 1, hy, hz + 1),
                        node_idx(hx + 1, hy + 1, hz + 1),
                        node_idx(hx, hy + 1, hz + 1),
                    ];
                    tets.extend([
                        [c[0], c[1], c[2], c[6]],
                        [c[0], c[2], c[3], c[6]],
                        [c[0], c[5], c[1], c[6]],
                        [c[0], c[3], c[7], c[6]],
                        [c[0], c[4], c[5], c[6]],
                        [c[0], c[7], c[4], c[6]],
                    ]);
                }
            }
        }
        (coords, tets)
    }

    fn nodes_where(coords: &[[f64; 3]], pred: impl Fn(&[f64; 3]) -> bool) -> Vec<usize> {
        (0..coords.len()).filter(|&n| pred(&coords[n])).collect()
    }

    fn loaded(
        coords: &[[f64; 3]],
        tets: &[[usize; 4]],
        patch: &[usize],
        resultant: [f64; 3],
    ) -> Vec<f64> {
        let mut f = vec![0.0; 3 * coords.len()];
        apply_patch_resultant(&mut f, coords, tets, patch, resultant);
        f
    }

    fn nodal(f: &[f64], n: usize) -> [f64; 3] {
        [f[3 * n], f[3 * n + 1], f[3 * n + 2]]
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn total(f: &[f64]) -> [f64; 3] {
        f.chunks_exact(3)
            .fold([0.0; 3], |s, c| [s[0] + c[0], s[1] + c[1], s[2] + c[2]])
    }

    /// Node `n`'s share of `F`: `(f_n · F) / |F|²`.
    fn share(f: &[f64], n: usize) -> f64 {
        dot(nodal(f, n), F) / dot(F, F)
    }

    /// `Σ_n s_n x_n` — the point the nodal load's moment is taken about.
    fn share_weighted_centroid(f: &[f64], coords: &[[f64; 3]]) -> [f64; 3] {
        (0..coords.len()).fold([0.0; 3], |c, n| {
            let s = share(f, n);
            [
                c[0] + s * coords[n][0],
                c[1] + s * coords[n][1],
                c[2] + s * coords[n][2],
            ]
        })
    }

    fn tri_area(coords: &[[f64; 3]], tri: &[usize; 3]) -> f64 {
        let [a, b, c] = tri.map(|n| coords[n]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        0.5 * dot(n, n).sqrt()
    }

    /// The fea_body_cantilever_adaptive.ri box (1.0 × 0.1 × 0.1 m) under three
    /// tip-face triangulations. Partition of unity makes `Σf = F` exact, and
    /// the centroid rule is exact for the linear integrand `x`, so `Σ s_i x_i`
    /// is the face's area centroid on every mesh; only fp rounding over < 100
    /// terms (~1e-14) remains against the 1e-12 bounds.
    #[test]
    fn patch_resultant_and_line_of_action_are_mesh_independent() {
        let xs = [0.0, 0.5, 1.0];
        let uniform_1x1 = [0.0, 0.1];
        let uniform_4x4 = [0.0, 0.025, 0.05, 0.075, 0.1];
        let graded_ys = [0.0, 0.005, 0.02, 0.1];
        let graded_zs = [0.0, 0.01, 0.03, 0.1];
        let cases: [(&str, &[f64], &[f64]); 3] = [
            ("uniform 1x1", &uniform_1x1, &uniform_1x1),
            ("uniform 4x4", &uniform_4x4, &uniform_4x4),
            ("graded", &graded_ys, &graded_zs),
        ];
        let face_centroid = [1.0, 0.05, 0.05];

        for (mesh, ys, zs) in cases {
            let (coords, tets) = graded_box_p1_mesh(&xs, ys, zs);
            let tip = nodes_where(&coords, |c| c[0] == 1.0);
            let f = loaded(&coords, &tets, &tip, F);

            let sum = total(&f);
            let miss = [sum[0] - F[0], sum[1] - F[1], sum[2] - F[2]];
            assert!(
                dot(miss, miss).sqrt() <= 1e-12 * dot(F, F).sqrt(),
                "{mesh}: Σf = {sum:?}, expected {F:?}",
            );
            for n in 0..coords.len() {
                let fn_ = nodal(&f, n);
                assert_eq!(
                    [fn_[0], fn_[1]],
                    [0.0, 0.0],
                    "{mesh}: f_{n} = {fn_:?} not ∥ F"
                );
            }
            let c = share_weighted_centroid(&f, &coords);
            for axis in 0..3 {
                assert!(
                    (c[axis] - face_centroid[axis]).abs() <= 1e-12,
                    "{mesh}: line of action {c:?}, face centroid {face_centroid:?}",
                );
            }
        }

        // Fixture validity: on the graded face the mean tip-node position —
        // the equal split's line of action — is far off the face centroid, so
        // this test tells the two load models apart.
        let (coords, _) = graded_box_p1_mesh(&xs, &graded_ys, &graded_zs);
        let tip = nodes_where(&coords, |c| c[0] == 1.0);
        for axis in [1, 2] {
            let mean = tip.iter().map(|&n| coords[n][axis]).sum::<f64>() / tip.len() as f64;
            assert!(
                (mean - 0.05).abs() >= 0.01,
                "graded fixture: mean tip-node coord[{axis}] = {mean} is too close to 0.05",
            );
        }
    }

    /// The centre of a 2×2 tip face touches ≥ 4 triangles (share ≥ 2A_q/3), a
    /// corner ≤ 2 (share ≤ A_q/3), whichever way the quads are split.
    #[test]
    fn patch_resultant_gives_interior_nodes_more_than_corners() {
        let ticks = [0.0, 0.05, 0.1];
        let (coords, tets) = graded_box_p1_mesh(&[0.0, 0.5, 1.0], &ticks, &ticks);
        let tip = nodes_where(&coords, |c| c[0] == 1.0);
        let f = loaded(&coords, &tets, &tip, F);

        let centre = nodes_where(&coords, |c| *c == [1.0, 0.05, 0.05]);
        let corners = nodes_where(&coords, |c| {
            c[0] == 1.0 && [0.0, 0.1].contains(&c[1]) && [0.0, 0.1].contains(&c[2])
        });
        assert_eq!((centre.len(), corners.len()), (1, 4));
        for &corner in &corners {
            assert!(
                share(&f, centre[0]) > share(&f, corner),
                "centre share {} <= corner {corner} share {}",
                share(&f, centre[0]),
                share(&f, corner),
            );
        }

        let share_sum: f64 = tip.iter().map(|&n| share(&f, n)).sum();
        assert!(
            (share_sum - 1.0).abs() <= 1e-12,
            "tip shares sum to {share_sum}"
        );
        for n in (0..coords.len()).filter(|n| !tip.contains(n)) {
            assert_eq!(nodal(&f, n), [0.0; 3], "non-patch node {n} was loaded");
        }
    }

    /// A patch covering no free-face area has no traction reading; it keeps
    /// the concentrated equal split.
    #[test]
    fn patch_resultant_on_zero_area_patch_splits_equally() {
        let ticks = [0.0, 0.05, 0.1];
        let (coords, tets) = graded_box_p1_mesh(&[0.0, 0.5, 1.0], &ticks, &ticks);
        let assert_equal_split = |patch_kind: &str, patch: &[usize]| {
            let f = loaded(&coords, &tets, patch, F);
            let per_node = F.map(|c| c / patch.len() as f64);
            for n in 0..coords.len() {
                let expected = if patch.contains(&n) {
                    per_node
                } else {
                    [0.0; 3]
                };
                assert_eq!(nodal(&f, n), expected, "{patch_kind}: node {n}");
            }
        };

        let vertex = nodes_where(&coords, |c| *c == [1.0, 0.0, 0.0]);
        assert_eq!(vertex.len(), 1);
        assert_equal_split("single vertex", &vertex);

        let edge = nodes_where(&coords, |c| c[0] == 1.0 && c[2] == 0.0);
        assert_eq!(edge.len(), 3);
        assert_equal_split("collinear tip-face edge", &edge);

        // Every triangle on x == 0.5 is shared by the tets on either side.
        let mid_plane = nodes_where(&coords, |c| c[0] == 0.5);
        assert_eq!(mid_plane.len(), 9);
        assert_equal_split("interior mid-plane", &mid_plane);
    }

    #[test]
    fn patch_resultant_is_additive_and_zero_force_is_a_noop() {
        let ticks = [0.0, 0.05, 0.1];
        let (coords, tets) = graded_box_p1_mesh(&[0.0, 0.5, 1.0], &ticks, &ticks);
        let tip = nodes_where(&coords, |c| c[0] == 1.0);
        let sentinels: Vec<f64> = (0..3 * coords.len())
            .map(|i| 1.0 + 0.25 * i as f64)
            .collect();
        let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();

        let mut f = sentinels.clone();
        apply_patch_resultant(&mut f, &coords, &tets, &tip, [0.0; 3]);
        assert_eq!(bits(&f), bits(&sentinels), "zero resultant changed f");

        let mut f = sentinels.clone();
        apply_patch_resultant(&mut f, &coords, &tets, &[], F);
        assert_eq!(bits(&f), bits(&sentinels), "empty patch changed f");

        // Per-face shares land on the sentinel instead of on 0.0, so the two
        // runs agree up to fp reassociation.
        let fresh = loaded(&coords, &tets, &tip, F);
        let mut f = sentinels.clone();
        apply_patch_resultant(&mut f, &coords, &tets, &tip, F);
        for i in 0..f.len() {
            let added = f[i] - sentinels[i];
            assert!(
                (added - fresh[i]).abs() <= 1e-12 * fresh[i].abs().max(sentinels[i]),
                "DOF {i}: added {added}, fresh {}",
                fresh[i],
            );
        }
    }

    #[test]
    fn free_faces_within_keeps_only_boundary_faces() {
        let (coords, tets) = graded_box_p1_mesh(&[0.0, 0.5, 1.0], &[0.0, 0.1], &[0.0, 0.1]);
        let area = |faces: &[[usize; 3]]| faces.iter().map(|t| tri_area(&coords, t)).sum::<f64>();

        let tip = free_faces_within(&tets, |n| coords[n][0] == 1.0);
        assert_eq!(tip.len(), 2);
        assert!(
            (area(&tip) - 0.1 * 0.1).abs() <= 1e-15,
            "tip area {}",
            area(&tip)
        );

        let surface = free_faces_within(&tets, |_| true);
        let surface_quads = 2 + 2 * 2 + 2 * 2; // one per x-end, two per ±y and ±z side
        assert_eq!(surface.len(), 2 * surface_quads);
        let surface_area = 2.0 * (0.1 * 0.1) + 4.0 * (1.0 * 0.1);
        assert!(
            (area(&surface) - surface_area).abs() <= 1e-14,
            "surface area {}, expected {surface_area}",
            area(&surface),
        );

        // Element order, never hash order: the owning tet index never decreases.
        let owner = |face: &[usize; 3]| {
            tets.iter()
                .position(|t| face.iter().all(|n| t.contains(n)))
                .expect("every free face belongs to a tet")
        };
        let owners: Vec<usize> = surface.iter().map(owner).collect();
        assert!(
            owners.is_sorted(),
            "free faces out of element order: {owners:?}"
        );
        assert_eq!(
            surface,
            free_faces_within(&tets, |_| true),
            "non-deterministic output"
        );

        assert!(free_faces_within(&tets, |n| coords[n][0] == 0.5).is_empty());
    }

    #[test]
    #[should_panic(expected = "patch node 99")]
    fn apply_patch_resultant_panics_on_out_of_range_patch_node() {
        let (coords, tets) = graded_box_p1_mesh(&[0.0, 1.0], &[0.0, 1.0], &[0.0, 1.0]);
        let mut f = vec![0.0; 3 * coords.len()];
        apply_patch_resultant(&mut f, &coords, &tets, &[99], F);
    }

    #[test]
    #[should_panic(expected = "f.len() = 10")]
    fn apply_patch_resultant_panics_on_f_len_mismatch() {
        let (coords, tets) = graded_box_p1_mesh(&[0.0, 1.0], &[0.0, 1.0], &[0.0, 1.0]);
        let mut f = vec![0.0; 10];
        apply_patch_resultant(&mut f, &coords, &tets, &[0], F);
    }
}
