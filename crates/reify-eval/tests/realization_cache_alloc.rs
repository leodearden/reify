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

    // Use a long entity name to defeat any potential short-string optimisation
    // that might skip heap allocation for small strings.
    let entity = "long_entity_name_to_defeat_any_potential_short_string_optimization_buffer_xxxxx";
    assert!(entity.len() >= 64, "entity must be ≥64 bytes to force a heap allocation");

    // Warm-up: the first insert legitimately allocates the entity String key once.
    let inserted = cache.insert(entity, reify_types::ReprKind::BRep, 0.001, 1u32);
    assert!(inserted, "warm-up insert must succeed");

    // Snapshot after warm-up — all legitimate allocations already counted.
    let before = ALLOCATIONS.load(Ordering::Relaxed);

    // Now fire 256 rejected inserts.  Each uses a looser tolerance (0.1 >> 0.001),
    // so `ToleranceBucket` short-circuits immediately (existing 0.001 ≤ 0.1 → reject).
    // The entity String key already exists in the inner HashMap.
    // With the fix:   the fast `get_mut` path is taken → zero allocations.
    // Without the fix: `entity.to_owned()` runs unconditionally → 256 allocations.
    for _ in 0..256 {
        let inserted = cache.insert(entity, reify_types::ReprKind::BRep, 0.1, 999u32);
        assert!(
            !inserted,
            "looser insert must be rejected by ToleranceBucket"
        );
    }

    let after = ALLOCATIONS.load(Ordering::Relaxed);
    let delta = after.saturating_sub(before);

    assert_eq!(
        delta, 0,
        "rejected inserts under existing entity must allocate zero times (delta = {delta})"
    );
}
