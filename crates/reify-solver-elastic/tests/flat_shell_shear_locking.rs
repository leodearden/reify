// `6 * node + axis` is the DOF-index idiom here; allowing `+ 0` keeps it visible.
#![allow(clippy::identity_op)]

//! Transverse shear of the flat-facet shell elements on triangles whose
//! in-plane metric is not the identity, and the thickness scaling it implies on
//! a structured cantilever.
//!
//! - Element section: rigid-body null space, constant-shear energy and
//!   constant-shear stress recovery on non-unit triangles, for bare MITC3
//!   (`shell_element_stiffness`) and MITC3+ (`shell_element_stiffness_mitc3_plus`).
//! - System section: a structured cantilever plate built from the public
//!   element seams, mirroring the `solve_flat_plate_shell` recipe (the driver
//!   returns no displacement), solved with each flat kernel. A locking-free
//!   element follows Euler-Bernoulli (`u ∝ 1/t³`, cubic profile); a
//!   shear-locked one collapses as `u ∝ 1/t` with a linear profile.

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgSolverOptions, DirichletBc, ElementStiffness,
    IsotropicElastic, SolverMode, SupportBodyKind, SupportKind, apply_dirichlet_row_elimination,
    assemble_global_stiffness, build_support_bcs, shell_element_stiffness,
    shell_element_stiffness_mitc3_plus, shell_element_stress, solve_cg,
};

// ─── element section ─────────────────────────────────────────────────────────

const NDOF: usize = 18;
const KAPPA: f64 = 5.0 / 6.0;
const ELEMENT_THICKNESS: f64 = 0.05;

struct Triangle {
    name: &'static str,
    nodes: [[f64; 3]; 3],
    area: f64,
}

// xy-plane triangles whose first edge runs along +x, so the local shell frame is
// the global frame and local DOFs equal global DOFs.
const DISTORTED: Triangle = Triangle {
    name: "DISTORTED",
    nodes: [[0.0, 0.0, 0.0], [1.3, 0.0, 0.0], [0.4, 0.9, 0.0]],
    area: 0.585,
};
const WIDE: Triangle = Triangle {
    name: "WIDE",
    nodes: [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.5, 0.0]],
    area: 1.5,
};
const PANEL_CELL: Triangle = Triangle {
    name: "PANEL_CELL",
    nodes: [[0.0, 0.0, 0.0], [0.055, 0.0, 0.0], [0.055, 0.05625, 0.0]],
    area: 1.546875e-3,
};
const NON_UNIT_TRIANGLES: [Triangle; 3] = [DISTORTED, WIDE, PANEL_CELL];

type FlatKernel = fn(&[[f64; 3]; 3], f64, &IsotropicElastic) -> ElementStiffness;
const FLAT_KERNELS: [(&str, FlatKernel); 2] = [
    ("bare MITC3", shell_element_stiffness),
    ("MITC3+", shell_element_stiffness_mitc3_plus),
];

fn element_material() -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: 200e9,
        poisson_ratio: 0.3,
    }
}

fn matvec(k: &ElementStiffness, u: &[f64; NDOF]) -> [f64; NDOF] {
    std::array::from_fn(|i| (0..NDOF).map(|j| k.get(i, j) * u[j]).sum())
}

fn linf(v: &[f64]) -> f64 {
    v.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
}

fn uniform_rotation(theta_x: f64, theta_y: f64) -> [f64; NDOF] {
    let mut u = [0.0_f64; NDOF];
    for node in 0..3 {
        u[6 * node + 3] = theta_x;
        u[6 * node + 4] = theta_y;
    }
    u
}

/// Three unit translations, then the rotations `u_i = ω × (x_i − c)`, `θ_i = ω`
/// about the centroid for `ω = e_x, e_y, e_z`.
fn rigid_body_modes(nodes: &[[f64; 3]; 3]) -> Vec<[f64; NDOF]> {
    let c: [f64; 3] = std::array::from_fn(|a| (nodes[0][a] + nodes[1][a] + nodes[2][a]) / 3.0);
    let mut modes = Vec::with_capacity(6);
    for axis in 0..3 {
        let mut u = [0.0_f64; NDOF];
        for node in 0..3 {
            u[6 * node + axis] = 1.0;
        }
        modes.push(u);
    }
    for axis in 0..3 {
        let mut w = [0.0_f64; 3];
        w[axis] = 1.0;
        let mut u = [0.0_f64; NDOF];
        for (node, p) in nodes.iter().enumerate() {
            let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
            u[6 * node + 0] = w[1] * d[2] - w[2] * d[1];
            u[6 * node + 1] = w[2] * d[0] - w[0] * d[2];
            u[6 * node + 2] = w[0] * d[1] - w[1] * d[0];
            u[6 * node + 3..6 * node + 6].copy_from_slice(&w);
        }
        modes.push(u);
    }
    modes
}

#[test]
fn flat_shell_elements_annihilate_rigid_body_modes_on_non_unit_triangles() {
    let mat = element_material();
    for tri in &NON_UNIT_TRIANGLES {
        for (kernel_name, kernel) in FLAT_KERNELS {
            let k = kernel(&tri.nodes, ELEMENT_THICKNESS, &mat);
            let k_max = linf(&k.data);
            for (mode_idx, mode) in rigid_body_modes(&tri.nodes).iter().enumerate() {
                let residual = linf(&matvec(&k, mode));
                let tol = 1e-9 * k_max * linf(mode);
                assert!(
                    residual < tol,
                    "{kernel_name} on {}: rigid mode {mode_idx} (0-2 translations, 3-5 \
                     rotations about x/y/z) gives ‖K·u‖∞ = {residual:.4e}, tol {tol:.4e} \
                     (rel {:.4e})",
                    tri.name,
                    residual / (k_max * linf(mode)),
                );
            }
        }
    }
}

#[test]
fn flat_shell_elements_reproduce_constant_transverse_shear_energy_on_non_unit_triangles() {
    let mat = element_material();
    let kappa_g = KAPPA * mat.shear_modulus();
    let theta = 0.003_f64;
    for tri in &NON_UNIT_TRIANGLES {
        for (kernel_name, kernel) in FLAT_KERNELS {
            let k = kernel(&tri.nodes, ELEMENT_THICKNESS, &mat);
            for (label, u) in [
                ("uniform θ_y", uniform_rotation(0.0, theta)),
                ("uniform θ_x", uniform_rotation(theta, 0.0)),
            ] {
                let ku = matvec(&k, &u);
                let energy = 0.5 * ku.iter().zip(&u).map(|(a, b)| a * b).sum::<f64>();
                let expected = 0.5 * theta * theta * kappa_g * ELEMENT_THICKNESS * tri.area;
                let rel = (energy - expected).abs() / expected;
                assert!(
                    rel < 1e-9,
                    "{kernel_name} on {} under {label}: ½uᵀKu = {energy:.6e}, \
                     ½θ²κGtA = {expected:.6e} (ratio {:.6}, rel err {rel:.3e})",
                    tri.name,
                    energy / expected,
                );
            }
        }
    }
}

#[test]
fn shell_element_stress_recovers_constant_transverse_shear_on_non_unit_triangle() {
    let mat = element_material();
    let kappa_g = KAPPA * mat.shear_modulus();
    let theta = 0.003_f64;
    let scale = kappa_g * theta;
    // (θ_x, θ_y) → physical shear (γ_xz, γ_yz) = (θ_y, −θ_x).
    let cases = [((0.0, theta), [scale, 0.0]), ((theta, 0.0), [0.0, -scale])];
    for tri in [&WIDE, &DISTORTED] {
        for ((theta_x, theta_y), [s_xz, s_yz]) in cases {
            let u = uniform_rotation(theta_x, theta_y);
            let stress = shell_element_stress(&tri.nodes, ELEMENT_THICKNESS, &mat, &u);
            for (layer, m) in [
                ("top", stress.top),
                ("mid", stress.mid),
                ("bottom", stress.bottom),
            ] {
                let got = [m[0][2], m[2][0], m[1][2], m[2][1]];
                let want = [s_xz, s_xz, s_yz, s_yz];
                let err = got
                    .iter()
                    .zip(&want)
                    .fold(0.0_f64, |e, (g, w)| e.max((g - w).abs()));
                assert!(
                    err < 1e-9 * scale,
                    "{} {layer} under θ = (θ_x={theta_x}, θ_y={theta_y}): \
                     [σ_xz, σ_zx, σ_yz, σ_zy] = {got:?}, expected {want:?} \
                     (max err / κGθ = {:.3e})",
                    tri.name,
                    err / scale,
                );
            }
        }
    }
}

// ─── system section: structured cantilever ───────────────────────────────────

const CANTILEVER_NX: usize = 20;

struct Cantilever {
    length: f64,
    width: f64,
    thickness: f64,
    tip_force: f64,
}

/// Downward deflection (`−u_z`) of every node of the cantilever's structured grid.
struct CantileverDeflection {
    nx: usize,
    ny: usize,
    deflection: Vec<f64>,
}

impl CantileverDeflection {
    fn at(&self, ix: usize, iy: usize) -> f64 {
        self.deflection[iy * (self.nx + 1) + ix]
    }

    fn mid_width(&self, ix: usize) -> f64 {
        0.5 * (self.at(ix, self.ny / 2) + self.at(ix, self.ny.div_ceil(2)))
    }

    fn tip_row(&self) -> Vec<f64> {
        (0..=self.ny).map(|iy| self.at(self.nx, iy)).collect()
    }
}

fn cantilever_material() -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: 205e9,
        poisson_ratio: 0.29,
    }
}

impl Cantilever {
    /// Euler-Bernoulli tip deflection `PL³/(3EI)` with `I = W·t³/12`.
    fn euler_bernoulli_tip(&self, mat: &IsotropicElastic) -> f64 {
        let second_moment = self.width * self.thickness.powi(3) / 12.0;
        self.tip_force * self.length.powi(3) / (3.0 * mat.youngs_modulus * second_moment)
    }

    /// Structured CCW grid, root `x = 0` clamped, θ_z pinned elsewhere, and the
    /// `−Z` tip force as a consistent traction over the last element column —
    /// the `solve_flat_plate_shell` recipe, solved with the flat `kernel`.
    fn solve(&self, mat: &IsotropicElastic, kernel: FlatKernel) -> CantileverDeflection {
        let nx = CANTILEVER_NX;
        let ny = ((nx as f64 * self.width / self.length).round() as usize).max(2);
        let nx1 = nx + 1;
        let n_nodes = nx1 * (ny + 1);
        let node = |ix: usize, iy: usize| iy * nx1 + ix;

        let nodes: Vec<[f64; 3]> = (0..=ny)
            .flat_map(|iy| {
                (0..=nx).map(move |ix| {
                    [
                        ix as f64 * self.length / nx as f64,
                        iy as f64 * self.width / ny as f64,
                        0.0,
                    ]
                })
            })
            .collect();
        let cell_triangles = |ix: usize, iy: usize| {
            let (a, b) = (node(ix, iy), node(ix + 1, iy));
            let (c, d) = (node(ix, iy + 1), node(ix + 1, iy + 1));
            [[a, b, d], [a, d, c]]
        };
        let connectivity: Vec<[usize; 3]> = (0..ny)
            .flat_map(|iy| (0..nx).flat_map(move |ix| cell_triangles(ix, iy)))
            .collect();

        let stiffness: Vec<ElementStiffness> = connectivity
            .iter()
            .map(|c| {
                let tri = [nodes[c[0]], nodes[c[1]], nodes[c[2]]];
                kernel(&tri, self.thickness, mat)
            })
            .collect();
        let elements: Vec<AssemblyElement<'_>> = connectivity
            .iter()
            .zip(&stiffness)
            .enumerate()
            .map(|(id, (c, k_e))| AssemblyElement {
                id,
                connectivity: c.as_slice(),
                k_e,
            })
            .collect();
        let mut k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        let root: Vec<usize> = (0..=ny).map(|iy| node(0, iy)).collect();
        let (mut bcs, _) = build_support_bcs(&root, SupportKind::Fixed, SupportBodyKind::Shell);
        bcs.extend((0..n_nodes).filter(|n| n % nx1 != 0).map(|n| DirichletBc {
            dof: 6 * n + 5,
            value: 0.0,
        }));

        let mut f = vec![0.0_f64; 6 * n_nodes];
        let per_vertex = -self.tip_force / (6.0 * ny as f64);
        for iy in 0..ny {
            for tri in cell_triangles(nx - 1, iy) {
                for n in tri {
                    f[6 * n + 2] += per_vertex;
                }
            }
        }
        apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

        let opts = CgSolverOptions {
            tolerance: 1e-10,
            max_iter: 50_000,
        };
        let cg = solve_cg(&k, &f, opts, SolverMode::Deterministic);
        assert!(
            cg.converged,
            "cantilever CG (t = {}) did not converge in {} iterations",
            self.thickness, cg.iterations,
        );
        CantileverDeflection {
            nx,
            ny,
            deflection: (0..n_nodes).map(|n| -cg.u()[6 * n + 2]).collect(),
        }
    }
}

#[test]
fn flat_shell_cantilever_panel_deflection_is_thickness_invariant() {
    let mat = cantilever_material();
    let thicknesses = [0.132, 0.066, 0.033, 0.0165, 0.00825];
    let mut tip_ratios_by_kernel = Vec::with_capacity(FLAT_KERNELS.len());
    for (kernel_name, kernel) in FLAT_KERNELS {
        let mut tip_ratios = Vec::with_capacity(thicknesses.len());
        for thickness in thicknesses {
            let panel = Cantilever {
                length: 1.1,
                width: 0.9,
                thickness,
                tip_force: 1000.0,
            };
            let solved = panel.solve(&mat, kernel);
            let tip = solved.mid_width(solved.nx);
            let tip_ratio = tip / panel.euler_bernoulli_tip(&mat);
            let profile = solved.mid_width(solved.nx / 2) / tip;
            println!(
                "{kernel_name}, t = {thickness}: tip/δ_EB = {tip_ratio:.4}, \
                 u(L/2)/u(L) = {profile:.4}"
            );
            assert!(
                (0.85..=1.15).contains(&tip_ratio),
                "{kernel_name}, t = {thickness}: mid-width tip deflection / δ_EB = \
                 {tip_ratio:.4}, outside [0.85, 1.15] (a shear-locked element collapses it \
                 as ~1/t)"
            );
            assert!(
                (0.28..=0.36).contains(&profile),
                "{kernel_name}, t = {thickness}: u_z(L/2)/u_z(L) = {profile:.4}, outside \
                 [0.28, 0.36] (Euler-Bernoulli 5/16; a locked, shear-dominated profile is \
                 ~linear, ~0.5)"
            );
            tip_ratios.push(tip_ratio);
        }
        for pair in tip_ratios.windows(2) {
            let successive = pair[1] / pair[0];
            assert!(
                (0.95..=1.05).contains(&successive),
                "{kernel_name}: halving t moved tip/δ_EB by ×{successive:.4} (ratios \
                 {tip_ratios:?}); a locking-free element keeps it thickness-invariant, a \
                 locked one gives ×0.25"
            );
        }
        tip_ratios_by_kernel.push(tip_ratios);
    }
    let [bare, plus] = tip_ratios_by_kernel.as_slice() else {
        unreachable!("FLAT_KERNELS is bare MITC3 then MITC3+")
    };
    for ((thickness, bare), plus) in thicknesses.iter().zip(bare).zip(plus) {
        assert!(
            (bare / plus - 1.0).abs() <= 0.01,
            "t = {thickness}: bare MITC3 tip/δ_EB = {bare:.4} and MITC3+ {plus:.4} differ \
             by more than 1%"
        );
    }
}

#[test]
fn flat_shell_flexure_tip_row_moves_without_corner_flap() {
    let mat = cantilever_material();
    let flexure = Cantilever {
        length: 0.05,
        width: 0.01,
        thickness: 0.001,
        tip_force: 10.0,
    };
    for (kernel_name, kernel) in FLAT_KERNELS {
        let solved = flexure.solve(&mat, kernel);
        let tip_ratio = solved.mid_width(solved.nx) / flexure.euler_bernoulli_tip(&mat);
        let tip_row = solved.tip_row();
        let flatness = tip_row.iter().fold(0.0_f64, |m, u| m.max(u.abs()))
            / tip_row.iter().fold(f64::INFINITY, |m, u| m.min(u.abs()));
        println!(
            "{kernel_name} flexure: tip/δ_EB = {tip_ratio:.4}, tip-row max/min = {flatness:.4}"
        );
        assert!(
            (0.85..=1.15).contains(&tip_ratio),
            "{kernel_name} flexure mid-width tip deflection / δ_EB = {tip_ratio:.4}, outside \
             [0.85, 1.15]"
        );
        assert!(
            flatness <= 1.05,
            "{kernel_name} flexure tip row max|u_z|/min|u_z| = {flatness:.4} > 1.05: the \
             free corners flap relative to mid-width (tip row {tip_row:?})"
        );
    }
}
