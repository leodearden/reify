//! Compile-time surface pin for `reify-ir`.
//!
//! Pins the full public API that `reify-ir` MUST export after the atomic
//! module move (step-2), in both the flat form (`reify_ir::Value`) and the
//! module-path form (`reify_ir::value::Value`).
//!
//! Both spellings remain in sync because `reify-ir/src/lib.rs` exports each
//! module as `pub mod` AND re-exports its symbols at the crate root.
//!
//! Compile-time guarantees that reify-ir's public API exposes the listed symbols
//! via both flat and module-path spellings.

// ── annotation (flat form) ───────────────────────────────────────────────────
use reify_ir::{Annotation, AnnotationArg, AnnotationArgValue, has_test_annotation};

// ── annotation (module-path form) ────────────────────────────────────────────
use reify_ir::annotation::{
    Annotation as AnnotationMod, AnnotationArg as AnnotationArgMod,
    AnnotationArgValue as AnnotationArgValueMod,
    has_test_annotation as has_test_annotation_mod,
};

// ── boundary_attachment (flat form) ──────────────────────────────────────────
use reify_ir::{BoundaryAssociation, NodeAttachment};

// ── boundary_attachment (module-path form) ────────────────────────────────────
use reify_ir::boundary_attachment::{
    BoundaryAssociation as BoundaryAssociationMod, NodeAttachment as NodeAttachmentMod,
};

// ── constraint (flat form) ───────────────────────────────────────────────────
use reify_ir::{
    AutoParam, ConstraintChecker, ConstraintDiagnostics, ConstraintDomain, ConstraintInput,
    ConstraintResult, ConstraintSolver, ObjectiveCombination, ObjectiveSense, ObjectiveSet,
    ObjectiveTerm, OptimizedImpl, OptimizedImplInput,
    OptimizedImplOutput, ResolutionProblem, SolveResult,
};

// ── constraint (module-path form) ────────────────────────────────────────────
use reify_ir::constraint::{
    AutoParam as AutoParamMod, ConstraintChecker as ConstraintCheckerMod,
    ConstraintDiagnostics as ConstraintDiagnosticsMod, ConstraintDomain as ConstraintDomainMod,
    ConstraintInput as ConstraintInputMod, ConstraintResult as ConstraintResultMod,
    ConstraintSolver as ConstraintSolverMod,
    ObjectiveCombination as ObjectiveCombinationMod, ObjectiveSense as ObjectiveSenseMod,
    ObjectiveSet as ObjectiveSetMod, ObjectiveTerm as ObjectiveTermMod,
    OptimizedImpl as OptimizedImplMod, OptimizedImplInput as OptimizedImplInputMod,
    OptimizedImplOutput as OptimizedImplOutputMod, ResolutionProblem as ResolutionProblemMod,
    SolveResult as SolveResultMod,
};

// ── expr (flat form) ─────────────────────────────────────────────────────────
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, CompiledMatchArm,
    DeterminacyPredicateKind, ResolvedFunction, SelectorKind,
    TAG_AD_HOC_SELECTOR, TAG_BIN_OP, TAG_CONDITIONAL, TAG_DETERMINACY_PREDICATE,
    TAG_FUNCTION_CALL, TAG_INDEX_ACCESS, TAG_LAMBDA, TAG_LIST_LITERAL, TAG_LITERAL,
    TAG_MAP_LITERAL, TAG_MATCH, TAG_META_ACCESS, TAG_METHOD_CALL, TAG_OPTION_NONE,
    TAG_OPTION_SOME, TAG_QUANTIFIER, TAG_RANGE_CONSTRUCTOR, TAG_REFLECTIVE_CELL_LIST,
    TAG_SET_LITERAL, TAG_UN_OP, TAG_USER_FUNCTION_CALL, TAG_VALUE_REF,
    UnOp,
};

// ── expr (module-path form) ──────────────────────────────────────────────────
use reify_ir::expr::{
    BinOp as BinOpMod, CompiledExpr as CompiledExprMod, CompiledExprKind as CompiledExprKindMod,
    CompiledFnBody as CompiledFnBodyMod, CompiledFunction as CompiledFunctionMod,
    CompiledMatchArm as CompiledMatchArmMod,
    DeterminacyPredicateKind as DeterminacyPredicateKindMod,
    ResolvedFunction as ResolvedFunctionMod, SelectorKind as SelectorKindMod,
    TAG_AD_HOC_SELECTOR as TAG_AD_HOC_SELECTOR_MOD, TAG_BIN_OP as TAG_BIN_OP_MOD,
    TAG_CONDITIONAL as TAG_CONDITIONAL_MOD,
    TAG_DETERMINACY_PREDICATE as TAG_DETERMINACY_PREDICATE_MOD,
    TAG_FUNCTION_CALL as TAG_FUNCTION_CALL_MOD, TAG_INDEX_ACCESS as TAG_INDEX_ACCESS_MOD,
    TAG_LAMBDA as TAG_LAMBDA_MOD, TAG_LIST_LITERAL as TAG_LIST_LITERAL_MOD,
    TAG_LITERAL as TAG_LITERAL_MOD, TAG_MAP_LITERAL as TAG_MAP_LITERAL_MOD,
    TAG_MATCH as TAG_MATCH_MOD, TAG_META_ACCESS as TAG_META_ACCESS_MOD,
    TAG_METHOD_CALL as TAG_METHOD_CALL_MOD, TAG_OPTION_NONE as TAG_OPTION_NONE_MOD,
    TAG_OPTION_SOME as TAG_OPTION_SOME_MOD, TAG_QUANTIFIER as TAG_QUANTIFIER_MOD,
    TAG_RANGE_CONSTRUCTOR as TAG_RANGE_CONSTRUCTOR_MOD,
    TAG_REFLECTIVE_CELL_LIST as TAG_REFLECTIVE_CELL_LIST_MOD,
    TAG_SET_LITERAL as TAG_SET_LITERAL_MOD, TAG_UN_OP as TAG_UN_OP_MOD,
    TAG_USER_FUNCTION_CALL as TAG_USER_FUNCTION_CALL_MOD, TAG_VALUE_REF as TAG_VALUE_REF_MOD,
    UnOp as UnOpMod,
};

// ── geometry (flat form) ─────────────────────────────────────────────────────
use reify_ir::{
    AttributeHistory, AxisSign, BRepKind, BooleanOpHistoryRecords, BooleanOpParents,
    BooleanOpParentsError, CapKind, CapabilityDescriptor, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
    DeletedRecord, EdgeCurveKind, ElementOrderTag, ExportError, ExportFormat, FaceSurfaceKind,
    FeatureId, FeatureTag, FeatureTagTable, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, HistoryRecord, KernelAttributeHook,
    KernelAttributeOutcome, KernelRegistration, LoftOpHistoryRecords, Mesh, ModEntry, Operation,
    QueryCapability, QueryError, ReprKind, Role, StepKind, SweepOpHistoryRecords, TessError,
    TopologyAttribute, TopologyAttributeTable, VolumeMesh, debug_assert_query_many_invariant,
};

// ── geometry (module-path form) ──────────────────────────────────────────────
use reify_ir::geometry::{
    AttributeHistory as AttributeHistoryMod, AxisSign as AxisSignMod, BRepKind as BRepKindMod,
    BooleanOpHistoryRecords as BooleanOpHistoryRecordsMod,
    BooleanOpParents as BooleanOpParentsMod,
    BooleanOpParentsError as BooleanOpParentsErrorMod, CapKind as CapKindMod,
    CapabilityDescriptor as CapabilityDescriptorMod,
    DEFAULT_POINT_ON_SHAPE_TOLERANCE_M as DEFAULT_POINT_ON_SHAPE_TOLERANCE_M_MOD,
    DeletedRecord as DeletedRecordMod, EdgeCurveKind as EdgeCurveKindMod,
    ElementOrderTag as ElementOrderTagMod, ExportError as ExportErrorMod,
    ExportFormat as ExportFormatMod, FaceSurfaceKind as FaceSurfaceKindMod,
    FeatureId as FeatureIdMod, FeatureTag as FeatureTagMod,
    FeatureTagTable as FeatureTagTableMod, GeometryError as GeometryErrorMod,
    GeometryHandle as GeometryHandleMod, GeometryHandleId as GeometryHandleIdMod,
    GeometryKernel as GeometryKernelMod, GeometryOp as GeometryOpMod,
    GeometryQuery as GeometryQueryMod, HistoryRecord as HistoryRecordMod,
    KernelAttributeHook as KernelAttributeHookMod,
    KernelAttributeOutcome as KernelAttributeOutcomeMod,
    KernelRegistration as KernelRegistrationMod,
    LoftOpHistoryRecords as LoftOpHistoryRecordsMod, Mesh as MeshMod, ModEntry as ModEntryMod,
    Operation as OperationMod, QueryCapability as QueryCapabilityMod,
    QueryError as QueryErrorMod, ReprKind as ReprKindMod, Role as RoleMod,
    StepKind as StepKindMod, SweepOpHistoryRecords as SweepOpHistoryRecordsMod,
    TessError as TessErrorMod, TopologyAttribute as TopologyAttributeMod,
    TopologyAttributeTable as TopologyAttributeTableMod, VolumeMesh as VolumeMeshMod,
    debug_assert_query_many_invariant as debug_assert_query_many_invariant_mod,
};

// ── kernel_validation (flat form) ────────────────────────────────────────────
use reify_ir::{
    BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
};

// ── kernel_validation (module-path form) ─────────────────────────────────────
use reify_ir::kernel_validation::{
    BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE as BOX_MSG_MOD,
    SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE as SPHERE_MSG_MOD,
};

// ── node_traits (flat form) ──────────────────────────────────────────────────
use reify_ir::{HasNodeKind, NodeKind, NodeTraits, NodeTraitsMap};

// ── node_traits (module-path form) ───────────────────────────────────────────
use reify_ir::node_traits::{
    HasNodeKind as HasNodeKindMod, NodeKind as NodeKindMod, NodeTraits as NodeTraitsMod,
    NodeTraitsMap as NodeTraitsMapMod,
};

// ── persistent (flat form) ───────────────────────────────────────────────────
use reify_ir::PersistentMap;

// ── persistent (module-path form) ────────────────────────────────────────────
use reify_ir::persistent::PersistentMap as PersistentMapMod;

// ── provenance (flat form) ───────────────────────────────────────────────────
use reify_ir::{FieldImportProvenance, SnapshotProvenance};

// ── provenance (module-path form) ────────────────────────────────────────────
use reify_ir::provenance::{
    FieldImportProvenance as FieldImportProvenanceMod,
    SnapshotProvenance as SnapshotProvenanceMod,
};

// ── sampled (flat form) ──────────────────────────────────────────────────────
// (sampled module: SampledField, SampledGridKind, InterpolationKind come
//  through value, but the module itself must be accessible too)
use reify_ir::sampled;

// ── structure_registry (flat form) ───────────────────────────────────────────
use reify_ir::{StructureMeta, StructureRegistry, StructureTypeId};

// ── structure_registry (module-path form) ────────────────────────────────────
use reify_ir::structure_registry::{
    StructureMeta as StructureMetaMod, StructureRegistry as StructureRegistryMod,
    StructureTypeId as StructureTypeIdMod,
};

// ── traits (flat form) ───────────────────────────────────────────────────────
use reify_ir::{EnumDef, TraitBound, TraitDef, TraitMember, TraitRef, TypeParam};

// ── traits (module-path form) ────────────────────────────────────────────────
use reify_ir::traits::{
    EnumDef as EnumDefMod, TraitBound as TraitBoundMod, TraitDef as TraitDefMod,
    TraitMember as TraitMemberMod, TraitRef as TraitRefMod, TypeParam as TypeParamMod,
};

// ── value (flat form) ────────────────────────────────────────────────────────
use reify_ir::{
    DeterminacyState, ErrorRef, EvalError, FieldSourceKind, Freshness, InterpolationKind,
    ResultRef, SampledField, SampledGridKind, Satisfaction, StructureInstanceData, Value, ValueMap,
    quaternion_is_finite,
};

// ── value (module-path form) ─────────────────────────────────────────────────
use reify_ir::value::{
    DeterminacyState as DeterminacyStateMod, ErrorRef as ErrorRefMod, EvalError as EvalErrorMod,
    FieldSourceKind as FieldSourceKindMod, Freshness as FreshnessMod,
    InterpolationKind as InterpolationKindMod, ResultRef as ResultRefMod,
    SampledField as SampledFieldMod, SampledGridKind as SampledGridKindMod,
    Satisfaction as SatisfactionMod, StructureInstanceData as StructureInstanceDataMod,
    Value as ValueMod, ValueMap as ValueMapMod, quaternion_is_finite as quaternion_is_finite_mod,
};

// ── warm (flat form) ─────────────────────────────────────────────────────────
use reify_ir::{OpaqueState, WarmStartable};

// ── warm (module-path form) ──────────────────────────────────────────────────
use reify_ir::warm::{OpaqueState as OpaqueStateMod, WarmStartable as WarmStartableMod};

// ── warm_registry (flat form) ────────────────────────────────────────────────
use reify_ir::{WarmStartableRegistration, WarmStartableRegistry};

// ── warm_registry (module-path form) ─────────────────────────────────────────
use reify_ir::warm_registry::{
    WarmStartableRegistration as WarmStartableRegistrationMod,
    WarmStartableRegistry as WarmStartableRegistryMod,
};

// ── cross-crate deps ─────────────────────────────────────────────────────────
use reify_ast::{Expr, ExprKind};
use reify_core::SourceSpan;

// =============================================================================
// Surface assertions
// =============================================================================

#[test]
fn annotation_flat_and_module_path_constructible() {
    let span = SourceSpan::new(0, 6);

    // Build an Annotation via flat path.
    let ann: Annotation = Annotation {
        name: "test".to_string(),
        args: vec![],
        span,
    };
    assert!(ann.is_test());

    // Build an Annotation via module-path.
    let _ann_mod: AnnotationMod = AnnotationMod {
        name: "other".to_string(),
        args: vec![],
        span,
    };

    // has_test_annotation: both spellings work.
    assert!(has_test_annotation(&[ann]));
    assert!(!has_test_annotation_mod(&[]));

    // AnnotationArg constructibility.
    let arg: AnnotationArg = AnnotationArg::positional(AnnotationArgValue::Bool(true));
    let _arg_mod: AnnotationArgMod = AnnotationArgMod::positional(AnnotationArgValueMod::Bool(false));
    assert_eq!(arg.name, None);
}

#[test]
fn annotation_arg_value_cross_assignment_proves_same_type() {
    let flat: AnnotationArgValue = AnnotationArgValue::String("hi".into());
    let _same: AnnotationArgValueMod = flat;
}

/// B7 smoke test (inline): assert AnnotationArgValue::Expr wraps a reify_ast::Expr.
/// The dedicated round-trip is in tests/embed_roundtrip.rs; this proves the embed
/// type-checks at the import boundary already in api_surface.rs.
#[test]
fn annotation_arg_value_expr_embed_compiles() {
    let span = SourceSpan::new(0, 3);
    let expr: Expr = Expr {
        kind: ExprKind::NumberLiteral { value: 42.0, is_real: false },
        span,
    };
    let arg_val = AnnotationArgValue::Expr(expr);
    match arg_val {
        AnnotationArgValue::Expr(inner) => {
            assert!(matches!(inner.kind, ExprKind::NumberLiteral { value, .. } if value == 42.0));
        }
        _ => panic!("expected Expr variant"),
    }
}

#[test]
fn boundary_attachment_types_in_scope() {
    let _: fn() -> Option<BoundaryAssociation> = || None;
    let _: fn() -> Option<NodeAttachment> = || None;
    let _: fn() -> Option<BoundaryAssociationMod> = || None;
    let _: fn() -> Option<NodeAttachmentMod> = || None;
}

#[test]
fn constraint_types_in_scope() {
    let _: fn() -> Option<ConstraintInput<'static>> = || None;
    let _: fn() -> Option<ConstraintResult> = || None;
    let _: fn() -> Option<ConstraintDiagnostics> = || None;
    let _: fn() -> Option<ConstraintDomain> = || None;
    let _: fn() -> Option<AutoParam> = || None;
    let _: fn() -> Option<SolveResult> = || None;
    let _: fn() -> Option<ResolutionProblem> = || None;
    let _: fn() -> Option<ObjectiveSense> = || None;
    let _: fn() -> Option<ObjectiveTerm> = || None;
    let _: fn() -> Option<ObjectiveCombination> = || None;
    let _: fn() -> Option<ObjectiveSet> = || None;
    let _: fn() -> Option<OptimizedImplInput<'static>> = || None;
    let _: fn() -> Option<OptimizedImplOutput> = || None;
    // Trait bounds only require the name to resolve.
    let _: fn() -> Option<Box<dyn ConstraintChecker>> = || None;
    let _: fn() -> Option<Box<dyn OptimizedImpl>> = || None;
    let _: fn() -> Option<Box<dyn ConstraintSolver>> = || None;
    // Module-path forms.
    let _: fn() -> Option<ConstraintInputMod<'static>> = || None;
    let _: fn() -> Option<ConstraintResultMod> = || None;
    let _: fn() -> Option<ConstraintDiagnosticsMod> = || None;
    let _: fn() -> Option<ConstraintDomainMod> = || None;
    let _: fn() -> Option<AutoParamMod> = || None;
    let _: fn() -> Option<SolveResultMod> = || None;
    let _: fn() -> Option<ResolutionProblemMod> = || None;
    let _: fn() -> Option<ObjectiveSenseMod> = || None;
    let _: fn() -> Option<ObjectiveTermMod> = || None;
    let _: fn() -> Option<ObjectiveCombinationMod> = || None;
    let _: fn() -> Option<ObjectiveSetMod> = || None;
    let _: fn() -> Option<OptimizedImplInputMod<'static>> = || None;
    let _: fn() -> Option<OptimizedImplOutputMod> = || None;
    let _: fn() -> Option<Box<dyn ConstraintCheckerMod>> = || None;
    let _: fn() -> Option<Box<dyn OptimizedImplMod>> = || None;
    let _: fn() -> Option<Box<dyn ConstraintSolverMod>> = || None;
}

#[test]
fn expr_types_in_scope() {
    let _: fn() -> Option<CompiledExpr> = || None;
    let _: fn() -> Option<CompiledExprKind> = || None;
    let _: fn() -> Option<CompiledFnBody> = || None;
    let _: fn() -> Option<CompiledFunction> = || None;
    let _: fn() -> Option<CompiledMatchArm> = || None;
    let _: fn() -> Option<DeterminacyPredicateKind> = || None;
    let _: fn() -> Option<ResolvedFunction> = || None;
    let _: fn() -> Option<SelectorKind> = || None;
    let _: fn() -> Option<BinOp> = || None;
    let _: fn() -> Option<UnOp> = || None;
    // Module-path forms.
    let _: fn() -> Option<CompiledExprMod> = || None;
    let _: fn() -> Option<CompiledExprKindMod> = || None;
    let _: fn() -> Option<CompiledFnBodyMod> = || None;
    let _: fn() -> Option<CompiledFunctionMod> = || None;
    let _: fn() -> Option<CompiledMatchArmMod> = || None;
    let _: fn() -> Option<DeterminacyPredicateKindMod> = || None;
    let _: fn() -> Option<ResolvedFunctionMod> = || None;
    let _: fn() -> Option<SelectorKindMod> = || None;
    let _: fn() -> Option<BinOpMod> = || None;
    let _: fn() -> Option<UnOpMod> = || None;
}

#[test]
fn expr_tag_constants_have_expected_values() {
    // Confirm TAG_* constants are in scope and round-trip.
    let _: u8 = TAG_LITERAL;
    let _: u8 = TAG_VALUE_REF;
    let _: u8 = TAG_BIN_OP;
    let _: u8 = TAG_UN_OP;
    let _: u8 = TAG_FUNCTION_CALL;
    let _: u8 = TAG_METHOD_CALL;
    let _: u8 = TAG_LAMBDA;
    let _: u8 = TAG_CONDITIONAL;
    let _: u8 = TAG_MATCH;
    let _: u8 = TAG_INDEX_ACCESS;
    let _: u8 = TAG_LIST_LITERAL;
    let _: u8 = TAG_MAP_LITERAL;
    let _: u8 = TAG_SET_LITERAL;
    let _: u8 = TAG_AD_HOC_SELECTOR;
    let _: u8 = TAG_QUANTIFIER;
    let _: u8 = TAG_RANGE_CONSTRUCTOR;
    let _: u8 = TAG_META_ACCESS;
    let _: u8 = TAG_OPTION_SOME;
    let _: u8 = TAG_OPTION_NONE;
    let _: u8 = TAG_DETERMINACY_PREDICATE;
    let _: u8 = TAG_USER_FUNCTION_CALL;
    let _: u8 = TAG_REFLECTIVE_CELL_LIST;
    // Module-path forms.
    assert_eq!(TAG_LITERAL, TAG_LITERAL_MOD);
    assert_eq!(TAG_VALUE_REF, TAG_VALUE_REF_MOD);
    assert_eq!(TAG_BIN_OP, TAG_BIN_OP_MOD);
    assert_eq!(TAG_UN_OP, TAG_UN_OP_MOD);
    assert_eq!(TAG_FUNCTION_CALL, TAG_FUNCTION_CALL_MOD);
    assert_eq!(TAG_METHOD_CALL, TAG_METHOD_CALL_MOD);
    assert_eq!(TAG_LAMBDA, TAG_LAMBDA_MOD);
    assert_eq!(TAG_CONDITIONAL, TAG_CONDITIONAL_MOD);
    assert_eq!(TAG_MATCH, TAG_MATCH_MOD);
    assert_eq!(TAG_INDEX_ACCESS, TAG_INDEX_ACCESS_MOD);
    assert_eq!(TAG_LIST_LITERAL, TAG_LIST_LITERAL_MOD);
    assert_eq!(TAG_MAP_LITERAL, TAG_MAP_LITERAL_MOD);
    assert_eq!(TAG_SET_LITERAL, TAG_SET_LITERAL_MOD);
    assert_eq!(TAG_AD_HOC_SELECTOR, TAG_AD_HOC_SELECTOR_MOD);
    assert_eq!(TAG_QUANTIFIER, TAG_QUANTIFIER_MOD);
    assert_eq!(TAG_RANGE_CONSTRUCTOR, TAG_RANGE_CONSTRUCTOR_MOD);
    assert_eq!(TAG_META_ACCESS, TAG_META_ACCESS_MOD);
    assert_eq!(TAG_OPTION_SOME, TAG_OPTION_SOME_MOD);
    assert_eq!(TAG_OPTION_NONE, TAG_OPTION_NONE_MOD);
    assert_eq!(TAG_DETERMINACY_PREDICATE, TAG_DETERMINACY_PREDICATE_MOD);
    assert_eq!(TAG_USER_FUNCTION_CALL, TAG_USER_FUNCTION_CALL_MOD);
    assert_eq!(TAG_REFLECTIVE_CELL_LIST, TAG_REFLECTIVE_CELL_LIST_MOD);
}

#[test]
fn geometry_types_in_scope() {
    let _: fn() -> Option<Mesh> = || None;
    let _: fn() -> Option<GeometryHandle> = || None;
    let _: fn() -> Option<GeometryHandleId> = || None;
    let _: fn() -> Option<Box<dyn GeometryKernel>> = || None;
    let _: fn() -> Option<Box<dyn KernelAttributeHook>> = || None;
    let _: fn() -> Option<KernelRegistration> = || None;
    let _: fn() -> Option<AttributeHistory> = || None;
    let _: fn() -> Option<AxisSign> = || None;
    let _: fn() -> Option<BRepKind> = || None;
    let _: fn() -> Option<BooleanOpHistoryRecords> = || None;
    let _: fn() -> Option<BooleanOpParents<'static>> = || None;
    let _: fn() -> Option<BooleanOpParentsError> = || None;
    let _: fn() -> Option<CapKind> = || None;
    let _: fn() -> Option<CapabilityDescriptor> = || None;
    let _: fn() -> Option<DeletedRecord> = || None;
    let _: fn() -> Option<EdgeCurveKind> = || None;
    let _: fn() -> Option<ElementOrderTag> = || None;
    let _: fn() -> Option<ExportError> = || None;
    let _: fn() -> Option<ExportFormat> = || None;
    let _: fn() -> Option<FaceSurfaceKind> = || None;
    let _: fn() -> Option<FeatureId> = || None;
    let _: fn() -> Option<FeatureTag> = || None;
    let _: fn() -> Option<FeatureTagTable> = || None;
    let _: fn() -> Option<GeometryError> = || None;
    let _: fn() -> Option<GeometryOp> = || None;
    let _: fn() -> Option<GeometryQuery> = || None;
    let _: fn() -> Option<HistoryRecord> = || None;
    let _: fn() -> Option<KernelAttributeOutcome> = || None;
    let _: fn() -> Option<LoftOpHistoryRecords> = || None;
    let _: fn() -> Option<ModEntry> = || None;
    let _: fn() -> Option<Operation> = || None;
    let _: fn() -> Option<QueryCapability> = || None;
    let _: fn() -> Option<QueryError> = || None;
    let _: fn() -> Option<ReprKind> = || None;
    let _: fn() -> Option<Role> = || None;
    let _: fn() -> Option<StepKind> = || None;
    let _: fn() -> Option<SweepOpHistoryRecords> = || None;
    let _: fn() -> Option<TessError> = || None;
    let _: fn() -> Option<TopologyAttribute> = || None;
    let _: fn() -> Option<TopologyAttributeTable> = || None;
    let _: fn() -> Option<VolumeMesh> = || None;
    // Module-path forms.
    let _: fn() -> Option<MeshMod> = || None;
    let _: fn() -> Option<GeometryHandleMod> = || None;
    let _: fn() -> Option<GeometryHandleIdMod> = || None;
    let _: fn() -> Option<Box<dyn GeometryKernelMod>> = || None;
    let _: fn() -> Option<Box<dyn KernelAttributeHookMod>> = || None;
    let _: fn() -> Option<KernelRegistrationMod> = || None;
    let _: fn() -> Option<AttributeHistoryMod> = || None;
    let _: fn() -> Option<AxisSignMod> = || None;
    let _: fn() -> Option<BRepKindMod> = || None;
    let _: fn() -> Option<BooleanOpHistoryRecordsMod> = || None;
    let _: fn() -> Option<BooleanOpParentsMod<'static>> = || None;
    let _: fn() -> Option<BooleanOpParentsErrorMod> = || None;
    let _: fn() -> Option<CapKindMod> = || None;
    let _: fn() -> Option<CapabilityDescriptorMod> = || None;
    let _: fn() -> Option<DeletedRecordMod> = || None;
    let _: fn() -> Option<EdgeCurveKindMod> = || None;
    let _: fn() -> Option<ElementOrderTagMod> = || None;
    let _: fn() -> Option<ExportErrorMod> = || None;
    let _: fn() -> Option<ExportFormatMod> = || None;
    let _: fn() -> Option<FaceSurfaceKindMod> = || None;
    let _: fn() -> Option<FeatureIdMod> = || None;
    let _: fn() -> Option<FeatureTagMod> = || None;
    let _: fn() -> Option<FeatureTagTableMod> = || None;
    let _: fn() -> Option<GeometryErrorMod> = || None;
    let _: fn() -> Option<GeometryOpMod> = || None;
    let _: fn() -> Option<GeometryQueryMod> = || None;
    let _: fn() -> Option<HistoryRecordMod> = || None;
    let _: fn() -> Option<KernelAttributeOutcomeMod> = || None;
    let _: fn() -> Option<LoftOpHistoryRecordsMod> = || None;
    let _: fn() -> Option<ModEntryMod> = || None;
    let _: fn() -> Option<OperationMod> = || None;
    let _: fn() -> Option<QueryCapabilityMod> = || None;
    let _: fn() -> Option<QueryErrorMod> = || None;
    let _: fn() -> Option<ReprKindMod> = || None;
    let _: fn() -> Option<RoleMod> = || None;
    let _: fn() -> Option<StepKindMod> = || None;
    let _: fn() -> Option<SweepOpHistoryRecordsMod> = || None;
    let _: fn() -> Option<TessErrorMod> = || None;
    let _: fn() -> Option<TopologyAttributeMod> = || None;
    let _: fn() -> Option<TopologyAttributeTableMod> = || None;
    let _: fn() -> Option<VolumeMeshMod> = || None;
    // DEFAULT_POINT_ON_SHAPE_TOLERANCE_M — same value in both spellings.
    assert_eq!(
        DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
        DEFAULT_POINT_ON_SHAPE_TOLERANCE_M_MOD
    );
    // fn item just needs to resolve as a symbol.
    let _ = debug_assert_query_many_invariant::<GeometryQuery, GeometryOp>;
    let _ = debug_assert_query_many_invariant_mod::<GeometryQueryMod, GeometryOpMod>;
}

#[test]
fn kernel_validation_constants() {
    assert!(!BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE.is_empty());
    assert!(!SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE.is_empty());
    assert_eq!(BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, BOX_MSG_MOD);
    assert_eq!(SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE, SPHERE_MSG_MOD);
}

#[test]
fn node_traits_types_in_scope() {
    let _: fn() -> Option<NodeKind> = || None;
    let _: fn() -> Option<NodeTraits> = || None;
    // HasNodeKind and NodeTraitsMap just need to resolve.
    let _: fn() -> Option<NodeKindMod> = || None;
    let _: fn() -> Option<NodeTraitsMod> = || None;
    // Parameterised — trait object is simplest.
    let _: fn() -> Option<Box<dyn HasNodeKind>> = || None;
    let _: fn() -> Option<Box<dyn HasNodeKindMod>> = || None;
    // NodeTraitsMap requires K: Eq + Hash + HasNodeKind.
    // NodeKind itself does not impl HasNodeKind — use a local witness.
    #[derive(Clone, Debug, Eq, Hash, PartialEq)]
    struct TestHnk;
    impl HasNodeKind for TestHnk {
        fn node_kind(&self) -> NodeKind { NodeKind::Value }
    }
    let _: fn() -> Option<NodeTraitsMap<TestHnk>> = || None;
    let _: fn() -> Option<NodeTraitsMapMod<TestHnk>> = || None;
}

#[test]
fn persistent_map_in_scope() {
    let _: fn() -> Option<PersistentMap<(), ()>> = || None;
    let _: fn() -> Option<PersistentMapMod<(), ()>> = || None;
}

#[test]
fn provenance_types_in_scope() {
    let _: fn() -> Option<FieldImportProvenance> = || None;
    let _: fn() -> Option<SnapshotProvenance> = || None;
    let _: fn() -> Option<FieldImportProvenanceMod> = || None;
    let _: fn() -> Option<SnapshotProvenanceMod> = || None;
}

#[test]
fn sampled_module_accessible() {
    // The sampled module must be re-exported so code can write
    // `reify_ir::sampled::…` or `reify_types::sampled::…`.
    // We just verify the module path is accessible (no public items to pin here
    // beyond those already covered in the value tests above).
    const { assert!(sampled::LINSPACE_MAX_INTERVALS > 0) };
}

#[test]
fn structure_registry_types_in_scope() {
    let _: fn() -> Option<StructureMeta> = || None;
    let _: fn() -> Option<StructureRegistry> = || None;
    let _: fn() -> Option<StructureTypeId> = || None;
    let _: fn() -> Option<StructureMetaMod> = || None;
    let _: fn() -> Option<StructureRegistryMod> = || None;
    let _: fn() -> Option<StructureTypeIdMod> = || None;
}

#[test]
fn traits_types_in_scope() {
    let _: fn() -> Option<EnumDef> = || None;
    let _: fn() -> Option<TraitBound> = || None;
    let _: fn() -> Option<TraitDef> = || None;
    let _: fn() -> Option<TraitMember> = || None;
    let _: fn() -> Option<TraitRef> = || None;
    let _: fn() -> Option<TypeParam> = || None;
    let _: fn() -> Option<EnumDefMod> = || None;
    let _: fn() -> Option<TraitBoundMod> = || None;
    let _: fn() -> Option<TraitDefMod> = || None;
    let _: fn() -> Option<TraitMemberMod> = || None;
    let _: fn() -> Option<TraitRefMod> = || None;
    let _: fn() -> Option<TypeParamMod> = || None;
}

#[test]
fn value_types_in_scope() {
    let _: fn() -> Option<Value> = || None;
    let _: fn() -> Option<ValueMap> = || None;
    let _: fn() -> Option<DeterminacyState> = || None;
    let _: fn() -> Option<ErrorRef> = || None;
    let _: fn() -> Option<EvalError> = || None;
    let _: fn() -> Option<FieldSourceKind> = || None;
    let _: fn() -> Option<Freshness> = || None;
    let _: fn() -> Option<InterpolationKind> = || None;
    let _: fn() -> Option<ResultRef> = || None;
    let _: fn() -> Option<SampledField> = || None;
    let _: fn() -> Option<SampledGridKind> = || None;
    let _: fn() -> Option<Satisfaction> = || None;
    let _: fn() -> Option<StructureInstanceData> = || None;
    // Module-path forms.
    let _: fn() -> Option<ValueMod> = || None;
    let _: fn() -> Option<ValueMapMod> = || None;
    let _: fn() -> Option<DeterminacyStateMod> = || None;
    let _: fn() -> Option<ErrorRefMod> = || None;
    let _: fn() -> Option<EvalErrorMod> = || None;
    let _: fn() -> Option<FieldSourceKindMod> = || None;
    let _: fn() -> Option<FreshnessMod> = || None;
    let _: fn() -> Option<InterpolationKindMod> = || None;
    let _: fn() -> Option<ResultRefMod> = || None;
    let _: fn() -> Option<SampledFieldMod> = || None;
    let _: fn() -> Option<SampledGridKindMod> = || None;
    let _: fn() -> Option<SatisfactionMod> = || None;
    let _: fn() -> Option<StructureInstanceDataMod> = || None;
    // function.
    assert!(quaternion_is_finite(1.0, 0.0, 0.0, 0.0));
    assert!(quaternion_is_finite_mod(1.0, 0.0, 0.0, 0.0));
}

#[test]
fn warm_types_in_scope() {
    let _: fn() -> Option<OpaqueState> = || None;
    let _: fn() -> Option<Box<dyn WarmStartable>> = || None;
    let _: fn() -> Option<OpaqueStateMod> = || None;
    let _: fn() -> Option<Box<dyn WarmStartableMod>> = || None;
}

#[test]
fn warm_registry_types_in_scope() {
    let _: fn() -> Option<WarmStartableRegistry> = || None;
    let _: fn() -> Option<WarmStartableRegistration> = || None;
    let _: fn() -> Option<WarmStartableRegistryMod> = || None;
    let _: fn() -> Option<WarmStartableRegistrationMod> = || None;
}
