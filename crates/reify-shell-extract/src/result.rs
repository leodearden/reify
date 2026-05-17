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
use crate::mid_surface_naming::{MidSurfaceAttributes, MidSurfaceEdgeRecord};
use crate::segmentation::{RegionClassification, RegionInfo, SegmentationResult};
use reify_types::diagnostics::{Diagnostic, DiagnosticLabel, Severity, SourceSpan};
use reify_types::geometry::{AxisSign, CapKind, FeatureId, ModEntry, Role, TopologyAttribute};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

// ---- Bounds on length-prefix fields read from the on-disk header ----
//
// These guards mirror `MAX_F64_ELEMENTS` at `persistent_cache.rs:362` and
// prevent a corrupted or tampered cache entry from triggering a
// gigabyte-scale `Vec::with_capacity` allocation during deserialization.
// Sized for shell-extract producer outputs at workstation scale.

/// Maximum number of vertices in a deserialized `MidSurfaceMesh`. Each
/// vertex is 24 bytes on the slab (3 × `f64`), so 1 << 24 ≈ 16 M
/// vertices ≈ 384 MiB. Mirrors `MAX_F64_ELEMENTS` rationale at
/// `persistent_cache.rs:343-361`.
const MAX_VERTICES: u64 = 1 << 24;
/// Maximum number of triangles in a deserialized `MidSurfaceMesh`. Each
/// triangle is 12 bytes on the slab (3 × `u32`), so 1 << 24 ≈ 16 M
/// triangles ≈ 192 MiB.
const MAX_TRIANGLES: u64 = 1 << 24;
/// Maximum length of per-vertex `f64` slabs (`thickness`).
const MAX_F64_ELEMENTS: u64 = 1 << 24;
/// Maximum length of `u32` label slabs (`vertex_labels`, `triangle_labels`).
const MAX_U32_ELEMENTS: u64 = 1 << 24;
/// Maximum voxels per region. Each voxel is 12 bytes on the slab (3 × `i32`).
const MAX_VOXELS_PER_REGION: u64 = 1 << 24;
/// Maximum bytes bincode is allowed to consume while deserializing the
/// header. Defends against the DoS hazard that the slab-side `check_len`
/// caps don't cover: bincode's seq deserializer reads a `u64` length
/// prefix for every nested `Vec` (`regions`, `naming.face_records`,
/// `naming.edges`, `diagnostics`, and the further-nested `mod_history`
/// / `labels` / `candidates`) and propagates it via `size_hint` to
/// `Vec::with_capacity`. A tampered header claiming
/// `regions: 1 << 60` would attempt a gigabyte-scale `try_reserve`
/// before EOF is ever observed. `with_limit` short-circuits that path:
/// once bincode has consumed `MAX_HEADER_BYTES`, deserialization fails
/// with `SizeLimit`. 4 MiB (`1 << 22`) sits comfortably above any
/// realistic shell-extract producer's header — real headers are
/// KB-range, since the bulk f64/u32 data lives in uncompressed slabs
/// (not subject to this cap) bounded by the per-field `MAX_*` constants
/// above. The previous 256 MiB ceiling left a quarter-gigabyte
/// `Vec::with_capacity` window on a tampered nested-Vec length prefix;
/// 4 MiB closes that window while remaining well above any realistic
/// header size.
const MAX_HEADER_BYTES: u64 = 1 << 22;

/// Construct the bincode `Options` chain used for both encode and decode
/// paths. Pinning the chain in one place ensures byte-shape parity
/// between `serialize_to_writer` and `deserialize_from_reader` —
/// `with_fixint_encoding` matches the legacy `bincode::serialize_into`
/// default (verified by the cross-format pin at
/// `crates/reify-eval/src/persistent_cache.rs:1670`), `with_limit`
/// is enforced on **both** encode and decode in bincode 1.3 (confirmed
/// by test `shell_extraction_result_bincode_options_enforces_limit_on_encode`
/// for the encode side and by
/// `shell_extraction_result_deserialize_rejects_header_above_max_header_bytes`
/// for the decode side), and `allow_trailing_bytes` is required because
/// the slab data follows the bincode header in the same stream.
fn bincode_options() -> impl bincode::Options + Copy {
    use bincode::Options;
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_HEADER_BYTES)
        .allow_trailing_bytes()
}

/// Convert a `u64` length prefix into a validated `usize`, returning
/// `io::Error(InvalidData)` if it exceeds `max`. Mirrors
/// `check_f64_vec_len` at `persistent_cache.rs:464`. The cast is safe
/// post-check because all `MAX_*` constants are ≤ `1 << 26`, well within
/// `u32`.
fn check_len(field: &str, len: u64, max: u64) -> io::Result<usize> {
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "ShellExtractionResult {field} length {len} exceeds limit {max} \
                 (corrupted or tampered cache entry?)"
            ),
        ));
    }
    Ok(len as usize)
}

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
/// external match exhaustiveness.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShellExtractionResultError {
    /// `mid_surface.thickness.len()` must equal `mid_surface.vertices.len()`.
    /// A mismatch indicates a caller-constructed (non-T2-produced) bundle
    /// with inconsistent parallel arrays. Shape mirrors
    /// [`crate::segmentation::SegmentationError::MeshLengthMismatch`] so
    /// the contract is uniform across the crate.
    ///
    /// The PRD's literal "`vertices.len() == 3 * thickness.len()`" wording
    /// is the flat-XYZ-coordinate interpretation; under the structured
    /// `Vec<[f64; 3]>` shape used by
    /// [`crate::mid_surface::MidSurfaceMesh`], the structurally equivalent
    /// invariant is one thickness per vertex.
    LengthInvariantViolation {
        /// Number of vertices in the bundled mid-surface mesh.
        vertices_len: usize,
        /// Number of thickness entries in the bundled mid-surface mesh.
        thickness_len: usize,
    },
}

impl std::fmt::Display for ShellExtractionResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellExtractionResultError::LengthInvariantViolation {
                vertices_len,
                thickness_len,
            } => write!(
                f,
                "mid_surface.thickness.len() ({thickness_len}) ≠ \
                 mid_surface.vertices.len() ({vertices_len}); \
                 the two parallel arrays must be the same length"
            ),
        }
    }
}

impl std::error::Error for ShellExtractionResultError {}

impl ShellExtractionResult {
    /// Construct a `ShellExtractionResult`, enforcing the length invariant
    /// `mid_surface.vertices.len() == mid_surface.thickness.len()`.
    ///
    /// Returns [`ShellExtractionResultError::LengthInvariantViolation`] if
    /// the two parallel arrays disagree. Producer code (the future
    /// shell-extract `ComputeNode`, task γ) should always route through
    /// this constructor; test fixtures that need to bypass the check may
    /// use direct field construction.
    pub fn new(
        mid_surface: MidSurfaceMesh,
        segmentation: SegmentationResult,
        naming: MidSurfaceAttributes,
        solve_time_ms: u64,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Self, ShellExtractionResultError> {
        if mid_surface.vertices.len() != mid_surface.thickness.len() {
            return Err(ShellExtractionResultError::LengthInvariantViolation {
                vertices_len: mid_surface.vertices.len(),
                thickness_len: mid_surface.thickness.len(),
            });
        }
        Ok(Self {
            mid_surface,
            segmentation,
            naming,
            solve_time_ms,
            diagnostics,
        })
    }

    /// Build the on-disk header struct from `&self`. Single source of truth
    /// for the wire-shape projection: called from both
    /// `serialize_to_writer` (to feed bincode) and `uncompressed_byte_size`
    /// (to feed `bincode::serialized_size`). Extracting this helper means a
    /// future field addition only needs one edit on the impl side
    /// (`ShellExtractionResultHeader` + this builder), not two. The
    /// round-trip test still pins drift against the actually-serialized
    /// bytes, so the previous "intentional duplication for drift check"
    /// claim is subsumed by that pin.
    fn build_on_disk_header(&self) -> ShellExtractionResultHeader {
        ShellExtractionResultHeader {
            solve_time_ms: self.solve_time_ms,
            vertices_len: self.mid_surface.vertices.len() as u64,
            triangles_len: self.mid_surface.triangles.len() as u64,
            thickness_len: self.mid_surface.thickness.len() as u64,
            vertex_labels_len: self.segmentation.vertex_labels.len() as u64,
            triangle_labels_len: self.segmentation.triangle_labels.len() as u64,
            regions: self
                .segmentation
                .regions
                .iter()
                .map(|r| RegionInfoOnDisk {
                    label: r.label,
                    voxels_len: r.voxels.len() as u64,
                    mean_thickness_bits: r.mean_thickness.to_bits(),
                    extent_bits: r.extent.to_bits(),
                    thickness_extent_ratio_bits: r.thickness_extent_ratio.to_bits(),
                    classification: classification_to_u8(r.classification),
                })
                .collect(),
            naming: MidSurfaceAttributesOnDisk {
                face_records: self
                    .naming
                    .face_records
                    .iter()
                    .map(topology_attribute_to_disk)
                    .collect(),
                edges: self
                    .naming
                    .edges
                    .iter()
                    .map(|e| MidSurfaceEdgeRecordOnDisk {
                        attribute: topology_attribute_to_disk(&e.attribute),
                        region_pair_a: e.region_pair.0,
                        region_pair_b: e.region_pair.1,
                    })
                    .collect(),
            },
            diagnostics: self.diagnostics.iter().map(diagnostic_to_disk).collect(),
        }
    }
}

// ---- On-disk wire-shape mirrors ----
//
// `reify_types::Diagnostic`, `reify_types::DiagnosticLabel`,
// `reify_types::geometry::TopologyAttribute`, etc. do not derive serde
// unconditionally (only `Severity` is feature-gated). Adding crate-wide
// serde derives would balloon the change beyond the α-task `crates touched`
// list, so we define private mirrors here that carry the same field set in
// a self-contained, serde-friendly shape. The mirrors are private to this
// module — they ARE the on-disk wire format and must NOT be made public
// without bumping `FORMAT_VERSION`.
//
// Lossy-by-design fields:
//   * `Diagnostic::code` is dropped. Shell-specific `DiagnosticCode`
//     variants (`E_SHELL_NO_VOXEL_GRID` etc.) don't exist yet (task ε
//     owns them), so preserving the current `code` value would only round
//     legacy non-shell variants — low signal. Round-trips to `None`.

#[derive(Serialize, Deserialize)]
struct ShellExtractionResultHeader {
    solve_time_ms: u64,
    vertices_len: u64,
    triangles_len: u64,
    thickness_len: u64,
    vertex_labels_len: u64,
    triangle_labels_len: u64,
    regions: Vec<RegionInfoOnDisk>,
    naming: MidSurfaceAttributesOnDisk,
    diagnostics: Vec<DiagnosticOnDisk>,
}

#[derive(Serialize, Deserialize)]
struct RegionInfoOnDisk {
    label: u32,
    voxels_len: u64,
    /// `f64` bit pattern of `RegionInfo::mean_thickness`. Stored as `u64`
    /// (NOT `f64`) so NaN payloads, signaling-NaN bits, and signed zeros
    /// survive serde NaN-normalization — same trick as
    /// `ElasticResultHeader::max_von_mises_bits` at `persistent_cache.rs:385`.
    mean_thickness_bits: u64,
    extent_bits: u64,
    /// `RegionInfo::thickness_extent_ratio` can be `f64::INFINITY` (per the
    /// field docstring at `segmentation.rs:167`); the `u64`-bit-pattern
    /// encoding preserves that exactly.
    thickness_extent_ratio_bits: u64,
    classification: u8,
}

#[derive(Serialize, Deserialize)]
struct MidSurfaceAttributesOnDisk {
    face_records: Vec<TopologyAttributeOnDisk>,
    edges: Vec<MidSurfaceEdgeRecordOnDisk>,
}

#[derive(Serialize, Deserialize)]
struct TopologyAttributeOnDisk {
    feature_id: String,
    /// Explicit u8 wire tag for the in-memory `Role` (and its `CapKind`
    /// payload). Decoupled from `Role`'s declaration order so that adding
    /// a new variant in the middle of `Role` or `CapKind` cannot silently
    /// shift any downstream tag's on-disk encoding. See
    /// [`role_to_u8`] / [`role_from_u8`] for the pinned tag table. Unknown
    /// discriminants on read are rejected with `InvalidData`.
    role: u8,
    local_index: u32,
    user_label: Option<String>,
    mod_history: Vec<ModEntryOnDisk>,
}

#[derive(Serialize, Deserialize)]
struct ModEntryOnDisk {
    splitting_feature_id: String,
    split_index: u32,
}

// ---- Pinned u8 wire tags for `Role` and its `CapKind` payload ----
//
// Rationale (review suggestion 1): a serde-derived `RoleOnDisk` /
// `CapKindOnDisk` would encode variant tags by declaration order under
// bincode, so adding a new variant in the middle of either enum could
// silently shift every downstream tag — corrupting cache reads written
// before the addition with no compile-time signal forcing a
// `FORMAT_VERSION` bump. By contrast, explicit u8 constants and `match`-
// based conversion (mirroring `severity_to_u8` / `classification_to_u8`)
// make wire-tag stability independent of enum declaration order.
//
// Layout: the high nibble separates payload-bearing `Cap` variants
// (`0x0X`) from unit variants (`0x1X`), so future additions in either
// space don't collide. Bumping `FORMAT_VERSION` is required if any
// existing tag value here is reassigned.
const ROLE_TAG_CAP_TOP: u8 = 0x00;
const ROLE_TAG_CAP_BOTTOM: u8 = 0x01;
const ROLE_TAG_CAP_START: u8 = 0x02;
const ROLE_TAG_CAP_END: u8 = 0x03;
const ROLE_TAG_SIDE: u8 = 0x10;
const ROLE_TAG_NEW_EDGE: u8 = 0x11;
const ROLE_TAG_REVOLVED_FACE: u8 = 0x12;
const ROLE_TAG_AXIS_FACE: u8 = 0x13;
const ROLE_TAG_SWEPT_FACE: u8 = 0x14;
const ROLE_TAG_LOFTED_FACE: u8 = 0x15;
const ROLE_TAG_MID_SURFACE_FACE: u8 = 0x16;
const ROLE_TAG_MID_SURFACE_EDGE: u8 = 0x17;
// CornerVertex: high nibble 0x2X, low nibble = bit-packed signs (bit2=x, bit1=y, bit0=z; Pos=0, Neg=1)
const ROLE_TAG_CORNER_VERTEX_PPP: u8 = 0x20; // (+x, +y, +z)
const ROLE_TAG_CORNER_VERTEX_PPN: u8 = 0x21; // (+x, +y, -z)
const ROLE_TAG_CORNER_VERTEX_PNP: u8 = 0x22; // (+x, -y, +z)
const ROLE_TAG_CORNER_VERTEX_PNN: u8 = 0x23; // (+x, -y, -z)
const ROLE_TAG_CORNER_VERTEX_NPP: u8 = 0x24; // (-x, +y, +z)
const ROLE_TAG_CORNER_VERTEX_NPN: u8 = 0x25; // (-x, +y, -z)
const ROLE_TAG_CORNER_VERTEX_NNP: u8 = 0x26; // (-x, -y, +z)
const ROLE_TAG_CORNER_VERTEX_NNN: u8 = 0x27; // (-x, -y, -z)
// CapCornerVertex: high nibble 0x3X, low nibble mirrors CapKind (Top=0, Bottom=1, Start=2, End=3)
const ROLE_TAG_CAP_CORNER_VERTEX_TOP: u8 = 0x30;
const ROLE_TAG_CAP_CORNER_VERTEX_BOTTOM: u8 = 0x31;
const ROLE_TAG_CAP_CORNER_VERTEX_START: u8 = 0x32;
const ROLE_TAG_CAP_CORNER_VERTEX_END: u8 = 0x33;

#[derive(Serialize, Deserialize)]
struct MidSurfaceEdgeRecordOnDisk {
    attribute: TopologyAttributeOnDisk,
    region_pair_a: u32,
    region_pair_b: u32,
}

#[derive(Serialize, Deserialize)]
struct DiagnosticOnDisk {
    /// 0 = `Severity::Info`, 1 = `Severity::Warning`, 2 = `Severity::Error`.
    /// Unknown discriminants on read are rejected with `InvalidData`.
    severity: u8,
    message: String,
    labels: Vec<DiagnosticLabelOnDisk>,
    candidates: Vec<String>,
    // `Diagnostic::code` is intentionally NOT carried — see module-level
    // wire-shape mirror comment above and PRD §7 task-ε ownership note.
}

#[derive(Serialize, Deserialize)]
struct DiagnosticLabelOnDisk {
    span_start: u32,
    span_end: u32,
    message: String,
}

// ---- In-memory ↔ on-disk conversion helpers ----

fn severity_to_u8(s: Severity) -> u8 {
    match s {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn severity_from_u8(b: u8) -> io::Result<Severity> {
    match b {
        0 => Ok(Severity::Info),
        1 => Ok(Severity::Warning),
        2 => Ok(Severity::Error),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ShellExtractionResult unknown Severity discriminant {other}"),
        )),
    }
}

fn classification_to_u8(c: RegionClassification) -> u8 {
    match c {
        RegionClassification::ShellEligible => 0,
        RegionClassification::TetEligible => 1,
        RegionClassification::MixedComponentOfBody => 2,
    }
}

fn classification_from_u8(b: u8) -> io::Result<RegionClassification> {
    match b {
        0 => Ok(RegionClassification::ShellEligible),
        1 => Ok(RegionClassification::TetEligible),
        2 => Ok(RegionClassification::MixedComponentOfBody),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ShellExtractionResult unknown RegionClassification discriminant {other}"),
        )),
    }
}

/// Map `Role` (including its `CapKind` payload) to a pinned `u8` wire tag.
/// The tag table is the explicit constant block above; the match-based
/// projection is decoupled from `Role`'s declaration order so reordering
/// the source enum cannot shift any tag.
fn role_to_u8(r: Role) -> u8 {
    match r {
        Role::Cap(CapKind::Top) => ROLE_TAG_CAP_TOP,
        Role::Cap(CapKind::Bottom) => ROLE_TAG_CAP_BOTTOM,
        Role::Cap(CapKind::Start) => ROLE_TAG_CAP_START,
        Role::Cap(CapKind::End) => ROLE_TAG_CAP_END,
        Role::Side => ROLE_TAG_SIDE,
        Role::NewEdge => ROLE_TAG_NEW_EDGE,
        Role::RevolvedFace => ROLE_TAG_REVOLVED_FACE,
        Role::AxisFace => ROLE_TAG_AXIS_FACE,
        Role::SweptFace => ROLE_TAG_SWEPT_FACE,
        Role::LoftedFace => ROLE_TAG_LOFTED_FACE,
        Role::MidSurfaceFace => ROLE_TAG_MID_SURFACE_FACE,
        Role::MidSurfaceEdge => ROLE_TAG_MID_SURFACE_EDGE,
        Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Pos } => ROLE_TAG_CORNER_VERTEX_PPP,
        Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Neg } => ROLE_TAG_CORNER_VERTEX_PPN,
        Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Pos } => ROLE_TAG_CORNER_VERTEX_PNP,
        Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Neg } => ROLE_TAG_CORNER_VERTEX_PNN,
        Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Pos } => ROLE_TAG_CORNER_VERTEX_NPP,
        Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Neg } => ROLE_TAG_CORNER_VERTEX_NPN,
        Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Pos } => ROLE_TAG_CORNER_VERTEX_NNP,
        Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Neg } => ROLE_TAG_CORNER_VERTEX_NNN,
        Role::CapCornerVertex { face: CapKind::Top } => ROLE_TAG_CAP_CORNER_VERTEX_TOP,
        Role::CapCornerVertex { face: CapKind::Bottom } => ROLE_TAG_CAP_CORNER_VERTEX_BOTTOM,
        Role::CapCornerVertex { face: CapKind::Start } => ROLE_TAG_CAP_CORNER_VERTEX_START,
        Role::CapCornerVertex { face: CapKind::End } => ROLE_TAG_CAP_CORNER_VERTEX_END,
    }
}

/// Inverse of [`role_to_u8`]. Unknown discriminants are rejected with
/// `io::ErrorKind::InvalidData`, mirroring `severity_from_u8` /
/// `classification_from_u8`.
fn role_from_u8(b: u8) -> io::Result<Role> {
    match b {
        ROLE_TAG_CAP_TOP => Ok(Role::Cap(CapKind::Top)),
        ROLE_TAG_CAP_BOTTOM => Ok(Role::Cap(CapKind::Bottom)),
        ROLE_TAG_CAP_START => Ok(Role::Cap(CapKind::Start)),
        ROLE_TAG_CAP_END => Ok(Role::Cap(CapKind::End)),
        ROLE_TAG_SIDE => Ok(Role::Side),
        ROLE_TAG_NEW_EDGE => Ok(Role::NewEdge),
        ROLE_TAG_REVOLVED_FACE => Ok(Role::RevolvedFace),
        ROLE_TAG_AXIS_FACE => Ok(Role::AxisFace),
        ROLE_TAG_SWEPT_FACE => Ok(Role::SweptFace),
        ROLE_TAG_LOFTED_FACE => Ok(Role::LoftedFace),
        ROLE_TAG_MID_SURFACE_FACE => Ok(Role::MidSurfaceFace),
        ROLE_TAG_MID_SURFACE_EDGE => Ok(Role::MidSurfaceEdge),
        ROLE_TAG_CORNER_VERTEX_PPP => Ok(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Pos }),
        ROLE_TAG_CORNER_VERTEX_PPN => Ok(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Neg }),
        ROLE_TAG_CORNER_VERTEX_PNP => Ok(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Pos }),
        ROLE_TAG_CORNER_VERTEX_PNN => Ok(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Neg }),
        ROLE_TAG_CORNER_VERTEX_NPP => Ok(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Pos }),
        ROLE_TAG_CORNER_VERTEX_NPN => Ok(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Neg }),
        ROLE_TAG_CORNER_VERTEX_NNP => Ok(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Pos }),
        ROLE_TAG_CORNER_VERTEX_NNN => Ok(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Neg }),
        ROLE_TAG_CAP_CORNER_VERTEX_TOP => Ok(Role::CapCornerVertex { face: CapKind::Top }),
        ROLE_TAG_CAP_CORNER_VERTEX_BOTTOM => Ok(Role::CapCornerVertex { face: CapKind::Bottom }),
        ROLE_TAG_CAP_CORNER_VERTEX_START => Ok(Role::CapCornerVertex { face: CapKind::Start }),
        ROLE_TAG_CAP_CORNER_VERTEX_END => Ok(Role::CapCornerVertex { face: CapKind::End }),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("ShellExtractionResult unknown Role wire tag {other:#04x}"),
        )),
    }
}

fn topology_attribute_to_disk(t: &TopologyAttribute) -> TopologyAttributeOnDisk {
    TopologyAttributeOnDisk {
        feature_id: t.feature_id.to_string(),
        role: role_to_u8(t.role),
        local_index: t.local_index,
        user_label: t.user_label.clone(),
        mod_history: t
            .mod_history
            .iter()
            .map(|m| ModEntryOnDisk {
                splitting_feature_id: m.splitting_feature_id.to_string(),
                split_index: m.split_index,
            })
            .collect(),
    }
}

fn topology_attribute_from_disk(t: &TopologyAttributeOnDisk) -> io::Result<TopologyAttribute> {
    Ok(TopologyAttribute {
        feature_id: FeatureId::new(t.feature_id.clone()),
        role: role_from_u8(t.role)?,
        local_index: t.local_index,
        user_label: t.user_label.clone(),
        mod_history: t
            .mod_history
            .iter()
            .map(|m| ModEntry {
                splitting_feature_id: FeatureId::new(m.splitting_feature_id.clone()),
                split_index: m.split_index,
            })
            .collect(),
    })
}

fn diagnostic_to_disk(d: &Diagnostic) -> DiagnosticOnDisk {
    DiagnosticOnDisk {
        severity: severity_to_u8(d.severity),
        message: d.message.clone(),
        labels: d
            .labels
            .iter()
            .map(|l| DiagnosticLabelOnDisk {
                span_start: l.span.start,
                span_end: l.span.end,
                message: l.message.clone(),
            })
            .collect(),
        candidates: d.candidates.clone(),
    }
}

fn diagnostic_from_disk(d: &DiagnosticOnDisk) -> io::Result<Diagnostic> {
    let severity = severity_from_u8(d.severity)?;
    let mut out = match severity {
        Severity::Info => Diagnostic::info(d.message.clone()),
        Severity::Warning => Diagnostic::warning(d.message.clone()),
        Severity::Error => Diagnostic::error(d.message.clone()),
    };
    // `code` is documented-lossy → None (already None from Diagnostic::*
    // builders). Field-mutation here keeps the wire-format restoration
    // contained without exporting more builders from reify-types.
    for ld in &d.labels {
        out = out.with_label(DiagnosticLabel::new(
            SourceSpan::new(ld.span_start, ld.span_end),
            ld.message.clone(),
        ));
    }
    out = out.with_candidates(d.candidates.clone());
    Ok(out)
}

// ---- f64 / u32 / i32 slab read/write helpers ----
//
// Mirrors `write_f64_slab` / `read_f64_slab` at `persistent_cache.rs:492 /
// :542`. On LE hosts a zero-copy `bytemuck::cast_slice` reinterprets the
// typed slice as `&[u8]` without copying; on BE hosts a manual per-element
// byte-swap path. Empty input produces zero bytes. On-disk format is
// unconditionally LE regardless of host byte order.

fn write_f64_slab<W: Write>(w: &mut W, slab: &[f64]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        w.write_all(bytemuck::cast_slice::<f64, u8>(slab))
    }
    #[cfg(target_endian = "big")]
    {
        let byte_count = slab.len() * 8;
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(byte_count).map_err(io::Error::other)?;
        for v in slab {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        w.write_all(&buf)
    }
}

fn read_f64_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<f64>> {
    let mut vec: Vec<f64> = Vec::new();
    vec.try_reserve_exact(len).map_err(io::Error::other)?;
    #[cfg(target_endian = "little")]
    {
        let spare = vec.spare_capacity_mut();
        // SAFETY: MaybeUninit<f64> has the same size (8 bytes) and no
        // stricter alignment than u8; from_raw_parts_mut covers the same
        // memory region; u8 has no validity invariants so &mut [u8] on
        // uninit memory is sound; read_exact overwrites every byte.
        let byte_slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 8) };
        r.read_exact(byte_slice)?;
        // SAFETY: capacity >= len; all len*8 bytes initialised by
        // read_exact; f64 is Pod so any bit pattern is valid.
        unsafe {
            vec.set_len(len);
        }
    }
    #[cfg(target_endian = "big")]
    {
        let bytes = len
            .checked_mul(8)
            .ok_or_else(|| io::Error::other("BE read: f64 slab byte size overflow"))?;
        let mut byte_buf: Vec<u8> = Vec::new();
        byte_buf.try_reserve_exact(bytes).map_err(io::Error::other)?;
        byte_buf.resize(bytes, 0u8);
        r.read_exact(&mut byte_buf)?;
        for chunk in byte_buf.chunks_exact(8) {
            vec.push(f64::from_le_bytes(
                chunk.try_into().expect("chunks_exact(8) yields 8-byte slices"),
            ));
        }
    }
    Ok(vec)
}

fn write_u32_slab<W: Write>(w: &mut W, slab: &[u32]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        w.write_all(bytemuck::cast_slice::<u32, u8>(slab))
    }
    #[cfg(target_endian = "big")]
    {
        let byte_count = slab.len() * 4;
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(byte_count).map_err(io::Error::other)?;
        for v in slab {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        w.write_all(&buf)
    }
}

fn read_u32_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<u32>> {
    let mut vec: Vec<u32> = Vec::new();
    vec.try_reserve_exact(len).map_err(io::Error::other)?;
    #[cfg(target_endian = "little")]
    {
        let spare = vec.spare_capacity_mut();
        // SAFETY: see read_f64_slab; u32 is Pod, 4-byte aligned ≥ u8.
        let byte_slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 4) };
        r.read_exact(byte_slice)?;
        // SAFETY: see read_f64_slab.
        unsafe {
            vec.set_len(len);
        }
    }
    #[cfg(target_endian = "big")]
    {
        let bytes = len
            .checked_mul(4)
            .ok_or_else(|| io::Error::other("BE read: u32 slab byte size overflow"))?;
        let mut byte_buf: Vec<u8> = Vec::new();
        byte_buf.try_reserve_exact(bytes).map_err(io::Error::other)?;
        byte_buf.resize(bytes, 0u8);
        r.read_exact(&mut byte_buf)?;
        for chunk in byte_buf.chunks_exact(4) {
            vec.push(u32::from_le_bytes(
                chunk.try_into().expect("chunks_exact(4) yields 4-byte slices"),
            ));
        }
    }
    Ok(vec)
}

/// `[u32; 3]` is Pod, so vertices/triangles flatten via bytemuck on LE.
fn write_u32x3_slab<W: Write>(w: &mut W, slab: &[[u32; 3]]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        w.write_all(bytemuck::cast_slice::<[u32; 3], u8>(slab))
    }
    #[cfg(target_endian = "big")]
    {
        let byte_count = slab.len() * 12;
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(byte_count).map_err(io::Error::other)?;
        for v in slab {
            buf.extend_from_slice(&v[0].to_le_bytes());
            buf.extend_from_slice(&v[1].to_le_bytes());
            buf.extend_from_slice(&v[2].to_le_bytes());
        }
        w.write_all(&buf)
    }
}

fn read_u32x3_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<[u32; 3]>> {
    let mut vec: Vec<[u32; 3]> = Vec::new();
    vec.try_reserve_exact(len).map_err(io::Error::other)?;
    #[cfg(target_endian = "little")]
    {
        let spare = vec.spare_capacity_mut();
        // SAFETY: [u32;3] is Pod (12 bytes, 4-byte aligned); see read_f64_slab.
        let byte_slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 12) };
        r.read_exact(byte_slice)?;
        // SAFETY: see read_f64_slab.
        unsafe {
            vec.set_len(len);
        }
    }
    #[cfg(target_endian = "big")]
    {
        let bytes = len
            .checked_mul(12)
            .ok_or_else(|| io::Error::other("BE read: [u32;3] slab byte size overflow"))?;
        let mut byte_buf: Vec<u8> = Vec::new();
        byte_buf.try_reserve_exact(bytes).map_err(io::Error::other)?;
        byte_buf.resize(bytes, 0u8);
        r.read_exact(&mut byte_buf)?;
        for chunk in byte_buf.chunks_exact(12) {
            let a = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
            let b = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
            let c = u32::from_le_bytes(chunk[8..12].try_into().unwrap());
            vec.push([a, b, c]);
        }
    }
    Ok(vec)
}

fn write_f64x3_slab<W: Write>(w: &mut W, slab: &[[f64; 3]]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        w.write_all(bytemuck::cast_slice::<[f64; 3], u8>(slab))
    }
    #[cfg(target_endian = "big")]
    {
        let byte_count = slab.len() * 24;
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(byte_count).map_err(io::Error::other)?;
        for v in slab {
            buf.extend_from_slice(&v[0].to_le_bytes());
            buf.extend_from_slice(&v[1].to_le_bytes());
            buf.extend_from_slice(&v[2].to_le_bytes());
        }
        w.write_all(&buf)
    }
}

fn read_f64x3_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<[f64; 3]>> {
    let mut vec: Vec<[f64; 3]> = Vec::new();
    vec.try_reserve_exact(len).map_err(io::Error::other)?;
    #[cfg(target_endian = "little")]
    {
        let spare = vec.spare_capacity_mut();
        // SAFETY: [f64;3] is Pod (24 bytes, 8-byte aligned); see read_f64_slab.
        let byte_slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 24) };
        r.read_exact(byte_slice)?;
        // SAFETY: see read_f64_slab.
        unsafe {
            vec.set_len(len);
        }
    }
    #[cfg(target_endian = "big")]
    {
        let bytes = len
            .checked_mul(24)
            .ok_or_else(|| io::Error::other("BE read: [f64;3] slab byte size overflow"))?;
        let mut byte_buf: Vec<u8> = Vec::new();
        byte_buf.try_reserve_exact(bytes).map_err(io::Error::other)?;
        byte_buf.resize(bytes, 0u8);
        r.read_exact(&mut byte_buf)?;
        for chunk in byte_buf.chunks_exact(24) {
            let a = f64::from_le_bytes(chunk[0..8].try_into().unwrap());
            let b = f64::from_le_bytes(chunk[8..16].try_into().unwrap());
            let c = f64::from_le_bytes(chunk[16..24].try_into().unwrap());
            vec.push([a, b, c]);
        }
    }
    Ok(vec)
}

fn write_i32x3_slab<W: Write>(w: &mut W, slab: &[[i32; 3]]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        w.write_all(bytemuck::cast_slice::<[i32; 3], u8>(slab))
    }
    #[cfg(target_endian = "big")]
    {
        let byte_count = slab.len() * 12;
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(byte_count).map_err(io::Error::other)?;
        for v in slab {
            buf.extend_from_slice(&v[0].to_le_bytes());
            buf.extend_from_slice(&v[1].to_le_bytes());
            buf.extend_from_slice(&v[2].to_le_bytes());
        }
        w.write_all(&buf)
    }
}

fn read_i32x3_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<[i32; 3]>> {
    let mut vec: Vec<[i32; 3]> = Vec::new();
    vec.try_reserve_exact(len).map_err(io::Error::other)?;
    #[cfg(target_endian = "little")]
    {
        let spare = vec.spare_capacity_mut();
        // SAFETY: [i32;3] is Pod (12 bytes, 4-byte aligned); see read_f64_slab.
        let byte_slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 12) };
        r.read_exact(byte_slice)?;
        // SAFETY: see read_f64_slab.
        unsafe {
            vec.set_len(len);
        }
    }
    #[cfg(target_endian = "big")]
    {
        let bytes = len
            .checked_mul(12)
            .ok_or_else(|| io::Error::other("BE read: [i32;3] slab byte size overflow"))?;
        let mut byte_buf: Vec<u8> = Vec::new();
        byte_buf.try_reserve_exact(bytes).map_err(io::Error::other)?;
        byte_buf.resize(bytes, 0u8);
        r.read_exact(&mut byte_buf)?;
        for chunk in byte_buf.chunks_exact(12) {
            let a = i32::from_le_bytes(chunk[0..4].try_into().unwrap());
            let b = i32::from_le_bytes(chunk[4..8].try_into().unwrap());
            let c = i32::from_le_bytes(chunk[8..12].try_into().unwrap());
            vec.push([a, b, c]);
        }
    }
    Ok(vec)
}

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

    /// Encoding pipeline mirrors `ElasticResult::serialize_to_writer` at
    /// `crates/reify-eval/src/persistent_cache.rs:700`:
    ///
    /// 1. `zstd::Encoder::new(w, 0)` — level 0 selects zstd's default level
    ///    (3 in zstd 0.13), which is byte-deterministic for identical input.
    ///    Single-threaded only — `Encoder::multithread()` would break
    ///    byte-determinism. Pinned by
    ///    `shell_extraction_result_serialization_is_byte_deterministic`
    ///    (step-7).
    /// 2. `bincode::serialize_into` (bincode 1.3 fixint-LE) for the
    ///    `ShellExtractionResultHeader`. `f64` metric fields are stored as
    ///    `u64` bit patterns so NaN payloads / signed zeros / `INFINITY`
    ///    survive — pinned by
    ///    `shell_extraction_result_serialize_deserialize_round_trip_preserves_all_buffers_bit_exact`
    ///    (step-5).
    /// 3. Raw little-endian slabs in fixed order: vertices, triangles,
    ///    thickness, vertex_labels, triangle_labels, per-region voxels.
    ///    On LE hosts the slabs go via `bytemuck::cast_slice` (zero-copy);
    ///    on BE hosts a per-element `to_le_bytes()` fallback. See
    ///    `write_f64x3_slab` etc. for the byte-order contract.
    ///
    /// Bumping `bincode` past `=1.3` or `zstd` past `0.13` must be paired
    /// with a `FORMAT_VERSION` bump in the same commit, mirroring
    /// `ELASTIC_RESULT_FORMAT_VERSION` (`persistent_cache.rs:296-313`).
    fn serialize_to_writer(&self, w: &mut impl Write) -> io::Result<()> {
        // Level 0 selects zstd's default compression level (3 in zstd
        // 0.13), which is byte-deterministic for identical input. Pinned
        // explicitly — `zstd 0.13` does not currently expose a
        // non-deterministic mode at this level, but byte-determinism is
        // a hard requirement of the persistent-cache PRD. The pin is
        // verified by `shell_extraction_result_serialization_is_byte_deterministic`
        // and `shell_extraction_result_reserialize_after_deserialize_is_byte_identical`;
        // bump the level if a future zstd release breaks default-level
        // determinism. Mirrors the comment block at
        // `persistent_cache.rs:700-710`.
        // Single-threaded only — `Encoder::multithread()` breaks byte-determinism.
        let mut encoder = zstd::Encoder::new(w, 0)?;

        let header = self.build_on_disk_header();
        // Use `bincode_options()` for byte-shape parity with the read
        // side — fixint+LE matches the legacy `bincode::serialize_into`
        // wire shape (cross-pinned by ElasticResult's literal-hex test),
        // and `with_limit` defends the deserialize allocation path
        // (ignored here on serialize, but constraining shared options
        // keeps both sides in lockstep).
        use bincode::Options;
        bincode_options()
            .serialize_into(&mut encoder, &header)
            .map_err(io::Error::other)?;

        // Bulk slab writes in the fixed order pinned by the round-trip test.
        write_f64x3_slab(&mut encoder, &self.mid_surface.vertices)?;
        write_u32x3_slab(&mut encoder, &self.mid_surface.triangles)?;
        write_f64_slab(&mut encoder, &self.mid_surface.thickness)?;
        write_u32_slab(&mut encoder, &self.segmentation.vertex_labels)?;
        write_u32_slab(&mut encoder, &self.segmentation.triangle_labels)?;
        for region in &self.segmentation.regions {
            write_i32x3_slab(&mut encoder, &region.voxels)?;
        }

        encoder.finish()?;
        Ok(())
    }

    /// Inverse of [`Self::serialize_to_writer`]. Error-propagation
    /// discipline mirrors `ElasticResult::deserialize_from_reader` at
    /// `persistent_cache.rs:730`:
    ///   * `zstd::Decoder::new(r)?` — `zstd::Error: Into<io::Error>`.
    ///   * `bincode::deserialize_from(...).map_err(io::Error::other)` —
    ///     `bincode::Error` does NOT implement `Into<io::Error>`.
    ///   * `check_len(...)` rejects oversize length-prefixes BEFORE any
    ///     `Vec` reservation, defending against corrupted/tampered cache
    ///     entries.
    ///   * `read_exact` returns `Err(UnexpectedEof)` on a short slab,
    ///     propagated via `?`.
    fn deserialize_from_reader(r: &mut impl Read) -> io::Result<Self> {
        let mut decoder = zstd::Decoder::new(r)?;
        // `bincode_options()` carries `with_limit(MAX_HEADER_BYTES)`,
        // bounding every bincode-managed Vec inside
        // `ShellExtractionResultHeader` (regions, naming.*, diagnostics,
        // and nested mod_history/labels/candidates). A tampered header
        // claiming an absurd `regions: 1<<60` count would otherwise
        // attempt a giant `Vec::with_capacity` via the seq deserializer's
        // `size_hint` propagation — see `MAX_HEADER_BYTES` rationale
        // block above. Pinned by
        // `shell_extraction_result_deserialize_rejects_oversize_header_via_bincode_limit`.
        use bincode::Options;
        let header: ShellExtractionResultHeader = bincode_options()
            .deserialize_from(&mut decoder)
            .map_err(io::Error::other)?;

        // Bound length-prefix fields BEFORE allocating slabs. The
        // bincode-managed Vecs (regions, naming.*, diagnostics, mod_history,
        // labels, candidates) are already materialised by bincode at this
        // point — bincode's varint/fixint length-prefix decoding errors out
        // on UnexpectedEof if a corrupt header claims more bytes than the
        // stream contains. The slab-length caps below provide additional
        // defense for the bulk-allocation paths that follow.
        let vertices_cap = check_len("vertices", header.vertices_len, MAX_VERTICES)?;
        let triangles_cap = check_len("triangles", header.triangles_len, MAX_TRIANGLES)?;
        let thickness_cap = check_len("thickness", header.thickness_len, MAX_F64_ELEMENTS)?;
        let vertex_labels_cap =
            check_len("vertex_labels", header.vertex_labels_len, MAX_U32_ELEMENTS)?;
        let triangle_labels_cap = check_len(
            "triangle_labels",
            header.triangle_labels_len,
            MAX_U32_ELEMENTS,
        )?;

        // Step-10 length-invariant check: enforce vertices.len() ==
        // thickness.len() at the deserialization boundary. Mirrors the
        // `ShellExtractionResult::new` check so a corrupted entry cannot
        // produce an in-memory value that the constructor would have
        // rejected. Pinned by
        // `shell_extraction_result_deserialize_rejects_length_invariant_violation`
        // (step-9 → step-10).
        if header.vertices_len != header.thickness_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ShellExtractionResult length-invariant violation on cache read: \
                     vertices_len {} != thickness_len {} (corrupted or tampered cache entry?)",
                    header.vertices_len, header.thickness_len
                ),
            ));
        }

        // Parallel-array invariants from SegmentationResult's docstring
        // (`segmentation.rs:181-194`): `vertex_labels` is parallel to
        // `mesh.vertices`, `triangle_labels` is parallel to
        // `mesh.triangles`. Enforce at the deserialization boundary so a
        // tampered cache entry cannot produce a struct whose parallel
        // arrays disagree — the invariant the producer-side
        // `segment_regions` guarantees and that all downstream consumers
        // rely on. Mirrors the principle "deserialize cannot produce a
        // struct new() would have refused" articulated for the
        // vertices/thickness check above.
        if header.vertex_labels_len != header.vertices_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ShellExtractionResult parallel-array invariant violation on cache read: \
                     vertex_labels_len {} != vertices_len {} (corrupted or tampered cache entry?)",
                    header.vertex_labels_len, header.vertices_len
                ),
            ));
        }
        if header.triangle_labels_len != header.triangles_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ShellExtractionResult parallel-array invariant violation on cache read: \
                     triangle_labels_len {} != triangles_len {} \
                     (corrupted or tampered cache entry?)",
                    header.triangle_labels_len, header.triangles_len
                ),
            ));
        }

        // Pre-validate every per-region voxel slab cap before any slab read.
        let mut per_region_voxel_caps: Vec<usize> = Vec::new();
        per_region_voxel_caps
            .try_reserve_exact(header.regions.len())
            .map_err(io::Error::other)?;
        for (i, ron) in header.regions.iter().enumerate() {
            let cap = check_len(
                &format!("regions[{i}].voxels"),
                ron.voxels_len,
                MAX_VOXELS_PER_REGION,
            )?;
            per_region_voxel_caps.push(cap);
        }

        // Bulk slab reads in the same fixed order as serialize_to_writer.
        let vertices = read_f64x3_slab(&mut decoder, vertices_cap)?;
        let triangles = read_u32x3_slab(&mut decoder, triangles_cap)?;
        let thickness = read_f64_slab(&mut decoder, thickness_cap)?;
        let vertex_labels = read_u32_slab(&mut decoder, vertex_labels_cap)?;
        let triangle_labels = read_u32_slab(&mut decoder, triangle_labels_cap)?;

        // Per-region voxel slabs + classification disambiguation.
        let mut regions: Vec<RegionInfo> = Vec::new();
        regions
            .try_reserve_exact(header.regions.len())
            .map_err(io::Error::other)?;
        for (ron, voxel_cap) in header.regions.iter().zip(per_region_voxel_caps.iter()) {
            let voxels = read_i32x3_slab(&mut decoder, *voxel_cap)?;
            regions.push(RegionInfo {
                label: ron.label,
                voxels,
                mean_thickness: f64::from_bits(ron.mean_thickness_bits),
                extent: f64::from_bits(ron.extent_bits),
                thickness_extent_ratio: f64::from_bits(ron.thickness_extent_ratio_bits),
                classification: classification_from_u8(ron.classification)?,
            });
        }

        // Reconstruct naming + diagnostics from the bincode-materialised
        // wire-shape mirrors. `topology_attribute_from_disk` now returns
        // `io::Result` (Role wire-tag validation), so we propagate via `?`
        // on each iteration rather than collecting into a Result directly.
        let mut face_records: Vec<TopologyAttribute> = Vec::new();
        face_records
            .try_reserve_exact(header.naming.face_records.len())
            .map_err(io::Error::other)?;
        for tod in &header.naming.face_records {
            face_records.push(topology_attribute_from_disk(tod)?);
        }
        let mut edges: Vec<MidSurfaceEdgeRecord> = Vec::new();
        edges
            .try_reserve_exact(header.naming.edges.len())
            .map_err(io::Error::other)?;
        for eod in &header.naming.edges {
            edges.push(MidSurfaceEdgeRecord {
                attribute: topology_attribute_from_disk(&eod.attribute)?,
                region_pair: (eod.region_pair_a, eod.region_pair_b),
            });
        }
        let naming = MidSurfaceAttributes {
            face_records,
            edges,
        };
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        diagnostics
            .try_reserve_exact(header.diagnostics.len())
            .map_err(io::Error::other)?;
        for d in &header.diagnostics {
            diagnostics.push(diagnostic_from_disk(d)?);
        }

        Ok(ShellExtractionResult {
            mid_surface: MidSurfaceMesh {
                vertices,
                triangles,
                thickness,
            },
            segmentation: SegmentationResult {
                regions,
                vertex_labels,
                triangle_labels,
            },
            naming,
            solve_time_ms: header.solve_time_ms,
            diagnostics,
        })
    }

    fn uncompressed_byte_size(&self) -> u64 {
        // After zstd decompression, the body layout is (in fixed order):
        //   1. bincode 1.3 fixint-LE encoded `ShellExtractionResultHeader`
        //      (variable size due to the embedded wire-shape mirror `Vec`s
        //      — `regions`, `naming.face_records`, `naming.edges`,
        //      `diagnostics` — each carrying nested `String`/`Vec`/`Option`).
        //   2. f64×3 vertices slab: 24 bytes per vertex (3 × 8).
        //   3. u32×3 triangles slab: 12 bytes per triangle (3 × 4).
        //   4. f64 thickness slab: 8 bytes per element.
        //   5. u32 vertex_labels slab: 4 bytes per element.
        //   6. u32 triangle_labels slab: 4 bytes per element.
        //   7. Per-region i32×3 voxels slabs: 12 bytes per voxel, summed.
        //
        // Pinned by
        // `shell_extraction_result_uncompressed_byte_size_matches_actual_buffer_sum`.
        //
        // `bincode::serialized_size` is used rather than a hardcoded magic
        // constant so that wire-shape mirror struct edits (e.g. adding a new
        // `Diagnostic` field to `DiagnosticOnDisk`) automatically update the
        // size accounting without a manual edit. Mirrors the `ElasticResult`
        // discipline at `persistent_cache.rs:768-797`.
        //
        // The on-disk header is built via `build_on_disk_header`, the same
        // helper `serialize_to_writer` uses — so byte-shape drift between
        // the size accounting and the actual serialization is impossible.
        // The round-trip test pins drift end-to-end against the real bytes.
        let header = self.build_on_disk_header();
        // Pinned options chain matching `serialize_to_writer` — must use
        // the same `bincode_options()` so the byte count this returns is
        // exactly the count written by the encode path.
        use bincode::Options;
        let header_bytes = bincode_options().serialized_size(&header).expect(
            "ShellExtractionResultHeader is composed entirely of serde-derived wire-shape \
             mirrors over plain Vec/Option/String/u8/u32/u64 fields; bincode 1.3 fixint-LE \
             serialization at the size-computation level cannot fail. If a future field with \
             a custom serializer is added, this expect will fire — at which point byte_size \
             accounting must be revisited.",
        );
        let vertices_bytes: u64 = 24 * self.mid_surface.vertices.len() as u64;
        let triangles_bytes: u64 = 12 * self.mid_surface.triangles.len() as u64;
        let thickness_bytes: u64 = 8 * self.mid_surface.thickness.len() as u64;
        let vertex_labels_bytes: u64 = 4 * self.segmentation.vertex_labels.len() as u64;
        let triangle_labels_bytes: u64 = 4 * self.segmentation.triangle_labels.len() as u64;
        let voxel_bytes: u64 = self
            .segmentation
            .regions
            .iter()
            .map(|r| 12 * r.voxels.len() as u64)
            .sum();
        header_bytes
            + vertices_bytes
            + triangles_bytes
            + thickness_bytes
            + vertex_labels_bytes
            + triangle_labels_bytes
            + voxel_bytes
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

    /// Build a fresh `MidSurfaceMesh` with `vertices.len() == thickness.len()`
    /// to use as a known-good fixture for the constructor pin tests.
    fn matched_lengths_mesh() -> MidSurfaceMesh {
        MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0, 1.0],
        }
    }

    fn empty_segmentation_for(mesh: &MidSurfaceMesh) -> SegmentationResult {
        SegmentationResult {
            regions: vec![],
            vertex_labels: vec![u32::MAX; mesh.vertices.len()],
            triangle_labels: vec![u32::MAX; mesh.triangles.len()],
        }
    }

    #[test]
    fn shell_extraction_result_new_rejects_length_invariant_violation() {
        // Construct a deliberately mismatched mesh: 3 vertices vs 2 thicknesses.
        // The constructor must reject with the parallel-array length pair —
        // mirroring `SegmentationError::MeshLengthMismatch`'s shape so the
        // contract is uniform across the crate.
        let bad_mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![1.0, 1.0],
        };
        let segmentation = empty_segmentation_for(&bad_mesh);
        let err = ShellExtractionResult::new(
            bad_mesh,
            segmentation,
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .expect_err("3 vertices vs 2 thicknesses must violate the length invariant");
        assert_eq!(
            err,
            ShellExtractionResultError::LengthInvariantViolation {
                vertices_len: 3,
                thickness_len: 2,
            }
        );
    }

    #[test]
    fn shell_extraction_result_new_accepts_matching_lengths() {
        // Matched lengths (3 == 3) must succeed.
        let good = ShellExtractionResult::new(
            matched_lengths_mesh(),
            empty_segmentation_for(&matched_lengths_mesh()),
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .expect("matched parallel-array lengths must be accepted");
        assert_eq!(good.mid_surface.vertices.len(), good.mid_surface.thickness.len());

        // The empty-mesh case (0 == 0) is also a legal match.
        let empty_mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let empty = ShellExtractionResult::new(
            empty_mesh,
            SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .expect("empty-mesh case (0 == 0) is a legal match");
        assert!(empty.mid_surface.vertices.is_empty());
    }

    #[test]
    fn shell_extraction_result_solve_time_ms_returns_constructor_value() {
        // Build two values with distinct solve_time_ms and confirm the trait
        // accessor reads the field. Mirrors `elastic_result_solve_time_ms_*`
        // at `persistent_cache.rs:1115` — the second sample with
        // solve_time_ms = 0 catches a hard-coded constant.
        let nine_thousand_nine_hundred_ninety_nine = ShellExtractionResult::new(
            MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            MidSurfaceAttributes::default(),
            9999,
            vec![],
        )
        .unwrap();
        assert_eq!(nine_thousand_nine_hundred_ninety_nine.solve_time_ms(), 9999);

        let zero = ShellExtractionResult::new(
            MidSurfaceMesh {
                vertices: vec![],
                triangles: vec![],
                thickness: vec![],
            },
            SegmentationResult {
                regions: vec![],
                vertex_labels: vec![],
                triangle_labels: vec![],
            },
            MidSurfaceAttributes::default(),
            0,
            vec![],
        )
        .unwrap();
        assert_eq!(zero.solve_time_ms(), 0);
    }

    // ---- step-5 round-trip pins ----
    // Diagnostic / DiagnosticLabel / SourceSpan / FeatureId / Role / etc.
    // are already pulled in via `super::*`; only re-import the items not
    // re-exported by super.
    use reify_types::geometry::{CapKind, ModEntry};

    /// Build a non-trivial `ShellExtractionResult` exercising every wire-format
    /// path: special-value f64 components (NaN / ±Inf / -0.0), non-empty u32
    /// label slabs, per-region voxel slabs, region metric f64::INFINITY, a
    /// `TopologyAttribute` with `Cap(Top)` role + `user_label = Some(...)` +
    /// non-empty `mod_history`, a `MidSurfaceEdgeRecord` with `MidSurfaceEdge`
    /// role + `user_label = Some(...)`, and a `Diagnostic` carrying a
    /// `DiagnosticLabel` + non-empty `candidates`.
    fn make_round_trip_fixture() -> ShellExtractionResult {
        let mid_surface = MidSurfaceMesh {
            vertices: vec![
                [f64::NAN, 0.0, -0.0],
                [f64::INFINITY, f64::NEG_INFINITY, std::f64::consts::PI],
                [1.0, 2.0, 3.0],
            ],
            triangles: vec![[0, 1, 2], [2, 1, 0]],
            thickness: vec![f64::NAN, 1.5, 0.0],
        };
        let region_a = RegionInfo {
            label: 0,
            voxels: vec![[0, 0, 0], [1, 0, 0], [2, 0, 0]],
            mean_thickness: 0.25,
            extent: 1.0,
            thickness_extent_ratio: 0.25,
            classification: RegionClassification::ShellEligible,
        };
        let region_b = RegionInfo {
            label: 1,
            voxels: vec![[5, 5, 5]],
            mean_thickness: 1.0,
            extent: 0.0,
            thickness_extent_ratio: f64::INFINITY,
            classification: RegionClassification::TetEligible,
        };
        let segmentation = SegmentationResult {
            regions: vec![region_a, region_b],
            vertex_labels: vec![0, 1, u32::MAX],
            triangle_labels: vec![0, u32::MAX],
        };
        let parent = FeatureId::new("Bracket#realization[0]");
        let derived = FeatureId::derived_mid_surface(&parent);
        let face_record = TopologyAttribute {
            feature_id: derived.clone(),
            role: Role::Cap(CapKind::Top),
            local_index: 7,
            user_label: Some("user-face".to_string()),
            mod_history: vec![ModEntry {
                splitting_feature_id: parent.clone(),
                split_index: 3,
            }],
        };
        let edge_attr = TopologyAttribute {
            feature_id: derived,
            role: Role::MidSurfaceEdge,
            local_index: 0,
            user_label: Some("user-edge".to_string()),
            mod_history: vec![],
        };
        let edge_record = MidSurfaceEdgeRecord {
            attribute: edge_attr,
            region_pair: (0, 1),
        };
        let naming = MidSurfaceAttributes {
            face_records: vec![face_record],
            edges: vec![edge_record],
        };
        let diagnostic = Diagnostic::warning("clipped mid-surface near domain boundary")
            .with_label(DiagnosticLabel::new(
                SourceSpan::new(10, 25),
                "diagnostic-label-message",
            ))
            .with_candidates(vec!["cand-a".to_string(), "cand-b".to_string()]);
        ShellExtractionResult {
            mid_surface,
            segmentation,
            naming,
            solve_time_ms: 4242,
            diagnostics: vec![diagnostic],
        }
    }

    /// Assert that two `f64` values share the exact same bit-pattern. Used
    /// for NaN / Inf / signed-zero round-trip pinning where `PartialEq`
    /// would fail (NaN != NaN) or silently accept sign-bit drift. Mirrors
    /// `persistent_cache.rs:1192` (the ElasticResult NaN bit-pattern pin).
    fn assert_f64_bits_eq(label: &str, a: f64, b: f64) {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{label}: bit-pattern drift (a={a:?}, b={b:?})"
        );
    }

    #[test]
    fn shell_extraction_result_serialize_deserialize_round_trip_preserves_all_buffers_bit_exact() {
        let original = make_round_trip_fixture();
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ShellExtractionResult::deserialize_from_reader(&mut &buf[..]).unwrap();

        // (a) every f64 in mid_surface.vertices round-trips bit-exact.
        assert_eq!(
            decoded.mid_surface.vertices.len(),
            original.mid_surface.vertices.len(),
            "vertices length drift"
        );
        for (i, (d, o)) in decoded
            .mid_surface
            .vertices
            .iter()
            .zip(original.mid_surface.vertices.iter())
            .enumerate()
        {
            for (axis, (dv, ov)) in d.iter().zip(o.iter()).enumerate() {
                assert_f64_bits_eq(&format!("vertices[{i}][{axis}]"), *dv, *ov);
            }
        }

        // (b) every f64 in mid_surface.thickness round-trips bit-exact.
        assert_eq!(
            decoded.mid_surface.thickness.len(),
            original.mid_surface.thickness.len(),
            "thickness length drift"
        );
        for (i, (d, o)) in decoded
            .mid_surface
            .thickness
            .iter()
            .zip(original.mid_surface.thickness.iter())
            .enumerate()
        {
            assert_f64_bits_eq(&format!("thickness[{i}]"), *d, *o);
        }

        // (c) triangles, vertex_labels, triangle_labels round-trip element-equal.
        assert_eq!(decoded.mid_surface.triangles, original.mid_surface.triangles);
        assert_eq!(
            decoded.segmentation.vertex_labels,
            original.segmentation.vertex_labels
        );
        assert_eq!(
            decoded.segmentation.triangle_labels,
            original.segmentation.triangle_labels
        );

        // (d) per-region voxels round-trip element-equal.
        // (e) per-region f64 metrics round-trip bit-exact (incl. INFINITY).
        assert_eq!(
            decoded.segmentation.regions.len(),
            original.segmentation.regions.len()
        );
        for (i, (d, o)) in decoded
            .segmentation
            .regions
            .iter()
            .zip(original.segmentation.regions.iter())
            .enumerate()
        {
            assert_eq!(d.label, o.label, "region[{i}].label");
            assert_eq!(d.voxels, o.voxels, "region[{i}].voxels");
            assert_f64_bits_eq(
                &format!("region[{i}].mean_thickness"),
                d.mean_thickness,
                o.mean_thickness,
            );
            assert_f64_bits_eq(&format!("region[{i}].extent"), d.extent, o.extent);
            assert_f64_bits_eq(
                &format!("region[{i}].thickness_extent_ratio"),
                d.thickness_extent_ratio,
                o.thickness_extent_ratio,
            );
            assert_eq!(
                d.classification, o.classification,
                "region[{i}].classification"
            );
        }

        // (f) solve_time_ms round-trips equal.
        assert_eq!(decoded.solve_time_ms, original.solve_time_ms);

        // (g) naming.face_records and naming.edges round-trip equal under PartialEq.
        assert_eq!(decoded.naming.face_records, original.naming.face_records);
        assert_eq!(decoded.naming.edges, original.naming.edges);

        // (h) diagnostics: severity + message + candidates + labels round-trip;
        // code is documented-lossy → None on round-trip.
        assert_eq!(decoded.diagnostics.len(), original.diagnostics.len());
        for (i, (d, o)) in decoded
            .diagnostics
            .iter()
            .zip(original.diagnostics.iter())
            .enumerate()
        {
            assert_eq!(d.severity, o.severity, "diagnostics[{i}].severity");
            assert_eq!(d.message, o.message, "diagnostics[{i}].message");
            assert_eq!(d.candidates, o.candidates, "diagnostics[{i}].candidates");
            assert_eq!(
                d.labels.len(),
                o.labels.len(),
                "diagnostics[{i}].labels length"
            );
            for (j, (dl, ol)) in d.labels.iter().zip(o.labels.iter()).enumerate() {
                assert_eq!(dl.span, ol.span, "diagnostics[{i}].labels[{j}].span");
                assert_eq!(dl.message, ol.message, "diagnostics[{i}].labels[{j}].message");
            }
            // `code` field is documented as lossy: shell-specific
            // DiagnosticCode variants don't exist yet (task ε owns them),
            // so the wire-shape mirror drops it. Round-trips to None.
            assert_eq!(d.code, None, "diagnostics[{i}].code must round-trip to None");
        }
    }

    #[test]
    fn shell_extraction_result_round_trips_with_empty_buffers() {
        // Pin that all-zero-length slabs and empty nested Vecs round-trip
        // cleanly — the slab loops must not assume "at least one element"
        // (cf. `elastic_result_round_trips_with_empty_field_arrays` at
        // `persistent_cache.rs:1207`).
        let original = ShellExtractionResult {
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
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ShellExtractionResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        assert!(decoded.mid_surface.vertices.is_empty());
        assert!(decoded.mid_surface.triangles.is_empty());
        assert!(decoded.mid_surface.thickness.is_empty());
        assert!(decoded.segmentation.regions.is_empty());
        assert!(decoded.segmentation.vertex_labels.is_empty());
        assert!(decoded.segmentation.triangle_labels.is_empty());
        assert!(decoded.naming.face_records.is_empty());
        assert!(decoded.naming.edges.is_empty());
        assert_eq!(decoded.solve_time_ms, 0);
        assert!(decoded.diagnostics.is_empty());
    }

    // ---- step-7 byte-determinism pins ----

    #[test]
    fn shell_extraction_result_serialization_is_byte_deterministic() {
        // Two independent serializations of the same fixture must produce
        // byte-identical output. Mirrors
        // `elastic_result_serialization_is_byte_deterministic` at
        // `persistent_cache.rs:1152`. The contract holds because:
        //   * zstd `Encoder::new(w, 0)` selects deterministic level-0;
        //   * bincode 1.3 fixint-LE is byte-stable for the header;
        //   * all Vec sources have stable insertion order (no HashMap
        //     iteration in serialize_to_writer).
        // If a future refactor swaps any of those for a non-deterministic
        // source (e.g. iterating an `FxHashSet`), this test fails.
        let a = make_round_trip_fixture();
        let b = make_round_trip_fixture();
        let mut buf_a: Vec<u8> = Vec::new();
        let mut buf_b: Vec<u8> = Vec::new();
        a.serialize_to_writer(&mut buf_a).unwrap();
        b.serialize_to_writer(&mut buf_b).unwrap();
        assert_eq!(buf_a, buf_b);
    }

    #[test]
    fn shell_extraction_result_reserialize_after_deserialize_is_byte_identical() {
        // Mirrors `elastic_result_reserialize_after_deserialize_is_byte_identical`
        // at `persistent_cache.rs:1163`. Serialize → deserialize → reserialize
        // must yield the same bytes; any field-shape drift in the wire
        // mirror (e.g. an `Option<String>` reordered relative to a sibling
        // field) would surface here as a mismatch.
        let original = make_round_trip_fixture();
        let mut bytes_a: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut bytes_a).unwrap();
        let decoded = ShellExtractionResult::deserialize_from_reader(&mut &bytes_a[..]).unwrap();
        let mut bytes_b: Vec<u8> = Vec::new();
        decoded.serialize_to_writer(&mut bytes_b).unwrap();
        assert_eq!(bytes_a, bytes_b);
    }

    // ---- step-9 invariant + truncation pins ----

    /// Hand-build a zstd-framed bincode-encoded `ShellExtractionResultHeader`
    /// so a test can simulate a tampered cache entry without going through
    /// `serialize_to_writer`. Mirrors `encode_header` at
    /// `persistent_cache.rs:1298`. Uses `bincode_options()` so the wire
    /// shape matches the production write path byte-for-byte.
    fn encode_header_only_for_test(header: &ShellExtractionResultHeader) -> Vec<u8> {
        use bincode::Options;
        let mut buf: Vec<u8> = Vec::new();
        let mut encoder = zstd::Encoder::new(&mut buf, 0).unwrap();
        bincode_options()
            .serialize_into(&mut encoder, header)
            .unwrap();
        encoder.finish().unwrap();
        buf
    }

    #[test]
    fn shell_extraction_result_deserialize_rejects_length_invariant_violation() {
        // Tampered/corrupted entry: vertices_len = 4 but thickness_len = 3.
        // The deserialization path must reject with `InvalidData` and a
        // message mentioning the length mismatch BEFORE attempting any
        // slab read — protecting consumers from a struct that the
        // `new()` constructor would have rejected.
        let header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 4,
            triangles_len: 0,
            thickness_len: 3,
            vertex_labels_len: 0,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![],
        };
        let buf = encode_header_only_for_test(&header);
        let err = ShellExtractionResult::deserialize_from_reader(&mut &buf[..])
            .expect_err("length-invariant violation must be rejected");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("length-invariant violation") && msg.contains("vertices_len 4")
                && msg.contains("thickness_len 3"),
            "error message must explain the mismatch, got: {msg}"
        );
    }

    #[test]
    fn role_from_u8_round_trips_every_known_variant_and_rejects_unknown() {
        // Exhaustive round-trip across every `Role` variant — if a future
        // refactor reorders `Role` or adds a variant, this test forces a
        // deliberate update of the wire-tag table (no silent shift).
        let all_roles = [
            Role::Cap(CapKind::Top),
            Role::Cap(CapKind::Bottom),
            Role::Cap(CapKind::Start),
            Role::Cap(CapKind::End),
            Role::Side,
            Role::NewEdge,
            Role::RevolvedFace,
            Role::AxisFace,
            Role::SweptFace,
            Role::LoftedFace,
            Role::MidSurfaceFace,
            Role::MidSurfaceEdge,
            // CornerVertex: high nibble 0x2X, low nibble = bit-packed signs (PPP→NNN)
            Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Pos },
            Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Neg },
            Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Pos },
            Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Neg },
            Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Pos },
            Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Neg },
            Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Pos },
            Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Neg },
            // CapCornerVertex: high nibble 0x3X, low nibble mirrors CapKind (Top→End)
            Role::CapCornerVertex { face: CapKind::Top },
            Role::CapCornerVertex { face: CapKind::Bottom },
            Role::CapCornerVertex { face: CapKind::Start },
            Role::CapCornerVertex { face: CapKind::End },
        ];
        for r in all_roles {
            let tag = role_to_u8(r);
            let decoded = role_from_u8(tag).expect("known tag must decode");
            assert_eq!(decoded, r, "Role {r:?} round-trip via tag {tag:#04x}");
        }
        // Pin tag-table layout: Cap variants live at 0x00-0x03, unit variants
        // at 0x10-0x17, CornerVertex at 0x20-0x27, CapCornerVertex at 0x30-0x33.
        // Any change here forces a `FORMAT_VERSION` bump.
        assert_eq!(role_to_u8(Role::Cap(CapKind::Top)), 0x00);
        assert_eq!(role_to_u8(Role::Cap(CapKind::End)), 0x03);
        assert_eq!(role_to_u8(Role::Side), 0x10);
        assert_eq!(role_to_u8(Role::MidSurfaceEdge), 0x17);
        // CornerVertex: all 8 sign combos are pinned individually (not just endpoints)
        // to lock the bit-pack contract (bit2=x, bit1=y, bit0=z; Pos=0, Neg=1) so
        // that a swap of any two arms in role_to_u8/role_from_u8 fails even if the
        // round-trip still passes.
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Pos }),
            0x20
        ); // PPP → bit2=0,bit1=0,bit0=0
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Neg }),
            0x21
        ); // PPN → bit2=0,bit1=0,bit0=1
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Pos }),
            0x22
        ); // PNP → bit2=0,bit1=1,bit0=0
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Neg, z: AxisSign::Neg }),
            0x23
        ); // PNN → bit2=0,bit1=1,bit0=1
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Pos }),
            0x24
        ); // NPP → bit2=1,bit1=0,bit0=0
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Pos, z: AxisSign::Neg }),
            0x25
        ); // NPN → bit2=1,bit1=0,bit0=1
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Pos }),
            0x26
        ); // NNP → bit2=1,bit1=1,bit0=0
        assert_eq!(
            role_to_u8(Role::CornerVertex { x: AxisSign::Neg, y: AxisSign::Neg, z: AxisSign::Neg }),
            0x27
        ); // NNN → bit2=1,bit1=1,bit0=1
        // CapCornerVertex: all 4 face values pinned individually (Top→Bottom→Start→End).
        assert_eq!(role_to_u8(Role::CapCornerVertex { face: CapKind::Top }), 0x30);
        assert_eq!(role_to_u8(Role::CapCornerVertex { face: CapKind::Bottom }), 0x31);
        assert_eq!(role_to_u8(Role::CapCornerVertex { face: CapKind::Start }), 0x32);
        assert_eq!(role_to_u8(Role::CapCornerVertex { face: CapKind::End }), 0x33);

        // Unknown discriminants (gap in the table) must be rejected with
        // `InvalidData` — mirrors `severity_from_u8` / `classification_from_u8`.
        // Boundary samples cover the edges of each gap region; the full interiors
        // of the CornerVertex (0x28..=0x2F) and CapCornerVertex (0x34..=0x3F) gaps
        // are chained in to close the low-nibble-mask aliasing hole: a hypothetical
        // `b & 0x07` decode regression would alias 0x2F→0x27 / 0x3F→a CapKind and
        // silently pass if only boundary bytes were probed (tamper-evidence per task 3658).
        for unknown in [0x04u8, 0x0Fu8, 0x18u8, 0x40u8, 0xFFu8]
            .into_iter()
            .chain(0x28u8..=0x2F)
            .chain(0x34u8..=0x3F)
        {
            let err =
                role_from_u8(unknown).expect_err(&format!("unknown tag {unknown:#04x} must fail"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        }
    }

    #[test]
    fn shell_extraction_result_deserialize_rejects_vertex_labels_invariant_violation() {
        // Tampered/corrupted entry: vertex_labels_len = 2 but vertices_len = 4
        // (parallel-array invariant from `SegmentationResult` docstring —
        // `vertex_labels` is parallel to `mesh.vertices`). The deserialize
        // path must reject this just as `vertices_len != thickness_len` is
        // rejected. Setting `thickness_len = vertices_len` so the earlier
        // length-invariant check passes and the vertex-labels check fires.
        let header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 4,
            triangles_len: 0,
            thickness_len: 4,
            vertex_labels_len: 2,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![],
        };
        let buf = encode_header_only_for_test(&header);
        let err = ShellExtractionResult::deserialize_from_reader(&mut &buf[..])
            .expect_err("vertex_labels parallel-array violation must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let msg = err.to_string();
        assert!(
            msg.contains("parallel-array invariant violation")
                && msg.contains("vertex_labels_len 2")
                && msg.contains("vertices_len 4"),
            "error message must explain the vertex_labels mismatch, got: {msg}"
        );
    }

    #[test]
    fn shell_extraction_result_deserialize_rejects_triangle_labels_invariant_violation() {
        // Tampered/corrupted entry: triangle_labels_len = 5 but
        // triangles_len = 2 (parallel-array invariant from
        // `SegmentationResult` docstring — `triangle_labels` is parallel to
        // `mesh.triangles`). Earlier checks (vertices==thickness,
        // vertex_labels==vertices) pass so the triangle-labels check fires.
        let header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 0,
            triangles_len: 2,
            thickness_len: 0,
            vertex_labels_len: 0,
            triangle_labels_len: 5,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![],
        };
        let buf = encode_header_only_for_test(&header);
        let err = ShellExtractionResult::deserialize_from_reader(&mut &buf[..])
            .expect_err("triangle_labels parallel-array violation must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let msg = err.to_string();
        assert!(
            msg.contains("parallel-array invariant violation")
                && msg.contains("triangle_labels_len 5")
                && msg.contains("triangles_len 2"),
            "error message must explain the triangle_labels mismatch, got: {msg}"
        );
    }

    /// Acceptable error kinds from a malformed/truncated input. Mirrors
    /// `assert_decode_error` at `persistent_cache.rs:1231`.
    fn assert_decode_error(label: &str, err: &io::Error) {
        let kind = err.kind();
        assert!(
            matches!(
                kind,
                io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData | io::ErrorKind::Other
            ),
            "{label}: unexpected io::ErrorKind {kind:?} (full error: {err:?})"
        );
    }

    #[test]
    fn shell_extraction_result_deserialize_from_truncated_reader_returns_io_error() {
        // Six truncation points exercise distinct decode stages:
        //   * 0 bytes → zstd::Decoder::new fails at frame magic
        //   * 1, 4 bytes → partial frame magic / header
        //   * len/4, len/2 → mid-bincode-header or mid-slab
        //   * len-1 → one byte short of the final block
        // Every offset must surface `Err(io::Error)` panic-free (mirrors
        // `persistent_cache.rs:1243`).
        let original = make_round_trip_fixture();
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let len = buf.len();
        let truncation_points: [usize; 6] = [0, 1, 4, len / 4, len / 2, len - 1];
        for &n in &truncation_points {
            let truncated = &buf[..n];
            let label = format!("truncation @ {n}/{len} bytes");
            let err = ShellExtractionResult::deserialize_from_reader(&mut &truncated[..])
                .expect_err(&format!("{label}: must return Err"));
            assert_decode_error(&label, &err);
        }
    }

    // ---- step-11 uncompressed_byte_size pin ----

    #[test]
    fn shell_extraction_result_uncompressed_byte_size_matches_actual_buffer_sum() {
        // Build a fixture with definite sizes so the manual byte accounting
        // is unambiguous:
        //   * 4 vertices  → 4 × 24 = 96 raw slab bytes
        //   * 2 triangles → 2 × 12 = 24
        //   * 4 thickness → 4 × 8 = 32
        //   * 4 vertex_labels → 16
        //   * 2 triangle_labels → 8
        //   * 2 regions × 3 voxels each → 2 × 36 = 72
        //   * Total slab bytes = 96 + 24 + 32 + 16 + 8 + 72 = 248
        let mid_surface = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            thickness: vec![1.0, 2.0, 3.0, 4.0],
        };
        let region_a = RegionInfo {
            label: 0,
            voxels: vec![[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            mean_thickness: 1.5,
            extent: 1.0,
            thickness_extent_ratio: 1.5,
            classification: RegionClassification::ShellEligible,
        };
        let region_b = RegionInfo {
            label: 1,
            voxels: vec![[5, 5, 5], [5, 5, 6], [5, 5, 7]],
            mean_thickness: 2.5,
            extent: 1.0,
            thickness_extent_ratio: 2.5,
            classification: RegionClassification::TetEligible,
        };
        let segmentation = SegmentationResult {
            regions: vec![region_a, region_b],
            vertex_labels: vec![0, 0, 1, 1],
            triangle_labels: vec![0, 1],
        };
        let value = ShellExtractionResult {
            mid_surface,
            segmentation,
            naming: MidSurfaceAttributes::default(),
            solve_time_ms: 0,
            diagnostics: vec![],
        };

        // Computed sum: header bincode size + slab bytes.
        let header = ShellExtractionResultHeader {
            solve_time_ms: value.solve_time_ms,
            vertices_len: value.mid_surface.vertices.len() as u64,
            triangles_len: value.mid_surface.triangles.len() as u64,
            thickness_len: value.mid_surface.thickness.len() as u64,
            vertex_labels_len: value.segmentation.vertex_labels.len() as u64,
            triangle_labels_len: value.segmentation.triangle_labels.len() as u64,
            regions: value
                .segmentation
                .regions
                .iter()
                .map(|r| RegionInfoOnDisk {
                    label: r.label,
                    voxels_len: r.voxels.len() as u64,
                    mean_thickness_bits: r.mean_thickness.to_bits(),
                    extent_bits: r.extent.to_bits(),
                    thickness_extent_ratio_bits: r.thickness_extent_ratio.to_bits(),
                    classification: classification_to_u8(r.classification),
                })
                .collect(),
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![],
        };
        use bincode::Options;
        let header_bytes = bincode_options().serialized_size(&header).unwrap();
        let slab_bytes: u64 = 96 + 24 + 32 + 16 + 8 + 72;
        let expected = header_bytes + slab_bytes;
        assert_eq!(
            value.uncompressed_byte_size(),
            expected,
            "uncompressed_byte_size must match header bincode size + slab byte sum"
        );

        // Empty-case pin: an all-empty value reports the empty-header
        // bincode size and nothing else.
        let empty = ShellExtractionResult {
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
        let empty_header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 0,
            triangles_len: 0,
            thickness_len: 0,
            vertex_labels_len: 0,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![],
        };
        let empty_header_bytes = bincode_options().serialized_size(&empty_header).unwrap();
        assert_eq!(empty.uncompressed_byte_size(), empty_header_bytes);
    }

    // ---- suggestion 2 (DoS hazard) tamper pin ----

    #[test]
    fn shell_extraction_result_deserialize_rejects_oversize_header_via_bincode_limit() {
        // Build a header that would, if accepted, force bincode's seq
        // deserializer to attempt a `Vec::with_capacity` of multiple-MB.
        // Note: we can't actually serialize a multi-GB header through
        // `encode_header_only_for_test` (the test fixture itself must
        // fit in RAM), so the tamper vector is: craft a header whose
        // *legitimate* bincode-encoded size exceeds `MAX_HEADER_BYTES`.
        //
        // The simplest synthesis: pack `diagnostics` with many entries
        // each carrying a large `message` String — bincode encodes
        // `String` as length-prefix + bytes, so the actual on-disk size
        // is predictable, and at >256 MiB total the limit fires before
        // the read completes.
        //
        // To keep the test fast, we instead pick a SMALLER fixture and
        // monkey-patch the limit indirectly: re-deserialize with a
        // tight limit applied. Since `bincode_options()` is private and
        // its limit is the only one in the wire path, this test
        // verifies the *limit-enforcement contract* by feeding a
        // serialized fixture whose bincode header byte count exceeds
        // a chosen ceiling, then asserting the limit fires.
        //
        // The minimal direct evidence we can produce in this test is
        // that `bincode_options()` returns an `Options + Copy` whose
        // `with_limit` is set to `MAX_HEADER_BYTES`. We can probe this
        // behaviourally by serializing a fixture larger than the limit
        // would allow, but that requires the limit to be reachable from
        // the test side. So instead, we feed bincode a deliberately
        // oversize length-prefix and assert decode fails with `Other`
        // (the io wrapper around bincode::ErrorKind::SizeLimit).
        //
        // Construct a synthetic zstd-framed bincode stream whose first
        // field is u64 = MAX_HEADER_BYTES + 1 (claims more bytes than
        // the limit allows for the `solve_time_ms` u64). The decode
        // path should fail without panicking.
        use bincode::Options;

        // Build a "header" whose serialized form claims a `diagnostics`
        // Vec length of u64::MAX. Since `with_limit` is enforced
        // mid-deserialization, bincode will detect that consuming
        // u64::MAX × DiagnosticOnDisk entries exceeds MAX_HEADER_BYTES
        // and abort. We construct the bytes directly: bincode's fixint
        // encoding of the leading u64 fields, followed by an absurd
        // length prefix on a later Vec.
        //
        // Layout of ShellExtractionResultHeader (bincode fixint-LE):
        //   solve_time_ms: u64   (8 bytes)
        //   vertices_len: u64    (8 bytes)
        //   triangles_len: u64   (8 bytes)
        //   thickness_len: u64   (8 bytes)
        //   vertex_labels_len: u64 (8 bytes)
        //   triangle_labels_len: u64 (8 bytes)
        //   regions: Vec<RegionInfoOnDisk> — u64 length prefix (8 bytes) + entries
        //
        // We claim `regions: u64::MAX`. bincode will read the length
        // prefix, then attempt to deserialize that many entries; with
        // the limit set to MAX_HEADER_BYTES, well before consuming
        // u64::MAX × 41-byte RegionInfoOnDisk entries the limit fires.
        let mut header_bytes: Vec<u8> = Vec::new();
        header_bytes.extend_from_slice(&0u64.to_le_bytes()); // solve_time_ms
        header_bytes.extend_from_slice(&0u64.to_le_bytes()); // vertices_len
        header_bytes.extend_from_slice(&0u64.to_le_bytes()); // triangles_len
        header_bytes.extend_from_slice(&0u64.to_le_bytes()); // thickness_len
        header_bytes.extend_from_slice(&0u64.to_le_bytes()); // vertex_labels_len
        header_bytes.extend_from_slice(&0u64.to_le_bytes()); // triangle_labels_len
        header_bytes.extend_from_slice(&u64::MAX.to_le_bytes()); // regions.len() = u64::MAX

        // zstd-wrap to match what `deserialize_from_reader` expects.
        let mut zstd_buf: Vec<u8> = Vec::new();
        {
            let mut encoder = zstd::Encoder::new(&mut zstd_buf, 0).unwrap();
            encoder.write_all(&header_bytes).unwrap();
            encoder.finish().unwrap();
        }

        let err = ShellExtractionResult::deserialize_from_reader(&mut &zstd_buf[..])
            .expect_err("oversize regions length must be rejected via bincode limit");
        // Acceptable: any io::Error whose payload mentions size-limit or
        // simply errors during decode. Mirrors `assert_decode_error`'s
        // permissive matcher — we care that decode FAILS, not the exact
        // error kind, because bincode's SizeLimit lowers to
        // `io::ErrorKind::Other` via the existing `io::Error::other`
        // wrapper. A panic here would be a clear regression.
        let kind = err.kind();
        assert!(
            matches!(
                kind,
                io::ErrorKind::Other
                    | io::ErrorKind::InvalidData
                    | io::ErrorKind::UnexpectedEof
            ),
            "expected limit-related decode error, got io::ErrorKind {kind:?}: {err:?}"
        );

        // Sanity: confirm the limit is actually wired (not silently 0
        // or u64::MAX). A serialize+deserialize of the empty fixture
        // through `bincode_options()` must succeed — the limit must be
        // larger than the empty header.
        let empty_header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 0,
            triangles_len: 0,
            thickness_len: 0,
            vertex_labels_len: 0,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![],
        };
        let mut round_trip_buf: Vec<u8> = Vec::new();
        bincode_options()
            .serialize_into(&mut round_trip_buf, &empty_header)
            .expect("empty header must serialize within MAX_HEADER_BYTES");
        let _: ShellExtractionResultHeader = bincode_options()
            .deserialize_from(&mut &round_trip_buf[..])
            .expect("empty header must round-trip within MAX_HEADER_BYTES");
    }

    // ---- oversize rejection test: fixture > 5 MiB is rejected by 4 MiB cap ----

    #[test]
    fn shell_extraction_result_deserialize_rejects_header_above_max_header_bytes() {
        // Construct a header whose bincode-encoded size falls between the old
        // 256 MiB cap and the new 4 MiB cap.  A single DiagnosticOnDisk with a
        // ~5 MiB message: above 4 MiB (fires SizeLimit at the new cap) and
        // well below 256 MiB (so this test was a no-op non-rejection at the
        // old cap — proving TDD-RED against 1 << 28).
        //
        // IMPORTANT: the fixture must be serialized through a *local unbounded*
        // options chain — NOT `bincode_options()` — because bincode 1.3's
        // `with_limit` is enforced on both serialize and deserialize.
        // Serializing a 5 MiB header through `bincode_options()` (or through
        // `encode_header_only_for_test`, which delegates to it) would fail at
        // the serialize step, never reaching the production deserialize path.
        use bincode::Options;

        let big_diagnostic = DiagnosticOnDisk {
            severity: 0, // Severity::Info
            message: "x".repeat(5 * 1024 * 1024), // ~5 MiB — above the 4 MiB new cap
            labels: vec![],
            candidates: vec![],
        };
        let oversize_header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 0,
            triangles_len: 0,
            thickness_len: 0,
            vertex_labels_len: 0,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![big_diagnostic],
        };

        // Serialize through an unbounded chain (no with_limit) so the
        // fixture exceeds 4 MiB in bincode bytes.
        let unbounded_opts = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .allow_trailing_bytes();
        let mut bincode_bytes: Vec<u8> = Vec::new();
        unbounded_opts
            .serialize_into(&mut bincode_bytes, &oversize_header)
            .expect("unbounded serialization of 5 MiB fixture must succeed");
        // Precondition: confirm the fixture actually exceeds MAX_HEADER_BYTES.
        // A future bincode-options change (e.g. moving to varint) would
        // silently shrink the encoded size and flip this test from a
        // rejection check to a no-op accept.
        assert!(
            bincode_bytes.len() > (1 << 22),
            "test fixture must exceed MAX_HEADER_BYTES — got {} bytes (fixture sizing bug)",
            bincode_bytes.len()
        );

        // zstd-wrap to match the production wire layout expected by
        // `deserialize_from_reader` (level 0, same as `encode_header`).
        let mut zstd_buf: Vec<u8> = Vec::new();
        {
            let mut encoder = zstd::Encoder::new(&mut zstd_buf, 0).unwrap();
            encoder.write_all(&bincode_bytes).unwrap();
            encoder.finish().unwrap();
        }

        // Production deserialize must reject this — the header byte count
        // exceeds MAX_HEADER_BYTES (1 << 22 = 4 MiB), so bincode fires
        // SizeLimit which lowers to io::ErrorKind::Other.
        let err = ShellExtractionResult::deserialize_from_reader(&mut &zstd_buf[..])
            .expect_err("5 MiB header must be rejected by the 4 MiB MAX_HEADER_BYTES cap");
        let kind = err.kind();
        assert!(
            matches!(
                kind,
                io::ErrorKind::Other
                    | io::ErrorKind::InvalidData
                    | io::ErrorKind::UnexpectedEof
            ),
            "expected SizeLimit-related decode error, got io::ErrorKind {kind:?}: {err:?}"
        );
        // The empty-header sanity check (limit is not accidentally 0) is
        // already covered by the immediately preceding test
        // `shell_extraction_result_deserialize_rejects_oversize_header_via_bincode_limit`
        // — no need to duplicate it here.
    }

    // ---- pin-cap test: brackets MAX_HEADER_BYTES to a ±200 KB window ----
    //
    // Uses fixtures sized just below and just above the 4 MiB constant so
    // that raising MAX_HEADER_BYTES to 16/32/64 MiB without re-examining
    // the security rationale fails at least one assertion.

    #[test]
    fn shell_extraction_result_max_header_bytes_pinned_at_4_mib() {
        // Fixture sizing: a single DiagnosticOnDisk with an N-byte message.
        // With fixint encoding the bincode overhead is ~105 bytes of headers
        // and length prefixes, so encoded ≈ 105 + N bytes.
        //
        // MAX_HEADER_BYTES = 1 << 22 = 4_194_304.
        //
        //   N = 4_000_000 → encoded ≈ 4_000_105 bytes  (< 4_194_304 → must succeed)
        //   N = 4_200_000 → encoded ≈ 4_200_105 bytes  (> 4_194_304 → must be rejected)
        //
        // Runtime assertions on `encoded_len` pin the fixture sizes and
        // guard against a future bincode-options change (e.g. varint) that
        // would silently shift the encoded sizes.
        use bincode::Options;

        let make_header = |msg_len: usize| ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 0,
            triangles_len: 0,
            thickness_len: 0,
            vertex_labels_len: 0,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![DiagnosticOnDisk {
                severity: 0,
                message: "x".repeat(msg_len),
                labels: vec![],
                candidates: vec![],
            }],
        };

        let unbounded_opts = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .allow_trailing_bytes();

        // ---- just-under fixture: must be accepted by bincode_options ----
        let under_header = make_header(4_000_000);
        let mut under_bytes = Vec::new();
        unbounded_opts
            .serialize_into(&mut under_bytes, &under_header)
            .expect("just-under fixture must serialize through unbounded opts");
        assert!(
            under_bytes.len() < (1 << 22),
            "just-under fixture must encode to < MAX_HEADER_BYTES, got {} bytes",
            under_bytes.len()
        );
        let _: ShellExtractionResultHeader = bincode_options()
            .deserialize_from(&mut &under_bytes[..])
            .expect("just-under fixture (< 4 MiB) must be accepted by bincode_options");

        // ---- just-over fixture: must be rejected by bincode_options ----
        let over_header = make_header(4_200_000);
        let mut over_bytes = Vec::new();
        unbounded_opts
            .serialize_into(&mut over_bytes, &over_header)
            .expect("just-over fixture must serialize through unbounded opts");
        assert!(
            over_bytes.len() > (1 << 22),
            "just-over fixture must encode to > MAX_HEADER_BYTES — got {} bytes (fixture sizing bug)",
            over_bytes.len()
        );
        assert!(
            bincode_options()
                .deserialize_from::<_, ShellExtractionResultHeader>(&mut &over_bytes[..])
                .is_err(),
            "just-over fixture (> 4 MiB) must be rejected by bincode_options"
        );
    }

    // ---- producer-headroom test: realistic large fixture fits within cap ----

    #[test]
    fn shell_extraction_result_header_large_legal_serializes_within_cap() {
        // Empirical upper-bound check: a synthetic "worst realistic producer
        // output" header must serialize (and deserialize) successfully through
        // `bincode_options()` — i.e. it must stay within MAX_HEADER_BYTES
        // (4 MiB = 1 << 22). If `bincode_options().serialize_into` panics or
        // returns `Err`, that proves the cap is too tight for legitimate
        // producers and the design decision must be revisited (raise the cap
        // or shrink per-field limits).
        //
        // Fixture sizing rationale (fixint LE encoding, ~bytes per entry):
        //   face_records: 2 000 entries × ~260 bytes
        //       (8+64 feature_id + 1 role + 4 local_index
        //        + 1+8+32 user_label(Some)
        //        + 8+3×(8+32+4) mod_history) ≈ 520 KB
        //   edges: 2 000 entries × ~268 bytes (same + 2×u32 region pair)
        //       ≈ 536 KB
        //   regions: 500 entries × 37 bytes ≈ 18 KB
        //   diagnostics: 50 entries × (1+8+256+8+5×(4+4+8+64)+8)
        //       ≈ 50 × 657 ≈ 33 KB
        //   fixed u64 fields + length prefixes: < 1 KB
        //   Total ≈ 1.1 MiB — comfortably under the 4 MiB cap, validating
        //   the ~4× headroom claim in the MAX_HEADER_BYTES doc-comment.
        use bincode::Options;

        fn make_topo(feature_id: &str) -> TopologyAttributeOnDisk {
            TopologyAttributeOnDisk {
                feature_id: feature_id.to_string(),
                role: ROLE_TAG_SIDE,
                local_index: 0,
                user_label: Some("user_lbl_xxxxxxxxxxxxxxxxxxxxxxxx".to_string()),
                mod_history: (0..3)
                    .map(|_| ModEntryOnDisk {
                        splitting_feature_id: "split_feat_xxxxxxxxxxxxxxxxxxxxxx"
                            .to_string(),
                        split_index: 0,
                    })
                    .collect(),
            }
        }

        let feature_id: String = "face_feature_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
            .to_string(); // 64 chars
        let face_records: Vec<TopologyAttributeOnDisk> =
            (0..2_000).map(|_| make_topo(&feature_id)).collect();
        let edges: Vec<MidSurfaceEdgeRecordOnDisk> = (0..2_000)
            .map(|i| MidSurfaceEdgeRecordOnDisk {
                attribute: make_topo(&feature_id),
                region_pair_a: i as u32,
                region_pair_b: (i + 1) as u32,
            })
            .collect();
        let regions: Vec<RegionInfoOnDisk> = (0..500)
            .map(|i| RegionInfoOnDisk {
                label: i,
                voxels_len: 1_000,
                mean_thickness_bits: 1.5f64.to_bits(),
                extent_bits: 2.0f64.to_bits(),
                thickness_extent_ratio_bits: 0.75f64.to_bits(),
                classification: 0,
            })
            .collect();
        let diag_message: String = "w".repeat(256);
        let diag_label_msg: String = "l".repeat(64);
        let diagnostics: Vec<DiagnosticOnDisk> = (0..50)
            .map(|_| DiagnosticOnDisk {
                severity: 1,
                message: diag_message.clone(),
                labels: (0..5)
                    .map(|_| DiagnosticLabelOnDisk {
                        span_start: 0,
                        span_end: 10,
                        message: diag_label_msg.clone(),
                    })
                    .collect(),
                candidates: vec![],
            })
            .collect();
        let large_legal_header = ShellExtractionResultHeader {
            solve_time_ms: 42_000,
            vertices_len: 1_000_000,
            triangles_len: 2_000_000,
            thickness_len: 1_000_000,
            vertex_labels_len: 1_000_000,
            triangle_labels_len: 2_000_000,
            regions,
            naming: MidSurfaceAttributesOnDisk { face_records, edges },
            diagnostics,
        };

        // Serialize through the production options chain (includes with_limit).
        // A failure here means the fixture exceeded MAX_HEADER_BYTES at the
        // encode step — the cap is too tight for this realistic fixture.
        let mut header_buf: Vec<u8> = Vec::new();
        bincode_options()
            .serialize_into(&mut header_buf, &large_legal_header)
            .expect("large-legal fixture must serialize within 4 MiB MAX_HEADER_BYTES");

        // Confirm it also round-trips through the deserialize path.
        let _: ShellExtractionResultHeader = bincode_options()
            .deserialize_from(&mut &header_buf[..])
            .expect("large-legal fixture must deserialize within MAX_HEADER_BYTES");

        // Explicit size guard so the test fails fast if the fixture grows past
        // the cap — rather than relying solely on the bincode limit error.
        assert!(
            header_buf.len() < (1 << 22),
            "large-legal fixture serialized to {} bytes, exceeds 4 MiB cap",
            header_buf.len()
        );
    }

    // ---- encode-side enforcement test: bincode_options rejects oversized encode ----

    #[test]
    fn shell_extraction_result_bincode_options_enforces_limit_on_encode() {
        // Directly confirm that `bincode_options()` enforces MAX_HEADER_BYTES
        // on the *encode* side. This backs the doc-comment claim that
        // `with_limit` is "enforced on both encode and decode in bincode 1.3"
        // with an actual assertion rather than indirect inference.
        use bincode::Options;

        let oversize_header = ShellExtractionResultHeader {
            solve_time_ms: 0,
            vertices_len: 0,
            triangles_len: 0,
            thickness_len: 0,
            vertex_labels_len: 0,
            triangle_labels_len: 0,
            regions: vec![],
            naming: MidSurfaceAttributesOnDisk {
                face_records: vec![],
                edges: vec![],
            },
            diagnostics: vec![DiagnosticOnDisk {
                severity: 0,
                message: "x".repeat(5 * 1024 * 1024), // ~5 MiB — above MAX_HEADER_BYTES
                labels: vec![],
                candidates: vec![],
            }],
        };

        let mut buf: Vec<u8> = Vec::new();
        let result = bincode_options().serialize_into(&mut buf, &oversize_header);
        assert!(
            result.is_err(),
            "bincode_options().serialize_into must fail for a header exceeding \
             MAX_HEADER_BYTES (encode-side limit not enforced)"
        );
    }
}
