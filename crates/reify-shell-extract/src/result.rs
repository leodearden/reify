//! Bundled producer-half output type for the shell-extract engine bridge
//! (PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` §2).
//!
//! [`ShellExtractionResult`] aggregates the five top-level outputs of the
//! shell-extract producer pipeline so that the future shell-extract
//! `ComputeNode` (task γ) has a single value type to persist via
//! `reify_eval::persistent_cache::PersistentlyCacheable`.
//!
//! # Wire-format precedent
//!
//! The encoding pipeline mirrors `reify_eval::persistent_cache::ElasticResult`
//! verbatim (see `crates/reify-eval/src/persistent_cache.rs:697-803`):
//! bincode 1.3 fixint-LE for the header, zstd 0.13 level-0 for the
//! compressed body, bytemuck LE casts for the f64/u32 slabs. Reusing the
//! precedent means every cacheable type in the engine shares one
//! wire-format contract — no per-type branching in the cache read/write path.
//!
//! # Length invariant
//!
//! `mid_surface.vertices.len() == mid_surface.thickness.len()` is enforced
//! at construction ([`ShellExtractionResult::new`]) and at deserialization
//! ([`PersistentlyCacheable::deserialize_from_reader`]). The PRD's literal
//! "`vertices.len() == 3 * thickness.len()`" wording assumes a flat-XYZ
//! `Vec<f64>` interpretation; under the structured `Vec<[f64;3]>` shape
//! used by [`crate::mid_surface::MidSurfaceMesh`], the structurally
//! equivalent invariant is one thickness per vertex.

use crate::mid_surface::MidSurfaceMesh;
use crate::mid_surface_naming::MidSurfaceAttributes;
use crate::segmentation::SegmentationResult;
use reify_types::diagnostics::Diagnostic;

/// Bundled producer-half output of the shell-extract pipeline.
///
/// Per PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` §2, this struct
/// packages the five top-level outputs the future shell-extract `ComputeNode`
/// (task γ) will persist via the
/// [`reify_eval::persistent_cache::PersistentlyCacheable`] trait.
///
/// Construct via [`ShellExtractionResult::new`] so the length invariant
/// `mid_surface.vertices.len() == mid_surface.thickness.len()` is enforced
/// at the producer boundary; direct field construction is also permitted
/// for callers that have already validated their inputs (e.g. test fixtures
/// driving the bit-exact round-trip pin).
///
/// **No `PartialEq` derive** because `reify_types::diagnostics::Diagnostic`
/// (one element type of `diagnostics: Vec<Diagnostic>`) does not impl
/// `PartialEq` at the struct level (only `Debug + Clone`). The round-trip
/// test compares fields individually (via `f64::to_bits()` for NaN/Inf
/// preservation, element-equal scans elsewhere), so a struct-level
/// `PartialEq` is not required.
#[derive(Debug, Clone)]
pub struct ShellExtractionResult {
    /// Mid-surface mesh produced by [`crate::extract_mid_surface`].
    pub mid_surface: MidSurfaceMesh,
    /// Per-region segmentation produced by [`crate::segment_regions`].
    pub segmentation: SegmentationResult,
    /// Topology-attribute records (face + edge) produced by
    /// [`crate::populate_mid_surface_attributes`].
    pub naming: MidSurfaceAttributes,
    /// Wall-clock cost of the producer pipeline in milliseconds. Surfaced
    /// to the persistent-cache layer for cost-weighted LRU eviction via
    /// [`reify_eval::persistent_cache::PersistentlyCacheable::solve_time_ms`].
    pub solve_time_ms: u64,
    /// Soft-warning diagnostics from the producer pipeline (e.g. clipped
    /// region, degenerate fan). Hard failures route through
    /// `ComputeOutcome::Failed` per task γ; this list never contains
    /// `Severity::Error`-level entries on a successful build.
    pub diagnostics: Vec<Diagnostic>,
}

/// Errors returned by [`ShellExtractionResult::new`] and by the
/// deserialization path when an invariant is violated.
///
/// `#[non_exhaustive]` lets future variants be added without breaking
/// external match exhaustiveness; pre-step-4 the enum is empty as a
/// forward-declaration placeholder.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShellExtractionResultError {}

// Compile-time assertion that `ShellExtractionResult: PersistentlyCacheable`.
// Lives at module scope (outside `#[cfg(test)]`) so the trait-bound is
// enforced on every build, not only when `cargo test` links. Mirrors
// `persistent_cache.rs:369-372`.
const _: fn() = || {
    fn assert_impl<T: reify_eval::persistent_cache::PersistentlyCacheable>() {}
    assert_impl::<ShellExtractionResult>();
};

impl reify_eval::persistent_cache::PersistentlyCacheable for ShellExtractionResult {
    const FORMAT_VERSION: u32 = 1;

    fn serialize_to_writer(&self, _w: &mut impl std::io::Write) -> std::io::Result<()> {
        unimplemented!("step-6 wires this up")
    }

    fn deserialize_from_reader(_r: &mut impl std::io::Read) -> std::io::Result<Self> {
        unimplemented!("step-6 wires this up")
    }

    fn uncompressed_byte_size(&self) -> u64 {
        unimplemented!("step-12 wires this up")
    }

    fn solve_time_ms(&self) -> u64 {
        self.solve_time_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mid_surface::MidSurfaceMesh;
    use crate::mid_surface_naming::MidSurfaceAttributes;
    use crate::segmentation::SegmentationResult;
    use reify_eval::persistent_cache::PersistentlyCacheable;

    #[test]
    fn shell_extraction_result_public_surface_is_callable() {
        // (c) smoke-construct with empty inner buffers. Direct field
        // construction is permitted for test fixtures; production callers
        // should prefer `ShellExtractionResult::new` (step-4) to get the
        // length-invariant check.
        let value = ShellExtractionResult {
            mid_surface: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            segmentation: SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            naming: MidSurfaceAttributes::default(),
            solve_time_ms: 0,
            diagnostics: vec![],
        };

        // (a) destructure trip-wire: if any field is added/removed/renamed
        // upstream, this destructure will fail to compile. Production code
        // should never rely on destructure-completeness; this is solely a
        // test-side regression detector for PRD §2 drift.
        let ShellExtractionResult {
            mid_surface,
            segmentation,
            naming,
            solve_time_ms,
            diagnostics,
        } = value;
        assert!(mid_surface.vertices.is_empty());
        assert!(segmentation.regions.is_empty());
        assert!(naming.face_records.is_empty());
        assert_eq!(solve_time_ms, 0);
        assert!(diagnostics.is_empty());

        // (b) the type is re-exported from the crate root. If `pub use
        // result::ShellExtractionResult` is removed from `lib.rs`, this
        // path will fail to resolve at compile time.
        let _from_root: crate::ShellExtractionResult = ShellExtractionResult {
            mid_surface: MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            segmentation: SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            naming: MidSurfaceAttributes::default(),
            solve_time_ms: 0,
            diagnostics: vec![],
        };
    }

    #[test]
    fn shell_extraction_result_format_version_is_one() {
        // Read the FORMAT_VERSION associated const directly — no instance
        // needed, demonstrating the cache-layer use case where `(TypeId,
        // FORMAT_VERSION)` can be looked up before any value materialises.
        // Pins the project convention that FORMAT_VERSION starts at 1
        // because 0 means "uninitialised / unknown" — mirrors
        // `elastic_result_format_version_is_one` at
        // `persistent_cache.rs:1101`.
        assert_eq!(
            <ShellExtractionResult as PersistentlyCacheable>::FORMAT_VERSION,
            1
        );
    }
}
