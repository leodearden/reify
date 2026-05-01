//! v0.2 multi-kernel registry collector.
//!
//! Materialises the static linker-collected set of [`KernelRegistration`]
//! records (submitted by adapter crates via `inventory::submit!`) into a
//! `BTreeMap<String, &'static KernelRegistration>` keyed on kernel name. The
//! lexicographic key order matches the dispatcher's tie-break contract in
//! `crates/reify-eval/src/dispatcher.rs`.
//!
//! # PRD reference
//!
//! `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions": each
//! kernel adapter lives in a separate crate, gated by Cargo features /
//! `cfg(has_occt)` build-time flags, registering via a static linker-
//! collection mechanism (`inventory`) read once at engine startup.
//!
//! # Dependency-inversion rationale
//!
//! The registration record [`KernelRegistration`] lives in `reify-types`,
//! NOT here. Co-locating the record with the trait it carries (`fn() ->
//! Box<dyn GeometryKernel>`) lets adapter crates `inventory::submit!`
//! without acquiring a dep on `reify-eval`. The collection consumer
//! ([`registry`]) lives here because the dispatcher and engine —
//! the consumers of the materialised map — also live in `reify-eval`.
//! See `crates/reify-types/src/geometry.rs:226-230` for the documented
//! "kernel adapters depend on reify-types but NOT on reify-eval"
//! inversion this preserves.
//!
//! # Determinism
//!
//! `inventory::iter::<T>()` does NOT guarantee link order. Materialising
//! into a `BTreeMap` keyed on `name` makes downstream iteration
//! lexicographic regardless of link ordering — required by the PRD's
//! "Selection deterministic given pinned runtime configuration" contract
//! and matched by the dispatcher's `BTreeMap<String, &CapabilityDescriptor>`
//! input shape.
//!
//! # Memoization
//!
//! [`registry`] wraps the inventory walk in a [`std::sync::OnceLock`] so the
//! map is materialised exactly once per process. This enforces the PRD's
//! "read once at engine startup" contract structurally rather than relying
//! on caller discipline — both [`crate::Engine::with_registered_kernel`] and
//! the future dispatcher wiring share the cached map and the centralised
//! tie-break helper [`pick_lexmin_kernel`].

use std::collections::BTreeMap;
use std::sync::OnceLock;

use reify_types::{CapabilityDescriptor, KernelRegistration};

/// Memoized BTreeMap of every static-collected [`KernelRegistration`], keyed
/// by `name`. Allocated once on first call and never rebuilt.
static REGISTRY: OnceLock<BTreeMap<String, &'static KernelRegistration>> = OnceLock::new();

/// Borrowed accessor over the memoized registry of [`KernelRegistration`]
/// records.
///
/// The first call walks `inventory::iter::<KernelRegistration>()` and
/// materialises a `BTreeMap` keyed on each kernel's name; subsequent calls
/// are O(1). Both [`crate::Engine::with_registered_kernel`] and (in v0.3+)
/// dispatcher wiring SHOULD consume this borrowed view rather than
/// re-walking inventory — that's the structural enforcement of the PRD's
/// "read once at engine startup" contract.
///
/// # Determinism
///
/// `BTreeMap` iteration is lexicographic on `String` keys regardless of the
/// underlying inventory link order, so callers can rely on `.values().next()`
/// or `.iter().next()` to produce the lex-smallest registration
/// deterministically — see [`pick_lexmin_kernel`] for the centralised helper
/// over that contract.
///
/// # Duplicate-name detection
///
/// If two adapters submit registrations under the same `name`, the BTreeMap
/// silently overwrites the earlier value. The build path inserts each entry
/// and trips a `debug_assert!` (and emits a `tracing::warn!`) on collision so
/// such misconfigurations surface loudly in dev/test rather than producing
/// arbitrary kernel selection in release.
pub fn registry() -> &'static BTreeMap<String, &'static KernelRegistration> {
    REGISTRY.get_or_init(build_registry)
}

/// The lexicographically smallest [`KernelRegistration`] in the memoized
/// registry, or `None` if no adapter has submitted one (e.g. stub-mode build
/// with `cfg(has_occt)` off).
///
/// Centralises the "lex-min on `name`" tie-break used by
/// [`crate::Engine::with_registered_kernel`] (and, in v0.3+, by any
/// dispatcher selection that wants the same fallback ordering). Routing
/// every caller through this helper guarantees the tie-break invariant lives
/// in one place — a future change (e.g. environment-variable-driven default
/// selection) would only need to update this function.
pub fn pick_lexmin_kernel() -> Option<&'static KernelRegistration> {
    registry().values().next().copied()
}

/// Iterate the static linker-collected set of [`KernelRegistration`] records
/// and materialise a `BTreeMap` keyed on each kernel's name, valued on
/// **owned** [`CapabilityDescriptor`]s.
///
/// Returns owned descriptors — the `descriptor` field on `KernelRegistration`
/// is a `fn() -> CapabilityDescriptor` returning by value, so the descriptors
/// cannot be borrowed as `&'static`. Callers wishing to hand the result to
/// `dispatcher::dispatch` (which expects `&BTreeMap<String,
/// &CapabilityDescriptor>`) materialise a borrowed view per dispatch:
///
/// ```ignore
/// let registry = reify_eval::collect_registry();
/// let borrowed: BTreeMap<String, &CapabilityDescriptor> =
///     registry.iter().map(|(k, v)| (k.clone(), v)).collect();
/// reify_eval::dispatch(&borrowed, op, demanded, &available);
/// ```
///
/// Internally delegates to the memoized [`registry`] accessor so the
/// inventory walk is never repeated; the per-call cost is one descriptor
/// invocation per registered kernel plus the surrounding `BTreeMap`
/// allocation. Per the PRD's "read once at engine startup" contract,
/// callers SHOULD NOT call this on the hot dispatch path.
pub fn collect_registry() -> BTreeMap<String, CapabilityDescriptor> {
    registry()
        .iter()
        .map(|(name, reg)| (name.clone(), (reg.descriptor)()))
        .collect()
}

/// Walk `inventory::iter::<KernelRegistration>()` once and produce the
/// `BTreeMap` cached by [`REGISTRY`]. Detects duplicate names and trips a
/// `debug_assert!` plus a `tracing::warn!` so misconfigurations (e.g. two
/// crates submitting `name = "occt"` after a feature-flag refactor) surface
/// loudly in dev/test instead of producing silent arbitrary selection.
fn build_registry() -> BTreeMap<String, &'static KernelRegistration> {
    let mut map: BTreeMap<String, &'static KernelRegistration> = BTreeMap::new();
    for reg in inventory::iter::<KernelRegistration>() {
        if let Some(prev) = map.insert(reg.name.to_string(), reg) {
            let prev_ptr = prev as *const KernelRegistration;
            let new_ptr = reg as *const KernelRegistration;
            tracing::warn!(
                kernel_name = %reg.name,
                ?prev_ptr,
                ?new_ptr,
                "duplicate KernelRegistration submitted: v0.2 design expects unique names \
                 per registered kernel; later submission silently overwrites earlier",
            );
            debug_assert!(
                false,
                "duplicate KernelRegistration name {:?}: prev = {:p}, new = {:p} — \
                 v0.2 design expects unique names per registered kernel",
                reg.name, prev_ptr, new_ptr,
            );
        }
    }
    map
}

// ── Synthetic test kernel ─────────────────────────────────────────────────
//
// Submitted under `#[cfg(test)]` so it appears in this crate's `cargo test
// --lib` build, where the smoke test below asserts non-empty content. Without
// this synthetic the smoke test would pass by construction (`first.len() ==
// second.len() == 0` for any iteration impl that returns nothing), so a
// regression that strips the inventory walk from `build_registry` would slip
// through unit tests entirely. The cross-crate end-to-end pin in
// `crates/reify-eval/tests/kernel_registry_inventory.rs` covers the populated
// case but is itself stub-mode-skipped, which is why the unit-level synthetic
// is needed in addition.
//
// The factory body is `unreachable!()`: any code path that picks the synthetic
// as a real kernel (e.g. `Engine::with_registered_kernel` invoked from a unit
// test) would surface a panic with a clear message. No unit test in
// `reify-eval` currently constructs an `Engine` via that constructor — the
// integration test that does so lives outside `src/` and therefore links the
// lib without `cfg(test)`, leaving the synthetic invisible there.
#[cfg(test)]
mod test_synthetic_kernel {
    use super::*;
    use reify_types::{GeometryKernel, Operation, ReprKind};

    /// Stable name for the cfg(test)-only synthetic kernel. The double-
    /// underscore prefix sorts before any plausible real adapter name in the
    /// `cargo test --lib` build for `reify-eval` — but the synthetic is
    /// invisible to integration test binaries (which compile the lib without
    /// `cfg(test)`) so the lex-min picked by
    /// [`Engine::with_registered_kernel`](crate::Engine::with_registered_kernel)
    /// in real builds is unaffected.
    const SYNTHETIC_KERNEL_NAME: &str = "__test_synthetic_kernel";

    fn synthetic_descriptor() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        }
    }

    fn synthetic_factory() -> Box<dyn GeometryKernel> {
        unreachable!(
            "synthetic test kernel factory must never be invoked: it exists only to \
             give `kernel_registry::tests::collect_registry_returns_typed_btreemap_smoke` \
             non-empty content to assert against. Reaching this branch means a unit \
             test (cargo test --lib for reify-eval) misused `Engine::with_registered_kernel` \
             — the lex-smallest synthetic was instantiated as if it were a real kernel."
        );
    }

    inventory::submit! {
        KernelRegistration {
            name: SYNTHETIC_KERNEL_NAME,
            descriptor: synthetic_descriptor,
            factory: synthetic_factory,
        }
    }

    pub(super) const NAME: &str = SYNTHETIC_KERNEL_NAME;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke pin: the function returns the right type, the result is
    /// deterministic across calls, and the iteration logic is non-trivially
    /// exercised — the cfg(test)-only synthetic registration submitted in
    /// `test_synthetic_kernel` MUST appear in the result.
    ///
    /// Without the synthetic, this test would pass by construction (an empty
    /// BTreeMap trivially equals another empty BTreeMap) for any
    /// implementation of `collect_registry` that returns nothing. Asserting
    /// the synthetic is present means a regression that drops the inventory
    /// walk would actually fail the test.
    #[test]
    fn collect_registry_returns_typed_btreemap_smoke() {
        // Compile-time signature pin: bind into the documented return type.
        let first: BTreeMap<String, CapabilityDescriptor> = collect_registry();
        let second: BTreeMap<String, CapabilityDescriptor> = collect_registry();

        assert_eq!(
            first.len(),
            second.len(),
            "collect_registry must produce maps of equal length across calls — \
             determinism contract for `Selection deterministic given pinned runtime configuration`",
        );

        // BTreeMap iteration is lexicographic on keys regardless of inventory
        // link order — pin this so a future change that swaps the materialised
        // container (e.g. to HashMap) is caught here.
        let first_keys: Vec<&String> = first.keys().collect();
        let second_keys: Vec<&String> = second.keys().collect();
        assert_eq!(
            first_keys, second_keys,
            "key sequence must be identical (lexicographic on kernel name)",
        );

        // Non-trivial coverage: the cfg(test)-only synthetic registration MUST
        // be visible here. This proves the iteration logic actually runs (a
        // regression that returns `BTreeMap::new()` regardless of inventory
        // contents would now fail this assertion).
        assert!(
            first.contains_key(test_synthetic_kernel::NAME),
            "smoke test must observe the cfg(test)-only synthetic kernel \
             ({:?}): collect_registry's iteration logic is not being exercised",
            test_synthetic_kernel::NAME,
        );
    }

    /// Contract pin: `pick_lexmin_kernel()` returns the lexicographically
    /// *smaller* kernel when multiple registrations are present.
    ///
    /// Two `cfg(test)`-only synthetic kernels are registered in
    /// `test_synthetic_kernel`: `__a_kernel` (sorts before `__b_kernel`).
    /// The test asserts:
    /// 1. Both synthetics are visible to `registry()` (proving the inventory
    ///    walk captured all submissions, not just the first).
    /// 2. `pick_lexmin_kernel()` returns `__a_kernel`, not `__b_kernel` —
    ///    the lex-smaller name wins.
    ///
    /// This is NOT tautological: a broken implementation that returns
    /// `registry().values().next()` from a `HashMap` (unordered), or one
    /// that returns the last-inserted entry, would fail assertion (2).
    #[test]
    fn pick_lexmin_kernel_returns_lex_smaller_of_known_pair() {
        // (1) Both named synthetics must be visible — proves the inventory walk
        //     captured all submissions rather than stopping at the first.
        assert!(
            registry().contains_key("__a_kernel"),
            "registry must contain synthetic kernel \"__a_kernel\" — \
             step-2 adds test_synthetic_kernel::NAME_A registration",
        );
        assert!(
            registry().contains_key("__b_kernel"),
            "registry must contain synthetic kernel \"__b_kernel\" — \
             step-2 adds test_synthetic_kernel::NAME_B registration",
        );

        // (2) pick_lexmin_kernel must return the lex-smaller of the two
        //     synthetics. "__a_kernel" < "__b_kernel" in ASCII order,
        //     so __a_kernel must win.
        let lexmin = pick_lexmin_kernel().expect(
            "registry must contain at least the cfg(test) synthetic kernels — \
             see test_synthetic_kernel module",
        );
        assert_eq!(
            lexmin.name, "__a_kernel",
            "pick_lexmin_kernel must return the lex-smallest registered name \
             (\"__a_kernel\"), but got {:?}",
            lexmin.name,
        );
    }
}
