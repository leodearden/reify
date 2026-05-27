//! Allocation-counting integration test for `RealizationCache::insert` with rotating
//! `options_hash` values.
//!
//! # INVARIANT: exactly one `#[test]` per alloc-counting binary
//!
//! Do NOT add a second `#[test]` to this file.  The `#[global_allocator]` counter
//! (`ALLOCATIONS`) is process-wide.  libtest runs `#[test]` functions in parallel by
//! default (threads = nproc), so two tests in the same binary race the shared counter
//! and produce spurious non-zero deltas in whichever test gets polled while the other
//! test's allocations land inside its measurement window.
//!
//! The pre-commit hook runs `cargo test --workspace --quiet` with NO `--test-threads`
//! override, so default parallelism applies.  Adding a second test here will silently
//! re-introduce the regression described in task 3680 / commit a35a682f93.
//!
//! Future alloc tests must live in their own `tests/*.rs` file (each compiles to a
//! separate process with its own private allocator counter).  The single-hash contract
//! lives in `realization_cache_alloc.rs`.
//!
//! # Why a separate integration binary?
//!
//! `#[global_allocator]` is process-wide for the binary it lives in.  If this were
//! placed as a `#[cfg(test)]` unit test inside `src/realization_cache.rs`, the counting
//! wrapper would intercept *every* allocation in the entire `reify-eval` unit-test binary,
//! perturbing unrelated tests and making future allocator-sensitive tests harder to add.
//! Each file under `tests/` compiles to its own separate integration test binary, so
//! isolating the allocator here confines the instrumentation to this one binary.

use std::sync::atomic::Ordering;

mod common;

#[global_allocator]
static GLOBAL: common::alloc_counter::CountingAllocator = common::alloc_counter::CountingAllocator;

/// Rejected inserts at rotating `options_hash` values under an existing entity must not
/// allocate a new `String` key — locking the "regardless of `options_hash`" clause of the
/// module-level allocation contract.
///
/// Module docs claim: "Subsequent inserts at the same `(entity, repr_kind)` —
/// regardless of `options_hash` — take the `get_mut` fast path and produce zero
/// `String` allocations."
///
/// A regression that re-introduces `entity.to_owned()` only when `options_hash` changes
/// (e.g. refactoring to `.entry(entity.to_owned()).or_default().entry(options_hash)…`)
/// would produce N entity-String allocations — one per new hash — and slip past the
/// existing single-hash alloc test.  This test locks the second clause.
///
/// Phase: warm up at N distinct `options_hash` values (legitimately allocates all
/// `ToleranceBucket` structures); snapshot; fire N rejected inserts at those same hashes
/// with a looser tolerance; assert the counter stays flat.
#[test]
fn rejected_insert_with_rotating_options_hash_does_not_allocate_entity_string() {
    let entity = "long_entity_name_to_defeat_any_potential_short_string_optimization_buffer_xxxxx";
    assert!(
        entity.len() >= 64,
        "entity must be ≥64 bytes (belt-and-suspenders guard)"
    );

    const N_HASHES: usize = 32;
    let hashes: Vec<reify_core::ContentHash> = (0..N_HASHES)
        .map(|i| reify_core::ContentHash::of_u64(i as u64))
        .collect();

    let mut cache = reify_eval::RealizationCache::<u32>::new();

    // Warm-up: insert at each hash with a tight tolerance.
    // The first call allocates the entity String once; each subsequent call takes
    // the get_mut fast path and allocates only the new ToleranceBucket Vec.
    for (i, &hash) in hashes.iter().enumerate() {
        let inserted = cache.insert(entity, reify_ir::ReprKind::BRep, 0.001, hash, i as u32);
        assert!(inserted, "warm-up insert at hash {i} must succeed");
    }

    // Snapshot after warm-up — all legitimate allocations already counted.
    let before = common::alloc_counter::ALLOCATIONS.load(Ordering::Relaxed);

    // Rejected inserts: loose tol 0.1 >> warm-up 0.001, so ToleranceBucket
    // short-circuits immediately without touching the Vec.
    // With the fix:    get_mut(entity) finds the key → zero entity String allocs.
    // Without the fix: entity.to_owned() runs → N entity String allocs (≈ 32).
    for &hash in &hashes {
        let inserted = cache.insert(entity, reify_ir::ReprKind::BRep, 0.1, hash, 999u32);
        assert!(
            !inserted,
            "looser insert must be rejected by ToleranceBucket"
        );
    }

    let after = common::alloc_counter::ALLOCATIONS.load(Ordering::Relaxed);
    let delta = after.saturating_sub(before);

    // Same reasoning as the sibling test (realization_cache_alloc.rs): background-thread
    // noise may add 1-2 allocations; the regression (entity.to_owned() on every call)
    // produces delta ≈ N = 32, which is far above the threshold of 4.
    // (This binary has a single `#[test]`, so background-thread noise comes exclusively
    // from libtest's own output-capture thread — `--test-threads` parallelism between
    // tests is not a factor.)
    assert!(
        delta <= 4,
        "rejected inserts under rotating options_hash must not re-allocate the entity \
         String; got delta = {delta}.  A delta near {N_HASHES} indicates \
         entity.to_owned() is being called when options_hash changes."
    );
}
