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

/// Opt-in trait for `ComputeNode` output value types that may be persisted
/// across sessions in the on-disk cache.
///
/// Implementations are responsible for byte-deterministic, round-trip-stable
/// encoding of their state. The cache layer dispatches on the concrete type
/// per cache key, so this trait is **not** object-safe.
pub trait PersistentlyCacheable: Sized {
    /// Serialize `self` to `w`. Encoding must be byte-deterministic for any
    /// given value (re-serializing a deserialized value must yield the
    /// identical byte sequence).
    fn serialize_to_writer(&self, w: &mut impl Write) -> io::Result<()>;

    /// Deserialize a value of `Self` from `r`. The inverse of
    /// [`serialize_to_writer`](Self::serialize_to_writer); a round-trip must
    /// preserve every field bit-exactly (including NaN payloads and signed
    /// zeros for any `f64` fields).
    fn deserialize_from_reader(r: &mut impl Read) -> io::Result<Self>;

    /// On-disk-layout version. Bumped when the encoding format changes,
    /// independently of any `engine_version_hash` (which invalidates result
    /// semantics rather than the wire format).
    fn format_version(&self) -> u32;

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

impl PersistentlyCacheable for ElasticResult {
    fn serialize_to_writer(&self, _w: &mut impl Write) -> io::Result<()> {
        unimplemented!("step-4")
    }

    fn deserialize_from_reader(_r: &mut impl Read) -> io::Result<Self> {
        unimplemented!("step-4")
    }

    fn format_version(&self) -> u32 {
        1
    }

    fn solve_time_ms(&self) -> u64 {
        self.solve_time_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check that `T` implements `PersistentlyCacheable`.
    fn assert_persistently_cacheable<T: PersistentlyCacheable>() {}

    #[test]
    fn elastic_result_implements_persistently_cacheable() {
        assert_persistently_cacheable::<ElasticResult>();
    }

    #[test]
    fn elastic_result_constructor_pins_six_field_shape() {
        let er = ElasticResult {
            displacement: vec![1.0, 2.0],
            stress: vec![3.0],
            max_von_mises: 42.0,
            converged: true,
            iterations: 17,
            solve_time_ms: 250,
        };
        assert_eq!(er.displacement, vec![1.0, 2.0]);
        assert_eq!(er.stress, vec![3.0]);
        assert_eq!(er.max_von_mises, 42.0);
        assert!(er.converged);
        assert_eq!(er.iterations, 17);
        assert_eq!(er.solve_time_ms, 250);
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
}
