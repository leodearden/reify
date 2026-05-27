// `Value` carries a `SampledField` whose `oob_emitted: AtomicBool` introduces
// interior mutability that does NOT participate in `PartialEq`/`Ord`/`Hash`/
// `content_hash`. `BTreeMap<Value, _>` (notably `Value::Map`) therefore preserves
// its ordering invariants, but `clippy::mutable_key_type` still fires. See
// `value.rs::SampledField` for the full rationale.
#![allow(clippy::mutable_key_type)]

// ── reify-core re-exports ────────────────────────────────────────────────────
// These eight modules now live in the `reify-core` leaf crate (task γ of
// docs/prds/core-ast-ir-layering.md).  Re-exporting them AS modules preserves
// the `reify_types::diagnostics::SourceSpan` module-path spellings used by 15
// files in the workspace, while the flat re-exports below preserve the
// `reify_types::SourceSpan` spellings.  No downstream crate requires changes.
pub use reify_core::{
    diagnostics, dimension, hash, identity, primitives, source_location, spanned_ident, ty,
};

// ── reify-ast re-exports ─────────────────────────────────────────────────────
// The parsed AST module now lives in the `reify-ast` crate (task δ of
// docs/prds/core-ast-ir-layering.md). Re-exporting it AS a module preserves
// the `reify_types::ast::Expr` module-path spellings. The flat re-exports
// below (`pub use ast::{…}`) continue to resolve unchanged — `ast` is now a
// re-exported module rather than a local `pub mod`, but the path is identical.
pub use reify_ast::ast;

// ── reify-ir re-exports ──────────────────────────────────────────────────────
// The 15 compiled-IR / runtime modules now live in the `reify-ir` crate (task ζ
// of docs/prds/core-ast-ir-layering.md). Re-exporting them AS modules preserves
// the `reify_types::value::Value`-style module-path spellings used across the
// workspace. The flat re-exports below (`pub use value::{…}`, etc.) continue to
// resolve unchanged — module names resolve through these re-exported paths
// instead of the `pub mod` declarations that were deleted here.
pub use reify_ir::{
    annotation, boundary_attachment, constraint, expr, geometry, kernel_validation, node_traits,
    persistent, provenance, sampled, structure_registry, traits, value, warm, warm_registry,
};

pub use annotation::{Annotation, AnnotationArg, AnnotationArgValue, has_test_annotation};
pub use ast::{
    DimOp, Expr, ExprKind, LambdaParam, MatchArm, QuantifierKind, TypeExpr, TypeExprKind,
};
pub use boundary_attachment::{BoundaryAssociation, NodeAttachment};
pub use constraint::{
    AutoParam, ConstraintChecker, ConstraintDiagnostics, ConstraintDomain, ConstraintInput,
    ConstraintResult, ConstraintSolver, OptimizationObjective, OptimizedImpl, OptimizedImplInput,
    OptimizedImplOutput, ResolutionProblem, SolveResult,
};
pub use diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticInfo, DiagnosticLabel, DiagnosticRef, Severity,
    SourceSpan,
};
pub use dimension::{DimensionVector, NAMED_DIMENSIONS, Rational};
pub use expr::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, CompiledMatchArm,
    DeterminacyPredicateKind, ResolvedFunction, SelectorKind, TAG_AD_HOC_SELECTOR,
    TAG_BIN_OP, TAG_CONDITIONAL, TAG_DETERMINACY_PREDICATE, TAG_FUNCTION_CALL, TAG_INDEX_ACCESS,
    TAG_LAMBDA, TAG_LIST_LITERAL, TAG_LITERAL, TAG_MAP_LITERAL, TAG_MATCH, TAG_META_ACCESS,
    TAG_METHOD_CALL, TAG_OPTION_NONE, TAG_OPTION_SOME, TAG_QUANTIFIER, TAG_RANGE_CONSTRUCTOR,
    TAG_REFLECTIVE_CELL_LIST, TAG_SET_LITERAL, TAG_UN_OP, TAG_USER_FUNCTION_CALL, TAG_VALUE_REF,
    UnOp,
};
pub use geometry::{
    AttributeHistory, AxisSign, BRepKind, BooleanOpHistoryRecords, BooleanOpParents,
    BooleanOpParentsError, CapKind, CapabilityDescriptor, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
    DeletedRecord, EdgeCurveKind, ElementOrderTag, ExportError, ExportFormat, FaceSurfaceKind,
    FeatureId, FeatureTag, FeatureTagTable, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, HistoryRecord, KernelAttributeHook,
    KernelAttributeOutcome, KernelRegistration, LoftOpHistoryRecords, Mesh, ModEntry, Operation,
    QueryCapability, QueryError, ReprKind, Role, StepKind, SweepOpHistoryRecords, TessError,
    TopologyAttribute, TopologyAttributeTable, VolumeMesh, debug_assert_query_many_invariant,
};
pub use hash::ContentHash;
pub use identity::*;
pub use kernel_validation::{
    BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
};
pub use node_traits::{HasNodeKind, NodeKind, NodeTraits, NodeTraitsMap};
pub use persistent::PersistentMap;
pub use primitives::{
    DEPRECATED_ANNOTATION, OPTIMIZED_ANNOTATION, PortDirection, SHELL_ANNOTATION, SOLID_ANNOTATION,
    SOLVER_HINT_ANNOTATION, TEST_ANNOTATION,
};
pub use provenance::{FieldImportProvenance, SnapshotProvenance};
pub use source_location::{
    SourceLocationInfo, build_line_offsets, byte_offset_to_line_col,
    line_col_to_byte_offset_with_offsets,
};
pub use spanned_ident::SpannedIdent;
pub use structure_registry::{StructureMeta, StructureRegistry, StructureTypeId};
pub use traits::{EnumDef, TraitBound, TraitDef, TraitMember, TraitRef, TypeParam};
pub use ty::Type;
pub use value::{
    DeterminacyState, ErrorRef, EvalError, FieldSourceKind, Freshness, InterpolationKind,
    ResultRef, SampledField, SampledGridKind, Satisfaction, StructureInstanceData, Value, ValueMap,
    quaternion_is_finite,
};
pub use warm::{OpaqueState, WarmStartable};
pub use warm_registry::{WarmStartableRegistration, WarmStartableRegistry};
