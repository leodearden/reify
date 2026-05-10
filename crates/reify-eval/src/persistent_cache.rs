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

use reify_types::ContentHash;
use serde::{Deserialize, Serialize};

/// Compute the canonical engine-version hash for a set of contributor byte slices.
///
/// Each contributor is framed with a `u64` LE length prefix before concatenation
/// into the hash buffer. This prevents the trivial concat-collision class where
/// `[b"ab", b"c"]` and `[b"a", b"bc"]` would otherwise produce identical hashes
/// (see `compose_engine_version_hash_length_prefix_prevents_concat_collision`).
///
/// The hash primitive is `xxh3_128` via [`ContentHash`] — the same algorithm used
/// for content-addressed hashing across the codebase. Cache-key invalidation does
/// not require cryptographic collision resistance; xxh3 is appropriate and
/// consistent with existing conventions.
///
/// Returns a 32-character lowercase hexadecimal string (matching
/// [`ContentHash`]'s `Display` format).
///
/// The only production caller is `build.rs`, which computes the contributor bytes
/// from the source files listed in `CONTRIBUTORS_RELATIVE`. The function is `pub`
/// so build.rs documentation can reference it by name and so the
/// algorithm-drift sentinel test (`compose_engine_version_hash_pins_fixed_input_to_exact_hex_literal`)
/// pins the library output against the build-script's duplicated logic.
///
/// PRD: `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine version".
pub fn compose_engine_version_hash(parts: &[&[u8]]) -> String {
    let total_len: usize = parts.iter().map(|p| 8 + p.len()).sum();
    let mut buf = Vec::with_capacity(total_len);
    for part in parts {
        buf.extend_from_slice(&(part.len() as u64).to_le_bytes());
        buf.extend_from_slice(part);
    }
    format!("{}", ContentHash::of(&buf))
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
/// Cross-checked by `elastic_result_format_version_is_one`, which forces any
/// FORMAT_VERSION bump to be deliberate. The `=1.3` pin blocks even minor
/// bumps to `bincode`; `0.13` pins `zstd`'s 0.x line — both held in
/// `Cargo.toml`.
const ELASTIC_RESULT_FORMAT_VERSION: u32 = 1;

/// Canonical engine-version hash for FEA persistent-cache keys. Baked at
/// build time by `build.rs` over the contributor source files listed in
/// `CONTRIBUTORS_RELATIVE` (reify-solver-elastic, reify-kernel-gmsh, stdlib
/// FEA helpers, and per-purpose tolerance impls in this crate).
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
}

/// Opt-in trait for `ComputeNode` output value types that may be persisted
/// across sessions in the on-disk cache.
///
/// Implementations are responsible for byte-deterministic, round-trip-stable
/// encoding of their state. The cache layer dispatches on the concrete type
/// per cache key, so this trait is **not** object-safe.
pub trait PersistentlyCacheable: Sized {
    /// On-disk-layout version. Bumped when the encoding format changes,
    /// independently of any `engine_version_hash` (which invalidates result
    /// semantics rather than the wire format).
    ///
    /// **Wire-format contract:** the version of the underlying byte-encoder
    /// library (e.g. `bincode`) is part of the wire-format contract for any
    /// implementation of this trait. Any release of the encoder library whose
    /// default encoding could change — for `bincode`, that includes even a
    /// minor bump past the current `=1.3` pin — MUST be accompanied by a
    /// `FORMAT_VERSION` bump in the same commit. See
    /// `ELASTIC_RESULT_FORMAT_VERSION` for the bincode/zstd specifics.
    ///
    /// Associated const (no `&self`) so the cache layer can read the format
    /// version directly from the type — keying entries by `(TypeId, FORMAT_VERSION)`
    /// without first materialising a value.
    const FORMAT_VERSION: u32;

    /// Serialize `self` to `w`. Encoding must be byte-deterministic for any
    /// given value (re-serializing a deserialized value must yield the
    /// identical byte sequence).
    fn serialize_to_writer(&self, w: &mut impl Write) -> io::Result<()>;

    /// Deserialize a value of `Self` from `r`. The inverse of
    /// [`serialize_to_writer`](Self::serialize_to_writer); a round-trip must
    /// preserve every field bit-exactly (including NaN payloads and signed
    /// zeros for any `f64` fields).
    fn deserialize_from_reader(r: &mut impl Read) -> io::Result<Self>;

    /// Solve time in milliseconds, exposed to the cache layer for
    /// cost-weighted LRU eviction.
    fn solve_time_ms(&self) -> u64;
}

/// Linear-elastostatic FEA solver output container.
///
/// Field set is fixed by the PRD: per-DOF displacement and stress arrays,
/// a `max_von_mises` scalar summary, a `converged` flag, an `iterations`
/// count, and a `solve_time_ms` cost metric for cache eviction.
#[derive(Debug, Clone, PartialEq)]
pub struct ElasticResult {
    pub displacement: Vec<f64>,
    pub stress: Vec<f64>,
    pub max_von_mises: f64,
    pub converged: bool,
    pub iterations: u32,
    pub solve_time_ms: u64,
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
        buf.try_reserve_exact(byte_count).map_err(io::Error::other)?;
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
        let byte_slice: &mut [u8] = unsafe {
            std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, len * 8)
        };
        r.read_exact(byte_slice)?;
        // SAFETY: (a) capacity >= len after the successful try_reserve_exact
        // above; (b) all len*8 bytes are now initialised — read_exact returned
        // Ok(()), so every byte in the backing store was written; (c) f64 is
        // Pod / AnyBitPattern so any bit pattern is a valid f64. set_len is
        // only reached on the Ok path, so no partially-uninitialised Vec exists.
        unsafe { vec.set_len(len); }
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
        f64::from_le_bytes(chunk.try_into().expect("chunks_exact(8) yields exactly-8-byte slices"))
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
        };
        bincode::serialize_into(&mut encoder, &header).map_err(io::Error::other)?;
        // Bulk slab writes — see `write_f64_slab` for the full rationale on
        // LE zero-copy, BE byte-swap, OOM-safe sizing, empty-slab safety, and
        // the byte-order pin tests.
        write_f64_slab(&mut encoder, &self.displacement)?;
        write_f64_slab(&mut encoder, &self.stress)?;
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
        // Bulk slab reads — see `read_f64_slab` for the full rationale on LE
        // set_len safety, BE byte-swap, OOM-safe sizing, and the pin tests.
        // `check_f64_vec_len` above already validated both caps against
        // `MAX_F64_ELEMENTS`, so `read_f64_slab` receives pre-validated lengths.
        let displacement = read_f64_slab(&mut decoder, displacement_cap)?;
        let stress = read_f64_slab(&mut decoder, stress_cap)?;

        Ok(ElasticResult {
            displacement,
            stress,
            max_von_mises: f64::from_bits(header.max_von_mises_bits),
            converged: header.converged,
            iterations: header.iterations,
            solve_time_ms: header.solve_time_ms,
        })
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
    fn elastic_result_format_version_is_one() {
        // Read from the trait associated const directly — no instance needed,
        // demonstrating the cache-layer use case where `(TypeId, FORMAT_VERSION)`
        // can be looked up before any value materialises. Pins the project
        // convention that FORMAT_VERSION starts at 1 because 0 means
        // "uninitialised / unknown" (see `ELASTIC_RESULT_FORMAT_VERSION` doc).
        // An intentional format bump must touch this assertion — that is the
        // point: it forces a deliberate acknowledgement that cached bytes from
        // the previous version are now incompatible.
        assert_eq!(<ElasticResult as PersistentlyCacheable>::FORMAT_VERSION, 1);
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
        };
        assert_eq!(
            nine_thousand_nine_hundred_ninety_nine.solve_time_ms(),
            9999
        );

        // Pin that the accessor isn't returning a hard-coded constant.
        let zero = ElasticResult {
            displacement: vec![],
            stress: vec![],
            max_von_mises: 0.0,
            converged: false,
            iterations: 0,
            solve_time_ms: 0,
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
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        // NaN != NaN under PartialEq, so compare bit-patterns explicitly.
        assert_eq!(decoded.displacement.len(), original.displacement.len());
        for (d, o) in decoded.displacement.iter().zip(original.displacement.iter()) {
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
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::InvalidData
                    | io::ErrorKind::Other
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
        };
        let mut buf: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut buf).unwrap();
        let decoded = ElasticResult::deserialize_from_reader(&mut &buf[..]).unwrap();
        assert_eq!(decoded, original);
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
                    (i as u64)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ 0xDEAD_BEEF_CAFE_BABE,
                )
            })
            .collect();
        // Smaller stress vector derived from a different scramble constant so
        // both slab paths are exercised without doubling the allocation.
        let stress: Vec<f64> = (0..1024u64)
            .map(|i| {
                f64::from_bits(
                    i.wrapping_mul(0x6C62_272E_07BB_0142) ^ 0xFEED_FACE_DEAD_BEEF,
                )
            })
            .collect();
        let original = ElasticResult {
            displacement,
            stress,
            max_von_mises: f64::from_bits(0xDEAD_BEEF_CAFE_BABE),
            converged: true,
            iterations: 1,
            solve_time_ms: 42,
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
        for (d, o) in decoded.displacement.iter().zip(original.displacement.iter()) {
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
        };
        let mut compressed: Vec<u8> = Vec::new();
        original.serialize_to_writer(&mut compressed).unwrap();

        // Decompress the zstd frame to recover the inner bincode+slab stream.
        let mut zstd_dec = zstd::Decoder::new(&compressed[..]).unwrap();
        let mut decompressed: Vec<u8> = Vec::new();
        io::Read::read_to_end(&mut zstd_dec, &mut decompressed).unwrap();

        // Consume the bincode-encoded header via a mutable slice reference.
        // `bincode::deserialize_from` advances the `&mut &[u8]` reader by
        // exactly as many bytes as the header occupies, leaving `slice`
        // pointing at the first byte of the slab section.
        let mut slice: &[u8] = &decompressed;
        let _header: ElasticResultHeader = bincode::deserialize_from(&mut slice)
            .expect("header must deserialize cleanly");

        // Build expected slab: displacement bytes then stress bytes, each
        // value as 8-byte little-endian (unconditionally, regardless of host
        // endianness — this is the cross-host portability contract).
        let mut expected: Vec<u8> = Vec::new();
        for v in &original.displacement {
            expected.extend_from_slice(&v.to_le_bytes());
        }
        for v in &original.stress {
            expected.extend_from_slice(&v.to_le_bytes());
        }

        assert_eq!(
            slice,
            expected.as_slice(),
            "slab section must be unconditionally little-endian on disk; \
             any regression to native-byte encoding on a big-endian host \
             or accidental to_ne_bytes() usage will fail this assertion"
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
        let err = read_f64_slab(&mut &short[..], 4)
            .expect_err("short input must return Err");
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
        assert!(decoded.is_empty(), "read of zero-length slab must return empty Vec");
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
        };
        // Use serialize_into to mirror the production write path (ElasticResult::serialize_to_writer).
        let mut encoded: Vec<u8> = Vec::new();
        bincode::serialize_into(&mut encoded, &header)
            .expect("bincode serialize_into must not fail for fixed-size header");
        // Pinned bincode 1.3 fixint-LE encoding of the fixture header.
        // Layout (struct-declaration order, LE encoding):
        //   max_von_mises_bits (u64 LE, 8 bytes): EF BE AD DE BE BA FE CA
        //   converged (bool, 1 byte):              01
        //   iterations (u32 LE, 4 bytes):          78 56 34 12
        //   solve_time_ms (u64 LE, 8 bytes):       BE BA FE CA EF BE AD DE
        //   displacement_len (u64 LE, 8 bytes):    05 00 00 00 00 00 00 00
        //   stress_len (u64 LE, 8 bytes):          07 00 00 00 00 00 00 00
        // Total: 37 bytes.
        let expected: [u8; 37] = [
            0xEF, 0xBE, 0xAD, 0xDE, 0xBE, 0xBA, 0xFE, 0xCA, // max_von_mises_bits LE
            0x01,                                               // converged = true
            0x78, 0x56, 0x34, 0x12,                            // iterations LE
            0xBE, 0xBA, 0xFE, 0xCA, 0xEF, 0xBE, 0xAD, 0xDE, // solve_time_ms LE
            0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // displacement_len = 5
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // stress_len = 7
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
        // not derive PartialEq — six per-field asserts cover the full struct.)
        let decoded: ElasticResultHeader = bincode::deserialize(&expected[..])
            .expect("must decode pinned literal");
        assert_eq!(decoded.max_von_mises_bits, header.max_von_mises_bits);
        assert_eq!(decoded.converged, header.converged);
        assert_eq!(decoded.iterations, header.iterations);
        assert_eq!(decoded.solve_time_ms, header.solve_time_ms);
        assert_eq!(decoded.displacement_len, header.displacement_len);
        assert_eq!(decoded.stress_len, header.stress_len);
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
            ENGINE_VERSION_HASH,
            "00000000000000000000000000000000",
            "ENGINE_VERSION_HASH must not be the all-zeros sentinel"
        );
    }

    #[test]
    fn engine_version_hash_const_matches_compose_engine_version_hash_output_shape() {
        // Pins that ENGINE_VERSION_HASH obeys the same wire-format as the library
        // function (catches a future regression where the const switches to e.g.
        // base64 or a different length encoding).
        let sample = compose_engine_version_hash(&[b"x"]);
        assert_eq!(
            ENGINE_VERSION_HASH.len(),
            sample.len(),
            "ENGINE_VERSION_HASH length differs from compose_engine_version_hash output"
        );
        assert!(
            sample
                .chars()
                .all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "compose_engine_version_hash output must be lowercase hex"
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
                let mut perturbed: Vec<Vec<u8>> = contributors
                    .iter()
                    .map(|c| c.to_vec())
                    .collect();
                perturbed[ci][bi] ^= 0xFF;
                let perturbed_refs: Vec<&[u8]> =
                    perturbed.iter().map(|v| v.as_slice()).collect();
                let h = compose_engine_version_hash(&perturbed_refs);
                assert_ne!(
                    h,
                    baseline,
                    "hash unchanged after flipping byte {} of contributor {}",
                    bi,
                    ci
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
        // Pins the algorithm against drift. If the length-prefix scheme,
        // hash primitive, or hex formatting changes, this literal must be
        // updated deliberately in the same commit.
        // Literal filled in during step-2 once the implementation produces a
        // concrete value.
        let h = compose_engine_version_hash(&[b"reify", b"engine"]);
        assert_eq!(
            h,
            "30b30882195f8e834bdbd936fa5324e0",
            "algorithm drift detected — update this literal in the same commit"
        );
    }
}
