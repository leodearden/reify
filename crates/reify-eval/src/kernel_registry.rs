//! v0.2 multi-kernel registry collector — TDD step 3 placeholder.
//!
//! This file currently contains only a failing test that references the yet-
//! to-be-added `collect_registry()` function. Step 4 of task 2642 lands the
//! `pub fn collect_registry()` body, the `inventory` dep on reify-eval, and
//! the `pub mod kernel_registry;` / re-export in lib.rs.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use reify_types::CapabilityDescriptor;

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
        let first: BTreeMap<String, CapabilityDescriptor> = super::collect_registry();
        let second: BTreeMap<String, CapabilityDescriptor> = super::collect_registry();

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
