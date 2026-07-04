//! Linear-elastostatic FEA solver output container: `ElasticResult`,
//! `ShellChannels`, and their `PersistentlyCacheable` wire format.
//!
//! Moved from `reify-eval`'s `persistent_cache.rs` (task A / #4934). The
//! orphan rule forces this island to co-locate: `ElasticResult`'s
//! `PersistentlyCacheable` impl needs the trait (from `reify-core`) and
//! `ElasticResult` in the same crate as each other, and the drift-guard
//! `From<&/PartialElasticResult>` impls need `reify-solver-elastic`. The
//! shared f64-slab codec (`write_f64_slab` / `read_f64_slab` /
//! `check_f64_vec_len` / `MAX_F64_ELEMENTS` / `decode_f64_slab_from_le_bytes`)
//! moves with it and is re-exported `pub` because `reify-eval`'s
//! `BucklingResultCache::PersistentlyCacheable` impl (which stays behind)
//! also uses it — see `.task/plan.json` design_decisions for the full
//! rationale.
//!
//! `reify-eval`'s `persistent_cache.rs` re-exports `ElasticResult`,
//! `ShellChannels`, and `max_deflection_magnitude` at their original
//! `crate::persistent_cache::` / `reify_eval::persistent_cache::` paths.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use reify_core::persistent_cache::PersistentlyCacheable;
use reify_solver_elastic::{BudgetReason, ConvergenceStatus};

/// On-disk-layout version for [`ElasticResult`]. Bump when the encoding
/// format changes (separate from `engine_version_hash`, which invalidates
/// result semantics rather than the wire format). Starting at 1 follows the
/// Reify convention that 0 means "uninitialised / unknown".
///
/// **Wire-format contract:** the `bincode` version in use at serialise time is
/// part of this contract. Bumping `bincode` past the current `=1.3` pin — any
/// release, whether minor (1.3 → 1.4) or major (1.x → 2.x), can alter default
/// integer/varint encoding — MUST be accompanied by a deliberate audit of the
/// new default encoding and, on any encoding-visible change, a bump of this
/// constant in the same commit; otherwise cache entries written under the
/// previous version will silently decode as garbage. The same logic applies to
/// any bump of `zstd` past the `0.13` pin (e.g. 0.13 → 0.14 or 0.x → 1.x).
/// Cross-checked by `elastic_result_format_version_is_3_after_v3_bump`, which
/// forces any FORMAT_VERSION bump to be deliberate. The `=1.3` pin blocks even
/// minor bumps to `bincode`; `0.13` pins `zstd`'s 0.x line — both held in
/// `Cargo.toml`.
///
/// **v1 → v2 bump:** PRD `docs/prds/v0_4/shell-extract-engine-bridge.md`
/// task β added an optional `shell_channels` tail (per-element top/bottom
/// stress + element local→global frame) appended after the existing
/// displacement+stress slabs. v2 readers detect a v1 stream by hitting EOF
/// while probing for the `shell_channels_present` discriminator byte; v1
/// entries are read with `shell_channels: None`.
///
/// **v2 → v3 bump (task #3428 step-4):** extended [`ElasticResult`] with
/// three new resampled channels (divergence/gradient/curl) and a grid spec
/// (bounds_min/max + counts). The new slab lengths and grid fields are encoded
/// in [`ElasticResultHeader`]; the new slabs are written after `stress` and
/// before the shell_channels tail. v2 entries are incompatible (body decode
/// fails → corruption-recovery miss), which is acceptable since
/// `ENGINE_VERSION_HASH` also changes when the source files that produce the
/// new channels are modified.
///
/// **No bump for the task #4942 `aposteriori` addition:** unlike the v2 and v3
/// bumps above, adding [`ElasticResult::aposteriori`] does NOT bump this
/// constant. It is carried by a SECOND probe-byte-gated tail
/// ([`AposterioriHeader`] / [`read_aposteriori_tail`]) appended after the
/// `shell_channels` tail, written only when `aposteriori.is_some()`. A `None`
/// value (every pre-4942 v3 entry, and every non-adaptive solve) therefore
/// serialises to the EXACT pre-4942 v3 byte layout — strictly additive and
/// fully backward-compatible, so no version bump is needed.
const ELASTIC_RESULT_FORMAT_VERSION: u32 = 3;

/// Upper bound on `Vec<f64>` length accepted from a serialized header during
/// [`ElasticResult::deserialize_from_reader`]. A corrupted or tampered cache
/// entry could otherwise advertise a near-`u64::MAX` length, triggering a
/// multi-gigabyte allocation that panics on 32-bit hosts (usize multiplication
/// overflow inside the allocator) or fails outright on 64-bit hosts without
/// overcommit (Windows, some macOS configs, CI sandboxes).
///
/// Sized for FEA solver outputs at workstation scale: `1 << 24` ≈ 16 million
/// `f64`s ≈ 128 MiB. This is orders of magnitude above any plausible
/// per-result workload (a typical structural problem is in the 10K–1M DOF
/// range) but bounded enough that a malicious-but-bound-passing claim cannot
/// weaponise the up-front reservation. The previous limit (`1 << 30` ≈ 8 GiB)
/// was tightened in response to review feedback on the deserialise allocation
/// hazard; pair this with `try_reserve_exact` in
/// [`ElasticResult::deserialize_from_reader`] for defence-in-depth on hosts
/// where even 128 MiB cannot be satisfied.
///
/// Pinned by `check_f64_vec_len_rejects_value_above_workload_limit`,
/// `elastic_result_deserialize_rejects_oversize_displacement_len`, and
/// `elastic_result_deserialize_rejects_oversize_stress_len`.
///
/// `pub`: re-imported by `reify-eval`'s `persistent_cache.rs` for
/// `BucklingResultCache`'s `PersistentlyCacheable` impl, which shares this
/// codec (also referenced from that impl's rustdoc intra-doc links).
pub const MAX_F64_ELEMENTS: u64 = 1 << 24;

// Compile-time assertion that `ElasticResult: PersistentlyCacheable`. Lives at
// module scope (outside `#[cfg(test)]`) so the trait-bound is enforced on every
// build, not only when `cargo test` links. Replaces a previous
// `#[test] fn elastic_result_implements_persistently_cacheable()` that wrapped
// the same compile-time check inside a runtime test wrapper.
const _: fn() = || {
    fn assert_impl<T: PersistentlyCacheable>() {}
    assert_impl::<ElasticResult>();
};

/// Compact bincode-encoded prefix that precedes the raw f64 byte slabs in the
/// zstd-wrapped body. `max_von_mises` is stored as its `u64` bit pattern
/// (NOT as `f64`) so NaN payloads, signaling-NaN bits, and signed zeros
/// survive serde NaN-normalization. Pinned by
/// `elastic_result_round_trip_preserves_nan_and_infinity_bit_patterns` in
/// step-9.
///
/// **v3 additions (task #3428 step-4):** three new slab lengths
/// (`divergence_len`, `gradient_len`, `curl_len`) and nine grid-spec scalar
/// fields (bounds_min/max stored as raw u64 bit-patterns for NaN safety,
/// counts as plain u64).  All appended at the end of the struct so bincode's
/// fixed-field-order encoding places them after the existing fields — a
/// strictly additive extension.
///
/// bincode 1.3 fixint-LE wire size:
///   v2: 8+1+4+8+8+8 = 37 bytes
///   v3: 37 + (3+9)*8 = 37 + 96 = 133 bytes
#[derive(Serialize, Deserialize)]
struct ElasticResultHeader {
    /// Encoded as raw u64 bit-pattern (NOT f64) to preserve NaN payloads
    /// through round-trip; pinned by
    /// `elastic_result_round_trip_preserves_nan_and_infinity_bit_patterns`.
    max_von_mises_bits: u64,
    converged: bool,
    iterations: u32,
    solve_time_ms: u64,
    displacement_len: u64,
    stress_len: u64,
    // ── v3 additions ─────────────────────────────────────────────────────────
    /// Number of f64 values in the divergence slab (stride-1 per grid node).
    divergence_len: u64,
    /// Number of f64 values in the gradient slab (stride-9 per grid node).
    gradient_len: u64,
    /// Number of f64 values in the curl slab (stride-3 per grid node).
    curl_len: u64,
    /// Grid lower bound, axis 0, as raw u64 bit-pattern (NaN-safe).
    grid_bounds_min_x_bits: u64,
    /// Grid lower bound, axis 1, as raw u64 bit-pattern.
    grid_bounds_min_y_bits: u64,
    /// Grid lower bound, axis 2, as raw u64 bit-pattern.
    grid_bounds_min_z_bits: u64,
    /// Grid upper bound, axis 0, as raw u64 bit-pattern.
    grid_bounds_max_x_bits: u64,
    /// Grid upper bound, axis 1, as raw u64 bit-pattern.
    grid_bounds_max_y_bits: u64,
    /// Grid upper bound, axis 2, as raw u64 bit-pattern.
    grid_bounds_max_z_bits: u64,
    /// Element-interval count along axis 0.
    grid_count_x: u64,
    /// Element-interval count along axis 1.
    grid_count_y: u64,
    /// Element-interval count along axis 2.
    grid_count_z: u64,
}

/// v2 tail header (PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` β).
/// Always written/read in v2; absent in v1 entries (detected via probe-byte
/// EOF in [`read_shell_channels_tail`]).
///
/// bincode 1.3 fixint-LE wire size: 1 (`bool`) + 24 (three `u64`) = 25 bytes.
/// `top_len` / `bottom_len` / `frame_len` are zero when `present` is false;
/// kept on the wire unconditionally so the trailer is a fixed 25 bytes
/// regardless of the present flag (simplifies the decoder and keeps `byte_size`
/// accounting agnostic to the discriminator).
#[derive(Serialize, Deserialize)]
struct ShellChannelsHeader {
    present: bool,
    top_len: u64,
    bottom_len: u64,
    frame_len: u64,
}

impl From<&Option<ShellChannels>> for ShellChannelsHeader {
    fn from(opt: &Option<ShellChannels>) -> Self {
        match opt {
            None => ShellChannelsHeader {
                present: false,
                top_len: 0,
                bottom_len: 0,
                frame_len: 0,
            },
            Some(c) => ShellChannelsHeader {
                present: true,
                top_len: c.top.len() as u64,
                bottom_len: c.bottom.len() as u64,
                frame_len: c.frame.len() as u64,
            },
        }
    }
}

/// Read the v2 shell-channels tail, dispatching on probe-byte EOF for
/// backward-compat with v1 entries.
///
/// Strategy: read one byte. If EOF (0 bytes) → v1 stream → return `Ok(None)`.
/// Otherwise that byte is the bincode-encoded `present` discriminator
/// (bincode 1.3 fixint-LE encodes `bool` as `0x00` / `0x01`); decode the
/// three trailing `u64` lens via `read_exact` of the remaining 24 bytes and
/// conditionally read the top/bottom/frame slabs.
///
/// Returning `Ok(None)` on EOF is the v1→v2 backward-compat contract:
/// pre-bump entries deserialize cleanly with `shell_channels: None`. Pinned
/// by `elastic_result_deserialize_of_v1_format_bytes_yields_shell_channels_none`.
fn read_shell_channels_tail<R: Read>(r: &mut R) -> io::Result<Option<ShellChannels>> {
    let mut probe = [0u8; 1];
    let probe_n = r.read(&mut probe)?;
    if probe_n == 0 {
        // v1 stream: nothing after the stress slab.
        return Ok(None);
    }
    let present = match probe[0] {
        0 => false,
        1 => true,
        b => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ShellChannelsHeader.present must be 0 or 1, got {b} \
                     (corrupted or tampered cache entry?)"
                ),
            ));
        }
    };
    // Three u64 lens always follow the bool, even when present = false.
    let mut len_buf = [0u8; 24];
    r.read_exact(&mut len_buf)?;
    let top_len = u64::from_le_bytes(len_buf[0..8].try_into().expect("8 bytes"));
    let bottom_len = u64::from_le_bytes(len_buf[8..16].try_into().expect("8 bytes"));
    let frame_len = u64::from_le_bytes(len_buf[16..24].try_into().expect("8 bytes"));
    if !present {
        // The three lens are reserved-zero when absent (defensive: a tampered
        // entry could advertise a non-zero len with present=false; refuse it
        // because no slabs follow and the decoder would otherwise read the
        // next entry's bytes — or, more often, hit EOF mid-decode).
        if top_len != 0 || bottom_len != 0 || frame_len != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ShellChannelsHeader present=false but lens are non-zero \
                     (top={top_len} bottom={bottom_len} frame={frame_len}); \
                     corrupted or tampered cache entry?"
                ),
            ));
        }
        return Ok(None);
    }
    let top_cap = check_f64_vec_len("shell_channels.top", top_len)?;
    let bottom_cap = check_f64_vec_len("shell_channels.bottom", bottom_len)?;
    let frame_cap = check_f64_vec_len("shell_channels.frame", frame_len)?;
    let top = read_f64_slab(r, top_cap)?;
    let bottom = read_f64_slab(r, bottom_cap)?;
    let frame = read_f64_slab(r, frame_cap)?;
    Ok(Some(ShellChannels { top, bottom, frame }))
}

/// Per-element shell stress + local-frame channels for MITC3 shell elements.
///
/// Layout follows PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` §3:
/// `top` / `bottom` are flattened per-element scalar/vector layouts aligned
/// with the existing `ElasticResult.stress` (which aliases the mid layer);
/// `frame` is the per-element row-major 3×3 local→global rotation matrix,
/// matching the [`ShellFrame::local_to_global`] convention at
/// `crates/reify-solver-elastic/src/shell_assembly.rs`.
///
/// PRD §11 OQ-1 (per-element vs per-vertex) is tactically resolved as
/// per-element here; nodal conversion lives in PRD task θ (GUI populator).
///
/// [`ShellFrame::local_to_global`]: ../../reify-solver-elastic/src/shell_assembly.rs
#[derive(Debug, Clone, PartialEq)]
pub struct ShellChannels {
    /// Per-element stress at z = +t/2 (outer fibre), flattened to match the
    /// layout of `ElasticResult.stress` (the mid layer).
    pub top: Vec<f64>,
    /// Per-element stress at z = -t/2 (inner fibre), flattened.
    pub bottom: Vec<f64>,
    /// Per-element 3×3 local→global rotation matrix, row-major, flattened
    /// (9 `f64` per element). Consumed by the GUI populator (PRD task θ)
    /// to map local-frame channels into global coordinates.
    pub frame: Vec<f64>,
}

/// task #4942 tail header for the optional [`AposterioriEstimate`], appended
/// AFTER the `shell_channels` tail. Unlike [`ShellChannelsHeader`] (which is
/// always written, with an internal `present` flag gating the trailing
/// slabs), this header is written ONLY when `aposteriori.is_some()` — when
/// `None`, NO bytes at all follow the shell_channels tail, so
/// `aposteriori: None` reproduces the EXACT pre-4942 v3 on-disk byte layout
/// (byte-for-byte identical, not merely semantically compatible).
/// [`read_aposteriori_tail`]'s probe-byte EOF check is therefore what encodes
/// `Option<AposterioriEstimate>` a level up — mirroring
/// [`read_shell_channels_tail`]'s v1→v2 detection one level further down the
/// tail chain.
///
/// `convergence_discriminant` doubles as the probe byte: `0` =
/// `ConvergenceStatus::Converged`, `1` = `ConvergenceStatus::NotConverged`
/// (any other value is a corrupted/tampered entry).
///
/// bincode 1.3 fixint-LE wire size: 1 (u8) + 1 (u8) + 8 (u64) + 1 (bool) +
/// 8 (u64) + 1 (bool) + 8 (u64) = 28 bytes, plus the `error_indicator` slab
/// (`error_indicator_len * 8` bytes) when `error_indicator_present`.
#[derive(Serialize, Deserialize)]
struct AposterioriHeader {
    /// `0` = `ConvergenceStatus::Converged`, `1` = `ConvergenceStatus::NotConverged`.
    convergence_discriminant: u8,
    /// `BudgetReason` discriminant (`0..=3`); meaningful only when
    /// `convergence_discriminant == 1`, reserved-zero otherwise.
    budget_reason_discriminant: u8,
    /// `ConvergenceStatus::Converged.final_indicator` as a raw `u64` bit
    /// pattern (NaN-safe, same idiom as `max_von_mises_bits`); reserved-zero
    /// when `convergence_discriminant == 1`.
    final_indicator_bits: u64,
    error_indicator_present: bool,
    /// Reserved-zero when `error_indicator_present == false`; validated by
    /// [`read_aposteriori_tail`] (mirrors `read_shell_channels_tail`'s
    /// present=false/non-zero-len rejection).
    error_indicator_len: u64,
    global_present: bool,
    /// `global_relative_energy_error` as a raw `u64` bit pattern (NaN-safe,
    /// same idiom as `max_von_mises_bits`); reserved-zero when
    /// `global_present == false`.
    global_bits: u64,
}

/// Map a [`BudgetReason`] to its cache-owned wire discriminant. The mapping
/// (`TargetMissed=0/MaxIterations=1/MaxDofs=2/Stalled=3`) is owned by this
/// cache layer rather than derived from the enum's declaration order, so a
/// future reordering of `BudgetReason`'s variants cannot silently reshuffle
/// the on-disk encoding.
fn budget_reason_to_u8(reason: &BudgetReason) -> u8 {
    match reason {
        BudgetReason::TargetMissed => 0,
        BudgetReason::MaxIterations => 1,
        BudgetReason::MaxDofs => 2,
        BudgetReason::Stalled => 3,
    }
}

/// Inverse of [`budget_reason_to_u8`]. Rejects any value outside `0..=3` with
/// `io::ErrorKind::InvalidData` — mirrors [`read_shell_channels_tail`]'s
/// strict rejection of an out-of-range `present` byte, turning a corrupt or
/// forward-version tail into a clean cache-miss error instead of a
/// wrong-but-plausible reconstruction.
fn budget_reason_from_u8(b: u8) -> io::Result<BudgetReason> {
    match b {
        0 => Ok(BudgetReason::TargetMissed),
        1 => Ok(BudgetReason::MaxIterations),
        2 => Ok(BudgetReason::MaxDofs),
        3 => Ok(BudgetReason::Stalled),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "AposterioriHeader.budget_reason_discriminant must be 0..=3, got {other} \
                 (corrupted or tampered cache entry?)"
            ),
        )),
    }
}

impl From<&AposterioriEstimate> for AposterioriHeader {
    fn from(est: &AposterioriEstimate) -> Self {
        let (convergence_discriminant, budget_reason_discriminant, final_indicator_bits) =
            match &est.convergence_status {
                ConvergenceStatus::Converged { final_indicator } => {
                    (0u8, 0u8, final_indicator.to_bits())
                }
                ConvergenceStatus::NotConverged { reason } => {
                    (1u8, budget_reason_to_u8(reason), 0u64)
                }
            };
        let (error_indicator_present, error_indicator_len) = match &est.error_indicator {
            Some(v) => (true, v.len() as u64),
            None => (false, 0u64),
        };
        let (global_present, global_bits) = match est.global_relative_energy_error {
            Some(v) => (true, v.to_bits()),
            None => (false, 0u64),
        };
        AposterioriHeader {
            convergence_discriminant,
            budget_reason_discriminant,
            final_indicator_bits,
            error_indicator_present,
            error_indicator_len,
            global_present,
            global_bits,
        }
    }
}

/// Read the task #4942 aposteriori tail, dispatching on probe-byte EOF:
/// `aposteriori: None` at write time (non-adaptive solves, and every
/// pre-4942 cache entry) writes NO bytes at all for this tail, so the probe
/// read here returns `Ok(0)` and this function returns `Ok(None)` —
/// mirroring [`read_shell_channels_tail`]'s v1-stream detection one level up
/// the tail chain.
///
/// The `convergence_discriminant` probe byte is validated for range
/// IMMEDIATELY on read, before attempting to read the rest of the fixed
/// header — a malformed single-byte tail (e.g. from a truncated/tampered
/// entry) must surface `InvalidData`, not `UnexpectedEof` from a subsequent
/// short `read_exact`.
fn read_aposteriori_tail<R: Read>(r: &mut R) -> io::Result<Option<AposterioriEstimate>> {
    let mut probe = [0u8; 1];
    let probe_n = r.read(&mut probe)?;
    if probe_n == 0 {
        // No aposteriori tail was written: a non-adaptive result, or a
        // pre-4942 cache entry that predates this field entirely.
        return Ok(None);
    }
    let convergence_discriminant = probe[0];
    if convergence_discriminant > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "AposterioriHeader.convergence_discriminant must be 0 or 1, got \
                 {convergence_discriminant} (corrupted or tampered cache entry?)"
            ),
        ));
    }
    // Remaining fixed-size AposterioriHeader fields (27 bytes): budget_reason_discriminant
    // (u8) + final_indicator_bits (u64) + error_indicator_present (bool) +
    // error_indicator_len (u64) + global_present (bool) + global_bits (u64)
    // = 1 + 8 + 1 + 8 + 1 + 8 = 27 bytes.
    let mut rest = [0u8; 27];
    r.read_exact(&mut rest)?;
    let budget_reason_discriminant = rest[0];
    let final_indicator_bits = u64::from_le_bytes(rest[1..9].try_into().expect("8 bytes"));
    let error_indicator_present_byte = rest[9];
    let error_indicator_len = u64::from_le_bytes(rest[10..18].try_into().expect("8 bytes"));
    let global_present_byte = rest[18];
    let global_bits = u64::from_le_bytes(rest[19..27].try_into().expect("8 bytes"));

    let convergence_status = if convergence_discriminant == 0 {
        ConvergenceStatus::Converged {
            final_indicator: f64::from_bits(final_indicator_bits),
        }
    } else {
        ConvergenceStatus::NotConverged {
            reason: budget_reason_from_u8(budget_reason_discriminant)?,
        }
    };

    let error_indicator_present = match error_indicator_present_byte {
        0 => false,
        1 => true,
        b => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "AposterioriHeader.error_indicator_present must be 0 or 1, got {b} \
                     (corrupted or tampered cache entry?)"
                ),
            ));
        }
    };
    if !error_indicator_present && error_indicator_len != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "AposterioriHeader error_indicator_present=false but error_indicator_len=\
                 {error_indicator_len} (corrupted or tampered cache entry?)"
            ),
        ));
    }
    let error_indicator = if error_indicator_present {
        let cap = check_f64_vec_len("aposteriori.error_indicator", error_indicator_len)?;
        Some(read_f64_slab(r, cap)?)
    } else {
        None
    };

    let global_present = match global_present_byte {
        0 => false,
        1 => true,
        b => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "AposterioriHeader.global_present must be 0 or 1, got {b} \
                     (corrupted or tampered cache entry?)"
                ),
            ));
        }
    };
    let global_relative_energy_error = if global_present {
        Some(f64::from_bits(global_bits))
    } else {
        None
    };

    Ok(Some(AposterioriEstimate {
        convergence_status,
        error_indicator,
        global_relative_energy_error,
    }))
}

/// Adaptive-refinement a-posteriori estimate (task #4942): the 3 fields the
/// FEA adaptive trampoline (PRD tasks #4902/#4910) produces which are not
/// captured by any of `ElasticResult`'s other channels. `None` on
/// `ElasticResult` for non-adaptive solves (the progressive solver's partial
/// snapshots never run an adaptive loop) and for every pre-4942 cache entry.
///
/// Persisted as a SECOND probe-byte-gated tail appended AFTER the
/// `shell_channels` tail — see [`AposterioriHeader`] / [`read_aposteriori_tail`].
#[derive(Debug, Clone, PartialEq)]
pub struct AposterioriEstimate {
    /// The adaptive loop's stop reason / confidence signal.
    pub convergence_status: ConvergenceStatus,
    /// Per-grid-node a-posteriori error indicator, stride-1, resampled onto
    /// the SAME `Regular3D` grid as `ElasticResult::divergence` / `gradient` /
    /// `curl` (`grid_bounds_min`/`grid_bounds_max` + `grid_counts`). `None`
    /// when the adaptive trampoline did not produce a spatial indicator field.
    pub error_indicator: Option<Vec<f64>>,
    /// Final global relative energy-norm error (dimensionless), when the
    /// adaptive trampoline computed one.
    pub global_relative_energy_error: Option<f64>,
}

/// Linear-elastostatic FEA solver output container.
///
/// Field set is fixed by the PRD: per-DOF displacement and stress arrays,
/// a `max_von_mises` scalar summary, a `converged` flag, an `iterations`
/// count, a `solve_time_ms` cost metric for cache eviction, and an optional
/// [`ShellChannels`] tail for shell-classified bodies (PRD
/// `docs/prds/v0_4/shell-extract-engine-bridge.md` §3 — populated by the
/// FEA trampoline in PRD task δ; absent / `None` for tet-only bodies).
///
/// **v3 additions (task #3428 step-4):** three new resampled Regular3D
/// channels (divergence stride-1, gradient stride-9, curl stride-3) plus the
/// grid spec (bounds_min/max and element-interval counts) needed to faithfully
/// reconstruct the `Value::Field` SampledField without re-solving.  The `From
/// <PartialElasticResult>` impls carry neutral defaults (empty vecs, zero
/// bounds) because the progressive solver does not produce grid-resampled
/// channels.
#[derive(Debug, Clone, PartialEq)]
pub struct ElasticResult {
    pub displacement: Vec<f64>,
    pub stress: Vec<f64>,
    pub max_von_mises: f64,
    pub converged: bool,
    pub iterations: u32,
    pub solve_time_ms: u64,
    /// Optional shell-element channels. `None` for tet-only bodies (the
    /// historical / v1 case); `Some(_)` for shell-classified bodies whose
    /// trampoline populates per-element top/bottom stress + local frames.
    /// Tet-only consumers ignore this field; the `result.stress` alias
    /// contract at stdlib `solver_elastic.ri:325-328`
    /// (`ShellStress.homogeneous(field).mid`) is unchanged.
    pub shell_channels: Option<ShellChannels>,
    // ── v3 additions (task #3428 step-4) ─────────────────────────────────────
    /// Grid lower bounds per axis (SI units). Matches `GridSpec::bounds_min`.
    pub grid_bounds_min: [f64; 3],
    /// Grid upper bounds per axis (SI units). Matches `GridSpec::bounds_max`.
    pub grid_bounds_max: [f64; 3],
    /// Element-interval counts per axis. Grid has `counts[i]+1` nodes along
    /// axis i. Stored as u64 for schema-stable serialisation.
    pub grid_counts: [u64; 3],
    /// Divergence field data: `tr(∇u)` per grid node, stride-1.
    pub divergence: Vec<f64>,
    /// Displacement-gradient field data: row-major ∇u per grid node, stride-9.
    pub gradient: Vec<f64>,
    /// Curl field data: `∇×u` per grid node, stride-3.
    pub curl: Vec<f64>,
    /// Optional a-posteriori adaptive-refinement estimate (task #4942).
    /// `None` for a non-adaptive solve OR a pre-#4942 cache entry — both
    /// reconstruct to the same non-adaptive default triple in `reify-eval`'s
    /// Value bridge. Persisted as a probe-byte-gated tail appended AFTER the
    /// shell_channels tail; see [`AposterioriEstimate`].
    pub aposteriori: Option<AposterioriEstimate>,
}

/// Compute the maximum per-point L2 norm from a stride-3 displacement buffer.
///
/// The buffer layout is `[u_x0, u_y0, u_z0, u_x1, u_y1, u_z1, ...]`.
/// Each triplet `(dx, dy, dz)` represents one node's displacement vector; this
/// function returns `max_i sqrt(dx_i² + dy_i² + dz_i²)`.
///
/// - Returns `0.0` for an empty buffer; trailing 1-2 elements of a non-multiple-of-3
///   buffer are silently ignored by `chunks_exact(3)`.
/// - Skips any triplet containing a non-finite component (`NaN`, `±Inf`)
///   defensively, so a single degenerate node does not dominate the result.
///
/// This is the single-`ElasticResult` scalar counterpart to the
/// `MultiCaseResult` `envelope_displacement_magnitude` builtin.
/// The `.ri` / gate exposure is deferred to consumer task #3787.
pub fn max_deflection_magnitude(displacement: &[f64]) -> f64 {
    displacement
        .chunks_exact(3)
        .filter_map(|chunk| {
            let (dx, dy, dz) = (chunk[0], chunk[1], chunk[2]);
            if dx.is_finite() && dy.is_finite() && dz.is_finite() {
                Some((dx * dx + dy * dy + dz * dz).sqrt())
            } else {
                None
            }
        })
        .fold(0.0_f64, f64::max)
}

impl ElasticResult {
    /// Scalar maximum displacement magnitude: the largest per-node L2 norm of
    /// the stride-3 `displacement` buffer.
    ///
    /// Delegates to [`max_deflection_magnitude`].
    pub fn max_deflection(&self) -> f64 {
        max_deflection_magnitude(&self.displacement)
    }
}

/// Compile-time drift guard between [`reify_solver_elastic::progressive::PartialElasticResult`]
/// and [`ElasticResult`].
///
/// The five shared fields (`displacement`, `stress`, `max_von_mises`, `converged`,
/// `iterations`) are mapped by name with their exact types.  The two
/// `ElasticResult`-only fields receive documented neutral defaults:
///
/// - `solve_time_ms: 0` — a mid-solve partial snapshot has no eviction-cost
///   metric; the caller fills this in when it promotes a snapshot to a final
///   cache entry.
/// - `shell_channels: None` — the v0_3 progressive solver is tet-based; shell
///   channels are populated only by the v0_4 FEA trampoline (PRD task δ).
///
/// **Guard mechanism:** the struct literal below is EXHAUSTIVE (no `..` spread).
/// Any of the following changes therefore becomes a hard compile error in CI:
/// - renaming a shared field in either struct (rustc E0560 "unknown field" on the
///   source access; E0026 / E0063 on the destination literal)
/// - changing a shared field's type (e.g. `iterations: u32` → `usize`)
/// - adding a new field to `ElasticResult` without a corresponding mapping here
///   (rustc E0063 "missing field `…` in initializer of `ElasticResult`")
///
/// **Asymmetric add-coverage:** renames and type-changes are caught on *both*
/// sides, but new-field coverage is one-directional — a field added to
/// `ElasticResult` is caught by E0063; a new field added *only* to
/// `PartialElasticResult` is silently ignored (Rust does not require exhaustive
/// source-field reads in a by-ref or by-value conversion).
impl From<&reify_solver_elastic::progressive::PartialElasticResult> for ElasticResult {
    fn from(partial: &reify_solver_elastic::progressive::PartialElasticResult) -> Self {
        ElasticResult {
            displacement: partial.displacement.clone(),
            stress: partial.stress.clone(),
            max_von_mises: partial.max_von_mises,
            converged: partial.converged,
            iterations: partial.iterations,
            solve_time_ms: 0,
            shell_channels: None,
            // v3 fields: the progressive solver is tet-only and does not produce
            // grid-resampled channels; neutral defaults are safe because partial
            // results are never written to the persistent cache.
            grid_bounds_min: [0.0; 3],
            grid_bounds_max: [0.0; 3],
            grid_counts: [0; 3],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            // The progressive solver runs no a-posteriori refinement loop.
            aposteriori: None,
        }
    }
}

/// By-value variant: moves `displacement` and `stress` instead of cloning them.
/// Prefer this when the caller consumes the [`PartialElasticResult`] at promotion
/// time and does not need to retain it afterwards — avoids a potentially large
/// double-allocation for refined meshes.  The by-ref impl above is appropriate
/// when the snapshot must remain valid after conversion (e.g. for snapshot reuse).
///
/// The same exhaustive-literal drift-guard applies here.
///
/// [`PartialElasticResult`]: reify_solver_elastic::progressive::PartialElasticResult
impl From<reify_solver_elastic::progressive::PartialElasticResult> for ElasticResult {
    fn from(partial: reify_solver_elastic::progressive::PartialElasticResult) -> Self {
        ElasticResult {
            displacement: partial.displacement,
            stress: partial.stress,
            max_von_mises: partial.max_von_mises,
            converged: partial.converged,
            iterations: partial.iterations,
            solve_time_ms: 0,
            shell_channels: None,
            // v3 fields: same neutral defaults as the by-ref impl.
            grid_bounds_min: [0.0; 3],
            grid_bounds_max: [0.0; 3],
            grid_counts: [0; 3],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        }
    }
}

/// Validate a header-declared `Vec<f64>` length against [`MAX_F64_ELEMENTS`]
/// before it is fed to a `Vec` reservation. Returns the length cast to `usize`
/// on success, or `io::Error(InvalidData)` with a descriptive message on
/// overflow. The cast is safe post-check because `MAX_F64_ELEMENTS = 1<<24`
/// fits in `u32`, so it cannot truncate even on a 32-bit `usize`.
///
/// `pub`: re-imported by `reify-eval`'s `persistent_cache.rs` for
/// `BucklingResultCache`'s `PersistentlyCacheable` impl, which shares this
/// codec.
pub fn check_f64_vec_len(field_name: &str, len: u64) -> io::Result<usize> {
    if len > MAX_F64_ELEMENTS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "ElasticResult {field_name} length {len} exceeds limit {MAX_F64_ELEMENTS} \
                 (corrupted or tampered cache entry?)"
            ),
        ));
    }
    Ok(len as usize)
}

/// Write a slice of `f64` values to `w` in unconditionally little-endian
/// byte order.
///
/// On little-endian hosts (the common case) the native f64 bytes are already
/// little-endian, so `bytemuck::cast_slice::<f64, u8>` reinterprets the
/// `&[f64]` buffer as `&[u8]` without any copy — a zero-copy fast path. On
/// big-endian hosts a temporary `Vec<u8>` is built via `to_le_bytes()` per
/// element (per-element CPU byte-swap, single bulk `write_all` to `w`). The
/// BE path uses `try_reserve_exact` for OOM-safe sizing; overflow of the byte
/// count is impossible because the slice already exists in memory, so its byte
/// length (`slab.len() * 8`) is by construction representable in `usize` on
/// any supported target.
///
/// Empty input produces zero bytes on disk. The on-disk format is
/// unconditionally little-endian regardless of host byte order.
///
/// `pub`: re-imported by `reify-eval`'s `persistent_cache.rs` for
/// `BucklingResultCache`'s `PersistentlyCacheable` impl, which shares this
/// codec.
pub fn write_f64_slab<W: Write>(w: &mut W, slab: &[f64]) -> io::Result<()> {
    #[cfg(target_endian = "little")]
    {
        w.write_all(bytemuck::cast_slice::<f64, u8>(slab))
    }
    #[cfg(target_endian = "big")]
    {
        // The slice already exists in memory, so its byte length
        // (slab.len() * 8) is by construction representable in usize on any
        // supported target — no overflow is possible.
        let byte_count = slab.len() * 8;
        let mut buf: Vec<u8> = Vec::new();
        buf.try_reserve_exact(byte_count)
            .map_err(io::Error::other)?;
        for v in slab {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        w.write_all(&buf)
    }
}

/// Read `len` little-endian `f64` values from `r` and return them as a
/// freshly allocated `Vec<f64>`.
///
/// The caller is responsible for validating `len` against
/// [`MAX_F64_ELEMENTS`] (via [`check_f64_vec_len`]) before calling this
/// function; `len: usize` arrives pre-validated so no field-name parameter
/// is needed here.
///
/// On little-endian hosts `read_exact` fills the `Vec<f64>` backing store
/// directly in a single call via `spare_capacity_mut` — no intermediate byte
/// buffer and no zero-initialisation pass. The previous LE path called
/// `resize(cap, 0.0_f64)` before the cast, which zeroed up to 128 MiB per
/// slab at the `MAX_F64_ELEMENTS = 1<<24` cap — immediately overwritten by
/// `read_exact`. `set_len` is called only after `read_exact` returns `Ok`,
/// saving up to 256 MiB of zeroing per cache lookup (displacement + stress)
/// and keeping the `unsafe` scope as narrow as possible. On big-endian hosts a
/// temporary `Vec<u8>` byte buffer is allocated, filled via `read_exact`,
/// then converted element-by-element via `f64::from_le_bytes` (byte-swap on
/// BE — the BE path already avoids zero-init: it `push`es each `f64` directly
/// from `chunks_exact(8)`).
///
/// `try_reserve_exact` surfaces allocation failure as `io::Error` rather than
/// aborting via `Vec::with_capacity`'s panic-on-OOM path. `checked_mul(8)` on
/// the BE byte-buffer sizing guards against a future increase to
/// `MAX_F64_ELEMENTS` silently overflowing the byte count.
///
/// `read_exact` returns `Err(UnexpectedEof)` on a short slab; the `?`
/// propagates before `set_len` is reached, so no partially-initialised `Vec`
/// is ever observed.
///
/// `pub`: re-imported by `reify-eval`'s `persistent_cache.rs` for
/// `BucklingResultCache`'s `PersistentlyCacheable` impl, which shares this
/// codec.
pub fn read_f64_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<f64>> {
    let mut vec: Vec<f64> = Vec::new();
    vec.try_reserve_exact(len).map_err(io::Error::other)?;
    #[cfg(target_endian = "little")]
    {
        // Fill via spare_capacity_mut so that set_len is only called after
        // read_exact succeeds. This avoids materialising &mut [f64] to
        // uninitialised memory: spare_capacity_mut() yields
        // &mut [MaybeUninit<f64>], which is always legal to hold regardless of
        // the underlying bytes' state.
        let spare = vec.spare_capacity_mut(); // &mut [MaybeUninit<f64>], len >= len
        // SAFETY: MaybeUninit<f64> has the same size (8 bytes) and no stricter
        // alignment than u8. from_raw_parts_mut with len*8 covers the same
        // memory region as the first `len` MaybeUninit<f64> slots. Materialising
        // &mut [u8] to uninitialised bytes is sound because u8 has no validity
        // invariants; we immediately overwrite every byte via read_exact.
        let byte_slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 8) };
        r.read_exact(byte_slice)?;
        // SAFETY: (a) capacity >= len after the successful try_reserve_exact
        // above; (b) all len*8 bytes are now initialised — read_exact returned
        // Ok(()), so every byte in the backing store was written; (c) f64 is
        // Pod / AnyBitPattern so any bit pattern is a valid f64. set_len is
        // only reached on the Ok path, so no partially-uninitialised Vec exists.
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
        byte_buf
            .try_reserve_exact(bytes)
            .map_err(io::Error::other)?;
        byte_buf.resize(bytes, 0u8);
        r.read_exact(&mut byte_buf)?;
        vec.extend(decode_f64_slab_from_le_bytes(&byte_buf));
    }
    Ok(vec)
}

/// Conversion-only kernel of the BE `read_f64_slab` branch, extracted so the
/// `chunks_exact(8) → f64::from_le_bytes` algorithm can be exercised on any host.
///
/// Returns a lazy iterator that decodes `f64` values from `bytes` in
/// little-endian order, consuming 8 bytes at a time. No intermediate `Vec` is
/// allocated — on the BE call site in `read_f64_slab`, `vec.extend(...)` pushes
/// each decoded `f64` directly into the pre-reserved output vector, avoiding the
/// extra heap allocation and copy that a `Vec<f64>`-returning signature would
/// require.
///
/// **Alignment contract:** `bytes.len()` must be a multiple of 8. A
/// `debug_assert_eq!` at entry enforces this in debug builds; in release builds
/// `chunks_exact(8)` silently ignores any trailing bytes. All callers pass
/// `len * 8` bytes (guaranteed by the `checked_mul(8)` guard and `read_exact` in
/// `read_f64_slab`).
///
/// The BE branch of `read_f64_slab` is `#[cfg(target_endian = "big")]`-gated
/// and unreachable on LE CI hosts; calling `read_f64_slab` from a test on a LE
/// host exercises the LE `set_len` fast path — NOT the `chunks_exact(8) →
/// f64::from_le_bytes` algorithm. Extracting the conversion-only logic here
/// allows the test
/// `decode_f64_slab_from_le_bytes_pins_chunks_exact_le_decode_algorithm` to run
/// on every host and pin the BE algorithm against byte-layout regressions.
///
/// The LE branch of `read_f64_slab` deliberately does NOT call this helper
/// because it uses zero-copy `read_exact` into `spare_capacity_mut` directly,
/// avoiding an intermediate byte buffer entirely.
///
/// On BE hosts `read_f64_slab` delegates to this helper after `read_exact` so
/// the algorithm is dogfooded on real BE hardware and not duplicated.
///
/// `#[cfg(any(test, target_endian = "big"))]` keeps this function out of LE
/// release builds (where it has no call site) without hiding it from tests on
/// any host.
#[cfg(any(test, target_endian = "big"))]
pub fn decode_f64_slab_from_le_bytes(bytes: &[u8]) -> impl Iterator<Item = f64> + '_ {
    debug_assert_eq!(
        bytes.len() % 8,
        0,
        "decode_f64_slab_from_le_bytes requires 8-byte-aligned input length; \
         got {} bytes (trailing bytes are silently ignored by chunks_exact)",
        bytes.len()
    );
    bytes.chunks_exact(8).map(|chunk| {
        f64::from_le_bytes(
            chunk
                .try_into()
                .expect("chunks_exact(8) yields exactly-8-byte slices"),
        )
    })
}

impl PersistentlyCacheable for ElasticResult {
    const FORMAT_VERSION: u32 = ELASTIC_RESULT_FORMAT_VERSION;

    fn serialize_to_writer(&self, w: &mut impl Write) -> io::Result<()> {
        // Level 0 selects zstd's default compression level (3 in zstd 0.13),
        // which is byte-deterministic for identical input. Pinned explicitly
        // — `zstd 0.13` does not currently expose a non-deterministic mode at
        // this level, but byte-determinism is a hard requirement of the
        // persistent-cache PRD. The pin is verified by
        // `elastic_result_serialization_is_byte_deterministic` and
        // `elastic_result_reserialize_after_deserialize_is_byte_identical`;
        // bump the level if a future zstd release breaks default-level
        // determinism.
        // Single-threaded only — Encoder::multithread() breaks byte-determinism.
        let mut encoder = zstd::Encoder::new(w, 0)?;
        let header = ElasticResultHeader {
            max_von_mises_bits: self.max_von_mises.to_bits(),
            converged: self.converged,
            iterations: self.iterations,
            solve_time_ms: self.solve_time_ms,
            displacement_len: self.displacement.len() as u64,
            stress_len: self.stress.len() as u64,
            // v3 slab lengths.
            divergence_len: self.divergence.len() as u64,
            gradient_len: self.gradient.len() as u64,
            curl_len: self.curl.len() as u64,
            // Grid spec stored as raw u64 bit-patterns (NaN-safe, same idiom as
            // max_von_mises_bits).
            grid_bounds_min_x_bits: self.grid_bounds_min[0].to_bits(),
            grid_bounds_min_y_bits: self.grid_bounds_min[1].to_bits(),
            grid_bounds_min_z_bits: self.grid_bounds_min[2].to_bits(),
            grid_bounds_max_x_bits: self.grid_bounds_max[0].to_bits(),
            grid_bounds_max_y_bits: self.grid_bounds_max[1].to_bits(),
            grid_bounds_max_z_bits: self.grid_bounds_max[2].to_bits(),
            grid_count_x: self.grid_counts[0],
            grid_count_y: self.grid_counts[1],
            grid_count_z: self.grid_counts[2],
        };
        bincode::serialize_into(&mut encoder, &header).map_err(io::Error::other)?;
        // Bulk slab writes — see `write_f64_slab` for the full rationale on
        // LE zero-copy, BE byte-swap, OOM-safe sizing, empty-slab safety, and
        // the byte-order pin tests.
        write_f64_slab(&mut encoder, &self.displacement)?;
        write_f64_slab(&mut encoder, &self.stress)?;
        // v3 new slabs (task #3428 step-4): divergence (stride-1), gradient
        // (stride-9), curl (stride-3) written after stress and before the
        // shell_channels tail so the probe-byte tail detection is unchanged.
        write_f64_slab(&mut encoder, &self.divergence)?;
        write_f64_slab(&mut encoder, &self.gradient)?;
        write_f64_slab(&mut encoder, &self.curl)?;
        // v2 tail (PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` β):
        // always-present `ShellChannelsHeader` (1 byte `present` + three u64
        // lens = 25 bytes) followed by top/bottom/frame slabs when present.
        // v1 readers stop after the stress slab and never see this; v2/v3
        // readers detect a v1 stream by hitting EOF on the probe byte.
        let shell_header = ShellChannelsHeader::from(&self.shell_channels);
        bincode::serialize_into(&mut encoder, &shell_header).map_err(io::Error::other)?;
        if let Some(channels) = &self.shell_channels {
            write_f64_slab(&mut encoder, &channels.top)?;
            write_f64_slab(&mut encoder, &channels.bottom)?;
            write_f64_slab(&mut encoder, &channels.frame)?;
        }
        // task #4942 tail: written ONLY when aposteriori.is_some() -- absent
        // writes NO bytes at all, so a reader's probe byte hits true EOF,
        // byte-identical to every pre-#4942 v3 entry.
        if let Some(est) = &self.aposteriori {
            let (convergence_discriminant, budget_reason_discriminant, final_indicator_bits) =
                match &est.convergence_status {
                    ConvergenceStatus::Converged { final_indicator } => {
                        (0u8, 0u8, final_indicator.to_bits())
                    }
                    ConvergenceStatus::NotConverged { reason } => {
                        (1u8, budget_reason_to_u8(reason), 0u64)
                    }
                };
            let (error_indicator_present, error_indicator_len) = match &est.error_indicator {
                Some(v) => (true, v.len() as u64),
                None => (false, 0u64),
            };
            let (global_present, global_bits) = match est.global_relative_energy_error {
                Some(g) => (true, g.to_bits()),
                None => (false, 0u64),
            };
            let aposteriori_header = AposterioriHeader {
                convergence_discriminant,
                budget_reason_discriminant,
                final_indicator_bits,
                error_indicator_present,
                error_indicator_len,
                global_present,
                global_bits,
            };
            bincode::serialize_into(&mut encoder, &aposteriori_header).map_err(io::Error::other)?;
            if let Some(v) = &est.error_indicator {
                write_f64_slab(&mut encoder, v)?;
            }
        }
        encoder.finish()?;
        Ok(())
    }

    fn deserialize_from_reader(r: &mut impl Read) -> io::Result<Self> {
        // Error-propagation discipline (pinned by
        // `elastic_result_deserialize_from_truncated_reader_returns_io_error`):
        //   * `zstd::Decoder::new(r)?` — `zstd::Error: Into<io::Error>`, so `?`
        //     surfaces frame-header faults as `io::Error` directly.
        //   * `.map_err(io::Error::other)` — `bincode::Error` does NOT
        //     implement `Into<io::Error>`, so it must be mapped explicitly.
        //   * `read_exact` (on both the LE direct-cast path and the BE byte-buffer
        //     path) returns `Err(io::ErrorKind::UnexpectedEof)` on a short slab
        //     read — pinned by `elastic_result_deserialize_accepts_lengths_at_the_limit`.
        //   * On BE: `chunks_exact(8)` only ever sees exactly-8-byte sub-slices,
        //     eliminating any partial-read-mid-element fault path.
        let mut decoder = zstd::Decoder::new(r)?;
        let header: ElasticResultHeader =
            bincode::deserialize_from(&mut decoder).map_err(io::Error::other)?;
        // Bound length-prefix fields BEFORE allocating to defend against
        // corrupted/tampered cache entries claiming `u64::MAX` (or values
        // that silently truncate via `as usize` on a 32-bit target). See
        // `MAX_F64_ELEMENTS` for the rationale on the limit value.
        let displacement_cap = check_f64_vec_len("displacement", header.displacement_len)?;
        let stress_cap = check_f64_vec_len("stress", header.stress_len)?;
        let divergence_cap = check_f64_vec_len("divergence", header.divergence_len)?;
        let gradient_cap = check_f64_vec_len("gradient", header.gradient_len)?;
        let curl_cap = check_f64_vec_len("curl", header.curl_len)?;
        // Bulk slab reads — see `read_f64_slab` for the full rationale on LE
        // set_len safety, BE byte-swap, OOM-safe sizing, and the pin tests.
        // `check_f64_vec_len` above already validated all caps against
        // `MAX_F64_ELEMENTS`, so `read_f64_slab` receives pre-validated lengths.
        let displacement = read_f64_slab(&mut decoder, displacement_cap)?;
        let stress = read_f64_slab(&mut decoder, stress_cap)?;
        // v3 new slabs (task #3428 step-4).
        let divergence = read_f64_slab(&mut decoder, divergence_cap)?;
        let gradient = read_f64_slab(&mut decoder, gradient_cap)?;
        let curl = read_f64_slab(&mut decoder, curl_cap)?;

        // v2/v3 tail dispatch: probe one byte. EOF → v1 stream (shell_channels =
        // None). Non-EOF → decode the v2/v3 `ShellChannelsHeader` (the probe byte
        // is the `present` bool; bincode 1.3 fixint encodes bool as exactly
        // 0x00 / 0x01), then conditionally read top/bottom/frame slabs.
        let shell_channels = read_shell_channels_tail(&mut decoder)?;

        // task #4942 tail: probe-byte EOF -> None (pre-#4942 v3 entry, or a
        // result that was written with aposteriori: None).
        let aposteriori = read_aposteriori_tail(&mut decoder)?;

        Ok(ElasticResult {
            displacement,
            stress,
            max_von_mises: f64::from_bits(header.max_von_mises_bits),
            converged: header.converged,
            iterations: header.iterations,
            solve_time_ms: header.solve_time_ms,
            shell_channels,
            // v3 new fields.
            grid_bounds_min: [
                f64::from_bits(header.grid_bounds_min_x_bits),
                f64::from_bits(header.grid_bounds_min_y_bits),
                f64::from_bits(header.grid_bounds_min_z_bits),
            ],
            grid_bounds_max: [
                f64::from_bits(header.grid_bounds_max_x_bits),
                f64::from_bits(header.grid_bounds_max_y_bits),
                f64::from_bits(header.grid_bounds_max_z_bits),
            ],
            grid_counts: [header.grid_count_x, header.grid_count_y, header.grid_count_z],
            divergence,
            gradient,
            curl,
            aposteriori,
        })
    }

    fn uncompressed_byte_size(&self) -> u64 {
        // After zstd decompression, the body is:
        //   1. bincode 1.3 fixint-LE encoded ElasticResultHeader (133 bytes v3;
        //      pinned by `elastic_result_header_bincode_encoding_matches_pinned_hex_literal`).
        //   2. displacement slab: displacement.len() * 8 bytes (little-endian f64).
        //   3. stress slab: stress.len() * 8 bytes (little-endian f64).
        //   4. (v3) divergence slab: divergence.len() * 8 bytes.
        //   5. (v3) gradient slab: gradient.len() * 8 bytes.
        //   6. (v3) curl slab: curl.len() * 8 bytes.
        //   7. (v2/v3) bincode 1.3 fixint-LE encoded ShellChannelsHeader (25 bytes).
        //   8. (v2/v3, only when shell_channels.is_some()) top/bottom/frame slabs.
        // This method returns that total uncompressed length.
        //
        // `bincode::serialized_size` is used rather than a hardcoded magic
        // constant so that future header field additions automatically update
        // the uncompressed size without a manual edit. bincode 1.3 fixint-LE
        // encoding of a struct with no variable-length fields cannot fail in
        // practice — the `.expect(...)` is unreachable for the current struct
        // shapes (only fixed-size fields).
        let header = ElasticResultHeader {
            max_von_mises_bits: self.max_von_mises.to_bits(),
            converged: self.converged,
            iterations: self.iterations,
            solve_time_ms: self.solve_time_ms,
            displacement_len: self.displacement.len() as u64,
            stress_len: self.stress.len() as u64,
            divergence_len: self.divergence.len() as u64,
            gradient_len: self.gradient.len() as u64,
            curl_len: self.curl.len() as u64,
            grid_bounds_min_x_bits: self.grid_bounds_min[0].to_bits(),
            grid_bounds_min_y_bits: self.grid_bounds_min[1].to_bits(),
            grid_bounds_min_z_bits: self.grid_bounds_min[2].to_bits(),
            grid_bounds_max_x_bits: self.grid_bounds_max[0].to_bits(),
            grid_bounds_max_y_bits: self.grid_bounds_max[1].to_bits(),
            grid_bounds_max_z_bits: self.grid_bounds_max[2].to_bits(),
            grid_count_x: self.grid_counts[0],
            grid_count_y: self.grid_counts[1],
            grid_count_z: self.grid_counts[2],
        };
        let header_bytes = bincode::serialized_size(&header).expect(
            "ElasticResultHeader has only fixed-size fields (u64, bool, u32, u64, ...); \
             bincode::serialized_size cannot fail. If a future field with variable-length \
             encoding (String/Vec/Option) is added, this expect will fire — at which point \
             byte_size accounting must be revisited.",
        );
        let slab_bytes = 8
            * (self.displacement.len() as u64
                + self.stress.len() as u64
                + self.divergence.len() as u64
                + self.gradient.len() as u64
                + self.curl.len() as u64);
        let shell_header = ShellChannelsHeader::from(&self.shell_channels);
        let shell_header_bytes = bincode::serialized_size(&shell_header).expect(
            "ShellChannelsHeader has only fixed-size fields (bool, u64, u64, u64); \
             bincode::serialized_size cannot fail.",
        );
        let shell_slab_bytes = match &self.shell_channels {
            None => 0,
            Some(c) => 8 * (c.top.len() as u64 + c.bottom.len() as u64 + c.frame.len() as u64),
        };
        // task #4942: aposteriori tail contributes 0 bytes when None (no tail
        // is ever written for that case).
        let aposteriori_bytes = match &self.aposteriori {
            None => 0,
            Some(est) => {
                let aposteriori_header = AposterioriHeader {
                    convergence_discriminant: 0,
                    budget_reason_discriminant: 0,
                    final_indicator_bits: 0,
                    error_indicator_present: est.error_indicator.is_some(),
                    error_indicator_len: est.error_indicator.as_ref().map_or(0, |v| v.len() as u64),
                    global_present: est.global_relative_energy_error.is_some(),
                    global_bits: 0,
                };
                let aposteriori_header_bytes = bincode::serialized_size(&aposteriori_header).expect(
                    "AposterioriHeader has only fixed-size fields (u8, u8, u64, bool, u64, bool, u64); \
                     bincode::serialized_size cannot fail.",
                );
                let aposteriori_slab_bytes =
                    est.error_indicator.as_ref().map_or(0, |v| 8 * v.len() as u64);
                aposteriori_header_bytes + aposteriori_slab_bytes
            }
        };
        header_bytes + slab_bytes + shell_header_bytes + shell_slab_bytes + aposteriori_bytes
    }

    fn solve_time_ms(&self) -> u64 {
        self.solve_time_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: trait/impl link is enforced at module scope via a `const _: fn() = ...`
    // assertion (see top of file). The previous `#[test]` wrapper around the
    // same compile-time check, plus a separate `*_constructor_pins_six_field_shape`
    // test that read back fields it had just constructed, were dropped — both
    // are subsumed by the round-trip pin (`*_round_trips_all_six_fields`)
    // and the static assertion.

    #[test]
    fn elastic_result_format_version_pinned() {
        // Read from the trait associated const directly — no instance needed,
        // demonstrating the cache-layer use case where `(TypeId, FORMAT_VERSION)`
        // can be looked up before any value materialises. Pins the current
        // FORMAT_VERSION value. An intentional format bump must touch this
        // assertion — that is the point: it forces a deliberate acknowledgement
        // that cached bytes from the previous version are now incompatible.
        // Bumped 1 → 2 in shell-extract-engine-bridge PRD task β (added optional
        // shell_channels tail; v2 reader still accepts v1 bytes). Bumped 2 → 3 in
        // task #3428 step-4 (added grid spec + divergence/gradient/curl slabs;
        // v2 streams are incompatible — no backward-compat reader for v2).
        assert_eq!(<ElasticResult as PersistentlyCacheable>::FORMAT_VERSION, 3);
    }

    #[test]
    fn elastic_result_solve_time_ms_returns_constructor_value() {
        let nine_thousand_nine_hundred_ninety_nine = ElasticResult {
            displacement: vec![],
            stress: vec![],
            max_von_mises: 0.0,
            converged: false,
            iterations: 0,
            solve_time_ms: 9999,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [0.0, 0.0, 0.0],
            grid_counts: [0, 0, 0],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        assert_eq!(nine_thousand_nine_hundred_ninety_nine.solve_time_ms(), 9999);

        // Pin that the accessor isn't returning a hard-coded constant.
        let zero = ElasticResult {
            displacement: vec![],
            stress: vec![],
            max_von_mises: 0.0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [0.0, 0.0, 0.0],
            grid_counts: [0, 0, 0],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        assert_eq!(zero.solve_time_ms(), 0);
    }

    /// Build an ElasticResult populated with the same non-trivial values used
    /// by the determinism + round-trip tests, so each test gets a fresh copy.
    fn make_sample_result() -> ElasticResult {
        ElasticResult {
            displacement: vec![1.0, -2.5, std::f64::consts::PI, 0.0, 1e-9],
            stress: vec![100e6, -50e6, 0.0, 250e6],
            max_von_mises: 250e6,
            converged: true,
            iterations: 423,
            solve_time_ms: 1234,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [0.0, 0.0, 0.0],
            grid_counts: [0, 0, 0],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        }
    }

    #[test]
    fn elastic_result_serialization_is_byte_deterministic() {
        let a = make_sample_result();
        let b = make_sample_result();
        let mut buf_a: Vec<u8> = Vec::new();
        let mut buf_b: Vec<u8> = Vec::new();
        a.serialize_to_writer(&mut buf_a).unwrap();
        b.serialize_to_writer(&mut buf_b).unwrap();
        assert_eq!(buf_a, buf_b);
    }

    #[test]
    fn elastic_result_reserialize_after_deserialize_is_byte_identical() {
        let original = make_sample_result();
        let mut bytes_a: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut bytes_a).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &bytes_a[..]).unwrap();
        let mut bytes_b: Vec<u8> = Vec::new();
        decoded.serialize_to_writer(&mut bytes_b).unwrap();
        assert_eq!(bytes_a, bytes_b);
    }

    #[test]
    fn elastic_result_round_trip_preserves_nan_and_infinity_bit_patterns() {
        let original = ElasticResult {
            displacement: vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 0.0],
            stress: vec![f64::NAN],
            max_von_mises: f64::NAN,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [0.0, 0.0, 0.0],
            grid_counts: [0, 0, 0],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        // NaN != NaN under PartialEq, so compare bit-patterns explicitly.
        assert_eq!(decoded.displacement.len(), original.displacement.len());
        for (d, o) in decoded
            .displacement
            .iter()
            .zip(original.displacement.iter())
        {
            assert_eq!(d.to_bits(), o.to_bits(), "displacement bit pattern drift");
        }
        assert_eq!(decoded.stress.len(), original.stress.len());
        for (d, o) in decoded.stress.iter().zip(original.stress.iter()) {
            assert_eq!(d.to_bits(), o.to_bits(), "stress bit pattern drift");
        }
        assert_eq!(
            decoded.max_von_mises.to_bits(),
            original.max_von_mises.to_bits(),
            "max_von_mises bit pattern drift"
        );
    }

    #[test]
    fn elastic_result_round_trips_with_empty_field_arrays() {
        // Pin that displacement_len = 0 / stress_len = 0 are handled cleanly
        // on both sides — the slab loops must not assume "at least one
        // element" via `.first().unwrap()` or similar.
        let original = ElasticResult {
            displacement: Vec::new(),
            stress: Vec::new(),
            max_von_mises: 0.0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [0.0, 0.0, 0.0],
            grid_counts: [0, 0, 0],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        assert_eq!(decoded, original);
    }

    /// Acceptable error kinds from a malformed/truncated input. The exact
    /// kind depends on which decode stage faults — `UnexpectedEof` from a
    /// short `read_exact`, `InvalidData` from zstd's frame parser or the
    /// bound check, `Other` for wrapped bincode errors. We accept any of
    /// these so the test stays stable across zstd / bincode patch bumps;
    /// what matters is "not a panic" and "Err, not Ok".
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
    fn elastic_result_deserialize_from_truncated_reader_returns_io_error() {
        // Truncating a valid encoded buffer at different offsets exercises
        // distinct decode stages:
        //   * 0 bytes        → zstd::Decoder::new fails at frame magic
        //   * 1, 4 bytes     → partial frame magic / header
        //   * len/4, len/2   → mid-bincode-header or mid-slab depending
        //                      on the encoded layout
        //   * len-1          → one byte short of the final block
        // Every offset must surface `Err(io::Error)` panic-free; pin via
        // `expect_err` rather than `unwrap()` so a regression that switches
        // any path to a panic surfaces as a test panic.
        let original = make_sample_result();
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let len = buf.len();
        let truncation_points: [usize; 6] = [0, 1, 4, len / 4, len / 2, len - 1];
        for &n in &truncation_points {
            let truncated = &buf[..n];
            let label = format!("truncation @ {n}/{len} bytes");
            let err = ElasticResult::deserialize_from_reader(&mut &truncated[..])
                .expect_err(&format!("{label}: must return Err"));
            assert_decode_error(&label, &err);
        }
    }

    #[test]
    fn elastic_result_deserialize_from_random_bytes_returns_io_error() {
        // Random bytes (not a valid zstd frame, not a valid bincode payload)
        // must not be silently accepted. The most likely failure mode is
        // zstd::Decoder::new rejecting the missing/wrong frame magic, but a
        // garbage stream that happens to start with a valid magic must still
        // fail downstream — the test uses bytes that begin with the zstd
        // magic (0x28 0xB5 0x2F 0xFD) followed by junk so we exercise the
        // "decoder accepts magic, then bincode/slab decode chokes" path too.
        let zstd_magic_then_garbage = [
            0x28, 0xB5, 0x2F, 0xFD, // valid zstd frame magic
            0xDE, 0xAD, 0xBE, 0xEF, // junk
            0xCA, 0xFE, 0xBA, 0xBE, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        ];
        let err = ElasticResult::deserialize_from_reader(&mut &zstd_magic_then_garbage[..])
            .expect_err("zstd-magic + garbage must not silently decode");
        assert_decode_error("zstd-magic + garbage", &err);

        // Pure random bytes (no valid magic) — most likely faults at
        // zstd::Decoder::new with InvalidData / Other.
        let pure_garbage = [0xDEu8, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
        let err = ElasticResult::deserialize_from_reader(&mut &pure_garbage[..])
            .expect_err("pure-garbage bytes must not decode");
        assert_decode_error("pure garbage", &err);
    }

    /// Helper used by the oversize-length and (later) garbage-bytes tests:
    /// emit a zstd frame containing a hand-built header so we can simulate a
    /// tampered cache entry without going through the public `serialize_to_writer`
    /// path. Returns the encoded bytes.
    fn encode_header(header: &ElasticResultHeader) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        let mut encoder = zstd::Encoder::new(&mut buf, 0).unwrap();
        bincode::serialize_into(&mut encoder, header).unwrap();
        encoder.finish().unwrap();
        buf
    }

    #[test]
    fn elastic_result_deserialize_rejects_oversize_displacement_len() {
        // A tampered/corrupted cache entry advertises a displacement_len just
        // past the limit. The decoder must refuse with `InvalidData` BEFORE
        // attempting `Vec::with_capacity(huge)` (which would either OOM-panic
        // or silently truncate on 32-bit hosts).
        let header = ElasticResultHeader {
            max_von_mises_bits: 0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            displacement_len: MAX_F64_ELEMENTS + 1,
            stress_len: 0,
            // v3 additions: zero-valued, not relevant to this specific test.
            divergence_len: 0,
            gradient_len: 0,
            curl_len: 0,
            grid_bounds_min_x_bits: 0,
            grid_bounds_min_y_bits: 0,
            grid_bounds_min_z_bits: 0,
            grid_bounds_max_x_bits: 0,
            grid_bounds_max_y_bits: 0,
            grid_bounds_max_z_bits: 0,
            grid_count_x: 0,
            grid_count_y: 0,
            grid_count_z: 0,
        };
        let buf = encode_header(&header);
        let err = ElasticResult::deserialize_from_reader(&mut &buf[..])
            .expect_err("oversize displacement_len must be rejected");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {err:?}"
        );
    }

    #[test]
    fn elastic_result_deserialize_rejects_oversize_stress_len() {
        // Symmetric pin for the stress field — both length-prefix paths must
        // be guarded.
        let header = ElasticResultHeader {
            max_von_mises_bits: 0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            displacement_len: 0,
            stress_len: u64::MAX,
            // v3 additions: zero-valued, not relevant to this specific test.
            divergence_len: 0,
            gradient_len: 0,
            curl_len: 0,
            grid_bounds_min_x_bits: 0,
            grid_bounds_min_y_bits: 0,
            grid_bounds_min_z_bits: 0,
            grid_bounds_max_x_bits: 0,
            grid_bounds_max_y_bits: 0,
            grid_bounds_max_z_bits: 0,
            grid_count_x: 0,
            grid_count_y: 0,
            grid_count_z: 0,
        };
        let buf = encode_header(&header);
        let err = ElasticResult::deserialize_from_reader(&mut &buf[..])
            .expect_err("oversize stress_len must be rejected");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {err:?}"
        );
    }

    #[test]
    fn elastic_result_deserialize_accepts_lengths_at_the_limit() {
        // The decoder must traverse the bound check successfully for
        // legal-but-non-zero header lengths and only fail later on the short
        // slab read (UnexpectedEof from `read_exact`), NOT on the bound check
        // (which would surface `InvalidData`). The off-by-one boundary of the
        // bound check is now pinned directly via
        // `check_f64_vec_len_rejects_value_above_workload_limit` (step-15) and
        // `elastic_result_deserialize_rejects_oversize_displacement_len`
        // (which uses `MAX_F64_ELEMENTS + 1`); this integration test only
        // needs to exercise the "header accepted, slab EOF" code path, so a
        // small length covers it without any incidental large allocation.
        let header = ElasticResultHeader {
            max_von_mises_bits: 0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            displacement_len: 4,
            stress_len: 0,
            // v3 additions: zero-valued, not relevant to this specific test.
            divergence_len: 0,
            gradient_len: 0,
            curl_len: 0,
            grid_bounds_min_x_bits: 0,
            grid_bounds_min_y_bits: 0,
            grid_bounds_min_z_bits: 0,
            grid_bounds_max_x_bits: 0,
            grid_bounds_max_y_bits: 0,
            grid_bounds_max_z_bits: 0,
            grid_count_x: 0,
            grid_count_y: 0,
            grid_count_z: 0,
        };
        let buf = encode_header(&header);
        let err = ElasticResult::deserialize_from_reader(&mut &buf[..])
            .expect_err("zero-payload slab must EOF, not InvalidData");
        assert_eq!(
            err.kind(),
            io::ErrorKind::UnexpectedEof,
            "expected UnexpectedEof on slab read, got {err:?} \
             (regression: header bound check may be incorrectly rejecting \
             a header-accepted, slab-truncated stream)"
        );
    }

    #[test]
    fn elastic_result_round_trips_all_six_fields() {
        let original = ElasticResult {
            displacement: vec![1.0, -2.5, std::f64::consts::PI, 0.0, 1e-9],
            stress: vec![100e6, -50e6, 0.0, 250e6],
            max_von_mises: 250e6,
            converged: true,
            iterations: 423,
            solve_time_ms: 1234,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [0.0, 0.0, 0.0],
            grid_counts: [0, 0, 0],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn elastic_result_round_trip_with_shell_channels_some_is_bit_exact() {
        // PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` task β
        // contract (a): every f64 in shell_channels.top / .bottom / .frame
        // survives a serialize → deserialize cycle with its raw bit pattern
        // intact, including NaN payloads / Inf / signed-zero. Bit-scrambled
        // payload (same idiom as the existing 1M-element test) ensures every
        // byte of every f64 is non-trivial, so a native-byte or wrong-len
        // regression in any of the three new slabs surfaces here rather than
        // silently aliasing.
        let mut top: Vec<f64> = (0..18u64)
            .map(|i| f64::from_bits(i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xABAD_1DEA_0BAD_F00D))
            .collect();
        // Sentinel non-canonical NaN payload pins that the slab path does not
        // normalise NaN through f64 arithmetic.
        top[0] = f64::from_bits(0x7FF8_DEAD_BEEF_CAFE);
        let bottom: Vec<f64> = (0..18u64)
            .map(|i| f64::from_bits(i.wrapping_mul(0x6C62_272E_07BB_0142) ^ 0xC0DE_FACE_DEAD_C0DE))
            .collect();
        // 9 f64 per element × 2 elements = 18 entries: row-major 3×3 frames.
        let frame: Vec<f64> = (0..18u64)
            .map(|i| f64::from_bits(i.wrapping_mul(0xD737_E5B5_2727_2727) ^ 0x1234_5678_9ABC_DEF0))
            .collect();
        let original = ElasticResult {
            displacement: vec![1.0, -2.5, std::f64::consts::PI, 0.0, 1e-9],
            stress: vec![100e6, -50e6, 0.0, 250e6, 75e6, -125e6],
            max_von_mises: 250e6,
            converged: true,
            iterations: 17,
            solve_time_ms: 4321,
            shell_channels: Some(ShellChannels {
                top: top.clone(),
                bottom: bottom.clone(),
                frame: frame.clone(),
            }),
            // v3 additions: zero-valued for this test (shell-channels focus).
            grid_bounds_min: [0.0; 3],
            grid_bounds_max: [0.0; 3],
            grid_counts: [0; 3],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        let decoded_channels = decoded
            .shell_channels
            .as_ref()
            .expect("Some(_) round-trip must yield Some(_) on decode");
        assert_eq!(decoded_channels.top.len(), top.len());
        assert_eq!(decoded_channels.bottom.len(), bottom.len());
        assert_eq!(decoded_channels.frame.len(), frame.len());
        for (d, o) in decoded_channels.top.iter().zip(top.iter()) {
            assert_eq!(
                d.to_bits(),
                o.to_bits(),
                "shell_channels.top bit pattern drift"
            );
        }
        for (d, o) in decoded_channels.bottom.iter().zip(bottom.iter()) {
            assert_eq!(
                d.to_bits(),
                o.to_bits(),
                "shell_channels.bottom bit pattern drift"
            );
        }
        for (d, o) in decoded_channels.frame.iter().zip(frame.iter()) {
            assert_eq!(
                d.to_bits(),
                o.to_bits(),
                "shell_channels.frame bit pattern drift"
            );
        }
    }

    #[test]
    fn elastic_result_round_trip_with_shell_channels_none_appends_25_byte_zero_trailer() {
        // PRD `docs/prds/v0_4/shell-extract-engine-bridge.md` task β
        // contract (b): a tet-only result with `shell_channels: None` round-trips
        // identically AND its decompressed wire layout retains the pre-bump
        // displacement+stress prefix bytewise — the only addition is a fixed
        // 25-byte ShellChannelsHeader trailer with present=0 and zero lens
        // (1-byte bool false + 3×8-byte u64 zero in bincode 1.3 fixint-LE
        // = 25 zero bytes). Pins that the shell-channels-trailer addition
        // does not perturb the existing tet-only on-disk byte layout other
        // than appending the trailer.
        let original = make_sample_result();
        assert!(
            original.shell_channels.is_none(),
            "make_sample_result must yield shell_channels: None"
        );
        let mut compressed: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut compressed).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &compressed[..]).unwrap();
        // Identity round-trip.
        assert_eq!(decoded, original);
        assert!(decoded.shell_channels.is_none());

        // Decompress and pin the trailing 25 bytes are exactly the
        // present=false, zero-len trailer.
        let mut zstd_dec = zstd::Decoder::new(&compressed[..]).unwrap();
        let mut decompressed: Vec<u8> = Vec::new();
        io::Read::read_to_end(&mut zstd_dec, &mut decompressed).unwrap();
        assert!(
            decompressed.len() >= 25,
            "decompressed stream must include the 25-byte trailer"
        );
        let tail = &decompressed[decompressed.len() - 25..];
        assert_eq!(
            tail,
            &[0u8; 25][..],
            "shell_channels=None v2 trailer must be 25 zero bytes \
             (present=0 + top_len=bottom_len=frame_len=0)"
        );
    }

    #[test]
    fn elastic_result_deserialize_without_shell_channels_tail_yields_shell_channels_none() {
        // Pins that a v3 stream without a ShellChannelsHeader trailer decodes
        // cleanly with shell_channels: None.  In v3, the header includes slab
        // lengths for divergence/gradient/curl (all zero here), so after reading
        // 0 bytes for those slabs the reader calls read_shell_channels_tail which
        // hits EOF on the 1-byte probe and returns None.
        //
        // Context: pre-v3 (v1/v2) streams are now INCOMPATIBLE with the v3 reader
        // (the larger v3 bincode header would fail to decode a 37-byte v1/v2 header
        // stream with UnexpectedEof — treated as corruption → cache miss per the
        // existing corruption-recovery policy).  The v1 backward-compat contract
        // tested in this way is superseded by the v3 bump.
        let displacement = vec![1.0_f64, -2.5_f64, std::f64::consts::PI];
        let stress = vec![100e6_f64, -50e6_f64];
        let header = ElasticResultHeader {
            max_von_mises_bits: 100e6_f64.to_bits(),
            converged: true,
            iterations: 7,
            solve_time_ms: 999,
            displacement_len: displacement.len() as u64,
            stress_len: stress.len() as u64,
            // v3 additions: zero slab lengths → no divergence/gradient/curl bytes
            // in the stream body; probe-byte EOF on read_shell_channels_tail → None.
            divergence_len: 0,
            gradient_len: 0,
            curl_len: 0,
            grid_bounds_min_x_bits: 0,
            grid_bounds_min_y_bits: 0,
            grid_bounds_min_z_bits: 0,
            grid_bounds_max_x_bits: 0,
            grid_bounds_max_y_bits: 0,
            grid_bounds_max_z_bits: 0,
            grid_count_x: 0,
            grid_count_y: 0,
            grid_count_z: 0,
        };
        let mut compressed: Vec<u8> = Vec::new();
        {
            let mut encoder = zstd::Encoder::new(&mut compressed, 0).unwrap();
            bincode::serialize_into(&mut encoder, &header).unwrap();
            for v in &displacement {
                io::Write::write_all(&mut encoder, &v.to_le_bytes()).unwrap();
            }
            for v in &stress {
                io::Write::write_all(&mut encoder, &v.to_le_bytes()).unwrap();
            }
            // No shell-channels trailer — probe byte hits EOF → None.
            encoder.finish().unwrap();
        }
        let decoded = ElasticResult::deserialize_from_reader(&mut &compressed[..]).unwrap();
        assert_eq!(decoded.displacement, displacement);
        assert_eq!(decoded.stress, stress);
        assert_eq!(decoded.max_von_mises.to_bits(), 100e6_f64.to_bits());
        assert!(decoded.converged);
        assert_eq!(decoded.iterations, 7);
        assert_eq!(decoded.solve_time_ms, 999);
        assert!(
            decoded.shell_channels.is_none(),
            "v1-format bytes must deserialize to shell_channels: None"
        );
    }

    #[test]
    fn elastic_result_round_trips_one_million_element_vectors() {
        // 1<<20 ≈ 1 million f64 elements — well below MAX_F64_ELEMENTS (1<<24)
        // so try_reserve_exact defence does not fire, but large enough to exercise
        // the bulk-transfer code path at workload-realistic scale (required by the
        // task description: "add at least one bench or assertion covering large-N
        // (e.g. 1M elements) to demonstrate the path is exercised").
        //
        // Bit-scrambled pattern (golden-ratio multiplier + XOR) rather than a
        // monotonic ramp: a naive byte-order bug that happens to be invariant on
        // small or structured inputs (e.g. all-zero / all-integer-valued floats)
        // would still be caught here because the scrambled pattern produces values
        // with significant entropy in every byte of every f64.
        let n = 1usize << 20;
        let displacement: Vec<f64> = (0..n)
            .map(|i| {
                f64::from_bits(
                    (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xDEAD_BEEF_CAFE_BABE,
                )
            })
            .collect();
        // Smaller stress vector derived from a different scramble constant so
        // both slab paths are exercised without doubling the allocation.
        let stress: Vec<f64> = (0..1024u64)
            .map(|i| f64::from_bits(i.wrapping_mul(0x6C62_272E_07BB_0142) ^ 0xFEED_FACE_DEAD_BEEF))
            .collect();
        let original = ElasticResult {
            displacement,
            stress,
            max_von_mises: f64::from_bits(0xDEAD_BEEF_CAFE_BABE),
            converged: true,
            iterations: 1,
            solve_time_ms: 42,
            shell_channels: None,
            // v3 additions: zero-valued for this test (large-N focus).
            grid_bounds_min: [0.0; 3],
            grid_bounds_max: [0.0; 3],
            grid_counts: [0; 3],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        // Assert length first so a length mismatch surfaces a clear error
        // before any per-element bit-pattern check.
        assert_eq!(decoded.displacement.len(), 1 << 20);
        assert_eq!(decoded.stress.len(), original.stress.len());
        // NaN-safe comparison: to_bits() compares raw bit patterns so NaN
        // payloads, signaling-NaN bits, and signed zeros survive the assertion.
        // Reuses the pattern from
        // elastic_result_round_trip_preserves_nan_and_infinity_bit_patterns.
        for (d, o) in decoded
            .displacement
            .iter()
            .zip(original.displacement.iter())
        {
            assert_eq!(d.to_bits(), o.to_bits(), "displacement bit pattern drift");
        }
        for (d, o) in decoded.stress.iter().zip(original.stress.iter()) {
            assert_eq!(d.to_bits(), o.to_bits(), "stress bit pattern drift");
        }
    }

    #[test]
    fn elastic_result_serialized_slab_section_is_little_endian_bytewise() {
        // Cross-host portability pin: verifies that the slab section of the
        // on-disk format is byte-for-byte little-endian regardless of host
        // endianness. The existing `elastic_result_serialization_is_byte_deterministic`
        // only asserts same-host run-to-run equality — a future regression to
        // native-byte encoding on a hypothetical big-endian host (or accidental
        // misuse of bytemuck::cast_slice on a non-LE host) would still pass
        // that test but would break this one. Also catches accidental `to_ne_bytes()`
        // (which would pass on LE but emit BE bytes on a BE host).
        //
        // Reuses `ElasticResultHeader` (in scope inside `mod tests` via `super::*`)
        // and the `bincode::deserialize_from` reader-advancing idiom from the
        // oversize-len tests to consume past the header and expose the raw slab bytes.
        let original = ElasticResult {
            displacement: vec![1.0_f64, -2.5_f64, std::f64::consts::PI],
            stress: vec![100e6_f64, -50e6_f64],
            max_von_mises: 100e6,
            converged: true,
            iterations: 7,
            solve_time_ms: 999,
            shell_channels: None,
            // v3 additions: zero/empty — this test focuses on the LE slab encoding
            // of displacement+stress; the new v3 slabs are empty here.
            grid_bounds_min: [0.0; 3],
            grid_bounds_max: [0.0; 3],
            grid_counts: [0; 3],
            divergence: Vec::new(),
            gradient: Vec::new(),
            curl: Vec::new(),
            aposteriori: None,
        };
        let mut compressed: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut compressed).unwrap();

        // Decompress the zstd frame to recover the inner bincode+slab stream.
        let mut zstd_dec = zstd::Decoder::new(&compressed[..]).unwrap();
        let mut decompressed: Vec<u8> = Vec::new();
        io::Read::read_to_end(&mut zstd_dec, &mut decompressed).unwrap();

        // Consume the bincode-encoded header via a mutable slice reference.
        // `bincode::deserialize_from` advances the `&mut &[u8]` reader by
        // exactly as many bytes as the header occupies (133 bytes in v3),
        // leaving `slice` pointing at the first byte of the slab section.
        let mut slice: &[u8] = &decompressed;
        let _header: ElasticResultHeader =
            bincode::deserialize_from(&mut slice).expect("header must deserialize cleanly");

        // Build expected slab: displacement bytes then stress bytes, each
        // value as 8-byte little-endian (unconditionally, regardless of host
        // endianness — this is the cross-host portability contract).
        // v3 divergence/gradient/curl are all empty → 0 additional bytes.
        let mut expected: Vec<u8> = Vec::new();
        for v in &original.displacement {
            expected.extend_from_slice(&v.to_le_bytes());
        }
        for v in &original.stress {
            expected.extend_from_slice(&v.to_le_bytes());
        }

        // The slab section (displacement + stress + empty divergence/gradient/curl)
        // is followed by the fixed-size 25-byte ShellChannelsHeader trailer.
        // For shell_channels: None the trailer is all-zero bytes (1-byte bool false
        // + 3×8-byte u64 zero in bincode 1.3 fixint-LE). The little-endian slab
        // contract applies to the slab section only, so assert it on the prefix
        // and pin the trailer separately.
        let slab_end = expected.len();
        assert_eq!(
            slice.len(),
            slab_end + 25,
            "decompressed stream must be slabs (disp+stress, v3 empty channels) \
             + 25-byte shell-channels trailer"
        );
        assert_eq!(
            &slice[..slab_end],
            expected.as_slice(),
            "slab section must be unconditionally little-endian on disk; \
             any regression to native-byte encoding on a big-endian host \
             or accidental to_ne_bytes() usage will fail this assertion"
        );
        assert_eq!(
            &slice[slab_end..],
            &[0u8; 25][..],
            "v2 trailer for shell_channels=None must be 25 zero bytes \
             (covered more thoroughly by \
              elastic_result_round_trip_with_shell_channels_none_appends_25_byte_zero_trailer)"
        );
    }

    #[test]
    fn check_f64_vec_len_rejects_value_above_workload_limit() {
        // Portable boundary pin: exercises the bound check without any Vec
        // allocation, so it remains stable on memory-constrained CI runners.
        let just_above_limit = MAX_F64_ELEMENTS + 1;
        let err = check_f64_vec_len("test", just_above_limit)
            .expect_err("value above MAX_F64_ELEMENTS must be rejected");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {err:?}"
        );
    }

    /// Direct round-trip test for the `write_f64_slab` and `read_f64_slab`
    /// helpers, independent of the zstd/bincode wrapper. The slab contains
    /// values whose byte patterns expose any LE-vs-native-endian bug AND any
    /// uninitialised-byte leak: a bit-scrambled integer, NaN, ±∞, and ±0.
    #[test]
    fn write_f64_slab_then_read_f64_slab_round_trips_bit_patterns_directly() {
        let slab: Vec<f64> = vec![
            1.0_f64,
            -2.5,
            f64::from_bits(0xDEAD_BEEF_CAFE_BABE),
            f64::NAN,
            f64::INFINITY,
            -0.0,
            0.0,
        ];
        let mut buf: Vec<u8> = Vec::new();
        write_f64_slab(&mut buf, &slab).unwrap();
        // Buffer length must equal slab.len() * 8 bytes.
        assert_eq!(buf.len(), slab.len() * 8);
        // First 8 bytes must equal `1.0_f64.to_le_bytes()` — pins LE on-disk
        // byte order independent of host endianness (mirrors
        // `elastic_result_serialized_slab_section_is_little_endian_bytewise`).
        assert_eq!(&buf[..8], &1.0_f64.to_le_bytes());
        // Read back and compare bit patterns (NaN-safe: to_bits() compares raw
        // 64-bit values, so signaling-NaN payloads, signed zeros, etc. are
        // preserved exactly — mirrors the pattern in
        // `elastic_result_round_trip_preserves_nan_and_infinity_bit_patterns`).
        let decoded = read_f64_slab(&mut &buf[..], slab.len()).unwrap();
        assert_eq!(decoded.len(), slab.len());
        for (d, o) in decoded.iter().zip(slab.iter()) {
            assert_eq!(d.to_bits(), o.to_bits(), "bit pattern drift");
        }
    }

    /// Pins that `read_f64_slab` fails loudly with `UnexpectedEof` on short
    /// input rather than reaching the unsafe `set_len` call. The post-condition
    /// this test verifies is that `set_len` is gated on `read_exact`'s Ok
    /// path — no partially-initialised `Vec` is ever exposed to the caller on
    /// a short read.
    #[test]
    fn read_f64_slab_returns_unexpected_eof_on_short_input() {
        // 7-byte buffer — one byte short of one f64 (which needs 8 bytes).
        // We request `len=4`, meaning 32 bytes are required, so the short-read
        // fault occurs at the very first element boundary.
        let short = [0u8; 7];
        let err = read_f64_slab(&mut &short[..], 4).expect_err("short input must return Err");
        assert_eq!(
            err.kind(),
            io::ErrorKind::UnexpectedEof,
            "expected UnexpectedEof, got {err:?}"
        );
    }

    /// Pins the empty-input edge case for the helpers independently of the
    /// `ElasticResult` wrapper: zero-length slab → zero bytes written →
    /// `read_f64_slab(_, 0)` returns `Vec::new()`.
    #[test]
    fn write_f64_slab_round_trips_empty_slice() {
        let empty: &[f64] = &[];
        let mut buf: Vec<u8> = Vec::new();
        write_f64_slab(&mut buf, empty).unwrap();
        assert_eq!(buf.len(), 0, "zero-element slab must produce zero bytes");
        let decoded = read_f64_slab(&mut &buf[..], 0).unwrap();
        assert!(
            decoded.is_empty(),
            "read of zero-length slab must return empty Vec"
        );
    }

    /// Pins the BE `chunks_exact(8) → f64::from_le_bytes` algorithm host-agnostically
    /// via a fixed byte-literal fixture. The BE branch of `read_f64_slab` is
    /// `#[cfg(target_endian = "big")]`-gated and unreachable on LE CI hosts — this test
    /// exercises the conversion-only logic on any host by calling the helper directly with
    /// known LE bytes and asserting the expected f64 bit patterns.
    ///
    /// Fixed literals catch a regression from `from_le_bytes` to `from_be_bytes` or
    /// `from_ne_bytes` more tightly than a `to_le_bytes` → `from_le_bytes` round-trip
    /// (which would be a tautology guaranteed by std on any host).
    #[test]
    fn decode_f64_slab_from_le_bytes_pins_chunks_exact_le_decode_algorithm() {
        // 1.0_f64:  bits = 0x3FF0_0000_0000_0000, LE bytes = [00 00 00 00 00 00 F0 3F]
        // -2.5_f64: bits = 0xC004_0000_0000_0000, LE bytes = [00 00 00 00 00 00 04 C0]
        let bytes: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // 1.0_f64
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xC0, // -2.5_f64
        ];
        let decoded: Vec<f64> = decode_f64_slab_from_le_bytes(bytes).collect();
        assert_eq!(decoded.len(), 2);
        assert_eq!(
            decoded[0].to_bits(),
            1.0_f64.to_bits(),
            "1.0 fixture: LE bytes [00..F0 3F] must decode to 1.0, not from_be/ne_bytes"
        );
        assert_eq!(
            decoded[1].to_bits(),
            (-2.5_f64).to_bits(),
            "-2.5 fixture: LE bytes [00..04 C0] must decode to -2.5, not from_be/ne_bytes"
        );
    }

    /// Pins the LE on-disk contract for `read_f64_slab` — the public entry
    /// point — using explicit LE byte-literal fixtures.
    ///
    /// This is the entry-point counterpart to
    /// `decode_f64_slab_from_le_bytes_pins_chunks_exact_le_decode_algorithm`,
    /// which exercises the BE conversion kernel in isolation. On LE CI hosts
    /// `read_f64_slab` takes the zero-copy `spare_capacity_mut` + `set_len`
    /// fast path and never calls the kernel; the kernel test therefore does
    /// NOT cover that path. On BE hosts, this test exercises the kernel path
    /// again, providing a cross-host pin. This test calls `read_f64_slab`
    /// directly with known LE bytes and asserts the decoded `to_bits()` values,
    /// complementing the existing `&buf[..8]` host-independent assertion in
    /// `elastic_result_serialized_slab_section_is_little_endian_bytewise`.
    ///
    /// Fixed literals (`[00..F0 3F]` → `1.0`, `[00..04 C0]` → `-2.5`) catch a
    /// `from_ne_bytes` / `from_be_bytes` regression more tightly than a
    /// `write_f64_slab` → `read_f64_slab` round-trip (which would be a
    /// tautology if both sides share the same bug).
    #[test]
    fn read_f64_slab_decodes_explicit_le_byte_fixture_pins_le_on_disk_contract() {
        // 1.0_f64:  bits = 0x3FF0_0000_0000_0000, LE bytes = [00 00 00 00 00 00 F0 3F]
        // -2.5_f64: bits = 0xC004_0000_0000_0000, LE bytes = [00 00 00 00 00 00 04 C0]
        let bytes: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // 1.0_f64
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xC0, // -2.5_f64
        ];
        let decoded = read_f64_slab(&mut &bytes[..], 2).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(
            decoded[0].to_bits(),
            0x3FF0_0000_0000_0000_u64,
            "1.0 fixture: LE bytes [00..F0 3F] must decode to 1.0, not from_be/ne_bytes"
        );
        assert_eq!(
            decoded[1].to_bits(),
            0xC004_0000_0000_0000_u64,
            "-2.5 fixture: LE bytes [00..04 C0] must decode to -2.5, not from_be/ne_bytes"
        );
    }

    /// Anchors the bincode 1.3.x default-options encoding
    /// (`DefaultOptions::new().with_fixint_encoding()` — the shared chain used by
    /// both free-function and `serialize_into` paths). Catches encoder drift INSIDE the `=1.3` Cargo pin that
    /// the version pin alone cannot block — a hypothetical patch-level change within
    /// the 1.3.x line would still be caught here because the byte sequence is pinned
    /// explicitly. Bumping bincode past `=1.3` requires both updating this literal AND
    /// bumping `ELASTIC_RESULT_FORMAT_VERSION` (cross-checked by
    /// `elastic_result_format_version_is_one`).
    ///
    /// Fixture uses recognisable, non-zero field values so the LE byte order is
    /// visually verifiable at the test site (e.g. `EF BE AD DE BE BA FE CA` for
    /// `max_von_mises_bits = 0xCAFE_BABE_DEAD_BEEF` in LE order). Distinct values
    /// per field defeat any accidental field-aliasing or field-duplication bug.
    #[test]
    fn elastic_result_header_bincode_encoding_matches_pinned_hex_literal() {
        let header = ElasticResultHeader {
            max_von_mises_bits: 0xCAFE_BABE_DEAD_BEEFu64,
            converged: true,
            iterations: 0x1234_5678u32,
            solve_time_ms: 0xDEAD_BEEF_CAFE_BABEu64,
            displacement_len: 5u64,
            stress_len: 7u64,
            // v3 additions: distinct non-zero values for visual LE-byte verification.
            divergence_len: 11u64,
            gradient_len: 99u64,
            curl_len: 33u64,
            grid_bounds_min_x_bits: 0x0000_0001_0000_0000u64,
            grid_bounds_min_y_bits: 0x0000_0002_0000_0000u64,
            grid_bounds_min_z_bits: 0x0000_0003_0000_0000u64,
            grid_bounds_max_x_bits: 0x0000_0004_0000_0000u64,
            grid_bounds_max_y_bits: 0x0000_0005_0000_0000u64,
            grid_bounds_max_z_bits: 0x0000_0006_0000_0000u64,
            grid_count_x: 8u64,
            grid_count_y: 6u64,
            grid_count_z: 4u64,
        };
        // Use serialize_into to mirror the production write path (ElasticResult::serialize_to_writer).
        let mut encoded: Vec<u8> = Vec::new();
        bincode::serialize_into(&mut encoded, &header)
            .expect("bincode serialize_into must not fail for fixed-size header");
        // Pinned bincode 1.3 fixint-LE encoding of the fixture header (v3 = 133 bytes).
        // Layout (struct-declaration order, LE encoding):
        //   max_von_mises_bits   (u64 LE, 8 bytes): EF BE AD DE BE BA FE CA
        //   converged            (bool,   1 byte):   01
        //   iterations           (u32 LE, 4 bytes):  78 56 34 12
        //   solve_time_ms        (u64 LE, 8 bytes):  BE BA FE CA EF BE AD DE
        //   displacement_len     (u64 LE, 8 bytes):  05 00 00 00 00 00 00 00
        //   stress_len           (u64 LE, 8 bytes):  07 00 00 00 00 00 00 00
        //   ── v3 additions (96 bytes total) ──────────────────────────────
        //   divergence_len       (u64 LE, 8 bytes):  0B 00 00 00 00 00 00 00  (= 11)
        //   gradient_len         (u64 LE, 8 bytes):  63 00 00 00 00 00 00 00  (= 99)
        //   curl_len             (u64 LE, 8 bytes):  21 00 00 00 00 00 00 00  (= 33)
        //   grid_bounds_min_x    (u64 LE, 8 bytes):  00 00 00 00 01 00 00 00
        //   grid_bounds_min_y    (u64 LE, 8 bytes):  00 00 00 00 02 00 00 00
        //   grid_bounds_min_z    (u64 LE, 8 bytes):  00 00 00 00 03 00 00 00
        //   grid_bounds_max_x    (u64 LE, 8 bytes):  00 00 00 00 04 00 00 00
        //   grid_bounds_max_y    (u64 LE, 8 bytes):  00 00 00 00 05 00 00 00
        //   grid_bounds_max_z    (u64 LE, 8 bytes):  00 00 00 00 06 00 00 00
        //   grid_count_x         (u64 LE, 8 bytes):  08 00 00 00 00 00 00 00  (= 8)
        //   grid_count_y         (u64 LE, 8 bytes):  06 00 00 00 00 00 00 00  (= 6)
        //   grid_count_z         (u64 LE, 8 bytes):  04 00 00 00 00 00 00 00  (= 4)
        // Total: 37 + 96 = 133 bytes.
        let expected: [u8; 133] = [
            // ── v2 base (37 bytes) ─────────────────────────────────────────────
            0xEF, 0xBE, 0xAD, 0xDE, 0xBE, 0xBA, 0xFE, 0xCA, // max_von_mises_bits LE
            0x01,                                              // converged = true
            0x78, 0x56, 0x34, 0x12,                           // iterations LE
            0xBE, 0xBA, 0xFE, 0xCA, 0xEF, 0xBE, 0xAD, 0xDE, // solve_time_ms LE
            0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // displacement_len = 5
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // stress_len = 7
            // ── v3 additions (96 bytes) ────────────────────────────────────────
            0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // divergence_len = 11
            0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // gradient_len = 99
            0x21, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // curl_len = 33
            0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // grid_bounds_min_x_bits
            0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, // grid_bounds_min_y_bits
            0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, // grid_bounds_min_z_bits
            0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, // grid_bounds_max_x_bits
            0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, // grid_bounds_max_y_bits
            0x00, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, // grid_bounds_max_z_bits
            0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // grid_count_x = 8
            0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // grid_count_y = 6
            0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // grid_count_z = 4
        ];
        assert_eq!(
            encoded.as_slice(),
            &expected[..],
            "bincode 1.3 default-options encoding of ElasticResultHeader has drifted \
             from the pinned wire-format fixture; if the change is intentional, bump \
             ELASTIC_RESULT_FORMAT_VERSION in the SAME commit and update this literal"
        );
        // Round-trip: decode from the pinned literal back to the original struct.
        // (Cannot use `assert_eq!(decoded, header)` because ElasticResultHeader does
        // not derive PartialEq — per-field asserts cover the full struct.)
        let decoded: ElasticResultHeader =
            bincode::deserialize(&expected[..]).expect("must decode pinned literal");
        assert_eq!(decoded.max_von_mises_bits, header.max_von_mises_bits);
        assert_eq!(decoded.converged, header.converged);
        assert_eq!(decoded.iterations, header.iterations);
        assert_eq!(decoded.solve_time_ms, header.solve_time_ms);
        assert_eq!(decoded.displacement_len, header.displacement_len);
        assert_eq!(decoded.stress_len, header.stress_len);
        // v3 additions.
        assert_eq!(decoded.divergence_len, header.divergence_len);
        assert_eq!(decoded.gradient_len, header.gradient_len);
        assert_eq!(decoded.curl_len, header.curl_len);
        assert_eq!(decoded.grid_bounds_min_x_bits, header.grid_bounds_min_x_bits);
        assert_eq!(decoded.grid_bounds_min_y_bits, header.grid_bounds_min_y_bits);
        assert_eq!(decoded.grid_bounds_min_z_bits, header.grid_bounds_min_z_bits);
        assert_eq!(decoded.grid_bounds_max_x_bits, header.grid_bounds_max_x_bits);
        assert_eq!(decoded.grid_bounds_max_y_bits, header.grid_bounds_max_y_bits);
        assert_eq!(decoded.grid_bounds_max_z_bits, header.grid_bounds_max_z_bits);
        assert_eq!(decoded.grid_count_x, header.grid_count_x);
        assert_eq!(decoded.grid_count_y, header.grid_count_y);
        assert_eq!(decoded.grid_count_z, header.grid_count_z);
    }

    #[test]
    fn partial_elastic_result_converts_to_elastic_result_field_for_field() {
        use reify_solver_elastic::progressive::PartialElasticResult;

        let partial = PartialElasticResult {
            displacement: vec![1.0, 2.0, 3.0],
            stress: vec![4.0, 5.0],
            max_von_mises: 123.5,
            converged: true,
            iterations: 7,
        };

        let full: ElasticResult = (&partial).into();

        // Shared fields must mirror the partial exactly.
        assert_eq!(full.displacement, vec![1.0, 2.0, 3.0]);
        assert_eq!(full.stress, vec![4.0, 5.0]);
        assert_eq!(full.max_von_mises, 123.5);
        assert!(full.converged);
        assert_eq!(full.iterations, 7);
        // ElasticResult-only fields must use their documented neutral defaults.
        assert_eq!(
            full.solve_time_ms, 0,
            "solve_time_ms must default to 0 for a partial snapshot"
        );
        assert!(
            full.shell_channels.is_none(),
            "shell_channels must default to None for tet-only solver"
        );
    }

    #[test]
    fn partial_elastic_result_by_value_moves_buffers() {
        use reify_solver_elastic::progressive::PartialElasticResult;

        let partial = PartialElasticResult {
            displacement: vec![10.0, 20.0],
            stress: vec![30.0, 40.0, 50.0],
            max_von_mises: 99.0,
            converged: false,
            iterations: 3,
        };

        // By-value conversion — moves displacement and stress without cloning.
        let full: ElasticResult = partial.into();

        assert_eq!(full.displacement, vec![10.0, 20.0]);
        assert_eq!(full.stress, vec![30.0, 40.0, 50.0]);
        assert_eq!(full.max_von_mises, 99.0);
        assert!(!full.converged);
        assert_eq!(full.iterations, 3);
        assert_eq!(full.solve_time_ms, 0);
        assert!(full.shell_channels.is_none());
    }

    /// step-3 RED (task #3428): `ElasticResult` with the new v3 fields (grid spec,
    /// divergence/gradient/curl slabs) round-trips through
    /// `serialize_to_writer` / `deserialize_from_reader` byte-deterministically.
    #[test]
    fn elastic_result_v3_grid_and_derivative_channels_round_trip_byte_deterministically() {
        // 2×3×4 element-count grid → (2+1)*(3+1)*(4+1) = 60 nodes.
        let n_nodes: usize = (2 + 1) * (3 + 1) * (4 + 1);
        let displacement: Vec<f64> = (0..n_nodes * 3).map(|i| i as f64 * 0.001).collect();
        let stress: Vec<f64> = (0..n_nodes * 9).map(|i| i as f64 * 1e6).collect();
        let divergence: Vec<f64> = (0..n_nodes).map(|i| i as f64 * 1e-5).collect();
        let gradient: Vec<f64> = (0..n_nodes * 9).map(|i| i as f64 * 1e-3).collect();
        let curl: Vec<f64> = (0..n_nodes * 3).map(|i| i as f64 * 2e-4).collect();

        let original = ElasticResult {
            displacement,
            stress,
            max_von_mises: 1.5e8,
            converged: true,
            iterations: 12,
            solve_time_ms: 500,
            shell_channels: None,
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [1.0, 0.3, 0.1],
            grid_counts: [2, 3, 4],
            divergence,
            gradient,
            curl,
            aposteriori: None,
        };

        // (a) Byte-determinism: two independent serialisations of identical data
        //     must produce bit-identical bytes.
        let mut buf_a: Vec<u8> = Vec::new();
        let mut buf_b: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf_a).unwrap();
        original.serialize_to_writer(&mut buf_b).unwrap();
        assert_eq!(buf_a, buf_b, "v3 serialisation must be byte-deterministic");

        // (b) Full round-trip: deserialised value must equal original across all
        //     new fields (grid_bounds_min/max/counts, divergence, gradient, curl).
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf_a[..]).unwrap();
        assert_eq!(decoded, original, "v3 round-trip must preserve all new fields losslessly");
    }

    #[test]
    fn elastic_result_format_version_is_3_after_v3_bump() {
        assert_eq!(<ElasticResult as PersistentlyCacheable>::FORMAT_VERSION, 3);
    }

    // ── task #4942: AposterioriEstimate probe-byte tail ──────────────────────
    //
    // Persists the 3 a-posteriori adaptive-refinement fields
    // (convergence_status, error_indicator, global_relative_energy_error) with
    // full fidelity, as a SECOND probe-byte-gated tail appended AFTER the
    // shell_channels tail -- mirroring the v1->v2 ShellChannels mechanism
    // (`ShellChannelsHeader` / `read_shell_channels_tail` above).
    // `aposteriori: None` (non-adaptive solves, AND every pre-4942 v3 cache
    // entry) writes NO tail bytes at all: the aposteriori-tail probe byte hits
    // true EOF, exactly like reading a pre-existing v3 entry.
    // `ELASTIC_RESULT_FORMAT_VERSION` stays 3 -- this is a strictly additive,
    // backward-compatible wire-format extension, not a version bump.
    //
    // RED: `AposterioriEstimate`, `ElasticResult.aposteriori`, and
    // `read_aposteriori_tail` do not exist yet -- compile-fail, the same RED
    // shape used throughout this module.

    /// (a) An adaptive `AposterioriEstimate` -- `NotConverged`, a populated
    /// stride-1 `error_indicator` slab, and a populated
    /// `global_relative_energy_error` -- round-trips BIT-EXACTLY through
    /// `serialize_to_writer` -> `deserialize_from_reader`.
    #[test]
    fn elastic_result_with_adaptive_aposteriori_round_trips_through_serialize_deserialize() {
        let error_indicator: Vec<f64> = (0..7u64)
            .map(|i| f64::from_bits(i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xABAD_1DEA_0BAD_F00D))
            .collect();
        let mut original = make_sample_result();
        original.aposteriori = Some(AposterioriEstimate {
            convergence_status: ConvergenceStatus::NotConverged { reason: BudgetReason::MaxDofs },
            error_indicator: Some(error_indicator.clone()),
            global_relative_energy_error: Some(0.0421),
        });

        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();

        assert_eq!(decoded, original, "adaptive aposteriori must round-trip bit-exactly");
        let est = decoded
            .aposteriori
            .as_ref()
            .expect("Some(_) round-trip must yield Some(_) on decode");
        assert_eq!(
            est.convergence_status,
            ConvergenceStatus::NotConverged { reason: BudgetReason::MaxDofs }
        );
        for (d, o) in est.error_indicator.as_ref().unwrap().iter().zip(error_indicator.iter()) {
            assert_eq!(d.to_bits(), o.to_bits(), "error_indicator bit pattern drift");
        }
        assert_eq!(
            est.global_relative_energy_error.unwrap().to_bits(),
            0.0421_f64.to_bits()
        );
    }

    /// (b) `aposteriori: None` must serialize to EXACTLY the pre-4942 v3 byte
    /// layout (header + slabs + 25-byte shell_channels trailer, nothing more),
    /// and hand-encoded pre-4942 bytes (no aposteriori tail at all) must
    /// deserialize back to `aposteriori: None` -- the aposteriori-tail probe
    /// byte hits true EOF, mirroring
    /// `elastic_result_deserialize_without_shell_channels_tail_yields_shell_channels_none`'s
    /// EOF probe one level up the tail chain.
    #[test]
    fn deserialize_of_pre_4942_v3_bytes_without_aposteriori_tail_yields_none() {
        let original = make_sample_result();
        assert!(original.aposteriori.is_none(), "make_sample_result must yield aposteriori: None");

        let mut new_format_bytes: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut new_format_bytes).unwrap();

        // Hand-encode the same logical content the pre-4942 serializer would
        // have produced: header + displacement/stress slabs + shell_channels
        // trailer, with NOTHING after it.
        let header = ElasticResultHeader {
            max_von_mises_bits: original.max_von_mises.to_bits(),
            converged: original.converged,
            iterations: original.iterations,
            solve_time_ms: original.solve_time_ms,
            displacement_len: original.displacement.len() as u64,
            stress_len: original.stress.len() as u64,
            divergence_len: 0,
            gradient_len: 0,
            curl_len: 0,
            grid_bounds_min_x_bits: 0,
            grid_bounds_min_y_bits: 0,
            grid_bounds_min_z_bits: 0,
            grid_bounds_max_x_bits: 0,
            grid_bounds_max_y_bits: 0,
            grid_bounds_max_z_bits: 0,
            grid_count_x: 0,
            grid_count_y: 0,
            grid_count_z: 0,
        };
        let shell_header =
            ShellChannelsHeader { present: false, top_len: 0, bottom_len: 0, frame_len: 0 };
        let mut pre_4942_bytes: Vec<u8> = Vec::new();
        {
            let mut encoder = zstd::Encoder::new(&mut pre_4942_bytes, 0).unwrap();
            bincode::serialize_into(&mut encoder, &header).unwrap();
            for v in &original.displacement {
                io::Write::write_all(&mut encoder, &v.to_le_bytes()).unwrap();
            }
            for v in &original.stress {
                io::Write::write_all(&mut encoder, &v.to_le_bytes()).unwrap();
            }
            bincode::serialize_into(&mut encoder, &shell_header).unwrap();
            // No aposteriori tail.
            encoder.finish().unwrap();
        }
        assert_eq!(
            new_format_bytes, pre_4942_bytes,
            "aposteriori: None must serialize to the exact pre-4942 v3 byte layout (no tail)"
        );

        let decoded = ElasticResult::deserialize_from_reader(&mut &pre_4942_bytes[..]).unwrap();
        assert!(
            decoded.aposteriori.is_none(),
            "pre-4942 v3 bytes (no aposteriori tail) must decode to aposteriori: None"
        );
    }

    /// (c) Coverage: every `BudgetReason` variant, a `Converged` status with a
    /// non-zero `final_indicator`, and `Some(estimate)` with `None`
    /// `error_indicator` / `None` `global_relative_energy_error` all
    /// round-trip.
    #[test]
    fn elastic_result_aposteriori_round_trips_every_budget_reason_and_optional_field_combination()
    {
        let statuses = [
            ConvergenceStatus::Converged { final_indicator: 0.0091 },
            ConvergenceStatus::NotConverged { reason: BudgetReason::TargetMissed },
            ConvergenceStatus::NotConverged { reason: BudgetReason::MaxIterations },
            ConvergenceStatus::NotConverged { reason: BudgetReason::MaxDofs },
            ConvergenceStatus::NotConverged { reason: BudgetReason::Stalled },
        ];
        for status in statuses {
            let mut original = make_sample_result();
            original.aposteriori = Some(AposterioriEstimate {
                convergence_status: status.clone(),
                error_indicator: None,
                global_relative_energy_error: None,
            });
            let mut buf: Vec<u8> = Vec::new();
            original.serialize_to_writer(&mut buf).unwrap();
            let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
            assert_eq!(
                decoded, original,
                "status {status:?} with None error_indicator/global_error must round-trip"
            );
        }
    }

    /// (d) Malformed aposteriori tails are rejected with `InvalidData` rather
    /// than silently misdecoded -- mirroring `read_shell_channels_tail`'s
    /// strict discriminant/length validation.
    ///
    /// Build a zstd-compressed stream: a minimal all-empty header + shell tail
    /// (present=false) followed by caller-supplied raw bytes standing in for a
    /// (possibly malformed) aposteriori tail. Lets the rejection tests below
    /// inject one bad byte without re-deriving the header/shell-tail
    /// boilerplate each time.
    fn encode_stream_with_raw_aposteriori_tail(tail_bytes: &[u8]) -> Vec<u8> {
        let header = ElasticResultHeader {
            max_von_mises_bits: 0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
            displacement_len: 0,
            stress_len: 0,
            divergence_len: 0,
            gradient_len: 0,
            curl_len: 0,
            grid_bounds_min_x_bits: 0,
            grid_bounds_min_y_bits: 0,
            grid_bounds_min_z_bits: 0,
            grid_bounds_max_x_bits: 0,
            grid_bounds_max_y_bits: 0,
            grid_bounds_max_z_bits: 0,
            grid_count_x: 0,
            grid_count_y: 0,
            grid_count_z: 0,
        };
        let shell_header =
            ShellChannelsHeader { present: false, top_len: 0, bottom_len: 0, frame_len: 0 };
        let mut buf: Vec<u8> = Vec::new();
        let mut encoder = zstd::Encoder::new(&mut buf, 0).unwrap();
        bincode::serialize_into(&mut encoder, &header).unwrap();
        bincode::serialize_into(&mut encoder, &shell_header).unwrap();
        io::Write::write_all(&mut encoder, tail_bytes).unwrap();
        encoder.finish().unwrap();
        buf
    }

    #[test]
    fn read_aposteriori_tail_rejects_unknown_convergence_discriminant() {
        // convergence_discriminant byte = 2 (only 0=Converged/1=NotConverged
        // are valid); the reader must fail on the probe byte itself, before
        // attempting to read the rest of the fixed header.
        let tail = [2u8];
        let stream = encode_stream_with_raw_aposteriori_tail(&tail);
        let err = ElasticResult::deserialize_from_reader(&mut &stream[..])
            .expect_err("unknown convergence discriminant must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "expected InvalidData, got {err:?}");
    }

    #[test]
    fn read_aposteriori_tail_rejects_unknown_budget_reason_discriminant() {
        // convergence_discriminant=1 (NotConverged), budget_reason_discriminant=99
        // (only 0..=3 are valid), final_indicator_bits=0 (unused for
        // NotConverged), error_indicator_present=0, error_indicator_len=0,
        // global_present=0, global_bits=0.
        let mut tail = vec![1u8, 99u8];
        tail.extend_from_slice(&0u64.to_le_bytes()); // final_indicator_bits
        tail.push(0u8); // error_indicator_present = false
        tail.extend_from_slice(&0u64.to_le_bytes()); // error_indicator_len
        tail.push(0u8); // global_present = false
        tail.extend_from_slice(&0u64.to_le_bytes()); // global_bits
        let stream = encode_stream_with_raw_aposteriori_tail(&tail);
        let err = ElasticResult::deserialize_from_reader(&mut &stream[..])
            .expect_err("unknown budget-reason discriminant must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "expected InvalidData, got {err:?}");
    }

    #[test]
    fn read_aposteriori_tail_rejects_present_false_with_nonzero_indicator_len() {
        // convergence_discriminant=0 (Converged), budget_reason_discriminant=0
        // (unused), final_indicator_bits=0, error_indicator_present=0 (false)
        // but error_indicator_len=5 (non-zero) -- a tampered/corrupted entry
        // claiming "absent" while advertising a slab length.
        let mut tail = vec![0u8, 0u8];
        tail.extend_from_slice(&0u64.to_le_bytes()); // final_indicator_bits
        tail.push(0u8); // error_indicator_present = false
        tail.extend_from_slice(&5u64.to_le_bytes()); // error_indicator_len = 5 (non-zero!)
        tail.push(0u8); // global_present = false
        tail.extend_from_slice(&0u64.to_le_bytes()); // global_bits
        let stream = encode_stream_with_raw_aposteriori_tail(&tail);
        let err = ElasticResult::deserialize_from_reader(&mut &stream[..])
            .expect_err("present=false with non-zero error_indicator_len must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "expected InvalidData, got {err:?}");
    }

    // ── Scalar deflection reducer tests (task #4757 step-1) ──────────────────

    /// `max_deflection_magnitude` returns the max per-point L2 norm over a
    /// stride-3 displacement buffer.
    #[test]
    fn max_deflection_magnitude_known_buffer() {
        // Points: (3,4,0) → norm=5, (0,0,0) → norm=0, (1,2,2) → norm=3
        let buf = vec![3.0_f64, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0];
        let result = max_deflection_magnitude(&buf);
        assert!(
            (result - 5.0_f64).abs() < 1e-10,
            "expected max L2 norm 5.0, got {result}"
        );
    }

    #[test]
    fn max_deflection_magnitude_empty_buffer() {
        assert_eq!(max_deflection_magnitude(&[]), 0.0);
    }

    /// Non-finite component in a point should be skipped (defensive).
    #[test]
    fn max_deflection_magnitude_skips_nonfinite() {
        // Point 0: (inf, 0, 0) → skip; Point 1: (3, 4, 0) → norm=5
        let buf = vec![f64::INFINITY, 0.0, 0.0, 3.0, 4.0, 0.0];
        let result = max_deflection_magnitude(&buf);
        assert!(
            (result - 5.0_f64).abs() < 1e-10,
            "expected 5.0 after skipping non-finite, got {result}"
        );
    }

    /// `ElasticResult::max_deflection` delegates to `max_deflection_magnitude`.
    #[test]
    fn elastic_result_max_deflection_delegates() {
        let er = ElasticResult {
            displacement: vec![3.0, 4.0, 0.0, 0.0, 0.0, 0.0],
            stress: vec![],
            max_von_mises: 0.0,
            converged: true,
            iterations: 0,
            solve_time_ms: 0,
            shell_channels: None,
            grid_bounds_min: [0.0; 3],
            grid_bounds_max: [0.0; 3],
            grid_counts: [0; 3],
            divergence: vec![],
            gradient: vec![],
            curl: vec![],
            aposteriori: None,
        };
        // Max L2 norm of (3,4,0)=5 and (0,0,0)=0 → 5.0
        assert!(
            (er.max_deflection() - 5.0_f64).abs() < 1e-10,
            "ElasticResult::max_deflection expected 5.0, got {}",
            er.max_deflection()
        );
    }
}
