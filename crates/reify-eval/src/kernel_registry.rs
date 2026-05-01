//! v0.2 multi-kernel registry collector.
//!
//! Materialises the static linker-collected set of [`KernelRegistration`]
//! records (submitted by adapter crates via `inventory::submit!`) into a
//! `BTreeMap<String, CapabilityDescriptor>` keyed on kernel name. The
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
//! (`collect_registry`) lives here because the dispatcher and engine —
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

use std::collections::BTreeMap;

use reify_types::{CapabilityDescriptor, KernelRegistration};

/// Iterate the static linker-collected set of [`KernelRegistration`] records
/// and materialise a `BTreeMap` keyed on each kernel's name.
///
/// Returns owned [`CapabilityDescriptor`] values — the `descriptor` field on
/// `KernelRegistration` is a `fn() -> CapabilityDescriptor` returning by value,
/// so the descriptors cannot be borrowed as `&'static`. Callers wishing to
/// hand the result to `dispatcher::dispatch` (which expects `&BTreeMap<String,
/// &CapabilityDescriptor>`) materialise a borrowed view per dispatch:
///
/// ```ignore
/// let registry = reify_eval::collect_registry();
/// let borrowed: BTreeMap<String, &CapabilityDescriptor> =
///     registry.iter().map(|(k, v)| (k.clone(), v)).collect();
/// reify_eval::dispatch(&borrowed, op, demanded, &available);
/// ```
///
/// Called once at engine startup by `Engine::with_registered_kernel`. Per
/// the PRD's "read once at engine startup" contract, callers SHOULD NOT
/// call this on the hot dispatch path.
pub fn collect_registry() -> BTreeMap<String, CapabilityDescriptor> {
    inventory::iter::<KernelRegistration>()
        .map(|reg| (reg.name.to_string(), (reg.descriptor)()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke pin: the function returns the right type and the result is
    /// deterministic across calls.
    ///
    /// Cannot assert specific size / contents because the test binary's
    /// dependency closure (reify-eval's `[dev-dependencies]` includes
    /// `reify-kernel-occt`) means OCCT's `inventory::submit!` fires here
    /// once step 8 lands — any emptiness or count-based assertion would
    /// regress at step 8. The populated end-to-end pin lives in step 9's
    /// integration test (`crates/reify-eval/tests/kernel_registry_inventory.rs`).
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
    }
}
