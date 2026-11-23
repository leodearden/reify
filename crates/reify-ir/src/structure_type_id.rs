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
    /// The id carried by an instance no registry entry stands behind, minted
    /// only via [`StructureInstanceData::registry_free`](crate::StructureInstanceData::registry_free).
    pub const REGISTRY_FREE: StructureTypeId = StructureTypeId(u32::MAX);
}
