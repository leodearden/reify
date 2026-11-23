//! The id a [`StructureInstanceData`](crate::StructureInstanceData) carries.
//!
//! A leaf module so that `structure_registry` can read instances by name
//! (`structure_registry -> value -> structure_type_id`) without a cycle.

/// Stable per-Engine identifier for an interned structure definition.
///
/// Opaque `u32` handle into the [`StructureRegistry`](crate::StructureRegistry)
/// side-table. Not stable across Engine restarts — cache-key composition uses
/// the structure *name*, not this id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructureTypeId(pub u32);

impl StructureTypeId {
    /// The id of an instance minted without registry access. It carries no
    /// identity: resolve the structure by name through
    /// [`StructureRegistry::meta_for`](crate::StructureRegistry::meta_for).
    pub const REGISTRY_FREE: StructureTypeId = StructureTypeId(u32::MAX);
}
