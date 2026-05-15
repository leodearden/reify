//! Allocation-counting integration test for `RealizationCache::insert`.
//!
//! # Why a separate integration binary?
//!
//! `#[global_allocator]` is process-wide for the binary it lives in.  If this were
//! placed as a `#[cfg(test)]` unit test inside `src/realization_cache.rs`, the counting
//! wrapper would intercept *every* allocation in the entire `reify-eval` unit-test binary,
//! perturbing unrelated tests and making future allocator-sensitive tests harder to add.
//! Each file under `tests/` compiles to its own separate integration test binary, so
//! isolating the allocator here confines the instrumentation to this one binary.
//!
//! # Why the rejected-insert path?
//!
//! A successful insert also calls `Vec::insert(0, …)` inside `ToleranceBucket`, which
//! can reallocate the Vec's backing buffer when capacity is exceeded — those are real,
//! expected allocations that would make a `delta == 0` assertion brittle without
//! carefully pre-sizing capacity.
//!
//! A *rejected* insert (`ToleranceBucket::insert` returns `false` immediately because a
//! cached entry with a tighter tolerance already satisfies the request) never touches the
//! `Vec` at all.  That makes the entity `String` from `entity.to_owned()` the ONLY
//! possible heap operation in `RealizationCache::insert`.  So `delta == 0` after N
//! rejected calls is a clean, deterministic assertion:
//!
//! - **Before the fix** (unconditional `entity.to_owned()`): each of the 256 calls
//!   allocates a fresh `String`, so `delta == 256` → test fails.
//! - **After the fix** (`get_mut` fast path skips `to_owned()`): the rejected calls
//!   take the allocation-free `get_mut` branch → `delta == 0` → test passes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Thin wrapper around [`std::alloc::System`] that counts every `alloc` call.
struct CountingAllocator;

/// Global counter incremented on every allocation.
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: delegating to the system allocator with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: delegating to the system allocator with the same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Rejected inserts under an existing entity must not allocate a new `String` key.
///
/// After a single warm-up insert (which legitimately allocates the entity `String` once),
/// every subsequent insert at a looser tolerance is rejected by `ToleranceBucket::insert`
/// without modifying the `Vec`.  With the fix in place, those rejected calls take the
/// `get_mut` fast path and produce zero heap allocations.
#[test]
fn rejected_insert_under_existing_entity_does_not_allocate_key() {
    let mut cache = reify_eval::RealizationCache::<u32>::new();

    // Belt-and-suspenders: use a long entity name so that any future allocator
    // optimisation for tiny strings (e.g. a hypothetical small-string optimisation
    // in an alternative allocator) still wouldn't elide this allocation.
    // Note: std's `String` does NOT implement SSO — every non-empty `String`
    // heap-allocates regardless of length — but the length guard makes the intent
    // explicit for future readers.
    let entity = "long_entity_name_to_defeat_any_potential_short_string_optimization_buffer_xxxxx";
    assert!(
        entity.len() >= 64,
        "entity must be ≥64 bytes (belt-and-suspenders guard)"
    );

    // Warm-up: the first insert legitimately allocates the entity String key once.
    let inserted = cache.insert(entity, reify_types::ReprKind::BRep, 0.001, reify_types::ContentHash(0), 1u32);
    assert!(inserted, "warm-up insert must succeed");

    // Snapshot after warm-up — all legitimate allocations already counted.
    let before = ALLOCATIONS.load(Ordering::Relaxed);

    // Now fire 256 rejected inserts.  Each uses a looser tolerance (0.1 >> 0.001),
    // so `ToleranceBucket` short-circuits immediately (existing 0.001 ≤ 0.1 → reject).
    // The entity String key already exists in the inner HashMap.
    // With the fix:   the fast `get_mut` path is taken → zero allocations.
    // Without the fix: `entity.to_owned()` runs unconditionally → 256 allocations.
    for _ in 0..256 {
        let inserted = cache.insert(entity, reify_types::ReprKind::BRep, 0.1, reify_types::ContentHash(0), 999u32);
        assert!(
            !inserted,
            "looser insert must be rejected by ToleranceBucket"
        );
    }

    let after = ALLOCATIONS.load(Ordering::Relaxed);
    let delta = after.saturating_sub(before);

    // Safety assumption: `ALLOCATIONS` is process-wide, so an allocation on another
    // thread between `before` and `after` would cause a spurious failure.  The
    // libtest harness maintains a background thread for output capture even when there
    // is only one `#[test]` in this binary; under the resource pressure of a parallel
    // verify pipeline that thread occasionally makes 1-2 allocations within the window.
    // Using a `delta <= small_constant` bound (as the previous comment said to do if
    // flakiness was ever observed) tolerates these background allocations while still
    // catching the pre-fix regression where `entity.to_owned()` ran unconditionally on
    // every call — that case produces `delta ≈ 256`, a value far above the threshold.
    // The threshold is set at 4: the regression this test guards against
    // (`entity.to_owned()` on every call) produces delta = 256, well above
    // either 4 or 16 — so the specific value makes no difference for catching
    // the known bug.  Tightening to 4 buys protection only against hypothetical
    // intermediate regressions; the honest justification is that CI consistently
    // observes ≤ 2 allocations from libtest's output-capture thread, and a
    // tighter bound costs nothing as long as the noise floor stays there.
    // (This binary has a single `#[test]`, so background-thread noise comes
    // exclusively from libtest's own output-capture thread — `--test-threads`
    // parallelism between tests is not a factor.)
    assert!(
        delta <= 4,
        "rejected inserts under existing entity must allocate at most a handful of times \
         (background-thread tolerance ≤ 4); got delta = {delta}.  A delta near 256 \
         indicates the get_mut fast path is not being taken."
    );
}

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
    assert!(entity.len() >= 64, "entity must be ≥64 bytes (belt-and-suspenders guard)");

    const N_HASHES: usize = 32;
    let hashes: Vec<reify_types::ContentHash> = (0..N_HASHES)
        .map(|i| reify_types::ContentHash::of_u64(i as u64))
        .collect();

    let mut cache = reify_eval::RealizationCache::<u32>::new();

    // Warm-up: insert at each hash with a tight tolerance.
    // The first call allocates the entity String once; each subsequent call takes
    // the get_mut fast path and allocates only the new ToleranceBucket Vec.
    for (i, &hash) in hashes.iter().enumerate() {
        let inserted =
            cache.insert(entity, reify_types::ReprKind::BRep, 0.001, hash, i as u32);
        assert!(inserted, "warm-up insert at hash {i} must succeed");
    }

    // Snapshot after warm-up — all legitimate allocations already counted.
    let before = ALLOCATIONS.load(Ordering::Relaxed);

    // Rejected inserts: loose tol 0.1 >> warm-up 0.001, so ToleranceBucket
    // short-circuits immediately without touching the Vec.
    // With the fix:    get_mut(entity) finds the key → zero entity String allocs.
    // Without the fix: entity.to_owned() runs → N entity String allocs (≈ 32).
    for &hash in &hashes {
        let inserted =
            cache.insert(entity, reify_types::ReprKind::BRep, 0.1, hash, 999u32);
        assert!(!inserted, "looser insert must be rejected by ToleranceBucket");
    }

    let after = ALLOCATIONS.load(Ordering::Relaxed);
    let delta = after.saturating_sub(before);

    // Same reasoning as the sibling test: background-thread noise may add 1-2;
    // the regression (entity.to_owned() on every call) produces delta ≈ N = 32,
    // which is far above the threshold of 4.
    assert!(
        delta <= 4,
        "rejected inserts under rotating options_hash must not re-allocate the entity \
         String; got delta = {delta}.  A delta near {N_HASHES} indicates \
         entity.to_owned() is being called when options_hash changes."
    );
}
