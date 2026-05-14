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
use reify_types::geometry::{CapKind, FeatureId, ModEntry, Role, TopologyAttribute};
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
    role: RoleOnDisk,
    local_index: u32,
    user_label: Option<String>,
    mod_history: Vec<ModEntryOnDisk>,
}

#[derive(Serialize, Deserialize)]
struct ModEntryOnDisk {
    splitting_feature_id: String,
    split_index: u32,
}

#[derive(Serialize, Deserialize)]
enum RoleOnDisk {
    Cap(CapKindOnDisk),
    Side,
    NewEdge,
    RevolvedFace,
    AxisFace,
    SweptFace,
    LoftedFace,
    MidSurfaceFace,
    MidSurfaceEdge,
}

#[derive(Serialize, Deserialize)]
enum CapKindOnDisk {
    Top,
    Bottom,
    Start,
    End,
}

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

fn cap_kind_to_disk(k: CapKind) -> CapKindOnDisk {
    match k {
        CapKind::Top => CapKindOnDisk::Top,
        CapKind::Bottom => CapKindOnDisk::Bottom,
        CapKind::Start => CapKindOnDisk::Start,
        CapKind::End => CapKindOnDisk::End,
    }
}

fn cap_kind_from_disk(k: &CapKindOnDisk) -> CapKind {
    match k {
        CapKindOnDisk::Top => CapKind::Top,
        CapKindOnDisk::Bottom => CapKind::Bottom,
        CapKindOnDisk::Start => CapKind::Start,
        CapKindOnDisk::End => CapKind::End,
    }
}

fn role_to_disk(r: Role) -> RoleOnDisk {
    match r {
        Role::Cap(k) => RoleOnDisk::Cap(cap_kind_to_disk(k)),
        Role::Side => RoleOnDisk::Side,
        Role::NewEdge => RoleOnDisk::NewEdge,
        Role::RevolvedFace => RoleOnDisk::RevolvedFace,
        Role::AxisFace => RoleOnDisk::AxisFace,
        Role::SweptFace => RoleOnDisk::SweptFace,
        Role::LoftedFace => RoleOnDisk::LoftedFace,
        Role::MidSurfaceFace => RoleOnDisk::MidSurfaceFace,
        Role::MidSurfaceEdge => RoleOnDisk::MidSurfaceEdge,
    }
}

fn role_from_disk(r: &RoleOnDisk) -> Role {
    match r {
        RoleOnDisk::Cap(k) => Role::Cap(cap_kind_from_disk(k)),
        RoleOnDisk::Side => Role::Side,
        RoleOnDisk::NewEdge => Role::NewEdge,
        RoleOnDisk::RevolvedFace => Role::RevolvedFace,
        RoleOnDisk::AxisFace => Role::AxisFace,
        RoleOnDisk::SweptFace => Role::SweptFace,
        RoleOnDisk::LoftedFace => Role::LoftedFace,
        RoleOnDisk::MidSurfaceFace => Role::MidSurfaceFace,
        RoleOnDisk::MidSurfaceEdge => Role::MidSurfaceEdge,
    }
}

fn topology_attribute_to_disk(t: &TopologyAttribute) -> TopologyAttributeOnDisk {
    TopologyAttributeOnDisk {
        feature_id: t.feature_id.to_string(),
        role: role_to_disk(t.role),
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

fn topology_attribute_from_disk(t: &TopologyAttributeOnDisk) -> TopologyAttribute {
    TopologyAttribute {
        feature_id: FeatureId::new(t.feature_id.clone()),
        role: role_from_disk(&t.role),
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
    }
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
        let mut encoder = zstd::Encoder::new(w, 0)?;

        let header = ShellExtractionResultHeader {
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
        };
        bincode::serialize_into(&mut encoder, &header).map_err(io::Error::other)?;

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
        let header: ShellExtractionResultHeader =
            bincode::deserialize_from(&mut decoder).map_err(io::Error::other)?;

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
        // wire-shape mirrors.
        let naming = MidSurfaceAttributes {
            face_records: header
                .naming
                .face_records
                .iter()
                .map(topology_attribute_from_disk)
                .collect(),
            edges: header
                .naming
                .edges
                .iter()
                .map(|eod| MidSurfaceEdgeRecord {
                    attribute: topology_attribute_from_disk(&eod.attribute),
                    region_pair: (eod.region_pair_a, eod.region_pair_b),
                })
                .collect(),
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
}
