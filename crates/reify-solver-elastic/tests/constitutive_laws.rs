/// Integration tests for `ConstitutiveLaw` trait and associated material types.
///
/// Convention: all D matrices use engineering-shear Voigt order
/// `[εxx, εyy, εzz, γxy, γyz, γxz]` with shear-block diagonal = G (not 2G).
///
/// See PRD `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C1/C2.
#[allow(clippy::needless_range_loop)]

// ─────────────────────────────────────────────────────────────────────────────
// Local test helpers (mirror of constitutive.rs::tests::assert_symmetric_finite)
// ─────────────────────────────────────────────────────────────────────────────

/// Assert that an N×N matrix is entry-wise finite and symmetric.
///
/// Symmetry tolerance: `|D[i][j] − D[j][i]| < 1e-9 · max(|D[i][j]|, |D[j][i]|, 1)`.
fn assert_symmetric_finite<const N: usize>(d: &[[f64; N]; N]) {
    for i in 0..N {
        for j in 0..N {
            assert!(
                d[i][j].is_finite(),
                "D[{i}][{j}] = {} is not finite",
                d[i][j]
            );
            let lhs = d[i][j];
            let rhs = d[j][i];
            let scale = lhs.abs().max(rhs.abs()).max(1.0);
            assert!(
                (lhs - rhs).abs() < 1e-9 * scale,
                "asymmetry at ({i},{j}): {lhs} vs {rhs}",
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 1: ConstitutiveLaw trait surface tests
// ─────────────────────────────────────────────────────────────────────────────

use reify_solver_elastic::{ConstitutiveLaw, IsotropicElastic};

/// Pin that `ConstitutiveLaw` is re-exported from the crate root and usable as
/// a generic bound.
#[test]
fn constitutive_law_is_re_exported_from_crate_root() {
    fn _take<T: ConstitutiveLaw>(_: &T) {}
    let mat = IsotropicElastic {
        youngs_modulus: 200e9,
        poisson_ratio: 0.3,
    };
    _take(&mat); // type-check call — ConstitutiveLaw must be in scope
}

/// `IsotropicElastic::d_matrix_local` must delegate to `d_matrix` exactly
/// (bitwise equality for all 36 entries).
#[test]
fn isotropic_elastic_implements_constitutive_law() {
    let mat = IsotropicElastic {
        youngs_modulus: 200e9,
        poisson_ratio: 0.3,
    };
    let d_trait = mat.d_matrix_local();
    let d_inherent = mat.d_matrix();
    for i in 0..6 {
        for j in 0..6 {
            assert_eq!(
                d_trait[i][j], d_inherent[i][j],
                "d_matrix_local()[{i}][{j}] != d_matrix()[{i}][{j}]"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 3: OrthotropicMaterial tests
// ─────────────────────────────────────────────────────────────────────────────

use reify_solver_elastic::OrthotropicMaterial;

/// CFRP-like constants used across several orthotropic tests.
fn cfrp_orthotropic() -> OrthotropicMaterial {
    OrthotropicMaterial {
        e1: 140e9,
        e2: 10e9,
        e3: 10e9,
        g12: 5e9,
        g13: 5e9,
        g23: 3.5e9,
        nu12: 0.3,
        nu13: 0.3,
        nu23: 0.5,
    }
}

/// Compute the closed-form orthotropic D given the 9 constants.
/// Voigt order: [εxx, εyy, εzz, γxy, γyz, γxz]
/// shear block diagonal: G12, G23, G13.
fn orthotropic_closed_form(
    e1: f64, e2: f64, e3: f64,
    g12: f64, g13: f64, g23: f64,
    nu12: f64, nu13: f64, nu23: f64,
) -> [[f64; 6]; 6] {
    // Reciprocal Poisson ratios from symmetry: νji = νij * Ej / Ei
    let nu21 = nu12 * e2 / e1;
    let nu31 = nu13 * e3 / e1;
    let nu32 = nu23 * e3 / e2;

    // Determinant Δ = 1 − ν12·ν21 − ν23·ν32 − ν31·ν13 − 2·ν21·ν32·ν13
    let delta = 1.0 - nu12 * nu21 - nu23 * nu32 - nu31 * nu13 - 2.0 * nu21 * nu32 * nu13;

    let d11 = (1.0 - nu23 * nu32) * e1 / delta;
    let d22 = (1.0 - nu13 * nu31) * e2 / delta;
    let d33 = (1.0 - nu12 * nu21) * e3 / delta;
    let d12 = (nu21 + nu23 * nu31) * e1 / delta; // = (ν12 + ν13·ν32)*E2/Δ
    let d13 = (nu31 + nu21 * nu32) * e1 / delta;
    let d23 = (nu32 + nu12 * nu31) * e2 / delta;
    let d44 = g12; // γxy
    let d55 = g23; // γyz
    let d66 = g13; // γxz

    let mut d = [[0.0_f64; 6]; 6];
    d[0][0] = d11; d[1][1] = d22; d[2][2] = d33;
    d[0][1] = d12; d[1][0] = d12;
    d[0][2] = d13; d[2][0] = d13;
    d[1][2] = d23; d[2][1] = d23;
    d[3][3] = d44; d[4][4] = d55; d[5][5] = d66;
    d
}

/// Closed-form D must match `OrthotropicMaterial::d_matrix_local` element-wise
/// within ≤1e-9 relative tolerance.
#[test]
fn orthotropic_d_matrix_local_matches_closed_form() {
    let mat = cfrp_orthotropic();
    let got = mat.d_matrix_local();
    let expected = orthotropic_closed_form(
        mat.e1, mat.e2, mat.e3,
        mat.g12, mat.g13, mat.g23,
        mat.nu12, mat.nu13, mat.nu23,
    );
    for i in 0..6 {
        for j in 0..6 {
            let exp = expected[i][j];
            let act = got[i][j];
            let scale = exp.abs().max(act.abs()).max(1.0);
            assert!(
                (act - exp).abs() <= 1e-9 * scale,
                "D[{i}][{j}]: got {act}, expected {exp}",
            );
        }
    }
}

/// D must be finite and symmetric.
#[test]
fn orthotropic_d_matrix_local_is_symmetric_finite() {
    assert_symmetric_finite(&cfrp_orthotropic().d_matrix_local());
}

/// When all constants collapse to isotropic values, OrthotropicMaterial must
/// match IsotropicElastic element-wise within 1e-9.
#[test]
fn orthotropic_reduces_to_isotropic_when_constants_collapse() {
    let e = 200e9_f64;
    let nu = 0.3_f64;
    let g = e / (2.0 * (1.0 + nu));
    let iso = IsotropicElastic { youngs_modulus: e, poisson_ratio: nu };
    let ortho = OrthotropicMaterial {
        e1: e, e2: e, e3: e,
        g12: g, g13: g, g23: g,
        nu12: nu, nu13: nu, nu23: nu,
    };
    let d_iso = iso.d_matrix();
    let d_ortho = ortho.d_matrix_local();
    for i in 0..6 {
        for j in 0..6 {
            let scale = d_iso[i][j].abs().max(d_ortho[i][j].abs()).max(1.0);
            assert!(
                (d_ortho[i][j] - d_iso[i][j]).abs() <= 1e-9 * scale,
                "D[{i}][{j}]: ortho={} iso={}", d_ortho[i][j], d_iso[i][j],
            );
        }
    }
}

/// PD violation: negative modulus must panic in debug builds.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "OrthotropicMaterial")]
fn orthotropic_panics_on_negative_modulus() {
    OrthotropicMaterial {
        e1: -1.0,
        e2: 10e9,
        e3: 10e9,
        g12: 5e9,
        g13: 5e9,
        g23: 3.5e9,
        nu12: 0.3,
        nu13: 0.3,
        nu23: 0.3,
    }
    .d_matrix_local();
}

/// PD violation: ν combo that makes Δ ≤ 0 must panic in debug builds.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "OrthotropicMaterial")]
fn orthotropic_panics_on_pd_violation() {
    // Extreme Poisson ratios that violate Δ > 0.
    // With E1=E2=E3, ν12=ν23=ν13=0.9 →
    // νji = νij, Δ = 1 - 3·(0.9)² - 2·(0.9)³ = 1 - 2.43 - 1.458 < 0.
    let e = 200e9_f64;
    let g = e / (2.0 * (1.0 + 0.9));
    OrthotropicMaterial {
        e1: e, e2: e, e3: e,
        g12: g, g13: g, g23: g,
        nu12: 0.9, nu13: 0.9, nu23: 0.9,
    }
    .d_matrix_local();
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 5: TransverseIsotropicMaterial tests
// ─────────────────────────────────────────────────────────────────────────────

use reify_solver_elastic::TransverseIsotropicMaterial;

/// `d_matrix_local` for a transversely isotropic material must equal the
/// equivalent OrthotropicMaterial specialization element-wise within 1e-9.
///
/// Note: for high E_axial/E_in_plane ratios, the PD constraint caps nu_axial
/// below `sqrt((1−nu_p²)/(2·(E_z/E_p)·(1+nu_p)))`. With E_p=10GPa,
/// E_z=140GPa, nu_p=0.3 this bound is ≈0.158, so nu_axial=0.02 is used
/// (physically realistic for CFRP transverse/axial pairing).
#[test]
fn transverse_iso_d_matrix_local_equals_orthotropic_specialization() {
    let e_p = 10e9_f64;  // in-plane Young's modulus
    let e_z = 140e9_f64; // axial Young's modulus
    let nu_p = 0.3_f64;  // in-plane Poisson's ratio
    let nu_a = 0.02_f64; // axial Poisson's ratio (physically valid for E_z >> E_p)
    let g_a = 5e9_f64;   // axial shear modulus (G13 = G23)

    let ti = TransverseIsotropicMaterial {
        e_in_plane: e_p,
        e_axial: e_z,
        nu_in_plane: nu_p,
        nu_axial: nu_a,
        g_axial: g_a,
    };

    // Equivalent OrthotropicMaterial: E1=E2=e_p, E3=e_z,
    // ν12=nu_p, ν13=ν23=nu_a, G12=e_p/(2(1+nu_p)), G13=G23=g_a.
    let g_in_plane = e_p / (2.0 * (1.0 + nu_p));
    let equiv = OrthotropicMaterial {
        e1: e_p, e2: e_p, e3: e_z,
        g12: g_in_plane, g13: g_a, g23: g_a,
        nu12: nu_p, nu13: nu_a, nu23: nu_a,
    };

    let d_ti = ti.d_matrix_local();
    let d_eq = equiv.d_matrix_local();
    for i in 0..6 {
        for j in 0..6 {
            let scale = d_ti[i][j].abs().max(d_eq[i][j].abs()).max(1.0);
            assert!(
                (d_ti[i][j] - d_eq[i][j]).abs() <= 1e-9 * scale,
                "D[{i}][{j}]: ti={} equiv={}", d_ti[i][j], d_eq[i][j],
            );
        }
    }
}

/// `TransverseIsotropicMaterial` implements `ConstitutiveLaw` (generic bound check).
#[test]
fn transverse_iso_implements_constitutive_law() {
    fn _take<T: ConstitutiveLaw>(_: &T) {}
    // nu_axial=0.02 is physically valid for E_axial/E_in_plane=14 (see above).
    let ti = TransverseIsotropicMaterial {
        e_in_plane: 10e9,
        e_axial: 140e9,
        nu_in_plane: 0.3,
        nu_axial: 0.02,
        g_axial: 5e9,
    };
    _take(&ti);
}

/// D must be finite and symmetric.
#[test]
fn transverse_iso_d_matrix_local_is_symmetric_finite() {
    // nu_axial=0.02 is physically valid for E_axial/E_in_plane=14 (see above).
    let ti = TransverseIsotropicMaterial {
        e_in_plane: 10e9,
        e_axial: 140e9,
        nu_in_plane: 0.3,
        nu_axial: 0.02,
        g_axial: 5e9,
    };
    assert_symmetric_finite(&ti.d_matrix_local());
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 7: rotate_voigt tests
// ─────────────────────────────────────────────────────────────────────────────

use reify_solver_elastic::rotate_voigt;

/// Build a 3×3 rotation matrix from an axis + angle (Rodrigues).
fn rotation_from_axis_angle(axis: [f64; 3], angle_rad: f64) -> [[f64; 3]; 3] {
    let [ux, uy, uz] = axis;
    let (s, c) = angle_rad.sin_cos();
    let t = 1.0 - c;
    [
        [t*ux*ux + c,    t*ux*uy - s*uz, t*ux*uz + s*uy],
        [t*ux*uy + s*uz, t*uy*uy + c,   t*uy*uz - s*ux],
        [t*ux*uz - s*uy, t*uy*uz + s*ux, t*uz*uz + c   ],
    ]
}

/// normalize a 3-vector
fn normalize3(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
    [v[0]/n, v[1]/n, v[2]/n]
}

/// Identity frame: `rotate_voigt` must be bitwise no-op for both isotropic and orthotropic D.
#[test]
fn rotate_voigt_identity_frame_is_bitwise_no_op() {
    let identity = [[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    let iso = IsotropicElastic { youngs_modulus: 200e9, poisson_ratio: 0.3 };
    let d_iso = iso.d_matrix_local();
    let d_rot_iso = rotate_voigt(&d_iso, &identity);
    for i in 0..6 {
        for j in 0..6 {
            assert_eq!(
                d_rot_iso[i][j], d_iso[i][j],
                "identity rotate ISO D[{i}][{j}]: expected {}, got {}",
                d_iso[i][j], d_rot_iso[i][j]
            );
        }
    }

    let ortho = cfrp_orthotropic();
    let d_ortho = ortho.d_matrix_local();
    let d_rot_ortho = rotate_voigt(&d_ortho, &identity);
    for i in 0..6 {
        for j in 0..6 {
            assert_eq!(
                d_rot_ortho[i][j], d_ortho[i][j],
                "identity rotate ORTHO D[{i}][{j}]: expected {}, got {}",
                d_ortho[i][j], d_rot_ortho[i][j]
            );
        }
    }
}

/// Rotating an isotropic D by any rotation must leave it unchanged (isotropy invariance).
#[test]
fn rotate_voigt_preserves_isotropic_d() {
    let axis = normalize3([1.0, 1.0, 1.0]);
    let angle = 30.0_f64.to_radians();
    let r = rotation_from_axis_angle(axis, angle);

    let iso = IsotropicElastic { youngs_modulus: 200e9, poisson_ratio: 0.3 };
    let d = iso.d_matrix_local();
    let d_rot = rotate_voigt(&d, &r);

    // Entries that are zero in D_iso should remain ~0 in D_rot. FP arithmetic
    // on 200 GPa inputs accumulates errors of order ε · d_max ≈ 1e-5 absolute
    // for zero entries. Use d_max * 1e-5 as the minimum scale so that zero
    // entries are tested at ~1e-14 relative to D's magnitude (not absolute 1e-9).
    let d_max: f64 = d.iter().flat_map(|r| r.iter().copied()).fold(0.0_f64, f64::max);
    for i in 0..6 {
        for j in 0..6 {
            let scale = d[i][j].abs().max(d_rot[i][j].abs()).max(d_max * 1e-5);
            assert!(
                (d_rot[i][j] - d[i][j]).abs() <= 1e-9 * scale,
                "isotropic invariance broken at D[{i}][{j}]: {} vs {}",
                d_rot[i][j], d[i][j]
            );
        }
    }
}

/// Rotating an orthotropic D by a non-trivial rotation must preserve SPD.
///
/// We verify: (1) result is symmetric within 1e-9; (2) all six eigenvalues are
/// positive. Eigenvalue positivity is checked via a simple bound: for a
/// symmetric PD matrix, all rayleigh quotients `v·Dv > 0`. We probe 12
/// random-ish unit vectors.
#[test]
fn rotate_voigt_preserves_spd_for_orthotropic() {
    let axis = normalize3([1.0, 1.0, 1.0]);
    let angle = 30.0_f64.to_radians();
    let r = rotation_from_axis_angle(axis, angle);

    let ortho = cfrp_orthotropic();
    let d = ortho.d_matrix_local();
    let d_rot = rotate_voigt(&d, &r);

    // (1) Symmetry
    assert_symmetric_finite(&d_rot);

    // (2) Positive Rayleigh quotients for a set of probe vectors
    // Use the canonical basis vectors plus some off-axis vectors.
    let probes: &[[f64; 6]] = &[
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        // off-axis
        [1.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    ];
    for (p_idx, v) in probes.iter().enumerate() {
        let dv: [f64; 6] = {
            let mut out = [0.0_f64; 6];
            for i in 0..6 {
                for j in 0..6 {
                    out[i] += d_rot[i][j] * v[j];
                }
            }
            out
        };
        let rq: f64 = v.iter().zip(dv.iter()).map(|(a, b)| a * b).sum();
        assert!(
            rq > 0.0,
            "Rayleigh quotient probe {p_idx} = {rq} ≤ 0 (D_rot is not PD)"
        );
    }
}

/// 90° rotation about z: for orthotropic with E1 ≠ E2, D_rotated[0][0] ≈ D_local[1][1]
/// and D_rotated[1][1] ≈ D_local[0][0].
#[test]
fn rotate_voigt_90deg_about_z_swaps_d11_d22_for_orthotropic() {
    // 90° about z: x → y, y → -x (active rotation of material frame)
    let r = rotation_from_axis_angle([0.0, 0.0, 1.0], 90.0_f64.to_radians());

    let ortho = cfrp_orthotropic(); // E1=140e9 ≠ E2=10e9
    let d = ortho.d_matrix_local();
    let d_rot = rotate_voigt(&d, &r);

    let tol = 1e-9;
    let scale_00 = d[0][0].abs().max(d_rot[0][0].abs()).max(1.0);
    let scale_11 = d[1][1].abs().max(d_rot[1][1].abs()).max(1.0);
    assert!(
        (d_rot[0][0] - d[1][1]).abs() <= tol * scale_00,
        "D_rot[0][0]={} should ≈ D[1][1]={}", d_rot[0][0], d[1][1]
    );
    assert!(
        (d_rot[1][1] - d[0][0]).abs() <= tol * scale_11,
        "D_rot[1][1]={} should ≈ D[0][0]={}", d_rot[1][1], d[0][0]
    );
}
