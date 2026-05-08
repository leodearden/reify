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

use serde::{Deserialize, Serialize};
use bytemuck;

/// On-disk-layout version for [`ElasticResult`]. Bump when the encoding
/// format changes (separate from `engine_version_hash`, which invalidates
/// result semantics rather than the wire format). Starting at 1 follows the
/// Reify convention that 0 means "uninitialised / unknown".
///
/// **Wire-format contract:** the `bincode` major version in use at serialise
/// time is part of this contract. Bumping `bincode` past major 1 (for example
/// the 1.x → 2.x transition, which changes default integer encoding) MUST be
/// accompanied by a bump of this constant in the same commit; otherwise cache
/// entries written under the previous `bincode` major will silently decode as
/// garbage. The same logic applies to `zstd`'s frame-format major (currently
/// the 0.13.x line). Cross-checked by `elastic_result_format_version_is_one`,
/// which forces any FORMAT_VERSION bump to be deliberate; the bincode/zstd
/// major is held by the `=1.3` / `0.13` pins in `Cargo.toml`.
const ELASTIC_RESULT_FORMAT_VERSION: u32 = 1;

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
    /// **Wire-format contract:** the major version of the underlying
    /// byte-encoder library (e.g. `bincode`) is part of the wire-format
    /// contract for any implementation of this trait. If an impl's encoder
    /// library takes a major version bump that changes its default encoding,
    /// `FORMAT_VERSION` MUST be bumped in the same commit. See
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
        // Bulk slab write — one `write_all` per slab rather than one per
        // element. `bytemuck::cast_slice::<f64, u8>` reinterprets the `&[f64]`
        // buffer as `&[u8]` without any copy; on little-endian hosts (the common
        // case) the native f64 bytes are already little-endian, so this is a
        // zero-copy fast path. The on-disk format is unconditionally little-endian
        // for cross-host portability; on big-endian hosts we build a temporary
        // `Vec<u8>` via `to_le_bytes()` per element (per-element CPU swap, single
        // bulk write to the encoder).
        //
        // Empty-safety: `cast_slice::<f64, u8>(&[])` returns `&[]`;
        // `write_all(&[])` is a no-op — empty `Vec`s still emit zero slab bytes.
        // Pinned by `elastic_result_round_trips_with_empty_field_arrays`.
        //
        // Large-N: the 1<<20-element bulk path is exercised by
        // `elastic_result_round_trips_one_million_element_vectors` (step-1 pin).
        //
        // Byte-order: the slab section is pinned as unconditionally little-endian
        // on disk by `elastic_result_serialized_slab_section_is_little_endian_bytewise`
        // (step-3 pin), which is stronger than the same-host run-to-run determinism
        // guard (`elastic_result_serialization_is_byte_deterministic`).
        #[cfg(target_endian = "little")]
        encoder.write_all(bytemuck::cast_slice::<f64, u8>(&self.displacement))?;
        #[cfg(target_endian = "big")]
        {
            let mut buf: Vec<u8> = Vec::with_capacity(self.displacement.len() * 8);
            for v in &self.displacement {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            encoder.write_all(&buf)?;
        }
        #[cfg(target_endian = "little")]
        encoder.write_all(bytemuck::cast_slice::<f64, u8>(&self.stress))?;
        #[cfg(target_endian = "big")]
        {
            let mut buf: Vec<u8> = Vec::with_capacity(self.stress.len() * 8);
            for v in &self.stress {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            encoder.write_all(&buf)?;
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
        //   * `read_exact` on a fixed `[u8; 8]` buffer returns
        //     `Err(io::ErrorKind::UnexpectedEof)` on a short read; the
        //     `f64::from_le_bytes` call then only ever sees a populated 8-byte
        //     array, so there's no slice-indexing panic path to guard.
        let mut decoder = zstd::Decoder::new(r)?;
        let header: ElasticResultHeader =
            bincode::deserialize_from(&mut decoder).map_err(io::Error::other)?;
        // Bound length-prefix fields BEFORE allocating to defend against
        // corrupted/tampered cache entries claiming `u64::MAX` (or values
        // that silently truncate via `as usize` on a 32-bit target). See
        // `MAX_F64_ELEMENTS` for the rationale on the limit value.
        let displacement_cap = check_f64_vec_len("displacement", header.displacement_len)?;
        let stress_cap = check_f64_vec_len("stress", header.stress_len)?;
        // Defence-in-depth (review feedback on the deserialise allocation
        // hazard): even with the bound check in place, an honest 128 MiB
        // reservation may fail on memory-constrained hosts (no overcommit,
        // small CI sandbox). `try_reserve_exact` surfaces such a failure as
        // `io::Error` rather than aborting the process via `Vec::with_capacity`'s
        // panic-on-OOM path.
        let mut displacement: Vec<f64> = Vec::new();
        displacement
            .try_reserve_exact(displacement_cap)
            .map_err(io::Error::other)?;
        for _ in 0..header.displacement_len {
            let mut bytes = [0u8; 8];
            decoder.read_exact(&mut bytes)?;
            displacement.push(f64::from_le_bytes(bytes));
        }
        let mut stress: Vec<f64> = Vec::new();
        stress
            .try_reserve_exact(stress_cap)
            .map_err(io::Error::other)?;
        for _ in 0..header.stress_len {
            let mut bytes = [0u8; 8];
            decoder.read_exact(&mut bytes)?;
            stress.push(f64::from_le_bytes(bytes));
        }
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
            displacement: vec![1.0, -2.5, 3.14159, 0.0, 1e-9],
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
            displacement: vec![1.0, -2.5, 3.14159, 0.0, 1e-9],
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
            displacement: vec![1.0_f64, -2.5_f64, 3.14159_f64],
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
}
