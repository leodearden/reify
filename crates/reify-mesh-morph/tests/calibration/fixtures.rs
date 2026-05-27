//! Procedural tetrahedral-mesh generators for the PRD task #13 calibration
//! suite. Each fixture takes parametric inputs and returns
//! `(VolumeMesh, surface_node_indices: Vec<u32>)` — connectivity is
//! deterministic across parameter values, so a morph from `param_0` to
//! `param_1` is a strict node-position update.

use reify_ir::{ElementOrderTag, VolumeMesh};
use std::collections::HashMap;
use std::f64::consts::TAU;

/// 6-tet decomposition of a unit hex with main diagonal between corners 0
/// (origin) and 6 (opposite corner). Each tet is right-handed for the
/// canonical CW-from-bottom corner ordering used in CFD/FEA codes
/// (z=0 face: 0→1→2→3; z=top face: 4→5→6→7).
///
/// The split is face-conforming: every quad face is bisected by the same
/// diagonal seen from either side, so adjacent hex cells share a matching
/// triangulation and the mesh has no T-junctions.
pub(crate) const HEX_TO_6TETS: [[usize; 4]; 6] = [
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
    [0, 5, 1, 6],
];

// ── plate_with_hole ──────────────────────────────────────────────────────────

/// Number of angular subdivisions used by [`plate_with_hole`]. Held constant
/// across the calibration sweep so connectivity is parameter-invariant.
const PLATE_N_THETA: usize = 8;

/// Outer plate boundary: half-edge length divided by `max(|cos θ|, |sin θ|)`
/// gives the distance from the centre to the square boundary in direction θ.
fn square_radius(side: f64, theta: f64) -> f64 {
    let half = side / 2.0;
    let c = theta.cos().abs();
    let s = theta.sin().abs();
    half / c.max(s)
}

/// Square plate `[0, side]^2` × `[0, thickness]` with a circular hole of
/// diameter `hole_diameter` centred at `(side/2, side/2)`. The xy mesh is a
/// polar-radial structured grid: `n_radial` radial cells, `PLATE_N_THETA`
/// angular cells (held constant — connectivity invariance under
/// `hole_diameter` sweeps requires a fixed angular count). The 2D grid is
/// extruded through-thickness with `n_through` layers; each hex is decomposed
/// into 6 right-handed tets via [`HEX_TO_6TETS`].
///
/// Each radial position r ∈ {0, …, n_radial} has parameter `t = r / n_radial`
/// and a vertex at angle θ sitting at
/// `centre + ((1-t)*hole_radius + t*square_radius(θ)) * (cos θ, sin θ)`.
/// As `hole_diameter` is varied, only the inner-ring vertices (`r == 0`)
/// move; the outer-ring vertices stay pinned to the square boundary, so the
/// connectivity is fully preserved across the calibration sweep.
///
/// Surface nodes include every vertex on the bottom face (z = 0), the top
/// face (z = thickness), the outer rim (`r == n_radial`), and the inner
/// (hole) rim (`r == 0`).
pub fn plate_with_hole(
    side: f64,
    hole_diameter: f64,
    thickness: f64,
    n_radial: usize,
    n_through: usize,
) -> (VolumeMesh, Vec<u32>) {
    assert!(n_radial >= 1, "plate_with_hole requires n_radial ≥ 1");
    assert!(n_through >= 1, "plate_with_hole requires n_through ≥ 1");
    let hole_radius = hole_diameter / 2.0;
    assert!(hole_radius > 0.0, "hole_diameter must be > 0");
    // Inner ring must fit inside the plate. Use the smallest square radius
    // (at θ = π/4) as the conservative bound.
    let r_sq_min = square_radius(side, TAU / 8.0);
    assert!(
        hole_radius < r_sq_min,
        "hole_radius {hole_radius:.5} ≥ min(square_radius) {r_sq_min:.5} — hole would punch through plate boundary"
    );

    let cx = side / 2.0;
    let cy = side / 2.0;
    let n_theta = PLATE_N_THETA;

    // Vertex layout: (layer_k, ring_r, angle_t) ↦ (k * (n_radial+1) + r) * n_theta + t.
    let nr_pts = n_radial + 1;
    let nz_pts = n_through + 1;

    let vertex_idx = |r: usize, t: usize, k: usize| -> u32 {
        ((k * nr_pts + r) * n_theta + (t % n_theta)) as u32
    };

    // Pre-compute angles + per-angle outer square radii.
    let theta: Vec<f64> = (0..n_theta)
        .map(|t| t as f64 * TAU / n_theta as f64)
        .collect();
    let r_sq: Vec<f64> = theta.iter().map(|&th| square_radius(side, th)).collect();

    // Emit vertices in (k, r, t) order to match vertex_idx.
    let mut vertices: Vec<f32> = Vec::with_capacity(3 * nz_pts * nr_pts * n_theta);
    for k in 0..nz_pts {
        let z = thickness * (k as f64 / n_through as f64);
        for r in 0..nr_pts {
            let trad = r as f64 / n_radial as f64;
            for t in 0..n_theta {
                let radius = (1.0 - trad) * hole_radius + trad * r_sq[t];
                let x = cx + radius * theta[t].cos();
                let y = cy + radius * theta[t].sin();
                vertices.push(x as f32);
                vertices.push(y as f32);
                vertices.push(z as f32);
            }
        }
    }

    // Tet decomposition: every (r, t, k) hex cell, 6 tets each.
    let mut tet_indices: Vec<u32> = Vec::with_capacity(4 * 6 * n_radial * n_theta * n_through);
    for k in 0..n_through {
        for r in 0..n_radial {
            for t in 0..n_theta {
                let t_next = (t + 1) % n_theta;
                // Hex corner ordering (CCW from above on bottom face, then top
                // face directly above — matches HEX_TO_6TETS's right-handed
                // canonical layout).
                let c = [
                    vertex_idx(r, t, k),              // 0
                    vertex_idx(r + 1, t, k),          // 1
                    vertex_idx(r + 1, t_next, k),     // 2
                    vertex_idx(r, t_next, k),         // 3
                    vertex_idx(r, t, k + 1),          // 4
                    vertex_idx(r + 1, t, k + 1),      // 5
                    vertex_idx(r + 1, t_next, k + 1), // 6
                    vertex_idx(r, t_next, k + 1),     // 7
                ];
                for tet in &HEX_TO_6TETS {
                    tet_indices.extend_from_slice(&[c[tet[0]], c[tet[1]], c[tet[2]], c[tet[3]]]);
                }
            }
        }
    }

    // Surface = {z=0 face} ∪ {z=thickness face} ∪ {outer rim} ∪ {inner rim}.
    // A vertex (r, t, k) is on the surface iff r ∈ {0, n_radial} OR
    // k ∈ {0, n_through}.
    let mut surface_indices: Vec<u32> = Vec::new();
    for k in 0..nz_pts {
        for r in 0..nr_pts {
            for t in 0..n_theta {
                if r == 0 || r == n_radial || k == 0 || k == n_through {
                    surface_indices.push(vertex_idx(r, t, k));
                }
            }
        }
    }

    let mesh = VolumeMesh {
        vertices,
        tet_indices,
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    (mesh, surface_indices)
}

// ── bracket ──────────────────────────────────────────────────────────────────

/// L-bracket fixture with a parametric inner-corner fillet. The 2D footprint
/// is `(Arm 1 ∪ Arm 2) \ ExclusionZone` where
///
/// - `Arm 1 = [0, arm_length] × [0, thickness]`
/// - `Arm 2 = [0, thickness] × [0, arm_length]`
/// - `ExclusionZone = { (x, y) ∈ [0, thickness]² : dist((x, y), (T, T)) < fillet_radius }`
///   with `T = thickness`.
///
/// Reify follows the OCCT `BRepFilletAPI` convention: a fillet at the inner
/// corner removes a quarter-disk of material centred on the corner point
/// `(thickness, thickness)`. Mesh vertices inside that quarter-disk would
/// lie outside the bracket's material and are therefore forbidden.
///
/// The 2D footprint is extruded through `thickness` in z and the hex cells
/// are decomposed into 6 right-handed tets via [`HEX_TO_6TETS`]. A small
/// triangular-prism "bridge" cell exists in each arm to connect the polar
/// fillet zone to the rectangular arm extension.
///
/// ## Block layout
///
/// 1. **Corner-block polar zone** — polar grid centred at `(thickness,
///    thickness)`, angular range `θ ∈ [π, 3π/2]`, radial range
///    `r ∈ [fillet_radius, r_out(θ)]`. Inner ring (`r = fillet_radius`) is
///    the fillet arc; outer corners reach `(0, 0)`, `(thickness, 0)`,
///    `(0, thickness)`.
/// 2. **Arm 1 Cartesian zone** — `x ∈ [thickness, arm_length]`,
///    `y ∈ [0, thickness]`; left column shares nodes with the polar zone's
///    `θ = 3π/2` boundary. One bridge wedge cell occupies the strip
///    `y ∈ [thickness - fillet_radius, thickness]` adjacent to `x = thickness`.
/// 3. **Arm 2 Cartesian zone** — analogous, mirrored across the diagonal.
///
/// ## Parameters
///
/// - `arm_length` — full extent of each arm along its long axis.
/// - `thickness` — uniform thickness and extrusion depth.
/// - `fillet_radius` — must satisfy `0 < fillet_radius < thickness`.
/// - `n` — base resolution (radial subdivs, angular subdivs, across-thickness
///   subdivs); arm-long-axis subdivisions = `max(n, 2)`.
///
/// ## Returns
///
/// `(mesh, surface_node_indices)` where `surface_node_indices` lists every
/// vertex on the bracket's outer surface, including the curved fillet
/// surface (`r ≈ fillet_radius` from `(thickness, thickness)`).
///
/// Only the inner fillet-arc vertices move when `fillet_radius` is varied,
/// so connectivity is preserved across the calibration sweep.
pub fn bracket(
    arm_length: f64,
    thickness: f64,
    fillet_radius: f64,
    n: usize,
) -> (VolumeMesh, Vec<u32>) {
    assert!(n >= 1, "bracket requires n ≥ 1");
    assert!(thickness > 0.0, "bracket requires thickness > 0");
    assert!(
        fillet_radius > 0.0 && fillet_radius < thickness,
        "bracket requires 0 < fillet_radius < thickness; got fillet_radius={fillet_radius}, thickness={thickness}"
    );
    assert!(
        arm_length > thickness,
        "bracket requires arm_length > thickness; got arm_length={arm_length}, thickness={thickness}"
    );

    let n_r = n; // radial cells in fillet zone (also across-thickness in arms)
    let n_a = n.max(2); // angular cells in fillet zone
    let n_arm = n.max(2); // long-direction cells per arm
    let n_z = n.max(2); // extrusion subdivisions

    let cx = thickness;
    let cy = thickness;

    // Compact vertex table: emit unique (label) → index, dedup at interfaces.
    let mut vertices: Vec<f32> = Vec::new();
    let mut compact: HashMap<(&'static str, usize, usize), u32> = HashMap::new();

    let mut emit = |label: (&'static str, usize, usize), x: f64, y: f64, z: f64| -> u32 {
        if let Some(&idx) = compact.get(&label) {
            return idx;
        }
        let idx = (vertices.len() / 3) as u32;
        vertices.push(x as f32);
        vertices.push(y as f32);
        vertices.push(z as f32);
        compact.insert(label, idx);
        idx
    };

    // ── Block 1: corner-block polar zone ─────────────────────────────────────
    let theta: Vec<f64> = (0..=n_a)
        .map(|a| std::f64::consts::PI + (a as f64) * (std::f64::consts::PI / 2.0) / n_a as f64)
        .collect();
    let r_out_at: Vec<f64> = theta
        .iter()
        .map(|&th| {
            let c = th.cos().abs();
            let s = th.sin().abs();
            thickness / c.max(s)
        })
        .collect();

    let z_at = |kz: usize| -> f64 { thickness * (kz as f64 / n_z as f64) };

    for kz in 0..=n_z {
        let z = z_at(kz);
        for a in 0..=n_a {
            let th = theta[a];
            let r_out = r_out_at[a];
            for k_r in 0..=n_r {
                let r = fillet_radius + (k_r as f64) * (r_out - fillet_radius) / n_r as f64;
                let x = cx + r * th.cos();
                let y = cy + r * th.sin();
                let label = if a == n_a {
                    // arm-1 interface column at x = thickness. j = n_r - k_r:
                    // j = 0 ↔ y = 0, j = n_r ↔ y = thickness - fillet_radius.
                    ("A1L", kz, n_r - k_r)
                } else if a == 0 {
                    // arm-2 interface row at y = thickness.
                    ("A2B", kz, n_r - k_r)
                } else {
                    ("P", kz, a * (n_r + 1) + k_r)
                };
                emit(label, x, y, z);
            }
        }
    }

    // ── Block 2: arm 1 Cartesian zone ────────────────────────────────────────
    let y_arm = |j: usize| -> f64 {
        if j <= n_r {
            j as f64 * (thickness - fillet_radius) / n_r as f64
        } else {
            thickness
        }
    };
    let x_arm1 =
        |i: usize| -> f64 { thickness + (i as f64) * (arm_length - thickness) / n_arm as f64 };
    for kz in 0..=n_z {
        let z = z_at(kz);
        for i in 0..=n_arm {
            let x = x_arm1(i);
            for j in 0..=n_r + 1 {
                if i == 0 && j == n_r + 1 {
                    continue; // (thickness, thickness) — exclusion zone
                }
                let y = y_arm(j);
                let label = if i == 0 {
                    ("A1L", kz, j)
                } else {
                    ("A1", kz, i * (n_r + 2) + j)
                };
                emit(label, x, y, z);
            }
        }
    }

    // ── Block 3: arm 2 Cartesian zone ────────────────────────────────────────
    let x_arm2 = |i: usize| -> f64 {
        if i <= n_r {
            i as f64 * (thickness - fillet_radius) / n_r as f64
        } else {
            thickness
        }
    };
    let y_arm2 =
        |j: usize| -> f64 { thickness + (j as f64) * (arm_length - thickness) / n_arm as f64 };
    for kz in 0..=n_z {
        let z = z_at(kz);
        for j in 0..=n_arm {
            let y = y_arm2(j);
            for i in 0..=n_r + 1 {
                if i == n_r + 1 && j == 0 {
                    continue; // (thickness, thickness) — exclusion zone
                }
                let x = x_arm2(i);
                let label = if j == 0 {
                    ("A2B", kz, i)
                } else {
                    ("A2", kz, j * (n_r + 2) + i)
                };
                emit(label, x, y, z);
            }
        }
    }

    // Closure `emit` is no longer used after this point; NLL releases the
    // `&mut compact` borrow so the read-only `look` closure can borrow it.

    let look = |label: (&'static str, usize, usize)| -> u32 {
        *compact
            .get(&label)
            .unwrap_or_else(|| panic!("bracket: vertex label {label:?} not found (logic bug)"))
    };

    // ── Tetrahedra ───────────────────────────────────────────────────────────
    let mut tet_indices: Vec<u32> = Vec::new();

    // Polar zone hex cells. Bottom-face CCW from +z: in the fillet quadrant,
    // angle θ runs π → 3π/2 (clockwise in xy) and r runs inward → outward.
    // Local hex corners (i, j) with i ∈ {a, a+1}, j ∈ {k_r, k_r+1}:
    //   c0 = (a,   k_r),     c1 = (a,   k_r+1),
    //   c2 = (a+1, k_r+1),   c3 = (a+1, k_r)
    // This gives a right-handed bottom face when viewed from +z because the
    // radial direction (k_r) ascends "outward from the fillet centre" which,
    // combined with the clockwise θ traversal, produces CCW winding.
    let polar_label = |kz: usize, a: usize, k_r: usize| -> (&'static str, usize, usize) {
        if a == n_a {
            ("A1L", kz, n_r - k_r)
        } else if a == 0 {
            ("A2B", kz, n_r - k_r)
        } else {
            ("P", kz, a * (n_r + 1) + k_r)
        }
    };
    for kz in 0..n_z {
        for a in 0..n_a {
            for k_r in 0..n_r {
                let c0 = look(polar_label(kz, a, k_r));
                let c1 = look(polar_label(kz, a, k_r + 1));
                let c2 = look(polar_label(kz, a + 1, k_r + 1));
                let c3 = look(polar_label(kz, a + 1, k_r));
                let c4 = look(polar_label(kz + 1, a, k_r));
                let c5 = look(polar_label(kz + 1, a, k_r + 1));
                let c6 = look(polar_label(kz + 1, a + 1, k_r + 1));
                let c7 = look(polar_label(kz + 1, a + 1, k_r));
                let c = [c0, c1, c2, c3, c4, c5, c6, c7];
                for tet in &HEX_TO_6TETS {
                    tet_indices.extend_from_slice(&[c[tet[0]], c[tet[1]], c[tet[2]], c[tet[3]]]);
                }
            }
        }
    }

    // Arm 1 hex cells. Bottom-face CCW from +z (x increases along +x, y along +y):
    //   c0 = (i, j), c1 = (i+1, j), c2 = (i+1, j+1), c3 = (i, j+1)
    // Cell (i = 0, j = n_r) bridges to the polar arc tip and is meshed as a
    // triangular prism because corner (i=0, j=n_r+1) does not exist.
    let arm1_label = |kz: usize, i: usize, j: usize| -> (&'static str, usize, usize) {
        if i == 0 {
            ("A1L", kz, j)
        } else {
            ("A1", kz, i * (n_r + 2) + j)
        }
    };
    for kz in 0..n_z {
        for i in 0..n_arm {
            for j in 0..=n_r {
                if i == 0 && j == n_r {
                    // Wedge: corners (0, n_r), (1, n_r), (1, n_r+1).
                    let p0_b = look(arm1_label(kz, 0, n_r));
                    let p1_b = look(arm1_label(kz, 1, n_r));
                    let p2_b = look(arm1_label(kz, 1, n_r + 1));
                    let p0_t = look(arm1_label(kz + 1, 0, n_r));
                    let p1_t = look(arm1_label(kz + 1, 1, n_r));
                    let p2_t = look(arm1_label(kz + 1, 1, n_r + 1));
                    tet_indices.extend_from_slice(&[p0_b, p1_b, p2_b, p2_t]);
                    tet_indices.extend_from_slice(&[p0_b, p1_b, p2_t, p1_t]);
                    tet_indices.extend_from_slice(&[p0_b, p1_t, p2_t, p0_t]);
                    continue;
                }
                let c0 = look(arm1_label(kz, i, j));
                let c1 = look(arm1_label(kz, i + 1, j));
                let c2 = look(arm1_label(kz, i + 1, j + 1));
                let c3 = look(arm1_label(kz, i, j + 1));
                let c4 = look(arm1_label(kz + 1, i, j));
                let c5 = look(arm1_label(kz + 1, i + 1, j));
                let c6 = look(arm1_label(kz + 1, i + 1, j + 1));
                let c7 = look(arm1_label(kz + 1, i, j + 1));
                let c = [c0, c1, c2, c3, c4, c5, c6, c7];
                for tet in &HEX_TO_6TETS {
                    tet_indices.extend_from_slice(&[c[tet[0]], c[tet[1]], c[tet[2]], c[tet[3]]]);
                }
            }
        }
    }

    // Arm 2 hex cells.
    let arm2_label = |kz: usize, i: usize, j: usize| -> (&'static str, usize, usize) {
        if j == 0 {
            ("A2B", kz, i)
        } else {
            ("A2", kz, j * (n_r + 2) + i)
        }
    };
    for kz in 0..n_z {
        for j in 0..n_arm {
            for i in 0..=n_r {
                if i == n_r && j == 0 {
                    // Wedge: corners (n_r, 0), (n_r+1, 1), (n_r, 1). The
                    // (n_r, 1)/(n_r+1, 1) order is reversed from arm 1's
                    // wedge because here i indexes x and j indexes y —
                    // the CCW-from-+z winding rotates differently.
                    let p0_b = look(arm2_label(kz, n_r, 0));
                    let p1_b = look(arm2_label(kz, n_r + 1, 1));
                    let p2_b = look(arm2_label(kz, n_r, 1));
                    let p0_t = look(arm2_label(kz + 1, n_r, 0));
                    let p1_t = look(arm2_label(kz + 1, n_r + 1, 1));
                    let p2_t = look(arm2_label(kz + 1, n_r, 1));
                    tet_indices.extend_from_slice(&[p0_b, p1_b, p2_b, p2_t]);
                    tet_indices.extend_from_slice(&[p0_b, p1_b, p2_t, p1_t]);
                    tet_indices.extend_from_slice(&[p0_b, p1_t, p2_t, p0_t]);
                    continue;
                }
                let c0 = look(arm2_label(kz, i, j));
                let c1 = look(arm2_label(kz, i + 1, j));
                let c2 = look(arm2_label(kz, i + 1, j + 1));
                let c3 = look(arm2_label(kz, i, j + 1));
                let c4 = look(arm2_label(kz + 1, i, j));
                let c5 = look(arm2_label(kz + 1, i + 1, j));
                let c6 = look(arm2_label(kz + 1, i + 1, j + 1));
                let c7 = look(arm2_label(kz + 1, i, j + 1));
                let c = [c0, c1, c2, c3, c4, c5, c6, c7];
                for tet in &HEX_TO_6TETS {
                    tet_indices.extend_from_slice(&[c[tet[0]], c[tet[1]], c[tet[2]], c[tet[3]]]);
                }
            }
        }
    }

    // ── Surface nodes ────────────────────────────────────────────────────────
    let n_vertices = vertices.len() / 3;
    let mut surface_indices: Vec<u32> = Vec::new();
    let tol = 1e-5_f32;
    let arc_tol = 1e-4_f64;
    for v in 0..n_vertices {
        let x = vertices[v * 3];
        let y = vertices[v * 3 + 1];
        let z = vertices[v * 3 + 2];
        let on_z = z.abs() < tol || (z - thickness as f32).abs() < tol;
        let on_y = y.abs() < tol || (y - arm_length as f32).abs() < tol;
        let on_x = x.abs() < tol || (x - arm_length as f32).abs() < tol;
        let dx = x as f64 - cx;
        let dy = y as f64 - cy;
        let r = (dx * dx + dy * dy).sqrt();
        let on_arc = (r - fillet_radius).abs() < arc_tol
            && x as f64 <= thickness + arc_tol
            && y as f64 <= thickness + arc_tol;
        if on_z || on_y || on_x || on_arc {
            surface_indices.push(v as u32);
        }
    }

    let mesh = VolumeMesh {
        vertices,
        tet_indices,
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    (mesh, surface_indices)
}
