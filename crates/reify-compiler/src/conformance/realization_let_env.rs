//! Resolves the geometry traits of `let`-bound and sibling-realization operands
//! for the geometry-trait leaf check.

use crate::geometry_traits_inference::{
    InferredTraits, LetBindingEnv, SubRefEnv, infer_traits_for_expr_in_env,
    infer_traits_for_op_in_env,
};
use crate::{TopologyTemplate, ValueCellKind};
use reify_core::ValueCellId;
use std::cell::RefCell;
use std::collections::HashMap;

/// RAII guard that pops the top entry from an in-flight Vec when dropped.
///
/// Ensures the `push`–`pop` invariant around the two recursive calls in
/// [`RealizationLetEnv::resolve_uncached`] — `infer_traits_for_op_in_env` on
/// the realization arm and `infer_traits_for_expr_in_env` on the value_cells
/// fallback — is upheld even if the inner call panics.
/// Used only within `RealizationLetEnv::lookup`.
struct InFlightGuard<'a> {
    stack: &'a RefCell<Vec<ValueCellId>>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.stack.borrow_mut().pop();
    }
}

/// Maximum sibling-let chase depth in [`RealizationLetEnv::lookup`] before the
/// env declines and safe-defaults.
///
/// A cycle guard bounds REVISITS, not chain LENGTH: `let t1 = translate(t0, ..)`
/// / `let t2 = translate(t1, ..)` / … is acyclic and recurses once per hop, so
/// without a depth cap an N-long chain costs N nested `lookup` frames. The
/// compiler's own [`crate::recursion_guard`] cap does not cover this: it bounds
/// `compile_expr` / `compile_geometry_call`, and each let in such a chain is
/// depth-1 for both.
///
/// A stack overflow aborts the process rather than unwinding, so the chain must
/// be bounded by a counter; it cannot be caught.
const MAX_SIBLING_CHASE_DEPTH: usize = 256;

/// A `LetBindingEnv` that resolves geometry-typed `let` bindings by looking up
/// their compiled op-arrays in the parent template's `realizations` list.
///
/// For non-geometry lets (`ValueCellKind::Let` with a `default_expr`), falls
/// back to `infer_traits_for_expr_in_env` on the resolved `CompiledExpr`,
/// threading `self` through so chained `ValueRef` lets are resolved recursively.
/// For any id that doesn't match a known let or realization, returns `None`
/// (safe-default: the caller falls back to `InferredTraits::all()`).
///
/// # Geometry-let lookup
///
/// `compile_entity` Pass 2 explicitly `continue`s past geometry-typed
/// `MemberDecl::Let`, so geometry lets never enter `value_cells`. They are
/// lowered in Pass 3 to `Vec<CompiledGeometryOp>` stored on
/// `RealizationDecl.operations` with `RealizationDecl.name = Some(let_name)`.
/// `lookup` therefore scans `realizations` first (geometry path) and falls
/// back to `value_cells` (non-geometry path).
///
/// # Chained let
///
/// `compile_geometry_call` already handles geometry-let chaining at the AST
/// level: for `let g = h; let h = union(box, box)`, the realization for `g`
/// recursively compiles `h`'s init expression and emits the SAME op array
/// under `realization.name = Some("g")`. No chained-ValueRef walk is needed
/// here.
///
/// # Sibling-realization lookup
///
/// A bare geometry operand naming a sibling let lowers to `GeomRef::Sub(name)`
/// rather than an inlined `GeomRef::Step`, so the realization arm resolves its
/// op-array through [`SiblingSubEnv`] — otherwise an unbounded producer reached
/// via a `let` would silently satisfy a `Bounded` slot. Compound keys
/// (`Sub("<sub>.<member>")`, emitted for cross-sub `self.<sub>.<member>`
/// references) are deliberately NOT chased: their referent lives on a different
/// template, so they keep the `InferredTraits::all()` safe default.
///
/// A Solid param's geometry default is lowered into a realization too, so it is
/// chased like a let and treated as authoritative, as the un-indirected
/// `Foo(g: p)` form already treats it.
///
/// # Cycle and depth guards
///
/// BOTH arms recurse back into `lookup`: the realization arm through
/// `SiblingSubEnv::lookup_sub`, the value_cells fallback through
/// `infer_traits_for_expr_in_env(expr, self)`. Either can cycle — `let a =
/// translate(b, ..)` / `let b = translate(a, ..)` is accepted source that forms
/// a genuine `Sub` 2-cycle, and malformed IR can produce `let g = ValueRef(g)`.
/// The `in_flight` visited set is therefore checked ONCE, above both arms, and
/// held across both; re-entry returns `None`, which the caller's
/// `unwrap_or(InferredTraits::all())` turns into the safe default rather than a
/// stack overflow. The same Vec's LENGTH is additionally bounded by
/// [`MAX_SIBLING_CHASE_DEPTH`], which a cycle check cannot provide.
///
/// # Memoisation
///
/// Each id is resolved at most once per env, and an env lives for exactly one
/// geometry-trait leaf check. That keeps chasing linear when a let names a
/// sibling more than once: `union(translate(x, ..), translate(x, ..))` reaches
/// `x` twice per hop, so an unmemoised chase is exponential in chain length. A
/// result computed while a cycle or depth cut fired lower in the stack is
/// memoised too, because declining it re-opens the exponential under the cut;
/// only the guards' own `None`s are not. The price is a possible false negative
/// on cyclic or past-the-cap inputs: a node first reached under a cut keeps the
/// cut's `all()` for the rest of the leaf check, which can hide an unbounded
/// producer that another path through that node would have reached.
pub(super) struct RealizationLetEnv<'a> {
    templates: &'a HashMap<String, &'a TopologyTemplate>,
    /// In-flight visited set used to terminate cycles on both lookup paths
    /// (sibling realizations and the value_cells fallback).
    ///
    /// `RefCell` is required because `LetBindingEnv::lookup` takes `&self`;
    /// changing the trait to `&mut self` would cascade to all other impls.
    /// A `Vec` because its length doubles as the chase depth that
    /// [`MAX_SIBLING_CHASE_DEPTH`] bounds.
    in_flight: RefCell<Vec<ValueCellId>>,
    /// Every completed resolution, keyed by id; see `# Memoisation`.
    memo: RefCell<HashMap<ValueCellId, Option<InferredTraits>>>,
}

impl<'a> RealizationLetEnv<'a> {
    pub(super) fn new(templates: &'a HashMap<String, &'a TopologyTemplate>) -> Self {
        Self {
            templates,
            in_flight: RefCell::new(Vec::new()),
            memo: RefCell::new(HashMap::new()),
        }
    }

    /// Resolve `id` from its template, with no memo and no guards; `lookup`
    /// supplies both around this call.
    fn resolve_uncached(&self, id: &ValueCellId) -> Option<InferredTraits> {
        let template = self.templates.get(id.entity.as_str())?;

        // Geometry-let path: realization with matching name. Its op-array may
        // carry `GeomRef::Sub(sibling)` operands, resolved through the env.
        if let Some(real) = template
            .realizations
            .iter()
            .find(|r| r.name.as_deref() == Some(id.member.as_str()))
        {
            return Some(infer_traits_for_op_in_env(
                &real.operations,
                &SiblingSubEnv {
                    parent: self,
                    entity: id.entity.as_str(),
                },
            ));
        }

        // Non-geometry-let fallback: value_cells Let with a resolved default_expr.
        // Match on both entity and member for symmetry with the realization arm
        // above (entity is already narrowed by `templates.get(id.entity.as_str())`,
        // but the explicit check makes the intent self-documenting and adds a
        // cheap guard against any future refactor that reuses cells across templates).
        if let Some(cell) = template.value_cells.iter().find(|vc| {
            vc.id.entity == id.entity
                && vc.id.member == id.member
                && matches!(vc.kind, ValueCellKind::Let)
                && vc.default_expr.is_some()
        }) {
            // Thread the env through the resolved CompiledExpr so that any
            // ValueRef nested inside a non-geometry let (e.g. a chained let
            // whose default_expr references a geometry binding) is resolved via
            // the env rather than silently safe-defaulting to all(). The
            // in-flight guard in `lookup` terminates any cycle in well-formed or
            // malformed IR before a stack overflow can occur.
            let expr = cell.default_expr.as_ref()?;
            let result = infer_traits_for_expr_in_env(expr, self);
            return Some(result);
        }

        None
    }
}

impl LetBindingEnv for RealizationLetEnv<'_> {
    fn lookup(&self, id: &ValueCellId) -> Option<InferredTraits> {
        let memoised = self.memo.borrow().get(id).copied();
        if let Some(resolved) = memoised {
            return resolved;
        }

        // Cycle guard: if `id` is already being resolved on the current call
        // stack, return None. Both arms of `resolve_uncached` recurse back into
        // `lookup`, so the check and the RAII guard sit above both. The caller's
        // `unwrap_or(InferredTraits::all())` converts None into the safe
        // default, so a cyclic sibling-let pair or forward-referencing IR
        // yields `all()` rather than a stack overflow.
        //
        // Depth guard: `in_flight.len()` IS the current chase depth, so the
        // same Vec bounds an acyclic-but-long chain, which the cycle check
        // above cannot see. Same `None` → `all()` collapse.
        if self.in_flight.borrow().len() >= MAX_SIBLING_CHASE_DEPTH {
            return None;
        }
        if self.in_flight.borrow().iter().any(|p| p == id) {
            return None;
        }
        self.in_flight.borrow_mut().push(id.clone());
        let _guard = InFlightGuard {
            stack: &self.in_flight,
        };

        let resolved = self.resolve_uncached(id);
        self.memo.borrow_mut().insert(id.clone(), resolved);
        resolved
    }
}

/// A [`SubRefEnv`] that resolves a `GeomRef::Sub(name)` operand to the traits
/// of the same-template sibling realization or let named `name`.
///
/// `entity` is the template whose realization is being inferred, so a bare
/// sibling name is addressable as `ValueCellId { entity, member: name }` — the
/// very key [`RealizationLetEnv::lookup`] already takes. Delegating back to it
/// reuses the geometry-let arm, the `value_cells` fallback and the cycle guard
/// in one place instead of forking a second resolution path.
///
/// Compound keys are declined. `GeomRef::Sub` is overloaded: besides bare
/// sibling names it also carries `"<sub>.<member>"` for cross-sub
/// `self.<sub>.<member>` references, whose referent is a realization on a
/// DIFFERENT template that this env cannot address. A bare identifier can
/// never contain a dot, so `contains('.')` is an exact discriminator.
struct SiblingSubEnv<'a, 'b> {
    parent: &'a RealizationLetEnv<'b>,
    entity: &'a str,
}

impl SubRefEnv for SiblingSubEnv<'_, '_> {
    fn lookup_sub(&self, name: &str) -> Option<InferredTraits> {
        if name.contains('.') {
            return None;
        }
        self.parent.lookup(&ValueCellId::new(self.entity, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityKind, RealizationDecl, ValueCellDecl, Visibility};
    use reify_core::{ContentHash, SourceSpan, Type};
    use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction};
    use std::collections::HashSet;

    /// Build a `TopologyTemplate` holding only `cells` and `realizations`; every
    /// other field is empty, since `RealizationLetEnv` reads nothing else.
    fn minimal_template(
        name: &str,
        cells: Vec<ValueCellDecl>,
        realizations: Vec<RealizationDecl>,
    ) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: cells,
            constraints: vec![],
            realizations,
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// Build a `Let`-kind `ValueCellDecl` whose `default_expr` is a `ValueRef`
    /// pointing at `ref_id`.  Used to construct cycle fixtures in the tests below.
    fn let_cell_with_ref(id: ValueCellId, ref_id: ValueCellId) -> ValueCellDecl {
        ValueCellDecl {
            id,
            kind: ValueCellKind::Let,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: Some(CompiledExpr::value_ref(
                ref_id,
                Type::dimensionless_scalar(),
            )),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        }
    }

    /// Cycle guard: a self-referential `let` binding (`let g : Real = ValueRef(g)`)
    /// must not overflow the stack; the in-flight visited set terminates the cycle
    /// and `lookup` returns the safe-default `Some(InferredTraits::all())`.
    ///
    /// # Why the value_cells path is exercised
    ///
    /// Entity "E" has no geometry realization for "g" (no realization in the
    /// template), so the realization arm is skipped and `lookup` falls through to
    /// the value_cells arm — the arm that contains the recursive call to
    /// `infer_traits_for_expr_in_env(expr, self)`.
    ///
    /// # Why `Type::dimensionless_scalar()`
    ///
    /// A non-geometry type confirms the fixture bypasses the realization arm
    /// and only exercises the value_cells fallback where the recursion lives.
    ///
    /// # Assertion rationale
    ///
    /// `Some(InferredTraits::all())` is the safe-default propagation chain:
    /// inner `lookup` hits the in-flight guard → returns `None` →
    /// `infer_traits_for_expr_in_env(...).unwrap_or(all())` returns `all()` →
    /// outer `lookup` wraps in `Some`.
    #[test]
    fn lookup_self_referential_let_returns_safe_default_without_overflow() {
        // let g : Real = ValueRef(g)  — direct self-reference
        let g_id = ValueCellId::new("E", "g");
        let template = minimal_template(
            "E",
            vec![let_cell_with_ref(g_id.clone(), g_id.clone())],
            vec![],
        );

        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv::new(&templates);

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::all()),
            "self-referential let must safe-default to all() rather than stack-overflowing"
        );
    }

    /// Cycle guard: a chained-cycle let (`let g = ValueRef(h); let h = ValueRef(g)`)
    /// must not overflow the stack; the in-flight visited set terminates the cycle
    /// at re-entry and `lookup` returns the safe-default `Some(InferredTraits::all())`.
    ///
    /// Pins that the guard handles cycles longer than direct self-reference.
    /// The assertion shape is identical to the self-referential test; both produce
    /// `Some(InferredTraits::all())`.
    #[test]
    fn lookup_chained_cycle_let_returns_safe_default_without_overflow() {
        // let g : Real = ValueRef(h)
        // let h : Real = ValueRef(g)
        let g_id = ValueCellId::new("E", "g");
        let h_id = ValueCellId::new("E", "h");
        let template = minimal_template(
            "E",
            vec![
                let_cell_with_ref(g_id.clone(), h_id.clone()),
                let_cell_with_ref(h_id.clone(), g_id.clone()),
            ],
            vec![],
        );

        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv::new(&templates);

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::all()),
            "chained-cycle let must safe-default to all() rather than stack-overflowing"
        );
    }

    /// Non-cyclic chain: `let g = ValueRef(h); let h = difference(box(), box())`.
    ///
    /// Asserts that the in-flight guard does **not** short-circuit a non-cyclic
    /// chain.  `lookup(h)` must resolve to `bounded_only()` — the inferred traits
    /// of `difference` applied to two bounded primitives — and `lookup(g)` must
    /// propagate that result rather than safe-defaulting to `all()`.
    ///
    /// This test distinguishes a correct guard (fires only when `id` is already
    /// in the in-flight set) from a guard that fires too eagerly — for example,
    /// one that returns `None` whenever `in_flight` is non-empty regardless of
    /// which id is being resolved.  Without this test, both cycle tests pass even
    /// with such a premature-firing guard because they assert `Some(all())`.
    ///
    /// # Fixture rationale
    ///
    /// `h`'s `default_expr` is `FunctionCall("difference", [box(), box()])` with
    /// `result_type: Type::Geometry`.  `infer_traits_for_expr_in_env` dispatches
    /// on `expr.kind` (not `cell_type`), so the `FunctionCall` arm fires and
    /// returns `combine_difference(all(), all()) = bounded_only()`.
    #[test]
    fn lookup_chained_noncyclic_let_propagates_terminal_traits() {
        // box_expr: FunctionCall("box", [], Geometry) → InferredTraits::all()
        let box_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "box".to_string(),
                    qualified_name: "std::box".to_string(),
                },
                args: vec![],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash(0),
        };

        // diff_expr: FunctionCall("difference", [box, box]) → bounded_only()
        let diff_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "difference".to_string(),
                    qualified_name: "std::difference".to_string(),
                },
                args: vec![box_expr.clone(), box_expr],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash(0),
        };

        let g_id = ValueCellId::new("E", "g");
        let h_id = ValueCellId::new("E", "h");

        // g: Let Real = ValueRef(h)  — bridges g into the value_cells fallback
        let g_cell = let_cell_with_ref(g_id.clone(), h_id.clone());
        // h: Let Real = difference(box(), box())  — terminal, resolves to bounded_only()
        let h_cell = ValueCellDecl {
            id: h_id,
            kind: ValueCellKind::Let,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: Some(diff_expr),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };

        let template = minimal_template("E", vec![g_cell, h_cell], vec![]);
        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv::new(&templates);

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::bounded_only()),
            "non-cyclic chain g→h→difference(box,box) must propagate bounded_only(), \
             not short-circuit to all() via premature cycle-guard firing"
        );
    }
}
