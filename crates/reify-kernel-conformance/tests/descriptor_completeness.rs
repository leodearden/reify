//! Descriptor completeness: every `Operation` the evaluator emits is declared
//! by some registered kernel's `CapabilityDescriptor`, with all five kernels
//! linked. See `docs/prds/v0_6/kernel-interface-typed-seam.md` §8 ι₂.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use reify_ir::{CapabilityDescriptor, KernelId, Operation};
use strum::IntoEnumIterator;

/// Each fn item's address is a linker anchor: it pulls that crate's
/// `register` object into this binary, so its `inventory::submit!` fires.
const LINKED_KERNELS: [fn() -> CapabilityDescriptor; 5] = [
    reify_kernel_fidget::register::fidget_capability_descriptor,
    reify_kernel_gmsh::register::gmsh_capability_descriptor,
    reify_kernel_manifold::register::manifold_capability_descriptor,
    reify_kernel_occt::register::occt_capability_descriptor,
    reify_kernel_openvdb::register::openvdb_capability_descriptor,
];

fn registered_descriptors() -> BTreeMap<String, CapabilityDescriptor> {
    for descriptor in LINKED_KERNELS {
        std::hint::black_box(descriptor());
    }
    let registry = reify_eval::collect_registry();
    let registered: BTreeSet<&str> = registry.keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = KernelId::ALL
        .map(KernelId::as_registry_name)
        .into_iter()
        .collect();
    assert_eq!(
        registered, expected,
        "registered kernels {registered:?} must equal KernelId::ALL {expected:?}; a missing \
         name means that kernel's native dependency is absent or its registration was \
         dead-stripped"
    );
    registry
}

/// `Convert` is a representation change the dispatcher plans between
/// stages; no `GeometryOp` maps to it.
fn never_emitted(op: &Operation) -> bool {
    matches!(op, Operation::Convert { .. })
}

#[test]
fn allow_list_names_exactly_the_operations_no_geometry_op_emits() {
    let emitted: HashSet<Operation> = reify_ir::geometry::GEOMETRY_OP_DESCRIPTORS
        .iter()
        .filter_map(|row| row.operation)
        .collect();
    let expected: HashSet<Operation> = Operation::iter().filter(|op| !never_emitted(op)).collect();
    assert_eq!(
        emitted, expected,
        "the operations GEOMETRY_OP_DESCRIPTORS emits must be exactly Operation::iter() minus \
         `never_emitted`; map a new Operation from a GeometryOp row, or allow-list it in \
         `never_emitted` with its reason"
    );
}

#[test]
fn every_emitted_operation_is_declared_by_some_kernel() {
    let registry = registered_descriptors();
    let undeclared: Vec<Operation> = Operation::iter()
        .filter(|op| !never_emitted(op))
        .filter(|op| {
            !registry
                .values()
                .any(|descriptor| descriptor.supports.iter().any(|(o, _)| o == op))
        })
        .collect();
    assert!(
        undeclared.is_empty(),
        "every emitted Operation must be declared by some kernel's capability descriptor; \
         undeclared: {undeclared:?}; registered kernels: {:?}",
        registry.keys()
    );
}
