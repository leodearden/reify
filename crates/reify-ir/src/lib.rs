//! Compiled IR and runtime vocabulary for Reify.
//!
//! This is the Phase 3 crate introduced in
//! `docs/prds/core-ast-ir-layering.md` (task ζ). It contains the 15 modules
//! that model *compiled, runtime-level* representations — resolved identifiers,
//! typed values, geometry handles, constraint solvers, warm-start state, etc.
//!
//! # B3 invariant
//!
//! This crate MUST have exactly two `reify-*` dependencies: `reify-core` and
//! `reify-ast`. No other intra-workspace `reify-*` dependency is permitted.
//! The structural invariant is locked in by
//! `crates/reify-ir/tests/dag_invariant.rs`, which reads `Cargo.toml` directly
//! and asserts both conditions. The workspace-wide assertion
//! (`scripts/assert-crate-dag.sh`) arrives under task η per PRD §10.

// `Value` carries a `SampledField` whose `oob_emitted: AtomicBool` introduces
// interior mutability that does NOT participate in `PartialEq`/`Ord`/`Hash`/
// `content_hash`. `BTreeMap<Value, _>` (notably `Value::Map`) therefore preserves
// its ordering invariants, but `clippy::mutable_key_type` still fires. See
// `value.rs::SampledField` for the full rationale.
#![allow(clippy::mutable_key_type)]

pub mod annotation;
pub mod boundary_attachment;
pub mod constraint;
pub mod expr;
pub mod geometry;
pub mod kernel_validation;
pub mod node_traits;
pub mod persistent;
pub mod provenance;
pub mod sampled;
pub mod structure_registry;
pub mod traits;
pub mod value;
pub mod warm;
pub mod warm_registry;

// ── flat root re-exports ─────────────────────────────────────────────────────
// Mirrors the flat surface previously at the reify-types lib root for these 15
// modules. Allows `reify_ir::Value` alongside the module-path form
// `reify_ir::value::Value`.

pub use annotation::{Annotation, AnnotationArg, AnnotationArgValue, has_test_annotation};
pub use boundary_attachment::{BoundaryAssociation, NodeAttachment};
pub use constraint::{
    AutoParam, ConstraintChecker, ConstraintDiagnostics, ConstraintDomain, ConstraintInput,
    ConstraintResult, ConstraintSolver, ObjectiveCombination, ObjectiveProvenance, ObjectiveSense,
    ObjectiveSet, ObjectiveTerm, OptimizedImpl, OptimizedImplInput,
    OptimizedImplOutput, ResolutionProblem, SolveResult, TermContribution,
};
pub use expr::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, CompiledMatchArm,
    DeterminacyPredicateKind, ResolvedFunction, SelectorKind, TAG_AD_HOC_SELECTOR,
    TAG_BIN_OP, TAG_CONDITIONAL, TAG_DETERMINACY_PREDICATE, TAG_FUNCTION_CALL, TAG_INDEX_ACCESS,
    TAG_LAMBDA, TAG_LIST_LITERAL, TAG_LITERAL, TAG_MAP_LITERAL, TAG_MATCH, TAG_META_ACCESS,
    TAG_METHOD_CALL, TAG_OPTION_NONE, TAG_OPTION_SOME, TAG_QUANTIFIER, TAG_RANGE_CONSTRUCTOR,
    TAG_REFLECTIVE_CELL_LIST, TAG_RESOLVE_SELECTOR, TAG_SET_LITERAL, TAG_UN_OP,
    TAG_USER_FUNCTION_CALL, TAG_VALUE_REF, UnOp,
};
pub use geometry::{
    AttributeHistory, AxisSign, BRepKind, BooleanOpHistoryRecords, BooleanOpParents,
    BooleanOpParentsError, CapKind, CapabilityDescriptor, DEFAULT_CONTAINS_TOLERANCE_M,
    DEFAULT_GEO_EQUIV_SAMPLE_COUNT, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
    DeletedRecord, EdgeCurveKind, ElementOrderTag, ExportError, ExportFormat, ExportOptions,
    ExportWarning, FaceSurfaceKind,
    FeatureId, FeatureTag, FeatureTagTable, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, HistoryRecord, KernelAttributeHook,
    KernelAttributeOutcome, KernelHandle, KernelId, KernelRegistration, LocalFeatureOpHistoryRecords,
    LoftOpHistoryRecords, Mesh, ModEntry, Operation, ThreeMfOptions, ThreeMfWarning, write_3mf, write_stl_ascii, write_stl_binary,
    QueryCapability, QueryError, ReprKind, Role, StepKind, StepSchema, SweepOpHistoryRecords, TessError,
    TopologyAttribute, TopologyAttributeTable, VolumeMesh, debug_assert_query_many_invariant,
};
pub use kernel_validation::{
    BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
};
pub use node_traits::{HasNodeKind, NodeKind, NodeTraits, NodeTraitsMap};
pub use persistent::PersistentMap;
pub use provenance::{FieldImportProvenance, SnapshotProvenance};
pub use structure_registry::{StructureMeta, StructureRegistry, StructureTypeId};
pub use traits::{EnumDef, TraitBound, TraitDef, TraitMember, TraitRef, TypeParam};
pub use value::{
    DeterminacyState, ErrorRef, EvalError, FieldSourceKind, Freshness, InterpolationKind,
    KeyedMember, MemberKey, ResultRef, SampledField, SampledGridKind, Satisfaction,
    StructureInstanceData, UndefCause, Value, ValueMap, keyed_member_cell, quaternion_is_finite,
};
pub use warm::{OpaqueState, WarmStartable};
pub use warm_registry::{WarmStartableRegistration, WarmStartableRegistry};
