//! Opt-in trait for `ComputeNode` output value types that may be persisted
//! across sessions in the on-disk cache.
//!
//! Moved here from `reify-eval::persistent_cache` during task γ (#3834) to
//! break the `reify-shell-extract → reify-eval` dependency cycle introduced
//! when `impl PersistentlyCacheable for ShellExtractionResult` needed to live
//! in the same crate as `ShellExtractionResult` (orphan rule). Moving the
//! trait declaration to this leaf crate (`reify-core`, B1 invariant: zero
//! `reify-*` deps) satisfies the orphan rule while keeping the 700-line impl
//! block co-located with its struct in `reify-shell-extract`.
//!
//! A re-export shim `pub use reify_core::persistent_cache::PersistentlyCacheable;`
//! lives at `reify_eval::persistent_cache` so every existing import path
//! continues to resolve unchanged.
//!
//! # B1 invariant
//!
//! This file MUST NOT import any `reify-*` crate. The trait body uses only
//! `std::io::{Read, Write}` and the `Sized` bound.

use std::io::{self, Read, Write};

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

    /// Uncompressed body byte count, used to populate
    /// [`CacheEntryHeader::byte_size`] so the value is reportable by
    /// `cache stats` without decompressing the body.
    ///
    /// MUST equal the number of bytes a caller would see after decompressing
    /// the output of [`serialize_to_writer`](Self::serialize_to_writer).
    ///
    /// No default implementation — every cacheable type must answer. Pinned by
    /// `write_entry_populates_byte_size_field_with_actually_uncompressed_body_byte_count`.
    fn uncompressed_byte_size(&self) -> u64;

    /// Solve time in milliseconds, exposed to the cache layer for
    /// cost-weighted LRU eviction.
    fn solve_time_ms(&self) -> u64;
}
