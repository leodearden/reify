use crate::hash::ContentHash;
use crate::identity::ValueCellId;
use crate::ty::Type;
use crate::value::Value;

/// A compiled expression tree — fully resolved, ready for evaluation.
/// Shared by reify-eval and reify-constraints (via reify-expr).
#[derive(Debug, Clone)]
pub struct CompiledExpr {
    pub kind: CompiledExprKind,
    pub result_type: Type,
    pub content_hash: ContentHash,
}

/// The kind of ad-hoc geometry selector: `@face`, `@point`, `@edge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectorKind {
    /// `@face("name")` — select a named face; resolves to a frame via centroid + normal.
    Face,
    /// `@point(x, y, z)` — select a point by coordinates; resolves to a frame at that point.
    Point,
    /// `@edge("name")` — select a named edge; resolves to a frame via midpoint + tangent.
    Edge,
}

/// The kinds of compiled expression nodes.
#[derive(Debug, Clone)]
pub enum CompiledExprKind {
    /// Literal value.
    Literal(Value),
    /// Reference to a value cell.
    ValueRef(ValueCellId),
    /// Binary operation.
    BinOp {
        op: BinOp,
        left: Box<CompiledExpr>,
        right: Box<CompiledExpr>,
    },
    /// Unary operation.
    UnOp {
        op: UnOp,
        operand: Box<CompiledExpr>,
    },
    /// Call to a resolved function (stdlib or built-in).
    FunctionCall {
        function: ResolvedFunction,
        args: Vec<CompiledExpr>,
    },
    /// Conditional expression: if cond then a else b.
    Conditional {
        condition: Box<CompiledExpr>,
        then_branch: Box<CompiledExpr>,
        else_branch: Box<CompiledExpr>,
    },
    /// Match expression: match discriminant { pattern => body, ... }
    Match {
        discriminant: Box<CompiledExpr>,
        arms: Vec<CompiledMatchArm>,
    },
    /// Call to a user-defined function.
    UserFunctionCall {
        function_name: String,
        args: Vec<CompiledExpr>,
    },
    /// Lambda expression: |params| body with captured outer-scope references.
    Lambda {
        params: Vec<(String, Option<Type>)>,
        param_ids: Vec<ValueCellId>,
        body: Box<CompiledExpr>,
        captures: Vec<ValueCellId>,
    },
    /// List literal: [expr1, expr2, ...]
    ListLiteral(Vec<CompiledExpr>),
    /// Set literal: set{expr1, expr2, ...}
    SetLiteral(Vec<CompiledExpr>),
    /// Map literal: map{key1 => val1, key2 => val2, ...}
    MapLiteral(Vec<(CompiledExpr, CompiledExpr)>),
    /// Index access: object[index]
    IndexAccess {
        object: Box<CompiledExpr>,
        index: Box<CompiledExpr>,
    },
    /// Method call: object.method(args...)
    MethodCall {
        object: Box<CompiledExpr>,
        method: String,
        args: Vec<CompiledExpr>,
    },
    /// Quantifier expression: forall/exists variable in collection: predicate
    Quantifier {
        kind: QuantifierKind,
        variable: String,
        variable_id: ValueCellId,
        collection: Box<CompiledExpr>,
        predicate: Box<CompiledExpr>,
    },
    /// Option-some: wraps an inner expression in Value::Option(Some(...)).
    /// Does NOT propagate Undef — some(undef) == Value::Option(Some(Value::Undef)).
    OptionSome(Box<CompiledExpr>),
    /// Option-none: the intentional absence value Value::Option(None).
    OptionNone,
    /// Meta access: resolves a key from an entity's meta block at runtime.
    /// Result type is always Type::String.
    MetaAccess { entity: String, key: String },
    /// Determinacy predicate: checks the determinacy state of a value cell.
    /// Returns Bool at the engine level (eval layer returns Undef — lacks DeterminacyState access).
    DeterminacyPredicate {
        kind: DeterminacyPredicateKind,
        cell: ValueCellId,
    },
    /// Range constructor: builds a `Value::Range` from optional lower/upper bounds.
    /// Both bounds (when present) must have the same dimension (checked at compile time).
    RangeConstructor {
        lower: Option<Box<CompiledExpr>>,
        upper: Option<Box<CompiledExpr>>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    },
    /// Ad-hoc geometry selector: `base @ face("top")`, `base @ point(x,y,z)`, etc.
    /// Evaluates to a `Value::Frame` derived from the geometry query result.
    AdHocSelector {
        base: Box<CompiledExpr>,
        selector_kind: SelectorKind,
        args: Vec<CompiledExpr>,
    },
    /// Reflective-aggregation placeholder for `purpose_param.<query_kind>`
    /// member access (e.g. `subject.params`, `subject.geometric_params`).
    /// Emitted by the compiler in lieu of an empty `ListLiteral` so that
    /// `Engine::activate_purpose` can unambiguously identify and rewrite
    /// these nodes into populated `ListLiteral([ValueRef(...), ...])` against
    /// the bound entity's value cells (task-2289).
    PurposeReflectiveAggregation {
        /// The purpose-parameter name this query was on (e.g. `"subject"`).
        param_name: String,
        /// The schema-query kind (e.g. `"params"`, `"geometric_params"`).
        query_kind: String,
    },
    /// Activation-time post-expansion shape produced exclusively by
    /// `expand_purpose_reflective_placeholders`. Distinguished from `ListLiteral`
    /// so that `eval_quantifier`'s cell-iteration mode triggers only on
    /// placeholder-derived lists, not user-written all-`ValueRef` literals
    /// (task-2458).
    ///
    /// At runtime (outside the quantifier evaluator), behaves identically to
    /// `ListLiteral` — it is purely a structural marker.
    ReflectiveCellList(Vec<CompiledExpr>),
    /// Typed discriminator for a synthetic cross-sub geometry value reference,
    /// emitted exclusively by
    /// `expr.rs::try_resolve_cross_sub_geometry_value_ref` (task-3508).
    ///
    /// The bare-let drop site in `entity.rs` matches this variant structurally
    /// to recognise the synthetic shape unambiguously — replacing the fragile
    /// `ValueRef + entity.contains('.')` heuristic used before task-3508.
    ///
    /// This variant is consumed by entity.rs before reaching any downstream
    /// evaluation or constraint pass; it should never appear in eval, the GUI
    /// formatter, or purpose-placeholder expansion. All downstream exhaustive
    /// matches mirror the `ValueRef` leaf behaviour via OR-patterns, except
    /// `map_value_refs` which rebuilds via `cross_sub_geometry_ref` to preserve
    /// the variant on hash-rebuild.
    CrossSubGeometryRef(ValueCellId),
}

/// Determinacy predicate kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeterminacyPredicateKind {
    Determined,
    Undetermined,
    Constrained,
    PartiallyDetermined,
}

/// The kind of quantifier: universal (forall) or existential (exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuantifierKind {
    ForAll,
    Exists,
}

/// A compiled match arm.
#[derive(Debug, Clone)]
pub struct CompiledMatchArm {
    pub patterns: Vec<String>,
    pub body: CompiledExpr,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
    Not,
}

/// A fully resolved function reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedFunction {
    pub name: String,
    /// Unique identifier for dispatch (e.g., "std::math::sin").
    pub qualified_name: String,
}

/// A compiled user-defined function.
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<(String, Type)>,
    /// Compiled default-value expression for each param, by position; `None` when
    /// the param has no default. Populated by `compile_function` in
    /// `reify-compiler/src/functions.rs`; consumed at call sites for argument
    /// defaulting (task 3688).
    ///
    /// **Length invariant:** always exactly `params.len()`; entry `i` is
    /// `Some(expr)` iff param `i` has a default, otherwise `None`. Built
    /// canonically by `compile_function` in `reify-compiler/src/functions.rs`
    /// and by `CompiledFunction::new_with_no_defaults` for tests/stubs
    /// (task-3702).
    ///
    /// **Compilation scope:** Default expressions are compiled in a neutral scope
    /// containing only module-level names — they cannot reference sibling params
    /// (e.g. `fn f(a: Real, b: Real = a)` is rejected at compile time with an
    /// "unresolved name" diagnostic) and cannot recurse into the enclosing function.
    /// Rationale: keeps defaults pure-by-construction and order-independent. See
    /// `crates/reify-compiler/src/functions.rs` (`compile_function`) for the inline
    /// implementation comment and
    /// `docs/initial-design/name-resolution-and-scoping-design-decisions.md` §2.3
    /// for the full language-design rationale. Locked in by the regression test
    /// `fn_param_default_sibling_param_ref_errors` in
    /// `crates/reify-compiler/tests/fn_param_default_consumption_tests.rs`.
    pub param_defaults: Vec<Option<CompiledExpr>>,
    pub return_type: Type,
    pub body: CompiledFnBody,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<crate::annotation::Annotation>,
    /// Target string from `@optimized("kernel::foo")` annotation, if present.
    ///
    /// Populated by `compile_function` in `reify-compiler/src/functions.rs`
    /// using the `optimized_target` extractor (first-valid-wins semantics).
    /// `None` when no well-formed `@optimized` annotation is present.
    ///
    /// Parallel to `CompiledConstraint::optimized_target`; downstream dispatch
    /// (P3.4 ComputeNode) can probe the same field name regardless of whether
    /// the node originated from a constraint or function definition.
    pub optimized_target: Option<String>,
}

impl CompiledFunction {
    /// Returns `true` if this function is tagged with `@test`.
    pub fn is_test(&self) -> bool {
        crate::annotation::has_test_annotation(&self.annotations)
    }

    /// Construct a `CompiledFunction` where every param has no default.
    ///
    /// Sets `param_defaults` to `vec![None; params.len()]`, satisfying the
    /// strict length invariant (`param_defaults.len() == params.len()`) while
    /// expressing "no parameter has a default value."
    ///
    /// Use this constructor for test stubs and any producer that does not need
    /// to supply defaults. For functions that carry defaults, build via
    /// `compile_function` in `reify-compiler/src/functions.rs` instead.
    ///
    /// task-3702 (canonicalize CompiledFunction.param_defaults representation)
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_no_defaults(
        name: String,
        is_pub: bool,
        params: Vec<(String, Type)>,
        return_type: Type,
        body: CompiledFnBody,
        content_hash: crate::hash::ContentHash,
        annotations: Vec<crate::annotation::Annotation>,
        optimized_target: Option<String>,
    ) -> Self {
        let n = params.len();
        CompiledFunction {
            name,
            is_pub,
            params,
            param_defaults: vec![None; n],
            return_type,
            body,
            content_hash,
            annotations,
            optimized_target,
        }
    }
}

/// A compiled function body: let bindings followed by a result expression.
#[derive(Debug, Clone)]
pub struct CompiledFnBody {
    pub let_bindings: Vec<(String, CompiledExpr)>,
    pub result_expr: CompiledExpr,
}

/// Content-hash tag bytes for each `CompiledExprKind` variant.
///
/// Each constructor seeds its hash with its tag so structurally-different
/// expression kinds cannot collide on identical sub-hashes.
///
/// Bytes `[20]`–`[23]` are reserved by `CachedResult::content_hash` in
/// `reify-eval/src/cache.rs` (a distinct hash domain; sharing bytes would
/// confuse future readers). Next new `CompiledExpr` variant: use `[28]`.
pub const TAG_LITERAL: u8 = 0;
pub const TAG_VALUE_REF: u8 = 1;
pub const TAG_BIN_OP: u8 = 2;
pub const TAG_UN_OP: u8 = 3;
pub const TAG_FUNCTION_CALL: u8 = 4;
pub const TAG_CONDITIONAL: u8 = 5;
pub const TAG_USER_FUNCTION_CALL: u8 = 6;
pub const TAG_LAMBDA: u8 = 7;
pub const TAG_LIST_LITERAL: u8 = 8;
pub const TAG_SET_LITERAL: u8 = 9;
pub const TAG_MAP_LITERAL: u8 = 10;
pub const TAG_INDEX_ACCESS: u8 = 11;
pub const TAG_METHOD_CALL: u8 = 12;
pub const TAG_QUANTIFIER: u8 = 13;
pub const TAG_OPTION_SOME: u8 = 14;
pub const TAG_OPTION_NONE: u8 = 15;
pub const TAG_META_ACCESS: u8 = 16;
pub const TAG_DETERMINACY_PREDICATE: u8 = 17;
pub const TAG_RANGE_CONSTRUCTOR: u8 = 18;
pub const TAG_AD_HOC_SELECTOR: u8 = 19;
pub const TAG_MATCH: u8 = 24;
pub const TAG_PURPOSE_REFLECTIVE_AGGREGATION: u8 = 25;
pub const TAG_REFLECTIVE_CELL_LIST: u8 = 26;
pub const TAG_CROSS_SUB_GEOMETRY_REF: u8 = 27;

impl CompiledExpr {
    /// Create a literal expression.
    pub fn literal(value: Value, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[TAG_LITERAL]).combine(value.content_hash());
        CompiledExpr {
            kind: CompiledExprKind::Literal(value),
            result_type,
            content_hash,
        }
    }

    /// Create a value reference expression.
    pub fn value_ref(id: ValueCellId, result_type: Type) -> Self {
        CompiledExpr {
            content_hash: Self::hash_ref(TAG_VALUE_REF, &id),
            kind: CompiledExprKind::ValueRef(id),
            result_type,
        }
    }

    /// Create a cross-sub geometry reference expression (task-3508).
    ///
    /// Uses `TAG_CROSS_SUB_GEOMETRY_REF` (27) as the seed via `hash_ref`,
    /// producing a distinct `content_hash` from a structurally-identical
    /// `ValueRef`. The variant is emitted exclusively by
    /// `expr.rs::try_resolve_cross_sub_geometry_value_ref` and consumed by
    /// the bare-let drop site in `entity.rs` (replaced the fragile
    /// `entity.contains('.')` heuristic).
    pub fn cross_sub_geometry_ref(id: ValueCellId, result_type: Type) -> Self {
        // The consumer at entity.rs:1140 uses `split_once('.')` to extract the
        // sub-geometry name. This assert is the canonical chokepoint for the
        // `<parent>.<sub>` shape invariant — any future creator routing through
        // this constructor is protected automatically (task-3663).
        debug_assert!(
            id.entity.contains('.'),
            "CrossSubGeometryRef entity must be a `<parent>.<sub>` stamp (task-3508)"
        );
        CompiledExpr {
            content_hash: Self::hash_ref(TAG_CROSS_SUB_GEOMETRY_REF, &id),
            kind: CompiledExprKind::CrossSubGeometryRef(id),
            result_type,
        }
    }

    /// Shared hash formula for ref-shaped variants: TAG-byte seed combined with
    /// the `ValueCellId` Display string.  Centralises the formula so that both
    /// `value_ref` and `cross_sub_geometry_ref` stay in sync if the hashing
    /// convention for ref-shaped variants ever changes.
    fn hash_ref(tag: u8, id: &ValueCellId) -> ContentHash {
        ContentHash::of(&[tag]).combine(ContentHash::of_str(&format!("{}", id)))
    }

    /// Create a binary operation expression.
    pub fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[TAG_BIN_OP, op as u8])
            .combine(left.content_hash)
            .combine(right.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            result_type,
            content_hash,
        }
    }

    /// Recursively walk the expression tree, calling `f` on each node (pre-order).
    ///
    /// This is the canonical traversal for `CompiledExprKind`. All callers
    /// that need to visit expression nodes should use this method rather than
    /// implementing their own match on `CompiledExprKind`. This ensures that
    /// when new variants are added, only this single method needs updating.
    pub fn walk(&self, f: &mut impl FnMut(&CompiledExpr)) {
        f(self);
        match &self.kind {
            CompiledExprKind::Literal(_) => {}
            CompiledExprKind::ValueRef(_) | CompiledExprKind::CrossSubGeometryRef(_) => {}
            CompiledExprKind::BinOp { left, right, .. } => {
                left.walk(f);
                right.walk(f);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                operand.walk(f);
            }
            CompiledExprKind::FunctionCall { args, .. } => {
                for arg in args {
                    arg.walk(f);
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                condition.walk(f);
                then_branch.walk(f);
                else_branch.walk(f);
            }
            CompiledExprKind::Match { discriminant, arms } => {
                discriminant.walk(f);
                for arm in arms {
                    arm.body.walk(f);
                }
            }
            CompiledExprKind::UserFunctionCall { args, .. } => {
                for arg in args {
                    arg.walk(f);
                }
            }
            CompiledExprKind::Lambda { body, .. } => {
                body.walk(f);
            }
            CompiledExprKind::ListLiteral(elements) => {
                for elem in elements {
                    elem.walk(f);
                }
            }
            CompiledExprKind::ReflectiveCellList(elements) => {
                for elem in elements {
                    elem.walk(f);
                }
            }
            CompiledExprKind::SetLiteral(elements) => {
                for elem in elements {
                    elem.walk(f);
                }
            }
            CompiledExprKind::MapLiteral(entries) => {
                for (key, val) in entries {
                    key.walk(f);
                    val.walk(f);
                }
            }
            CompiledExprKind::IndexAccess { object, index } => {
                object.walk(f);
                index.walk(f);
            }
            CompiledExprKind::MethodCall { object, args, .. } => {
                object.walk(f);
                for arg in args {
                    arg.walk(f);
                }
            }
            CompiledExprKind::Quantifier {
                collection,
                predicate,
                ..
            } => {
                collection.walk(f);
                predicate.walk(f);
            }
            CompiledExprKind::OptionSome(inner) => {
                inner.walk(f);
            }
            CompiledExprKind::OptionNone => {}
            CompiledExprKind::MetaAccess { .. } => {}
            CompiledExprKind::DeterminacyPredicate { .. } => {}
            CompiledExprKind::RangeConstructor { lower, upper, .. } => {
                if let Some(lo) = lower {
                    lo.walk(f);
                }
                if let Some(hi) = upper {
                    hi.walk(f);
                }
            }
            CompiledExprKind::AdHocSelector { base, args, .. } => {
                base.walk(f);
                for arg in args {
                    arg.walk(f);
                }
            }
            // Placeholder is a leaf — no children to traverse.
            CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        }
    }

    /// Rewrite every `ValueRef` cell ID in this expression tree by applying
    /// `f`, returning a fresh `CompiledExpr` with recomputed `content_hash`
    /// values on every rebuilt node. (Task 2629 — runtime forall re-elaboration.)
    ///
    /// Unlike `remap_entity` and `remap_cell`, which mutate cell IDs in place
    /// and leave ancestor `content_hash` values stale, `map_value_refs` rebuilds
    /// each node via the existing `value_ref`/`binop`/etc. constructors so that
    /// downstream hash-based caching (e.g. the `EvaluationCache`) sees a
    /// structurally fresh expression. This is the contract the runtime
    /// per-element forall emission relies on.
    ///
    /// Variant coverage MUST match `walk()` arm-for-arm — adding a new
    /// `CompiledExprKind` variant should fail to compile here so the new
    /// variant is forced through this transform's hash-rebuilding path.
    ///
    /// On `Literal` and other leaf nodes (`OptionNone`, `MetaAccess`,
    /// `DeterminacyPredicate` w/o ValueRef cell rewrite, `PurposeReflectiveAggregation`),
    /// the transform is clone-only and reuses the original `content_hash`.
    pub fn map_value_refs(self, f: &mut impl FnMut(ValueCellId) -> ValueCellId) -> CompiledExpr {
        let result_type = self.result_type.clone();
        match self.kind {
            CompiledExprKind::Literal(_) | CompiledExprKind::OptionNone => {
                // Leaf: nothing to rewrite, preserve hash.
                CompiledExpr {
                    kind: self.kind,
                    result_type,
                    content_hash: self.content_hash,
                }
            }
            CompiledExprKind::ValueRef(id) => {
                let new_id = f(id);
                CompiledExpr::value_ref(new_id, result_type)
            }
            CompiledExprKind::CrossSubGeometryRef(id) => {
                // Preserve the variant on rebuild so downstream pattern-match
                // sites still treat the rebuilt expression as the synthetic shape.
                let new_id = f(id);
                CompiledExpr::cross_sub_geometry_ref(new_id, result_type)
            }
            CompiledExprKind::BinOp { op, left, right } => {
                let new_left = left.map_value_refs(f);
                let new_right = right.map_value_refs(f);
                CompiledExpr::binop(op, new_left, new_right, result_type)
            }
            CompiledExprKind::UnOp { op, operand } => {
                let new_operand = operand.map_value_refs(f);
                CompiledExpr::unop(op, new_operand, result_type)
            }
            CompiledExprKind::FunctionCall { function, args } => {
                let new_args: Vec<CompiledExpr> =
                    args.into_iter().map(|a| a.map_value_refs(f)).collect();
                // FunctionCall has no public constructor; rebuild manually with
                // a fresh content hash. Mirror compile_expr's combine order:
                // qualified_name + each arg hash.
                let mut content_hash = ContentHash::of(&[TAG_FUNCTION_CALL])
                    .combine(ContentHash::of_str(&function.qualified_name));
                for a in &new_args {
                    content_hash = content_hash.combine(a.content_hash);
                }
                CompiledExpr {
                    kind: CompiledExprKind::FunctionCall {
                        function,
                        args: new_args,
                    },
                    result_type,
                    content_hash,
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                let new_cond = condition.map_value_refs(f);
                let new_then = then_branch.map_value_refs(f);
                let new_else = else_branch.map_value_refs(f);
                let content_hash = ContentHash::of(&[TAG_CONDITIONAL])
                    .combine(new_cond.content_hash)
                    .combine(new_then.content_hash)
                    .combine(new_else.content_hash);
                CompiledExpr {
                    kind: CompiledExprKind::Conditional {
                        condition: Box::new(new_cond),
                        then_branch: Box::new(new_then),
                        else_branch: Box::new(new_else),
                    },
                    result_type,
                    content_hash,
                }
            }
            CompiledExprKind::Match { discriminant, arms } => {
                let new_disc = discriminant.map_value_refs(f);
                let new_arms: Vec<CompiledMatchArm> = arms
                    .into_iter()
                    .map(|arm| CompiledMatchArm {
                        patterns: arm.patterns,
                        body: arm.body.map_value_refs(f),
                    })
                    .collect();
                CompiledExpr::match_expr(new_disc, new_arms, result_type)
            }
            CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            } => {
                let new_args: Vec<CompiledExpr> =
                    args.into_iter().map(|a| a.map_value_refs(f)).collect();
                CompiledExpr::user_function_call(function_name, new_args, result_type)
            }
            CompiledExprKind::Lambda {
                params,
                param_ids,
                body,
                captures,
            } => {
                let new_param_ids: Vec<ValueCellId> = param_ids.into_iter().map(&mut *f).collect();
                let new_body = body.map_value_refs(f);
                let new_captures: Vec<ValueCellId> = captures.into_iter().map(&mut *f).collect();
                CompiledExpr::lambda(params, new_param_ids, new_body, new_captures, result_type)
            }
            CompiledExprKind::ListLiteral(elements) => {
                let new_elements: Vec<CompiledExpr> =
                    elements.into_iter().map(|e| e.map_value_refs(f)).collect();
                CompiledExpr::list_literal(new_elements, result_type)
            }
            CompiledExprKind::ReflectiveCellList(elements) => {
                let new_elements: Vec<CompiledExpr> =
                    elements.into_iter().map(|e| e.map_value_refs(f)).collect();
                CompiledExpr::reflective_cell_list(new_elements, result_type)
            }
            CompiledExprKind::SetLiteral(elements) => {
                let new_elements: Vec<CompiledExpr> =
                    elements.into_iter().map(|e| e.map_value_refs(f)).collect();
                CompiledExpr::set_literal(new_elements, result_type)
            }
            CompiledExprKind::MapLiteral(entries) => {
                let new_entries: Vec<(CompiledExpr, CompiledExpr)> = entries
                    .into_iter()
                    .map(|(k, v)| (k.map_value_refs(f), v.map_value_refs(f)))
                    .collect();
                CompiledExpr::map_literal(new_entries, result_type)
            }
            CompiledExprKind::IndexAccess { object, index } => {
                let new_obj = object.map_value_refs(f);
                let new_idx = index.map_value_refs(f);
                CompiledExpr::index_access(new_obj, new_idx, result_type)
            }
            CompiledExprKind::MethodCall {
                object,
                method,
                args,
            } => {
                let new_obj = object.map_value_refs(f);
                let new_args: Vec<CompiledExpr> =
                    args.into_iter().map(|a| a.map_value_refs(f)).collect();
                CompiledExpr::method_call(new_obj, method, new_args, result_type)
            }
            CompiledExprKind::Quantifier {
                kind,
                variable,
                variable_id,
                collection,
                predicate,
            } => {
                let new_var_id = f(variable_id);
                let new_coll = collection.map_value_refs(f);
                let new_pred = predicate.map_value_refs(f);
                CompiledExpr::quantifier(kind, variable, new_var_id, new_coll, new_pred)
            }
            CompiledExprKind::OptionSome(inner) => {
                let new_inner = inner.map_value_refs(f);
                CompiledExpr::option_some(new_inner, result_type)
            }
            CompiledExprKind::MetaAccess { entity, key } => {
                // Leaf — entity/key strings are unchanged by ValueRef rewrites.
                CompiledExpr::meta_access(entity, key)
            }
            CompiledExprKind::DeterminacyPredicate { kind, cell } => {
                let new_cell = f(cell);
                CompiledExpr::determinacy_predicate(kind, new_cell)
            }
            CompiledExprKind::RangeConstructor {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                let new_lower = lower.map(|b| (*b).map_value_refs(f));
                let new_upper = upper.map(|b| (*b).map_value_refs(f));
                CompiledExpr::range_constructor(
                    new_lower,
                    new_upper,
                    lower_inclusive,
                    upper_inclusive,
                    result_type,
                )
            }
            CompiledExprKind::AdHocSelector {
                base,
                selector_kind,
                args,
            } => {
                let new_base = base.map_value_refs(f);
                let new_args: Vec<CompiledExpr> =
                    args.into_iter().map(|a| a.map_value_refs(f)).collect();
                CompiledExpr::ad_hoc_selector(new_base, selector_kind, new_args)
            }
            CompiledExprKind::PurposeReflectiveAggregation {
                param_name,
                query_kind,
            } => {
                // Leaf — placeholder carries no cells until activation expands it.
                CompiledExpr::purpose_reflective_aggregation(param_name, query_kind, result_type)
            }
        }
    }

    /// Create a unary operation expression.
    pub fn unop(op: UnOp, operand: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[TAG_UN_OP, op as u8]).combine(operand.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::UnOp {
                op,
                operand: Box::new(operand),
            },
            result_type,
            content_hash,
        }
    }

    /// Collect all ValueRef ValueCellIds from this expression tree.
    ///
    /// For Lambda nodes, emits `captures` only — does NOT recurse into body.
    /// This is the correct behavior for dependency tracking: a lambda's
    /// dependencies are its captures, not the refs inside its body.
    pub fn collect_value_refs(&self) -> Vec<ValueCellId> {
        let mut refs = Vec::new();
        self.collect_value_refs_inner(&mut refs);
        refs
    }

    fn collect_value_refs_inner(&self, refs: &mut Vec<ValueCellId>) {
        match &self.kind {
            CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
                refs.push(id.clone())
            }
            CompiledExprKind::Literal(_) => {}
            CompiledExprKind::BinOp { left, right, .. } => {
                left.collect_value_refs_inner(refs);
                right.collect_value_refs_inner(refs);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                operand.collect_value_refs_inner(refs);
            }
            CompiledExprKind::FunctionCall { args, .. } => {
                for arg in args {
                    arg.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                condition.collect_value_refs_inner(refs);
                then_branch.collect_value_refs_inner(refs);
                else_branch.collect_value_refs_inner(refs);
            }
            CompiledExprKind::Match { discriminant, arms } => {
                discriminant.collect_value_refs_inner(refs);
                for arm in arms {
                    arm.body.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::UserFunctionCall { args, .. } => {
                for arg in args {
                    arg.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::Lambda { captures, .. } => {
                for cap in captures {
                    refs.push(cap.clone());
                }
            }
            CompiledExprKind::ListLiteral(elements) => {
                for elem in elements {
                    elem.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::ReflectiveCellList(elements) => {
                for elem in elements {
                    elem.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::SetLiteral(elements) => {
                for elem in elements {
                    elem.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::MapLiteral(entries) => {
                for (key, val) in entries {
                    key.collect_value_refs_inner(refs);
                    val.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::IndexAccess { object, index } => {
                object.collect_value_refs_inner(refs);
                index.collect_value_refs_inner(refs);
            }
            CompiledExprKind::MethodCall { object, args, .. } => {
                object.collect_value_refs_inner(refs);
                for arg in args {
                    arg.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::Quantifier {
                variable_id,
                collection,
                predicate,
                ..
            } => {
                // Collection refs are always dependencies
                collection.collect_value_refs_inner(refs);
                // Predicate refs excluding the bound variable
                let mut pred_refs = Vec::new();
                predicate.collect_value_refs_inner(&mut pred_refs);
                for r in pred_refs {
                    if r != *variable_id {
                        refs.push(r);
                    }
                }
            }
            CompiledExprKind::OptionSome(inner) => {
                inner.collect_value_refs_inner(refs);
            }
            CompiledExprKind::OptionNone => {}
            CompiledExprKind::MetaAccess { .. } => {}
            CompiledExprKind::DeterminacyPredicate { cell, .. } => {
                refs.push(cell.clone());
            }
            CompiledExprKind::RangeConstructor { lower, upper, .. } => {
                if let Some(lo) = lower {
                    lo.collect_value_refs_inner(refs);
                }
                if let Some(hi) = upper {
                    hi.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::AdHocSelector { base, args, .. } => {
                base.collect_value_refs_inner(refs);
                for arg in args {
                    arg.collect_value_refs_inner(refs);
                }
            }
            // Placeholder carries no concrete cell IDs — activation will
            // expand it before any dependency-tracking pass runs.
            CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        }
    }

    /// Create a lambda expression.
    pub fn lambda(
        params: Vec<(String, Option<Type>)>,
        param_ids: Vec<ValueCellId>,
        body: CompiledExpr,
        captures: Vec<ValueCellId>,
        result_type: Type,
    ) -> Self {
        let mut content_hash = ContentHash::of(&[TAG_LAMBDA]).combine(body.content_hash);
        for (name, ty) in &params {
            content_hash = content_hash.combine(ContentHash::of_str(name));
            if let Some(t) = ty {
                content_hash = content_hash.combine(ContentHash::of_str(&format!("{:?}", t)));
            }
        }
        for id in &param_ids {
            content_hash = content_hash.combine(ContentHash::of_str(&format!("{}", id)));
        }
        for cap in &captures {
            content_hash = content_hash.combine(ContentHash::of_str(&format!("{}", cap)));
        }
        CompiledExpr {
            kind: CompiledExprKind::Lambda {
                params,
                param_ids,
                body: Box::new(body),
                captures,
            },
            result_type,
            content_hash,
        }
    }

    /// Create a list literal expression.
    pub fn list_literal(elements: Vec<CompiledExpr>, result_type: Type) -> Self {
        let mut content_hash = ContentHash::of(&[TAG_LIST_LITERAL]);
        for elem in &elements {
            content_hash = content_hash.combine(elem.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::ListLiteral(elements),
            result_type,
            content_hash,
        }
    }

    /// Create a reflective-cell-list expression (task-2458).
    ///
    /// Post-expansion shape produced exclusively by
    /// `expand_purpose_reflective_placeholders`; distinguished from `ListLiteral`
    /// by tag byte `TAG_REFLECTIVE_CELL_LIST` so `eval_quantifier`'s
    /// cell-iteration trigger fires only on placeholder-derived lists.
    pub fn reflective_cell_list(elements: Vec<CompiledExpr>, result_type: Type) -> Self {
        // Invariant (task-2552, follow-up to task-2544): every element must be a
        // `ValueRef`. The split no-op arm for `ReflectiveCellList` in
        // `expand_purpose_reflective_placeholders` (reify-eval) elides recursion on
        // the basis of this invariant — a non-ValueRef element here would silently
        // skip placeholder expansion in release builds.
        debug_assert!(
            elements
                .iter()
                .all(|e| matches!(e.kind, CompiledExprKind::ValueRef(_))),
            "ReflectiveCellList elements must be ValueRefs by construction \
             (task-2544 / task-2552; see comment above)"
        );
        let mut content_hash = ContentHash::of(&[TAG_REFLECTIVE_CELL_LIST]);
        for elem in &elements {
            content_hash = content_hash.combine(elem.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::ReflectiveCellList(elements),
            result_type,
            content_hash,
        }
    }

    /// Create a set literal expression.
    pub fn set_literal(elements: Vec<CompiledExpr>, result_type: Type) -> Self {
        let mut content_hash = ContentHash::of(&[TAG_SET_LITERAL]);
        for elem in &elements {
            content_hash = content_hash.combine(elem.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::SetLiteral(elements),
            result_type,
            content_hash,
        }
    }

    /// Create a map literal expression.
    pub fn map_literal(entries: Vec<(CompiledExpr, CompiledExpr)>, result_type: Type) -> Self {
        let mut content_hash = ContentHash::of(&[TAG_MAP_LITERAL]);
        for (key, val) in &entries {
            content_hash = content_hash
                .combine(key.content_hash)
                .combine(val.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::MapLiteral(entries),
            result_type,
            content_hash,
        }
    }

    /// Create an index access expression.
    pub fn index_access(object: CompiledExpr, index: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[TAG_INDEX_ACCESS])
            .combine(object.content_hash)
            .combine(index.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::IndexAccess {
                object: Box::new(object),
                index: Box::new(index),
            },
            result_type,
            content_hash,
        }
    }

    /// Create a quantifier expression.
    pub fn quantifier(
        kind: QuantifierKind,
        variable: String,
        variable_id: ValueCellId,
        collection: CompiledExpr,
        predicate: CompiledExpr,
    ) -> Self {
        let kind_byte = match kind {
            QuantifierKind::ForAll => 0,
            QuantifierKind::Exists => 1,
        };
        let content_hash = ContentHash::of(&[TAG_QUANTIFIER, kind_byte])
            .combine(ContentHash::of_str(&variable))
            .combine(ContentHash::of_str(&format!("{}", variable_id)))
            .combine(collection.content_hash)
            .combine(predicate.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::Quantifier {
                kind,
                variable,
                variable_id,
                collection: Box::new(collection),
                predicate: Box::new(predicate),
            },
            result_type: Type::Bool,
            content_hash,
        }
    }

    /// Create an `option_some` expression wrapping an inner expression.
    /// Note: result_type should be Type::Option(Box::new(inner.result_type.clone())).
    pub fn option_some(inner: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[TAG_OPTION_SOME]).combine(inner.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::OptionSome(Box::new(inner)),
            result_type,
            content_hash,
        }
    }

    /// Create an `option_none` expression.
    /// Note: result_type should be Type::Option(Box::new(inner_type)).
    pub fn option_none(result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[TAG_OPTION_NONE]);
        CompiledExpr {
            kind: CompiledExprKind::OptionNone,
            result_type,
            content_hash,
        }
    }

    /// Rewrite all `ValueRef` cell IDs whose entity matches `from_entity`,
    /// replacing the entity part with `to_entity`. This is used during purpose
    /// activation to remap compiled references from the purpose's parameter
    /// namespace to the concrete entity being bound.
    ///
    /// CONTRACT — content-hash staleness: this rewrite mutates leaf cell IDs
    /// in place but **does not** rebuild `content_hash` on ancestor nodes.
    /// Callers that need a structurally-consistent hash on the rewritten
    /// tree must reseed it themselves (e.g. `engine_purposes::activate_purpose`
    /// reseeds each injected constraint's `content_hash` from
    /// `purpose:<name>:constraint:<i>`). Same caveat applies to `remap_cell`
    /// and to `engine_purposes::expand_purpose_reflective_placeholders`.
    pub fn remap_entity(&mut self, from_entity: &str, to_entity: &str) {
        match &mut self.kind {
            CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
                if id.entity == from_entity {
                    id.entity = to_entity.to_string();
                }
            }
            CompiledExprKind::Literal(_) => {}
            CompiledExprKind::BinOp { left, right, .. } => {
                left.remap_entity(from_entity, to_entity);
                right.remap_entity(from_entity, to_entity);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                operand.remap_entity(from_entity, to_entity);
            }
            CompiledExprKind::FunctionCall { args, .. } => {
                for arg in args {
                    arg.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                condition.remap_entity(from_entity, to_entity);
                then_branch.remap_entity(from_entity, to_entity);
                else_branch.remap_entity(from_entity, to_entity);
            }
            CompiledExprKind::Match { discriminant, arms } => {
                discriminant.remap_entity(from_entity, to_entity);
                for arm in arms {
                    arm.body.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::UserFunctionCall { args, .. } => {
                for arg in args {
                    arg.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::Lambda {
                body,
                captures,
                param_ids,
                ..
            } => {
                body.remap_entity(from_entity, to_entity);
                for cap in captures {
                    if cap.entity == from_entity {
                        cap.entity = to_entity.to_string();
                    }
                }
                for pid in param_ids {
                    if pid.entity == from_entity {
                        pid.entity = to_entity.to_string();
                    }
                }
            }
            CompiledExprKind::ListLiteral(elements) => {
                for elem in elements {
                    elem.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::ReflectiveCellList(elements) => {
                for elem in elements {
                    elem.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::SetLiteral(elements) => {
                for elem in elements {
                    elem.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::MapLiteral(entries) => {
                for (key, val) in entries {
                    key.remap_entity(from_entity, to_entity);
                    val.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::IndexAccess { object, index } => {
                object.remap_entity(from_entity, to_entity);
                index.remap_entity(from_entity, to_entity);
            }
            CompiledExprKind::MethodCall { object, args, .. } => {
                object.remap_entity(from_entity, to_entity);
                for arg in args {
                    arg.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::Quantifier {
                variable_id,
                collection,
                predicate,
                ..
            } => {
                if variable_id.entity == from_entity {
                    variable_id.entity = to_entity.to_string();
                }
                collection.remap_entity(from_entity, to_entity);
                predicate.remap_entity(from_entity, to_entity);
            }
            CompiledExprKind::OptionSome(inner) => {
                inner.remap_entity(from_entity, to_entity);
            }
            CompiledExprKind::OptionNone => {}
            CompiledExprKind::MetaAccess { entity, .. } => {
                if entity == from_entity {
                    *entity = to_entity.to_string();
                }
            }
            CompiledExprKind::DeterminacyPredicate { cell, .. } => {
                if cell.entity == from_entity {
                    cell.entity = to_entity.to_string();
                }
            }
            CompiledExprKind::RangeConstructor { lower, upper, .. } => {
                if let Some(lo) = lower {
                    lo.remap_entity(from_entity, to_entity);
                }
                if let Some(hi) = upper {
                    hi.remap_entity(from_entity, to_entity);
                }
            }
            CompiledExprKind::AdHocSelector { base, args, .. } => {
                base.remap_entity(from_entity, to_entity);
                for arg in args {
                    arg.remap_entity(from_entity, to_entity);
                }
            }
            // Placeholder carries no entity-bearing cell IDs; the activation
            // walk in `engine_purposes::activate_purpose` resolves it against
            // the bound entity directly. No-op here.
            CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        }
    }

    /// Rewrite every `ValueCellId` equal to `from` to `to`, traversing all
    /// variants that carry a cell. This is used by activation-time expansion
    /// of reflective-aggregation placeholders to carry a populated element's
    /// cell ID into a quantifier predicate (e.g. `DeterminacyPredicate { cell }`)
    /// after binding the synthetic loop var to the iterated cell (task-2289).
    ///
    /// Mirrors the structure of `remap_entity` arm-for-arm so future variant
    /// additions touch one place.
    ///
    /// CONTRACT — content-hash staleness: same as `remap_entity`. This walk
    /// rewrites cell IDs in place but does not rebuild ancestor `content_hash`
    /// values. Callers that consume the rewritten tree's hash must reseed it.
    pub fn remap_cell(&mut self, from: &ValueCellId, to: &ValueCellId) {
        match &mut self.kind {
            CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
                if id == from {
                    *id = to.clone();
                }
            }
            CompiledExprKind::Literal(_) => {}
            CompiledExprKind::BinOp { left, right, .. } => {
                left.remap_cell(from, to);
                right.remap_cell(from, to);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                operand.remap_cell(from, to);
            }
            CompiledExprKind::FunctionCall { args, .. } => {
                for arg in args {
                    arg.remap_cell(from, to);
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                condition.remap_cell(from, to);
                then_branch.remap_cell(from, to);
                else_branch.remap_cell(from, to);
            }
            CompiledExprKind::Match { discriminant, arms } => {
                discriminant.remap_cell(from, to);
                for arm in arms {
                    arm.body.remap_cell(from, to);
                }
            }
            CompiledExprKind::UserFunctionCall { args, .. } => {
                for arg in args {
                    arg.remap_cell(from, to);
                }
            }
            CompiledExprKind::Lambda {
                body,
                captures,
                param_ids,
                ..
            } => {
                body.remap_cell(from, to);
                for cap in captures {
                    if cap == from {
                        *cap = to.clone();
                    }
                }
                for pid in param_ids {
                    if pid == from {
                        *pid = to.clone();
                    }
                }
            }
            CompiledExprKind::ListLiteral(elements) => {
                for elem in elements {
                    elem.remap_cell(from, to);
                }
            }
            CompiledExprKind::ReflectiveCellList(elements) => {
                for elem in elements {
                    elem.remap_cell(from, to);
                }
            }
            CompiledExprKind::SetLiteral(elements) => {
                for elem in elements {
                    elem.remap_cell(from, to);
                }
            }
            CompiledExprKind::MapLiteral(entries) => {
                for (key, val) in entries {
                    key.remap_cell(from, to);
                    val.remap_cell(from, to);
                }
            }
            CompiledExprKind::IndexAccess { object, index } => {
                object.remap_cell(from, to);
                index.remap_cell(from, to);
            }
            CompiledExprKind::MethodCall { object, args, .. } => {
                object.remap_cell(from, to);
                for arg in args {
                    arg.remap_cell(from, to);
                }
            }
            CompiledExprKind::Quantifier {
                variable_id,
                collection,
                predicate,
                ..
            } => {
                if variable_id == from {
                    *variable_id = to.clone();
                }
                collection.remap_cell(from, to);
                predicate.remap_cell(from, to);
            }
            CompiledExprKind::OptionSome(inner) => {
                inner.remap_cell(from, to);
            }
            CompiledExprKind::OptionNone => {}
            CompiledExprKind::MetaAccess { .. } => {}
            CompiledExprKind::DeterminacyPredicate { cell, .. } => {
                if cell == from {
                    *cell = to.clone();
                }
            }
            CompiledExprKind::RangeConstructor { lower, upper, .. } => {
                if let Some(lo) = lower {
                    lo.remap_cell(from, to);
                }
                if let Some(hi) = upper {
                    hi.remap_cell(from, to);
                }
            }
            CompiledExprKind::AdHocSelector { base, args, .. } => {
                base.remap_cell(from, to);
                for arg in args {
                    arg.remap_cell(from, to);
                }
            }
            // Placeholder has no cell to rewrite — activation expands it.
            CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        }
    }

    /// Create a method call expression.
    pub fn method_call(
        object: CompiledExpr,
        method: String,
        args: Vec<CompiledExpr>,
        result_type: Type,
    ) -> Self {
        let mut content_hash = ContentHash::of(&[TAG_METHOD_CALL])
            .combine(object.content_hash)
            .combine(ContentHash::of_str(&method));
        for arg in &args {
            content_hash = content_hash.combine(arg.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::MethodCall {
                object: Box::new(object),
                method,
                args,
            },
            result_type,
            content_hash,
        }
    }

    /// Create a meta access expression (resolves a key from an entity's meta block).
    pub fn meta_access(entity: String, key: String) -> Self {
        let content_hash = ContentHash::of(&[TAG_META_ACCESS])
            .combine(ContentHash::of_str(&entity))
            .combine(ContentHash::of_str(&key));
        CompiledExpr {
            kind: CompiledExprKind::MetaAccess { entity, key },
            result_type: Type::String,
            content_hash,
        }
    }

    /// Create a range constructor expression.
    pub fn range_constructor(
        lower: Option<CompiledExpr>,
        upper: Option<CompiledExpr>,
        lower_inclusive: bool,
        upper_inclusive: bool,
        result_type: Type,
    ) -> Self {
        let mut content_hash = ContentHash::of(&[
            TAG_RANGE_CONSTRUCTOR,
            lower_inclusive as u8,
            upper_inclusive as u8,
        ]);
        if let Some(lo) = &lower {
            content_hash = content_hash.combine(lo.content_hash);
        }
        if let Some(hi) = &upper {
            content_hash = content_hash.combine(hi.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::RangeConstructor {
                lower: lower.map(Box::new),
                upper: upper.map(Box::new),
                lower_inclusive,
                upper_inclusive,
            },
            result_type,
            content_hash,
        }
    }

    /// Create an ad-hoc selector expression. Result type is always `Type::Frame(3)`.
    pub fn ad_hoc_selector(
        base: CompiledExpr,
        selector_kind: SelectorKind,
        args: Vec<CompiledExpr>,
    ) -> Self {
        let kind_byte: u8 = match selector_kind {
            SelectorKind::Face => 0,
            SelectorKind::Point => 1,
            SelectorKind::Edge => 2,
        };
        let mut content_hash =
            ContentHash::of(&[TAG_AD_HOC_SELECTOR, kind_byte]).combine(base.content_hash);
        for arg in &args {
            content_hash = content_hash.combine(arg.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::AdHocSelector {
                base: Box::new(base),
                selector_kind,
                args,
            },
            result_type: Type::Frame(3),
            content_hash,
        }
    }

    /// Create a determinacy predicate expression.
    ///
    /// Hash uses stable byte discriminators (not Debug repr) following the
    /// QuantifierKind pattern: `[17, kind_byte]` where kind_byte is
    /// Determined=0, Undetermined=1, Constrained=2, PartiallyDetermined=3.
    pub fn determinacy_predicate(kind: DeterminacyPredicateKind, cell: ValueCellId) -> Self {
        let kind_byte: u8 = match kind {
            DeterminacyPredicateKind::Determined => 0,
            DeterminacyPredicateKind::Undetermined => 1,
            DeterminacyPredicateKind::Constrained => 2,
            DeterminacyPredicateKind::PartiallyDetermined => 3,
        };
        let content_hash = ContentHash::of(&[TAG_DETERMINACY_PREDICATE, kind_byte])
            .combine(ContentHash::of_str(&format!("{}", cell)));
        CompiledExpr {
            kind: CompiledExprKind::DeterminacyPredicate { kind, cell },
            result_type: Type::Bool,
            content_hash,
        }
    }

    /// Create a user-defined function call expression.
    ///
    /// Hash tag byte: `TAG_USER_FUNCTION_CALL` (= `[6]`), matching the inline
    /// implementation in `reify-compiler/src/expr.rs`. Combines the function
    /// name then each argument's content hash in order, following the same
    /// pattern as `method_call`.
    pub fn user_function_call(
        function_name: String,
        args: Vec<CompiledExpr>,
        result_type: Type,
    ) -> Self {
        let mut content_hash =
            ContentHash::of(&[TAG_USER_FUNCTION_CALL]).combine(ContentHash::of_str(&function_name));
        for arg in &args {
            content_hash = content_hash.combine(arg.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            },
            result_type,
            content_hash,
        }
    }

    /// Create a match expression.
    ///
    /// Hash tag byte: `TAG_MATCH` (= `[24]`), matching the inline
    /// implementation in `reify-compiler/src/expr.rs`. Combines the
    /// discriminant hash then, for each arm, each pattern string followed by
    /// the arm body hash — the same combine order as the production inline code.
    pub fn match_expr(
        discriminant: CompiledExpr,
        arms: Vec<CompiledMatchArm>,
        result_type: Type,
    ) -> Self {
        let mut content_hash = ContentHash::of(&[TAG_MATCH]).combine(discriminant.content_hash);
        for arm in &arms {
            for pattern in &arm.patterns {
                content_hash = content_hash.combine(ContentHash::of_str(pattern));
            }
            content_hash = content_hash.combine(arm.body.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
            result_type,
            content_hash,
        }
    }

    /// Create a reflective-aggregation placeholder expression (task-2289).
    ///
    /// Emitted by the compiler in lieu of an empty `ListLiteral` to mark a
    /// `purpose_param.<query_kind>` reference (e.g. `subject.params`) so that
    /// `Engine::activate_purpose` can unambiguously rewrite it into a
    /// populated list of `ValueRef`s against the bound entity's value cells.
    ///
    /// Hash tag byte: `TAG_PURPOSE_REFLECTIVE_AGGREGATION` (= `[25]`).
    /// Combines the tag, then the param_name and query_kind strings, so two
    /// structurally-equal placeholders share a hash and any field difference
    /// produces a distinct hash.
    pub fn purpose_reflective_aggregation(
        param_name: String,
        query_kind: String,
        result_type: Type,
    ) -> Self {
        let content_hash = ContentHash::of(&[TAG_PURPOSE_REFLECTIVE_AGGREGATION])
            .combine(ContentHash::of_str(&param_name))
            .combine(ContentHash::of_str(&query_kind));
        CompiledExpr {
            kind: CompiledExprKind::PurposeReflectiveAggregation {
                param_name,
                query_kind,
            },
            result_type,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::ContentHash;
    use crate::identity::ValueCellId;

    fn make_conditional(
        condition: CompiledExpr,
        then_branch: CompiledExpr,
        else_branch: CompiledExpr,
        result_type: Type,
    ) -> CompiledExpr {
        let hash = ContentHash::of(&[TAG_CONDITIONAL])
            .combine(condition.content_hash)
            .combine(then_branch.content_hash)
            .combine(else_branch.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            result_type,
            content_hash: hash,
        }
    }

    fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
        let hash = ContentHash::of(name.as_bytes());
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: format!("std::{}", name),
                },
                args,
            },
            result_type,
            content_hash: hash,
        }
    }

    #[test]
    fn walk_visits_literal() {
        let expr = CompiledExpr::literal(Value::Int(42), Type::Int);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn walk_collects_value_ref() {
        let id = ValueCellId::new("Part", "x");
        let expr = CompiledExpr::value_ref(id.clone(), Type::length());
        let mut refs = Vec::new();
        expr.walk(&mut |node| {
            if let CompiledExprKind::ValueRef(vid) = &node.kind {
                refs.push(vid.clone());
            }
        });
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], id);
    }

    #[test]
    fn walk_traverses_binop_children() {
        let a = CompiledExpr::value_ref(ValueCellId::new("P", "a"), Type::length());
        let b = CompiledExpr::value_ref(ValueCellId::new("P", "b"), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, a, b, Type::Bool);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn walk_traverses_function_call_args() {
        let arg1 = CompiledExpr::literal(Value::Int(1), Type::Int);
        let arg2 = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = make_function_call("foo", vec![arg1, arg2], Type::Int);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn walk_traverses_conditional_branches() {
        let cond = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let then_br = CompiledExpr::literal(Value::Int(1), Type::Int);
        let else_br = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = make_conditional(cond, then_br, else_br, Type::Int);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 4);
    }

    #[test]
    fn user_function_call_constructs_expected_expr() {
        let arg1 = CompiledExpr::literal(Value::Int(1), Type::Int);
        let arg2 = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = CompiledExpr::user_function_call("f".to_string(), vec![arg1, arg2], Type::Bool);

        // Kind and fields.
        match &expr.kind {
            CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            } => {
                assert_eq!(function_name, "f");
                assert_eq!(args.len(), 2);
                // Verify arg contents are preserved (not swapped or dropped).
                assert!(
                    matches!(&args[0].kind, CompiledExprKind::Literal(Value::Int(1))),
                    "args[0] should be Literal(Int(1))"
                );
                assert!(
                    matches!(&args[1].kind, CompiledExprKind::Literal(Value::Int(2))),
                    "args[1] should be Literal(Int(2))"
                );
            }
            other => panic!("expected UserFunctionCall, got {other:?}"),
        }
        assert_eq!(expr.result_type, Type::Bool);

        // Content hash differs for different function names.
        let other_name = CompiledExpr::user_function_call("g".to_string(), vec![], Type::Bool);
        assert_ne!(
            expr.content_hash, other_name.content_hash,
            "hash should differ for different function names"
        );

        // Content hash differs for different args.
        let no_args = CompiledExpr::user_function_call("f".to_string(), vec![], Type::Bool);
        assert_ne!(
            expr.content_hash, no_args.content_hash,
            "hash should differ for different args"
        );
    }

    #[test]
    fn match_expr_constructs_expected_expr() {
        let discriminant = CompiledExpr::literal(Value::Int(1), Type::Int);
        let arm_body = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let arm = CompiledMatchArm {
            patterns: vec!["_".to_string()],
            body: arm_body,
        };
        let expr = CompiledExpr::match_expr(discriminant.clone(), vec![arm], Type::Bool);

        // Kind and fields.
        match &expr.kind {
            CompiledExprKind::Match {
                discriminant: d,
                arms,
            } => {
                assert_eq!(arms.len(), 1);
                assert_eq!(arms[0].patterns, vec!["_".to_string()]);
                // Verify the discriminant was preserved (not dropped or replaced).
                assert!(
                    matches!(&d.kind, CompiledExprKind::Literal(Value::Int(1))),
                    "discriminant should be Literal(Int(1))"
                );
                assert_eq!(
                    d.result_type,
                    Type::Int,
                    "discriminant result_type should be Int"
                );
            }
            other => panic!("expected Match, got {other:?}"),
        }
        assert_eq!(expr.result_type, Type::Bool);

        // Content hash differs when discriminant changes.
        let different_discriminant = CompiledExpr::literal(Value::Int(99), Type::Int);
        let arm2 = CompiledMatchArm {
            patterns: vec!["_".to_string()],
            body: CompiledExpr::literal(Value::Bool(true), Type::Bool),
        };
        let expr2 = CompiledExpr::match_expr(different_discriminant, vec![arm2], Type::Bool);
        assert_ne!(
            expr.content_hash, expr2.content_hash,
            "hash should differ when discriminant changes"
        );

        // Content hash differs when arm body changes.
        let arm3 = CompiledMatchArm {
            patterns: vec!["_".to_string()],
            body: CompiledExpr::literal(Value::Bool(false), Type::Bool),
        };
        let expr3 = CompiledExpr::match_expr(discriminant.clone(), vec![arm3], Type::Bool);
        assert_ne!(
            expr.content_hash, expr3.content_hash,
            "hash should differ when arm body changes"
        );

        // Content hash differs when arm patterns change.
        let arm4 = CompiledMatchArm {
            patterns: vec!["A".to_string()],
            body: CompiledExpr::literal(Value::Bool(true), Type::Bool),
        };
        let arm5 = CompiledMatchArm {
            patterns: vec!["B".to_string()],
            body: CompiledExpr::literal(Value::Bool(true), Type::Bool),
        };
        let expr4 = CompiledExpr::match_expr(discriminant.clone(), vec![arm4], Type::Bool);
        let expr5 = CompiledExpr::match_expr(discriminant.clone(), vec![arm5], Type::Bool);
        assert_ne!(
            expr4.content_hash, expr5.content_hash,
            "hash should differ when arm patterns change"
        );

        // Reproducibility: two constructions with identical inputs yield equal hashes.
        let arm_a = CompiledMatchArm {
            patterns: vec!["_".to_string()],
            body: CompiledExpr::literal(Value::Bool(true), Type::Bool),
        };
        let arm_b = CompiledMatchArm {
            patterns: vec!["_".to_string()],
            body: CompiledExpr::literal(Value::Bool(true), Type::Bool),
        };
        let expr_a = CompiledExpr::match_expr(discriminant.clone(), vec![arm_a], Type::Bool);
        let expr_b = CompiledExpr::match_expr(discriminant, vec![arm_b], Type::Bool);
        assert_eq!(
            expr_a.content_hash, expr_b.content_hash,
            "identical inputs should produce identical hashes"
        );
    }

    /// Lock in the combine order for arms with multiple patterns.
    ///
    /// The hash for a match arm is: `pattern[0]` → `pattern[1]` → … → `body`.
    /// Swapping pattern order must produce a different hash; adding a second
    /// pattern must differ from the single-pattern case.  This test pins that
    /// behaviour so a refactor that accidentally collapses or reorders combines
    /// fails here rather than silently emitting wrong hashes.
    #[test]
    fn match_expr_multi_pattern_arm_combine_order() {
        let discriminant = CompiledExpr::literal(Value::Int(1), Type::Int);
        let body = CompiledExpr::literal(Value::Bool(true), Type::Bool);

        // Arm with patterns ["A", "B"]
        let arm_ab = CompiledMatchArm {
            patterns: vec!["A".to_string(), "B".to_string()],
            body: body.clone(),
        };
        let expr_ab = CompiledExpr::match_expr(discriminant.clone(), vec![arm_ab], Type::Bool);

        // Arm with patterns ["B", "A"] — same set, reversed order.
        let arm_ba = CompiledMatchArm {
            patterns: vec!["B".to_string(), "A".to_string()],
            body: body.clone(),
        };
        let expr_ba = CompiledExpr::match_expr(discriminant.clone(), vec![arm_ba], Type::Bool);

        assert_ne!(
            expr_ab.content_hash, expr_ba.content_hash,
            "hash should differ when multi-pattern arm order is reversed: \
             pattern combine order must be stable"
        );

        // Arm with only ["A"] — a strict prefix of ["A", "B"].
        let arm_a_only = CompiledMatchArm {
            patterns: vec!["A".to_string()],
            body: body.clone(),
        };
        let expr_a_only =
            CompiledExpr::match_expr(discriminant.clone(), vec![arm_a_only], Type::Bool);

        assert_ne!(
            expr_ab.content_hash, expr_a_only.content_hash,
            "hash should differ between arm [\"A\",\"B\"] and arm [\"A\"]: \
             second pattern must actually be combined"
        );

        // Reproducibility: same multi-pattern arm twice → equal hashes.
        let arm_ab2 = CompiledMatchArm {
            patterns: vec!["A".to_string(), "B".to_string()],
            body: body.clone(),
        };
        let expr_ab2 = CompiledExpr::match_expr(discriminant, vec![arm_ab2], Type::Bool);
        assert_eq!(
            expr_ab.content_hash, expr_ab2.content_hash,
            "identical multi-pattern arm inputs should produce identical hashes"
        );
    }

    /// Regression guard: Match must not share any occupied tag byte.
    ///
    /// This test reconstructs the hash that Match *would* produce using each
    /// tag byte from [0] through [23] (all CompiledExpr tags [0..=19] plus the
    /// [20..=23] range reserved for `CachedResult` in `reify-eval`) and asserts
    /// that the actual `match_expr` hash is different from every one of them.
    ///
    /// This guards against two classes of regression:
    /// - Accidentally reverting to [6] (the pre-fix UserFunctionCall collision).
    /// - Accidentally picking any other already-occupied tag byte for Match.
    ///
    /// The test fails against the pre-fix code (where Match used [6]) and passes
    /// once Match is assigned tag [24].
    #[test]
    fn match_expr_does_not_share_tag_byte_with_user_function_call() {
        let disc = CompiledExpr::literal(Value::Int(0), Type::Int);
        let body = CompiledExpr::literal(Value::Int(1), Type::Int);

        // Capture sub-hashes before the values are moved into the constructor.
        let disc_hash = disc.content_hash;
        let body_hash = body.content_hash;
        let pattern_hash = ContentHash::of_str("p");

        let arm = CompiledMatchArm {
            patterns: vec!["p".to_string()],
            body,
        };
        let match_expr = CompiledExpr::match_expr(disc, vec![arm], Type::Int);

        // For every tag byte in [0..=23], reconstruct the hash using the same
        // combine formula as `match_expr` and assert the actual hash differs.
        // ContentHash is Copy, so disc_hash/body_hash/pattern_hash can be reused
        // across loop iterations.
        for tag in 0u8..=23 {
            let hypothetical = ContentHash::of(&[tag])
                .combine(disc_hash)
                .combine(pattern_hash)
                .combine(body_hash);
            assert_ne!(
                match_expr.content_hash, hypothetical,
                "Match hash must not equal the hash produced by tag byte [{}] \
                 (Match must use a unique tag to avoid collisions with existing \
                 CompiledExpr or CachedResult variants)",
                tag
            );
        }
    }

    #[test]
    fn walk_traverses_deeply_nested() {
        let a = CompiledExpr::value_ref(ValueCellId::new("P", "a"), Type::length());
        let b = CompiledExpr::value_ref(ValueCellId::new("P", "b"), Type::length());
        let condition = CompiledExpr::binop(BinOp::Gt, a, b, Type::Bool);
        let c = CompiledExpr::value_ref(ValueCellId::new("P", "c"), Type::length());
        let one_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.001,
                dimension: crate::DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let then_br = CompiledExpr::binop(BinOp::Gt, c, one_mm, Type::Bool);
        let d = CompiledExpr::value_ref(ValueCellId::new("P", "d"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: crate::DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let else_br = CompiledExpr::binop(BinOp::Gt, d, two_mm, Type::Bool);
        let expr = make_conditional(condition, then_br, else_br, Type::Bool);
        let mut refs = Vec::new();
        expr.walk(&mut |node| {
            if let CompiledExprKind::ValueRef(vid) = &node.kind {
                refs.push(vid.clone());
            }
        });
        assert_eq!(refs.len(), 4);
    }

    /// Assert that every TAG_* constant carries the expected byte value.
    ///
    /// This test is the "red" step: it fails to compile until the 21 `TAG_*`
    /// constants are defined (step 2).  Once they exist, the assertions lock
    /// their values so no future edit can silently change the byte assigned to
    /// a variant.
    #[test]
    fn tag_byte_constants_have_expected_values() {
        use super::{
            TAG_AD_HOC_SELECTOR, TAG_BIN_OP, TAG_CONDITIONAL, TAG_DETERMINACY_PREDICATE,
            TAG_FUNCTION_CALL, TAG_INDEX_ACCESS, TAG_LAMBDA, TAG_LIST_LITERAL, TAG_LITERAL,
            TAG_MAP_LITERAL, TAG_MATCH, TAG_META_ACCESS, TAG_METHOD_CALL, TAG_OPTION_NONE,
            TAG_OPTION_SOME, TAG_QUANTIFIER, TAG_RANGE_CONSTRUCTOR, TAG_REFLECTIVE_CELL_LIST,
            TAG_SET_LITERAL, TAG_UN_OP, TAG_USER_FUNCTION_CALL, TAG_VALUE_REF,
        };

        assert_eq!(TAG_LITERAL, 0u8);
        assert_eq!(TAG_VALUE_REF, 1u8);
        assert_eq!(TAG_BIN_OP, 2u8);
        assert_eq!(TAG_UN_OP, 3u8);
        assert_eq!(TAG_FUNCTION_CALL, 4u8);
        assert_eq!(TAG_CONDITIONAL, 5u8);
        assert_eq!(TAG_USER_FUNCTION_CALL, 6u8);
        assert_eq!(TAG_LAMBDA, 7u8);
        assert_eq!(TAG_LIST_LITERAL, 8u8);
        assert_eq!(TAG_SET_LITERAL, 9u8);
        assert_eq!(TAG_MAP_LITERAL, 10u8);
        assert_eq!(TAG_INDEX_ACCESS, 11u8);
        assert_eq!(TAG_METHOD_CALL, 12u8);
        assert_eq!(TAG_QUANTIFIER, 13u8);
        assert_eq!(TAG_OPTION_SOME, 14u8);
        assert_eq!(TAG_OPTION_NONE, 15u8);
        assert_eq!(TAG_META_ACCESS, 16u8);
        assert_eq!(TAG_DETERMINACY_PREDICATE, 17u8);
        assert_eq!(TAG_RANGE_CONSTRUCTOR, 18u8);
        assert_eq!(TAG_AD_HOC_SELECTOR, 19u8);
        assert_eq!(TAG_MATCH, 24u8);
        // task-2458: new variant tag byte
        assert_eq!(TAG_REFLECTIVE_CELL_LIST, 26u8);

        // Lock: constructing via the public API and reconstructing via TAG_*
        // must produce the same content hash.

        // user_function_call with no args: tag + function name.
        let ufc = CompiledExpr::user_function_call("f".to_string(), vec![], Type::Bool);
        let expected_ufc =
            ContentHash::of(&[TAG_USER_FUNCTION_CALL]).combine(ContentHash::of_str("f"));
        assert_eq!(
            ufc.content_hash, expected_ufc,
            "user_function_call hash must equal TAG_USER_FUNCTION_CALL-based reconstruction"
        );

        // match_expr with one wildcard arm: tag + disc + pattern + body.
        let disc = CompiledExpr::literal(Value::Int(1), Type::Int);
        let body = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let disc_hash = disc.content_hash;
        let body_hash = body.content_hash;
        let arm = CompiledMatchArm {
            patterns: vec!["_".to_string()],
            body,
        };
        let match_e = CompiledExpr::match_expr(disc, vec![arm], Type::Bool);
        let expected_match = ContentHash::of(&[TAG_MATCH])
            .combine(disc_hash)
            .combine(ContentHash::of_str("_"))
            .combine(body_hash);
        assert_eq!(
            match_e.content_hash, expected_match,
            "match_expr hash must equal TAG_MATCH-based reconstruction"
        );
    }

    /// step-1 (task-2289): `remap_cell` rewrites every occurrence of a target
    /// `ValueCellId` to a replacement, traversing all variants that carry a cell.
    ///
    /// Builds a tree exercising every cell-bearing variant the remap must touch:
    /// `ValueRef`, `BinOp` containing `ValueRef`s, `Quantifier { variable_id }`,
    /// `DeterminacyPredicate { cell }`, and `Lambda { captures, param_ids }`.
    /// Calls `remap_cell(old, new)` once, then asserts every matching id was
    /// rewritten and every non-matching id is unchanged.
    ///
    /// RED before step-2: `remap_cell` does not yet exist on `CompiledExpr`.
    #[test]
    fn remap_cell_rewrites_all_matching_occurrences() {
        let old = ValueCellId::new("E", "x");
        let other = ValueCellId::new("E", "y");
        let new_id = ValueCellId::new("E2", "x_renamed");
        let unrelated = ValueCellId::new("U", "z");

        // ValueRef(old) → BinOp ValueRef(old) > ValueRef(other)
        let lhs = CompiledExpr::value_ref(old.clone(), Type::Real);
        let rhs = CompiledExpr::value_ref(other.clone(), Type::Real);
        let binop = CompiledExpr::binop(BinOp::Gt, lhs, rhs, Type::Bool);

        // Quantifier with variable_id == old, predicate references determinacy on old
        let det =
            CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, old.clone());
        let coll = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Real)));
        let quant = CompiledExpr::quantifier(
            QuantifierKind::ForAll,
            "p".to_string(),
            old.clone(),
            coll,
            det,
        );

        // Lambda with captures = [old, unrelated] and param_ids = [old]
        let lambda_body = CompiledExpr::value_ref(unrelated.clone(), Type::Real);
        let lambda = CompiledExpr::lambda(
            vec![("p".to_string(), Some(Type::Real))],
            vec![old.clone()],
            lambda_body,
            vec![old.clone(), unrelated.clone()],
            Type::Real,
        );

        // Combine all under a top-level BinOp so a single remap_cell call walks them.
        // Use a Conditional to bundle the three sub-expressions (binop, quant, lambda)
        // since they have different result types and BinOp wants matching children.
        let cond_then = CompiledExpr::list_literal(
            vec![binop, quant, lambda],
            Type::List(Box::new(Type::Real)),
        );
        let cond_else = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Real)));
        let mut tree = make_conditional(
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            cond_then,
            cond_else,
            Type::List(Box::new(Type::Real)),
        );

        tree.remap_cell(&old, &new_id);

        // Walk and collect every cell-bearing site we touched.
        let mut value_refs = Vec::new();
        let mut quant_var_ids = Vec::new();
        let mut det_cells = Vec::new();
        let mut lambda_captures = Vec::new();
        let mut lambda_param_ids = Vec::new();
        tree.walk(&mut |node| match &node.kind {
            CompiledExprKind::ValueRef(id) => value_refs.push(id.clone()),
            CompiledExprKind::Quantifier { variable_id, .. } => {
                quant_var_ids.push(variable_id.clone())
            }
            CompiledExprKind::DeterminacyPredicate { cell, .. } => det_cells.push(cell.clone()),
            CompiledExprKind::Lambda {
                captures,
                param_ids,
                ..
            } => {
                lambda_captures.extend(captures.iter().cloned());
                lambda_param_ids.extend(param_ids.iter().cloned());
            }
            _ => {}
        });

        // Every direct ValueRef(old) is rewritten to new_id; ValueRef(other) is
        // unchanged; ValueRef(unrelated) (lambda body) is unchanged.
        assert!(
            value_refs.contains(&new_id),
            "ValueRef(old) should be rewritten to new_id"
        );
        assert!(value_refs.contains(&other), "ValueRef(other) must remain");
        assert!(
            value_refs.contains(&unrelated),
            "ValueRef(unrelated) must remain"
        );
        assert!(!value_refs.contains(&old), "no ValueRef(old) should remain");

        // Quantifier.variable_id rewritten.
        assert_eq!(quant_var_ids.len(), 1);
        assert_eq!(
            quant_var_ids[0], new_id,
            "Quantifier.variable_id should be rewritten"
        );

        // DeterminacyPredicate.cell rewritten.
        assert_eq!(det_cells.len(), 1);
        assert_eq!(
            det_cells[0], new_id,
            "DeterminacyPredicate.cell should be rewritten"
        );

        // Lambda captures: old → new_id, unrelated → unchanged.
        assert!(
            lambda_captures.contains(&new_id),
            "Lambda capture(old) should be rewritten"
        );
        assert!(
            lambda_captures.contains(&unrelated),
            "Lambda capture(unrelated) must remain"
        );
        assert!(
            !lambda_captures.contains(&old),
            "no Lambda capture(old) should remain"
        );

        // Lambda param_ids: old → new_id.
        assert!(
            lambda_param_ids.contains(&new_id),
            "Lambda param_id(old) should be rewritten"
        );
        assert!(
            !lambda_param_ids.contains(&old),
            "no Lambda param_id(old) should remain"
        );
    }

    /// step-3 (task-2289): constructor for the new
    /// `PurposeReflectiveAggregation` variant builds a node with the expected
    /// shape and result_type.
    ///
    /// RED before step-4: variant + constructor do not yet exist.
    #[test]
    fn purpose_reflective_aggregation_constructs_expected_kind() {
        let expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        match &expr.kind {
            CompiledExprKind::PurposeReflectiveAggregation {
                param_name,
                query_kind,
            } => {
                assert_eq!(param_name, "subject");
                assert_eq!(query_kind, "params");
            }
            other => panic!("expected PurposeReflectiveAggregation, got {other:?}"),
        }
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Real)));
    }

    /// step-3 (task-2289): `walk` visits the placeholder node itself but has no
    /// children — same shape as `OptionNone` / `MetaAccess`.
    #[test]
    fn purpose_reflective_aggregation_walk_has_no_children() {
        let expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 1, "placeholder must be a leaf node");
    }

    /// step-3 (task-2289): `collect_value_refs` returns an empty Vec for the
    /// placeholder — it has no `ValueCellId` until activation expands it.
    #[test]
    fn purpose_reflective_aggregation_has_no_value_refs() {
        let expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );
        assert!(expr.collect_value_refs().is_empty());
    }

    // ── step-1 (task-2458): ReflectiveCellList variant tests ─────────────────

    /// task-2458 step-1: `reflective_cell_list` constructor builds a node with
    /// the expected kind, result_type, and a non-zero content_hash.
    ///
    /// Also verifies the hash is structural: an empty RCL differs from a
    /// populated one, and an RCL differs from a `ListLiteral` with the same
    /// elements (tag-byte isolation).
    ///
    /// RED before step-2: variant, tag, and constructor do not yet exist.
    #[test]
    fn reflective_cell_list_constructs_expected_expr() {
        use super::TAG_REFLECTIVE_CELL_LIST;

        let cell_x = ValueCellId::new("E", "x");
        let cell_y = ValueCellId::new("E", "y");
        let elem_x = CompiledExpr::value_ref(cell_x.clone(), Type::Real);
        let elem_y = CompiledExpr::value_ref(cell_y.clone(), Type::Real);
        let result_type = Type::List(Box::new(Type::Real));

        let rcl = CompiledExpr::reflective_cell_list(
            vec![elem_x.clone(), elem_y.clone()],
            result_type.clone(),
        );

        match &rcl.kind {
            CompiledExprKind::ReflectiveCellList(elements) => {
                assert_eq!(elements.len(), 2, "should have 2 elements");
                assert!(
                    matches!(&elements[0].kind, CompiledExprKind::ValueRef(id) if *id == cell_x),
                    "first element should be ValueRef(E.x)"
                );
                assert!(
                    matches!(&elements[1].kind, CompiledExprKind::ValueRef(id) if *id == cell_y),
                    "second element should be ValueRef(E.y)"
                );
            }
            other => panic!("expected ReflectiveCellList, got {other:?}"),
        }
        assert_eq!(rcl.result_type, result_type, "result_type must match");
        assert_ne!(
            rcl.content_hash,
            ContentHash::of(&[0u8; 0]),
            "content_hash must be non-zero"
        );

        // Empty RCL has a different hash from a populated one.
        let empty_rcl = CompiledExpr::reflective_cell_list(vec![], result_type.clone());
        assert_ne!(
            rcl.content_hash, empty_rcl.content_hash,
            "empty RCL must differ from populated RCL"
        );

        // RCL must differ from ListLiteral with the same elements (tag-byte isolation).
        let ll = CompiledExpr::list_literal(
            vec![
                CompiledExpr::value_ref(cell_x.clone(), Type::Real),
                CompiledExpr::value_ref(cell_y.clone(), Type::Real),
            ],
            result_type.clone(),
        );
        assert_ne!(
            rcl.content_hash, ll.content_hash,
            "RCL tag byte must distinguish hash from ListLiteral with same elements"
        );
        assert_ne!(
            TAG_REFLECTIVE_CELL_LIST, TAG_LIST_LITERAL,
            "tag bytes must differ"
        );
    }

    /// task-2458 step-1: `walk` visits the RCL root plus all element nodes.
    ///
    /// An RCL with 3 ValueRef elements must report 4 visited nodes (root + 3).
    ///
    /// RED before step-2.
    #[test]
    fn reflective_cell_list_walk_traverses_elements() {
        let elements: Vec<CompiledExpr> = (0..3)
            .map(|i| CompiledExpr::value_ref(ValueCellId::new("E", format!("c{i}")), Type::Real))
            .collect();
        let rcl = CompiledExpr::reflective_cell_list(elements, Type::List(Box::new(Type::Real)));
        let mut count = 0;
        rcl.walk(&mut |_| count += 1);
        assert_eq!(count, 4, "walk must visit root + 3 element nodes");
    }

    /// task-2458 step-1: `collect_value_refs` on an RCL returns all element
    /// cell IDs in order.
    ///
    /// RED before step-2.
    #[test]
    fn reflective_cell_list_collect_value_refs_includes_all_elements() {
        let cell_a = ValueCellId::new("E", "a");
        let cell_b = ValueCellId::new("E", "b");
        let rcl = CompiledExpr::reflective_cell_list(
            vec![
                CompiledExpr::value_ref(cell_a.clone(), Type::Real),
                CompiledExpr::value_ref(cell_b.clone(), Type::Real),
            ],
            Type::List(Box::new(Type::Real)),
        );
        let refs = rcl.collect_value_refs();
        assert_eq!(
            refs,
            vec![cell_a, cell_b],
            "collect_value_refs must return both cell IDs in order"
        );
    }

    /// task-2458 step-1: `remap_entity` recurses into RCL elements and rewrites
    /// matching entity names.
    ///
    /// RED before step-2.
    #[test]
    fn reflective_cell_list_remap_entity_recurses_into_elements() {
        let cell_x = ValueCellId::new("E", "x");
        let cell_y = ValueCellId::new("E", "y");
        let mut rcl = CompiledExpr::reflective_cell_list(
            vec![
                CompiledExpr::value_ref(cell_x.clone(), Type::Real),
                CompiledExpr::value_ref(cell_y.clone(), Type::Real),
            ],
            Type::List(Box::new(Type::Real)),
        );
        rcl.remap_entity("E", "F");

        let refs = rcl.collect_value_refs();
        assert_eq!(refs.len(), 2);
        assert_eq!(
            refs[0].entity, "F",
            "first element entity must be rewritten to F"
        );
        assert_eq!(
            refs[1].entity, "F",
            "second element entity must be rewritten to F"
        );
        assert_eq!(refs[0].member, "x", "member must be unchanged");
        assert_eq!(refs[1].member, "y", "member must be unchanged");
    }

    /// task-2458 step-1: `remap_cell` recurses into RCL elements and rewrites
    /// matching cell IDs.
    ///
    /// RED before step-2.
    #[test]
    fn reflective_cell_list_remap_cell_recurses_into_elements() {
        let cell_x = ValueCellId::new("E", "x");
        let cell_y = ValueCellId::new("E", "y");
        let cell_new = ValueCellId::new("E2", "x_new");
        let mut rcl = CompiledExpr::reflective_cell_list(
            vec![
                CompiledExpr::value_ref(cell_x.clone(), Type::Real),
                CompiledExpr::value_ref(cell_y.clone(), Type::Real),
            ],
            Type::List(Box::new(Type::Real)),
        );
        rcl.remap_cell(&cell_x, &cell_new);

        let refs = rcl.collect_value_refs();
        assert_eq!(refs.len(), 2);
        assert_eq!(
            refs[0], cell_new,
            "first element must be rewritten to cell_new"
        );
        assert_eq!(refs[1], cell_y, "second element must remain cell_y");
    }

    // ── end task-2458 step-1 tests ────────────────────────────────────────────

    // ── task-2552: constructor-level ValueRef invariant tests ─────────────────

    /// task-2552: In debug builds, `reflective_cell_list` must panic when any
    /// element is not a `ValueRef`.
    ///
    /// Rationale: the `ReflectiveCellList(_)` no-op arm in
    /// `expand_purpose_reflective_placeholders` (reify-eval, task-2544) elides
    /// recursion on the basis that all elements are `ValueRef`s — a non-ValueRef
    /// element would silently bypass placeholder expansion in release builds.
    /// Moving the invariant into the constructor (task-2552) protects every
    /// future caller automatically.
    ///
    /// RED before step-2 (constructor has no debug_assert yet).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "ReflectiveCellList elements must be ValueRefs")]
    fn reflective_cell_list_panics_in_debug_when_element_is_not_value_ref() {
        let cell_a = ValueCellId::new("E", "a");
        let elements = vec![
            CompiledExpr::value_ref(cell_a, Type::Real),
            CompiledExpr::literal(Value::Int(0), Type::Int),
        ];
        // Must panic in debug builds because the second element is a Literal,
        // not a ValueRef.
        let _rcl = CompiledExpr::reflective_cell_list(elements, Type::List(Box::new(Type::Real)));
    }

    // ── end task-2552 tests ───────────────────────────────────────────────────

    // ── task-2629 step-1 tests ────────────────────────────────────────────

    /// task-2629 step-1: `map_value_refs` rewrites every `ValueRef` cell ID
    /// matching the predicate, recomputing `content_hash` on every node it
    /// rewrites — distinct from `remap_cell` (which mutates in place and
    /// leaves stale ancestor hashes).
    ///
    /// Builds `BinOp(Lt, ValueRef("S.vents[0]","mass"), Literal(50kg))`,
    /// rewrites `S.vents[0]` → `S.vents[2]`, and asserts the resulting
    /// expr is `BinOp(Lt, ValueRef("S.vents[2]","mass"), Literal(50kg))`
    /// with a fresh content_hash that differs from the input.
    ///
    /// RED before step-2: `map_value_refs` does not yet exist.
    #[test]
    fn map_value_refs_rewrites_nested_value_ref_and_recomputes_hash() {
        let from_id = ValueCellId::new("S.vents[0]", "mass");
        let to_id = ValueCellId::new("S.vents[2]", "mass");
        let mass_ty = Type::Scalar {
            dimension: crate::DimensionVector::MASS,
        };

        let lhs = CompiledExpr::value_ref(from_id.clone(), mass_ty.clone());
        let rhs = CompiledExpr::literal(
            Value::Scalar {
                si_value: 50.0,
                dimension: crate::DimensionVector::MASS,
            },
            mass_ty.clone(),
        );
        let original = CompiledExpr::binop(BinOp::Lt, lhs, rhs, Type::Bool);
        let original_hash = original.content_hash;

        let rewritten = original.map_value_refs(&mut |id| {
            if id.entity == "S.vents[0]" {
                ValueCellId::new("S.vents[2]", id.member)
            } else {
                id
            }
        });

        // Top-level shape preserved.
        assert!(
            matches!(
                &rewritten.kind,
                CompiledExprKind::BinOp { op: BinOp::Lt, .. }
            ),
            "rewritten expr must remain BinOp(Lt, ...)"
        );
        match &rewritten.kind {
            CompiledExprKind::BinOp { left, right, .. } => {
                match &left.kind {
                    CompiledExprKind::ValueRef(id) => {
                        assert_eq!(
                            *id, to_id,
                            "left ValueRef must be rewritten to S.vents[2].mass"
                        );
                    }
                    other => panic!("expected left ValueRef, got {other:?}"),
                }
                match &right.kind {
                    CompiledExprKind::Literal(_) => {}
                    other => panic!("expected right Literal, got {other:?}"),
                }
            }
            other => panic!("expected BinOp, got {other:?}"),
        }
        assert_eq!(rewritten.result_type, Type::Bool);

        // The content_hash was recomputed (differs from the input).
        assert_ne!(
            rewritten.content_hash, original_hash,
            "content_hash must differ after rewrite"
        );

        // Reproducibility: two equivalent rewrites produce equal hashes.
        let lhs2 = CompiledExpr::value_ref(from_id.clone(), mass_ty.clone());
        let rhs2 = CompiledExpr::literal(
            Value::Scalar {
                si_value: 50.0,
                dimension: crate::DimensionVector::MASS,
            },
            mass_ty,
        );
        let original2 = CompiledExpr::binop(BinOp::Lt, lhs2, rhs2, Type::Bool);
        let rewritten2 = original2.map_value_refs(&mut |id| {
            if id.entity == "S.vents[0]" {
                ValueCellId::new("S.vents[2]", id.member)
            } else {
                id
            }
        });
        assert_eq!(
            rewritten.content_hash, rewritten2.content_hash,
            "two equivalent rewrites must yield identical hashes"
        );
    }

    /// task-2629 step-1: `map_value_refs` is a no-op (clone-only) on Literal
    /// nodes — it simply rebuilds the literal node with its existing hash.
    ///
    /// RED before step-2.
    #[test]
    fn map_value_refs_noop_on_literal() {
        let original = CompiledExpr::literal(Value::Int(42), Type::Int);
        let original_hash = original.content_hash;
        // Identity transform on cells; should still yield the same hash.
        let rewritten = original.map_value_refs(&mut |id| id);
        match &rewritten.kind {
            CompiledExprKind::Literal(Value::Int(n)) => assert_eq!(*n, 42_i64),
            other => panic!("expected Literal(Int(42)), got {other:?}"),
        }
        assert_eq!(
            rewritten.content_hash, original_hash,
            "identity rewrite on Literal must preserve content_hash"
        );
    }

    // ── end task-2629 step-1 tests ───────────────────────────────────────

    /// step-3 (task-2289): structurally-equal placeholders share content_hash;
    /// structurally-different placeholders (different `query_kind`) differ.
    #[test]
    fn purpose_reflective_aggregation_content_hash_is_structural() {
        let a = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );
        let b = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );
        let different_kind = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "geometric_params".to_string(),
            Type::List(Box::new(Type::Real)),
        );
        let different_param = CompiledExpr::purpose_reflective_aggregation(
            "other".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        assert_eq!(
            a.content_hash, b.content_hash,
            "identical inputs → identical hashes"
        );
        assert_ne!(
            a.content_hash, different_kind.content_hash,
            "different query_kind must change the hash"
        );
        assert_ne!(
            a.content_hash, different_param.content_hash,
            "different param_name must change the hash"
        );
    }

    // ── task-3663 tests ───────────────────────────────────────────────────────

    /// The consumer at `entity.rs:1140` uses `split_once('.')` to extract the
    /// sub-geometry name from the entity stamp (`"<parent>.<sub>"`).  If a
    /// caller constructs a `CrossSubGeometryRef` with a dot-free entity,
    /// `split_once` returns `None` and the consumer silently falls back to the
    /// full entity string as the sub name — a hard-to-diagnose bug.
    ///
    /// Moving the invariant into `cross_sub_geometry_ref` (step-4, task-3663)
    /// makes `CompiledExpr::cross_sub_geometry_ref` the canonical chokepoint:
    /// every future creator (direct or via `map_value_refs` rebuild) is
    /// protected automatically.  The check was introduced alongside the typed
    /// variant in task-3508 but left to the caller; this closes the gap.
    ///
    /// Modelled after `reflective_cell_list_panics_in_debug_when_element_is_not_value_ref`
    /// (task-2552, above).
    ///
    /// RED before step-4: the constructor has no `debug_assert`, so no panic
    /// fires and the test fails.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "CrossSubGeometryRef entity must be a `<parent>.<sub>` stamp")]
    fn cross_sub_geometry_ref_panics_in_debug_when_entity_lacks_dot() {
        // "NoDotEntity" contains no '.', violating the <parent>.<sub> invariant.
        let _expr = CompiledExpr::cross_sub_geometry_ref(
            ValueCellId::new("NoDotEntity", "member"),
            Type::Geometry,
        );
    }

    /// `map_value_refs` on a `CrossSubGeometryRef` preserves the `<parent>.<sub>`
    /// entity-stamp shape.
    ///
    /// The `map_value_refs` arm at `expr.rs:513-518` reconstructs the variant via
    /// `CompiledExpr::cross_sub_geometry_ref(f(id), result_type)`.  Because the
    /// constructor contains the canonical `debug_assert!(id.entity.contains('.'))`,
    /// any remap closure `f` that strips the dot is caught in debug builds.
    ///
    /// This test confirms the positive case: a closure that remaps one dotted
    /// entity to another dotted entity round-trips without panic and yields a
    /// `CrossSubGeometryRef` whose entity still satisfies the shape invariant.
    /// It makes the coupling between the `map_value_refs` rebuild path and the
    /// constructor assertion explicit and regression-guarded.
    ///
    /// task-3508 introduced the typed variant; task-3663 introduced the constructor
    /// assert and this test.
    #[test]
    fn map_value_refs_preserves_cross_sub_geometry_ref_stamp_shape() {
        let original_id = ValueCellId::new("Parent.sub", "body");
        let expected_id = ValueCellId::new("OtherParent.outer", "body");

        let expr = CompiledExpr::cross_sub_geometry_ref(original_id.clone(), Type::Geometry);

        // Remap "Parent.sub" → "OtherParent.outer", preserving the dotted shape.
        let remapped = expr.map_value_refs(&mut |id| {
            if id.entity == "Parent.sub" {
                ValueCellId::new("OtherParent.outer", id.member)
            } else {
                id
            }
        });

        // Must remain a CrossSubGeometryRef after the remap.
        match &remapped.kind {
            CompiledExprKind::CrossSubGeometryRef(id) => {
                assert_eq!(
                    *id, expected_id,
                    "entity stamp must be remapped to OtherParent.outer.body"
                );
                assert!(
                    id.entity.contains('.'),
                    "remapped entity stamp must still satisfy the <parent>.<sub> shape invariant"
                );
            }
            other => panic!("expected CrossSubGeometryRef after map_value_refs, got {other:?}"),
        }
        assert_eq!(remapped.result_type, Type::Geometry);
    }

    // ── task-3702 tests ───────────────────────────────────────────────────────

    /// `CompiledFunction::new_with_no_defaults` produces the canonical
    /// `param_defaults` shape: length == params.len(), every entry is `None`.
    ///
    /// Tests three arities (0, 1, 2) to confirm `vec![None; n]` is produced
    /// for all sizes, including the nullary case where `vec![None; 0]` ==
    /// `Vec::new()`.
    ///
    /// RED before step-2: the constructor does not yet exist, so this test
    /// fails to compile.
    ///
    /// task-3702 (canonicalize CompiledFunction.param_defaults representation)
    #[test]
    fn compiled_function_new_with_no_defaults_produces_canonical_shape() {
        let stub_body = || CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Real(0.0), Type::Real),
        };
        let hash = ContentHash::of_str("stub");

        // arity 0
        let f0 = CompiledFunction::new_with_no_defaults(
            "f0".to_string(),
            false,
            vec![],
            Type::Real,
            stub_body(),
            hash.clone(),
            vec![],
            None,
        );
        assert_eq!(
            f0.param_defaults.len(),
            f0.params.len(),
            "arity-0: param_defaults.len() must equal params.len()"
        );
        assert!(
            f0.param_defaults.iter().all(|d| d.is_none()),
            "arity-0: every param_defaults entry must be None"
        );

        // arity 1
        let f1 = CompiledFunction::new_with_no_defaults(
            "f1".to_string(),
            false,
            vec![("x".to_string(), Type::Real)],
            Type::Real,
            stub_body(),
            hash.clone(),
            vec![],
            None,
        );
        assert_eq!(
            f1.param_defaults.len(),
            f1.params.len(),
            "arity-1: param_defaults.len() must equal params.len()"
        );
        assert!(
            f1.param_defaults.iter().all(|d| d.is_none()),
            "arity-1: every param_defaults entry must be None"
        );

        // arity 2
        let f2 = CompiledFunction::new_with_no_defaults(
            "f2".to_string(),
            false,
            vec![
                ("x".to_string(), Type::Real),
                ("y".to_string(), Type::Real),
            ],
            Type::Real,
            stub_body(),
            hash.clone(),
            vec![],
            None,
        );
        assert_eq!(
            f2.param_defaults.len(),
            f2.params.len(),
            "arity-2: param_defaults.len() must equal params.len()"
        );
        assert!(
            f2.param_defaults.iter().all(|d| d.is_none()),
            "arity-2: every param_defaults entry must be None"
        );
    }
}
