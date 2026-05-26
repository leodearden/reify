//! Per-element material sampling and the `AnisotropicMaterial` evaluated value.
//!
//! See PRD `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C3.
//!
//! This module ships:
//!
//! - [`AnisotropicMaterial`]: the **concrete evaluated value** carrying a
//!   resolved 6×6 stiffness in the material's local frame, plus the
//!   local→global rotation matrix. Construction via
//!   [`AnisotropicMaterial::from_law`] calls
//!   [`crate::constitutive::ConstitutiveLaw::d_matrix_local`] once at
//!   field-build time so the codomain is uniform across heterogeneous laws
//!   (isotropic + orthotropic + transverse-iso all collapse to the same
//!   36-`f64` representation) and the value remains `Copy` (no heap alloc
//!   in the assembly hot path).
//!
//! - [`MaterialField`]: the trait every field implementation satisfies — a
//!   single `material_at(point)` lookup. Used by the assembly hook to sample
//!   one D per element at the element centroid.
//!
//! - [`ConstantField`]: the **constant lift** of a single
//!   `AnisotropicMaterial` to a field. This is the bit-identity anchor for
//!   the C4 assembly hook regression: a `ConstantField` of an
//!   identity-frame isotropic material produces a `K_e` that is bit-equal
//!   to today's v0.3 isotropic path.
//!
//! - [`DiscreteCellField`]: a cell-indexed field that dispatches lookups via
//!   a caller-supplied `locator` closure (`[f64;3] -> Option<usize>`). The
//!   spatial-index backing (BVH vs uniform grid vs linear scan) is deferred
//!   per PRD Q2 to the FDM field producer; the closure surface lets any
//!   backing slot in without breaking the trait.

use crate::constitutive::{ConstitutiveLaw, rotate_voigt};

/// A concrete evaluated material value carrying a 6×6 stiffness in the
/// material's local frame plus the local→global rotation matrix.
///
/// # PRD reference
///
/// `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C3 calls
/// this "the concrete evaluated value carrying a resolved stiffness plus
/// its frame." Storing the already-evaluated 6×6 `d_local` rather than a
/// `Box<dyn ConstitutiveLaw>` gives three concrete wins:
///
/// 1. The codomain is uniform across heterogeneous laws (isotropic +
///    orthotropic + transverse-iso all collapse to the same 36-`f64`
///    representation), so a `Field<Point3, AnisotropicMaterial>` can
///    scatter cleanly without trait-object plumbing.
/// 2. The value is `Copy`, so per-element sampling in the assembly hot
///    path never heap-allocates.
/// 3. No trait-object dispatch in the inner `K_e = ∫ BᵀD_globalB |det J| dV`
///    loop.
///
/// # Frame convention
///
/// `frame` is the **local → global** rotation (columns = local basis
/// vectors in global coordinates), matching the convention pinned by
/// [`rotate_voigt`]. See its docstring for the worked example.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnisotropicMaterial {
    /// 6×6 elasticity matrix in the material's local frame
    /// (engineering-shear Voigt order; shear-block diagonal = G, not 2G).
    /// Pre-computed via `ConstitutiveLaw::d_matrix_local()` at field-build
    /// time so the value is `Copy` and the assembly hot path can avoid
    /// trait-object dispatch.
    pub d_local: [[f64; 6]; 6],
    /// 3×3 local → global rotation. Columns are the local basis vectors
    /// expressed in global coordinates (same convention as
    /// [`rotate_voigt`]).
    pub frame: [[f64; 3]; 3],
}

impl AnisotropicMaterial {
    /// Construct an `AnisotropicMaterial` from any
    /// [`ConstitutiveLaw`] + a local→global frame.
    ///
    /// Calls `law.d_matrix_local()` **once** at construction; subsequent
    /// `d_matrix_global()` calls reuse the cached 6×6.
    #[inline]
    pub fn from_law<L: ConstitutiveLaw>(law: &L, frame: [[f64; 3]; 3]) -> Self {
        Self {
            d_local: law.d_matrix_local(),
            frame,
        }
    }

    /// Return the 6×6 elasticity matrix rotated into the global frame.
    ///
    /// Delegates to [`rotate_voigt`]. When `frame` is the identity, the
    /// returned matrix is **bitwise** equal to `d_local` (pinned by
    /// `tests::anisotropic_material_from_law_with_identity_frame_d_global_is_bitwise_d_local`
    /// and by `rotate_voigt`'s own identity-frame bitwise-no-op contract).
    #[inline]
    pub fn d_matrix_global(&self) -> [[f64; 6]; 6] {
        rotate_voigt(&self.d_local, &self.frame)
    }
}

/// A spatial field of [`AnisotropicMaterial`] values — the C3 surface
/// the assembly hook (C4) samples once per element at the element
/// centroid.
///
/// # PRD reference
///
/// `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C3.
///
/// # Object safety
///
/// The trait is intentionally **object-safe** (no `Self` in return types,
/// no associated types, no generic methods). The
/// `material_field_trait_is_object_safe_via_constant_field` test pins
/// that `&dyn MaterialField` typechecks. Wrappers in `assembly::*` use a
/// generic `F: MaterialField` bound for monomorphisation in the hot path,
/// but the object-safe surface remains available for callers that need
/// trait-object polymorphism.
pub trait MaterialField {
    /// Return the material value at the given point in global coordinates.
    ///
    /// Implementations must be deterministic — the same point yields the
    /// same material. Used by the assembly hook to sample one D per
    /// element at the element centroid.
    fn material_at(&self, point: [f64; 3]) -> AnisotropicMaterial;
}

/// Constant lift of a single [`AnisotropicMaterial`] to a field — the
/// bit-identity anchor for the C4 assembly hook regression.
///
/// A `ConstantField` of an identity-frame isotropic material assembled
/// via the field-aware `element_stiffness_*_with_field` entry points
/// must produce a `K_e` that is bit-equal to today's v0.3 isotropic
/// path (`element_stiffness_p1(&phys, &iso)` etc.). Pinned by the
/// step-9/11 integration tests.
#[derive(Debug, Clone, Copy)]
pub struct ConstantField {
    /// The single material returned at every point.
    pub material: AnisotropicMaterial,
}

impl MaterialField for ConstantField {
    #[inline]
    fn material_at(&self, _point: [f64; 3]) -> AnisotropicMaterial {
        self.material
    }
}

/// A cell-indexed [`AnisotropicMaterial`] field — dispatches every
/// lookup through a caller-supplied `locator` closure that maps a
/// global-coordinate point to the index of the cell containing it.
///
/// # PRD reference
///
/// `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C3.
///
/// # Spatial-index backing (PRD Q2)
///
/// PRD Q2 defers the backing-structure choice (BVH vs uniform grid vs
/// linear scan) to the FDM field producer — "decide alongside the FDM
/// field producer." Rather than commit to a specific backing here, the
/// `locator` closure surface lets any backing slot in without breaking
/// the trait. Test fixtures use simple per-cell-AABB closures; the
/// production FDM consumer will supply a BVH-backed locator.
///
/// # Panic contract
///
/// In debug builds, `material_at` panics with a `DiscreteCellField`-
/// prefixed message if the locator returns `None` (no containing cell)
/// or an out-of-range index. The prefix matches the
/// `OrthotropicMaterial`/`TransverseIsotropicMaterial` convention so
/// `#[should_panic(expected = "DiscreteCellField")]` tests can pin
/// exactly which field rejected. Release builds skip the check; a
/// caller-supplied locator that lies about cell membership in a release
/// build will read an arbitrary `cells[idx]` (in-bounds via the
/// `Send + Sync` closure's discretion) or panic at the `Vec` boundary
/// — either way the contract is the caller's responsibility, matching
/// the existing `debug_assert!` policy across the constitutive module.
pub struct DiscreteCellField {
    /// Per-cell materials. The locator's returned index references this
    /// vector.
    pub cells: Vec<AnisotropicMaterial>,
    /// `point -> Some(cell_index)` for the cell containing `point`,
    /// or `None` if no cell contains it (debug-asserted to be `Some`
    /// at every assembly call site).
    pub locator: Box<dyn Fn([f64; 3]) -> Option<usize> + Send + Sync>,
}

// `Box<dyn Fn>` doesn't derive `Debug`; hand-write it so the struct is
// still discoverable via `{:?}` without exposing the closure body.
impl std::fmt::Debug for DiscreteCellField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscreteCellField")
            .field("cells.len()", &self.cells.len())
            .field("locator", &"<closure>")
            .finish()
    }
}

impl MaterialField for DiscreteCellField {
    fn material_at(&self, point: [f64; 3]) -> AnisotropicMaterial {
        match (self.locator)(point) {
            Some(idx) => {
                debug_assert!(
                    idx < self.cells.len(),
                    "DiscreteCellField: locator returned cell index {idx} but only {n} cells are registered",
                    n = self.cells.len(),
                );
                self.cells[idx]
            }
            None => {
                debug_assert!(
                    false,
                    "DiscreteCellField: locator returned None for point {point:?} (no containing cell)",
                );
                // Release-build fallback: panic at the Vec boundary by
                // indexing with `usize::MAX`, which produces a clear
                // out-of-bounds error rather than silent undefined data.
                self.cells[usize::MAX]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::{IsotropicElastic, OrthotropicMaterial, rotate_voigt};

    /// Identity 3×3 frame — local axes align with global.
    const IDENTITY_3X3: [[f64; 3]; 3] =
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    /// CFRP-like orthotropic material (mirrors `tests/constitutive_laws.rs::cfrp_orthotropic`).
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

    /// Rotation about z (Rodrigues, axis = ẑ): used by the non-trivial-frame test.
    fn rotation_about_z(angle_rad: f64) -> [[f64; 3]; 3] {
        let (s, c) = angle_rad.sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    }

    // ── Step 1 RED tests ────────────────────────────────────────────────────

    /// Identity-frame `d_matrix_global()` must be bitwise equal to the
    /// source `d_matrix()` (linchpin of the C4 bit-identity contract).
    #[test]
    fn anisotropic_material_from_law_with_identity_frame_d_global_is_bitwise_d_local() {
        let iso = IsotropicElastic {
            youngs_modulus: 200e9,
            poisson_ratio: 0.3,
        };
        let mat = AnisotropicMaterial::from_law(&iso, IDENTITY_3X3);
        let d_global = mat.d_matrix_global();
        let d_source = iso.d_matrix();
        for i in 0..6 {
            for j in 0..6 {
                assert_eq!(
                    d_global[i][j].to_bits(),
                    d_source[i][j].to_bits(),
                    "identity-frame D_global[{i}][{j}] = {} must be bitwise equal to D_source[{i}][{j}] = {}",
                    d_global[i][j],
                    d_source[i][j],
                );
            }
        }
    }

    /// `d_matrix_global()` for a non-trivial frame must equal
    /// `rotate_voigt(d_local, frame)` entry-by-entry (delegation pin).
    #[test]
    fn anisotropic_material_d_global_equals_rotate_voigt_for_nontrivial_frame() {
        let law = cfrp_orthotropic();
        let frame = rotation_about_z(30.0_f64.to_radians());
        let mat = AnisotropicMaterial::from_law(&law, frame);
        let d_global = mat.d_matrix_global();
        let expected = rotate_voigt(&law.d_matrix_local(), &frame);
        for i in 0..6 {
            for j in 0..6 {
                assert_eq!(
                    d_global[i][j].to_bits(),
                    expected[i][j].to_bits(),
                    "D_global[{i}][{j}] = {} must equal rotate_voigt(d_local, frame)[{i}][{j}] = {} entry-by-entry",
                    d_global[i][j],
                    expected[i][j],
                );
            }
        }
    }

    // ── Step 3 RED tests ────────────────────────────────────────────────────

    /// Entry-by-entry bitwise equality on both `d_local` and `frame`.
    fn assert_anisotropic_material_bitwise_eq(
        got: AnisotropicMaterial,
        expected: AnisotropicMaterial,
        ctx: &str,
    ) {
        for i in 0..6 {
            for j in 0..6 {
                assert_eq!(
                    got.d_local[i][j].to_bits(),
                    expected.d_local[i][j].to_bits(),
                    "{ctx}: d_local[{i}][{j}] = {} must equal {} bitwise",
                    got.d_local[i][j],
                    expected.d_local[i][j],
                );
            }
        }
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(
                    got.frame[i][j].to_bits(),
                    expected.frame[i][j].to_bits(),
                    "{ctx}: frame[{i}][{j}] = {} must equal {} bitwise",
                    got.frame[i][j],
                    expected.frame[i][j],
                );
            }
        }
    }

    /// `ConstantField::material_at(p)` returns the same material at every
    /// sampled point (entry-by-entry bitwise equality on both fields).
    #[test]
    fn constant_field_material_at_returns_same_material_at_any_point() {
        let iso = IsotropicElastic {
            youngs_modulus: 200e9,
            poisson_ratio: 0.3,
        };
        let material = AnisotropicMaterial::from_law(&iso, IDENTITY_3X3);
        let field = ConstantField { material };
        for p in [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [-5.5, 7.7, -9.9]] {
            assert_anisotropic_material_bitwise_eq(
                field.material_at(p),
                material,
                &format!("ConstantField::material_at({p:?})"),
            );
        }
    }

    /// Pin that `&dyn MaterialField` is a valid type — catches accidental
    /// generic-self / associated-type additions that would break object
    /// safety.
    #[test]
    fn material_field_trait_is_object_safe_via_constant_field() {
        let iso = IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        };
        let field = ConstantField {
            material: AnisotropicMaterial::from_law(&iso, IDENTITY_3X3),
        };
        let _: &dyn MaterialField = &field;
    }

    // ── Step 5 RED tests ────────────────────────────────────────────────────

    /// `DiscreteCellField::material_at` dispatches through the locator and
    /// returns the cell-indexed material entry-by-entry bitwise.
    #[test]
    fn discrete_cell_field_material_at_dispatches_through_locator_to_indexed_cell() {
        let iso_a = IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        };
        let iso_b = IsotropicElastic {
            youngs_modulus: 2.0,
            poisson_ratio: 0.3,
        };
        let mat_a = AnisotropicMaterial::from_law(&iso_a, IDENTITY_3X3);
        let mat_b = AnisotropicMaterial::from_law(&iso_b, IDENTITY_3X3);

        let field = DiscreteCellField {
            cells: vec![mat_a, mat_b],
            locator: Box::new(|p: [f64; 3]| if p[0] < 0.5 { Some(0) } else { Some(1) }),
        };

        assert_anisotropic_material_bitwise_eq(
            field.material_at([0.25, 0.0, 0.0]),
            mat_a,
            "DiscreteCellField at x=0.25 → cell 0 (mat_a)",
        );
        assert_anisotropic_material_bitwise_eq(
            field.material_at([0.75, 0.0, 0.0]),
            mat_b,
            "DiscreteCellField at x=0.75 → cell 1 (mat_b)",
        );
    }

    /// Out-of-range cell index must panic with a descriptive
    /// `DiscreteCellField`-prefixed message (matches the
    /// `OrthotropicMaterial::debug_assert_valid` panic-message-prefix
    /// convention).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "DiscreteCellField")]
    fn discrete_cell_field_panics_on_out_of_range_cell_index() {
        let iso = IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        };
        let mat = AnisotropicMaterial::from_law(&iso, IDENTITY_3X3);
        let field = DiscreteCellField {
            cells: vec![mat, mat], // 2 cells
            locator: Box::new(|_p: [f64; 3]| Some(99)), // out-of-range
        };
        let _ = field.material_at([0.0, 0.0, 0.0]);
    }
}
