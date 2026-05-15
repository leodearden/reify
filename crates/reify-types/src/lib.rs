// `Value` carries a `SampledField` whose `oob_emitted: AtomicBool` introduces
// interior mutability that does NOT participate in `PartialEq`/`Ord`/`Hash`/
// `content_hash`. `BTreeMap<Value, _>` (notably `Value::Map`) therefore preserves
// its ordering invariants, but `clippy::mutable_key_type` still fires. See
// `value.rs::SampledField` for the full rationale.
#![allow(clippy::mutable_key_type)]

pub mod annotation;
pub mod constraint;
pub mod diagnostics;
pub mod dimension;
pub mod expr;
pub mod geometry;
pub mod hash;
pub mod identity;
pub mod kernel_validation;
pub mod node_traits;
pub mod persistent;
pub mod provenance;
pub mod sampled;
pub mod source_location;
pub mod spanned_ident;
pub mod traits;
pub mod ty;
pub mod value;
pub mod warm;

pub use annotation::{
    Annotation, AnnotationArg, DEPRECATED_ANNOTATION, OPTIMIZED_ANNOTATION, SHELL_ANNOTATION,
    SOLID_ANNOTATION, SOLVER_HINT_ANNOTATION, TEST_ANNOTATION, has_test_annotation,
};
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
    DeterminacyPredicateKind, QuantifierKind, ResolvedFunction, SelectorKind, TAG_AD_HOC_SELECTOR,
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
pub use provenance::{FieldImportProvenance, SnapshotProvenance};
pub use source_location::{SourceLocationInfo, byte_offset_to_line_col};
pub use spanned_ident::SpannedIdent;
pub use traits::{EnumDef, PortDirection, TraitBound, TraitDef, TraitMember, TraitRef, TypeParam};
pub use ty::Type;
pub use value::{
    DeterminacyState, ErrorRef, EvalError, FieldSourceKind, Freshness, InterpolationKind,
    ResultRef, SampledField, SampledGridKind, Satisfaction, Value, ValueMap, quaternion_is_finite,
};
pub use warm::{OpaqueState, WarmStartable};
