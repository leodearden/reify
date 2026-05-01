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
/// registry paired with the total registry size, or `None` if no adapter has
/// submitted one (e.g. stub-mode build with `cfg(has_occt)` off).
///
/// Returns the total count alongside the selected entry so callers can pass
/// both directly to [`emit_kernel_selection`] without a second [`registry`]
/// call. The count and the pick are taken from the same snapshot, making their
/// consistency guaranteed rather than merely assumed.
///
/// Centralises the "lex-min on `name`" tie-break used by
/// [`crate::Engine::with_registered_kernel`] (and, in v0.3+, by any
/// dispatcher selection that wants the same fallback ordering). Routing
/// every caller through this helper guarantees the tie-break invariant lives
/// in one place — a future change (e.g. environment-variable-driven default
/// selection) would only need to update this function.
pub fn pick_lexmin_kernel() -> Option<(&'static KernelRegistration, usize)> {
    let reg = registry();
    let total = reg.len();
    reg.values().next().copied().map(|r| (r, total))
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

/// Emit a structured tracing event recording which kernel was selected and how
/// many are registered.
///
/// # Why a helper is extracted
///
/// `Engine::with_registered_kernel` is not unit-testable for the multi-kernel
/// INFO path in `cargo test --lib` for `reify-eval`: the cfg(test) synthetics'
/// factories are all `unreachable!()`, so calling the constructor from a unit
/// test would panic. This free helper takes `(name, total)` as synthetic args,
/// letting us drive both branches from unit tests without touching any factory.
/// The helper also lives next to [`pick_lexmin_kernel`] so the v0.3+ dispatcher
/// path can reuse it (see module doc-comment: "both `Engine::with_registered_kernel`
/// and (in v0.3+) any dispatcher selection share the same tie-break helper").
///
/// # Operator-visibility contract
///
/// | `total`  | level emitted                                          |
/// |----------|--------------------------------------------------------|
/// | `> 1`    | `INFO` — lex-min tie-break among multiple kernels      |
/// | `== 1`   | `DEBUG` — single kernel, no tie-break needed           |
/// | `== 0`   | *(nothing — no kernel available)*                     |
///
/// Branches are mutually exclusive: one event per call, keeping the
/// signal-to-noise clean for `RUST_LOG=info` operators (who see a tie-break
/// notification iff a second kernel adapter was actually registered).
///
/// # Structured fields
///
/// `picked = %name` — name of the selected kernel registration
/// `total_registered = total` — total count visible in the registry at call time
pub(crate) fn emit_kernel_selection(name: &str, total: usize) {
    debug_assert!(total >= 1, "emit_kernel_selection requires total >= 1");
    if total > 1 {
        tracing::info!(
            picked = %name,
            total_registered = total,
            "selected kernel via lex-min tie-break",
        );
    } else if total == 1 {
        tracing::debug!(
            picked = %name,
            "selected kernel from inventory registry",
        );
    }
    // total == 0: no event (matches doc table)
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

// ── Synthetic test kernels ────────────────────────────────────────────────
//
// All synthetic registrations below are `#[cfg(test)]`-only. They appear in
// `cargo test --lib` builds for this crate but are invisible to integration
// test binaries (which compile the lib without `cfg(test)`).
//
// Three synthetics are registered:
//
//   __a_kernel  — lex-min in the test build; descriptor: PrimitiveBox/BRep
//   __b_kernel  — second; descriptor: PrimitiveCylinder/BRep
//   __test_synthetic_kernel — third; descriptor: PrimitiveBox/BRep
//
// ASCII sort order: '_' = 0x5F, 'a' = 0x61, 'b' = 0x62, 't' = 0x74.
// Therefore: __a_kernel < __b_kernel < __test_synthetic_kernel.
//
// This means the lex-min test (`pick_lexmin_kernel_returns_lex_smaller_of_known_pair`)
// can assert pick_lexmin_kernel() == __a_kernel non-tautologically, and the
// smoke test (`collect_registry_returns_typed_btreemap_smoke`) still finds
// __test_synthetic_kernel by its stable NAME constant.
//
// All factories are `unreachable!()`: any code path that instantiates a
// synthetic as a real kernel (e.g. Engine::with_registered_kernel from a unit
// test) surfaces a clear panic. No unit test in reify-eval invokes that
// constructor — the integration test that does lives outside `src/` and links
// the lib without cfg(test), so synthetics are invisible there.
#[cfg(test)]
mod test_synthetic_kernel {
    use super::*;
    use reify_types::{GeometryKernel, Operation, ReprKind};

    // ── __a_kernel ─────────────────────────────────────────────────────────
    // Lex-smallest synthetic in the test build. Used by the lex-min contract
    // test to assert pick_lexmin_kernel() returns the smaller of a known pair.
    pub(super) const NAME_A: &str = "__a_kernel";

    fn descriptor_a() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        }
    }

    // ── __b_kernel ─────────────────────────────────────────────────────────
    // Second-smallest synthetic. Present so the lex-min test can confirm
    // pick_lexmin_kernel() chose __a_kernel over __b_kernel (not just "first
    // synthetic seen" from an unordered walk). Uses PrimitiveCylinder/BRep
    // to provide structural variation from NAME_A and NAME's PrimitiveBox/BRep.
    pub(super) const NAME_B: &str = "__b_kernel";

    fn descriptor_b() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveCylinder, ReprKind::BRep)],
        }
    }

    // ── __test_synthetic_kernel ────────────────────────────────────────────
    // Original synthetic, kept so the smoke test's contains_key(NAME) assertion
    // is unaffected. Uses PrimitiveBox/BRep (same as NAME_A); structural
    // variation lives in NAME_B (PrimitiveCylinder/BRep).
    pub(super) const NAME: &str = "__test_synthetic_kernel";

    fn synthetic_descriptor() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        }
    }

    // ── Shared factory ─────────────────────────────────────────────────────
    // All three synthetics share one unreachable!() factory: the bodies are
    // identical (panics with a clear message), so a single DRY function
    // serves all three registrations.
    fn unreachable_factory() -> Box<dyn GeometryKernel> {
        unreachable!(
            "synthetic test kernel factory must never be invoked: these registrations \
             exist only to give unit tests non-empty and structurally-varied registry \
             content. Reaching this branch means a unit test (cargo test --lib for \
             reify-eval) misused Engine::with_registered_kernel — a synthetic was \
             instantiated as if it were a real kernel."
        );
    }

    inventory::submit! {
        KernelRegistration {
            name: NAME_A,
            descriptor: descriptor_a,
            factory: unreachable_factory,
        }
    }

    inventory::submit! {
        KernelRegistration {
            name: NAME_B,
            descriptor: descriptor_b,
            factory: unreachable_factory,
        }
    }

    inventory::submit! {
        KernelRegistration {
            name: NAME,
            descriptor: synthetic_descriptor,
            factory: unreachable_factory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{Operation, ReprKind};

    /// When `total > 1` (multi-kernel build), `emit_kernel_selection` must emit
    /// exactly one `INFO`-level event and no `DEBUG`-level events at the
    /// `reify_eval::kernel_registry` target.
    ///
    /// This exercises the multi-kernel INFO branch introduced so that an
    /// `RUST_LOG=info` operator sees a tie-break notification iff a second
    /// kernel adapter was actually registered (i.e. the lex-min selection was
    /// non-trivial). Passing `("foo", 3)` as synthetic args avoids invoking any
    /// kernel factory — the helper is decoupled from the inventory walk.
    #[test]
    fn emit_kernel_selection_emits_info_at_lex_min_target_when_total_above_one() {
        use reify_test_support::CountingSubscriberBuilder;
        use std::sync::atomic::Ordering;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::INFO)
            .count_level(tracing::Level::DEBUG)
            .target_prefix("reify_eval::kernel_registry")
            .build();
        let info_count = counters[&tracing::Level::INFO].clone();
        let debug_count = counters[&tracing::Level::DEBUG].clone();

        tracing::subscriber::with_default(subscriber, || {
            emit_kernel_selection("foo", 3);
        });

        assert_eq!(
            info_count.load(Ordering::Acquire),
            1,
            "emit_kernel_selection(name, total > 1) must emit exactly one INFO event \
             at reify_eval::kernel_registry — operator visibility when lex-min tie-break fires",
        );
        assert_eq!(
            debug_count.load(Ordering::Acquire),
            0,
            "emit_kernel_selection(name, total > 1) must not emit DEBUG events — \
             mutually-exclusive branches: only INFO fires in the multi-kernel case",
        );
    }

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

        // Descriptor-content identity pin: assert the .supports vec for the
        // known synthetic holds exactly the entries we expect. The type system
        // already rules out nondeterminism (fn() -> CapabilityDescriptor cannot
        // hold mutable state), so the assertion's value is pinning *which*
        // content the descriptor returns — a future accidental change to
        // synthetic_descriptor() (wrong copy-paste, field removal, etc.) would
        // fail here where a cross-call determinism comparison would not.
        let expected_supports = vec![(Operation::PrimitiveBox, ReprKind::BRep)];
        assert_eq!(
            first.get(test_synthetic_kernel::NAME).map(|d| &d.supports),
            Some(&expected_supports),
            "descriptor .supports for {:?} must be exactly [(PrimitiveBox, BRep)] — \
             update this assertion if the synthetic's descriptor is intentionally changed",
            test_synthetic_kernel::NAME,
        );
    }

    /// When `total == 1` (v0.2 single-kernel build), `emit_kernel_selection`
    /// must emit exactly one `DEBUG`-level event and no `INFO`-level events at
    /// the `reify_eval::kernel_registry` target.
    ///
    /// This exercises the single-kernel DEBUG branch so that an `RUST_LOG=debug`
    /// operator always sees a selection event while an `RUST_LOG=info` operator
    /// only sees events when a lex-min tie-break between multiple kernels
    /// actually occurred. Passing `("only", 1)` avoids invoking any factory.
    #[test]
    fn emit_kernel_selection_emits_debug_only_when_total_is_one() {
        use reify_test_support::CountingSubscriberBuilder;
        use std::sync::atomic::Ordering;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::INFO)
            .count_level(tracing::Level::DEBUG)
            .target_prefix("reify_eval::kernel_registry")
            .build();
        let info_count = counters[&tracing::Level::INFO].clone();
        let debug_count = counters[&tracing::Level::DEBUG].clone();

        tracing::subscriber::with_default(subscriber, || {
            emit_kernel_selection("only", 1);
        });

        assert_eq!(
            info_count.load(Ordering::Acquire),
            0,
            "emit_kernel_selection(name, total == 1) must not emit INFO — \
             INFO is reserved for the multi-kernel tie-break case (total > 1)",
        );
        assert_eq!(
            debug_count.load(Ordering::Acquire),
            1,
            "emit_kernel_selection(name, total == 1) must emit exactly one DEBUG event \
             at reify_eval::kernel_registry — single-kernel selection always visible at RUST_LOG=debug",
        );
    }

    /// The doc table at lines 150-154 declares `total == 0` emits no event.
    /// The `debug_assert!(total >= 1, …)` enforces this structurally: callers
    /// must guarantee `total >= 1` so a future v0.3+ dispatcher reuser cannot
    /// silently call `emit_kernel_selection` with an empty registry and have
    /// the helper quietly emit a spurious DEBUG event. This test confirms the
    /// panic fires when `total == 0` (in debug builds, i.e. `cargo test`).
    #[test]
    #[should_panic(expected = "emit_kernel_selection requires total >= 1")]
    fn emit_kernel_selection_panics_when_total_is_zero() {
        emit_kernel_selection("nothing", 0);
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
            registry().contains_key(test_synthetic_kernel::NAME_A),
            "registry must contain synthetic kernel {:?} — \
             see test_synthetic_kernel::NAME_A",
            test_synthetic_kernel::NAME_A,
        );
        assert!(
            registry().contains_key(test_synthetic_kernel::NAME_B),
            "registry must contain synthetic kernel {:?} — \
             see test_synthetic_kernel::NAME_B",
            test_synthetic_kernel::NAME_B,
        );

        // (2) pick_lexmin_kernel must return the lex-smaller of the two
        //     synthetics. NAME_A = "__a_kernel" < NAME_B = "__b_kernel" in
        //     ASCII order, so __a_kernel must win.
        let lexmin = pick_lexmin_kernel().expect(
            "registry must contain at least the cfg(test) synthetic kernels — \
             see test_synthetic_kernel module",
        );
        assert_eq!(
            lexmin.name,
            test_synthetic_kernel::NAME_A,
            "pick_lexmin_kernel must return the lex-smallest registered name \
             ({:?}), but got {:?}",
            test_synthetic_kernel::NAME_A,
            lexmin.name,
        );
    }
}
