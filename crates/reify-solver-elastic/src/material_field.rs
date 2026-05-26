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
}
