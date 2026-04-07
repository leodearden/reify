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
    pub return_type: Type,
    pub body: CompiledFnBody,
    pub content_hash: ContentHash,
}

/// A compiled function body: let bindings followed by a result expression.
#[derive(Debug, Clone)]
pub struct CompiledFnBody {
    pub let_bindings: Vec<(String, CompiledExpr)>,
    pub result_expr: CompiledExpr,
}

impl CompiledExpr {
    /// Create a literal expression.
    pub fn literal(value: Value, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[0]).combine(value.content_hash());
        CompiledExpr {
            kind: CompiledExprKind::Literal(value),
            result_type,
            content_hash,
        }
    }

    /// Create a value reference expression.
    pub fn value_ref(id: ValueCellId, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[1]).combine(ContentHash::of_str(&format!("{}", id)));
        CompiledExpr {
            kind: CompiledExprKind::ValueRef(id),
            result_type,
            content_hash,
        }
    }

    /// Create a binary operation expression.
    pub fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[2, op as u8])
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
            CompiledExprKind::ValueRef(_) => {}
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
        }
    }

    /// Create a unary operation expression.
    pub fn unop(op: UnOp, operand: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[3, op as u8]).combine(operand.content_hash);
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
            CompiledExprKind::ValueRef(id) => refs.push(id.clone()),
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
        let mut content_hash = ContentHash::of(&[7]).combine(body.content_hash);
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
        let mut content_hash = ContentHash::of(&[8]);
        for elem in &elements {
            content_hash = content_hash.combine(elem.content_hash);
        }
        CompiledExpr {
            kind: CompiledExprKind::ListLiteral(elements),
            result_type,
            content_hash,
        }
    }

    /// Create a set literal expression.
    pub fn set_literal(elements: Vec<CompiledExpr>, result_type: Type) -> Self {
        let mut content_hash = ContentHash::of(&[9]);
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
        let mut content_hash = ContentHash::of(&[10]);
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
        let content_hash = ContentHash::of(&[11])
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
        let content_hash = ContentHash::of(&[13, kind_byte])
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
        let content_hash = ContentHash::of(&[14]).combine(inner.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::OptionSome(Box::new(inner)),
            result_type,
            content_hash,
        }
    }

    /// Create an `option_none` expression.
    /// Note: result_type should be Type::Option(Box::new(inner_type)).
    pub fn option_none(result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[15]);
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
    pub fn remap_entity(&mut self, from_entity: &str, to_entity: &str) {
        match &mut self.kind {
            CompiledExprKind::ValueRef(id) => {
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
        }
    }

    /// Create a method call expression.
    pub fn method_call(
        object: CompiledExpr,
        method: String,
        args: Vec<CompiledExpr>,
        result_type: Type,
    ) -> Self {
        let mut content_hash = ContentHash::of(&[12])
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
        let content_hash = ContentHash::of(&[16])
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
        let mut content_hash = ContentHash::of(&[18, lower_inclusive as u8, upper_inclusive as u8]);
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
        let content_hash =
            ContentHash::of(&[17, kind_byte]).combine(ContentHash::of_str(&format!("{}", cell)));
        CompiledExpr {
            kind: CompiledExprKind::DeterminacyPredicate { kind, cell },
            result_type: Type::Bool,
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
        let hash = ContentHash::of(&[5])
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
}
