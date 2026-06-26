//! sRGB byte-triple type shared between the appearance-resolution seam
//! (`resolve_color`, task β #4761) and the 3MF mesh-color egress channel
//! (task δ).

/// An sRGB byte-triple: the resolved-color payload of `resolve_color`
/// (task β #4761) and the per-body color channel in the 3MF mesh egress
/// path (task δ).
///
/// Each component is a gamma-corrected sRGB byte in `[0, 255]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
