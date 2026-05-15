//! Per-Engine structure-definition registry.
//!
//! Maps interned [`StructureTypeId`]s to [`StructureMeta`] (declared trait
//! bounds, `@version(N)`, source span, field layout). Backs the
//! `Value::StructureInstance` side-table per
//! `docs/prds/v0_3/structure-instance-runtime.md` (task SIR-α / 3540).
//!
//! Module skeleton only at this stage — full field definitions and the
//! intern/lookup methods land in a subsequent step.

/// Stable per-Engine identifier for an interned structure definition.
///
/// Opaque `u32` handle into the [`StructureRegistry`] side-table. Not stable
/// across Engine restarts — cache-key composition uses the structure *name*,
/// not this id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructureTypeId(pub u32);

/// Side-table metadata for a structure definition.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StructureMeta;

/// Per-Engine registry mapping structure names ↔ ids and ids → meta.
#[derive(Debug, Clone, Default)]
pub struct StructureRegistry;
