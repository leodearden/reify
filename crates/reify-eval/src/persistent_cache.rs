//! Cross-session persistent cache for `ComputeNode` value types.
//!
//! See `docs/prds/v0_3/persistent-fea-cache.md` for the full PRD. This module
//! defines the opt-in [`PersistentlyCacheable`] trait that value types must
//! implement to participate in the on-disk persistent cache, and provides the
//! first concrete impl: [`ElasticResult`], the linear-elastostatic FEA solver
//! output container.
//!
//! # Co-location rationale
//!
//! The Rust `ElasticResult` struct is co-located with the trait here rather
//! than living in `reify-stdlib::fea` (as the task description initially
//! suggested) because `reify-stdlib` cannot depend on `reify-eval` — the
//! reverse edge (`reify-eval -> reify-expr -> reify-stdlib`) already exists,
//! so adding `reify-stdlib -> reify-eval` would form a cycle. The orphan rule
//! then forces either the trait or the impl into `reify-eval`; co-locating
//! both here is the smallest blast-radius option. Recorded as escalation
//! `esc-2969-65` for steward visibility.
//!
//! # Encoding strategy
//!
//! The trait is intentionally NOT object-safe: `serialize_to_writer` and
//! `deserialize_from_reader` use `impl Write` / `impl Read` generics so the
//! cache layer can monomorphise the zstd Encoder/Decoder paths for each
//! concrete writer/reader. The cache keys on concrete types per entry, so
//! static dispatch is sufficient.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Compute the canonical engine-version hash for a set of contributor byte slices.
///
/// Re-exported from [`crate::engine_hash_algo`], which is the single source of
/// truth shared between the library crate and `build.rs` (included via
/// `include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/engine_hash_algo.rs"))`).
/// Any change to the algorithm in `src/engine_hash_algo.rs` affects both callers
/// simultaneously — there is no duplicate copy.
///
/// See [`crate::engine_hash_algo::compose_engine_version_hash`] for the full
/// documentation including framing rationale and PRD reference.
pub use crate::engine_hash_algo::compose_engine_version_hash;

/// Magic byte written as the sole content of every `.meta` sidecar file.
///
/// Mnemonic: `CA` = **CA**che. The single-byte content is self-identifying —
/// a future `cache fsck` command can reject any `.meta` file whose first byte
/// is not `0xCA` without reading the adjacent `.bin`. The content is
/// deliberately minimal; the mtime of the `.meta` file is the real
/// last-access signal (see [`touch_sidecar`]).
pub const SIDECAR_MAGIC_BYTE: u8 = 0xCA;

/// Return the `mtime` (`SystemTime`) of the `.meta` sidecar file at `path`.
///
/// This is the last-access signal for GC eviction sorting and `cache stats`
/// display. The mtime belongs to the `.meta` file specifically — NOT to the
/// `.bin` file — per PRD policy (the `.bin` mtime stays fixed at
/// `written_at`, which is useful for debugging; only the sidecar is touched
/// on every read).
///
/// Propagates `io::Error` for both `NotFound` (missing sidecar) and
/// permission errors via the `?` operator.
pub fn read_sidecar_mtime(path: &Path) -> io::Result<std::time::SystemTime> {
    std::fs::metadata(path)?.modified()
}

/// Update the mtime of an existing `.meta` sidecar file to `now` without
/// altering its content.
///
/// Touching the sidecar (rather than the `.bin`) on every cache read preserves
/// the `.bin` mtime at the `written_at` wall-time, which is useful for
/// debugging ("when was this entry written?"). The sidecar mtime then carries
/// the last-access signal, used for cost-weighted LRU eviction by the GC.
///
/// Per PRD `docs/prds/v0_3/persistent-fea-cache.md` §"Sidecar `.meta` file":
/// works correctly under `noatime` and `relatime` mount options, where direct
/// `atime` on the `.bin` would be either suppressed entirely or rounded to
/// 24-hour resolution.
///
/// Uses [`std::fs::FileTimes::set_modified`] (stable since Rust 1.75,
/// Dec 2023) — avoids adding the `filetime` crate to reify-eval's dep set.
///
/// # Race condition: evicted sidecar
///
/// If the GC evicts an entry between the cache read and this `touch_sidecar`
/// call, the `.meta` file will have been removed and `open` will return
/// `ErrorKind::NotFound`. On the read-path this is benign — the entry is gone
/// and the touch is a no-op — so this function returns `Ok(())` for
/// `NotFound`. All other errors (e.g. permission denied) are propagated to the
/// caller.
///
/// # Permission notes
///
/// Opening with `write(true)` requires write permission on the file. On
/// read-only mounts or under restrictive ACLs this returns `PermissionDenied`,
/// which is propagated to the caller.
pub fn touch_sidecar(path: &Path) -> io::Result<()> {
    use std::fs::File;
    use std::time::SystemTime;
    let f = match File::options().write(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    f.set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()))
}

/// Create (or overwrite) a `.meta` sidecar file at `path` containing exactly
/// [`SIDECAR_MAGIC_BYTE`].
///
/// The parent directory must already exist; callers are expected to call
/// `fs::create_dir_all(&shard_dir(...))` before writing any files into the
/// shard. Failure to create the parent surfaces as an `io::Error` from the
/// underlying `fs::write`.
///
/// # Concurrency
///
/// Safe to call concurrently because the payload is a single byte. If the
/// sidecar ever grows past one byte, switch to a temp-file + atomic-rename
/// strategy to avoid torn reads.
pub fn write_sidecar(path: &Path) -> io::Result<()> {
    std::fs::write(path, [SIDECAR_MAGIC_BYTE])
}

/// On-disk layout version for the [`CacheEntryHeader`] struct. Bump when the
/// on-disk entry-header schema changes (fields added, removed, reordered, or
/// bincode/zstd wire-format shifts).
///
/// **Distinct from `ELASTIC_RESULT_FORMAT_VERSION`** (which versions the
/// *body* encoding per-type) and from [`ENGINE_VERSION_HASH`] (which
/// invalidates result *semantics* when solver sources change). These three
/// version namespaces must never be conflated — see PRD
/// `docs/prds/v0_3/persistent-fea-cache.md` §"Storage format":
/// "Format-version is separate from engine-version-hash — engine bumps
/// invalidate result semantics; format bumps invalidate on-disk layout.
/// Don't conflate."
///
/// **Wire-format contract:** `ENTRY_FORMAT_VERSION` covers the `bincode 1.3`
/// fixint-LE encoding of [`CacheEntryHeader`] (4+32+32+8+8+8 = 92 bytes)
/// AND the `zstd 0.13` compressed body that follows it. Any change to either
/// encoder that produces different bytes on disk — including a minor version
/// bump within the `=1.3` or `0.13` pins — MUST be accompanied by a bump of
/// this constant in the same commit. Pinned by
/// `cache_entry_header_bincode_encoding_matches_pinned_hex_literal` and
/// `entry_format_version_const_is_one`.
///
/// Starting at 1 follows the Reify convention that 0 means "uninitialised /
/// unknown", matching `ELASTIC_RESULT_FORMAT_VERSION`.
pub const ENTRY_FORMAT_VERSION: u32 = 1;

/// Fixed byte length of a bincode-1.3 fixint-LE encoded [`CacheEntryHeader`].
///
/// Computed from the field sizes:
/// - `format_version` (u32, 4 bytes)
/// - `engine_version_hash` ([u8; 32], 32 bytes)
/// - `input_hash` ([u8; 32], 32 bytes)
/// - `solve_time_ms` (u64, 8 bytes)
/// - `byte_size` (u64, 8 bytes)
/// - `written_at` (i64, 8 bytes)
///
/// Total: 4 + 32 + 32 + 8 + 8 + 8 = 92 bytes.
///
/// Pinned by `cache_entry_header_bincode_encoding_matches_pinned_hex_literal`.
pub const ENTRY_HEADER_ENCODED_LEN: usize = 92;

/// Header placed at the leading `ENTRY_HEADER_ENCODED_LEN` bytes of every
/// `.bin` cache-entry file, before the zstd-compressed body written by
/// [`PersistentlyCacheable::serialize_to_writer`].
///
/// # PRD reference
///
/// Defined in `docs/prds/v0_3/persistent-fea-cache.md` §"Header schema".
///
/// # Wire-format contract
///
/// Field order in this struct IS the on-disk byte order. Reordering fields IS
/// a wire-format change that requires a [`ENTRY_FORMAT_VERSION`] bump.
/// [`bincode`] 1.3 fixint-LE encodes this struct as a fixed-size 92-byte
/// sequence — pinned by
/// `cache_entry_header_bincode_encoding_matches_pinned_hex_literal`.
///
/// # Fields
///
/// - `format_version`: Echoes [`ENTRY_FORMAT_VERSION`]; allows a reader to
///   detect layout mismatches before attempting to decode the body.
/// - `engine_version_hash`: 32 ASCII bytes of the directory-level
///   [`ENGINE_VERSION_HASH`] hex string. Stored as ASCII bytes (NOT 16 raw
///   hash bytes) so corruption detection is a memcmp on the same bytes the
///   filesystem already returns as a `&str`.
/// - `input_hash`: 32 ASCII bytes of the filename hex string
///   (`ContentHash::Display`). Same rationale as `engine_version_hash`.
/// - `solve_time_ms`: solver wall-time cost metric, used for cost-weighted
///   LRU eviction.
/// - `byte_size`: uncompressed body byte count for `cache stats` display,
///   readable without decompressing the body.
/// - `written_at`: unix-millisecond timestamp of when the entry was written.
///   Signed `i64`; -1 is a valid sentinel for "unknown". Range is ample
///   (covers year ~292M CE).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CacheEntryHeader {
    /// Echoes [`ENTRY_FORMAT_VERSION`]; mismatch → layout incompatibility.
    pub format_version: u32,
    /// 32 ASCII bytes of the `engine_version_hash` hex string (directory name).
    pub engine_version_hash: [u8; 32],
    /// 32 ASCII bytes of the `input_hash` hex string (file stem).
    pub input_hash: [u8; 32],
    /// Solver wall-time in milliseconds.
    pub solve_time_ms: u64,
    /// Uncompressed body byte count.
    pub byte_size: u64,
    /// Write timestamp as unix milliseconds (signed; -1 = unknown).
    pub written_at: i64,
}

impl CacheEntryHeader {
    /// Verify that `format_version` in this header matches
    /// [`ENTRY_FORMAT_VERSION`], returning `Err(io::ErrorKind::InvalidData)`
    /// on mismatch.
    ///
    /// Call this alongside [`Self::verify_field_echoes`] after decoding a
    /// header to ensure the on-disk layout is compatible before attempting to
    /// decode the body. Keeping the check here (rather than at every call
    /// site) means readers do not need to import `ENTRY_FORMAT_VERSION`
    /// directly — the contract is fully expressed by `CacheEntryHeader`.
    pub fn verify_format_version(&self) -> io::Result<()> {
        if self.format_version != ENTRY_FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CacheEntryHeader format_version {} does not match \
                     ENTRY_FORMAT_VERSION {} (incompatible on-disk layout?)",
                    self.format_version, ENTRY_FORMAT_VERSION
                ),
            ));
        }
        Ok(())
    }

    /// Verify that the echo fields in this header match the expected key
    /// components, returning `Err(io::ErrorKind::InvalidData)` on mismatch.
    ///
    /// Called by the cache reader after decoding the header to detect
    /// corrupted entries (a misplaced or bit-flipped `.bin` file where
    /// the header echoes disagree with the directory name / filename).
    ///
    /// Per PRD `docs/prds/v0_3/persistent-fea-cache.md` §"Header schema":
    /// "engine_version_hash and input_hash are echoes of the directory-level
    /// and filename-level values, so corruption is detectable."
    ///
    /// Callers should also call [`Self::verify_format_version`] to check that
    /// the on-disk layout version is compatible — that check is separate
    /// because a version mismatch and a corrupted echo are distinct failure
    /// modes requiring different handling.
    pub fn verify_field_echoes(
        &self,
        expected_engine_version_hash: &[u8; 32],
        expected_input_hash: &[u8; 32],
    ) -> io::Result<()> {
        if &self.engine_version_hash != expected_engine_version_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CacheEntryHeader engine_version_hash echo does not match \
                 directory name (corrupted entry?)",
            ));
        }
        if &self.input_hash != expected_input_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CacheEntryHeader input_hash echo does not match \
                 filename (corrupted entry?)",
            ));
        }
        Ok(())
    }

    /// Encode `self` into `w` using bincode 1.3 fixint-LE encoding.
    ///
    /// Same error-mapping discipline as
    /// [`ElasticResult::serialize_to_writer`]: `bincode::Error` is wrapped via
    /// [`io::Error::other`] because `bincode::Error` does not implement
    /// `Into<io::Error>`.
    pub fn write_to<W: Write>(&self, w: &mut W) -> io::Result<()> {
        bincode::serialize_into(w, self).map_err(io::Error::other)
    }

    /// Decode a [`CacheEntryHeader`] from `r`.
    ///
    /// Same error-mapping discipline as
    /// [`ElasticResult::deserialize_from_reader`].
    pub fn read_from<R: Read>(r: &mut R) -> io::Result<Self> {
        bincode::deserialize_from(r).map_err(io::Error::other)
    }
}

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
const ELASTIC_RESULT_FORMAT_VERSION: u32 = 3;

/// Canonical engine-version hash for FEA persistent-cache keys. Baked at
/// build time by `build.rs` over the contributor source files listed in
/// `CONTRIBUTORS_RELATIVE` (reify-solver-elastic, reify-kernel-gmsh, stdlib
/// FEA helpers, per-purpose tolerance impls in this crate, and the workspace
/// `Cargo.lock` for transitive-dep version pinning).
///
/// **Distinct from `ELASTIC_RESULT_FORMAT_VERSION`**: `FORMAT_VERSION` tracks
/// the wire format (encoding layout — bump when `bincode`/`zstd` encoding
/// changes). `ENGINE_VERSION_HASH` tracks result semantics — bump happens
/// automatically when any contributor source file changes; no manual bump is
/// ever needed.
///
/// When any contributor changes, `build.rs` recomputes this hash; all existing
/// cache entries miss and are recomputed from scratch (invalidate-by-miss per
/// PRD `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine
/// version"). Cross-reference: [`compose_engine_version_hash`] is the library
/// function that documents and pins the hashing algorithm.
pub const ENGINE_VERSION_HASH: &str = env!("REIFY_ENGINE_VERSION_HASH");

// Compile-time sentinel: `bincode::ErrorKind` is part of the public bincode
// 1.x API but does not exist in bincode 2.x (which ships an entirely different
// error model). If the `=1.3` pin in `Cargo.toml` is ever relaxed past the
// 1.x major and the resolver picks up a 2.x release, this alias will fail to
// compile — a secondary tripwire alongside the doc-level contract above.
#[allow(dead_code)]
type _BincodeV1Sentinel = bincode::ErrorKind;

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
const MAX_F64_ELEMENTS: u64 = 1 << 24;

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

/// Opt-in trait for `ComputeNode` output value types that may be persisted
/// across sessions in the on-disk cache.
///
/// The trait declaration was moved to `reify-core::persistent_cache` during
/// task γ (#3834) to break the `reify-shell-extract → reify-eval` dependency
/// cycle. This re-export shim preserves every existing
/// `use reify_eval::persistent_cache::PersistentlyCacheable;` import path
/// without change.
///
/// See [`reify_core::persistent_cache::PersistentlyCacheable`] for the full
/// documentation.
pub use reify_core::persistent_cache::PersistentlyCacheable;

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
        }
    }
}

/// Validate a header-declared `Vec<f64>` length against [`MAX_F64_ELEMENTS`]
/// before it is fed to a `Vec` reservation. Returns the length cast to `usize`
/// on success, or `io::Error(InvalidData)` with a descriptive message on
/// overflow. The cast is safe post-check because `MAX_F64_ELEMENTS = 1<<24`
/// fits in `u32`, so it cannot truncate even on a 32-bit `usize`.
fn check_f64_vec_len(field_name: &str, len: u64) -> io::Result<usize> {
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
fn write_f64_slab<W: Write>(w: &mut W, slab: &[f64]) -> io::Result<()> {
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
fn read_f64_slab<R: Read>(r: &mut R, len: usize) -> io::Result<Vec<f64>> {
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
fn decode_f64_slab_from_le_bytes(bytes: &[u8]) -> impl Iterator<Item = f64> + '_ {
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

/// Construct the two-level shard directory for a given set of key components.
///
/// # Layout
///
/// ```text
/// <cache_root>/<engine_version_hash>/<input_hash[0..2]>
/// ```
///
/// Callers create this directory once via `fs::create_dir_all(&shard_dir(...))`
/// and then write both the `.bin` and `.meta` files into it. The directory is
/// the shared parent of both files, which is a structural requirement for
/// atomic-rename and for GC sweeps to touch related files together.
///
/// See [`entry_bin_path`] for the full layout and precondition documentation.
pub fn shard_dir(cache_root: &Path, engine_version_hash: &str, input_hash: &str) -> PathBuf {
    debug_assert!(
        input_hash.len() >= 2,
        "shard_dir: input_hash must be at least 2 chars, got {:?}",
        input_hash
    );
    cache_root.join(engine_version_hash).join(&input_hash[..2])
}

/// Construct the `.meta` sidecar path for a given set of key components.
///
/// The `.meta` file lives in the same directory as the corresponding `.bin`
/// file — this parent-dir invariant is exercised by
/// `entry_meta_path_uses_meta_extension_under_same_shard_dir_as_bin` and is
/// structurally required for atomic-rename semantics (the write orchestrator
/// can `create_dir_all(&shard_dir(...))` once and then write both files into
/// it).
///
/// Delegates to [`shard_dir`] for the parent directory; see [`shard_dir`] for
/// the precondition documentation.
pub fn entry_meta_path(cache_root: &Path, engine_version_hash: &str, input_hash: &str) -> PathBuf {
    shard_dir(cache_root, engine_version_hash, input_hash).join(format!("{input_hash}.meta"))
}

/// Construct the `.bin` cache-entry path for a given set of key components.
///
/// # Layout
///
/// ```text
/// <cache_root>/<engine_version_hash>/<input_hash[0..2]>/<input_hash>.bin
/// ```
///
/// Two-level git-style sharding per PRD
/// `docs/prds/v0_3/persistent-fea-cache.md` §"Filesystem layout". The first
/// level (`engine_version_hash`) groups all entries for the same engine build
/// together, making engine-version invalidation (directory removal) O(1). The
/// second level (`input_hash[0..2]`) limits directory fanout for large caches.
///
/// Delegates to [`shard_dir`] for the parent directory, so the layout
/// invariant (and the `debug_assert!` on `input_hash.len() >= 2`) lives in
/// one place. See [`shard_dir`] for the precondition documentation.
pub fn entry_bin_path(cache_root: &Path, engine_version_hash: &str, input_hash: &str) -> PathBuf {
    shard_dir(cache_root, engine_version_hash, input_hash).join(format!("{input_hash}.bin"))
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
        header_bytes + slab_bytes + shell_header_bytes + shell_slab_bytes
    }

    fn solve_time_ms(&self) -> u64 {
        self.solve_time_ms
    }
}

/// On-disk-layout version for [`BucklingResultCache`]. FORMAT_VERSION = 1.
///
/// Separate from [`ELASTIC_RESULT_FORMAT_VERSION`] and [`ENGINE_VERSION_HASH`]
/// — format bumps invalidate the encoding layout; engine-hash changes
/// invalidate result semantics. Starting at 1 follows the Reify convention.
const BUCKLING_RESULT_FORMAT_VERSION: u32 = 1;

/// Compact bincode-encoded prefix for [`BucklingResultCache`]'s zstd body.
///
/// All `f64` scalars are stored as `u64` bit-patterns (NaN-safe, identical
/// discipline to [`ElasticResultHeader`]).
///
/// Layout on disk (bincode 1.3 fixint-LE):
///   n_modes:                   u64 (8 bytes)
///   mode_shape_stride:         u64 (8 bytes) — base_node_positions.len()
///   ps_displacement_len:       u64 (8 bytes)
///   ps_stress_len:             u64 (8 bytes)
///   converged:                 bool (1 byte)
///   iterations:                u32 (4 bytes)
///   ps_max_von_mises_bits:     u64 (8 bytes) — f64 NaN-safe
///   ps_converged:              bool (1 byte)
///   ps_iterations:             u32 (4 bytes)
///   ps_grid_bounds_{min,max}_{x,y,z}_bits: 6 × u64 (48 bytes)
///   ps_grid_count_{x,y,z}:    3 × u64 (24 bytes)
///   solve_time_ms:             u64 (8 bytes)
///
/// Total (fixed): 8+8+8+8+1+4+8+1+4+48+24+8 = 130 bytes.
#[derive(Serialize, Deserialize)]
struct BucklingResultHeader {
    /// Number of buckling modes stored.
    n_modes: u64,
    /// Length of `mode_shapes` per mode AND of `base_node_positions`
    /// (= 3 × n_active_nodes). Total mode_shapes slab = n_modes × mode_shape_stride.
    mode_shape_stride: u64,
    /// Length of `ps_displacement` (= grid_count × 3).
    ps_displacement_len: u64,
    /// Length of `ps_stress` (= grid_count × 9).
    ps_stress_len: u64,
    /// `BucklingResult.converged`.
    converged: bool,
    /// `BucklingResult.iterations` cast to u32.
    iterations: u32,
    /// `pre_stress.max_von_mises` as raw u64 bit-pattern (NaN-safe).
    ps_max_von_mises_bits: u64,
    /// `pre_stress.converged`.
    ps_converged: bool,
    /// `pre_stress.iterations` cast to u32.
    ps_iterations: u32,
    // Grid spec: stored as raw u64 bit-patterns for NaN safety.
    ps_grid_bounds_min_x_bits: u64,
    ps_grid_bounds_min_y_bits: u64,
    ps_grid_bounds_min_z_bits: u64,
    ps_grid_bounds_max_x_bits: u64,
    ps_grid_bounds_max_y_bits: u64,
    ps_grid_bounds_max_z_bits: u64,
    /// Element-interval count along axis 0.
    ps_grid_count_x: u64,
    /// Element-interval count along axis 1.
    ps_grid_count_y: u64,
    /// Element-interval count along axis 2.
    ps_grid_count_z: u64,
    /// Solver wall-time for cost-weighted LRU eviction.
    solve_time_ms: u64,
}

/// Buckling eigensolver output container for the persistent on-disk cache.
///
/// Captures the full [`BucklingResult`]-shaped `Value::StructureInstance` emitted
/// by [`crate::compute_targets::buckling::solve_buckling_trampoline`].
///
/// # Encoding
///
/// Single zstd stream containing:
/// 1. bincode 1.3 fixint-LE [`BucklingResultHeader`] (130 bytes).
/// 2. `eigenvalues` f64 slab (n_modes × 8 bytes).
/// 3. `mode_shapes` f64 slab (n_modes × mode_shape_stride × 8 bytes).
/// 4. `base_node_positions` f64 slab (mode_shape_stride × 8 bytes).
/// 5. `ps_displacement` f64 slab (ps_displacement_len × 8 bytes).
/// 6. `ps_stress` f64 slab (ps_stress_len × 8 bytes).
///
/// All slabs use the same NaN-safe little-endian raw f64 encoding as
/// [`ElasticResult`] (via [`write_f64_slab`] / [`read_f64_slab`] /
/// [`check_f64_vec_len`] / [`MAX_F64_ELEMENTS`]).
///
/// # Hash-identity contract
///
/// [`crate::compute_targets::buckling::value_from_buckling_result`] must
/// reconstruct a `Value` whose `content_hash()` is bit-identical to the
/// original trampoline output. This is guaranteed by reconstructing the
/// `pre_stress` StructureInstance with **exactly 6 fields** (the trampoline's
/// layout), not the 10-field layout of [`crate::compute_targets::elastic_static::value_from_elastic_result`].
#[derive(Debug, Clone, PartialEq)]
pub struct BucklingResultCache {
    /// Buckling eigenvalues λ, one per mode. Length = n_modes.
    pub eigenvalues: Vec<f64>,
    /// Flat displaced positions: all modes concatenated.
    /// Layout: modes[0]||modes[1]||...; each mode has `mode_shape_stride` f64s
    /// (= 3 × n_active_nodes: flat xyz of base + eigenvector per node).
    pub mode_shapes: Vec<f64>,
    /// Undeformed node positions; length = mode_shape_stride (= 3 × n_active_nodes).
    pub base_node_positions: Vec<f64>,
    /// `BucklingResult.converged`.
    pub converged: bool,
    /// `BucklingResult.iterations`.
    pub iterations: u32,
    /// Pre-stress displacement field data (grid resampled, stride 3).
    pub ps_displacement: Vec<f64>,
    /// Pre-stress stress field data (grid resampled, stride 9).
    pub ps_stress: Vec<f64>,
    /// `pre_stress.max_von_mises` (SI Pascals).
    pub ps_max_von_mises: f64,
    /// `pre_stress.converged`.
    pub ps_converged: bool,
    /// `pre_stress.iterations`.
    pub ps_iterations: u32,
    /// Grid lower bounds per axis (SI units). Matches `GridSpec::bounds_min`.
    pub ps_grid_bounds_min: [f64; 3],
    /// Grid upper bounds per axis (SI units). Matches `GridSpec::bounds_max`.
    pub ps_grid_bounds_max: [f64; 3],
    /// Element-interval counts per axis. Grid has `counts[i]+1` nodes per axis.
    pub ps_grid_counts: [u64; 3],
    /// Solver wall-time in milliseconds, for cost-weighted LRU eviction.
    pub solve_time_ms: u64,
}

// Compile-time sentinel: BucklingResultCache: PersistentlyCacheable.
// Lives at module scope (outside #[cfg(test)]) so the trait-bound is enforced
// on every build, not only when `cargo test` links.
const _: fn() = || {
    fn assert_impl<T: PersistentlyCacheable>() {}
    assert_impl::<BucklingResultCache>();
};

impl PersistentlyCacheable for BucklingResultCache {
    const FORMAT_VERSION: u32 = BUCKLING_RESULT_FORMAT_VERSION;

    fn serialize_to_writer(&self, w: &mut impl Write) -> io::Result<()> {
        // Single zstd stream — zstd level 0 (default = 3 in zstd 0.13),
        // byte-deterministic for identical input, single-threaded only.
        let mut encoder = zstd::Encoder::new(w, 0)?;

        let mode_shape_stride = if self.eigenvalues.is_empty() {
            self.base_node_positions.len() as u64
        } else {
            // All modes share the same stride; derive from the total length.
            (self.mode_shapes.len() / self.eigenvalues.len().max(1)) as u64
        };

        // Serialize-time coupling invariant: base_node_positions.len() must
        // equal the per-mode mode_shape stride.  The deserializer reads
        // base_node_positions with stride_cap == mode_shape_stride, so any
        // divergence (e.g. condensed P2 mode shapes) would silently mis-align
        // every subsequent slab.  Enforce the coupling here rather than
        // producing a corrupt cache entry that only fails on read.
        debug_assert_eq!(
            self.base_node_positions.len() as u64,
            mode_shape_stride,
            "BucklingResultCache serialize: base_node_positions.len() ({}) != \
             mode_shape_stride ({}) — a future mode-shape format change diverged; \
             add a base_node_positions_len header field if the lengths must differ",
            self.base_node_positions.len(),
            mode_shape_stride,
        );

        let header = BucklingResultHeader {
            n_modes: self.eigenvalues.len() as u64,
            mode_shape_stride,
            ps_displacement_len: self.ps_displacement.len() as u64,
            ps_stress_len: self.ps_stress.len() as u64,
            converged: self.converged,
            iterations: self.iterations,
            ps_max_von_mises_bits: self.ps_max_von_mises.to_bits(),
            ps_converged: self.ps_converged,
            ps_iterations: self.ps_iterations,
            ps_grid_bounds_min_x_bits: self.ps_grid_bounds_min[0].to_bits(),
            ps_grid_bounds_min_y_bits: self.ps_grid_bounds_min[1].to_bits(),
            ps_grid_bounds_min_z_bits: self.ps_grid_bounds_min[2].to_bits(),
            ps_grid_bounds_max_x_bits: self.ps_grid_bounds_max[0].to_bits(),
            ps_grid_bounds_max_y_bits: self.ps_grid_bounds_max[1].to_bits(),
            ps_grid_bounds_max_z_bits: self.ps_grid_bounds_max[2].to_bits(),
            ps_grid_count_x: self.ps_grid_counts[0],
            ps_grid_count_y: self.ps_grid_counts[1],
            ps_grid_count_z: self.ps_grid_counts[2],
            solve_time_ms: self.solve_time_ms,
        };
        bincode::serialize_into(&mut encoder, &header).map_err(io::Error::other)?;

        // Slabs: eigenvalues → mode_shapes → base_node_positions →
        // ps_displacement → ps_stress.
        write_f64_slab(&mut encoder, &self.eigenvalues)?;
        write_f64_slab(&mut encoder, &self.mode_shapes)?;
        write_f64_slab(&mut encoder, &self.base_node_positions)?;
        write_f64_slab(&mut encoder, &self.ps_displacement)?;
        write_f64_slab(&mut encoder, &self.ps_stress)?;

        encoder.finish()?;
        Ok(())
    }

    fn deserialize_from_reader(r: &mut impl Read) -> io::Result<Self> {
        let mut decoder = zstd::Decoder::new(r)?;

        let header: BucklingResultHeader =
            bincode::deserialize_from(&mut decoder).map_err(io::Error::other)?;

        // Validate all slab lengths before allocation.
        let n_modes_cap = check_f64_vec_len("buckling.eigenvalues", header.n_modes)?;
        let stride_cap =
            check_f64_vec_len("buckling.mode_shape_stride", header.mode_shape_stride)?;
        // Total mode_shapes slab = n_modes × stride; check for overflow and cap.
        let mode_shapes_total = header
            .n_modes
            .checked_mul(header.mode_shape_stride)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "BucklingResultCache: n_modes × mode_shape_stride overflows u64 \
                     (corrupted or tampered cache entry?)",
                )
            })?;
        let mode_shapes_cap =
            check_f64_vec_len("buckling.mode_shapes_total", mode_shapes_total)?;
        let ps_displacement_cap =
            check_f64_vec_len("buckling.ps_displacement", header.ps_displacement_len)?;
        let ps_stress_cap = check_f64_vec_len("buckling.ps_stress", header.ps_stress_len)?;

        // Decode slabs in order.
        let eigenvalues = read_f64_slab(&mut decoder, n_modes_cap)?;
        let mode_shapes = read_f64_slab(&mut decoder, mode_shapes_cap)?;
        let base_node_positions = read_f64_slab(&mut decoder, stride_cap)?;
        let ps_displacement = read_f64_slab(&mut decoder, ps_displacement_cap)?;
        let ps_stress = read_f64_slab(&mut decoder, ps_stress_cap)?;

        Ok(BucklingResultCache {
            eigenvalues,
            mode_shapes,
            base_node_positions,
            converged: header.converged,
            iterations: header.iterations,
            ps_displacement,
            ps_stress,
            ps_max_von_mises: f64::from_bits(header.ps_max_von_mises_bits),
            ps_converged: header.ps_converged,
            ps_iterations: header.ps_iterations,
            ps_grid_bounds_min: [
                f64::from_bits(header.ps_grid_bounds_min_x_bits),
                f64::from_bits(header.ps_grid_bounds_min_y_bits),
                f64::from_bits(header.ps_grid_bounds_min_z_bits),
            ],
            ps_grid_bounds_max: [
                f64::from_bits(header.ps_grid_bounds_max_x_bits),
                f64::from_bits(header.ps_grid_bounds_max_y_bits),
                f64::from_bits(header.ps_grid_bounds_max_z_bits),
            ],
            ps_grid_counts: [
                header.ps_grid_count_x,
                header.ps_grid_count_y,
                header.ps_grid_count_z,
            ],
            solve_time_ms: header.solve_time_ms,
        })
    }

    fn uncompressed_byte_size(&self) -> u64 {
        // bincode 1.3 fixint-LE serialized size of BucklingResultHeader (130 bytes).
        let mode_shape_stride = if self.eigenvalues.is_empty() {
            self.base_node_positions.len() as u64
        } else {
            (self.mode_shapes.len() / self.eigenvalues.len().max(1)) as u64
        };
        let header = BucklingResultHeader {
            n_modes: self.eigenvalues.len() as u64,
            mode_shape_stride,
            ps_displacement_len: self.ps_displacement.len() as u64,
            ps_stress_len: self.ps_stress.len() as u64,
            converged: self.converged,
            iterations: self.iterations,
            ps_max_von_mises_bits: self.ps_max_von_mises.to_bits(),
            ps_converged: self.ps_converged,
            ps_iterations: self.ps_iterations,
            ps_grid_bounds_min_x_bits: self.ps_grid_bounds_min[0].to_bits(),
            ps_grid_bounds_min_y_bits: self.ps_grid_bounds_min[1].to_bits(),
            ps_grid_bounds_min_z_bits: self.ps_grid_bounds_min[2].to_bits(),
            ps_grid_bounds_max_x_bits: self.ps_grid_bounds_max[0].to_bits(),
            ps_grid_bounds_max_y_bits: self.ps_grid_bounds_max[1].to_bits(),
            ps_grid_bounds_max_z_bits: self.ps_grid_bounds_max[2].to_bits(),
            ps_grid_count_x: self.ps_grid_counts[0],
            ps_grid_count_y: self.ps_grid_counts[1],
            ps_grid_count_z: self.ps_grid_counts[2],
            solve_time_ms: self.solve_time_ms,
        };
        let header_bytes = bincode::serialized_size(&header).expect(
            "BucklingResultHeader has only fixed-size fields; \
             bincode::serialized_size cannot fail.",
        );
        let slab_bytes = 8
            * (self.eigenvalues.len() as u64
                + self.mode_shapes.len() as u64
                + self.base_node_positions.len() as u64
                + self.ps_displacement.len() as u64
                + self.ps_stress.len() as u64);
        header_bytes + slab_bytes
    }

    fn solve_time_ms(&self) -> u64 {
        self.solve_time_ms
    }
}

/// Convert a 32-character ASCII `&str` cache key component into a fixed
/// `[u8; 32]` byte array for storage in [`CacheEntryHeader`] echo fields.
///
/// # Contract
///
/// Validates **length only** (exactly 32 bytes), not hex-ness — the caller
/// may pass any 32-character ASCII string and this function will accept it.
/// The name reflects the actual contract: the echo fields hold arbitrary
/// length-32 ASCII cache key slices, not decoded hex values. The hex format
/// is a convention at the call site, not a constraint enforced here.
///
/// Used by both [`write_entry`] (to populate the header echoes) and
/// [`read_entry`] (to compute the expected echoes for verification against the
/// on-disk header). A single helper keeps one source of truth for the
/// conversion and ensures both sites produce the same `InvalidInput` error
/// if a non-32-char string is accidentally passed.
///
/// The `debug_assert!` in [`shard_dir`] only guards `len >= 2`; this function
/// enforces the stricter `len == 32` requirement that the echo fields demand.
fn cache_key_to_ascii_32(s: &str) -> io::Result<[u8; 32]> {
    s.as_bytes().try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cache key must be exactly 32 ASCII chars, got {} chars: {:?}",
                s.len(),
                s
            ),
        )
    })
}

/// Write a cache entry to disk atomically using a temp-file + rename approach.
///
/// The body is pre-buffered into a `Vec<u8>` before the header is written so
/// that `CacheEntryHeader::byte_size` is known without seeking. After
/// `sync_all` for durability, `tempfile::NamedTempFile::persist` performs a
/// POSIX `rename(2)` — atomic and last-writer-wins under concurrent writers.
/// The shard directory is fsynced after the rename to ensure the directory
/// entry (dirent) is durable on the same flush cycle as the file data.
/// The sidecar `.meta` file is written AFTER the rename so a failed rename
/// never leaves an orphan sidecar pointing at a non-existent `.bin`.
///
/// # Concurrency
///
/// Per PRD `docs/prds/v0_3/persistent-fea-cache.md` §"Concurrency": two
/// concurrent callers writing the same `(engine_version_hash, input_hash)` key
/// both succeed without any lock. The `tempfile::Builder::new().prefix(".tmp.")`
/// placement in the same shard directory satisfies the POSIX requirement for
/// `rename(2)` atomicity (same filesystem). The `.tmp.` prefix also matches the
/// PRD's convention for orphan identification, so the future startup-sweep task
/// can glob `**/.tmp.*` to find and remove any temp files left behind by a
/// writer killed between creation and rename. `sync_all()` before `persist()`
/// makes the file *content* crash-durable; the subsequent directory `sync_all()`
/// makes the directory *entry* crash-durable. A crash between `persist()` and
/// the sidecar write leaves the `.bin` consistent (last atomic rename wins) but
/// the sidecar absent; the next `read_entry` tolerates an absent sidecar
/// gracefully via sidecar recreation on hit.
///
/// Pinned by test
/// `concurrent_write_entry_calls_for_same_input_both_succeed_and_final_read_entry_decodes_to_original_value`.
///
/// # Errors
///
/// Propagates `io::Error` for unexpected I/O failures (directory creation,
/// file creation, write errors, sync, rename). The caller is responsible for
/// any higher-level retry or eviction logic.
pub fn write_entry<V: PersistentlyCacheable>(
    cache_root: &Path,
    engine_version_hash: &str,
    input_hash: &str,
    value: &V,
) -> io::Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Validate cache-key lengths BEFORE any filesystem side effects. `shard_dir`
    // slices `&input_hash[..2]` and would panic on a string shorter than 2 ASCII
    // bytes (or with a multi-byte UTF-8 char straddling that boundary); even for
    // the common "31 chars" case, doing FS work before validation would leave an
    // orphan shard dir and tempfile behind on `InvalidInput`.
    let engine_bytes: [u8; 32] = cache_key_to_ascii_32(engine_version_hash)?;
    let input_bytes: [u8; 32] = cache_key_to_ascii_32(input_hash)?;

    let sd = shard_dir(cache_root, engine_version_hash, input_hash);
    std::fs::create_dir_all(&sd)?;

    // Retry once if tempfile_in returns NotFound — a concurrent evict_over_cap
    // call may have pruned the now-empty shard dir in the window between
    // create_dir_all (above) and tempfile_in here.  One re-create covers the
    // race; unbounded retries are unnecessary because the freshly re-created dir
    // is empty and cannot be pruned again until another entry is written into it
    // and then fully evicted.
    let mut temp = match tempfile::Builder::new().prefix(".tmp.").tempfile_in(&sd) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&sd)?;
            tempfile::Builder::new().prefix(".tmp.").tempfile_in(&sd)?
        }
        Err(e) => return Err(e),
    };

    let written_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(-1);

    let header = CacheEntryHeader {
        format_version: ENTRY_FORMAT_VERSION,
        engine_version_hash: engine_bytes,
        input_hash: input_bytes,
        solve_time_ms: value.solve_time_ms(),
        // byte_size is the UNCOMPRESSED body byte count (per CacheEntryHeader
        // doc, lines 195-196 / 210-211), supplied by uncompressed_byte_size()
        // — NOT the compressed length, which would make the field redundant
        // with `file_size - ENTRY_HEADER_ENCODED_LEN`.
        byte_size: value.uncompressed_byte_size(),
        written_at,
    };

    // Write the header then stream the compressed body directly into the
    // tempfile. No intermediate Vec<u8> buffer is needed: byte_size is known
    // from value.uncompressed_byte_size() before serialization, so there is
    // no chicken-and-egg ordering constraint. Avoiding the buffer saves a
    // peak transient allocation of tens of MiB for large FEA results
    // (displacement+stress up to 128 MiB uncompressed).
    header.write_to(&mut temp)?;
    value.serialize_to_writer(&mut temp)?;
    temp.as_file().sync_all()?;

    let bin_path = entry_bin_path(cache_root, engine_version_hash, input_hash);
    temp.persist(&bin_path).map_err(|e| e.error)?;

    // fsync the shard directory so the directory entry pointing at the newly
    // renamed `.bin` is durable on the same flush cycle as the file data.
    // Without this, a kernel crash between persist() and the next filesystem
    // sync could lose the dirent even though the `.bin` data is on disk
    // (the file content is durable from sync_all() above, but the directory
    // inode update from rename(2) may still be in the page cache).
    // Impact is bounded — a missing entry is a cache miss — but GC eviction
    // policy depends on the sidecar being co-located with the .bin, so
    // entries that vanish post-crash without their sidecar being cleaned up
    // could linger as orphan sidecars. The directory fsync avoids this.
    std::fs::File::open(&sd)?.sync_all()?;

    // Write the sidecar AFTER the atomic rename (and after the dir fsync) so
    // a failed rename or crash never leaves an orphan sidecar pointing at a
    // non-existent .bin. write_sidecar (not touch_sidecar) is used here
    // because the .meta may not exist yet on the first write of a fresh entry
    // — write_sidecar creates-or-overwrites, covering both cases.
    write_sidecar(&entry_meta_path(
        cache_root,
        engine_version_hash,
        input_hash,
    ))?;

    Ok(())
}

/// Read a cache entry from disk.
///
/// Returns `Ok(None)` on cache miss (file absent) or on a corrupt/stale
/// entry (format-version mismatch, echo mismatch, body decode error — all
/// treated as miss per PRD corruption-recovery policy). Returns
/// `Ok(Some(value))` on a successful hit. Propagates `Err` only for genuine
/// I/O infrastructure problems (e.g. EACCES on the `.bin` file) where the
/// caller must be informed.
pub fn read_entry<V: PersistentlyCacheable>(
    cache_root: &Path,
    engine_version_hash: &str,
    input_hash: &str,
) -> io::Result<Option<V>> {
    use std::fs::File;
    use std::io::BufReader;

    // Validate cache-key lengths BEFORE constructing the bin path. `entry_bin_path`
    // → `shard_dir` slices `&input_hash[..2]` and would panic on a short or
    // non-ASCII-boundary input. Computing the expected echo bytes up front also
    // means we never advance to file open with an unusable key.
    let expected_engine = cache_key_to_ascii_32(engine_version_hash)?;
    let expected_input = cache_key_to_ascii_32(input_hash)?;

    let bin_path = entry_bin_path(cache_root, engine_version_hash, input_hash);
    // NotFound is the cache-miss signal per PRD; any other Err is an
    // infrastructure problem the caller must know about.
    let f = match File::open(&bin_path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    // BufReader amortizes the many small read(2) syscalls that bincode's
    // fixed-int decoder would otherwise issue for the 92-byte header fields.
    // zstd::Decoder downstream does its own internal buffering, so this
    // BufReader primarily benefits the header decode path on every cache hit.
    let mut f = BufReader::new(f);
    // Truncated or otherwise corrupt headers (file shorter than the encoded
    // length, or bincode decode failure) are treated as a cache miss per PRD
    // corruption-recovery policy — same as the body-decode branch below. Only
    // genuine infrastructure errors (e.g. EACCES on a read mid-header) propagate.
    let header = match CacheEntryHeader::read_from(&mut f) {
        Ok(h) => h,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData
            ) =>
        {
            tracing::warn!(
                ?e,
                cache_root = %cache_root.display(),
                engine_version_hash,
                input_hash,
                "cache entry rejected: header read failed (treating as miss)"
            );
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    // Check format_version BEFORE body decode — a stale-format entry must never
    // advance to the more expensive decompression path.
    if let Err(e) = header.verify_format_version() {
        tracing::warn!(
            ?e,
            cache_root = %cache_root.display(),
            engine_version_hash,
            input_hash,
            "cache entry rejected: format_version mismatch (treating as miss)"
        );
        return Ok(None);
    }

    // Verify that the header's echo fields match the key components from the
    // path. A mismatch indicates corruption (misplaced or bit-flipped .bin).
    if let Err(e) = header.verify_field_echoes(&expected_engine, &expected_input) {
        tracing::warn!(
            ?e,
            cache_root = %cache_root.display(),
            engine_version_hash,
            input_hash,
            "cache entry rejected: header echo mismatch (treating as miss)"
        );
        return Ok(None);
    }

    // Body-decode errors (bad zstd frame, truncated slab, bincode schema drift)
    // are treated as cache miss per PRD corruption-recovery policy. Only genuine
    // I/O infrastructure errors before the header read (e.g. EACCES on file open)
    // surface as Err — those are not corruption, they are infrastructure problems.
    let value = match V::deserialize_from_reader(&mut f) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                ?e,
                cache_root = %cache_root.display(),
                engine_version_hash,
                input_hash,
                "cache entry rejected: body decode failed (treating as miss)"
            );
            return Ok(None);
        }
    };

    // Update the sidecar mtime as the LRU last-access signal.
    //
    // The sidecar may be absent if write_entry was killed (or errored) between
    // persist() and write_sidecar(), leaving the .bin on disk with no companion
    // .meta. In that case we recreate the sidecar via write_sidecar so the
    // entry gets a proper LRU signal and is visible to GC cost-weighted
    // eviction. Without recreation, the orphan .bin would have no LRU signal
    // and GC would be unable to evict it under the cost-weighted policy.
    //
    // Branch on a metadata probe (TOCTOU-tolerant: both paths are safe):
    //   - .meta exists  → touch_sidecar (update mtime only, preserve magic byte)
    //   - .meta absent  → write_sidecar (create-or-overwrite with magic byte)
    //
    // Any other error (e.g. EACCES on a read-only mount) is logged at debug
    // level only — the cache hit is valid regardless of whether the LRU signal
    // update succeeded.
    let meta_path = entry_meta_path(cache_root, engine_version_hash, input_hash);
    let sidecar_result = match std::fs::metadata(&meta_path) {
        Ok(_) => touch_sidecar(&meta_path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Recreate the missing sidecar so this .bin gets an LRU signal.
            write_sidecar(&meta_path)
        }
        Err(e) => Err(e),
    };
    if let Err(e) = sidecar_result {
        tracing::debug!(
            ?e,
            cache_root = %cache_root.display(),
            engine_version_hash,
            input_hash,
            "sidecar update failed on cache hit; LRU signal will be stale"
        );
    }

    Ok(Some(value))
}

// ── Eviction primitive ────────────────────────────────────────────────────────

/// Result returned by [`evict_over_cap`] describing what was removed.
///
/// All three fields together give the caller — and `reify cache stats` —
/// everything needed to understand the post-eviction state:
/// * `evicted_count` + `evicted_bytes` describe what was removed.
/// * `remaining_bytes` describes the post-eviction footprint (callers can
///   assert the cap was actually met by checking `remaining_bytes <= cap_bytes`).
///
/// Excluded intentionally: per-entry detail (would couple the report shape to
/// the internal candidate format), time taken (callers can measure externally),
/// and the cap echo (the caller supplied it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvictionReport {
    /// Number of `.bin` entries removed.
    pub evicted_count: u64,
    /// Total bytes removed (sum of evicted `.bin` file sizes).
    pub evicted_bytes: u64,
    /// On-disk `.bin` byte total that remains after eviction.
    pub remaining_bytes: u64,
}

/// Evict entries from `cache_root/<engine_version_hash>` until the total
/// `.bin` footprint is at or below `cap_bytes`.
///
/// # Scope
///
/// Only the subdir for `engine_version_hash` is walked. Cross-version GC
/// is a separate startup-sweep task per PRD `docs/prds/v0_3/persistent-fea-cache.md`
/// §"GC policy".
///
/// # Algorithm
///
/// 1. Walk every `.bin` file in `<cache_root>/<engine_version_hash>/**/*.bin`.
///    `.tmp.*` in-flight writes and `.meta` sidecars are skipped.
/// 2. Read [`CacheEntryHeader`] (92 bytes, no body decompression) for `solve_time_ms`.
/// 3. Last-access signal = `.meta` sidecar mtime via [`read_sidecar_mtime`];
///    if the sidecar is absent (crash-orphan `.bin`), falls back to `.bin` mtime.
/// 4. Sort candidates by [`eviction_score`] **descending** (highest score = first to evict).
/// 5. Remove `.bin` + `.meta` pairs in score order until `remaining ≤ cap_bytes`.
///
/// # Errors
///
/// Returns `Ok(EvictionReport::default())` when the engine-version subdir does
/// not exist (no entries → nothing to evict). Other `io::Error` kinds propagate.
///
/// # Observability
///
/// Emits one `tracing::info!` event at the `reify_eval::persistent_cache::gc`
/// target (message: `"evict_over_cap complete"`) **only** on the happy-path
/// return — i.e., after the eviction loop completes without error.  Early-return
/// paths (engine-version subdir absent, total already ≤ cap) stay silent.
/// Any `Err` propagated from within the eviction loop exits without emitting the
/// INFO summary; callers that want GC diagnostics on error paths must inspect the
/// returned `io::Error` value directly.
pub fn evict_over_cap(
    cache_root: &Path,
    engine_version_hash: &str,
    cap_bytes: u64,
) -> io::Result<EvictionReport> {
    let subdir = cache_root.join(engine_version_hash);
    match subdir.metadata() {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(EvictionReport::default());
        }
        Err(e) => return Err(e),
    }

    // Walk engine-version subdir: shard_dirs are the two-char subdirs under subdir.
    // Collect all .bin candidates (skip .tmp.* in-flight writes and .meta sidecars).
    let mut candidates: Vec<EvictionCandidate> = Vec::new();
    let mut total_bytes: u64 = 0;

    for shard_entry in std::fs::read_dir(&subdir)? {
        let shard_entry = shard_entry?;
        let shard_path = shard_entry.path();
        if !shard_path.is_dir() {
            // Non-directory entries directly under the engine-version subdir
            // (e.g., a stale debug-touch file or a partial cross-version migration
            // leftover) are silently skipped.  A future `reify cache fsck` pass
            // will surface these; no action needed here.
            continue;
        }
        for file_entry in std::fs::read_dir(&shard_path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();
            // Only include .bin files; skip .tmp.*, .meta, and anything else.
            match file_path.extension().and_then(|e| e.to_str()) {
                Some("bin") => {}
                _ => continue,
            }
            // Also skip .tmp.* prefixed files (in-flight tempfile writes).
            if file_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(".tmp."))
                .unwrap_or(false)
            {
                continue;
            }
            // Derive .meta path: same stem, .meta extension.
            let meta_path = file_path.with_extension("meta");

            // Read last-access from sidecar mtime.  When the sidecar is absent
            // (crash-orphan .bin: `write_entry` persists `.bin` then calls
            // `write_sidecar`; a crash between those two steps leaves an orphan
            // `.bin` with no `.meta`), fall back to the `.bin` file's own mtime.
            // `.bin` mtime is set at write time and is a safe substitute until
            // the sidecar is recreated by the next `read_entry` hit.
            // Any other I/O error propagates.
            //
            // Race site #1 — concurrent eviction: another reify process may
            // have removed this .bin between read_dir and here (see the
            // `we_removed_bin` match in the remove loop below for the same
            // race on the eviction side).  On NotFound from metadata(),
            // skip to the next file_entry rather than propagating the
            // transient error.
            let last_access = match read_sidecar_mtime(&meta_path) {
                Ok(t) => t,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // Sidecar absent — fall back to .bin mtime.  If .bin is
                    // also gone (concurrent eviction hit after read_dir),
                    // continue to the next file_entry.
                    match std::fs::metadata(&file_path) {
                        Ok(m) => m.modified()?,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => return Err(e),
            };

            // Open .bin once: read both the file size and the 92-byte fixed-length
            // header from the same file descriptor, saving one stat(2) syscall per
            // candidate compared to `file_entry.metadata()` + a second `File::open`.
            //
            // On a corrupt or truncated .bin (header decode fails), assign
            // `solve_time_ms = 0` so the entry scores maximally high and self-heals
            // on the next GC run — mirroring `read_entry` which returns `Ok(None)`
            // on header mismatch rather than propagating the error.
            //
            // Race site #2/3 — concurrent eviction: File::open or f.metadata()
            // returns NotFound when another process deletes the .bin after
            // read_dir listed it but before we open it (race site #2), or in
            // the narrow window between open(2) and fstat(2) (race site #3).
            // Both cases: continue to the next file_entry.  Mirrors race site
            // #1 above and the `we_removed_bin` NotFound suppression in the
            // eviction loop below.
            use std::fs::File;
            use std::io::BufReader;
            let f = match File::open(&file_path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue, // race site #2
                Err(e) => return Err(e),
            };
            let bin_size = match f.metadata() {
                Ok(m) => m.len(),
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue, // race site #3
                Err(e) => return Err(e),
            };
            let solve_time_ms = match CacheEntryHeader::read_from(&mut BufReader::new(f)) {
                Ok(hdr) => hdr.solve_time_ms,
                Err(_) => 0, // corrupt/truncated: treat as free-to-evict; self-heals
            };

            total_bytes += bin_size;
            candidates.push(EvictionCandidate {
                bin_path: file_path,
                meta_path,
                bin_size,
                last_access,
                solve_time_ms,
            });
        }
    }

    // Under cap — nothing to evict; report the actual on-disk total.
    if total_bytes <= cap_bytes {
        return Ok(EvictionReport {
            evicted_count: 0,
            evicted_bytes: 0,
            remaining_bytes: total_bytes,
        });
    }

    // Cost-aware sort: highest eviction_score first.
    //
    // `eviction_score(now, last_access, solve_time_ms) = age_secs / max(solve_time_ms, 1)`
    // per PRD `docs/prds/v0_3/persistent-fea-cache.md` §"GC policy". A high score
    // means the entry is cheap to recompute AND has not been accessed recently —
    // evict it before an expensive-but-recently-accessed entry.
    //
    // `f64::partial_cmp` covers all finite values; NaN is impossible here (age_secs
    // is non-negative, denominator is ≥ 1), but `unwrap_or(Equal)` is used for
    // soundness rather than panicking on a hypothetical NaN.
    let now = std::time::SystemTime::now();
    candidates.sort_by(|a, b| {
        let sa = eviction_score(now, a.last_access, a.solve_time_ms);
        let sb = eviction_score(now, b.last_access, b.solve_time_ms);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Evict candidates in order until remaining ≤ cap_bytes.
    let mut evicted_count: u64 = 0;
    let mut evicted_bytes: u64 = 0;
    let mut remaining: u64 = total_bytes;

    for candidate in &candidates {
        if remaining <= cap_bytes {
            break;
        }
        // Remove .bin; suppress NotFound (concurrent eviction race: another
        // `reify` process may have removed this entry between the candidate-walk
        // above and now).  Regardless of who removed the file the bytes are gone
        // from disk — `remaining` is always decremented.  Only `evicted_count`
        // and `evicted_bytes` credit work done by THIS invocation.
        let we_removed_bin = match std::fs::remove_file(&candidate.bin_path) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e),
        };
        // Remove .meta — suppress NotFound only (crash-orphan .bin never had one,
        // or a concurrent eviction already cleaned it up).
        // All other errors propagate via `?`.
        match std::fs::remove_file(&candidate.meta_path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if we_removed_bin {
            evicted_count += 1;
            evicted_bytes += candidate.bin_size;
            let age_secs = now
                .duration_since(candidate.last_access)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // Hot-path: fires once per evicted entry inside the eviction loop.
            // With `RUST_LOG=reify_eval::persistent_cache::gc=debug` enabled on
            // a large cache, bin_path formatting + field encoding will dominate
            // loop cost.  Keep at DEBUG; do NOT elevate to INFO.
            tracing::debug!(
                target: "reify_eval::persistent_cache::gc",
                bin_path = %candidate.bin_path.display(),
                bin_size = candidate.bin_size,
                solve_time_ms = candidate.solve_time_ms,
                age_secs,
                "evicting cache entry"
            );
        }
        remaining -= candidate.bin_size;

        // Best-effort shard-dir housekeeping: attempt to prune the two-char
        // shard dir once this candidate's files are gone.  Three cases:
        //   `Ok(())`             — shard is now empty and was removed.
        //   `DirectoryNotEmpty`  — other entries in this shard survive this
        //                          eviction run; will be pruned on a future
        //                          call that drains the shard.
        //   `NotFound`           — a concurrent reify process already pruned it.
        //   `Err(_)` catch-all   — PermissionDenied and other unexpected kinds
        //                          are silently swallowed so that housekeeping
        //                          never aborts an otherwise-successful eviction.
        // Intentionally does NOT attempt to remove the engine-version subdir —
        // that is owned by the startup-sweep task (cross-version orphan pruning).
        if let Some(parent) = candidate.bin_path.parent() {
            match std::fs::remove_dir(parent) {
                Ok(()) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(e) => {
                    // Unexpected error kind (e.g. PermissionDenied, Interrupted,
                    // ReadOnlyFilesystem from a FUSE backend).  Best-effort:
                    // shard-dir housekeeping must never abort an otherwise-successful
                    // eviction run.  Log at debug so the error is observable in
                    // diagnostics without surfacing to the caller.
                    tracing::debug!(
                        ?e,
                        shard_dir = %parent.display(),
                        "evict_over_cap: unexpected error pruning shard dir (suppressed; best-effort)"
                    );
                }
            }
        }
    }

    tracing::info!(
        target: "reify_eval::persistent_cache::gc",
        evicted_count,
        evicted_bytes,
        remaining_bytes = remaining,
        cap_bytes,
        engine_version_hash = %engine_version_hash,
        "evict_over_cap complete"
    );

    Ok(EvictionReport {
        evicted_count,
        evicted_bytes,
        remaining_bytes: remaining,
    })
}

/// Internal candidate record used by the eviction loop.
struct EvictionCandidate {
    bin_path: PathBuf,
    meta_path: PathBuf,
    bin_size: u64,
    last_access: std::time::SystemTime,
    solve_time_ms: u64,
}

/// Compute the cost-weighted LRU eviction score for a cache entry.
///
/// Formula per PRD `docs/prds/v0_3/persistent-fea-cache.md` §"GC policy":
///
/// ```text
/// score = age_secs / max(solve_time_ms, 1)
/// ```
///
/// A **higher** score means the entry should be evicted **first** — it is
/// old (large numerator) and cheap to re-compute (small denominator).
///
/// # Arguments
///
/// * `now` — wall-clock instant used as the reference for age calculation.
///   Pass `SystemTime::now()` once per eviction run and reuse it for all
///   candidates to produce a stable total ordering.
/// * `last_access` — mtime of the `.meta` sidecar file, or `.bin` mtime as
///   a fallback (see [`evict_over_cap`]).
/// * `solve_time_ms` — solver wall-time from [`CacheEntryHeader::solve_time_ms`].
///   The `max(_, 1)` clamp prevents division-by-zero for sub-millisecond
///   solves (those entries are essentially free to recompute and score very
///   high, which is correct behaviour — evict them first).
///
/// # Clock-skew safety
///
/// If `last_access` is in the future relative to `now`
/// (`SystemTime::duration_since` returns `Err`), the age is treated as `0.0`
/// seconds — the entry is considered just-touched and scores low. This is the
/// conservative choice for clock-skew scenarios.
pub fn eviction_score(
    now: std::time::SystemTime,
    last_access: std::time::SystemTime,
    solve_time_ms: u64,
) -> f64 {
    let age_secs = now
        .duration_since(last_access)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    age_secs / solve_time_ms.max(1) as f64
}

// ── Startup-sweep public API ─────────────────────────────────────────────────

/// Minimum mtime age for a `.tmp.*` file to be considered a crashed-writer
/// leftover and removed by [`sweep_stale_tempfiles`].
///
/// 1 hour gives any in-flight `write_entry` call plenty of time to finish
/// before its tempfile is collected.
pub const STALE_TEMPFILE_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

/// Minimum mtime age for a non-current engine-version subdirectory to be
/// pruned by [`prune_orphan_engine_version_dirs`].
///
/// 30 days ensures an older build's cache is not discarded during rapid
/// iteration, while still reclaiming disk space over time.
pub const ORPHAN_DIR_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Outcome of a startup-sweep operation.
///
/// Mirrors [`EvictionReport`] in shape: a plain `Copy` value returned from
/// each sweep function so callers can log or assert on the results without
/// coupling to internal bookkeeping types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Number of stale `.tmp.*` files removed by [`sweep_stale_tempfiles`].
    pub tempfiles_removed: u64,
    /// Number of orphan engine-version subdirectories removed by
    /// [`prune_orphan_engine_version_dirs`].
    pub orphan_dirs_removed: u64,
}

/// Delete stale `.tmp.*` crashed-writer leftovers under `cache_root`.
///
/// Recursively walks the entire `cache_root` subtree. Any regular file whose
/// name starts with `.tmp.` and whose mtime is older than [`STALE_TEMPFILE_AGE`]
/// is removed. Files and directories that cannot be read or removed are skipped
/// with a `tracing::debug!` log entry — individual failures never abort the
/// sweep or propagate to the caller.
///
/// Returns a [`SweepReport`] with `tempfiles_removed` set to the number of
/// files actually deleted. `orphan_dirs_removed` is always 0 (this function
/// does not prune directories; see [`prune_orphan_engine_version_dirs`]).
///
/// # Non-blocking
///
/// If `cache_root` does not exist, returns `SweepReport::default()` immediately
/// without any error — the sweep is a no-op on a first run or a clean system.
// G-allow: task #2978 stale-tempfile sweep; called by the sweep_persistent_cache_at_startup engine-admin wrapper
pub fn sweep_stale_tempfiles(cache_root: &Path) -> SweepReport {
    let mut report = SweepReport::default();
    let now = std::time::SystemTime::now();

    // Guard: absent cache_root → silent no-op (mirrors evict_over_cap's
    // NotFound → Ok(default) idiom).
    match std::fs::metadata(cache_root) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return report,
        Err(e) => {
            tracing::debug!(
                "sweep_stale_tempfiles: cannot stat cache_root {:?}: {e}",
                cache_root
            );
            return report;
        }
    }

    sweep_stale_tempfiles_recursive(cache_root, now, &mut report);
    report
}

/// Recursive helper for [`sweep_stale_tempfiles`].
fn sweep_stale_tempfiles_recursive(
    dir: &Path,
    now: std::time::SystemTime,
    report: &mut SweepReport,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!("sweep_stale_tempfiles: cannot read_dir {:?}: {e}", dir);
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(
                    "sweep_stale_tempfiles: cannot read dir entry in {:?}: {e}",
                    dir
                );
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                tracing::debug!(
                    "sweep_stale_tempfiles: cannot get file_type for {:?}: {e}",
                    path
                );
                continue;
            }
        };

        if file_type.is_dir() {
            sweep_stale_tempfiles_recursive(&path, now, report);
            continue;
        }

        // Only act on regular files whose name starts with ".tmp.".
        if !file_type.is_file() {
            continue;
        }
        let is_tmp = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with(".tmp."))
            .unwrap_or(false);
        if !is_tmp {
            continue;
        }

        // Age check: Err from duration_since (mtime in the future / clock
        // skew) → treat as age 0 → NOT stale (conservative).
        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(
                    "sweep_stale_tempfiles: cannot read mtime for {:?}: {e}",
                    path
                );
                continue;
            }
        };
        let age = match now.duration_since(mtime) {
            Ok(d) => d,
            Err(_) => continue, // mtime in the future → keep
        };
        if age <= STALE_TEMPFILE_AGE {
            continue;
        }

        match std::fs::remove_file(&path) {
            Ok(()) => {
                tracing::debug!("sweep_stale_tempfiles: removed stale tempfile {:?}", path);
                report.tempfiles_removed += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Concurrently removed by another process — not an error.
            }
            Err(e) => {
                tracing::debug!("sweep_stale_tempfiles: cannot remove {:?}: {e}", path);
            }
        }
    }
}

/// Prune engine-version subdirectories of `cache_root` that have not been
/// touched in [`ORPHAN_DIR_AGE`] (30 days) and are not the current build's
/// directory.
///
/// Only the immediate subdirectories of `cache_root` are inspected — each
/// represents one engine-version-hash. The subdirectory named
/// `current_engine_version` is **never** removed, even if its mtime is
/// somehow older than the threshold.
///
/// Returns a [`SweepReport`] with `orphan_dirs_removed` set to the number of
/// directories recursively deleted. `tempfiles_removed` is always 0.
///
/// Individual metadata-read or `remove_dir_all` failures are swallowed with a
/// `tracing::debug!` and the loop continues (best-effort). An absent
/// `cache_root` returns `SweepReport::default()`.
// G-allow: task #2978 orphan-engine-version pruning; called by the sweep_persistent_cache_at_startup engine-admin wrapper
pub fn prune_orphan_engine_version_dirs(
    cache_root: &Path,
    current_engine_version: &str,
) -> SweepReport {
    let mut report = SweepReport::default();

    // Safety net: an empty current_engine_version means the caller could not
    // determine the live build's version (e.g. a build-time env var
    // resolution failure that silently fell back to ""). In that case we have
    // no reliable way to identify the live cache subdir and must skip the
    // prune entirely rather than risk deleting it.
    if current_engine_version.is_empty() {
        tracing::warn!(
            "prune_orphan_engine_version_dirs: current_engine_version is empty; \
             skipping prune to avoid destroying live cache subdirs"
        );
        return report;
    }

    let now = std::time::SystemTime::now();

    // Guard: absent cache_root → silent no-op.
    match std::fs::metadata(cache_root) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return report,
        Err(e) => {
            tracing::debug!(
                "prune_orphan_engine_version_dirs: cannot stat cache_root {:?}: {e}",
                cache_root
            );
            return report;
        }
    }

    let entries = match std::fs::read_dir(cache_root) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "prune_orphan_engine_version_dirs: cannot read_dir {:?}: {e}",
                cache_root
            );
            return report;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(
                    "prune_orphan_engine_version_dirs: cannot read entry in {:?}: {e}",
                    cache_root
                );
                continue;
            }
        };

        // Only inspect directories.
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                tracing::debug!(
                    "prune_orphan_engine_version_dirs: cannot get file_type for {:?}: {e}",
                    entry.path()
                );
                continue;
            }
        };
        if !file_type.is_dir() {
            continue;
        }

        // Never prune the current-build subdir — check by exact name BEFORE
        // any age check, as specified in the task design decisions.
        let dir_name = entry.file_name();
        if dir_name.to_str() == Some(current_engine_version) {
            continue;
        }

        // Age check via directory mtime.
        //
        // Note: a directory's mtime advances only when a *direct* child is
        // created, removed, or renamed — not when files deeper in the tree
        // are modified or accessed. For an engine-version subdir, direct
        // children are shard dirs (2-hex prefix dirs), so the mtime
        // essentially freezes once all ~256 possible shards exist. This is
        // intentional: non-current engine-version dirs receive no new writes
        // after a build transition, so a stale mtime reliably signals
        // "this version is no longer in use".
        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(
                    "prune_orphan_engine_version_dirs: cannot read mtime for {:?}: {e}",
                    entry.path()
                );
                continue;
            }
        };
        let age = match now.duration_since(mtime) {
            Ok(d) => d,
            Err(_) => continue, // mtime in the future → keep (conservative)
        };
        if age <= ORPHAN_DIR_AGE {
            continue;
        }

        // Prune this orphan subtree.
        let path = entry.path();
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                tracing::debug!(
                    "prune_orphan_engine_version_dirs: removed orphan dir {:?}",
                    path
                );
                report.orphan_dirs_removed += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Concurrently removed — not an error.
            }
            Err(e) => {
                tracing::debug!(
                    "prune_orphan_engine_version_dirs: cannot remove {:?}: {e}",
                    path
                );
            }
        }
    }

    report
}

/// Perform the full startup-sweep of `cache_root`: stale tempfiles first,
/// then orphan engine-version directories.
///
/// This is the canonical entry point for the two-pass startup cleanup. It
/// composites [`sweep_stale_tempfiles`] and
/// [`prune_orphan_engine_version_dirs`] in a fixed order:
///
/// 1. **Tempfile sweep** — removes `.tmp.*` crashed-writer leftovers.
/// 2. **Orphan-dir prune** — removes engine-version subdirs older than 30 days
///    (except `current_engine_version`).
///
/// The fixed order is documented so callers can reason about the interaction:
/// a `.tmp.*` file inside an orphan dir may be collected by the tempfile pass
/// before the dir is pruned, or the dir prune may subsume it — both outcomes
/// are correct. Each pass is independently idempotent, so the composition is
/// also idempotent.
///
/// **Performance note:** because the tempfile sweep runs first and recurses the
/// entire `cache_root` subtree unconditionally, it descends into orphan
/// engine-version dirs that the subsequent prune pass will remove wholesale.
/// For a long-lived cache with several stale build dirs this is a small amount
/// of wasted stat(2) work. It is accepted for v1 given the startup-sweep
/// framing ("cheap, run synchronously") and the simplicity of keeping the two
/// passes independent. A future optimisation could run prune first and then
/// limit the tempfile walk to surviving subdirs, but that would couple the
/// passes.
///
/// Returns the field-wise sum of the two [`SweepReport`]s. An absent
/// `cache_root` returns `SweepReport::default()`.
pub fn sweep_on_startup(cache_root: &Path, current_engine_version: &str) -> SweepReport {
    let a = sweep_stale_tempfiles(cache_root);
    let b = prune_orphan_engine_version_dirs(cache_root, current_engine_version);
    SweepReport {
        tempfiles_removed: a.tempfiles_removed + b.tempfiles_removed,
        orphan_dirs_removed: a.orphan_dirs_removed + b.orphan_dirs_removed,
    }
}

/// Backdate the mtime of `path` to `age_secs` seconds in the past.
///
/// Works for both regular files (opened write-only) and directories
/// (opened read-only; on Linux `futimens` only requires ownership of the
/// inode, not write access on the file descriptor).
///
/// Exposed `pub(crate)` so test modules across the crate can share this
/// helper without duplicating it.
#[cfg(test)]
pub(crate) fn backdate_mtime(path: &std::path::Path, age_secs: u64) {
    use std::fs::FileTimes;
    use std::time::{Duration, SystemTime};
    let t = SystemTime::now()
        .checked_sub(Duration::from_secs(age_secs))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let times = FileTimes::new().set_modified(t);
    if path.is_dir() {
        // Directories can be opened O_RDONLY; futimens checks ownership not
        // fd write-access on Linux.
        let f = std::fs::File::open(path).unwrap();
        f.set_times(times).unwrap();
    } else {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_times(times).unwrap();
    }
}

/// Forward-date the mtime of `path` to `secs_in_future` seconds in the future.
///
/// Mirror of [`backdate_mtime`]: uses `checked_add` instead of `checked_sub`.
/// Falls back to `SystemTime::now()` on overflow (astronomically unlikely).
///
/// Simulates clock-skew scenarios (NTP correction, live-migration, etc.) where
/// a file's mtime appears to be ahead of the current wall clock. The sweep
/// functions guard `now.duration_since(mtime)` with `Err(_) => continue` to
/// keep such entries rather than treating them as stale.
///
/// Exposed `pub(crate)` so test modules across the crate can share this helper.
#[cfg(test)]
pub(crate) fn forward_mtime(path: &std::path::Path, secs_in_future: u64) {
    use std::fs::FileTimes;
    use std::time::{Duration, SystemTime};
    let t = SystemTime::now()
        .checked_add(Duration::from_secs(secs_in_future))
        .unwrap_or_else(SystemTime::now);
    let times = FileTimes::new().set_modified(t);
    if path.is_dir() {
        // Directories can be opened O_RDONLY; futimens checks ownership not
        // fd write-access on Linux.
        let f = std::fs::File::open(path).unwrap();
        f.set_times(times).unwrap();
    } else {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_times(times).unwrap();
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

    // ── ENGINE_VERSION_HASH const tests ──────────────────────────────────────

    #[test]
    fn engine_version_hash_const_is_thirty_two_lowercase_hex_chars() {
        assert_eq!(
            ENGINE_VERSION_HASH.len(),
            32,
            "ENGINE_VERSION_HASH must be exactly 32 chars, got {:?}",
            ENGINE_VERSION_HASH
        );
        assert!(
            ENGINE_VERSION_HASH
                .chars()
                .all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "ENGINE_VERSION_HASH must be all lowercase hex, got {:?}",
            ENGINE_VERSION_HASH
        );
    }

    #[test]
    fn engine_version_hash_const_is_not_all_zeros() {
        // A regression that wires up an empty contributor list (or fails to read
        // any file) would collapse to the all-zeros sentinel. A real build with at
        // least one non-empty contributor cannot collide on this value.
        assert_ne!(
            ENGINE_VERSION_HASH, "00000000000000000000000000000000",
            "ENGINE_VERSION_HASH must not be the all-zeros sentinel"
        );
    }

    // ── compose_engine_version_hash tests ────────────────────────────────────

    #[test]
    fn compose_engine_version_hash_returns_32char_lowercase_hex() {
        let h = compose_engine_version_hash(&[b"hello", b"world"]);
        assert_eq!(h.len(), 32, "expected 32 hex chars, got {:?}", h);
        assert!(
            h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "expected all lowercase hex chars, got {:?}",
            h
        );
    }

    #[test]
    fn compose_engine_version_hash_is_deterministic_for_same_input() {
        let parts: &[&[u8]] = &[b"reify", b"engine", b"version"];
        let h1 = compose_engine_version_hash(parts);
        let h2 = compose_engine_version_hash(parts);
        assert_eq!(h1, h2);
    }

    /// PRD-required sentinel (docs/prds/v0_3/persistent-fea-cache.md §"Cache
    /// invalidation on engine version"): any single-byte flip in any contributor
    /// must change the hash. DO NOT REMOVE without revisiting the PRD section.
    #[test]
    fn compose_engine_version_hash_flipping_one_contributor_changes_hash_prd_sentinel() {
        let contributors: &[&[u8]] = &[b"alpha", b"beta", b"gamma"];
        let baseline = compose_engine_version_hash(contributors);

        for (ci, contributor) in contributors.iter().enumerate() {
            for bi in 0..contributor.len() {
                let mut perturbed: Vec<Vec<u8>> = contributors.iter().map(|c| c.to_vec()).collect();
                perturbed[ci][bi] ^= 0xFF;
                let perturbed_refs: Vec<&[u8]> = perturbed.iter().map(|v| v.as_slice()).collect();
                let h = compose_engine_version_hash(&perturbed_refs);
                assert_ne!(
                    h, baseline,
                    "hash unchanged after flipping byte {} of contributor {}",
                    bi, ci
                );
            }
        }
    }

    #[test]
    fn compose_engine_version_hash_is_order_sensitive() {
        let a: &[&[u8]] = &[b"first", b"second"];
        let b: &[&[u8]] = &[b"second", b"first"];
        assert_ne!(
            compose_engine_version_hash(a),
            compose_engine_version_hash(b),
            "hash must differ when contributor order changes"
        );
    }

    #[test]
    fn compose_engine_version_hash_length_prefix_prevents_concat_collision() {
        // Without length-prefix framing, [b"ab", b"c"] and [b"a", b"bc"]
        // would concatenate to the same bytes "abc" and hash identically.
        let a: &[&[u8]] = &[b"ab", b"c"];
        let b: &[&[u8]] = &[b"a", b"bc"];
        assert_ne!(
            compose_engine_version_hash(a),
            compose_engine_version_hash(b),
            "length-prefix framing must prevent concat collision"
        );
    }

    #[test]
    fn compose_engine_version_hash_drop_one_contributor_changes_hash() {
        let full: &[&[u8]] = &[b"foo", b"bar", b"baz"];
        let dropped: &[&[u8]] = &[b"foo", b"bar"];
        assert_ne!(
            compose_engine_version_hash(full),
            compose_engine_version_hash(dropped),
            "dropping a contributor must change the hash"
        );
    }

    #[test]
    fn compose_engine_version_hash_pins_fixed_input_to_exact_hex_literal() {
        // Pins the single canonical algorithm in `src/engine_hash_algo.rs`
        // against drift. Because `build.rs` uses that SAME source via
        // `include!()`, any change to the length-prefix scheme, hash
        // primitive, or hex formatting simultaneously breaks this test AND
        // changes the emitted ENGINE_VERSION_HASH — making the drift
        // immediately visible in CI.
        // Update this literal deliberately whenever the algorithm changes.
        let h = compose_engine_version_hash(&[b"reify", b"engine"]);
        assert_eq!(
            h, "30b30882195f8e834bdbd936fa5324e0",
            "algorithm drift detected — update this literal in the same commit"
        );
    }

    // ── walk_contributor tests ────────────────────────────────────────────────
    //
    // These tests verify `crate::engine_hash_algo::walk_contributor`, which is
    // the SINGLE source of truth for both the library (via engine_hash_algo.rs)
    // and build.rs (via `include!()` of the same file). Tests fail to compile
    // until step-6 creates `src/engine_hash_algo.rs` and declares
    // `pub(crate) mod engine_hash_algo;` in `lib.rs`.

    #[test]
    fn walk_contributor_for_a_single_file_root_emits_rerun_path_for_the_file_and_two_framed_parts()
    {
        use std::io::Write as _;

        let mut tmpfile = tempfile::NamedTempFile::new().expect("must create tempfile");
        tmpfile.write_all(b"hello").expect("must write to tempfile");
        tmpfile.flush().expect("must flush tempfile");
        let path = tmpfile.path().to_path_buf();

        let walk = crate::engine_hash_algo::walk_contributor("label", &path);

        assert_eq!(
            walk.rerun_paths,
            vec![path.clone()],
            "single-file root: rerun_paths must be [the file itself]"
        );
        assert_eq!(
            walk.parts,
            vec![b"label".to_vec(), b"hello".to_vec()],
            "single-file root: parts must be [label_bytes, file_bytes]"
        );
    }

    /// PRIMARY regression guard for review issue #1: the build script must emit
    /// a `cargo:rerun-if-changed` directive for the directory itself — not just
    /// the files inside it — so that adding a brand-new file to a contributor
    /// directory triggers a rebuild and includes the new file's bytes in
    /// ENGINE_VERSION_HASH. Without the directory-level directive, cargo only
    /// re-runs when an already-listed file changes; a new file is silently
    /// excluded from the hash. DO NOT REMOVE this test without understanding
    /// that consequence.
    #[test]
    fn walk_contributor_for_a_directory_root_emits_rerun_path_for_the_directory_itself_so_added_files_trigger_rebuild()
     {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let dir_path = tmpdir.path().to_path_buf();
        let file_path = dir_path.join("foo.rs");
        std::fs::write(&file_path, b"// content").expect("must write file");

        let walk = crate::engine_hash_algo::walk_contributor("root", &dir_path);

        assert!(
            walk.rerun_paths.contains(&dir_path),
            "directory walk must emit rerun_path for the directory itself (issue #1 fix): \
             adding a new file to a contributor dir must trigger a rebuild; \
             got: {:?}",
            walk.rerun_paths
        );
        assert!(
            walk.rerun_paths.contains(&file_path),
            "directory walk must emit rerun_path for each file inside; got: {:?}",
            walk.rerun_paths
        );
    }

    #[test]
    fn walk_contributor_for_nested_subdirectories_emits_rerun_paths_for_every_intermediate_directory()
     {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let root = tmpdir.path().to_path_buf();
        let a = root.join("a");
        let b = a.join("b");
        std::fs::create_dir_all(&b).expect("must create nested dirs");
        let file = b.join("c.rs");
        std::fs::write(&file, b"// content").expect("must write file");

        let walk = crate::engine_hash_algo::walk_contributor("root", &root);

        assert!(
            walk.rerun_paths.contains(&root),
            "must emit rerun_path for the root dir; got: {:?}",
            walk.rerun_paths
        );
        assert!(
            walk.rerun_paths.contains(&a),
            "must emit rerun_path for intermediate dir 'a'; got: {:?}",
            walk.rerun_paths
        );
        assert!(
            walk.rerun_paths.contains(&b),
            "must emit rerun_path for intermediate dir 'a/b'; got: {:?}",
            walk.rerun_paths
        );
        assert!(
            walk.rerun_paths.contains(&file),
            "must emit rerun_path for 'a/b/c.rs'; got: {:?}",
            walk.rerun_paths
        );
    }

    #[test]
    fn walk_contributor_sorts_directory_entries_by_name_for_byte_determinism_across_platforms() {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let root = tmpdir.path().to_path_buf();
        // Write files out of alphabetical order to expose sort regressions.
        std::fs::write(root.join("b.rs"), b"// b").expect("must write b.rs");
        std::fs::write(root.join("a.rs"), b"// a").expect("must write a.rs");
        std::fs::write(root.join("c.rs"), b"// c").expect("must write c.rs");

        let walk = crate::engine_hash_algo::walk_contributor("root", &root);
        let walk_refs: Vec<&[u8]> = walk.parts.iter().map(|v| v.as_slice()).collect();
        let hash_from_walk = compose_engine_version_hash(&walk_refs);

        // Manually build the expected parts in alphabetical order.
        // If `walk_contributor` does NOT sort, its parts will arrive in a
        // different order and the hashes will diverge — catching a sort
        // regression on any platform regardless of filesystem iteration order.
        // Driving through the public hash surface avoids coupling to the private
        // `parts` Vec layout (e.g. if a directory-marker entry were ever
        // interleaved, step_by(2) would silently extract the wrong elements).
        let expected_parts: &[&[u8]] = &[
            b"root/a.rs",
            b"// a",
            b"root/b.rs",
            b"// b",
            b"root/c.rs",
            b"// c",
        ];
        let expected_hash = compose_engine_version_hash(expected_parts);

        assert_eq!(
            hash_from_walk, expected_hash,
            "walk_contributor must visit directory entries in sorted (alphabetical) \
             order for byte-determinism across platforms; hash mismatch indicates a \
             sort regression"
        );
    }

    /// Equivalence proof that directly addresses review issue #2: since build.rs
    /// uses the EXACT same source for both `walk_contributor` and
    /// `compose_engine_version_hash` via `include!()`, any algorithm drift in
    /// either function breaks this test. The pinned hex literal (filled in during
    /// step-6 GREEN) provides a standalone algorithm-drift sentinel.
    #[test]
    fn walk_contributor_drives_compose_engine_version_hash_end_to_end_for_a_synthetic_two_file_contributor_set()
     {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let root = tmpdir.path().to_path_buf();
        // Files created in reverse alphabetical order to confirm sorting:
        std::fs::write(root.join("beta.rs"), b"// beta content").expect("must write beta.rs");
        std::fs::write(root.join("alpha.rs"), b"// alpha content").expect("must write alpha.rs");

        let walk = crate::engine_hash_algo::walk_contributor("mydir", &root);
        let walk_refs: Vec<&[u8]> = walk.parts.iter().map(|v| v.as_slice()).collect();
        let hash_from_walk = compose_engine_version_hash(&walk_refs);

        // Manually construct the expected parts in sorted order to prove
        // `walk_contributor` matches the hand-crafted list.
        // For directory walk, path_bytes = "{label}/{relative_path}".
        let expected_parts: &[&[u8]] = &[
            b"mydir/alpha.rs",
            b"// alpha content",
            b"mydir/beta.rs",
            b"// beta content",
        ];
        let hash_from_manual = compose_engine_version_hash(expected_parts);

        assert_eq!(
            hash_from_walk, hash_from_manual,
            "walk_contributor output must match manually constructed parts \
             when fed through compose_engine_version_hash"
        );

        // Pinned hex literal — captured during step-6 GREEN. Any change to the
        // length-prefix scheme, hash primitive, path format, or sort order must
        // update this literal deliberately in the same commit.
        assert_eq!(
            hash_from_manual, "a2cfd904bb7edc68837b0069bafa3469",
            "algorithm drift sentinel — update this literal when the \
             length-prefix scheme, hash primitive, or path format changes"
        );
    }

    /// Regression guard for editor-debris leaking into the cache key.
    ///
    /// If developer A's editor leaves `.swp`, `.orig`, `.DS_Store`, etc. in a
    /// contributor directory, those bytes must NOT perturb `ENGINE_VERSION_HASH`.
    /// Otherwise developer B (or CI) building the same git SHA would observe a
    /// different hash → spurious cache miss + spurious `cargo:rerun-if-changed`
    /// triggers.
    ///
    /// Debris files tested (one entry per `is_editor_debris` branch):
    ///   `.foo.swp`       — vim swap (hidden, extension .swp)
    ///   `bar.swo`        — vim swap alt (extension .swo)
    ///   `baz.swn`        — vim swap variant (extension .swn)
    ///   `qux.orig`       — merge-conflict residue (extension .orig)
    ///   `quux.bk`        — backup (extension .bk)
    ///   `.DS_Store`      — macOS metadata (exact name, case-insensitive)
    ///   `thumbs.db`      — Windows thumbnail cache (exact name, case-insensitive)
    ///   `desktop.ini`    — Windows folder settings (exact name, case-insensitive)
    ///   `emacs_backup~`  — Emacs backup (trailing tilde)
    ///   `FOO.SWP`        — uppercase extension (exercises to_lowercase path)
    #[test]
    fn walk_contributor_filters_editor_debris_so_two_developers_produce_the_same_hash() {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let root = tmpdir.path().to_path_buf();

        // Real source file that must be included.
        std::fs::write(root.join("clean.rs"), b"// real source").expect("must write clean.rs");

        // Editor debris files that must be excluded from the hash and from
        // cargo:rerun-if-changed directives.
        //
        // Each entry exercises a distinct branch of `is_editor_debris`:
        //   .swp / .swo / .swn / .orig / .bk  — extension-based matches
        //   .DS_Store / thumbs.db / desktop.ini — exact-name matches (case-insensitive)
        //   emacs_backup~                        — trailing-tilde match
        //   FOO.SWP                              — case-insensitive extension match
        let debris_names = [
            ".foo.swp",
            "bar.swo",
            "baz.swn", // vim swap variant (.swn)
            "qux.orig",
            "quux.bk",
            ".DS_Store",
            "thumbs.db",   // Windows thumbnail cache (exact-name)
            "desktop.ini", // Windows folder settings (exact-name)
            "emacs_backup~",
            "FOO.SWP", // uppercase extension — exercises to_lowercase path
        ];
        for name in &debris_names {
            std::fs::write(root.join(name), b"DEBRIS - must not appear in hash")
                .unwrap_or_else(|e| panic!("must write debris file {name}: {e}"));
        }

        let walk = crate::engine_hash_algo::walk_contributor("root", &root);

        // --- rerun_paths must NOT include any debris path ---
        for name in &debris_names {
            let debris_path = root.join(name);
            assert!(
                !walk.rerun_paths.contains(&debris_path),
                "debris file '{name}' must not appear in rerun_paths but it did; \
                 this means cargo would spuriously re-run the build script when \
                 the editor writes/removes that debris file"
            );
        }

        // --- hash must equal the hash of [clean.rs path, clean.rs content] ---
        let walk_refs: Vec<&[u8]> = walk.parts.iter().map(|v| v.as_slice()).collect();
        let hash_from_walk = compose_engine_version_hash(&walk_refs);

        let expected_parts: &[&[u8]] = &[b"root/clean.rs", b"// real source"];
        let expected_hash = compose_engine_version_hash(expected_parts);

        assert_eq!(
            hash_from_walk, expected_hash,
            "walk_contributor must produce the same hash as a hand-constructed \
             parts list containing only the real source file; debris files must \
             not enter the hash input. If this fails, check which debris pattern \
             leaked: \
             .swp (vim swap), \
             .swo (vim swap alt), \
             .swn (vim swap variant), \
             .orig (merge residue), \
             .bk (backup), \
             .DS_Store (macOS metadata), \
             thumbs.db (Windows thumbnail cache), \
             desktop.ini (Windows folder settings), \
             trailing ~ (Emacs backup), \
             FOO.SWP (uppercase extension — case-insensitive path)"
        );
    }

    /// Regression guard: a debris file whose name is a bare dot-prefixed
    /// extension with no stem (e.g. `.swp`, `.bak`) must be excluded from the
    /// hash and from `rerun_paths`.
    ///
    /// `std::path::Path::extension()` returns `None` for filenames like `.swp`
    /// because the leading dot is treated as the start of the stem, not as a
    /// separator — there is no suffix to extract.  The extension-branch in
    /// `is_editor_debris` therefore silently misses such names.  A separate
    /// dot-prefix branch (`name_lower.strip_prefix('.')`) is required to catch
    /// them.  This test pins that the branch exists and fires correctly.
    #[test]
    fn walk_contributor_filters_bare_dot_prefixed_debris_names_with_no_stem() {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let root = tmpdir.path().to_path_buf();

        // Real source file that must be included in the hash.
        std::fs::write(root.join("clean.rs"), b"// real source").expect("must write clean.rs");

        // Bare dot-prefixed name: `Path::new(".swp").extension()` returns
        // `None`, so only a dedicated strip_prefix('.') branch catches this.
        std::fs::write(root.join(".swp"), b"DEBRIS - must not appear in hash")
            .expect("must write .swp debris file");

        let walk = crate::engine_hash_algo::walk_contributor("root", &root);

        // --- rerun_paths must NOT include the bare-name debris path ---
        let debris_path = root.join(".swp");
        assert!(
            !walk.rerun_paths.contains(&debris_path),
            "bare '.swp' debris file must not appear in rerun_paths but it did; \
             Path::extension() returns None for names with no stem before the dot, \
             so is_editor_debris needs a dedicated dot-prefix branch to catch these"
        );

        // --- hash must equal the hash of [clean.rs path, clean.rs content] ---
        let walk_refs: Vec<&[u8]> = walk.parts.iter().map(|v| v.as_slice()).collect();
        let hash_from_walk = compose_engine_version_hash(&walk_refs);

        let expected_parts: &[&[u8]] = &[b"root/clean.rs", b"// real source"];
        let expected_hash = compose_engine_version_hash(expected_parts);

        assert_eq!(
            hash_from_walk, expected_hash,
            "walk_contributor must produce the same hash as a clean-only parts list; \
             the bare '.swp' debris file must not enter the hash input. \
             If this fails, is_editor_debris is missing the dot-prefix branch \
             that handles names like '.swp' where Path::extension() returns None."
        );
    }

    /// Regression guard for symlinks leaking into the cache key.
    ///
    /// `Path::is_file()` follows symlinks via `fs::metadata()` (std docs).
    /// A symlink to a regular file therefore passes `is_file()` and its target
    /// bytes would be framed into `parts`, making `ENGINE_VERSION_HASH`
    /// machine-specific (the target path / content differs per developer or CI
    /// host).  `walk_recursive` must instead use `symlink_metadata()` so that
    /// symlinks — whether dangling, valid, or pointing outside the contributor
    /// tree — are always silently skipped.
    #[cfg(unix)]
    #[test]
    fn walk_contributor_skips_symlinks_via_symlink_metadata_so_machine_local_links_do_not_perturb_the_hash()
     {
        use std::os::unix::fs::symlink;

        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let root = tmpdir.path().to_path_buf();

        // Real contributor file that MUST be included.
        std::fs::write(root.join("real.rs"), b"// real content").expect("must write real.rs");

        // Create an external file with distinguishable content in a SEPARATE
        // tempdir so it is genuinely outside the walked directory tree.
        // (Placing it under `root` would make the walker visit it directly.)
        let extern_tmpdir = tempfile::TempDir::new().expect("must create extern tempdir");
        let outside_target = extern_tmpdir.path().join("outside.txt");
        std::fs::write(&outside_target, b"DO NOT INCLUDE").expect("must write outside target");

        // Symlink named `link.rs` points outside the contributor tree.
        let symlink_path = root.join("link.rs");
        symlink(&outside_target, &symlink_path).expect("must create symlink");

        let walk = crate::engine_hash_algo::walk_contributor("root", &root);

        // The symlink path must NOT appear in rerun_paths.
        assert!(
            !walk.rerun_paths.contains(&symlink_path),
            "symlink path must not appear in rerun_paths; got: {:?}",
            walk.rerun_paths
        );

        // The sentinel bytes from the symlink target must NOT appear anywhere
        // in parts (tested with a sliding window to defeat any framing/length
        // prefix that might split the literal across two Vec<u8> chunks).
        let sentinel = b"DO NOT INCLUDE";
        let leaked = walk
            .parts
            .iter()
            .any(|chunk| chunk.windows(sentinel.len()).any(|w| w == sentinel));
        assert!(
            !leaked,
            "symlink target bytes 'DO NOT INCLUDE' must not appear in walk.parts; \
             walk_recursive is following the symlink via is_file() instead of \
             using symlink_metadata()"
        );

        // The symlink's own path key must also not appear in parts.
        // A future regression might frame the symlink's path-key bytes (e.g.
        // `b"root/link.rs"`) without reading the target — the target-bytes check
        // above would still pass, but the hash would silently diverge.
        let path_key_leaked = walk
            .parts
            .iter()
            .any(|chunk| chunk.windows(b"link.rs".len()).any(|w| w == b"link.rs"));
        assert!(
            !path_key_leaked,
            "symlink path key bytes 'link.rs' must not appear in walk.parts; \
             a regression may be framing the symlink's path key without reading \
             its target — the hash would still diverge across machines"
        );

        // The walk hash must equal the hash of [real.rs path, real.rs content].
        let walk_refs: Vec<&[u8]> = walk.parts.iter().map(|v| v.as_slice()).collect();
        let hash_from_walk = compose_engine_version_hash(&walk_refs);

        let expected_parts: &[&[u8]] = &[b"root/real.rs", b"// real content"];
        let expected_hash = compose_engine_version_hash(expected_parts);

        assert_eq!(
            hash_from_walk, expected_hash,
            "walk_contributor hash must equal the hash of the real file only; \
             symlink target bytes must not enter the hash input"
        );
    }

    /// Behavioral regression guard: verifies that a one-byte mutation in the
    /// workspace `Cargo.lock` produces a different `compose_engine_version_hash`
    /// output when processed through the same `walk_contributor` path that
    /// `build.rs` uses.
    ///
    /// This test covers the end-to-end contract required by the PRD
    /// (`docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine
    /// version"): "any change to the FEA engine" must invalidate.  The structural
    /// inclusion test (below) proves Cargo.lock is *listed*; this test proves the
    /// algorithm actually *distinguishes* different lock-file revisions.
    #[test]
    fn walking_workspace_cargo_lock_then_modifying_one_byte_changes_compose_engine_version_hash_output()
     {
        use std::io::Write as _;

        let lock_path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        assert!(
            lock_path.exists(),
            "workspace Cargo.lock not found at {}; the test path resolution \
             (CARGO_MANIFEST_DIR/../../Cargo.lock) must match the path used by \
             build.rs and CONTRIBUTORS_RELATIVE",
            lock_path.display()
        );

        // Baseline: walk the real workspace Cargo.lock.
        let baseline_walk =
            crate::engine_hash_algo::walk_contributor("../../Cargo.lock", &lock_path);
        let baseline_refs: Vec<&[u8]> = baseline_walk.parts.iter().map(|v| v.as_slice()).collect();
        let baseline_hash = compose_engine_version_hash(&baseline_refs);

        // Mutated copy: flip the first byte of the lock file content.
        let mut bytes = std::fs::read(&lock_path).expect("must read workspace Cargo.lock");
        bytes[0] ^= 0xFF;
        let mut mutated_tmpfile =
            tempfile::NamedTempFile::new().expect("must create mutated tempfile");
        mutated_tmpfile
            .write_all(&bytes)
            .expect("must write mutated bytes to tempfile");
        mutated_tmpfile
            .flush()
            .expect("must flush mutated tempfile");
        let mutated_path = mutated_tmpfile.path().to_path_buf();

        // Walk the mutated file under the SAME label so the only difference is
        // content, not the path key.
        let mutated_walk =
            crate::engine_hash_algo::walk_contributor("../../Cargo.lock", &mutated_path);
        let mutated_refs: Vec<&[u8]> = mutated_walk.parts.iter().map(|v| v.as_slice()).collect();
        let mutated_hash = compose_engine_version_hash(&mutated_refs);

        assert_ne!(
            baseline_hash, mutated_hash,
            "a one-byte flip in the workspace Cargo.lock must change \
             compose_engine_version_hash output (PRD requirement: any change to \
             the FEA engine — including transitive dep version bumps captured by \
             Cargo.lock — must invalidate all cache entries)"
        );
    }

    /// PRD-required structural guard: the workspace `Cargo.lock` must appear in
    /// `CONTRIBUTORS_RELATIVE` so that any transitive dependency version bump
    /// (e.g. `nalgebra`, `faer`, `gmsh-sys`, `nalgebra-sparse`) causes
    /// `ENGINE_VERSION_HASH` to change and all existing cache entries to miss.
    ///
    /// Without this entry, a transitive dep upgrade that alters FEA semantics
    /// (different LU pivoting strategy, different eigensolver tolerances) would
    /// leave the persistent cache serving stale results indefinitely.
    ///
    /// Reference: `docs/prds/v0_3/persistent-fea-cache.md`
    /// §"Cache invalidation on engine version" — "any change to the FEA engine"
    /// must invalidate.
    #[test]
    fn contributors_relative_includes_workspace_cargo_lock_for_transitive_dep_invalidation() {
        let found = crate::engine_hash_algo::CONTRIBUTORS_RELATIVE.contains(&"../../Cargo.lock");
        assert!(
            found,
            "CONTRIBUTORS_RELATIVE must contain \"../../Cargo.lock\" so that any \
             transitive dependency version bump causes ENGINE_VERSION_HASH to change \
             (PRD docs/prds/v0_3/persistent-fea-cache.md §\"Cache invalidation on engine \
             version\"). Actual list: {:#?}",
            crate::engine_hash_algo::CONTRIBUTORS_RELATIVE
        );
    }

    // ── path-layout tests ────────────────────────────────────────────────────

    #[test]
    fn shard_dir_returns_two_level_directory_under_engine_version_hash() {
        use std::path::PathBuf;
        let root = PathBuf::from("/some/cache");
        let engine = "abc123def456abc123def456abc123ff";
        let input = "0123456789abcdef0123456789abcdef";
        let dir = shard_dir(&root, engine, input);
        assert_eq!(
            dir,
            PathBuf::from("/some/cache/abc123def456abc123def456abc123ff/01"),
            "shard_dir must produce <root>/<engine>/<input[0..2]>"
        );
        // Callers do `create_dir_all(&shard_dir(...))` once, then write both
        // .bin and .meta into it — the parent must match entry_bin_path's parent.
        let bin = entry_bin_path(&root, engine, input);
        assert_eq!(
            Some(dir.as_path()),
            bin.parent(),
            "shard_dir must equal entry_bin_path(...).parent()"
        );
    }

    #[test]
    fn entry_meta_path_uses_meta_extension_under_same_shard_dir_as_bin() {
        use std::path::PathBuf;
        let root = PathBuf::from("/some/cache");
        let engine = "abc123def456abc123def456abc123ff";
        let input = "0123456789abcdef0123456789abcdef";
        let meta = entry_meta_path(&root, engine, input);
        assert_eq!(
            meta,
            PathBuf::from(
                "/some/cache/abc123def456abc123def456abc123ff/01/0123456789abcdef0123456789abcdef.meta"
            ),
        );
        // Sidecar must share the same parent directory as the .bin file so
        // atomic-rename semantics work (both files land in the same dir).
        let bin = entry_bin_path(&root, engine, input);
        assert_eq!(
            meta.parent(),
            bin.parent(),
            "entry_meta_path and entry_bin_path must share the same parent directory"
        );
    }

    #[test]
    fn entry_bin_path_uses_two_level_shard_layout() {
        use std::path::PathBuf;
        let root = PathBuf::from("/some/cache");
        let engine = "abc123def456abc123def456abc123ff";
        let input = "0123456789abcdef0123456789abcdef";
        let got = entry_bin_path(&root, engine, input);
        assert_eq!(
            got,
            PathBuf::from(
                "/some/cache/abc123def456abc123def456abc123ff/01/0123456789abcdef0123456789abcdef.bin"
            ),
            "entry_bin_path must produce <root>/<engine>/<input[0..2]>/<input>.bin"
        );
        // The shard directory is determined by input[0..2] = "01".
        assert_eq!(
            got.parent().unwrap().file_name().unwrap().to_str().unwrap(),
            &input[..2],
            "shard dir name must be input_hash[0..2]"
        );
    }

    // ── sidecar tests ────────────────────────────────────────────────────────

    #[test]
    fn read_sidecar_mtime_returns_value_consistent_with_fs_metadata_modified() {
        use std::fs::File;
        use std::time::{Duration, UNIX_EPOCH};

        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let path = tmpdir.path().join("entry.meta");
        write_sidecar(&path).expect("write_sidecar must succeed");

        // Back-date to a well-known absolute time so we can verify the value.
        let known_mtime = UNIX_EPOCH + Duration::from_secs(42_424_242);
        {
            let f = File::options().write(true).open(&path).expect("must open");
            f.set_times(std::fs::FileTimes::new().set_modified(known_mtime))
                .expect("must set mtime");
        }

        let got = read_sidecar_mtime(&path).expect("read_sidecar_mtime must succeed");
        let expected = std::fs::metadata(&path)
            .expect("must stat")
            .modified()
            .expect("must have mtime");
        assert_eq!(
            got, expected,
            "read_sidecar_mtime must match fs::metadata().modified()"
        );
        assert_eq!(
            got, known_mtime,
            "read_sidecar_mtime must return the back-dated value"
        );

        // Non-existent path must return Err with kind NotFound.
        let missing = tmpdir.path().join("no_such.meta");
        let err = read_sidecar_mtime(&missing).expect_err("must fail for missing file");
        assert_eq!(
            err.kind(),
            io::ErrorKind::NotFound,
            "expected NotFound for missing sidecar, got {:?}",
            err
        );
    }

    #[test]
    fn touch_sidecar_updates_mtime_to_a_strictly_later_value_without_changing_content() {
        use std::fs::File;
        use std::time::{Duration, UNIX_EPOCH};

        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let path = tmpdir.path().join("entry.meta");
        write_sidecar(&path).expect("write_sidecar must succeed");

        // Back-date the file to a known-old mtime to guarantee strictly-earlier
        // baseline regardless of filesystem mtime resolution.
        let old_mtime = UNIX_EPOCH + Duration::from_secs(1_000_000);
        {
            let f = File::options().write(true).open(&path).expect("must open");
            f.set_times(std::fs::FileTimes::new().set_modified(old_mtime))
                .expect("must set old mtime");
        }

        touch_sidecar(&path).expect("touch_sidecar must succeed");

        let mtime_after = std::fs::metadata(&path)
            .expect("must stat")
            .modified()
            .expect("must have mtime");
        assert!(
            mtime_after > old_mtime,
            "touch_sidecar must advance mtime beyond the back-dated baseline"
        );
        // Content must be unchanged.
        let contents = std::fs::read(&path).expect("must read");
        assert_eq!(
            contents,
            vec![SIDECAR_MAGIC_BYTE],
            "content must be unchanged after touch"
        );
    }

    #[test]
    fn write_sidecar_creates_file_with_single_magic_byte() {
        let tmpdir = tempfile::TempDir::new().expect("must create tempdir");
        let meta_path = tmpdir.path().join("entry.meta");
        write_sidecar(&meta_path).expect("write_sidecar must succeed");
        assert!(
            meta_path.exists(),
            "sidecar file must exist after write_sidecar"
        );
        let contents = std::fs::read(&meta_path).expect("must read sidecar");
        assert_eq!(
            contents,
            vec![SIDECAR_MAGIC_BYTE],
            "sidecar must contain exactly one byte equal to SIDECAR_MAGIC_BYTE"
        );
        assert_eq!(
            SIDECAR_MAGIC_BYTE, 0xCAu8,
            "SIDECAR_MAGIC_BYTE must be 0xCA"
        );
    }

    // ── CacheEntryHeader tests ────────────────────────────────────────────────

    #[test]
    fn cache_entry_header_verify_echoes_rejects_input_hash_mismatch_with_invalid_data() {
        let header = CacheEntryHeader {
            format_version: 1,
            engine_version_hash: [0xAAu8; 32],
            input_hash: [0xBBu8; 32],
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        };
        let correct_engine = [0xAAu8; 32];
        let wrong_input = [0xDDu8; 32];
        let err = header
            .verify_field_echoes(&correct_engine, &wrong_input)
            .expect_err("input_hash mismatch must return Err");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("input_hash"),
            "error message must contain 'input_hash', got: {err}"
        );
    }

    #[test]
    fn cache_entry_header_verify_echoes_rejects_engine_version_hash_mismatch_with_invalid_data() {
        let header = CacheEntryHeader {
            format_version: 1,
            engine_version_hash: [0xAAu8; 32],
            input_hash: [0xBBu8; 32],
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        };
        let wrong_engine = [0xCCu8; 32];
        let correct_input = [0xBBu8; 32];
        let err = header
            .verify_field_echoes(&wrong_engine, &correct_input)
            .expect_err("engine_version_hash mismatch must return Err");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("engine_version_hash"),
            "error message must contain 'engine_version_hash', got: {err}"
        );
    }

    #[test]
    fn cache_entry_header_verify_echoes_returns_ok_when_both_echoes_match() {
        let engine = [0xAAu8; 32];
        let input = [0xBBu8; 32];
        let header = CacheEntryHeader {
            format_version: 1,
            engine_version_hash: engine,
            input_hash: input,
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        };
        assert!(
            header.verify_field_echoes(&engine, &input).is_ok(),
            "verify_field_echoes must return Ok when both echoes match"
        );
    }

    #[test]
    fn cache_entry_header_verify_format_version_returns_ok_for_current_version() {
        let header = CacheEntryHeader {
            format_version: ENTRY_FORMAT_VERSION,
            engine_version_hash: [0u8; 32],
            input_hash: [0u8; 32],
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        };
        assert!(
            header.verify_format_version().is_ok(),
            "verify_format_version must return Ok when format_version matches ENTRY_FORMAT_VERSION"
        );
    }

    #[test]
    fn cache_entry_header_verify_format_version_rejects_stale_version_with_invalid_data() {
        let header = CacheEntryHeader {
            format_version: 0, // stale / uninitialised sentinel
            engine_version_hash: [0u8; 32],
            input_hash: [0u8; 32],
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        };
        let err = header
            .verify_format_version()
            .expect_err("format_version mismatch must return Err");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("format_version"),
            "error message must contain 'format_version', got: {err}"
        );
    }

    #[test]
    fn entry_format_version_const_is_one() {
        // Pins the start-at-1 convention (0 = uninitialised / unknown).
        // An intentional on-disk-layout bump must touch this assertion — that
        // is the point: it forces a deliberate acknowledgement that cached bytes
        // from the previous version are now incompatible. Mirrors the
        // `elastic_result_format_version_is_one` pattern for body-format
        // versioning; these two consts are intentionally distinct namespaces
        // (entry-header layout vs. body encoding).
        assert_eq!(ENTRY_FORMAT_VERSION, 1);
    }

    #[test]
    fn cache_entry_header_bincode_encoding_matches_pinned_hex_literal() {
        // Fixture uses recognisable, distinct non-zero per-field values so the
        // LE byte order is visually verifiable at the test site.
        // Field values:
        //   format_version       = 0xDEAD_BEEF  (u32 LE: EF BE AD DE)
        //   engine_version_hash  = [0xA0..=0xBF] (32 ascending bytes)
        //   input_hash           = [0xC0..=0xDF] (32 ascending bytes)
        //   solve_time_ms        = 0xCAFE_BABE_DEAD_BEEF (u64 LE)
        //   byte_size            = 0x1234_5678_9ABC_DEF0 (u64 LE)
        //   written_at           = 0x7EDC_BA98_7654_3210 (i64 LE)
        let mut engine_hash = [0u8; 32];
        for (i, b) in engine_hash.iter_mut().enumerate() {
            *b = 0xA0u8 + i as u8;
        }
        let mut input_hash = [0u8; 32];
        for (i, b) in input_hash.iter_mut().enumerate() {
            *b = 0xC0u8 + i as u8;
        }
        let fixture = CacheEntryHeader {
            format_version: 0xDEAD_BEEFu32,
            engine_version_hash: engine_hash,
            input_hash,
            solve_time_ms: 0xCAFE_BABE_DEAD_BEEFu64,
            byte_size: 0x1234_5678_9ABC_DEF0u64,
            written_at: 0x7EDC_BA98_7654_3210i64,
        };
        let mut encoded: Vec<u8> = Vec::new();
        fixture
            .write_to(&mut encoded)
            .expect("write_to must not fail");

        // Pinned bincode 1.3 fixint-LE encoding of the fixture.
        // Layout (struct-declaration order, LE encoding):
        //   format_version (u32, 4 bytes):              EF BE AD DE
        //   engine_version_hash ([u8;32], 32 bytes):    A0 A1 A2 ... BF
        //   input_hash ([u8;32], 32 bytes):              C0 C1 C2 ... DF
        //   solve_time_ms (u64, 8 bytes):                EF BE AD DE BE BA FE CA
        //   byte_size (u64, 8 bytes):                    F0 DE BC 9A 78 56 34 12
        //   written_at (i64, 8 bytes):                   10 32 54 76 98 BA DC 7E
        // Total: 92 bytes.
        // Pinned bincode 1.3 fixint-LE encoding, observed and captured in
        // step-6 GREEN. Any encoder drift (within or beyond the =1.3 pin)
        // that alters the wire format will break this assertion; fix by
        // updating the literal AND bumping ENTRY_FORMAT_VERSION in the same
        // commit.
        let expected: [u8; 92] = [
            // format_version = 0xDEAD_BEEF (u32 LE, 4 bytes)
            0xEF, 0xBE, 0xAD, 0xDE, // engine_version_hash = [0xA0..=0xBF] (32 bytes)
            0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
            0xAE, 0xAF, 0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xBB,
            0xBC, 0xBD, 0xBE, 0xBF, // input_hash = [0xC0..=0xDF] (32 bytes)
            0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD,
            0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xDB,
            0xDC, 0xDD, 0xDE, 0xDF,
            // solve_time_ms = 0xCAFE_BABE_DEAD_BEEF (u64 LE, 8 bytes)
            0xEF, 0xBE, 0xAD, 0xDE, 0xBE, 0xBA, 0xFE, 0xCA,
            // byte_size = 0x1234_5678_9ABC_DEF0 (u64 LE, 8 bytes)
            0xF0, 0xDE, 0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12,
            // written_at = 0x7EDC_BA98_7654_3210 (i64 LE, 8 bytes)
            0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0x7E,
        ];

        assert_eq!(
            encoded.len(),
            ENTRY_HEADER_ENCODED_LEN,
            "encoded length must be ENTRY_HEADER_ENCODED_LEN = {ENTRY_HEADER_ENCODED_LEN}"
        );
        assert_eq!(
            encoded.as_slice(),
            &expected[..],
            "bincode 1.3 fixint-LE encoding of CacheEntryHeader has drifted from \
             the pinned wire-format fixture; if intentional, bump ENTRY_FORMAT_VERSION \
             in the SAME commit and update this literal"
        );
        // Round-trip: decode from the pinned literal back to the original struct.
        let decoded =
            CacheEntryHeader::read_from(&mut &expected[..]).expect("must decode pinned literal");
        assert_eq!(decoded, fixture);
    }

    #[test]
    fn cache_entry_header_round_trips_all_six_fields() {
        // Forces `CacheEntryHeader` + `write_to`/`read_from` to exist and
        // validates that all six fields survive a bincode round-trip.
        // Uses non-zero, distinct-per-field values so any field aliasing
        // or field-swap bug surfaces immediately.
        let engine_hash = [0xABu8; 32];
        let input_hash = [0xCDu8; 32];
        let original = CacheEntryHeader {
            format_version: 1,
            engine_version_hash: engine_hash,
            input_hash,
            solve_time_ms: 1234,
            byte_size: 5_678_901,
            written_at: 1_700_000_000_000,
        };
        let mut buf: Vec<u8> = Vec::new();
        original.write_to(&mut buf).expect("write_to must succeed");
        let decoded = CacheEntryHeader::read_from(&mut &buf[..]).expect("read_from must succeed");
        assert_eq!(decoded.format_version, original.format_version);
        assert_eq!(decoded.engine_version_hash, original.engine_version_hash);
        assert_eq!(decoded.input_hash, original.input_hash);
        assert_eq!(decoded.solve_time_ms, original.solve_time_ms);
        assert_eq!(decoded.byte_size, original.byte_size);
        assert_eq!(decoded.written_at, original.written_at);
    }

    // ── write_entry / read_entry I/O tests ───────────────────────────────────

    #[test]
    fn write_entry_then_read_entry_round_trips_an_elastic_result_value() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "abcdef0123456789abcdef0123456789";
        let inp = "fedcba9876543210fedcba9876543210";
        let original = make_sample_result();
        write_entry(root, eng, inp, &original).unwrap();
        let read_back = read_entry::<ElasticResult>(root, eng, inp).unwrap();
        assert_eq!(read_back, Some(original));
    }

    #[test]
    fn write_entry_creates_meta_sidecar_with_magic_byte_in_same_shard_dir_as_bin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "aabbccddeeff00112233445566778899";
        let inp = "99887766554433221100ffeeddccbbaa";
        let original = make_sample_result();

        write_entry(root, eng, inp, &original).unwrap();

        let meta_path = entry_meta_path(root, eng, inp);
        let bin_path = entry_bin_path(root, eng, inp);

        // Sidecar must exist and contain exactly the magic byte.
        assert!(
            meta_path.exists(),
            "write_entry must create the .meta sidecar; path: {}",
            meta_path.display()
        );
        let content = std::fs::read(&meta_path).unwrap();
        assert_eq!(
            content,
            vec![SIDECAR_MAGIC_BYTE],
            ".meta sidecar must contain exactly [SIDECAR_MAGIC_BYTE=0xCA]"
        );

        // Structural invariant: both files live in the same shard dir.
        assert_eq!(
            meta_path.parent().unwrap(),
            bin_path.parent().unwrap(),
            ".meta and .bin must share the same parent shard directory"
        );
    }

    #[test]
    fn read_entry_advances_meta_sidecar_mtime_above_backdated_baseline_on_hit_and_succeeds_when_sidecar_pre_deleted()
     {
        use std::time::{Duration, UNIX_EPOCH};

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "cccccccccccccccccccccccccccccccc";
        let inp = "dddddddddddddddddddddddddddddddd";
        let original = make_sample_result();

        write_entry(root, eng, inp, &original).unwrap();

        let meta_path = entry_meta_path(root, eng, inp);

        // --- Phase 1: back-date the sidecar mtime to a known-old value ---
        let old_mtime = UNIX_EPOCH + Duration::from_secs(1_000_000);
        {
            let f = std::fs::File::options()
                .write(true)
                .open(&meta_path)
                .expect("must open sidecar for mtime back-dating");
            f.set_times(std::fs::FileTimes::new().set_modified(old_mtime))
                .expect("must set modified time to old_mtime");
        }

        // read_entry must succeed and return the value.
        let hit = read_entry::<ElasticResult>(root, eng, inp).unwrap();
        assert_eq!(
            hit,
            Some(original.clone()),
            "phase 1: cache hit must return the original value"
        );

        // Sidecar mtime must have advanced above the backdated baseline.
        let new_mtime = std::fs::metadata(&meta_path)
            .expect("sidecar must still exist after read_entry")
            .modified()
            .unwrap();
        assert!(
            new_mtime > old_mtime,
            "read_entry must touch the sidecar mtime (LRU signal update); \
             got new_mtime={new_mtime:?} <= old_mtime={old_mtime:?}"
        );

        // --- Phase 2: sidecar deleted before second read ---
        // Simulates the GC evicting the sidecar between write and read;
        // read_entry must still succeed (data is in the .bin).
        std::fs::remove_file(&meta_path).unwrap();
        let hit2 = read_entry::<ElasticResult>(root, eng, inp).unwrap();
        assert_eq!(
            hit2,
            Some(original),
            "phase 2: read must succeed even if sidecar was pre-deleted"
        );
    }

    #[test]
    fn concurrent_write_entry_calls_for_same_input_both_succeed_and_final_read_entry_decodes_to_original_value()
     {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "0011223344556677889900aabbccddee";
        let inp = "ffeeddccbbaa99887766554433221100";
        let original = make_sample_result();

        // Compute the expected compressed body len from a single-writer reference.
        let mut body_ref: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut body_ref).unwrap();
        let expected_file_size = ENTRY_HEADER_ENCODED_LEN + body_ref.len();

        // Both concurrent writers must succeed without panicking or returning Err.
        std::thread::scope(|s| {
            let h1 = s.spawn(|| write_entry(root, eng, inp, &original));
            let h2 = s.spawn(|| write_entry(root, eng, inp, &original));
            h1.join().expect("writer thread 1 must not panic").unwrap();
            h2.join().expect("writer thread 2 must not panic").unwrap();
        });

        // The final read must return the original value.
        let hit = read_entry::<ElasticResult>(root, eng, inp).unwrap();
        assert_eq!(
            hit,
            Some(original.clone()),
            "concurrent writers must produce a valid, decodable entry"
        );

        // The .bin must be exactly ENTRY_HEADER_ENCODED_LEN + body_len bytes —
        // no torn writes, no extra bytes from concurrent interleaving.
        let actual_file_size = std::fs::metadata(entry_bin_path(root, eng, inp))
            .unwrap()
            .len() as usize;
        assert_eq!(
            actual_file_size,
            expected_file_size,
            "concurrent writers must produce a .bin of exactly \
             ENTRY_HEADER_ENCODED_LEN({ENTRY_HEADER_ENCODED_LEN}) + body_len({}) = {expected_file_size} bytes; \
             got {actual_file_size}",
            body_ref.len()
        );
    }

    /// Write a raw header plus `body_suffix` bytes to the .bin for a given key.
    /// Useful for constructing corrupt-body .bin files in tests.
    fn write_header_and_body_to_bin(
        root: &Path,
        eng: &str,
        inp: &str,
        header: &CacheEntryHeader,
        body_suffix: &[u8],
    ) {
        let sd = shard_dir(root, eng, inp);
        std::fs::create_dir_all(&sd).unwrap();
        let mut f = std::fs::File::create(entry_bin_path(root, eng, inp)).unwrap();
        header.write_to(&mut f).unwrap();
        if !body_suffix.is_empty() {
            use std::io::Write as _;
            f.write_all(body_suffix).unwrap();
        }
    }

    /// Build a CacheEntryHeader with correct echoes for the given key (for
    /// tests that need a structurally valid header to get past echo verification).
    fn make_correct_header(eng: &str, inp: &str) -> CacheEntryHeader {
        CacheEntryHeader {
            format_version: ENTRY_FORMAT_VERSION,
            engine_version_hash: *eng.as_bytes().first_chunk::<32>().unwrap(),
            input_hash: *inp.as_bytes().first_chunk::<32>().unwrap(),
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        }
    }

    #[test]
    fn read_entry_returns_ok_none_when_compressed_body_is_corrupted_or_truncated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Sub-scenario (a): valid header + zero body bytes.
        // zstd::Decoder::new will fail on the empty reader (no frame magic).
        {
            let eng = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeee55";
            let inp = "ffffffffffffffffffffffffffff5566";
            let h = make_correct_header(eng, inp);
            write_header_and_body_to_bin(root, eng, inp, &h, &[]);
            let result = read_entry::<ElasticResult>(root, eng, inp).unwrap();
            assert_eq!(
                result, None,
                "zero body bytes must be treated as cache miss"
            );
        }

        // Sub-scenario (b): valid header + 16 bytes of garbage.
        // zstd will fail to parse the frame (bad magic number).
        {
            let eng = "aaaaaaaaaaaaaaaaaaaaaaaaaaaa7788";
            let inp = "bbbbbbbbbbbbbbbbbbbbbbbbbbbb9900";
            let h = make_correct_header(eng, inp);
            let garbage = b"not-a-zstd-frame";
            write_header_and_body_to_bin(root, eng, inp, &h, garbage);
            let result = read_entry::<ElasticResult>(root, eng, inp).unwrap();
            assert_eq!(
                result, None,
                "garbage body bytes must be treated as cache miss"
            );
        }
    }

    /// Helper: write a raw CacheEntryHeader (and nothing else) to the .bin path
    /// for a given key in `root`. The caller controls the header fields, allowing
    /// sub-tests to inject mismatched echoes or other corruption.
    fn write_raw_header_to_bin(root: &Path, eng: &str, inp: &str, header: &CacheEntryHeader) {
        let sd = shard_dir(root, eng, inp);
        std::fs::create_dir_all(&sd).unwrap();
        let mut f = std::fs::File::create(entry_bin_path(root, eng, inp)).unwrap();
        header.write_to(&mut f).unwrap();
    }

    #[test]
    fn read_entry_returns_ok_none_when_header_echo_fields_do_not_match_path_components() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Sub-scenario (a): engine_version_hash echo is wrong.
        {
            let eng = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";
            let inp = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb22";
            let wrong_engine_bytes = *b"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
            let h = CacheEntryHeader {
                format_version: ENTRY_FORMAT_VERSION,
                engine_version_hash: wrong_engine_bytes,
                input_hash: *inp.as_bytes().first_chunk::<32>().unwrap(),
                solve_time_ms: 0,
                byte_size: 0,
                written_at: 0,
            };
            write_raw_header_to_bin(root, eng, inp, &h);
            let result = read_entry::<ElasticResult>(root, eng, inp).unwrap();
            assert_eq!(
                result, None,
                "engine_version_hash echo mismatch must be treated as cache miss"
            );
        }

        // Sub-scenario (b): input_hash echo is wrong.
        {
            let eng = "cccccccccccccccccccccccccccccc33";
            let inp = "dddddddddddddddddddddddddddddd44";
            let wrong_input_bytes = *b"yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy";
            let h = CacheEntryHeader {
                format_version: ENTRY_FORMAT_VERSION,
                engine_version_hash: *eng.as_bytes().first_chunk::<32>().unwrap(),
                input_hash: wrong_input_bytes,
                solve_time_ms: 0,
                byte_size: 0,
                written_at: 0,
            };
            write_raw_header_to_bin(root, eng, inp, &h);
            let result = read_entry::<ElasticResult>(root, eng, inp).unwrap();
            assert_eq!(
                result, None,
                "input_hash echo mismatch must be treated as cache miss"
            );
        }
    }

    #[test]
    fn read_entry_returns_ok_none_when_header_format_version_does_not_match_expected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let inp = "ffffffffffffffffffffffffffffffff";

        // Build a .bin with a mismatched format_version but correct echoes.
        let sd = shard_dir(root, eng, inp);
        std::fs::create_dir_all(&sd).unwrap();
        let stale_header = CacheEntryHeader {
            format_version: ENTRY_FORMAT_VERSION + 99,
            engine_version_hash: *eng.as_bytes().first_chunk::<32>().unwrap(),
            input_hash: *inp.as_bytes().first_chunk::<32>().unwrap(),
            solve_time_ms: 0,
            byte_size: 0,
            written_at: 0,
        };
        // Write header only — no body bytes follow (read_entry must reject on
        // format_version before attempting body decode).
        let mut bin_file = std::fs::File::create(entry_bin_path(root, eng, inp)).unwrap();
        stale_header.write_to(&mut bin_file).unwrap();
        drop(bin_file);

        let result = read_entry::<ElasticResult>(root, eng, inp).unwrap();
        assert_eq!(
            result, None,
            "format_version mismatch must be treated as cache miss"
        );
    }

    #[test]
    fn read_entry_returns_ok_none_when_bin_file_is_absent_even_with_orphaned_tempfile_in_shard_dir()
    {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "1111111111111111111111111111111a";
        let inp = "2222222222222222222222222222222b";

        // Create the shard dir and drop a stray orphan tempfile (simulates
        // a writer that was killed mid-write; the .bin was never renamed in).
        let sd = shard_dir(root, eng, inp);
        std::fs::create_dir_all(&sd).unwrap();
        std::fs::write(sd.join(".tmp.orphan"), b"garbage bytes not a valid entry").unwrap();

        // The .bin for this key does NOT exist; read_entry must return Ok(None).
        let result = read_entry::<ElasticResult>(root, eng, inp).unwrap();
        assert_eq!(result, None);
    }

    /// Pins that `CacheEntryHeader.byte_size` holds the *uncompressed* body byte
    /// count, as documented in lines 195-196 and 210-211 of this file.
    ///
    /// The current `write_entry` impl stores `body_buf.len()` which is the
    /// zstd-COMPRESSED length — this test must FAIL until step-18 fixes the impl
    /// by computing the true uncompressed size via `value.uncompressed_byte_size()`.
    ///
    /// Asserts the semantic contract:
    ///   `header.byte_size == decompressed.len() as u64`
    ///
    /// A "compressed < uncompressed" check is deliberately NOT included here: on
    /// a tiny fixture like `make_sample_result` (~109 uncompressed bytes with
    /// limited redundancy) zstd's frame/block framing overhead (~13–17 bytes)
    /// can make the compressed body ≥ uncompressed, so such a check would be
    /// fragile and zstd-version-dependent. The primary equality assertion fully
    /// pins the contract — if the impl regresses to storing `body_buf.len()`
    /// (the compressed length), the equality fails.
    #[test]
    fn write_entry_populates_byte_size_field_with_actually_uncompressed_body_byte_count() {
        use std::fs::File;
        use std::io::Read as _;

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "deadbeef00112233deadbeef00112233";
        let inp = "cafebabe44556677cafebabe44556677";
        let original = make_sample_result();

        write_entry(root, eng, inp, &original).unwrap();

        // Read the on-disk header directly.
        let bin_path = entry_bin_path(root, eng, inp);
        let mut f = File::open(&bin_path).unwrap();
        let header = CacheEntryHeader::read_from(&mut f).unwrap();

        // Collect the compressed body bytes that follow the header.
        let mut compressed_body: Vec<u8> = Vec::new();
        f.read_to_end(&mut compressed_body).unwrap();

        // Independently decompress to recover the raw (uncompressed) body.
        let mut decoder = zstd::Decoder::new(&compressed_body[..]).unwrap();
        let mut decompressed: Vec<u8> = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();

        assert_eq!(
            header.byte_size,
            decompressed.len() as u64,
            "byte_size must be the uncompressed body byte count per CacheEntryHeader doc \
             (lines 195-196 / 210-211) — got {got}, expected {expected} (decompressed length). \
             If the impl stores body_buf.len() (compressed size) instead, this fails.",
            got = header.byte_size,
            expected = decompressed.len() as u64,
        );
    }

    // ── Eviction primitive tests ──────────────────────────────────────────────

    #[test]
    fn eviction_score_formula_is_age_secs_over_max_solve_time_ms_one() {
        use std::time::{Duration, SystemTime};

        const EPS: f64 = 1e-9;

        let now = SystemTime::now();

        // (a) Ordinary case: last_access 100 s ago, solve_time_ms=10 → 100.0/10 = 10.0.
        let last_access_a = now - Duration::from_secs(100);
        let score_a = eviction_score(now, last_access_a, 10);
        assert!(
            (score_a - 10.0).abs() < EPS,
            "ordinary case: expected 10.0, got {score_a}"
        );

        // (b) Zero-clamp: last_access 60 s ago, solve_time_ms=0 → 60.0/max(0,1)=60.0 (NOT INFINITY).
        let last_access_b = now - Duration::from_secs(60);
        let score_b = eviction_score(now, last_access_b, 0);
        assert!(
            (score_b - 60.0).abs() < EPS,
            "zero-clamp case: expected 60.0, got {score_b}"
        );

        // (c) Just-touched: last_access == now, solve_time_ms=5 → 0.0.
        let score_c = eviction_score(now, now, 5);
        assert!(
            score_c.abs() < EPS,
            "just-touched case: expected 0.0, got {score_c}"
        );
    }

    /// Core PRD eviction policy test (deterministic RED/GREEN structure).
    ///
    /// Two entries in eng_A; one sentinel in eng_B.
    ///
    /// The discriminating scenario: `(a)` is CHEAP (solve_time_ms=1) with a
    /// slightly NEWER mtime than `(b)`; `(b)` is EXPENSIVE (solve_time_ms=10_000)
    /// with a slightly OLDER mtime.
    ///
    /// Naive LRU (sort by mtime ascending) evicts `(b)` first (oldest mtime).
    /// Cost-aware sort by `eviction_score` descending picks `(a)` first:
    ///   score(a) ≈ age_a / 1        (huge — cheap)
    ///   score(b) ≈ age_b / 10_000   (10_000× smaller — expensive)
    /// Since both are backdated to ~1970, age_a ≈ age_b and score(a) >> score(b).
    ///
    /// Cap = sz(b) so exactly one entry survives in eng_A.
    ///
    /// With cost-aware sort: evict (a) → remaining = sz(b) = cap → break.
    ///   → (a) gone, (b) survives: test PASSES.
    /// With naive LRU: evict (b) → remaining = sz(a); sz(a) ≤ cap = sz(b)
    ///   → (b) evicted, (a) survives: assertion "(a) .bin must be evicted" FAILS.
    #[test]
    fn evict_over_cap_evicts_cheap_stale_first_keeps_expensive_old_removes_meta_and_respects_engine_version_scope()
     {
        use std::time::{Duration, UNIX_EPOCH};

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng_a = "aaaa0000000000000000000000000000";
        let eng_b = "bbbb0000000000000000000000000000";

        // (a) cheap-stale: low solve_time_ms, mtime = epoch_base + 60 s (slightly newer).
        // (b) expensive-old: high solve_time_ms, mtime = epoch_base (slightly older).
        // (d) sentinel in eng_B: untouched by eviction scoped to eng_A.
        let inp_a = "aa00000000000000000000000000aaaa"; // cheap-stale
        let inp_b = "bb00000000000000000000000000bbbb"; // expensive-old
        let inp_d = "dd00000000000000000000000000dddd"; // sentinel in eng_B

        let cheap = ElasticResult {
            solve_time_ms: 1,
            ..make_sample_result()
        };
        let expensive = ElasticResult {
            solve_time_ms: 10_000,
            ..make_sample_result()
        };

        write_entry(root, eng_a, inp_a, &cheap).unwrap();
        write_entry(root, eng_a, inp_b, &expensive).unwrap();
        write_entry(root, eng_b, inp_d, &cheap).unwrap();

        // epoch_base is ~1970 so both entries are "ancient" relative to now.
        // (a) is 60 s newer than (b) — naive LRU evicts (b) first.
        // score(a) = age_a/1 >> score(b) = age_b/10_000 — cost-aware evicts (a) first.
        let epoch_base = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mtime_a = epoch_base + Duration::from_secs(60); // newer mtime
        let mtime_b = epoch_base; // older mtime

        for (inp, mtime) in [(inp_a, mtime_a), (inp_b, mtime_b)] {
            let meta = entry_meta_path(root, eng_a, inp);
            let f = std::fs::File::options()
                .write(true)
                .open(&meta)
                .expect("must open sidecar");
            f.set_times(std::fs::FileTimes::new().set_modified(mtime))
                .expect("must set mtime");
        }

        // Cap = sz(b): cost-aware evicts (a) → remaining = sz(b) = cap → break,
        // leaving (b) intact.  Naive LRU evicts (b) → remaining = sz(a) which may
        // be ≤ cap but assertion "(a) gone" then fails.
        let sz_a = std::fs::metadata(entry_bin_path(root, eng_a, inp_a))
            .unwrap()
            .len();
        let sz_b = std::fs::metadata(entry_bin_path(root, eng_a, inp_b))
            .unwrap()
            .len();
        let cap = sz_b;

        let report = evict_over_cap(root, eng_a, cap).unwrap();

        // (a) cheap-stale must be evicted (higher cost-weighted eviction score).
        assert!(
            !entry_bin_path(root, eng_a, inp_a).exists(),
            "(a) cheap-stale .bin must be evicted (cost-weighted score is higher)"
        );
        assert!(
            !entry_meta_path(root, eng_a, inp_a).exists(),
            "(a) cheap-stale .meta must be removed alongside .bin"
        );

        // (b) expensive-old must survive (lower cost-weighted score despite older mtime).
        assert!(
            entry_bin_path(root, eng_a, inp_b).exists(),
            "(b) expensive-old .bin must survive"
        );
        assert!(
            entry_meta_path(root, eng_a, inp_b).exists(),
            "(b) expensive-old .meta must survive"
        );

        // (d) sentinel in eng_B must be entirely untouched (engine-version scope).
        assert!(
            entry_bin_path(root, eng_b, inp_d).exists(),
            "(d) sentinel in eng_B .bin must be untouched"
        );
        assert!(
            entry_meta_path(root, eng_b, inp_d).exists(),
            "(d) sentinel in eng_B .meta must be untouched"
        );

        assert!(
            report.remaining_bytes <= cap,
            "remaining_bytes {} must be <= cap {}",
            report.remaining_bytes,
            cap
        );

        // Pin the report counters surfaced to `reify cache stats` — prevents a
        // future refactor from silently double-counting or misreporting.
        assert_eq!(
            report.evicted_count, 1,
            "exactly one entry must be evicted (only (a) cheap-stale)"
        );
        assert_eq!(
            report.evicted_bytes, sz_a,
            "evicted_bytes must equal the on-disk size of evicted entry (a)"
        );

        // No orphaned .meta files in eng_A (only (b)'s .meta should remain).
        for shard in std::fs::read_dir(root.join(eng_a)).unwrap() {
            let shard = shard.unwrap();
            for f in std::fs::read_dir(shard.path()).unwrap() {
                let f = f.unwrap();
                if f.path().extension().and_then(|e| e.to_str()) == Some("meta") {
                    let stem = f.path().file_stem().unwrap().to_str().unwrap().to_owned();
                    assert_eq!(
                        stem,
                        inp_b,
                        "unexpected orphaned .meta in eng_A: {:?}",
                        f.path()
                    );
                }
            }
        }
    }

    #[test]
    fn evict_over_cap_reduces_total_bytes_to_at_or_under_cap_when_over_cap() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "cccccccccccccccccccccccccccccc00";
        let inp1 = "aaaa111111111111111111111111aabb";
        let inp2 = "bbbb111111111111111111111111ccdd";
        let inp3 = "cccc111111111111111111111111eeff";

        let v = make_sample_result();
        write_entry(root, eng, inp1, &v).unwrap();
        write_entry(root, eng, inp2, &v).unwrap();
        write_entry(root, eng, inp3, &v).unwrap();

        // Measure one entry size and set cap so only ~1 entry can remain.
        let sz = std::fs::metadata(entry_bin_path(root, eng, inp1))
            .unwrap()
            .len();
        let total = sz * 3;
        // Cap just above one entry → at least two must evict.
        let cap = sz + sz / 2;
        assert!(total > cap, "test precondition: total must exceed cap");

        let report = evict_over_cap(root, eng, cap).unwrap();

        assert!(
            report.remaining_bytes <= cap,
            "remaining_bytes {} must be <= cap {}",
            report.remaining_bytes,
            cap
        );
        assert!(
            report.evicted_count >= 1,
            "at least one entry must have been evicted"
        );
        assert!(
            report.evicted_bytes >= 1,
            "evicted_bytes must be non-zero when entries were evicted"
        );

        // Recompute on-disk total and verify it matches remaining_bytes.
        let on_disk: u64 = [inp1, inp2, inp3]
            .iter()
            .filter_map(|inp| {
                let p = entry_bin_path(root, eng, inp);
                std::fs::metadata(&p).ok().map(|m| m.len())
            })
            .sum();
        assert_eq!(
            on_disk, report.remaining_bytes,
            "on-disk total must equal report.remaining_bytes"
        );
    }

    #[test]
    fn evict_over_cap_returns_zero_evictions_with_correct_remaining_bytes_when_under_cap() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb00";
        let inp1 = "1111111111111111111111111111aabb";
        let inp2 = "2222222222222222222222222222ccdd";

        let v1 = make_sample_result();
        let v2 = make_sample_result();
        write_entry(root, eng, inp1, &v1).unwrap();
        write_entry(root, eng, inp2, &v2).unwrap();

        // Measure the on-disk .bin sizes.
        let bin1 = entry_bin_path(root, eng, inp1);
        let bin2 = entry_bin_path(root, eng, inp2);
        let sz1 = std::fs::metadata(&bin1).unwrap().len();
        let sz2 = std::fs::metadata(&bin2).unwrap().len();
        let total = sz1 + sz2;

        // Cap is well above current total — nothing should be evicted.
        let cap = total + 1024;
        let report = evict_over_cap(root, eng, cap).unwrap();

        assert_eq!(
            report.evicted_count, 0,
            "evicted_count must be 0 when under cap"
        );
        assert_eq!(
            report.evicted_bytes, 0,
            "evicted_bytes must be 0 when under cap"
        );
        assert_eq!(
            report.remaining_bytes, total,
            "remaining_bytes must equal total .bin size: got {} expected {}",
            report.remaining_bytes, total
        );

        // Both .bin and .meta files must still exist.
        assert!(bin1.exists(), ".bin for inp1 must survive under-cap call");
        assert!(bin2.exists(), ".bin for inp2 must survive under-cap call");
        assert!(
            entry_meta_path(root, eng, inp1).exists(),
            ".meta for inp1 must survive"
        );
        assert!(
            entry_meta_path(root, eng, inp2).exists(),
            ".meta for inp2 must survive"
        );
    }

    #[test]
    fn evict_over_cap_returns_zero_evictions_when_engine_version_subdir_is_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        // 32-char hex engine-version hash; no subdir created under root.
        let eng = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0";

        let report = evict_over_cap(root, eng, 1024).unwrap();
        assert_eq!(report.evicted_count, 0, "evicted_count");
        assert_eq!(report.evicted_bytes, 0, "evicted_bytes");
        assert_eq!(report.remaining_bytes, 0, "remaining_bytes");
    }

    /// Verify that a corrupt or zero-byte `.bin` file does not abort the entire
    /// eviction run.  The `read_entry` path already returns `Ok(None)` on a bad
    /// header; the GC path must be at least as forgiving.  A corrupt entry is
    /// treated as maximally evictable (`solve_time_ms = 0`) so it self-heals on
    /// the next GC run.
    #[test]
    fn evict_over_cap_treats_corrupt_bin_as_maximally_evictable_and_does_not_abort() {
        use std::time::{Duration, UNIX_EPOCH};

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "cc00000000000000000000000000cccc";
        let inp_good = "gg00000000000000000000000000gggg"; // well-formed entry
        let inp_bad = "bb00000000000000000000000000bbbb"; // will be overwritten with garbage

        // Write two real entries first so shard dirs exist.
        let good_val = ElasticResult {
            solve_time_ms: 10_000, // expensive — should be last to evict normally
            ..make_sample_result()
        };
        write_entry(root, eng, inp_good, &good_val).unwrap();
        write_entry(root, eng, inp_bad, &make_sample_result()).unwrap();

        // Overwrite the "bad" .bin with a 4-byte truncated body — header decode fails.
        let bad_bin = entry_bin_path(root, eng, inp_bad);
        std::fs::write(&bad_bin, b"\x00\x00\x00\x00").unwrap();

        // Back-date the bad entry's .meta so it is "ancient" relative to now.
        // (The good entry will have a recent mtime.)
        let ancient = UNIX_EPOCH + Duration::from_secs(1_000);
        let bad_meta = entry_meta_path(root, eng, inp_bad);
        {
            let f = std::fs::File::options()
                .write(true)
                .open(&bad_meta)
                .unwrap();
            f.set_times(std::fs::FileTimes::new().set_modified(ancient))
                .expect("must set mtime");
        }

        let sz_good = std::fs::metadata(entry_bin_path(root, eng, inp_good))
            .unwrap()
            .len();
        let sz_bad = std::fs::metadata(&bad_bin).unwrap().len();
        let total = sz_good + sz_bad;

        // Cap = sz_good → exactly one eviction needed.
        let cap = sz_good;
        assert!(total > cap, "test precondition");

        // Must NOT return Err — corrupt header must not abort the eviction run.
        let report = evict_over_cap(root, eng, cap)
            .expect("evict_over_cap must not abort on corrupt .bin header");

        // The corrupt (bad) entry must be evicted (solve_time_ms=0 → maximal score).
        assert!(!bad_bin.exists(), "corrupt .bin must be evicted");

        // The well-formed expensive entry must survive.
        assert!(
            entry_bin_path(root, eng, inp_good).exists(),
            "well-formed expensive .bin must survive"
        );

        assert!(
            report.remaining_bytes <= cap,
            "remaining_bytes {} must be <= cap {}",
            report.remaining_bytes,
            cap
        );
        assert_eq!(report.evicted_count, 1, "exactly one entry evicted");
    }

    /// Verify that the eviction walker skips in-flight `.tmp.*` tempfiles,
    /// orphaned `.meta` files without a companion `.bin`, and files with
    /// non-`.bin` extensions.  If any of these decoys were included,
    /// `remaining_bytes` would be inflated and — in the `.tmp.*` case — a
    /// future eviction could corrupt a concurrent `write_entry` caller's
    /// in-flight tempfile.
    #[test]
    fn evict_over_cap_skips_tmp_prefix_files_orphaned_meta_and_non_bin_extensions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "ff00000000000000000000000000ffff";
        let inp = "ff00000000000000000000000000ffee";

        // Write one real entry so a shard dir exists for dropping decoys into.
        let v = make_sample_result();
        write_entry(root, eng, inp, &v).unwrap();
        let real_bin = entry_bin_path(root, eng, inp);
        let real_sz = std::fs::metadata(&real_bin).unwrap().len();

        // Locate the shard directory and drop three decoy files into it.
        let shard = shard_dir(root, eng, inp);

        // Decoy 1: `.tmp.deadbeef.bin` — has `.bin` extension but `.tmp.` prefix.
        //           Simulates an in-flight `write_entry` tempfile; must never be evicted.
        let decoy_tmp = shard.join(".tmp.deadbeef.bin");
        std::fs::write(&decoy_tmp, b"in-flight").unwrap();

        // Decoy 2: `orphan.meta` — `.meta` extension with no companion `.bin`.
        //           Simulates a leaked sidecar; walker must not count or touch it.
        let decoy_meta = shard.join("orphan.meta");
        std::fs::write(&decoy_meta, b"orphan").unwrap();

        // Decoy 3: `notes.txt` — wrong extension entirely.
        let decoy_txt = shard.join("notes.txt");
        std::fs::write(&decoy_txt, b"notes").unwrap();

        // Cap well above the real entry — nothing should be evicted.
        let cap = real_sz + 4096;
        let report = evict_over_cap(root, eng, cap).unwrap();

        // Only the real .bin is counted; decoys must not inflate remaining_bytes.
        assert_eq!(
            report.remaining_bytes, real_sz,
            "remaining_bytes must equal only the real .bin size; decoys must not be counted"
        );
        assert_eq!(
            report.evicted_count, 0,
            "nothing should be evicted when under cap"
        );
        assert_eq!(
            report.evicted_bytes, 0,
            "evicted_bytes must be 0 when under cap"
        );

        // All decoy files must be untouched.
        assert!(
            decoy_tmp.exists(),
            ".tmp.* in-flight file must not be touched"
        );
        assert!(
            decoy_meta.exists(),
            "orphaned .meta file must not be touched"
        );
        assert!(decoy_txt.exists(), "non-.bin file must not be touched");
    }

    /// Step-11 RED: verify that `evict_over_cap` does NOT propagate
    /// `io::ErrorKind::NotFound` when a `.bin` entry has no companion `.meta`
    /// sidecar, simulating the `write_entry` crash window where `.bin` was
    /// persisted but `write_sidecar` was not yet called.
    ///
    /// When the sidecar is absent the `.bin` file's **own mtime** should be
    /// used as the LRU signal.  Under step-8's naive impl, `read_sidecar_mtime`
    /// propagates `NotFound` — so the call returns `Err` rather than `Ok`.
    /// This test surfaces that gap.
    #[test]
    fn evict_over_cap_uses_bin_mtime_as_lru_fallback_when_meta_sidecar_is_absent() {
        use std::time::{Duration, UNIX_EPOCH};

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "ee00000000000000000000000000eeee";
        let inp_orphan = "oo00000000000000000000000000oooo"; // orphan: no sidecar
        let inp_normal = "nn00000000000000000000000000nnnn"; // normal: sidecar present

        let v = make_sample_result();
        write_entry(root, eng, inp_orphan, &v).unwrap();
        write_entry(root, eng, inp_normal, &v).unwrap();

        // Remove the orphan's sidecar to simulate a crash between write_entry's
        // persist(.bin) step and its write_sidecar(.meta) step.
        let meta_orphan = entry_meta_path(root, eng, inp_orphan);
        std::fs::remove_file(&meta_orphan).expect("orphan .meta must be present to remove");

        // Back-date the orphan .bin mtime so it is the oldest on disk and will
        // be chosen for eviction (highest LRU score via .bin mtime fallback).
        let ancient = UNIX_EPOCH + Duration::from_secs(1_000);
        let bin_orphan = entry_bin_path(root, eng, inp_orphan);
        {
            let f = std::fs::File::options()
                .write(true)
                .open(&bin_orphan)
                .unwrap();
            f.set_times(std::fs::FileTimes::new().set_modified(ancient))
                .expect("must set .bin mtime");
        }

        // Measure sizes; set cap so only one entry can survive.
        let sz_orphan = std::fs::metadata(&bin_orphan).unwrap().len();
        let sz_normal = std::fs::metadata(entry_bin_path(root, eng, inp_normal))
            .unwrap()
            .len();
        let total = sz_orphan + sz_normal;
        // cap = sz_normal → exactly one eviction required.
        let cap = sz_normal;
        assert!(total > cap, "test precondition: total must exceed cap");

        // MUST NOT return Err — NotFound from absent .meta must be caught.
        let report = evict_over_cap(root, eng, cap)
            .expect("evict_over_cap must not propagate NotFound when .meta is absent");

        // The orphan .bin (oldest by file mtime) must be the one evicted.
        assert!(
            !bin_orphan.exists(),
            "orphan .bin (oldest by .bin mtime) must be evicted"
        );

        // Normal entry must survive.
        assert!(
            entry_bin_path(root, eng, inp_normal).exists(),
            "normal .bin must survive"
        );

        assert!(
            report.remaining_bytes <= cap,
            "remaining_bytes {} must be ≤ cap {}",
            report.remaining_bytes,
            cap
        );
        assert_eq!(report.evicted_count, 1, "exactly one entry must be evicted");
    }

    // Shared fixture for both dangling-symlink race-site tests below.
    //
    // with_meta = true  → race site #2: sidecar present, File::open(.bin) returns NotFound
    // with_meta = false → race site #1: sidecar absent, metadata(.bin) returns NotFound
    //
    // Creates a temp-dir with three hash entries for `eng`:
    //   inp1 — phantom: shard dir + dangling .bin symlink + optional .meta
    //   inp2, inp3 — real entries written via write_entry
    // Sets cap = size_of(inp2) (one entry), calls evict_over_cap, and asserts
    // the four shared invariants (a)–(d).
    #[cfg(unix)]
    fn run_dangling_symlink_race_scenario(
        with_meta: bool,
        eng: &str,
        inp1: &str,
        inp2: &str,
        inp3: &str,
    ) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        let v = make_sample_result();
        write_entry(root, eng, inp2, &v).unwrap();
        write_entry(root, eng, inp3, &v).unwrap();

        // Measure size from a real entry (all entries written with the same
        // value, so they serialize to identical sizes).
        let sz = std::fs::metadata(entry_bin_path(root, eng, inp2))
            .unwrap()
            .len();

        // Plant inp1 to simulate the concurrent-removal race:
        //   1. Create the shard dir (same layout as write_entry).
        //   2. Conditionally create a .meta sidecar: with_meta = true plants a
        //      real .meta so read_sidecar_mtime succeeds (race window is
        //      File::open of .bin); with_meta = false leaves no .meta so the
        //      sidecar-absent fallback path is exercised instead.
        //   3. Create a dangling symlink as .bin: read_dir sees the entry, but
        //      the subsequent open/metadata call follows the symlink and returns
        //      ENOENT (NotFound) because the target does not exist.
        let shard1 = shard_dir(root, eng, inp1);
        std::fs::create_dir_all(&shard1).unwrap();
        if with_meta {
            std::fs::write(entry_meta_path(root, eng, inp1), b"").unwrap();
        }
        let bin1 = entry_bin_path(root, eng, inp1);
        std::os::unix::fs::symlink("/nonexistent/reify-race-simulation", &bin1).unwrap();

        // cap = sz (one entry): walker skips the dangling-symlink inp1 and
        // observes two real candidates (inp2, inp3) summing to 2*sz > cap,
        // so exactly one must be evicted to bring remaining ≤ cap.
        let cap = sz;

        // (a) Must not propagate Err(NotFound).
        let report = evict_over_cap(root, eng, cap)
            .expect("evict_over_cap must not propagate NotFound on dangling-symlink phantom entry");

        // (b) Exactly one eviction — the phantom inp1 was skipped, not counted.
        assert_eq!(report.evicted_count, 1, "evicted_count must be 1");

        // (c) evicted_bytes must equal the size of the one real .bin removed.
        assert_eq!(
            report.evicted_bytes, sz,
            "evicted_bytes must equal one entry's size"
        );

        // (d) On-disk .bin total (metadata() on a dangling symlink returns
        //     NotFound and is filtered out by .ok()) must be ≤ cap.
        let on_disk: u64 = [inp1, inp2, inp3]
            .iter()
            .filter_map(|inp| {
                std::fs::metadata(entry_bin_path(root, eng, inp))
                    .ok()
                    .map(|m| m.len())
            })
            .sum();
        assert_eq!(
            on_disk, cap,
            "on-disk .bin total {} must equal cap {} (dangling symlink filtered; exactly one real entry survives)",
            on_disk, cap
        );
    }

    /// Step-1 RED: verify that `evict_over_cap` tolerates `NotFound` in the
    /// candidate-walker loop when a concurrent reify-process has removed a
    /// candidate's `.bin` between the `read_dir` scan and the `File::open` at
    /// line 1390.  The `.meta` is left in place to exercise the primary race
    /// window: `read_sidecar_mtime` succeeds (sidecar present) but the
    /// subsequent `File::open(&file_path)?` returns `NotFound`.
    ///
    /// To reproduce the race from a single-threaded test we plant inp1 as a
    /// dangling symlink: `read_dir` lists it (the directory entry exists) but
    /// `File::open` follows the symlink and returns `NotFound` because the
    /// target does not exist — the same `ENOENT` the kernel surfaces when
    /// another process deletes the file between `readdir(3)` and `open(2)`.
    ///
    /// The current impl propagates `Err(NotFound)` at that `?`; after the fix
    /// the walker continues to the next `file_entry`.
    #[cfg(unix)]
    #[test]
    fn evict_over_cap_tolerates_notfound_in_candidate_walker_when_bin_concurrently_removed() {
        run_dangling_symlink_race_scenario(
            true, // with_meta: race site #2 — sidecar present, File::open returns NotFound
            "ff00000000000000000000000000ffff",
            "cc00000000000000000000000000cccc",
            "dd00000000000000000000000000dddd",
            "ee00000000000000000000000000eeee",
        );
    }

    /// Amendment (suggestion 3): verify that `evict_over_cap` tolerates
    /// `NotFound` from `std::fs::metadata(&file_path)` in the sidecar-absent
    /// fallback path (race site #1).
    ///
    /// When the sidecar (`.meta`) is absent, the walker falls back to the `.bin`
    /// mtime via `std::fs::metadata(&file_path)`.  If the `.bin` has also been
    /// concurrently removed (simulated here with a dangling symlink so that
    /// `metadata()` — which follows symlinks — returns `NotFound`), the new
    /// `continue` arm at race site #1 must fire, skipping the phantom entry
    /// rather than propagating the error.
    ///
    /// This complements step-1 (`evict_over_cap_tolerates_notfound_in_candidate_walker_when_bin_concurrently_removed`),
    /// which covers race site #2 (sidecar present, `File::open` returns NotFound).
    /// Here the sidecar is intentionally absent, exercising the
    /// `Err(e) if e.kind() == io::ErrorKind::NotFound => continue` arm inside
    /// the sidecar-absent fallback of `evict_over_cap`'s candidate walker.
    #[cfg(unix)]
    #[test]
    fn evict_over_cap_tolerates_notfound_when_both_meta_and_bin_concurrently_removed() {
        run_dangling_symlink_race_scenario(
            false, // with_meta: race site #1 — sidecar absent, metadata() returns NotFound
            "gg00000000000000000000000000gggg",
            "hh00000000000000000000000000hhhh",
            "ii00000000000000000000000000iiii",
            "jj00000000000000000000000000jjjj",
        );
    }

    /// Step-3 RED: verify that `evict_over_cap` prunes the two-char shard dir
    /// after evicting ALL entries from it, reclaiming dirs that would otherwise
    /// accumulate forever as the cache turns over.
    ///
    /// Two input hashes sharing the same two-character prefix are used so they
    /// hash into the same shard dir.  `cap = 0` forces the eviction loop to
    /// remove every entry, fully draining the shard.  After the call the shard
    /// dir must not exist on disk, but the engine-version subdir must survive
    /// (pruning the version dir is the startup-sweep task's concern, not this
    /// function's).
    ///
    /// Test must FAIL on current main because no shard-dir cleanup is wired in.
    #[test]
    fn evict_over_cap_prunes_empty_shard_dir_after_evicting_all_entries_from_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "aa00000000000000000000000000aaaa";
        // Both hashes share "aa" prefix → same shard dir.
        let inp1 = "aa00000000000000000000000000aabb";
        let inp2 = "aa00000000000000000000000000aacc";

        let v = make_sample_result();
        write_entry(root, eng, inp1, &v).unwrap();
        write_entry(root, eng, inp2, &v).unwrap();

        // Precondition: shard dir must exist before the call.
        let shard = shard_dir(root, eng, inp1);
        assert!(
            shard.exists(),
            "shard dir must exist before evict_over_cap (test precondition)"
        );
        // Both hashes share the same shard dir.
        assert_eq!(
            shard,
            shard_dir(root, eng, inp2),
            "inp1 and inp2 must share the same shard dir"
        );

        // cap = 0 → evict every entry, fully draining the shard.
        let report = evict_over_cap(root, eng, 0)
            .expect("evict_over_cap must return Ok when draining the cache");

        // (a) Ok — already asserted above via .expect()
        // (b) Both entries evicted by THIS call.
        assert_eq!(report.evicted_count, 2, "evicted_count must be 2");
        // (c) No bytes remain.
        assert_eq!(report.remaining_bytes, 0, "remaining_bytes must be 0");
        // (d) The shared shard dir must have been pruned.
        assert!(
            !shard.exists(),
            "shard dir must be pruned after all entries evicted"
        );
        // (e) Engine-version subdir must survive — pruning the version dir is
        //     owned by the startup-sweep task (cross-version orphan pruning),
        //     NOT this function.
        assert!(
            root.join(eng).exists(),
            "engine-version subdir must survive after shard-dir pruning"
        );
    }

    /// Amendment (suggestion 4): verify that `evict_over_cap` does NOT prune
    /// the shard dir when entries remain in it after a partial eviction.
    ///
    /// Three entries share the same two-character shard prefix so they all land
    /// in the same shard dir.  The cap is set so that exactly one entry is evicted
    /// (cap = 2 × entry_size; after evicting one the remaining 2 × entry_size ≤ cap
    /// triggers the break).  After the call the shard dir must still exist and must
    /// contain the two surviving `.bin` + `.meta` pairs — locking in the
    /// `DirectoryNotEmpty` branch and preventing accidental `force_remove_dir_all`
    /// regressions.
    #[test]
    fn evict_over_cap_preserves_shard_dir_when_entries_remain_in_it() {
        use std::time::{Duration, UNIX_EPOCH};

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "dd00000000000000000000000000dddd";
        // All three hashes share "bb" prefix → same shard dir.
        let inp1 = "bb01000000000000000000000000bb11";
        let inp2 = "bb02000000000000000000000000bb22";
        let inp3 = "bb03000000000000000000000000bb33";

        let v = make_sample_result();
        write_entry(root, eng, inp1, &v).unwrap();
        write_entry(root, eng, inp2, &v).unwrap();
        write_entry(root, eng, inp3, &v).unwrap();

        // All entries are written with the same value → identical file size.
        let sz = std::fs::metadata(entry_bin_path(root, eng, inp1))
            .unwrap()
            .len();
        let total = 3 * sz;

        // Back-date inp1's .meta so it is oldest and will be evicted first
        // (highest eviction score: max age / same solve_time_ms).
        let ancient = UNIX_EPOCH + Duration::from_secs(1_000);
        let meta1 = entry_meta_path(root, eng, inp1);
        {
            let f = std::fs::File::options().write(true).open(&meta1).unwrap();
            f.set_times(std::fs::FileTimes::new().set_modified(ancient))
                .expect("must set .meta mtime for inp1");
        }

        // cap = 2*sz: total (3*sz) > cap → evict one; remaining (2*sz) = cap → stop.
        let cap = 2 * sz;
        assert!(total > cap, "test precondition: total must exceed cap");

        let shard = shard_dir(root, eng, inp1);
        assert!(
            shard.exists(),
            "shard dir must exist before call (test precondition)"
        );
        // All three share the same shard.
        assert_eq!(shard, shard_dir(root, eng, inp2), "inp2 same shard as inp1");
        assert_eq!(shard, shard_dir(root, eng, inp3), "inp3 same shard as inp1");

        let report = evict_over_cap(root, eng, cap)
            .expect("evict_over_cap must return Ok for partial eviction");

        // Exactly one entry evicted (the oldest, inp1).
        assert_eq!(report.evicted_count, 1, "evicted_count must be 1");
        assert_eq!(
            report.remaining_bytes,
            2 * sz,
            "remaining_bytes must equal 2 surviving entries"
        );

        // Shard dir must still exist because two entries remain inside it.
        assert!(
            shard.exists(),
            "shard dir must be preserved when entries remain in it"
        );

        // Surviving entries (inp2, inp3) must still have both .bin and .meta.
        assert!(
            entry_bin_path(root, eng, inp2).exists(),
            "inp2 .bin must survive"
        );
        assert!(
            entry_meta_path(root, eng, inp2).exists(),
            "inp2 .meta must survive"
        );
        assert!(
            entry_bin_path(root, eng, inp3).exists(),
            "inp3 .bin must survive"
        );
        assert!(
            entry_meta_path(root, eng, inp3).exists(),
            "inp3 .meta must survive"
        );

        // Evicted entry (inp1) must no longer have a .bin on disk.
        assert!(
            !entry_bin_path(root, eng, inp1).exists(),
            "inp1 .bin must be removed"
        );
    }

    // ── evict_over_cap tracing tests ─────────────────────────────────────────

    /// Step-1 RED: `evict_over_cap` must emit exactly one INFO event at the
    /// `reify_eval::persistent_cache::gc` target when eviction actually runs,
    /// with message `"evict_over_cap complete"` and structured fields
    /// `evicted_count`, `evicted_bytes`, `remaining_bytes`, `cap_bytes`,
    /// `engine_version_hash`.
    ///
    /// Fails RED before the `tracing::info!` site is added in step-2.
    #[test]
    fn evict_over_cap_emits_info_summary_with_expected_fields() {
        reify_test_support::prime_tracing_callsite_cache();

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "dddddddddddddddddddddddddddddd00";
        let inp1 = "1111111111111111111111111111aaaa";
        let inp2 = "2222222222222222222222222222bbbb";
        let inp3 = "3333333333333333333333333333cccc";

        let v = make_sample_result();
        write_entry(root, eng, inp1, &v).unwrap();
        write_entry(root, eng, inp2, &v).unwrap();
        write_entry(root, eng, inp3, &v).unwrap();

        // Measure one entry size; set cap to evict at least 2 entries.
        let sz = std::fs::metadata(entry_bin_path(root, eng, inp1))
            .unwrap()
            .len();
        let cap = sz + sz / 2;

        let (subscriber, capture) =
            reify_test_support::CapturingSubscriberBuilder::new(tracing::Level::INFO)
                .target_prefix("reify_eval::persistent_cache::gc")
                .build();

        let report = tracing::subscriber::with_default(subscriber, || {
            evict_over_cap(root, eng, cap).unwrap()
        });

        assert!(
            report.evicted_count >= 1,
            "test precondition: at least 1 entry must have been evicted"
        );

        let msgs = capture.messages();
        assert_eq!(
            msgs.len(),
            1,
            "expected exactly 1 INFO event at reify_eval::persistent_cache::gc target; got {}",
            msgs.len()
        );
        assert_eq!(
            msgs[0], "evict_over_cap complete",
            "INFO event message mismatch"
        );

        let all_fields = capture.fields_by_event();
        let f = &all_fields[0];

        assert!(
            f.contains_key("evicted_count"),
            "field 'evicted_count' missing from INFO event; got: {f:?}"
        );
        assert!(
            f.contains_key("evicted_bytes"),
            "field 'evicted_bytes' missing from INFO event; got: {f:?}"
        );
        assert!(
            f.contains_key("remaining_bytes"),
            "field 'remaining_bytes' missing from INFO event; got: {f:?}"
        );
        assert!(
            f.contains_key("cap_bytes"),
            "field 'cap_bytes' missing from INFO event; got: {f:?}"
        );
        assert!(
            f.contains_key("engine_version_hash"),
            "field 'engine_version_hash' missing from INFO event; got: {f:?}"
        );

        // Verify field values match the report and the inputs.
        assert_eq!(
            f["evicted_count"],
            report.evicted_count.to_string(),
            "evicted_count field value mismatch"
        );
        assert_eq!(
            f["evicted_bytes"],
            report.evicted_bytes.to_string(),
            "evicted_bytes field value mismatch"
        );
        assert_eq!(
            f["remaining_bytes"],
            report.remaining_bytes.to_string(),
            "remaining_bytes field value mismatch"
        );
        assert_eq!(
            f["cap_bytes"],
            cap.to_string(),
            "cap_bytes field value mismatch"
        );
        assert_eq!(
            f["engine_version_hash"], eng,
            "engine_version_hash field value mismatch"
        );
    }

    /// Step-3 RED: `evict_over_cap` must emit exactly one DEBUG event at the
    /// `reify_eval::persistent_cache::gc` target for each entry that was
    /// actually evicted by THIS invocation (i.e. `we_removed_bin == true`).
    ///
    /// Setup: 3 entries, backdated so LRU order is deterministic (inp1 is
    /// oldest); cap set to evict exactly 2 entries.  Expected DEBUG count == 2.
    /// Also pins `report.evicted_count == 2` to anchor the count equality.
    ///
    /// Fails RED before the `tracing::debug!` site is added in step-4.
    #[test]
    fn evict_over_cap_debug_count_equals_evicted_count() {
        reify_test_support::prime_tracing_callsite_cache();
        use reify_test_support::CountingSubscriberBuilder;
        use std::sync::atomic::Ordering;

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeee00";
        let inp1 = "1111111111111111111111111111dddd";
        let inp2 = "2222222222222222222222222222eeee";
        let inp3 = "3333333333333333333333333333ffff";

        let v = make_sample_result();
        write_entry(root, eng, inp1, &v).unwrap();
        write_entry(root, eng, inp2, &v).unwrap();
        write_entry(root, eng, inp3, &v).unwrap();

        // Backdate inp1 and inp2 metas so they score highest (oldest, cheapest)
        // and are evicted before inp3.
        backdate_mtime(&entry_meta_path(root, eng, inp1), 7200);
        backdate_mtime(&entry_meta_path(root, eng, inp2), 3600);

        let sz = std::fs::metadata(entry_bin_path(root, eng, inp1))
            .unwrap()
            .len();

        // Cap: allow exactly 1 entry to remain → evict exactly 2.
        let cap = sz + sz / 2;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::DEBUG)
            .target_prefix("reify_eval::persistent_cache::gc")
            .build();
        let debug_count = counters[&tracing::Level::DEBUG].clone();

        let report = tracing::subscriber::with_default(subscriber, || {
            evict_over_cap(root, eng, cap).unwrap()
        });

        assert_eq!(
            report.evicted_count, 2,
            "test precondition: exactly 2 entries must have been evicted"
        );
        assert_eq!(
            debug_count.load(Ordering::Acquire),
            2,
            "expected exactly 2 DEBUG events at reify_eval::persistent_cache::gc \
             (one per actually-evicted entry); got {}",
            debug_count.load(Ordering::Acquire)
        );
    }

    /// Regression guard: `evict_over_cap` must NOT emit any event at the
    /// `reify_eval::persistent_cache::gc` target for either silent early-
    /// return path:
    ///
    /// - Sub-scenario A: two entries totalling under cap → under-cap fast-path
    ///   returns without evicting anything.
    /// - Sub-scenario B: engine-version subdir entirely absent → first early
    ///   return at the top of the function.
    ///
    /// Both INFO and DEBUG counters must stay at 0 for both paths.  Pins the
    /// negative-space contract defined in design-decision #1 and prevents
    /// future regressions that would add an info!/debug! at the wrong site.
    ///
    /// This guard test passes immediately against the step-2 + step-4 impl
    /// because those sites were deliberately placed at the post-loop return
    /// only.
    #[test]
    fn evict_over_cap_silent_on_under_cap_and_absent_subdir() {
        reify_test_support::prime_tracing_callsite_cache();
        use reify_test_support::CountingSubscriberBuilder;
        use std::sync::atomic::Ordering;

        // ── Sub-scenario A: under-cap fast path ─────────────────────────────
        {
            let tmp = tempfile::TempDir::new().unwrap();
            let root = tmp.path();
            let eng = "ffffffffffffffffffffffffffffffff";
            let inp1 = "1111111111111111111111111111aaaa";
            let inp2 = "2222222222222222222222222222bbbb";

            let v = make_sample_result();
            write_entry(root, eng, inp1, &v).unwrap();
            write_entry(root, eng, inp2, &v).unwrap();

            let sz1 = std::fs::metadata(entry_bin_path(root, eng, inp1))
                .unwrap()
                .len();
            let sz2 = std::fs::metadata(entry_bin_path(root, eng, inp2))
                .unwrap()
                .len();
            // Cap well above total → under-cap fast path, no eviction.
            let cap = sz1 + sz2 + 1024;

            let (subscriber, counters) = CountingSubscriberBuilder::new()
                .count_level(tracing::Level::INFO)
                .count_level(tracing::Level::DEBUG)
                .target_prefix("reify_eval::persistent_cache::gc")
                .build();
            let info_count = counters[&tracing::Level::INFO].clone();
            let debug_count = counters[&tracing::Level::DEBUG].clone();

            let report = tracing::subscriber::with_default(subscriber, || {
                evict_over_cap(root, eng, cap).unwrap()
            });

            assert_eq!(
                report.evicted_count, 0,
                "sub-scenario A: no eviction should occur under cap"
            );
            assert_eq!(
                info_count.load(Ordering::Acquire),
                0,
                "sub-scenario A: under-cap fast path must emit NO INFO events \
                 at reify_eval::persistent_cache::gc"
            );
            assert_eq!(
                debug_count.load(Ordering::Acquire),
                0,
                "sub-scenario A: under-cap fast path must emit NO DEBUG events \
                 at reify_eval::persistent_cache::gc"
            );
        }

        // ── Sub-scenario B: absent engine-version subdir ─────────────────────
        {
            let tmp = tempfile::TempDir::new().unwrap();
            let root = tmp.path();
            // 32-char hex engine hash; no subdir created under root.
            let eng = "0000000000000000000000000000dead";

            let (subscriber, counters) = CountingSubscriberBuilder::new()
                .count_level(tracing::Level::INFO)
                .count_level(tracing::Level::DEBUG)
                .target_prefix("reify_eval::persistent_cache::gc")
                .build();
            let info_count = counters[&tracing::Level::INFO].clone();
            let debug_count = counters[&tracing::Level::DEBUG].clone();

            let report = tracing::subscriber::with_default(subscriber, || {
                evict_over_cap(root, eng, 1024).unwrap()
            });

            assert_eq!(
                report.evicted_count, 0,
                "sub-scenario B: no eviction should occur when subdir is absent"
            );
            assert_eq!(
                info_count.load(Ordering::Acquire),
                0,
                "sub-scenario B: absent-subdir early-return must emit NO INFO events \
                 at reify_eval::persistent_cache::gc"
            );
            assert_eq!(
                debug_count.load(Ordering::Acquire),
                0,
                "sub-scenario B: absent-subdir early-return must emit NO DEBUG events \
                 at reify_eval::persistent_cache::gc"
            );
        }
    }

    // ── startup-sweep tests ──────────────────────────────────────────────────

    /// Step-1 RED: `sweep_stale_tempfiles` removes only old `.tmp.*` files.
    ///
    /// Fixture: a shard dir containing
    /// * a fresh `.tmp.fresh` file (mtime ≈ now),
    /// * an old `.tmp.old` file (backdated well past `STALE_TEMPFILE_AGE`),
    /// * a `.bin` and `.meta` pair also backdated past the threshold (but
    ///   NOT `.tmp.*`-prefixed, so they must survive).
    ///
    /// After `sweep_stale_tempfiles(cache_root)`:
    /// * `.tmp.old` is gone,
    /// * `.tmp.fresh`, `.bin`, `.meta` all remain,
    /// * `SweepReport { tempfiles_removed: 1, orphan_dirs_removed: 0 }`.
    #[test]
    fn sweep_stale_tempfiles_removes_only_old_tmp_files_not_bin_or_meta() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let eng = "aabb000000000000000000000000aabb";
        let inp = "ccdd111111111111111111111111ccdd";

        let sd = shard_dir(root, eng, inp);
        std::fs::create_dir_all(&sd).unwrap();

        // Fresh .tmp file — mtime = now, must NOT be swept.
        let fresh = sd.join(".tmp.fresh");
        std::fs::write(&fresh, b"in-flight write").unwrap();

        // Old .tmp file — backdated past threshold, must be swept.
        let old_tmp = sd.join(".tmp.old");
        std::fs::write(&old_tmp, b"crashed-writer leftover").unwrap();
        backdate_mtime(&old_tmp, STALE_TEMPFILE_AGE.as_secs() + 60);

        // Old .bin + .meta pair — old but NOT .tmp.* prefixed → must survive.
        let bin = entry_bin_path(root, eng, inp);
        let meta = entry_meta_path(root, eng, inp);
        std::fs::write(&bin, b"bin-data").unwrap();
        std::fs::write(&meta, b"meta-data").unwrap();
        backdate_mtime(&bin, STALE_TEMPFILE_AGE.as_secs() + 60);
        backdate_mtime(&meta, STALE_TEMPFILE_AGE.as_secs() + 60);

        let report = sweep_stale_tempfiles(root);

        assert!(
            !old_tmp.exists(),
            ".tmp.old must be removed by sweep_stale_tempfiles"
        );
        assert!(fresh.exists(), ".tmp.fresh must survive (mtime is recent)");
        assert!(
            bin.exists(),
            ".bin must survive regardless of age (not a .tmp.* file)"
        );
        assert!(
            meta.exists(),
            ".meta must survive regardless of age (not a .tmp.* file)"
        );
        assert_eq!(
            report.tempfiles_removed, 1,
            "SweepReport.tempfiles_removed must be 1"
        );
        assert_eq!(
            report.orphan_dirs_removed, 0,
            "sweep_stale_tempfiles must not touch dirs (orphan_dirs_removed == 0)"
        );
    }

    // ── step-3 defensiveness tests ───────────────────────────────────────────

    /// Step-3(a): `sweep_stale_tempfiles` on a non-existent `cache_root`
    /// returns `SweepReport::default()` without panicking or returning Err.
    #[test]
    fn sweep_stale_tempfiles_absent_cache_root_returns_default_no_panic() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Point to a path that definitely doesn't exist.
        let nonexistent = tmp.path().join("no_such_cache_root");
        let report = sweep_stale_tempfiles(&nonexistent);
        assert_eq!(
            report,
            SweepReport::default(),
            "absent cache_root must yield SweepReport::default()"
        );
    }

    /// Step-3(b): `sweep_stale_tempfiles` on an empty `cache_root` returns a
    /// zeroed report and makes no filesystem changes.
    #[test]
    fn sweep_stale_tempfiles_empty_cache_root_returns_default_no_fs_change() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let report = sweep_stale_tempfiles(root);
        assert_eq!(
            report,
            SweepReport::default(),
            "empty cache_root must yield SweepReport::default()"
        );
        // The tempdir itself must still exist.
        assert!(root.exists(), "empty cache_root directory must still exist");
    }

    /// Step-3(c): an old `.tmp.*` file placed inside a non-current
    /// engine-version subdir (orphan subdir) is swept by
    /// `sweep_stale_tempfiles` — proves the walker recurses across the whole
    /// subtree, not just the live subdir.
    ///
    /// Layout: `cache_root/<orphan_eng_ver>/ab/.tmp.x` (backdated > threshold).
    #[test]
    fn sweep_stale_tempfiles_recurses_into_orphan_engine_version_subdirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Use a hash that is NOT the live ENGINE_VERSION_HASH.
        let orphan_eng = "dead000000000000000000000000dead";
        let inp = "abcd111111111111111111111111abcd";

        let sd = shard_dir(root, orphan_eng, inp);
        std::fs::create_dir_all(&sd).unwrap();

        let stale_tmp = sd.join(".tmp.x");
        std::fs::write(&stale_tmp, b"orphan-shard-crash-leftover").unwrap();
        backdate_mtime(&stale_tmp, STALE_TEMPFILE_AGE.as_secs() + 120);

        let report = sweep_stale_tempfiles(root);

        assert!(
            !stale_tmp.exists(),
            ".tmp.x inside an orphan engine-version subdir must be swept"
        );
        assert_eq!(
            report.tempfiles_removed, 1,
            "tempfiles_removed must be 1 for the file in the orphan subdir"
        );
        assert_eq!(report.orphan_dirs_removed, 0);
    }

    /// Clock-skew defensiveness: entries whose mtime is in the **future**
    /// relative to `now` cause `now.duration_since(mtime)` to return
    /// `Err(SystemTimeError)`. Both sweep functions guard this with
    /// `Err(_) => continue` (the `Err(_) => continue` arm in
    /// `sweep_stale_tempfiles_recursive`; the matching `Err(_) => continue`
    /// arm in the candidate loop of `prune_orphan_engine_version_dirs`),
    /// keeping the entry rather than treating it as stale.
    ///
    /// This test pins that conservative-keep behavior. If either `Err(_) =>
    /// continue` branch were changed to treat the error as "stale" (e.g.
    /// `Err(_) => Duration::from_secs(u64::MAX)`), the file and directory below
    /// would be removed and both assertions would fail.
    #[test]
    fn sweep_keeps_entries_with_future_mtime_under_clock_skew() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Use a hash distinct from ENGINE_VERSION_HASH so the orphan dir is a
        // genuine prune candidate (name != current).
        let orphan_eng = "dead000000000000000000000000dead";
        let inp = "abcd111111111111111111111111abcd";

        // Create a .tmp.* file inside the orphan shard dir.
        let sd = shard_dir(root, orphan_eng, inp);
        std::fs::create_dir_all(&sd).unwrap();
        let future_tmp = sd.join(".tmp.fresh_skew");
        std::fs::write(&future_tmp, b"in-flight write from the future").unwrap();
        // Forward-date well past STALE_TEMPFILE_AGE: if the function treated
        // future mtimes as stale this file would be removed.
        forward_mtime(&future_tmp, STALE_TEMPFILE_AGE.as_secs() + 60);

        // Forward-date the orphan engine-version dir itself well past
        // ORPHAN_DIR_AGE: if the function treated future mtimes as stale this
        // dir would be pruned.
        // NOTE: this call must come AFTER creating the shard dir and writing the
        // tmp file above — those fs operations update orphan_dir's mtime to ~now,
        // which would make it look fresh (age <= ORPHAN_DIR_AGE) and let it
        // survive for the wrong reason, masking any regression in the Err(_)
        // branch.
        let orphan_dir = root.join(orphan_eng);
        forward_mtime(&orphan_dir, ORPHAN_DIR_AGE.as_secs() + 60);
        // Defensive re-stat: confirm forward_mtime actually advanced the
        // directory's mtime well into the future (at least ORPHAN_DIR_AGE/2
        // past now).  A bare `mtime > now` check would pass even if
        // File::set_times silently updated the dir mtime to ~now rather than
        // the intended now + ORPHAN_DIR_AGE + 60s — the failure mode on some
        // tmpfs/overlayfs configs where set_times on directory FDs is a
        // no-op.  prune_orphan_engine_version_dirs would then see the dir as
        // fresh (age <= ORPHAN_DIR_AGE → continue), passing the test for the
        // wrong reason and masking any regression in the Err(_) branch.
        let threshold = std::time::SystemTime::now()
            .checked_add(ORPHAN_DIR_AGE / 2)
            .expect("ORPHAN_DIR_AGE/2 overflow is astronomically unlikely");
        assert!(
            std::fs::metadata(&orphan_dir).unwrap().modified().unwrap() > threshold,
            "forward_mtime must advance orphan_dir's mtime by more than \
             ORPHAN_DIR_AGE/2 (≈15 days) past `now`; a bare `mtime > now` \
             passes even when set_times silently updates the dir mtime to ~now \
             (the no-op case on some tmpfs/overlayfs configs), masking \
             regressions in the `Err(_) => continue` branch of \
             prune_orphan_engine_version_dirs"
        );

        // Use a current version distinct from orphan_eng so orphan_dir is
        // considered a candidate by prune_orphan_engine_version_dirs.
        let current = "cccc000000000000000000000000cccc";

        let report_a = sweep_stale_tempfiles(root);
        assert_eq!(
            report_a,
            SweepReport::default(),
            "sweep_stale_tempfiles must not remove a .tmp.* file with a future mtime (clock skew)"
        );
        assert!(
            future_tmp.exists(),
            ".tmp.fresh_skew must survive: future mtime → Err(_) → keep branch"
        );

        let report_b = prune_orphan_engine_version_dirs(root, current);
        assert_eq!(
            report_b,
            SweepReport::default(),
            "prune_orphan_engine_version_dirs must not prune a dir with a future mtime (clock skew)"
        );
        assert!(
            orphan_dir.exists(),
            "orphan dir must survive: future mtime → Err(_) → keep branch"
        );
    }

    // ── prune_orphan_engine_version_dirs core behavior (step-5) ─────────────

    /// Step-5 RED: core behavior of `prune_orphan_engine_version_dirs`.
    ///
    /// Fixture: `cache_root` with three immediate subdirs:
    /// * `"old_orphan"` — backdated `> ORPHAN_DIR_AGE`, contains a file → must
    ///   be removed recursively.
    /// * `"fresh_orphan"` — recent mtime → must be kept.
    /// * one subdir named exactly equal to `current` and backdated `> 30d` →
    ///   must be kept (never prune the live cache dir).
    ///
    /// After the call:
    /// * `old_orphan` is gone (recursively),
    /// * `fresh_orphan` and the current-named subdir both remain,
    /// * `SweepReport { orphan_dirs_removed: 1, tempfiles_removed: 0 }`.
    #[test]
    fn prune_orphan_engine_version_dirs_removes_old_orphan_keeps_fresh_and_current() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let current = "cccc000000000000000000000000cccc";

        // old_orphan: old dir with a file inside → must be pruned recursively.
        let old_orphan = root.join("aaaa111111111111111111111111aaaa");
        std::fs::create_dir_all(&old_orphan).unwrap();
        std::fs::write(old_orphan.join("some_entry.bin"), b"data").unwrap();
        backdate_mtime(&old_orphan, ORPHAN_DIR_AGE.as_secs() + 60);

        // fresh_orphan: recent mtime → must be kept.
        let fresh_orphan = root.join("bbbb222222222222222222222222bbbb");
        std::fs::create_dir_all(&fresh_orphan).unwrap();
        // mtime = now (just created)

        // current subdir: old mtime, but must NEVER be pruned.
        let current_dir = root.join(current);
        std::fs::create_dir_all(&current_dir).unwrap();
        backdate_mtime(&current_dir, ORPHAN_DIR_AGE.as_secs() + 60);

        let report = prune_orphan_engine_version_dirs(root, current);

        assert!(
            !old_orphan.exists(),
            "old_orphan must be removed (older than ORPHAN_DIR_AGE)"
        );
        assert!(
            fresh_orphan.exists(),
            "fresh_orphan must survive (mtime is recent)"
        );
        assert!(
            current_dir.exists(),
            "current engine-version subdir must NEVER be pruned, even if old"
        );
        assert_eq!(
            report.orphan_dirs_removed, 1,
            "orphan_dirs_removed must be 1"
        );
        assert_eq!(
            report.tempfiles_removed, 0,
            "prune_orphan_engine_version_dirs must not touch tempfiles"
        );
    }

    // ── prune_orphan_engine_version_dirs defensiveness (step-7) ─────────────

    /// Step-7(a): `prune_orphan_engine_version_dirs` on a non-existent
    /// `cache_root` returns `SweepReport::default()` without panicking.
    #[test]
    fn prune_orphan_engine_version_dirs_absent_cache_root_returns_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let nonexistent = tmp.path().join("no_such_cache");
        let report =
            prune_orphan_engine_version_dirs(&nonexistent, "aaaa0000000000000000000000000000");
        assert_eq!(
            report,
            SweepReport::default(),
            "absent cache_root must yield SweepReport::default()"
        );
    }

    /// Step-7(b): a stray non-directory file directly under `cache_root`
    /// (even if old) is ignored — only directories are candidates for pruning.
    /// `orphan_dirs_removed` counts only dirs, never plain files.
    #[test]
    fn prune_orphan_engine_version_dirs_ignores_stray_files_at_top_level() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let current = "cccc000000000000000000000000cccc";

        // Stray non-directory file at cache_root level (e.g. leftover index).
        let stray = root.join(".tmp.junk");
        std::fs::write(&stray, b"stray").unwrap();
        backdate_mtime(&stray, ORPHAN_DIR_AGE.as_secs() + 60);

        let report = prune_orphan_engine_version_dirs(root, current);

        // Stray file must not be removed (only dirs are candidates).
        assert!(stray.exists(), "stray non-dir file must not be removed");
        assert_eq!(
            report.orphan_dirs_removed, 0,
            "orphan_dirs_removed must be 0 (no dirs pruned)"
        );
    }

    /// Step-7(c): an unfamiliar-named subdir (not 32-hex) that is old IS
    /// pruned by age alone — no hash-format validation is applied. The current
    /// subdir is still never touched.
    #[test]
    fn prune_orphan_engine_version_dirs_prunes_unfamiliar_named_old_subdir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let current = "cccc000000000000000000000000cccc";

        // Old subdir with an unusual name (not 32-hex).
        let weird_old = root.join("legacy-build-2024");
        std::fs::create_dir_all(&weird_old).unwrap();
        backdate_mtime(&weird_old, ORPHAN_DIR_AGE.as_secs() + 60);

        // Current subdir (old mtime but must not be pruned).
        let current_dir = root.join(current);
        std::fs::create_dir_all(&current_dir).unwrap();
        backdate_mtime(&current_dir, ORPHAN_DIR_AGE.as_secs() + 60);

        let report = prune_orphan_engine_version_dirs(root, current);

        assert!(
            !weird_old.exists(),
            "unfamiliar-named old subdir must be pruned by age alone"
        );
        assert!(
            current_dir.exists(),
            "current engine-version subdir must never be pruned"
        );
        assert_eq!(report.orphan_dirs_removed, 1);
        assert_eq!(report.tempfiles_removed, 0);
    }

    /// Empty-`current_engine_version` safeguard: when the caller cannot
    /// determine the live build's engine-version hash (e.g. a build-time env
    /// var resolution failure that silently fell back to `""`), the prune is
    /// skipped wholesale to avoid catastrophically deleting the live cache
    /// subdir (which would also be unidentifiable).
    ///
    /// This test pins the `if current_engine_version.is_empty()` early-return
    /// guard at the top of `prune_orphan_engine_version_dirs`. If that guard
    /// were removed, the old subdir created below would be pruned and the
    /// `old_orphan.exists()` assertion would fail.
    #[test]
    fn prune_orphan_engine_version_dirs_with_empty_current_version_skips_all() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Create an old orphan-looking subdir that would normally be pruned if
        // given a real current_engine_version.
        let old_orphan = root.join("aaaa111111111111111111111111aaaa");
        std::fs::create_dir_all(&old_orphan).unwrap();
        backdate_mtime(&old_orphan, ORPHAN_DIR_AGE.as_secs() + 60);

        let report = prune_orphan_engine_version_dirs(root, "");

        assert!(
            old_orphan.exists(),
            "empty current_engine_version must skip the prune entirely; no dirs removed"
        );
        assert_eq!(
            report,
            SweepReport::default(),
            "empty current_engine_version must return SweepReport::default() without pruning"
        );
    }

    // ── sweep_on_startup composition + idempotence (step-9) ─────────────────

    /// Step-9 RED: `sweep_on_startup` aggregates both passes and is idempotent.
    ///
    /// Combined fixture:
    /// * An old `.tmp.*` file in an orphan shard dir.
    /// * An old orphan engine-version subdir (different from `current`).
    /// * The `current`-named subdir backdated > 30d (must survive).
    /// * A fresh regular entry (must survive).
    ///
    /// First call: both removals happen; `SweepReport` aggregates counts.
    /// Second call on the now-swept tree: `SweepReport::default()` (idempotent).
    /// Absent-root call: `SweepReport::default()` with no error.
    #[test]
    fn sweep_on_startup_aggregates_both_passes_and_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let current = ENGINE_VERSION_HASH;

        // Old orphan engine-version subdir (will be pruned).
        let orphan_eng = "dead000000000000000000000000dead";
        let orphan_dir = root.join(orphan_eng);
        std::fs::create_dir_all(orphan_dir.join("ab")).unwrap();

        // Stale .tmp.* inside the orphan subdir — will be swept by tempfile pass.
        let stale_tmp = orphan_dir.join("ab").join(".tmp.crashed");
        std::fs::write(&stale_tmp, b"crash").unwrap();
        backdate_mtime(&stale_tmp, STALE_TEMPFILE_AGE.as_secs() + 120);
        // Backdate the orphan dir itself so the dir-prune pass removes it.
        backdate_mtime(&orphan_dir, ORPHAN_DIR_AGE.as_secs() + 60);

        // Current engine-version subdir — old mtime but must survive.
        let current_dir = root.join(current);
        std::fs::create_dir_all(&current_dir).unwrap();
        backdate_mtime(&current_dir, ORPHAN_DIR_AGE.as_secs() + 60);

        // Fresh regular entry in the current subdir (must survive).
        let live_sd = shard_dir(root, current, "ff001111111111111111111111111111");
        std::fs::create_dir_all(&live_sd).unwrap();
        let live_bin = live_sd.join("ff001111111111111111111111111111.bin");
        std::fs::write(&live_bin, b"live-data").unwrap();

        let report = sweep_on_startup(root, current);

        // tempfiles_removed: stale .tmp.* was swept (may be 0 if the orphan dir
        // was pruned first — the fixed order is tempfiles first, dir prune second,
        // so the stale tempfile pass runs first and collects 1 file, then the
        // dir prune removes the orphan dir).
        assert_eq!(
            report.tempfiles_removed, 1,
            "stale .tmp.* in the orphan dir must be swept by the tempfile pass (runs first)"
        );
        assert_eq!(
            report.orphan_dirs_removed, 1,
            "the orphan engine-version dir must be pruned"
        );
        assert!(
            current_dir.exists(),
            "current engine-version subdir must survive"
        );
        assert!(
            live_bin.exists(),
            "live entry in current subdir must survive"
        );

        // Second call: idempotent — tree is already clean.
        let report2 = sweep_on_startup(root, current);
        assert_eq!(
            report2,
            SweepReport::default(),
            "second sweep_on_startup call on a clean tree must return SweepReport::default()"
        );

        // Absent cache_root: silent no-op.
        let tmp2 = tempfile::TempDir::new().unwrap();
        let nonexistent = tmp2.path().join("no_cache");
        let report3 = sweep_on_startup(&nonexistent, current);
        assert_eq!(
            report3,
            SweepReport::default(),
            "absent cache_root must yield SweepReport::default()"
        );
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
    ///
    /// RED: `ElasticResult` does not yet have `grid_bounds_min`, `grid_bounds_max`,
    /// `grid_counts`, `divergence`, `gradient`, or `curl` fields — this test will
    /// fail to compile until step-4 extends the struct and bumps FORMAT_VERSION 2→3.
    #[test]
    fn elastic_result_v3_grid_and_derivative_channels_round_trip_byte_deterministically() {
        // 2×3×4 element-count grid → (2+1)*(3+1)*(4+1) = 60 nodes.
        let n_nodes: usize = (2 + 1) * (3 + 1) * (4 + 1);
        let displacement: Vec<f64> = (0..n_nodes * 3).map(|i| i as f64 * 0.001).collect();
        let stress: Vec<f64> = (0..n_nodes * 9).map(|i| i as f64 * 1e6).collect();
        let divergence: Vec<f64> = (0..n_nodes).map(|i| i as f64 * 1e-5).collect();
        let gradient: Vec<f64> = (0..n_nodes * 9).map(|i| i as f64 * 1e-3).collect();
        let curl: Vec<f64> = (0..n_nodes * 3).map(|i| i as f64 * 2e-4).collect();

        // Constructing ElasticResult with the new v3 fields — fails to compile until step-4.
        let original = ElasticResult {
            displacement,
            stress,
            max_von_mises: 1.5e8,
            converged: true,
            iterations: 12,
            solve_time_ms: 500,
            shell_channels: None,
            // New fields added in step-4 (schema v3):
            grid_bounds_min: [0.0, 0.0, 0.0],
            grid_bounds_max: [1.0, 0.3, 0.1],
            grid_counts: [2, 3, 4],
            divergence,
            gradient,
            curl,
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

    /// step-3 RED (task #3428): FORMAT_VERSION must be 3 after the v3 bump.
    ///
    /// RED: currently 2; will flip to 3 in step-4.
    #[test]
    fn elastic_result_format_version_is_3_after_v3_bump() {
        assert_eq!(<ElasticResult as PersistentlyCacheable>::FORMAT_VERSION, 3);
    }

    // ── step-1 RED (task #3459): BucklingResultCache round-trip ──────────────
    //
    // Fails to compile until step-2 defines `BucklingResultCache` and its
    // `PersistentlyCacheable` implementation.

    #[test]
    fn buckling_result_cache_format_version_and_round_trip() {
        // 2×2×2 node grid (counts=[1,1,1] → 2 nodes per axis → 8 total).
        let n_nodes: usize = 8;
        let n_modes: usize = 2;
        let stride: usize = n_nodes * 3; // 24 f64 per mode (displaced positions)

        let original = BucklingResultCache {
            eigenvalues: vec![1.5_f64, 3.0_f64],
            mode_shapes: vec![0.5_f64; n_modes * stride],
            base_node_positions: vec![0.1_f64; stride],
            converged: true,
            iterations: 0_u32,
            ps_displacement: vec![0.1_f64; n_nodes * 3],
            ps_stress: vec![0.2_f64; n_nodes * 9],
            ps_max_von_mises: 42.0_f64,
            ps_converged: true,
            ps_iterations: 3_u32,
            ps_grid_bounds_min: [0.0_f64, 0.0_f64, 0.0_f64],
            ps_grid_bounds_max: [1.0_f64, 1.0_f64, 1.0_f64],
            ps_grid_counts: [1_u64, 1_u64, 1_u64], // 2 nodes per axis
            solve_time_ms: 100_u64,
        };

        let tmp = tempfile::TempDir::new().unwrap();
        let input_hash = "aa11bb22cc33dd44aa11bb22cc33dd44";

        write_entry::<BucklingResultCache>(tmp.path(), ENGINE_VERSION_HASH, input_hash, &original)
            .expect("write_entry for BucklingResultCache must succeed");

        let bin_path = entry_bin_path(tmp.path(), ENGINE_VERSION_HASH, input_hash);
        assert!(
            bin_path.exists(),
            "BucklingResultCache .bin must exist after write_entry: {:?}",
            bin_path,
        );

        let read_back =
            read_entry::<BucklingResultCache>(tmp.path(), ENGINE_VERSION_HASH, input_hash)
                .expect("read_entry must not return Err")
                .expect("read_entry must return Some for a just-written entry");

        assert_eq!(read_back.eigenvalues, original.eigenvalues, "eigenvalues must round-trip");
        assert_eq!(read_back.mode_shapes, original.mode_shapes, "mode_shapes must round-trip");
        assert_eq!(
            read_back.base_node_positions,
            original.base_node_positions,
            "base_node_positions must round-trip"
        );
        assert_eq!(read_back.converged, original.converged, "converged must round-trip");
        assert_eq!(read_back.iterations, original.iterations, "iterations must round-trip");
        assert_eq!(
            read_back.ps_displacement,
            original.ps_displacement,
            "ps_displacement must round-trip"
        );
        assert_eq!(read_back.ps_stress, original.ps_stress, "ps_stress must round-trip");
        assert_eq!(
            read_back.ps_max_von_mises.to_bits(),
            original.ps_max_von_mises.to_bits(),
            "ps_max_von_mises must round-trip bit-identically"
        );
        assert_eq!(read_back.ps_converged, original.ps_converged, "ps_converged must round-trip");
        assert_eq!(
            read_back.ps_iterations,
            original.ps_iterations,
            "ps_iterations must round-trip"
        );
        assert_eq!(
            read_back.ps_grid_bounds_min,
            original.ps_grid_bounds_min,
            "ps_grid_bounds_min must round-trip"
        );
        assert_eq!(
            read_back.ps_grid_bounds_max,
            original.ps_grid_bounds_max,
            "ps_grid_bounds_max must round-trip"
        );
        assert_eq!(
            read_back.ps_grid_counts,
            original.ps_grid_counts,
            "ps_grid_counts must round-trip"
        );
        assert_eq!(
            read_back.solve_time_ms,
            original.solve_time_ms,
            "solve_time_ms must round-trip (used for cost-weighted LRU eviction)"
        );

        // FORMAT_VERSION must be 1 for BucklingResultCache.
        assert_eq!(
            <BucklingResultCache as PersistentlyCacheable>::FORMAT_VERSION,
            1,
            "BucklingResultCache FORMAT_VERSION must be 1"
        );
    }
}
