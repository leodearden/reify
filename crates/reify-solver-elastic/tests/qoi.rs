//! Quantity-of-interest functionals (`LocalDisplacementQoi`,
//! `LocalNormalStressQoi`): contributing-set resolution, the `J(v) = gᵀv`
//! identity, and the typed C4 errors, driven through the crate-root public
//! surface only.
//!
//! PRD reference: `docs/prds/v0_6/goal-oriented-error-estimation.md` §5.1,
//! §6 C2–C6. The seam-level DWR boundary tests BT1–BT4 live in
//! `tests/dwr_boundary_tests.rs`.

use reify_solver_elastic::{
    IsotropicElastic, LocalDisplacementQoi, LocalNormalStressQoi, P1TetMeshRef, QoiError, QoiKind,
    QuantityOfInterest, point_in_tet_p1, tet_volume_p1,
};

fn dimensionless_steel_like() -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    }
}

/// f64 node coordinates of the standard 5-node, 2-tet fan fixture shared
/// with `crate::error_estimator`'s `two_tet_fan_mesh`.
///
/// Topology: tet0 = [0,1,2,3] (the canonical unit tet), tet1 = [1,2,3,4]
/// (shares face {1,2,3} with tet0).
///
/// The two tets have volumes 1/6 and 1/3 — a 1:2 ratio, NOT the equal
/// volumes `error_estimator`'s copy of this fixture claims in its doc
/// comment. That inequality is load-bearing here: it is what lets
/// `evaluate_is_the_volume_weighted_mean_over_elements_of_unequal_volume`
/// tell a volume-weighted mean apart from a plain elementwise average.
///
/// Element centroids — the coordinates every ball test is placed against:
///   tet0 → (0.25, 0.25, 0.25)
///   tet1 → (0.50, 0.50, 0.50)
fn two_tet_fan_coords() -> Vec<[f64; 3]> {
    vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ]
}

fn two_tet_fan_tets() -> Vec<[usize; 4]> {
    vec![[0, 1, 2, 3], [1, 2, 3, 4]]
}

/// A non-trivial nodal displacement field over the 2-tet fan (15 entries,
/// 3 per node). Deliberately NOT a rigid-body or zero field, so an error
/// case cannot pass for the wrong reason.
fn two_tet_fan_u() -> Vec<f64> {
    vec![
        0.00, 0.00, 0.00, // node 0
        0.07, -0.02, 0.01, // node 1
        -0.03, 0.05, 0.04, // node 2
        0.02, 0.06, -0.05, // node 3
        0.09, 0.03, 0.08, // node 4
    ]
}

/// Largest `|u|` entry of [`two_tet_fan_u`] — the fan fixture's own
/// magnitude, and hence the absolute floor [`assert_close`] uses on it.
const FAN_U_SCALE: f64 = 0.09;

/// Stress scale of the Kuhn-cube fixtures: `E · max|u| / L`, with the
/// dimensionless material's `E = 1`, the unit cube's `L = 1`, and
/// `max|u| ≤ 0.06` across both cube displacement fields. No `σ` these
/// fixtures can produce is more than an order of magnitude from it.
const CUBE_STRESS_SCALE: f64 = 0.06;

/// Assert `got` matches `want` to `rel` RELATIVE, with an absolute floor
/// of `rel · scale`.
///
/// The floor is the whole point. A relative-only bound collapses to a
/// demand for BIT equality wherever `want == 0`, and `want == 0` is not
/// hypothetical here: with `d = [0, 0, 1]` over tet0 alone, the fan
/// field's z-components (0.00, 0.01, 0.04, −0.05) cancel EXACTLY in f64
/// — `0.01 + 0.04` rounds to exactly `double(0.05)`. The two sides of
/// these comparisons are summed in different orders (`evaluate` element
/// by element, the expectation node by node), so demanding bit equality
/// of them would pass only by coincidence of rounding, test nothing
/// about the tolerance in the meantime, and turn any reordering or
/// fixture edit into a spurious failure. Flooring at the FIXTURE's own
/// magnitude keeps the bound meaningful at `want == 0` without loosening
/// it where `want` is large.
#[track_caller]
fn assert_close(got: f64, want: f64, rel: f64, scale: f64, case: &str) {
    let tol = rel * want.abs().max(scale);
    let delta = (got - want).abs();
    assert!(
        delta <= tol,
        "{case}: expected {want}, got {got} (|Δ| = {delta} exceeds the \
         tolerance {tol} = {rel} · max(|expected|, fixture scale {scale}))",
    );
}

/// Assert that BOTH `evaluate` and `dual_load` reject `qoi` with an error
/// satisfying `is_expected`, on the shared 2-tet fan fixture.
///
/// C4 requires the typed error from *both* directions — a QoI that
/// validated only in `evaluate` would hand the solver a silently zero `g`,
/// which is the failure mode the contract names explicitly. Returning
/// `Err` is what makes "never a zero `g`, never `NaN`" structural: there
/// is no `Vec<f64>` to be wrong.
///
/// The expectation is a PREDICATE rather than a `QoiError` compared with
/// `assert_eq!`, because one of the cases this helper must express is a
/// NaN `radius`: `QoiError`'s derived `PartialEq` inherits `f64`'s, and
/// `NaN != NaN`, so an equality assertion against
/// `NonPositiveRadius { radius: NaN }` could never hold however correct
/// the implementation. Callers match the variant and compare the payload
/// bitwise where it matters.
fn assert_both_directions_reject(
    qoi: &dyn QuantityOfInterest,
    case: &str,
    is_expected: impl Fn(&QoiError) -> bool,
) {
    let coords = two_tet_fan_coords();
    let tets = two_tet_fan_tets();
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = two_tet_fan_u();

    match qoi.evaluate(mesh, &mat, &u) {
        Err(e) => assert!(
            is_expected(&e),
            "{case}: evaluate returned the wrong QoiError variant: {e:?}",
        ),
        Ok(j) => panic!(
            "{case}: evaluate must return a typed error, got Ok({j}) — C4 \
             forbids resolving an unresolvable QoI",
        ),
    }
    match qoi.dual_load(mesh, &mat, &u) {
        Err(e) => assert!(
            is_expected(&e),
            "{case}: dual_load returned the wrong QoiError variant: {e:?}",
        ),
        Ok(g) => panic!(
            "{case}: dual_load must return a typed error, got Ok(g) with \
             {} entries — C4 forbids a silently zero (or any) g here",
            g.len(),
        ),
    }
}

/// Every direction/normal input that has no unit form, and so must reach
/// [`QoiError::NonNormalizableDirection`].
///
/// Shared by the displacement and normal-stress error matrices, because
/// the arm they exercise is literally shared — one `resolve`. Listing the
/// cases once is what keeps the two matrices from drifting into covering
/// different halves of the same contract.
///
/// The zero vector is C4's named case. The rest are the non-finite ones.
/// The last entry is finite componentwise but has `‖d‖²` overflow to `+inf`,
/// so it fails the same normalizability question by a different route.
fn unusable_directions() -> [[f64; 3]; 6] {
    [
        [0.0, 0.0, 0.0],
        [f64::NAN, 0.0, 0.0],
        [f64::INFINITY, 0.0, 0.0],
        [f64::NEG_INFINITY, 0.0, 0.0],
        [1.0, 0.0, f64::NAN],
        [1e200, 1e200, 1e200],
    ]
}

/// Bit patterns of a direction's three components, so a payload carrying
/// `NaN` can be compared for exact identity.
///
/// `QoiError`'s derived `PartialEq` inherits `f64`'s, under which
/// `NaN != NaN` — the same reason `assert_both_directions_reject` takes a
/// predicate rather than a `QoiError`.
fn direction_bits(direction: &[f64; 3]) -> [u64; 3] {
    [
        direction[0].to_bits(),
        direction[1].to_bits(),
        direction[2].to_bits(),
    ]
}

/// BT4 / C4 — `LocalDisplacementQoi` returns a TYPED [`QoiError`] from
/// both `evaluate` and `dual_load` for every unresolvable input, never a
/// panic and never a silently zero `g`.
///
/// Cases, per PRD §6 C4:
///
/// * `radius = 0.0`, `radius = -1.0`, `radius` non-finite (NaN, +inf) →
///   [`QoiError::NonPositiveRadius`]. §5.1 makes `radius` a *required,
///   positive* payload field; a default "small" radius was rejected
///   because it silently reintroduces the point delta whose divergence
///   §3 measured.
/// * `direction = [0,0,0]`, and `direction` with a non-finite component
///   (NaN, ±inf) → [`QoiError::NonNormalizableDirection`], each naming
///   the offending vector. Both are covered because both reach the same
///   arm and the variant's name has to be true of each: a NaN direction
///   reported as "zero length" misdescribes the input a caller is then
///   sent to debug.
/// * `at` outside every element, with a radius too small to catch any
///   centroid → [`QoiError::PointOutsideBody`].
///
/// # The non-normalizable cases pin the check ORDER
///
/// `resolve` NORMALIZES the direction, so a vector with no unit form has
/// to reach the typed error *before* that division. Reversed, C4's named
/// `direction = vec3(0,0,0)` case would come back as a `NaN`-direction
/// ball mean instead of [`QoiError::NonNormalizableDirection`] — a `NaN`
/// the module's contract says is unrepresentable. That is why the ordering
/// inside `resolve` is fixed and documented rather than incidental.
#[test]
fn local_displacement_qoi_returns_typed_error_from_both_directions_for_every_unresolvable_input()
{
    let good_at = [0.25, 0.25, 0.25];
    let good_dir = [0.0, -1.0, 0.0];

    for bad_radius in [0.0_f64, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_both_directions_reject(
            &LocalDisplacementQoi {
                at: good_at,
                radius: bad_radius,
                direction: good_dir,
            },
            &format!("radius = {bad_radius}"),
            // Bitwise payload comparison, so the NaN case pins the
            // offending value just as tightly as the finite ones.
            |e| {
                matches!(e, QoiError::NonPositiveRadius { radius }
                         if radius.to_bits() == bad_radius.to_bits())
            },
        );
    }

    for bad_direction in unusable_directions() {
        assert_both_directions_reject(
            &LocalDisplacementQoi {
                at: good_at,
                radius: 0.1,
                direction: bad_direction,
            },
            &format!("direction = {bad_direction:?}"),
            // Bitwise payload comparison, so the NaN cases pin the
            // offending vector just as tightly as the zero one.
            |e| {
                matches!(e, QoiError::NonNormalizableDirection { direction }
                         if direction_bits(direction) == direction_bits(&bad_direction))
            },
        );
    }

    let far_outside = [100.0, 100.0, 100.0];
    assert_both_directions_reject(
        &LocalDisplacementQoi {
            at: far_outside,
            radius: 0.1,
            direction: good_dir,
        },
        "at far outside the body",
        |e| *e == QoiError::PointOutsideBody { at: far_outside },
    );
}

/// The BT4 error cases above are not vacuous: a well-formed
/// `LocalDisplacementQoi` on the same fixture resolves, and its dual load
/// is non-zero.
///
/// Without this control, an implementation whose `resolve` unconditionally
/// returned `Err` would pass every assertion in the test above.
#[test]
fn local_displacement_qoi_resolves_and_produces_a_non_zero_dual_load_on_the_same_fixture() {
    let coords = two_tet_fan_coords();
    let tets = two_tet_fan_tets();
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = two_tet_fan_u();

    // Centred on tet0's centroid with a radius that catches it alone.
    let qoi = LocalDisplacementQoi {
        at: [0.25, 0.25, 0.25],
        radius: 0.1,
        direction: [0.0, -1.0, 0.0],
    };

    let j = qoi
        .evaluate(mesh, &mat, &u)
        .expect("a well-formed QoI centred on a real element centroid must resolve");
    assert!(j.is_finite(), "J(u_h) must be finite, got {j}");

    let g = qoi
        .dual_load(mesh, &mat, &u)
        .expect("a well-formed QoI must produce a dual load");
    assert_eq!(
        g.len(),
        3 * coords.len(),
        "dual load must be one entry per DOF",
    );
    assert!(
        g.iter().all(|x| x.is_finite()),
        "C4: the dual load must never contain NaN or infinity",
    );
    assert!(
        g.iter().any(|&x| x != 0.0),
        "C4: g is never zero for a non-zero direction and a non-empty \
         contributing set; got an all-zero g",
    );
}

/// `QoiKind` ships exactly the two §5.1 variants, and
/// `LocalDisplacementQoi` reports the displacement one.
///
/// "Exactly two" is asserted structurally by the wildcard-free `match`
/// below: adding a third variant makes `label` fail to compile.
/// `kind()` is what tells the result layer which `QoIEstimate` variant —
/// hence which dimension — `evaluate` returned.
///
/// There is deliberately no assertion ON `label`. Checking that a
/// function defined six lines above returns the string literal its own
/// body is written to return exercises no production code; `label` earns
/// its place through the compile-time claim, and by naming the offending
/// variant when one of the assertions below fails.
#[test]
fn qoi_kind_has_exactly_the_two_shipped_variants_and_local_displacement_reports_its_own() {
    fn label(k: QoiKind) -> &'static str {
        match k {
            QoiKind::LocalDisplacement => "LocalDisplacement",
            QoiKind::LocalNormalStress => "LocalNormalStress",
        }
    }

    let displacement = LocalDisplacementQoi {
        at: [0.25, 0.25, 0.25],
        radius: 0.1,
        direction: [0.0, -1.0, 0.0],
    };
    let stress = LocalNormalStressQoi {
        at: [0.25, 0.25, 0.25],
        radius: 0.1,
        normal: [0.0, -1.0, 0.0],
    };
    assert_eq!(
        displacement.kind(),
        QoiKind::LocalDisplacement,
        "LocalDisplacementQoi must report the displacement kind, got {}",
        label(displacement.kind()),
    );
    assert_eq!(
        stress.kind(),
        QoiKind::LocalNormalStress,
        "LocalNormalStressQoi must report the normal-stress kind, got {}",
        label(stress.kind()),
    );
}

// ── §5.1 contributing-set semantics, functional/dual algebra ───────────

/// Node indices with any non-zero DOF entry in `g`, ascending.
///
/// The dual load's *support* is how these tests pin contributing-set
/// membership without reaching into the private `resolve`: `g` is
/// accumulated node-by-node over exactly the elements of `E` with
/// strictly positive weights, so its support is the union of those
/// elements' nodes and nothing else. Pinning membership through the
/// public surface this way leaves the internal `ContributingSet` free to
/// change shape.
fn dual_load_support(g: &[f64]) -> Vec<usize> {
    assert_eq!(g.len() % 3, 0, "a dual load has 3 entries per node");
    (0..g.len() / 3)
        .filter(|&n| (0..3).any(|k| g[3 * n + k] != 0.0))
        .collect()
}

/// `(J(u_h), g)` for `qoi` on the shared 2-tet fan fixture.
fn eval_and_dual_on_fan(qoi: &dyn QuantityOfInterest, case: &str) -> (f64, Vec<f64>) {
    let coords = two_tet_fan_coords();
    let tets = two_tet_fan_tets();
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = two_tet_fan_u();
    let j = qoi
        .evaluate(mesh, &mat, &u)
        .unwrap_or_else(|e| panic!("{case}: evaluate must resolve, got {e}"));
    let g = qoi
        .dual_load(mesh, &mat, &u)
        .unwrap_or_else(|e| panic!("{case}: dual_load must resolve, got {e}"));
    (j, g)
}

/// `d · ū_K` over an explicit node list — the closed form, derived from
/// the fixture arrays alone and sharing no code with `super`.
fn direction_dot_nodal_mean(nodes: &[usize], u: &[f64], d: [f64; 3]) -> f64 {
    let mut q = 0.0_f64;
    for &n in nodes {
        for k in 0..3 {
            q += d[k] * u[3 * n + k];
        }
    }
    q / nodes.len() as f64
}

/// Euclidean distance, for stating each fixture's geometric premises as
/// assertions rather than as trusted comment arithmetic.
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The two fan-fixture element centroids, `[tet0, tet1]`.
const FAN_CENTROIDS: [[f64; 3]; 2] = [[0.25, 0.25, 0.25], [0.5, 0.5, 0.5]];

/// §5.1 / §11 Q1 — the contributing set is EXACTLY the elements whose
/// **centroids** lie within `radius` of `at`, and an element straddling
/// the sphere is in or out **wholesale**.
///
/// Centroid membership, no sphere–tet clipping: clipping was rejected at
/// v1 because it makes the contributing set a function of the sphere's
/// intersection geometry, so an arbitrarily small mesh perturbation moves
/// `J` discontinuously — and a discontinuous functional has no meaningful
/// dual.
///
/// The radius-0.2 configuration exercises BOTH halves of that rule at
/// once, and the test asserts the geometry that makes it do so rather
/// than asserting it in a comment:
///
/// * tet1 is **excluded** even though the ball reaches across the shared
///   face into tet1's interior (the ball's closest approach to the plane
///   `x+y+z=1` is ≈0.144 < 0.2, and the nearest point is the shared
///   face's own centroid) — its centroid is ≈0.433 away, so out wholesale.
/// * tet0 is **included** even though most of its volume lies outside the
///   ball (its node 0 is ≈0.433 away) — its centroid is in, so in
///   wholesale.
///
/// The 0.42/0.44 pair then brackets tet1's centroid distance from both
/// sides, pinning the membership threshold without an fp-equality
/// comparison against `0.25·√3`.
#[test]
fn contributing_set_is_the_centroid_ball_and_straddling_elements_are_in_or_out_wholesale() {
    let c0 = FAN_CENTROIDS[0];
    let c1 = FAN_CENTROIDS[1];
    let d = [0.0, -1.0, 0.0];

    // Geometric premises of the straddle configuration, asserted.
    let centroid_gap = dist(c0, c1);
    let ball_to_shared_face = dist(c0, [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0]);
    assert!(
        ball_to_shared_face < 0.2 && 0.2 < centroid_gap,
        "fixture premise: a radius of 0.2 about tet0's centroid must \
         reach into tet1 ({ball_to_shared_face}) while excluding tet1's \
         centroid ({centroid_gap})",
    );
    assert!(
        dist(c0, [0.0, 0.0, 0.0]) > 0.2,
        "fixture premise: tet0's node 0 must lie OUTSIDE the 0.2 ball, so \
         including tet0 is genuinely a wholesale inclusion",
    );

    for (radius, expected_support, why) in [
        (0.2, vec![0, 1, 2, 3], "straddle: tet1 out wholesale, tet0 in wholesale"),
        (0.42, vec![0, 1, 2, 3], "just inside tet1's centroid distance"),
        (0.44, vec![0, 1, 2, 3, 4], "just outside it — tet1 joins"),
    ] {
        let (_, g) = eval_and_dual_on_fan(
            &LocalDisplacementQoi {
                at: c0,
                radius,
                direction: d,
            },
            why,
        );
        assert_eq!(
            dual_load_support(&g),
            expected_support,
            "radius {radius} ({why}): the dual load's support must be \
             exactly the union of the contributing elements' nodes",
        );
    }
}

/// §5.1 fallback arm — when NO element centroid lies in the ball, the
/// contributing set falls back to the single element that CONTAINS `at`.
///
/// Without this arm a QoI is only addressable at radii comparable to the
/// local element size: on a mesh the refiner has just coarsened, a
/// user-specified radius that resolved fine one iteration ago suddenly
/// catches no centroid, and the QoI reports `PointOutsideBody` for a
/// point that is manifestly inside the body. The fallback makes "inside
/// the body" the real precondition, independent of mesh density.
///
/// `at = (0.05, 0.05, 0.05)` is strictly inside tet0 and strictly outside
/// tet1 (every point of tet1 has `x+y+z ≥ 1`), so the expected answer is
/// unambiguous — both assertions the test makes as premises.
#[test]
fn contributing_set_falls_back_to_the_containing_element_when_no_centroid_is_in_the_ball() {
    let coords = two_tet_fan_coords();
    let u = two_tet_fan_u();
    let at = [0.05, 0.05, 0.05];
    let radius = 0.01;
    let d = [0.0, -1.0, 0.0];

    // Premises: no centroid is in the ball, and `at` lies in tet0 alone.
    for (i, c) in FAN_CENTROIDS.iter().enumerate() {
        assert!(
            dist(at, *c) > radius,
            "fixture premise: tet{i}'s centroid must lie OUTSIDE the ball, \
             or this exercises the ball rule rather than the fallback",
        );
    }
    let tet0_nodes = [coords[0], coords[1], coords[2], coords[3]];
    let tet1_nodes = [coords[1], coords[2], coords[3], coords[4]];
    assert!(
        point_in_tet_p1(&tet0_nodes, at, 1e-9),
        "fixture premise: `at` must lie inside tet0",
    );
    assert!(
        !point_in_tet_p1(&tet1_nodes, at, 1e-9),
        "fixture premise: `at` must lie outside tet1, so the expected \
         fallback element is unambiguous",
    );

    let (j, g) = eval_and_dual_on_fan(
        &LocalDisplacementQoi {
            at,
            radius,
            direction: d,
        },
        "fallback to the containing element",
    );

    assert_eq!(
        dual_load_support(&g),
        vec![0, 1, 2, 3],
        "the fallback set is exactly {{tet0}}, so `g`'s support is tet0's \
         four nodes — node 4 belongs to tet1 alone and must stay zero",
    );
    let expected = direction_dot_nodal_mean(&[0, 1, 2, 3], &u, d);
    assert_close(
        j,
        expected,
        1e-12,
        FAN_U_SCALE,
        "a single-element contributing set makes J the plain nodal mean d·ū_0",
    );
}

/// §5.1 / §11 Q2 — on a shared face the LOWEST containing element index
/// wins, and the location tolerance is a **scale-invariant barycentric
/// slack** that is never scaled by an edge length.
///
/// `at` is the shared face's centroid, so tet0 and tet1 both contain it
/// (one barycentric coordinate is zero in each) and the tie-break is what
/// decides the answer. The support assertion discriminates cleanly:
/// tet0's nodes are `{0,1,2,3}` and tet1's are `{1,2,3,4}`, so picking
/// the wrong element swaps node 0 for node 4.
///
/// The 1000× rerun is the scale-invariance half. Barycentric coordinates
/// live in `[0,1]` for any non-degenerate tet regardless of physical
/// size, so the SAME `1e-9` must classify the same point the same way on
/// a metre-scale mesh and a kilometre-scale one. A tolerance scaled by
/// edge length would silently change the tie-break — or miss the face
/// entirely — on a rescaled model, which for a coordinate-addressed QoI
/// (C6) means the same design query answering differently in different
/// units.
#[test]
fn shared_face_point_falls_back_to_the_lowest_containing_element_at_a_scale_invariant_slack() {
    let d = [0.0, -1.0, 0.0];
    let base_at = [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0];
    let base_radius = 0.01;

    for scale in [1.0_f64, 1000.0] {
        let coords: Vec<[f64; 3]> = two_tet_fan_coords()
            .iter()
            .map(|c| [c[0] * scale, c[1] * scale, c[2] * scale])
            .collect();
        let tets = two_tet_fan_tets();
        let mesh = P1TetMeshRef {
            coords: &coords,
            tets: &tets,
        };
        let mat = dimensionless_steel_like();
        let u = two_tet_fan_u();
        let at = [base_at[0] * scale, base_at[1] * scale, base_at[2] * scale];
        let radius = base_radius * scale;

        // Premises: both elements contain `at` at the SAME unscaled
        // barycentric slack, and neither centroid is in the ball.
        let tet0_nodes = [coords[0], coords[1], coords[2], coords[3]];
        let tet1_nodes = [coords[1], coords[2], coords[3], coords[4]];
        assert!(
            point_in_tet_p1(&tet0_nodes, at, 1e-9) && point_in_tet_p1(&tet1_nodes, at, 1e-9),
            "scale {scale}: fixture premise — both elements must contain \
             the shared-face point at the unscaled slack 1e-9, or there \
             is no tie to break",
        );
        for (i, c) in FAN_CENTROIDS.iter().enumerate() {
            let scaled = [c[0] * scale, c[1] * scale, c[2] * scale];
            assert!(
                dist(at, scaled) > radius,
                "scale {scale}: fixture premise — tet{i}'s centroid must \
                 lie outside the ball so the fallback arm is what runs",
            );
        }

        let qoi = LocalDisplacementQoi {
            at,
            radius,
            direction: d,
        };
        let g = qoi
            .dual_load(mesh, &mat, &u)
            .unwrap_or_else(|e| panic!("scale {scale}: dual_load must resolve, got {e}"));
        assert_eq!(
            dual_load_support(&g),
            vec![0, 1, 2, 3],
            "scale {scale}: the LOWEST containing element index wins, so \
             the support is tet0's nodes — node 4 appearing (or node 0 \
             missing) means tet1 was chosen",
        );
    }
}

/// The algebraic identity the whole dual formulation rests on:
/// `J(v) = gᵀv` exactly, for every resolvable configuration.
///
/// This is what licenses the DWR derivation to replace `J(u − u_h)` with
/// `gᵀ(u − u_h)` and hence with the dual-weighted residual. If it held
/// only approximately, every downstream error estimate would carry an
/// unquantified modelling error on top of the discretization error it is
/// trying to measure.
///
/// Both functionals are strictly linear in `u` with no affine offset, so
/// the only defect is floating-point summation ORDER — `evaluate` sums
/// element-by-element, `gᵀu` node-by-node. That is `O(n_e · ε)` against a
/// fixture of magnitude [`FAN_U_SCALE`], so the bound carries many orders
/// of margin and would still catch any genuine algebraic discrepancy.
///
/// The bound is relative to `J` but FLOORED at that fixture scale (see
/// [`assert_close`]), because one case in the matrix below —
/// `d = [0, 0, 1]` over tet0 alone — makes `J` exactly zero. Relative
/// only, that case would demand bit equality between two summation
/// orders and assert nothing about the tolerance at all.
#[test]
fn evaluate_equals_the_dual_load_contracted_with_the_displacement_field() {
    let u = two_tet_fan_u();
    let third = 1.0 / 3.0;
    let two_thirds = 2.0 / 3.0;

    for (at, radius, label) in [
        (FAN_CENTROIDS[0], 0.2, "tet0 alone"),
        (FAN_CENTROIDS[0], 0.44, "both elements"),
        ([0.375, 0.375, 0.375], 0.25, "both, ball centred between them"),
    ] {
        for direction in [
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [third, two_thirds, two_thirds],
        ] {
            let case = format!("{label}, d = {direction:?}");
            let (j, g) = eval_and_dual_on_fan(
                &LocalDisplacementQoi {
                    at,
                    radius,
                    direction,
                },
                &case,
            );
            let contracted: f64 = g.iter().zip(&u).map(|(gi, ui)| gi * ui).sum();
            assert_close(
                contracted,
                j,
                1e-12,
                FAN_U_SCALE,
                &format!(
                    "{case}: J(u_h) = gᵀu_h must hold (evaluate is the \
                     expectation, gᵀu_h the measurement)"
                ),
            );
        }
    }
}

/// C4's positive half — `g` is never all-zero for a non-zero direction on
/// a non-empty contributing set.
///
/// The negative half (BT4 above) makes an unresolvable QoI a typed error.
/// This one closes the remaining hole: a *resolvable* QoI must not return
/// a silently zero `g` either, because `K z_h = 0` solves to the zero
/// dual field and reports an error estimate of exactly zero — i.e.
/// convergence — for every mesh.
#[test]
fn dual_load_is_never_all_zero_for_a_unit_direction_on_a_non_empty_contributing_set() {
    let third = 1.0 / 3.0;
    let two_thirds = 2.0 / 3.0;
    for (at, radius, label) in [
        (FAN_CENTROIDS[0], 0.2, "tet0 alone"),
        (FAN_CENTROIDS[0], 0.44, "both elements"),
    ] {
        for direction in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
            [third, two_thirds, two_thirds],
            [-two_thirds, third, -two_thirds],
        ] {
            let case = format!("{label}, d = {direction:?}");
            let (_, g) = eval_and_dual_on_fan(
                &LocalDisplacementQoi {
                    at,
                    radius,
                    direction,
                },
                &case,
            );
            assert!(
                g.iter().any(|&x| x != 0.0),
                "{case}: C4 — g is never zero for a non-zero direction",
            );
            assert!(
                g.iter().all(|x| x.is_finite()),
                "{case}: C4 — g never contains NaN or infinity",
            );
        }
    }
}

/// `evaluate` is the **volume-weighted** mean `Σ V_K q_K / Σ V_K`, not the
/// plain elementwise average.
///
/// The fan fixture's two tets have volumes 1/6 and 1/3 — a 1:2 ratio — so
/// this configuration discriminates: the volume-weighted answer is
/// `(q₀ + 2q₁)/3 = -0.0275` and the unweighted one is
/// `(q₀ + q₁)/2 = -0.02625`, ~4.5% apart. The test asserts that gap
/// explicitly, so a future fixture edit that equalised the volumes would
/// fail here rather than silently making the check vacuous.
///
/// Weighting matters because a refined mesh is a graded one: the elements
/// near a stress concentration are orders of magnitude smaller than those
/// away from it, so an unweighted mean over a ball would let a cloud of
/// tiny elements outvote the large element holding most of the ball's
/// volume, and `J` would drift with mesh density instead of converging.
#[test]
fn evaluate_is_the_volume_weighted_mean_over_elements_of_unequal_volume() {
    let coords = two_tet_fan_coords();
    let u = two_tet_fan_u();
    let d = [0.0, -1.0, 0.0];

    let v0 = tet_volume_p1(&[coords[0], coords[1], coords[2], coords[3]]);
    let v1 = tet_volume_p1(&[coords[1], coords[2], coords[3], coords[4]]);
    assert!(
        (v0 - 1.0 / 6.0).abs() < 1e-15 && (v1 - 1.0 / 3.0).abs() < 1e-15,
        "fixture premise: the fan's tets have volumes 1/6 and 1/3, got \
         {v0} and {v1} — equal volumes would make this test vacuous",
    );

    let q0 = direction_dot_nodal_mean(&[0, 1, 2, 3], &u, d);
    let q1 = direction_dot_nodal_mean(&[1, 2, 3, 4], &u, d);
    let weighted = (v0 * q0 + v1 * q1) / (v0 + v1);
    let unweighted = 0.5 * (q0 + q1);
    assert!(
        (weighted - unweighted).abs() > 1e-3 * weighted.abs(),
        "fixture premise: the weighted ({weighted}) and unweighted \
         ({unweighted}) means must differ materially, or this test \
         cannot detect a missing V_K",
    );

    let (j, _) = eval_and_dual_on_fan(
        &LocalDisplacementQoi {
            at: FAN_CENTROIDS[0],
            radius: 0.44,
            direction: d,
        },
        "volume-weighted mean over both elements",
    );
    assert_close(
        j,
        weighted,
        1e-15,
        FAN_U_SCALE,
        &format!(
            "J must be the volume-weighted mean (= -0.0275 by hand); the \
             unweighted mean would be {unweighted}"
        ),
    );
}


/// A contributing set with no volume is a TYPED error — in release too.
///
/// [`tet_volume_p1`] returns exactly `0.0` for four coplanar nodes, so a
/// collapsed element is reachable from real mesh data rather than only
/// from a hand-built pathology. Both ball means divide by `Σ_K V_K`, so
/// without an unconditional check `evaluate` returns `0.0/0.0 = NaN` and
/// `dual_load` an all-`NaN` `g` that the dual solve consumes as if it
/// were a load — and it was open for exactly the callers who cannot see
/// assertions.
///
/// # Exactly what the volume guards close, and what they leave open
///
/// This test and
/// `a_collapsed_element_beside_a_good_one_yields_a_typed_error` between
/// them close the EXACTLY-COLLAPSED case, `V_K == 0.0` — whether the
/// ball resolves to only such elements (here, the set-level guard) or
/// to one standing beside healthy ones (there, the per-element guard).
///
/// They do NOT make the module's "typed error, never a `NaN`" contract
/// (C4) airtight. Two cases stay open:
///
/// * **Merely tiny `det J`.** A unit tet squashed to thickness `1e-12`
///   has a strictly positive `V_K`, so it passes both volume guards, and
///   `LocalNormalStressQoi::evaluate` returns a finite but garbage value
///   on it. Finite, so not a C4 violation; garbage all the same.
///   `element_stress_p1`'s debug-only `MIN_JACOBIAN_DET` floor does not
///   catch it either: that floor is `1e-30` and this tet's `|det J|` is
///   `1e-12`, twelve orders above it.
/// * **Element INVERSION.** [`tet_volume_p1`] returns `|det| / 6`, so an
///   inverted tet reports the same volume as its good twin and is
///   indistinguishable BY VOLUME — no volume guard can see it. The value
///   it yields is finite, plausible-looking, and wrong. That is mesh
///   validity (PRD task #21), not C4.
#[test]
fn a_collapsed_element_yields_a_typed_error_rather_than_a_nan_ball_mean() {
    // Four COPLANAR nodes (every z = 0), i.e. a zero-volume "tet".
    let coords = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let tets = vec![[0, 1, 2, 3]];
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = vec![0.01_f64; 12];
    // The ball is centred ON the element centroid, so the centroid rule
    // selects it and the fallback arm never runs.
    let at = [0.5, 0.5, 0.0];

    assert_eq!(
        tet_volume_p1(&[coords[0], coords[1], coords[2], coords[3]]),
        0.0,
        "fixture premise: the element must be EXACTLY zero-volume, or \
         this exercises a merely ill-conditioned tet instead",
    );

    let expected = QoiError::DegenerateContributingSet {
        at,
        total_volume: 0.0,
    };
    let displacement = LocalDisplacementQoi {
        at,
        radius: 0.5,
        direction: [0.0, -1.0, 0.0],
    };
    assert_eq!(
        displacement.evaluate(mesh, &mat, &u),
        Err(expected.clone()),
        "evaluate must report the degenerate set, not a NaN ball mean",
    );
    assert_eq!(
        displacement.dual_load(mesh, &mat, &u),
        Err(expected.clone()),
        "dual_load must report it from its own direction too (C4), not \
         hand the solver an all-NaN g",
    );

    let stress = LocalNormalStressQoi {
        at,
        radius: 0.5,
        normal: [0.0, -1.0, 0.0],
    };
    assert_eq!(
        stress.evaluate(mesh, &mat, &u),
        Err(expected.clone()),
        "the check lives in the shared `resolve`, so the stress \
         functional reports the same error on the same mesh",
    );
    assert_eq!(
        stress.dual_load(mesh, &mat, &u),
        Err(expected),
        "and from its dual-load direction as well",
    );
}

/// A collapsed element sitting BESIDE a good one is a typed error too.
///
/// The `Σ_K V_K` guard above is a SET-level check, so a single
/// zero-volume tet in a ball that *also* catches healthy tets leaves
/// the sum strictly positive: the set passes, and every consumer then
/// goes on to index the broken element. [`element_stress_p1`]'s own
/// degenerate-Jacobian guard is a `debug_assert!`, so in RELEASE
/// `inverse_transpose_3x3` divides by `det == 0` and the `NaN` escapes
/// into the dual load.
///
/// # `LocalNormalStressQoi` is the witnessing arm
///
/// The stress functional inverts each element's Jacobian through
/// [`element_stress_p1`], whose guard is a `debug_assert!`. Without the
/// per-element guard, its RELEASE `evaluate` and `dual_load` carry
/// non-finite values into `J` and `g`, and that `g` is what
/// [`solve_dual_cg`] hands straight to CG. The displacement ball mean
/// inverts no Jacobian, so its `0.0 · finite` term genuinely vanishes and
/// it never `NaN`s — which makes a displacement-only version of this test
/// one that would pass VACUOUSLY without the guard. It is asserted anyway,
/// labelled as the non-witnessing direction, because the guard lives in the
/// shared [`resolve`] and must reject uniformly across both functionals.
///
/// A plain `#[test]` — no `should_panic`, no reliance on a debug
/// assertion — so the RELEASE leg of the verify pipeline is the one
/// that pins this; release callers are exactly the ones who cannot see
/// the `debug_assert!`. `resolve` rejects before either primitive runs,
/// so both profiles take the same path and this assertion holds in both.
#[test]
fn a_collapsed_element_beside_a_good_one_yields_a_typed_error() {
    // tet0 is the canonical unit tet (V = 1/6). tet1 shares its face
    // {0,1,2} and closes onto node 4, whose z = 0 like theirs — four
    // COPLANAR nodes, so V = 0 exactly.
    let coords = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
    ];
    let tets = vec![[0, 1, 2, 3], [0, 1, 2, 4]];
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = two_tet_fan_u();
    // Both centroids — (0.25, 0.25, 0.25) and (0.5, 0.5, 0.0) — sit
    // well inside this ball, so the centroid arm selects BOTH and the
    // `locate_containing_element` fallback never runs.
    let at = [0.4, 0.4, 0.2];
    let radius = 5.0;
    let direction = [0.0, -1.0, 0.0];

    let good = tet_volume_p1(&[coords[0], coords[1], coords[2], coords[3]]);
    let collapsed = tet_volume_p1(&[coords[0], coords[1], coords[2], coords[4]]);
    assert_eq!(
        collapsed, 0.0,
        "fixture premise: element 1 must be EXACTLY zero-volume, or \
         this exercises a merely ill-conditioned tet instead",
    );
    assert!(
        good + collapsed > 0.0,
        "fixture premise: the SET must stay viable (Σ V_K = {} must be \
         > 0), or this exercises the set-level guard rather than the \
         per-element one",
        good + collapsed,
    );

    let expected = QoiError::DegenerateElement {
        at,
        element: 1,
        volume: 0.0,
    };
    let reject = |qoi: &dyn QuantityOfInterest, arm: &str| {
        assert_eq!(
            qoi.evaluate(mesh, &mat, &u),
            Err(expected.clone()),
            "{arm}: evaluate must NAME the collapsed element rather \
             than average over it",
        );
        assert_eq!(
            qoi.dual_load(mesh, &mat, &u),
            Err(expected.clone()),
            "{arm}: dual_load must reject from its own direction too \
             (C4), not hand the solver a g carrying non-finite entries",
        );
    };
    reject(
        &LocalNormalStressQoi {
            at,
            radius,
            normal: direction,
        },
        "LocalNormalStress (the witnessing arm)",
    );
    reject(
        &LocalDisplacementQoi {
            at,
            radius,
            direction,
        },
        "LocalDisplacement (the non-witnessing arm)",
    );
}

/// The containment fallback can never select a degenerate element — a
/// RELEASE-only characterization.
///
/// The per-element guard sits where both selection arms funnel into one
/// `elements` vector, so it covers the fallback too. This test pins the
/// SEPARATE reason the fallback was already safe, so that a later edit
/// to either arm cannot widen the surface unnoticed.
///
/// With a lone collapsed element and a radius too small for the
/// centroid rule, `resolve` falls back to `locate_containing_element`.
/// [`barycentric_p1`] carries only a `debug_assert!`, so in release its
/// `det == 0` produces non-finite barycentric coordinates, every
/// [`point_in_tet_p1`] comparison against a `NaN` is false, and no
/// element is selected at all.
///
/// The `cfg` is not a convenience. In DEBUG this fixture panics inside
/// `interpolation.rs`'s guard before any `Result` exists, so no single
/// assertion is true of both profiles — and release is the profile
/// whose behaviour needs pinning anyway, being the one with no
/// assertion to fall back on.
#[cfg(not(debug_assertions))]
#[test]
fn the_containment_fallback_never_selects_a_degenerate_element() {
    let coords = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let tets = vec![[0, 1, 2, 3]];
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = vec![0.01_f64; 12];
    // The element centroid is (0.5, 0.5, 0.0); this ball misses it by
    // far more than its radius, which is what forces the fallback arm.
    let at = [0.1, 0.1, 0.0];
    let radius = 0.05;
    let direction = [0.0, -1.0, 0.0];

    let expected = QoiError::PointOutsideBody { at };
    assert_eq!(
        LocalDisplacementQoi {
            at,
            radius,
            direction,
        }
        .evaluate(mesh, &mat, &u),
        Err(expected.clone()),
        "the fallback must decline to select the degenerate element, \
         leaving `at` outside every element of this mesh",
    );
    assert_eq!(
        LocalNormalStressQoi {
            at,
            radius,
            normal: direction,
        }
        .evaluate(mesh, &mat, &u),
        Err(expected),
        "and identically for the stress functional — the fallback is \
         in the shared `resolve`",
    );
}

/// A non-unit direction states a DIRECTION, not a scale.
///
/// Nothing upstream of this module guarantees the `vec3` a designer
/// wrote was normalized, and both functionals are pure projections onto
/// it: an un-normalized direction would scale `J` AND `g` together, so
/// the resulting error estimate would be wrong by that factor while
/// still reporting as perfectly valid. `resolve` normalizes instead, so
/// the premise is discharged rather than assumed.
///
/// `[0, −2, 0]` normalizes to `[0, −1, 0]` EXACTLY in f64 — its norm is
/// a power of two — which is what lets this be a BITWISE comparison
/// rather than one carrying a tolerance that could mask a partial fix.
#[test]
fn a_non_unit_direction_resolves_to_the_same_functional_as_its_unit_form() {
    let at = FAN_CENTROIDS[0];
    let (j_unit, g_unit) = eval_and_dual_on_fan(
        &LocalDisplacementQoi {
            at,
            radius: 0.44,
            direction: [0.0, -1.0, 0.0],
        },
        "unit direction",
    );
    let (j_scaled, g_scaled) = eval_and_dual_on_fan(
        &LocalDisplacementQoi {
            at,
            radius: 0.44,
            direction: [0.0, -2.0, 0.0],
        },
        "the same direction, twice as long",
    );
    assert_eq!(
        j_scaled.to_bits(),
        j_unit.to_bits(),
        "J must be the ball mean along the direction, not one scaled by \
         ‖d‖; got {j_scaled} against {j_unit}",
    );
    assert_eq!(
        g_scaled, g_unit,
        "and the dual load must be scale-free for the same reason — a \
         scaled g solves to a scaled adjoint field",
    );

    // `n·σ·n` is QUADRATIC in ‖n‖, so an un-normalized normal would be
    // off by FOUR here rather than two.
    let u = unit_cube_nonuniform_u();
    let (s_unit, _) = eval_and_dual_on_cube(
        &LocalNormalStressQoi {
            at: [0.5, 0.5, 0.5],
            radius: 0.4,
            normal: [0.0, -1.0, 0.0],
        },
        &u,
        "unit normal",
    );
    let (s_scaled, _) = eval_and_dual_on_cube(
        &LocalNormalStressQoi {
            at: [0.5, 0.5, 0.5],
            radius: 0.4,
            normal: [0.0, -2.0, 0.0],
        },
        &u,
        "the same normal, twice as long",
    );
    assert_eq!(
        s_scaled.to_bits(),
        s_unit.to_bits(),
        "the normal-stress mean must not scale with ‖n‖² either; got \
         {s_scaled} against {s_unit}",
    );
}

// ── LocalNormalStressQoi ───────────────────────────────────────────────

/// The 8 corners of the unit cube, indexed so corner `i + 2j + 4k` sits
/// at `(i, j, k)`.
fn unit_cube_coords() -> Vec<[f64; 3]> {
    let mut c = Vec::with_capacity(8);
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..2 {
                c.push([i as f64, j as f64, k as f64]);
            }
        }
    }
    c
}

/// The unit cube Kuhn-split into 6 P1 tets, each of volume 1/6.
///
/// Each tet is one monotone corner-0-to-corner-7 path, so tet `t` is the
/// region where the coordinates are sorted in that path's order — tet 0
/// is `1 ≥ x ≥ y ≥ z ≥ 0`, tet 1 is `1 ≥ x ≥ z ≥ y ≥ 0`, and so on.
/// That makes "which element contains this point" answerable by
/// inspection, which is what the fallback configuration below relies on.
///
/// A box rather than the 2-tet fan because the normal-stress functional
/// is a *gradient* quantity: it needs elements of several distinct
/// orientations before a transposed or mis-strided extraction has
/// anywhere to hide.
fn unit_cube_kuhn_tets() -> Vec<[usize; 4]> {
    vec![
        [0, 1, 3, 7],
        [0, 1, 5, 7],
        [0, 2, 3, 7],
        [0, 2, 6, 7],
        [0, 4, 5, 7],
        [0, 4, 6, 7],
    ]
}

/// Centroid of each tet of [`unit_cube_kuhn_tets`], in element order.
///
/// All six lie at the same distance `√0.125 ≈ 0.354` from the cube
/// centre, by the symmetry of the Kuhn split.
const CUBE_CENTROIDS: [[f64; 3]; 6] = [
    [0.75, 0.5, 0.25],
    [0.75, 0.25, 0.5],
    [0.5, 0.75, 0.25],
    [0.25, 0.75, 0.5],
    [0.5, 0.25, 0.75],
    [0.25, 0.5, 0.75],
];

/// `u(x) = (a·x, 0, 0)` sampled at `coords`: the uniaxial-strain patch
/// field whose closed-form stress `element_stress_p1` already pins.
fn uniaxial_strain_u(coords: &[[f64; 3]], a: f64) -> Vec<f64> {
    let mut u = vec![0.0_f64; 3 * coords.len()];
    for (n, c) in coords.iter().enumerate() {
        u[3 * n] = a * c[0];
    }
    u
}

/// A deliberately NON-linear nodal field over the cube's 8 nodes.
///
/// Non-linear matters: a linear field has a globally constant gradient,
/// so every element sees the *same* σ, and an extraction that read the
/// wrong element — or strided node/component the wrong way within an
/// element — could still agree with `evaluate`. With this field each
/// element's σ differs, so the contraction test has something to catch.
fn unit_cube_nonuniform_u() -> Vec<f64> {
    vec![
        0.000, 0.000, 0.000, // node 0
        0.031, -0.012, 0.024, // node 1
        -0.018, 0.045, -0.007, // node 2
        0.052, 0.009, 0.038, // node 3
        0.014, -0.033, 0.021, // node 4
        -0.026, 0.017, 0.049, // node 5
        0.043, 0.028, -0.015, // node 6
        -0.009, 0.036, 0.011, // node 7
    ]
}

/// `(J(u_h), g)` for `qoi` on the Kuhn cube with the supplied field.
fn eval_and_dual_on_cube(
    qoi: &dyn QuantityOfInterest,
    u: &[f64],
    case: &str,
) -> (f64, Vec<f64>) {
    let coords = unit_cube_coords();
    let tets = unit_cube_kuhn_tets();
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let j = qoi
        .evaluate(mesh, &mat, u)
        .unwrap_or_else(|e| panic!("{case}: evaluate must resolve, got {e}"));
    let g = qoi
        .dual_load(mesh, &mat, u)
        .unwrap_or_else(|e| panic!("{case}: dual_load must resolve, got {e}"));
    (j, g)
}

/// Three ball configurations selecting very different contributing sets
/// on the Kuhn cube: `(at, radius, label)`.
///
/// All six elements; exactly one element by the centroid rule; and one
/// element via the fallback arm (`at` is strictly inside tet 0,
/// since `1 > 0.8 > 0.5 > 0.2 > 0`, and the nearest centroid is ≈0.071
/// away — well outside the 0.01 ball).
const CUBE_BALLS: [([f64; 3], f64, &str); 3] = [
    ([0.5, 0.5, 0.5], 0.4, "all six elements"),
    ([0.75, 0.5, 0.25], 0.05, "tet0 alone, by centroid"),
    ([0.8, 0.5, 0.2], 0.01, "tet0 alone, via the fallback arm"),
];

/// The Lamé constants of the shared test material, recomputed from `E`
/// and `ν` exactly as `result.rs`'s and `constitutive.rs`'s patch tests
/// do — independently of `d_matrix`, so this test can disagree with it.
fn lame() -> (f64, f64) {
    let mat = dimensionless_steel_like();
    let nu = mat.poisson_ratio;
    let factor = mat.youngs_modulus / ((1.0 + nu) * (1.0 - 2.0 * nu));
    (factor * nu, factor * (1.0 - 2.0 * nu))
}

/// On a uniform-strain field, `LocalNormalStressQoi` recovers the Lamé
/// diagonal exactly — whichever elements the ball selects.
///
/// `u(x) = (a·x, 0, 0)` gives `σ = diag((λ+2μ)a, λa, λa)` in every
/// element (`element_stress_p1`'s own patch test,
/// `element_stress_p1_uniaxial_strain_patch_test_recovers_lame_diagonal`,
/// pins that closed form and the Voigt layout behind it). A
/// volume-weighted mean of a constant is that constant, so the ball mean
/// must lift the closed form through unchanged — which is why the same
/// three expected values hold across all three contributing sets, and
/// why disagreement between the configurations would localise the bug to
/// the weighting rather than to the stress kernel.
///
/// `n·σ·n` for the three axis normals reads off the diagonal directly,
/// so this also pins that `evaluate` contracts σ with `n ⊗ n` rather
/// than, say, taking a trace or a von Mises norm.
#[test]
fn local_normal_stress_recovers_the_lame_diagonal_on_a_uniform_strain_field() {
    let a = 0.01_f64;
    let coords = unit_cube_coords();
    let u = uniaxial_strain_u(&coords, a);
    let (lambda, two_mu) = lame();

    for (at, radius, label) in CUBE_BALLS {
        for (normal, expected, which) in [
            ([1.0, 0.0, 0.0], (lambda + two_mu) * a, "σ_xx = (λ+2μ)a"),
            ([0.0, 1.0, 0.0], lambda * a, "σ_yy = λa"),
            ([0.0, 0.0, 1.0], lambda * a, "σ_zz = λa"),
        ] {
            let case = format!("{label}, {which}");
            let (j, _) = eval_and_dual_on_cube(
                &LocalNormalStressQoi {
                    at,
                    radius,
                    normal,
                },
                &u,
                &case,
            );
            assert_close(j, expected, 1e-12, CUBE_STRESS_SCALE, &case);
        }
    }
}

/// `J(v) = gᵀv` for the normal-stress functional too — the real check
/// that the twelve-unit-displacement column extraction reproduces the
/// same linear map `element_stress_p1` applies.
///
/// The extraction recovers `g`'s element row by feeding
/// `element_stress_p1` the twelve unit element displacements and
/// contracting each image with `n ⊗ n`. That is exact rather than
/// approximate only because the map really is linear with no affine
/// offset. This test is what would catch a transposed or mis-strided
/// node/component index in that probe loop: a wrong stride still
/// produces a plausible non-zero `g`, but not one whose contraction with
/// `u` reproduces `evaluate`.
///
/// Run on the NON-linear field, where each element's σ differs — see
/// [`unit_cube_nonuniform_u`].
#[test]
fn local_normal_stress_evaluate_equals_the_dual_load_contracted_with_the_field() {
    let u = unit_cube_nonuniform_u();
    let third = 1.0 / 3.0;
    let two_thirds = 2.0 / 3.0;

    for (at, radius, label) in CUBE_BALLS {
        for normal in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [third, two_thirds, two_thirds],
            [-two_thirds, third, -two_thirds],
        ] {
            let case = format!("{label}, n = {normal:?}");
            let (j, g) = eval_and_dual_on_cube(
                &LocalNormalStressQoi {
                    at,
                    radius,
                    normal,
                },
                &u,
                &case,
            );
            let contracted: f64 = g.iter().zip(&u).map(|(gi, ui)| gi * ui).sum();
            assert_close(
                contracted,
                j,
                1e-12,
                CUBE_STRESS_SCALE,
                &format!(
                    "{case}: J(u_h) = gᵀu_h must hold (evaluate is the \
                     expectation, gᵀu_h the measurement)"
                ),
            );
        }
    }
}

/// `dual_load` does not depend on `u` — BITWISE.
///
/// This is the property that lets a caller assemble `g` BEFORE the primal
/// solve, which is what makes the `f = g` self-dual fixture (BT2)
/// constructible at all: there is no chicken-and-egg between "solve for
/// `u_h`" and "build the load". `evaluate`'s `u` argument exists so a
/// linearized nonlinear functional fits the same seam later; for both
/// functionals shipped today the dual load ignores it, and the assertion
/// is bitwise rather than approximate so that a `g` computed by
/// differencing around `u` could not slip through.
#[test]
fn local_normal_stress_dual_load_is_independent_of_the_displacement_field() {
    let coords = unit_cube_coords();
    let tets = unit_cube_kuhn_tets();
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let zero = vec![0.0_f64; 3 * coords.len()];
    let nonzero = unit_cube_nonuniform_u();

    for (at, radius, label) in CUBE_BALLS {
        let qoi = LocalNormalStressQoi {
            at,
            radius,
            normal: [0.0, 1.0, 0.0],
        };
        let g_zero = qoi.dual_load(mesh, &mat, &zero).expect("resolves on zero u");
        let g_nonzero = qoi
            .dual_load(mesh, &mat, &nonzero)
            .expect("resolves on a non-zero u");
        assert_eq!(g_zero.len(), g_nonzero.len(), "{label}: length differs");
        for (i, (a, b)) in g_zero.iter().zip(&g_nonzero).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "{label}: g[{i}] depends on u ({a} vs {b}); the dual load \
                 must be assemblable before the primal solve",
            );
        }
    }
}

/// C4's positive half for the stress functional, plus `kind()`.
///
/// A silently zero `g` here would be just as fatal as for the
/// displacement functional: `K z_h = 0` solves to the zero dual field and
/// reports an error estimate of exactly zero — convergence — on every
/// mesh.
#[test]
fn local_normal_stress_dual_load_is_never_zero_and_reports_its_own_kind() {
    let u = unit_cube_nonuniform_u();
    let third = 1.0 / 3.0;
    let two_thirds = 2.0 / 3.0;

    for (at, radius, label) in CUBE_BALLS {
        for normal in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [third, two_thirds, two_thirds],
        ] {
            let case = format!("{label}, n = {normal:?}");
            let qoi = LocalNormalStressQoi {
                at,
                radius,
                normal,
            };
            assert_eq!(
                qoi.kind(),
                QoiKind::LocalNormalStress,
                "{case}: kind() is what tells the result layer which \
                 dimension evaluate() returned",
            );
            let (_, g) = eval_and_dual_on_cube(&qoi, &u, &case);
            assert!(
                g.iter().any(|&x| x != 0.0),
                "{case}: C4 — g is never zero for a non-zero normal",
            );
            assert!(
                g.iter().all(|x| x.is_finite()),
                "{case}: C4 — g never contains NaN or infinity",
            );
        }
    }
}

/// BT4 / C4 for `LocalNormalStressQoi`: the SAME typed-error arms as the
/// displacement functional, from both `evaluate` and `dual_load`.
///
/// Reusing `assert_both_directions_reject` verbatim is the point. Both
/// functionals validate through one shared `resolve`, so the contract is
/// enforced in exactly one place — and this test asserts that sameness
/// rather than restating it: if a future functional grew its own
/// validation path, only the shared helper's expectations would still
/// hold here.
///
/// A `normal` with no unit form — zero, or non-finite — maps to
/// [`QoiError::NonNormalizableDirection`], the same variant a
/// displacement direction produces: in both cases it is the vector the
/// functional projects onto that has no direction to give.
#[test]
fn local_normal_stress_qoi_returns_typed_error_from_both_directions_for_every_unresolvable_input()
{
    let good_at = [0.25, 0.25, 0.25];
    let good_normal = [1.0, 0.0, 0.0];

    for bad_radius in [0.0_f64, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_both_directions_reject(
            &LocalNormalStressQoi {
                at: good_at,
                radius: bad_radius,
                normal: good_normal,
            },
            &format!("normal-stress radius = {bad_radius}"),
            |e| {
                matches!(e, QoiError::NonPositiveRadius { radius }
                         if radius.to_bits() == bad_radius.to_bits())
            },
        );
    }

    for bad_normal in unusable_directions() {
        assert_both_directions_reject(
            &LocalNormalStressQoi {
                at: good_at,
                radius: 0.1,
                normal: bad_normal,
            },
            &format!("normal = {bad_normal:?}"),
            |e| {
                matches!(e, QoiError::NonNormalizableDirection { direction }
                         if direction_bits(direction) == direction_bits(&bad_normal))
            },
        );
    }

    let far_outside = [100.0, 100.0, 100.0];
    assert_both_directions_reject(
        &LocalNormalStressQoi {
            at: far_outside,
            radius: 0.1,
            normal: good_normal,
        },
        "normal-stress `at` far outside the body",
        |e| *e == QoiError::PointOutsideBody { at: far_outside },
    );
}


/// The three [`CUBE_BALLS`] configurations really do select three
/// different contributing sets.
///
/// Every other cube test loops over all three and expects the same
/// answer from each. That is only evidence of anything if the three
/// genuinely differ — on a uniform-strain field, in particular, σ is the
/// same in every element, so those assertions would pass unchanged even
/// if all three balls resolved to one identical set. This test is what
/// makes "a disagreement BETWEEN configurations localises the bug to the
/// weighting" a true statement about the suite rather than a hope.
///
/// Membership is probed with `LocalDisplacementQoi`, not the stress
/// functional: its `g_i = (V_K/V_E)·¼·d` is non-zero at every node of
/// every contributing element for any non-zero `d`, so its support is
/// exactly the union of those elements' nodes. The stress functional's
/// support is a SUBSET — an individual probe's contraction can vanish
/// for a particular normal and node — so it cannot pin membership. Both
/// share one `resolve`, so what is measured here holds for both.
#[test]
fn the_three_cube_ball_configurations_select_three_different_contributing_sets() {
    let coords = unit_cube_coords();
    let tets = unit_cube_kuhn_tets();
    let mesh = P1TetMeshRef {
        coords: &coords,
        tets: &tets,
    };
    let mat = dimensionless_steel_like();
    let u = unit_cube_nonuniform_u();

    // Expected support, and how many centroids the ball catches — the
    // latter is what separates "one element by the centroid rule" from
    // "one element via the fallback arm".
    let expected: [(Vec<usize>, usize); 3] = [
        ((0..8).collect(), 6),
        (vec![0, 1, 3, 7], 1),
        (vec![0, 1, 3, 7], 0),
    ];

    for ((at, radius, label), (support, n_centroids)) in CUBE_BALLS.iter().zip(expected) {
        let caught = CUBE_CENTROIDS
            .iter()
            .filter(|c| dist(*at, **c) <= *radius)
            .count();
        assert_eq!(
            caught, n_centroids,
            "{label}: expected the ball to catch {n_centroids} element \
             centroid(s), it catches {caught}",
        );

        let g = LocalDisplacementQoi {
            at: *at,
            radius: *radius,
            direction: [0.0, -1.0, 0.0],
        }
        .dual_load(mesh, &mat, &u)
        .unwrap_or_else(|e| panic!("{label}: must resolve, got {e}"));
        assert_eq!(
            dual_load_support(&g),
            support,
            "{label}: contributing set differs from the one its label claims",
        );
    }
}
