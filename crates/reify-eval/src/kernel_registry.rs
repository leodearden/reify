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
//! tie-break helper [`pick_lexmin_kernel`]. The `(Operation, ReprKind)`
//! uniqueness diagnostic ([`warn_if_duplicate_op_repr_pairs`]) is also
//! amortised behind the same OnceLock: it runs once inside the init closure
//! alongside [`build_registry`] and never repeats, mirroring the existing
//! duplicate-NAME walk.
//!
//! **Debug-build note:** `OnceLock::get_or_init` leaves the cell
//! uninitialised if the init closure panics (per stdlib semantics). In debug
//! builds, a duplicate `(Operation, ReprKind)` pair triggers a
//! `debug_assert!` inside [`warn_if_duplicate_op_repr_pairs`], which panics
//! and leaves the cell uninitialised. Every subsequent `registry()` call will
//! re-run the closure, re-allocate the descriptor map, re-run the duplicate
//! walk, and re-panic — emitting multiple WARN events rather than exactly
//! one. This only occurs on an adapter misconfiguration (a programming error)
//! in debug builds; release builds follow the WARN path without panicking and
//! the OnceLock is populated normally.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use reify_ir::{CapabilityDescriptor, KernelRegistration, Operation, ReprKind};

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
///
/// # Duplicate `(Operation, ReprKind)` detection
///
/// The OnceLock init closure also invokes each kernel's descriptor function
/// once (lazily, via iterator) and passes the results to
/// [`warn_if_duplicate_op_repr_pairs`], surfacing duplicate-(op, repr)-pair
/// misconfigurations at startup via `tracing::warn!` (and `debug_assert!` in
/// debug builds). No intermediate collection is built — the iterator is
/// consumed directly. This diagnostic runs exactly once per process,
/// structurally guaranteed by the surrounding `OnceLock`.
pub fn registry() -> &'static BTreeMap<String, &'static KernelRegistration> {
    REGISTRY.get_or_init(|| {
        let map = build_registry();
        warn_if_duplicate_op_repr_pairs(
            map.iter()
                .map(|(name, reg)| (name.as_str(), (reg.descriptor)())),
        );
        map
    })
}

/// The lexicographically smallest [`KernelRegistration`] in the memoized
/// registry, or `None` if no adapter has submitted one (e.g. stub-mode build
/// with `cfg(has_occt)` off).
///
/// Centralises the "lex-min on `name`" tie-break used historically by
/// [`crate::Engine::with_registered_kernel`] (and, in v0.3+, by any
/// dispatcher selection that wants the same fallback ordering). Routing
/// every caller through this helper guarantees the tie-break invariant lives
/// in one place — a future change (e.g. environment-variable-driven default
/// selection) would only need to update this function.
///
/// **Engine construction should prefer [`pick_lexmin_brep_kernel`]** which
/// applies a BRep-capability filter first so a Mesh-only kernel registered
/// under a lex-smaller name (e.g. `"manifold" < "occt"`) cannot silently win
/// the pick when a BRep-capable kernel is also registered.  This function
/// retains its existing pure lex-min contract for v0.3 dispatcher reuse and
/// any other caller that explicitly wants the lex-min regardless of capability.
pub fn pick_lexmin_kernel() -> Option<&'static KernelRegistration> {
    registry().values().next().copied()
}

/// Returns the lex-smallest BRep-capable registered kernel, or falls back to
/// lex-min overall when no registered kernel claims any BRep pair.
///
/// Returns `None` on an empty registry (preserving the stub-mode "no kernel
/// registered" semantics used by [`crate::Engine::with_registered_kernel`]).
///
/// # Why prefer this over [`pick_lexmin_kernel`] for engine construction
///
/// `pick_lexmin_kernel` does a pure name-order walk — whichever kernel name
/// sorts first lexicographically wins.  If both `"manifold"` (Mesh-only
/// stub) and `"occt"` (full BRep) are linked into the same binary,
/// `"manifold" < "occt"` so `pick_lexmin_kernel` silently routes every BRep
/// operation through the Manifold stub, which returns `OperationFailed`.
/// This function prevents that by filtering for BRep capability first.
///
/// # Fallback semantics
///
/// When no registered kernel claims any `(_, ReprKind::BRep)` pair (e.g. a
/// hypothetical Mesh-only build), the function falls back to the pure lex-min
/// of all registered kernels rather than returning `None`.  A Mesh-only build
/// still wants *some* kernel; refusing to pick one would degrade further than
/// the current `pick_lexmin_kernel` behaviour.  The fallback chain is:
/// `find(brep) → values().next()`.
///
/// Note: for `Operation::Convert{from}` entries the tuple's repr is the
/// *output*, so a `BRep→Mesh` tessellation kernel would not match this filter
/// even though it consumes BRep input.  Acceptable for v0.2 because OCCT is
/// the only BRep producer.
///
/// # Performance
///
/// Each call invokes `(reg.descriptor)()` once per registered kernel until a
/// BRep-capable entry is found — allocating a fresh `CapabilityDescriptor`
/// (and its inner `Vec`) per iteration.  The `registry()` `OnceLock` does
/// **not** cache these calls.  With v0.2's single OCCT adapter the cost is
/// trivial (one allocation, found immediately), but as v0.3 adds 3–4 adapters
/// the scan is O(N) descriptor allocations per call.  **Do not call this on a
/// hot path.**  It is intended exclusively for one-shot engine construction
/// inside [`crate::Engine::with_registered_kernel`].
///
/// # Consumer
///
/// [`crate::Engine::with_registered_kernel`] uses this function (task 3224).
/// The generic helper [`pick_lexmin_brep_kernel_in`] contains the testable
/// filter-and-fallback logic.
pub fn pick_lexmin_brep_kernel() -> Option<&'static KernelRegistration> {
    pick_lexmin_brep_kernel_in(registry(), |reg| (reg.descriptor)()).copied()
}

/// Generic BRep-preferring lex-min helper over a caller-supplied map.
///
/// Returns the lex-smallest entry in `registered` whose descriptor (as
/// returned by `descriptor_of`) claims at least one `(_, ReprKind::BRep)`
/// pair.  Falls back to the lex-smallest entry overall when no entry claims
/// any BRep pair.  Returns `None` when `registered` is empty.
///
/// # Why a generic helper is extracted
///
/// `registry()` is a `BTreeMap<String, &'static KernelRegistration>` whose
/// descriptor is obtained by calling `(reg.descriptor)()`.  Lifting the
/// filter-and-fallback logic into a generic `pick_lexmin_brep_kernel_in<V>`
/// lets unit tests drive it with a synthetic
/// `BTreeMap<String, CapabilityDescriptor>` and a plain `|d| d.clone()`
/// descriptor closure — covering the three behavioral cases (BRep wins,
/// no-BRep falls back to lex-min, empty returns None) without registering
/// additional `cfg(test)` synthetics that would shift the global lex-min and
/// force coordinated edits to existing tests.
///
/// Mirrors the extraction pattern used for [`emit_kernel_selection`] and
/// [`warn_if_duplicate_op_repr_pairs`].
///
/// # Caller
///
/// [`pick_lexmin_brep_kernel`] is the only production caller.
pub(crate) fn pick_lexmin_brep_kernel_in<V>(
    registered: &BTreeMap<String, V>,
    descriptor_of: impl Fn(&V) -> CapabilityDescriptor,
) -> Option<&V> {
    registered
        .values()
        .find(|v| descriptor_of(v).supports_any_repr(ReprKind::BRep))
        .or_else(|| registered.values().next())
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
/// allocation. The [`warn_if_duplicate_op_repr_pairs`] uniqueness diagnostic
/// runs once per process inside [`registry`]'s OnceLock initialisation, so
/// callers of `collect_registry()` do not pay that cost. The result itself is
/// not memoized by design: callers receive a fresh, mutable `BTreeMap` each
/// call. Per the PRD's "read once at engine startup" contract, callers SHOULD
/// NOT call this on the hot dispatch path.
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
/// Preconditions: callers must pass `total >= 1` (enforced via `debug_assert!`).
///
/// | `total`  | level emitted                                          |
/// |----------|--------------------------------------------------------|
/// | `> 1`    | `INFO` — lex-min tie-break among multiple kernels      |
/// | `== 1`   | `DEBUG` — single kernel, no tie-break needed           |
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
    // total == 0: unreachable in debug builds (debug_assert above panics); release-mode no-op.
}

/// Check that no two kernels in `registered` claim the same
/// `(Operation, ReprKind)` pair.
///
/// # Why a helper is extracted
///
/// `collect_registry()` materialises the `BTreeMap<String, CapabilityDescriptor>`
/// from the inventory walk, so this uniqueness check naturally lives there.
/// Extracting it as a `pub(crate)` free function lets unit tests drive the
/// collision path with synthetic [`CapabilityDescriptor`] values without
/// touching `inventory::submit!` (process-global; would corrupt the
/// `OnceLock`-memoized registry seen by every other test in the binary).
/// This is the same testability rationale that motivated extracting
/// [`emit_kernel_selection`].
///
/// # Always-warn / debug-only-panic semantics
///
/// On a collision the function always emits a `tracing::warn!` (operator
/// visibility in release builds where `debug_assert!` compiles to a no-op)
/// followed by `debug_assert!(false, ...)` that panics in debug builds.
/// Mirrors [`build_registry`]'s duplicate-name detection pattern verbatim.
///
/// # Iterator-based signature
///
/// Accepts `impl IntoIterator<Item = (&'a str, CapabilityDescriptor)>` so
/// callers can pass a lazily-mapped iterator directly (e.g. from
/// `map.iter().map(|(name, reg)| (name.as_str(), (reg.descriptor)()))`)
/// without building a throwaway `BTreeMap<String, CapabilityDescriptor>`.
/// The `&'a str` keys must outlive the internal `seen` map — they are
/// borrowed from the caller's key collection, not from the iterator itself.
///
/// # Why `HashMap`, not `BTreeSet`
///
/// [`Operation`] derives `Hash + Eq` but **not** `Ord/PartialOrd`, so
/// `BTreeSet<(Operation, ReprKind)>` would not compile without adding `Ord`
/// to `Operation` (scope expansion into `reify-types`, not in this task's
/// listed modules). `HashMap<(Operation, ReprKind), &str>` requires only the
/// existing `Hash + Eq` and additionally records the previous claimer's name
/// so the panic message reads `"kernels: {prev} vs {new}"` — a bare
/// `insert() -> bool` would discard the previous owner's identity.
/// Iteration-order determinism is preserved by the outer iterator order
/// (lexicographic on kernel name when driven from a `BTreeMap`) and the
/// inner `Vec` order (`supports` insertion order); `HashMap` only stores
/// the lookup, not the iteration order.
pub(crate) fn warn_if_duplicate_op_repr_pairs<'a>(
    registered: impl IntoIterator<Item = (&'a str, CapabilityDescriptor)>,
) {
    let mut seen: HashMap<(Operation, ReprKind), &'a str> = HashMap::new();
    for (name, descriptor) in registered {
        for &(op, repr) in &descriptor.supports {
            if let Some(prev_owner) = seen.insert((op, repr), name) {
                if prev_owner == name {
                    // Intra-kernel: this kernel's own `supports` Vec lists the same
                    // pair twice (e.g. a copy-paste bug in an adapter's descriptor
                    // function).  Disambiguate from the inter-kernel case so the
                    // diagnostic message isn't the confusing "kernels: foo vs foo".
                    tracing::warn!(
                        op = ?op,
                        repr = ?repr,
                        kernel = %name,
                        "duplicate kernel claim for (Operation, ReprKind) pair: \
                         kernel lists the same (op, repr) pair twice in its supports \
                         table; dispatcher's lex-min tie-break would silently pick a \
                         winner",
                    );
                    debug_assert!(
                        false,
                        "duplicate kernel claim for ({:?}, {:?}) — kernel {} lists \
                         the same pair twice in its supports table",
                        op, repr, name,
                    );
                } else {
                    // Inter-kernel: two different kernels claim the same pair.
                    tracing::warn!(
                        op = ?op,
                        repr = ?repr,
                        prev_kernel = prev_owner,
                        new_kernel = %name,
                        "duplicate kernel claim for (Operation, ReprKind) pair: \
                         v0.2 design expects each pair claimed by at most one kernel; \
                         dispatcher's lex-min tie-break would silently pick a winner",
                    );
                    debug_assert!(
                        false,
                        "duplicate kernel claim for ({:?}, {:?}) — kernels: {} vs {}",
                        op, repr, prev_owner, name,
                    );
                }
            }
        }
    }
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
// Four synthetics are registered:
//
//   __0_mesh_kernel — lex-min (BTreeMap key order) in the test build;
//                     descriptor: BooleanUnion/Mesh (NO BRep entry).
//                     Added in task 3224 to exercise the BRep filter in
//                     pick_lexmin_brep_kernel(): this entry sorts before
//                     __a_kernel but must not win the brep-preferring pick.
//   __a_kernel      — second in lex order; descriptor: PrimitiveBox/BRep.
//                     BRep-capable → must be returned by pick_lexmin_brep_kernel().
//   __b_kernel      — third; descriptor: PrimitiveCylinder/BRep.
//   __test_synthetic_kernel — fourth; descriptor: PrimitiveSphere/BRep.
//
// ASCII sort order:
//   '0' = 0x30, '_' = 0x5F, 'a' = 0x61, 'b' = 0x62, 't' = 0x74.
// For names starting with "__":
//   __0_mesh_kernel < __a_kernel < __b_kernel < __test_synthetic_kernel
// because '0' (0x30) < 'a' (0x61).
//
// Impact on existing tests:
//   pick_lexmin_kernel_returns_lex_smaller_of_known_pair:
//     Assertion (2): `lexmin.name <= NAME_A` — still satisfied because
//       __0_mesh_kernel < __a_kernel, so lexmin.name == "__0_mesh_kernel" ≤ __a_kernel.
//     Assertion (2b): `lexmin.name < NAME_B` — still satisfied for the same reason.
//     Assertion (3): `lexmin.name == registry().keys().next()` — still satisfied
//       because __0_mesh_kernel is now the BTreeMap minimum, and pick_lexmin_kernel()
//       returns values().next() = the BTreeMap minimum. All three assertions hold.
//
//   collect_registry_returns_typed_btreemap_smoke:
//     The distinctness and content assertions reference NAME_A and NAME_B by their
//     stable names; adding __0_mesh_kernel as a fourth entry doesn't affect them.
//
// All factories are `unreachable!()`: any code path that instantiates a
// synthetic as a real kernel (e.g. Engine::with_registered_kernel from a unit
// test) surfaces a clear panic. No unit test in reify-eval invokes that
// constructor — the integration test that does lives outside `src/` and links
// the lib without cfg(test), so synthetics are invisible there.
#[cfg(test)]
mod test_synthetic_kernel {
    use super::*;
    use reify_ir::{GeometryKernel, Operation, ReprKind};

    // ── __0_mesh_kernel ────────────────────────────────────────────────────
    // BTreeMap-minimum synthetic (task 3224). Uses BooleanUnion/Mesh so it
    // claims NO BRep entry — pick_lexmin_brep_kernel() must skip it and return
    // __a_kernel instead. Name prefix '0' (0x30) sorts before 'a' (0x61), so
    // __0_mesh_kernel is lex-smaller than __a_kernel.
    pub(super) const NAME_MESH_ONLY: &str = "__0_mesh_kernel";

    fn descriptor_mesh_only() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        }
    }

    // ── __a_kernel ─────────────────────────────────────────────────────────
    // Lex-second synthetic (after __0_mesh_kernel) in the test build. Used by
    // the lex-min contract test and the BRep-preference test to verify
    // pick_lexmin_brep_kernel() returns the lex-smallest BRep-capable entry.
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
    // to provide structural variation from NAME_A's PrimitiveBox/BRep.
    pub(super) const NAME_B: &str = "__b_kernel";

    fn descriptor_b() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveCylinder, ReprKind::BRep)],
        }
    }

    // ── __test_synthetic_kernel ────────────────────────────────────────────
    // Original synthetic, kept so the smoke test's contains_key(NAME) assertion
    // is unaffected. Uses a distinct descriptor (PrimitiveSphere/BRep) so that
    // all three synthetics have structurally-unique descriptors:
    //   NAME_A → PrimitiveBox/BRep, NAME_B → PrimitiveCylinder/BRep, NAME → PrimitiveSphere/BRep.
    pub(super) const NAME: &str = "__test_synthetic_kernel";

    fn descriptor_name() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveSphere, ReprKind::BRep)],
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
            name: NAME_MESH_ONLY,
            descriptor: descriptor_mesh_only,
            factory: unreachable_factory,
        }
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
            descriptor: descriptor_name,
            factory: unreachable_factory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{Operation, ReprKind};

    /// Shared assertion harness for the two `*_always_emits_warn_*` tests.
    ///
    /// Drives `warn_if_duplicate_op_repr_pairs(fixture)` under a WARN-counting
    /// subscriber scoped to `reify_eval::kernel_registry`, swallowing the
    /// debug-mode panic via `catch_unwind` so the warn count can be observed,
    /// then asserts exactly one WARN was emitted. `ctx` is interpolated into
    /// the failure message so each callsite remains diagnosable.
    pub(super) fn assert_emits_one_warn(
        fixture: &BTreeMap<String, CapabilityDescriptor>,
        ctx: &str,
    ) {
        use reify_test_support::CountingSubscriberBuilder;
        use std::sync::atomic::Ordering;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::kernel_registry")
            .build();
        let warn_count = counters[&tracing::Level::WARN].clone();

        tracing::subscriber::with_default(subscriber, || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                warn_if_duplicate_op_repr_pairs(
                    fixture.iter().map(|(k, v)| (k.as_str(), v.clone())),
                );
            }));
        });

        assert_eq!(
            warn_count.load(Ordering::Acquire),
            1,
            "warn_if_duplicate_op_repr_pairs must emit exactly one WARN event \
             at reify_eval::kernel_registry for {ctx} — operator visibility contract: \
             warn! fires in all builds, not just debug",
        );
    }

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
    /// exercised. The test pins:
    ///
    /// (a) compile-time signature: return type is `BTreeMap<String, CapabilityDescriptor>`,
    /// (b) cross-call determinism: two consecutive calls produce maps of equal length and
    ///     identical key sequences,
    /// (c) lexicographic key ordering: key iteration order is stable across calls,
    /// (d) NAME presence: the cfg(test)-only `__test_synthetic_kernel` appears in the result,
    /// (e) NAME_A / NAME_B descriptor wiring distinctness: the descriptor stored under
    ///     NAME_A has different `.supports` content from the descriptor stored under NAME_B —
    ///     catches a one-sided wiring regression (NAME_A→`descriptor_b` while NAME_B stays,
    ///     making both identical). Note: a *paired* swap (NAME_A→`descriptor_b` AND
    ///     NAME_B→`descriptor_a`) produces still-distinct results and evades this `!=`.
    /// (f) NAME_A descriptor content: NAME_A's `.supports` equals `[(PrimitiveBox, BRep)]`,
    ///     asserted using `reify_types` production constants (not cfg(test) fixture literals).
    ///     This catches the paired-swap blind spot of (e): a paired swap would leave
    ///     NAME_A returning `PrimitiveCylinder/BRep` instead of `PrimitiveBox/BRep`.
    ///
    /// Items (e) and (f) together cover all wiring regressions. The RHS in (f) uses
    /// `Operation::PrimitiveBox` / `ReprKind::BRep` from `reify_types` — production
    /// enum-variant constants independent of the cfg(test) fixture functions — so this
    /// is not the "paired-edit defeats" fragility that a fixture-vs-fixture pin would have.
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

        // Descriptor-wiring distinctness (one-sided regression guard): NAME_A and NAME_B
        // must map to *different* .supports content. Catches a one-sided swap (NAME_A's
        // submit→descriptor_b while NAME_B stays→descriptor_b, making both identical),
        // but NOT a paired swap (NAME_A→descriptor_b AND NAME_B→descriptor_a) — both
        // entries remain distinct after a paired swap, so this `!=` still passes.
        // The content pin below covers the paired-swap blind spot.
        assert_ne!(
            first
                .get(test_synthetic_kernel::NAME_A)
                .map(|d| &d.supports),
            first
                .get(test_synthetic_kernel::NAME_B)
                .map(|d| &d.supports),
            "descriptor .supports for {:?} and {:?} must differ — \
             they use distinct descriptor functions (descriptor_a vs descriptor_b); \
             equal content would indicate a wiring regression in inventory::submit!",
            test_synthetic_kernel::NAME_A,
            test_synthetic_kernel::NAME_B,
        );
        // NAME_A content pin (paired-swap guard): NAME_A's .supports must equal
        // [(PrimitiveBox, BRep)]. Uses Operation::PrimitiveBox / ReprKind::BRep from
        // reify_types — production constants independent of cfg(test) fixture functions.
        // A paired swap (NAME_A→descriptor_b, NAME_B→descriptor_a) satisfies the `!=`
        // above (both entries still differ) but fails here: NAME_A would return
        // PrimitiveCylinder/BRep instead of PrimitiveBox/BRep.
        assert_eq!(
            first
                .get(test_synthetic_kernel::NAME_A)
                .map(|d| &d.supports),
            Some(&vec![(Operation::PrimitiveBox, ReprKind::BRep)]),
            "NAME_A descriptor must have supports [(PrimitiveBox, BRep)] — \
             descriptor_a() must be wired to NAME_A's inventory::submit!; \
             RHS uses reify_types constants, not cfg(test) fixture literals",
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

    /// When the lex-smallest registered kernel is Mesh-only,
    /// `pick_lexmin_brep_kernel_in` must return the lex-smallest BRep-capable
    /// entry instead.
    ///
    /// Constructs a synthetic `BTreeMap` with `"__0_mesh"` (Mesh-only,
    /// lex-smaller) and `"__a_brep"` (BRep-capable, lex-larger). The BTreeMap
    /// minimum is `"__0_mesh"`, so a pure lex-min pick would return it.
    /// `pick_lexmin_brep_kernel_in` must skip it and return `"__a_brep"`'s
    /// descriptor instead.
    ///
    /// This test is RED before step-2 impl: `pick_lexmin_brep_kernel_in` does
    /// not yet exist, so the test fails to compile.
    #[test]
    fn pick_lexmin_brep_kernel_in_returns_brep_capable_when_lex_smaller_kernel_is_mesh_only() {
        let mut map: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        map.insert(
            "__0_mesh".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
            },
        );
        map.insert(
            "__a_brep".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
            },
        );

        let result = pick_lexmin_brep_kernel_in(&map, |d| d.clone());

        let brep_desc = map.get("__a_brep").expect("__a_brep must be in the map");
        assert_eq!(
            result.map(|d| &d.supports),
            Some(&brep_desc.supports),
            "pick_lexmin_brep_kernel_in must return the BRep-capable entry (__a_brep), \
             not the lex-smaller Mesh-only entry (__0_mesh); \
             a pure lex-min pick would wrongly select __0_mesh",
        );
    }

    /// When NO registered kernel claims any BRep pair, `pick_lexmin_brep_kernel_in`
    /// must fall back to the lex-min of all registered kernels.
    ///
    /// Constructs a synthetic BTreeMap with two Mesh-only entries:
    /// `"__0_mesh"` and `"__1_mesh"`.  No BRep-capable entry exists, so the
    /// BRep filter produces no match.  The fallback should select `"__0_mesh"`
    /// (lex-min of the full map).
    ///
    /// This test is RED before step-4 impl: the current helper has no fallback,
    /// so it returns `None` instead of the expected lex-min value.
    #[test]
    fn pick_lexmin_brep_kernel_in_falls_back_to_lex_min_when_no_brep_kernel() {
        let mut map: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        map.insert(
            "__0_mesh".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
            },
        );
        map.insert(
            "__1_mesh".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanDifference, ReprKind::Mesh)],
            },
        );

        let result = pick_lexmin_brep_kernel_in(&map, |d: &CapabilityDescriptor| d.clone());

        // __0_mesh is lex-min; expect fallback to return its descriptor.
        let expected = map.get("__0_mesh").expect("__0_mesh must be in the map");
        assert_eq!(
            result.map(|d| &d.supports),
            Some(&expected.supports),
            "pick_lexmin_brep_kernel_in must fall back to lex-min (__0_mesh) \
             when no entry claims a BRep pair; got None instead of the expected \
             fallback (step-4 impl adds .or_else(|| registered.values().next()))",
        );
    }

    /// An empty registry must produce `None` from `pick_lexmin_brep_kernel_in`.
    ///
    /// This pins the empty-registry contract relied on by
    /// [`crate::Engine::with_registered_kernel`]'s "no kernel registered"
    /// diagnostic path (stub-mode build with `cfg(has_occt)` off).
    ///
    /// After step-4, both `find(brep)` and `values().next()` on an empty map
    /// return `None`, so the test already passes — it is written here for
    /// explicit contract documentation and as a guard against future refactors
    /// that might accidentally return a sentinel value on empty input.
    #[test]
    fn pick_lexmin_brep_kernel_in_returns_none_for_empty_registry() {
        let map: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        let result = pick_lexmin_brep_kernel_in(&map, |d: &CapabilityDescriptor| d.clone());
        assert_eq!(
            result, None,
            "pick_lexmin_brep_kernel_in must return None for an empty registry — \
             preserves Engine::with_registered_kernel's 'no kernel registered' semantics",
        );
    }

    /// The Operator-visibility contract table on `emit_kernel_selection`
    /// declares `total == 0` emits no event.
    /// The `debug_assert!(total >= 1, …)` enforces this structurally: callers
    /// must guarantee `total >= 1` so a future v0.3+ dispatcher reuser cannot
    /// silently call `emit_kernel_selection` with an empty registry and have
    /// the helper quietly emit a spurious DEBUG event. This test confirms the
    /// panic fires when `total == 0` (in debug builds, i.e. `cargo test`).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "emit_kernel_selection requires total >= 1")]
    fn emit_kernel_selection_panics_when_total_is_zero() {
        emit_kernel_selection("nothing", 0);
    }

    /// The helper `warn_if_duplicate_op_repr_pairs` must panic in debug
    /// builds when two kernels claim the same `(Operation, ReprKind)` pair.
    ///
    /// Constructs a synthetic `BTreeMap<String, CapabilityDescriptor>` with
    /// two entries — `"kernel_a"` and `"kernel_b"` — both claiming
    /// `(BooleanUnion, BRep)` in their `supports` tables, then calls the
    /// helper directly. This bypasses the global `inventory` registry (no
    /// `OnceLock` mutation, no test pollution) following the same testability
    /// rationale as the existing `emit_kernel_selection` extraction.
    ///
    /// The `#[cfg(debug_assertions)]` guard is required because `debug_assert!`
    /// compiles to a no-op in release builds — `#[should_panic]` would falsely
    /// pass if the test ran in a build where the assertion is elided.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "duplicate kernel claim for")]
    fn warn_if_duplicate_op_repr_pairs_panics_on_duplicate_pair() {
        let mut registered: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        registered.insert(
            "kernel_a".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
            },
        );
        registered.insert(
            "kernel_b".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
            },
        );
        warn_if_duplicate_op_repr_pairs(registered.iter().map(|(k, v)| (k.as_str(), v.clone())));
    }

    /// Contract pin: `warn_if_duplicate_op_repr_pairs` must emit exactly one
    /// `WARN`-level event when a duplicate `(Operation, ReprKind)` pair is
    /// detected, regardless of whether debug assertions are enabled.
    ///
    /// The `tracing::warn!` call is straight-line code that precedes the
    /// `debug_assert!`, so it always fires — even in release builds where the
    /// panic is compiled out.  This test pins that "always-warn" contract
    /// explicitly using `CountingSubscriberBuilder`, complementing the
    /// `#[should_panic]` test above which only exercises the debug-build path.
    ///
    /// In debug builds the helper panics after emitting WARN.  We wrap the
    /// call in `std::panic::catch_unwind` so the panic does not propagate to
    /// the test runner before we can read `warn_count`.
    #[test]
    fn warn_if_duplicate_op_repr_pairs_always_emits_warn_on_duplicate() {
        let mut registered: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        registered.insert(
            "kernel_a".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
            },
        );
        registered.insert(
            "kernel_b".to_string(),
            CapabilityDescriptor {
                supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
            },
        );

        assert_emits_one_warn(&registered, "an inter-kernel duplicate (op, repr) pair");
    }

    /// Contract pin: `warn_if_duplicate_op_repr_pairs` must emit exactly one
    /// `WARN`-level event via the **intra-kernel** branch when a single
    /// kernel's `supports` Vec contains the same `(Operation, ReprKind)` pair
    /// more than once.
    ///
    /// This mirrors
    /// `warn_if_duplicate_op_repr_pairs_always_emits_warn_on_duplicate`
    /// (the inter-kernel warn test) which covers the inter-kernel branch.  A
    /// separate test is needed here because an arm-swap regression that drops
    /// only the intra-kernel `tracing::warn!` would still pass the inter-kernel
    /// test (inter-kernel scenario unchanged).  Pinning the warn count for the
    /// intra-kernel fixture independently ensures the `warn!` call is present
    /// in both branches.
    ///
    /// The `tracing::warn!` call is straight-line code that precedes the
    /// `debug_assert!`, so it always fires — even in release builds where the
    /// panic is compiled out.  In debug builds the helper panics after emitting
    /// WARN; we wrap the call in `std::panic::catch_unwind` inside the
    /// subscriber scope so `warn_count` is incremented before we assert on it.
    ///
    /// ## Coverage note
    ///
    /// `CountingSubscriberBuilder` counts events by level and target; it does
    /// not capture event fields.  Both the intra-kernel and inter-kernel arms
    /// emit at the same level (`WARN`) and target (`reify_eval::kernel_registry`),
    /// so this count-only assertion cannot distinguish *which* arm emitted the
    /// event.  An arm-swap regression would still pass this test.
    ///
    /// That gap is acceptable here because:
    /// * There is no in-tree behavioral oracle for branch routing within the
    ///   intra-kernel arm.  If a future regression makes branch routing
    ///   observable, the right fix is to extend `CountingSubscriberBuilder` in
    ///   `reify-test-support` to capture event fields and assert on the field
    ///   set (`kernel` only vs `prev_kernel`/`new_kernel`) — not to extract the
    ///   panic-message substring into a shared `const`.
    /// * This test pins the orthogonal *operator-visibility* contract — that
    ///   `warn!` fires in **all** builds, including release where `debug_assert!`
    ///   is compiled out.  That contract holds regardless of arm identity.
    ///
    /// Field-level verification (asserting presence of `kernel` field rather
    /// than `prev_kernel`/`new_kernel`) would require extending
    /// `CountingSubscriberBuilder` in `reify-test-support` to capture recorded
    /// fields — a larger change intentionally deferred.
    #[test]
    fn warn_if_duplicate_op_repr_pairs_always_emits_warn_on_intra_kernel_duplicate() {
        let mut registered: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        registered.insert(
            "kernel_a".to_string(),
            CapabilityDescriptor {
                supports: vec![
                    (Operation::BooleanUnion, ReprKind::BRep),
                    (Operation::BooleanUnion, ReprKind::BRep),
                ],
            },
        );

        assert_emits_one_warn(&registered, "an intra-kernel duplicate (op, repr) pair");
    }

    /// `pick_lexmin_brep_kernel()` must return the lex-smallest BRep-capable
    /// entry from the global registry, NOT the lex-smaller `__0_mesh_kernel`
    /// synthetic which is Mesh-only.
    ///
    /// Asserts:
    /// (a) `registry().contains_key("__0_mesh_kernel")` — the Mesh-only
    ///     synthetic is registered and visible in the global walk.
    /// (b) `pick_lexmin_brep_kernel().name == NAME_A` — the BRep filter picks
    ///     `__a_kernel` over the lex-smaller `__0_mesh_kernel`.
    ///
    /// This test is RED before step-8 impl: both `pick_lexmin_brep_kernel` and
    /// `__0_mesh_kernel` synthetic do not yet exist in the global registry.
    #[test]
    fn pick_lexmin_brep_kernel_returns_lex_smallest_brep_capable_synthetic_when_lex_smaller_mesh_only_synthetic_present()
     {
        // (a) The Mesh-only synthetic must be visible in the global registry.
        assert!(
            registry().contains_key(test_synthetic_kernel::NAME_MESH_ONLY),
            "registry must contain the Mesh-only synthetic {:?} (step-8 adds it); \
             if absent, the BRep-preference test has no Mesh-only entry to filter",
            test_synthetic_kernel::NAME_MESH_ONLY,
        );
        // (b) pick_lexmin_brep_kernel must return __a_kernel (BRep-capable),
        //     not __0_mesh_kernel (Mesh-only, lex-smaller in ASCII order).
        let picked = pick_lexmin_brep_kernel()
            .expect("pick_lexmin_brep_kernel must return Some in a cfg(test) build");
        assert_eq!(
            picked.name,
            test_synthetic_kernel::NAME_A,
            "pick_lexmin_brep_kernel must return __a_kernel ({:?}), not the lex-smaller \
             Mesh-only synthetic __0_mesh_kernel ({:?}); \
             '0'=0x30 < 'a'=0x61 so __0_mesh_kernel < __a_kernel in ASCII order",
            test_synthetic_kernel::NAME_A,
            test_synthetic_kernel::NAME_MESH_ONLY,
        );
    }

    /// `openvdb_kernel_name()` returns the canonical registry name for the
    /// OpenVDB kernel — the same string as
    /// `reify_kernel_openvdb::register::OPENVDB_KERNEL_NAME`.
    ///
    /// Pins the centralized accessor used by δ's projection arm so that a
    /// future rename of the kernel's registry entry is caught here rather
    /// silently diverging in `realization_content.rs`.
    ///
    /// Also asserts (under `has_openvdb`) that `registry()` actually contains
    /// the key, verifying that the name survives the round-trip through the
    /// inventory walk and BTreeMap materialization.
    #[test]
    fn openvdb_kernel_name_matches_register_constant_and_is_in_registry_under_has_openvdb() {
        let canonical = reify_kernel_openvdb::register::OPENVDB_KERNEL_NAME;
        let returned = openvdb_kernel_name();
        assert_eq!(
            returned,
            canonical,
            "openvdb_kernel_name() must equal reify_kernel_openvdb::register::OPENVDB_KERNEL_NAME \
             ({canonical:?}); got {returned:?}",
        );

        #[cfg(has_openvdb)]
        {
            assert!(
                registry().contains_key(returned),
                "registry() must contain the OpenVDB kernel name {:?} under has_openvdb; \
                 kernel likely failed to submit its KernelRegistration",
                returned,
            );
        }
    }

    /// Contract pin: `pick_lexmin_kernel()` returns the lexicographically
    /// *smaller* kernel when multiple registrations are present.
    ///
    /// Two `cfg(test)`-only synthetic kernels are registered in
    /// `test_synthetic_kernel`: `__a_kernel` (sorts before `__b_kernel`).
    /// The test asserts:
    /// 1. Both synthetics are visible to `registry()` (proving the inventory
    ///    walk captured all submissions, not just the first).
    /// 2. `pick_lexmin_kernel()` returns a name `<= NAME_A` AND `< NAME_B` —
    ///    bounds the result against known synthetics for human-readable failure
    ///    messages; a future synthetic lex-smaller than NAME_A would become the
    ///    new BTreeMap minimum and still satisfy both bounds.
    /// 3. `pick_lexmin_kernel()` returns the same name as `registry().keys().next()` —
    ///    pins the actual semantic contract ("lex-min = BTreeMap-minimum key") directly.
    ///    Future-proof: a future lex-smaller synthetic becomes the new minimum and
    ///    satisfies this assertion without falsely breaking the test.
    ///
    /// This is NOT tautological: a broken implementation that returns
    /// `registry().values().next()` from a `HashMap` (unordered), or one
    /// that returns the last-inserted entry, would fail assertion (2) (which
    /// requires the result to be strictly less than NAME_B = `"__b_kernel"`).
    /// Assertion (3) pins the strongest form of the contract: the result must
    /// equal the actual BTreeMap minimum key, independent of any specific
    /// synthetic name. A future lex-smaller synthetic satisfies both (2) and (3)
    /// simultaneously — no test breakage from new synthetics.
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

        // (2) pick_lexmin_kernel must return a name bounded by [lex-min, NAME_B).
        //     NAME_A = "__a_kernel" < NAME_B = "__b_kernel" in ASCII order.
        //     The two-assertion pair rules out HashMap-/last-wins implementations
        //     without coupling to the absence of any future lex-smaller synthetic.
        let lexmin = pick_lexmin_kernel().expect(
            "registry must contain at least the cfg(test) synthetic kernels — \
             see test_synthetic_kernel module",
        );
        assert!(
            lexmin.name <= test_synthetic_kernel::NAME_A,
            "pick_lexmin_kernel must return a name <= NAME_A ({:?}), but got {:?}; \
             a future cfg(test) synthetic with a lex-smaller name (e.g. __0_kernel) \
             would still satisfy this bound — it remains meaningful as new synthetics \
             are added",
            test_synthetic_kernel::NAME_A,
            lexmin.name,
        );
        assert!(
            lexmin.name < test_synthetic_kernel::NAME_B,
            "pick_lexmin_kernel must return a name strictly < NAME_B ({:?}), but got {:?}; \
             this rules out HashMap-/last-wins implementations: any value at NAME_B-or-later \
             fails here. Combined with the previous assertion, the lex-min ordering \
             contract is pinned without hard-coupling to NAME_A specifically",
            test_synthetic_kernel::NAME_B,
            lexmin.name,
        );
        // Tightest pin: lex-min must equal the BTreeMap's actual first key.
        // This is the most direct expression of the contract. A future cfg(test)
        // synthetic lex-smaller than NAME_A becomes the new BTreeMap minimum and
        // still satisfies this assertion — no false breakage. Catches any
        // HashMap-/iterator-order implementation that can't guarantee .next() == min.
        assert_eq!(
            lexmin.name,
            registry().keys().next().unwrap().as_str(),
            "pick_lexmin_kernel must return the BTreeMap-minimum key; \
             this pins the actual lex-min contract independently of any specific synthetic name",
        );
    }
}
