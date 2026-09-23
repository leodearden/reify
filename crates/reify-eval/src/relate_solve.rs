//! Per-scope relate-solve — geometric-relations ζ (task 4386).
//!
//! Given a compiled structure scope (a [`TopologyTemplate`]) carrying, per ζ
//! step-2's threading, (i) a flat source-ordered geometric relation set
//! ([`TopologyTemplate::relations`]) and (ii) a per-`at auto`-sub auto-pose spec
//! ([`SubComponentDecl::auto_pose`]), the relate-solve must:
//!
//!   1. **collect** the scope into the solve's three inputs — the auto Frame
//!      unknowns, the flat relation list, and the grounded anchor set (this
//!      module's [`collect_relate_scope`], ζ step-4);
//!   2. **realize** each relation operand's local datum single-shot (step-6);
//!   3. **partition** the relations into a driving set + redundant remainder
//!      (reify-constraints, step-8);
//!   4. **solve** the driving set for the auto Frame (reify-constraints, step-10);
//!   5. **verify** the remainder as geometry-backed assertions (step-14); and
//!   6. **place** the solved Frame back as the auto sub's pose (step-18).
//!
//! This file lands stages 1 (collection, [`collect_relate_scope`]) and 2
//! (operand datum realization, [`realize_operand_datums`]). The collection logic
//! reads structurally off the compiled template — no geometry kernel is involved;
//! realization needs a real kernel (it builds each referenced sub's structure to
//! project its local datums). Later ζ steps extend it.

use std::collections::{HashMap, HashSet};

use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_constraints::relate_solve::{
    FrameUnknown, Operand, Pose, RelateTolerance, RelationInstance, comparable_datum_operands,
    max_relation_residual, ResidualRow, ResidualUnit, partition_driving_set, pose_from_frame,
    solve_frame, static_relation_residuals,
};
use reify_core::{Diagnostic, DiagnosticCode, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, ExportFormat, SolveResult, Value, ValueMap};

use crate::Engine;

/// One `at auto` Frame unknown in a relate-solve scope.
///
/// Each `at auto` sub contributes a single 6-DOF Frame unknown the relate-solve
/// must determine. This carries the data the solve needs from ζ step-2's threaded
/// [`AutoPoseSpec`](reify_compiler::AutoPoseSpec): the sub's id, the `free` flag,
/// and the ordered seed / component-fix params.
#[derive(Debug, Clone)]
pub struct AutoUnknown {
    /// The sub-component's instance name (id) — e.g. `"bolt"`.
    pub sub: String,
    /// `false` for bare `at auto` (strict — a residual non-gauge DOF surfaces the
    /// solver's `unique:false` under-determined signal, which ζ COMPUTES and carries
    /// as the residual DOF count in [`RelateSolution::free`]; its `W_UNDERDETERMINED`
    /// ledger is rendered by θ #4388, NOT failed by ζ — the residual DOF is still
    /// gauge-seeded so the build succeeds). `true` for `at auto(free)` (a residual
    /// DOF is gauge-seeded with the uniqueness check waived).
    pub free: bool,
    /// Ordered `name = value` seed / component-fix params from an
    /// `auto(seed = …)` / `auto(x = …)` form. Empty for bare `auto` / `auto(free)`.
    pub seed_params: Vec<(String, CompiledExpr)>,
}

/// The three inputs to a per-scope relate-solve, collected off the compiled
/// scope template (ζ step-4).
///
/// * [`auto_unknowns`](Self::auto_unknowns) — the `at auto` Frame unknowns, in
///   sub-declaration order;
/// * [`relations`](Self::relations) — the flat, source-ordered relate-block
///   members in declaration order: normally direct geometric-relation
///   `FunctionCall`s, but any `Type::Relation`-typed expr the compiler
///   accepted (e.g. a call to a user `fn ... -> Relation` wrapper) can
///   appear — see [`ScopeInstances`] for how the solve handles a member it
///   cannot consume; and
/// * [`ground`](Self::ground) — the names of the non-auto subs that serve as the
///   fixed anchor (ζ's grounding model: a non-auto sub is fixed at identity, and
///   the auto subs trace to it via the relations).
#[derive(Debug, Clone)]
pub struct RelateScope {
    /// The `at auto` Frame unknowns the solve must determine.
    pub auto_unknowns: Vec<AutoUnknown>,
    /// The flat, source-ordered per-scope geometric relation set.
    pub relations: Vec<CompiledExpr>,
    /// The grounded anchor subs (non-auto), by instance name.
    pub ground: Vec<String>,
    /// The subs declared with a CONCRETE `at <pose>`, by instance name, in
    /// sub-declaration order (DIC α, task 5415).
    ///
    /// An ADDITIONAL classification, not a partition of [`ground`](Self::ground):
    /// a concretely-posed sub is genuinely a fixed non-auto anchor and appears in
    /// BOTH lists. An `at auto` sub never appears here —
    /// [`SubComponentDecl`](reify_compiler::SubComponentDecl) guarantees
    /// structurally that `auto_pose.is_some()` implies `pose.is_none()`.
    ///
    /// # Why this has to be known
    ///
    /// [`resolve_operands`] keys realized datums by `(structure, member)` — each
    /// structure's LOCAL datum in its OWN identity frame — and a declared
    /// `SubComponentDecl.pose` is never composed into them. So a verdict computed
    /// over a posed sub's datums would be evaluated at the WRONG configuration,
    /// and would be confidently wrong in EITHER direction: it could report a
    /// correctly-placed assembly as violated, or a misplaced one as satisfied.
    ///
    /// The zero-auto static-verification arm therefore uses this list to classify
    /// such a scope's relations UNVERIFIABLE and say why, rather than emit a
    /// confident wrong answer (`docs/prds/v0_6/declared-intent-consumption-\
    /// accounting.md` §10 open question 5; honest non-consumption over a false
    /// verdict, `docs/legibility/design-invariants.md` INV-SF-3). Composing
    /// declared poses into the operand frames — which would make these scopes
    /// genuinely verifiable — is a clean follow-up seam, deliberately not taken
    /// here.
    ///
    /// The auto-FUL path ignores this field entirely: [`solve_relate_scope`]
    /// never reads it, so the solve is byte-identical with and without it.
    pub posed: Vec<String>,
}

/// Collect a compiled scope [`TopologyTemplate`] into the relate-solve's three
/// inputs (ζ step-4).
///
/// Walks the template's sub-components once, classifying each into an
/// [`AutoUnknown`] (when it carries an `auto_pose` spec, i.e. it was declared
/// `at auto` / `at auto(…)`) or into the [`ground`](RelateScope::ground) set
/// (every other sub), and carries the already-threaded relation set through
/// verbatim in source order. Sub-declaration order is preserved for both
/// `auto_unknowns` and `ground`; relation source order (which encodes "newest
/// member" for ζ's conflict attribution) is preserved by
/// [`TopologyTemplate::relations`] itself.
///
/// The same single walk also records the subs carrying a CONCRETE `at <pose>`
/// into [`posed`](RelateScope::posed) — an additional classification layered over
/// `ground`, not a partition of it; see that field for why the zero-auto arm
/// needs it.
///
/// No solve is performed here — this is pure structural classification.
pub fn collect_relate_scope(template: &TopologyTemplate) -> RelateScope {
    let mut auto_unknowns = Vec::new();
    let mut ground = Vec::new();
    let mut posed = Vec::new();

    for sub in &template.sub_components {
        match &sub.auto_pose {
            Some(spec) => auto_unknowns.push(AutoUnknown {
                sub: sub.name.clone(),
                free: spec.free,
                seed_params: spec.params.clone(),
            }),
            None => ground.push(sub.name.clone()),
        }
        // Layered over the auto/ground split rather than folded into it: a
        // concretely-posed sub is a ground anchor AND posed. `auto_pose.is_some()`
        // implies `pose.is_none()`, so this can never fire for an auto sub.
        if sub.pose.is_some() {
            posed.push(sub.name.clone());
        }
    }

    RelateScope {
        auto_unknowns,
        relations: template.relations.clone(),
        ground,
        posed,
    }
}

/// The realized LOCAL datum `Value`s for a relate-solve scope's relation operands
/// (ζ step-6).
///
/// Keyed by `(sub-instance name, datum member name)` — e.g. `("bolt",
/// "shank_axis")` → a `Value::Axis`. Each entry is the operand's datum realized in
/// its sub's OWN frame (single-shot, pose-independent): the assembly placement is
/// applied later (step-18, `ApplyTransform`), never baked into these local datums.
#[derive(Debug, Clone, Default)]
pub struct RealizedDatums {
    operands: HashMap<(String, String), Value>,
}

impl RealizedDatums {
    /// The realized local datum `Value` for `sub.member`, if that operand was
    /// realized (`None` if no relation references it).
    pub fn get(&self, sub: &str, member: &str) -> Option<&Value> {
        self.operands.get(&(sub.to_string(), member.to_string()))
    }

    /// The number of distinct `(sub, member)` operands realized.
    pub fn len(&self) -> usize {
        self.operands.len()
    }

    /// `true` when no operand was realized.
    pub fn is_empty(&self) -> bool {
        self.operands.is_empty()
    }

    /// Iterate the realized `(sub, member) → datum` entries.
    pub fn iter(&self) -> impl Iterator<Item = (&(String, String), &Value)> {
        self.operands.iter()
    }
}

/// A relation operand reference — the structurally-decoded `<sub>.<member>` datum
/// access an operand expr denotes.
///
/// A compiled relation operand such as `bolt.shank_axis` lowers to
/// `IndexAccess { object: ValueRef(<scope>.<sub>) : StructureRef(<Struct>),
/// index: Literal(String(<member>)) }` (the cross-sub datum-access shape). This is
/// the decoded `(sub = "bolt", structure = "Bolt", member = "shank_axis")`.
#[derive(Clone)]
struct OperandRef {
    /// The sub-instance name (e.g. `"bolt"`).
    sub: String,
    /// The sub's structure type name (e.g. `"Bolt"`) — the template to realize.
    structure: String,
    /// The datum member projected off the sub (e.g. `"shank_axis"`).
    member: String,
}

/// Decode a relation operand expr into its `(sub, structure, member)` datum
/// reference, or `None` if the operand is not a `<sub>.<member>` datum access.
///
/// Matches the compiled cross-sub datum-access shape (verified against the §1
/// compiled relations): an [`IndexAccess`](CompiledExprKind::IndexAccess) whose
/// `object` is a sub-instance ref typed [`Type::StructureRef`] and whose `index`
/// is a [`Value::String`] member literal. The structure name is read from the
/// object's `result_type` (robust to how the sub is referenced); the sub name is
/// the last `.`-segment of the object ref's value-cell entity
/// (`"BoltPlate.bolt"` → `"bolt"`).
fn decode_operand(expr: &CompiledExpr) -> Option<OperandRef> {
    let CompiledExprKind::IndexAccess { object, index } = &expr.kind else {
        return None;
    };

    let member = match &index.kind {
        CompiledExprKind::Literal(Value::String(s)) => s.clone(),
        _ => return None,
    };

    let structure = match &object.result_type {
        Type::StructureRef(name) => name.clone(),
        _ => return None,
    };

    let sub = match &object.kind {
        CompiledExprKind::ValueRef(cell) | CompiledExprKind::CrossSubGeometryRef(cell) => {
            cell.entity.rsplit('.').next().unwrap_or(&cell.entity).to_string()
        }
        _ => return None,
    };

    Some(OperandRef {
        sub,
        structure,
        member,
    })
}

/// The transitive closure of structure names reachable from `root` via
/// sub-component edges (`root` ∪ every structure it instantiates as a sub,
/// recursively).
///
/// Realizing a structure standalone requires keeping any sub-structures it
/// instantiates so the build resolves. For the §1 leaf structures (`Bolt` /
/// `Plate` have only `let` datums, no subs) the closure is the singleton `{root}`.
fn structure_closure(root: &str, module: &CompiledModule) -> HashSet<String> {
    let mut keep = HashSet::new();
    let mut stack = vec![root.to_string()];
    while let Some(name) = stack.pop() {
        if !keep.insert(name.clone()) {
            continue;
        }
        if let Some(t) = module.templates.iter().find(|t| t.name == name) {
            for sub in &t.sub_components {
                if !keep.contains(&sub.structure_name) {
                    stack.push(sub.structure_name.clone());
                }
            }
        }
    }
    keep
}

/// Realize each relation operand's LOCAL datum `Value`, single-shot and
/// pose-independent (ζ step-6).
///
/// For every relation in `scope`, decodes each operand into its `<sub>.<member>`
/// datum reference ([`decode_operand`]) and realizes the referenced sub's
/// structure **once** — building it standalone (in its own identity frame) through
/// the `engine`'s geometry kernel, then projecting the operand's datum member off
/// the realized structure (the ε feature→datum bridge + β datum projections run
/// during that build, so `shank.axis` becomes a concrete `Value::Axis`). Each
/// referenced structure is built at most once per call (single-shot); multiple
/// subs of the same structure share its local datums.
///
/// **Pose independence.** `seeds` carries the relate-solve's CURRENT Frame
/// estimate for each `at auto` unknown (the assembly pose the sub would be placed
/// at). Local datums are realized in each sub's OWN frame and are therefore
/// invariant to `seeds`: the parameter is deliberately not consumed here — the
/// assembly pose is applied later by the placement step (ζ step-18,
/// `ApplyTransform`), never baked into these local datums. This single-shot
/// pose-independence is the guarantee the step-5 test pins (two distinct seed
/// Frames ⇒ identical local datums); the parameter is kept to match the
/// relate-solve's call shape and make the contract explicit at the type level.
pub fn realize_operand_datums(
    scope: &RelateScope,
    module: &CompiledModule,
    engine: &mut Engine,
    seeds: &HashMap<String, Value>,
) -> RealizedDatums {
    // Local datum realization is pose-independent — see the doc-comment. The seed
    // estimate is consumed downstream (partition/solve/place), not here.
    let _ = seeds;

    // Decode this scope's operands, realize their structures in ONE filtered
    // sub-build, then resolve each operand to its realized local datum. (The
    // build-pass entry [`solve_scopes`] shares a SINGLE such build across every
    // relate scope; this single-scope path is the same two helpers over one scope.)
    let refs = scope_operand_refs(scope);
    if refs.is_empty() {
        return RealizedDatums::default();
    }
    let values = realize_structures(&refs, module, engine);
    resolve_operands(&refs, &values)
}

/// Decode every relation operand in `scope` into its `(sub, structure, member)`
/// datum reference ([`decode_operand`]); non-datum operands (scalar magnitudes) are
/// skipped. The shared front half of realization — used both per-scope
/// ([`realize_operand_datums`]) and once across all scopes ([`solve_scopes`]).
fn scope_operand_refs(scope: &RelateScope) -> Vec<OperandRef> {
    scope
        .relations
        .iter()
        .flat_map(|rel| match &rel.kind {
            CompiledExprKind::FunctionCall { args, .. } => {
                args.iter().filter_map(decode_operand).collect::<Vec<_>>()
            }
            _ => Vec::new(),
        })
        .collect()
}

/// Realize the union closure of every structure referenced by `refs` in ONE filtered
/// sub-build, returning its value map (single-shot, pose-independent — each structure
/// realizes in its own identity frame). Filtering to the operand structures' closure
/// (+ their sub-structure deps) keeps the build minimal; building the union ONCE lets
/// a caller realize many scopes without cloning + rebuilding the whole module per
/// scope. Empty `refs` ⇒ no build (the value map is empty).
fn realize_structures(refs: &[OperandRef], module: &CompiledModule, engine: &mut Engine) -> ValueMap {
    let mut keep = HashSet::new();
    for r in refs {
        keep.extend(structure_closure(&r.structure, module));
    }
    if keep.is_empty() {
        return ValueMap::default();
    }
    let mut sub_module = module.clone();
    sub_module.templates.retain(|t| keep.contains(&t.name));
    engine.build(&sub_module, ExportFormat::Step).values
}

/// Resolve each operand `ref` to its realized LOCAL datum from a built `values` map.
/// A structure's local datum cell is keyed `ValueCellId { entity: <Struct>, member:
/// <datum> }`; the result is re-keyed by `(sub-instance, member)` for the solve. An
/// operand whose structure datum is absent resolves to `Value::Undef`.
fn resolve_operands(refs: &[OperandRef], values: &ValueMap) -> RealizedDatums {
    let mut operands = HashMap::new();
    for r in refs {
        let datum = values
            .get(&ValueCellId::new(&r.structure, &r.member))
            .cloned()
            .unwrap_or(Value::Undef);
        operands.insert((r.sub.clone(), r.member.clone()), datum);
    }
    RealizedDatums { operands }
}

// ── Per-scope relate-solve orchestration (ζ steps 14/16/18) ──────────────────

/// The [`ValueCellId`] under which the relate-solve writes back an `at auto`
/// sub's solved assembly-pose `Frame` (ζ step-18).
///
/// The build pass writes each solved Frame here during scope resolution
/// ([`solve_scopes`] → the writeback in `build_with_geometry_output`); the
/// surfacing walk's `eval_sub_pose` auto arm reads it back to place the sub. Both
/// sides MUST construct the SAME key — this single constructor is that contract.
/// The entity is the sub's scope-qualified instance path (`"<scope>.<sub>"`); the
/// synthetic `__auto_pose` member is namespaced so it never collides with a
/// user-declared datum cell on the sub.
pub fn auto_pose_cell(scope: &str, sub: &str) -> reify_core::ValueCellId {
    reify_core::ValueCellId::new(format!("{scope}.{sub}"), "__auto_pose")
}

/// The [`ValueCellId`] of the joint `Value::Map` a relate scope mounts for `sub`
/// (geometric-joints ε, task #4399 — the DD1 operand-reference association rule).
///
/// Resolves the joint cell associated with the relate-scope mount for `sub` by
/// scanning the `scope` template's `value_cells` for a motion-joint constructor
/// (Revolute / Prismatic / Cylindrical / Planar / Spherical / Fixed) whose
/// `default_expr` FunctionCall args include an operand that `decode_operand`
/// decodes to an [`OperandRef`] with `sub == <target sub>`.
///
/// **First-match semantics**: returns the first matching cell in declaration
/// order.  If two joint cells reference the same mounted sub, only the first
/// receives the solved origin; others are skipped.  The single-joint-per-sub
/// authoring convention is assumed; a `tracing::warn!` is emitted when more
/// than one cell matches so authors can identify unintended ambiguous-mount
/// patterns.  See `tests_mounted_joint_cell::
/// mounted_joint_cell_first_match_returns_declaration_order_first` for the
/// test that pins this behavior.
///
/// Returns `None` when no cell matches — unknown scope / sub, or the joint
/// cell's args carry only literal operands (the B9 non-match invariant).
/// When `None` the engine_build seam writes no `"origin"` key, preserving
/// the byte-identical B9 back-compat invariant (KIN-OFFSET α absent-origin →
/// identity no-op).
///
/// [`reify_stdlib::set_mount_origin`]: reify_stdlib::set_mount_origin
pub fn mounted_joint_cell(
    scope: &str,
    sub: &str,
    module: &reify_compiler::CompiledModule,
) -> Option<reify_core::ValueCellId> {
    // Find the scope's TopologyTemplate.
    let template = module.templates.iter().find(|t| t.name == scope)?;

    // Collect ALL matching joint cells (Revolute/Prismatic/etc.) whose
    // default_expr FunctionCall has at least one arg that decode_operand decodes to
    // an OperandRef with `sub == <target sub>` — the DD1 joint↔mount association rule.
    //
    // Walking the full list (not short-circuiting at the first match) lets us emit
    // a diagnostic when more than one cell references the same sub.
    //
    // B9 back-compat: a literal-axis joint `revolute(vec3(0,0,1), range)` has no
    // IndexAccess arg, so decode_operand returns None for every arg → no match →
    // not included in `matches` → no origin written.
    let matches: Vec<_> = template
        .value_cells
        .iter()
        .filter_map(|cell| {
            if !is_motion_joint_cell_type(&cell.cell_type) {
                return None;
            }
            let args = match cell.default_expr.as_ref().map(|e| &e.kind) {
                Some(CompiledExprKind::FunctionCall { args, .. }) => args,
                _ => return None,
            };
            args.iter()
                .any(|arg| decode_operand(arg).is_some_and(|op| op.sub == sub))
                .then(|| cell.id.clone())
        })
        .collect();

    // Ambiguous-mount diagnostic: if more than one joint cell references the same
    // mounted sub, emit a warning so authoring errors surface rather than producing
    // a confusing geometric result (only the first cell receives the solved origin).
    let n = matches.len();
    if n > 1 {
        tracing::warn!(
            scope = %scope,
            sub = %sub,
            n,
            "relate-mount: ambiguous mount — {n} joint cells in {scope:?} all \
             reference mounted sub {sub:?}; only the first (in declaration order) \
             receives the solved origin — check for unintended multi-joint-per-sub authoring"
        );
    }

    matches.into_iter().next()
}

/// Returns `true` if `ty` is a motion joint [`Type::StructureRef`] — one of the
/// driving joint kinds (Revolute, Prismatic, Cylindrical, Planar, Spherical) or
/// Fixed — the cell types produced by joint-constructor builtins. These are the
/// only cells whose `default_expr` args can reference a sub datum and thus be
/// associated with a relate-scope mount (DD1).
///
/// **Coupling concern**: the six tags here mirror the driving-joint
/// `Type::StructureRef` results of `joint_ctor_result_type` in
/// `crates/reify-compiler/src/joint_signatures.rs` (5 driving constructors:
/// revolute / prismatic / cylindrical / planar / spherical, plus fixed).
/// If a new driving joint kind is added to `joint_ctor_result_type` there,
/// this match arm MUST also be updated — otherwise the new kind's relate-mount
/// origin would never be written (silent drift, no compile error).
/// **Coupling guard**: the test
/// `tests_mounted_joint_cell::is_motion_joint_cell_type_covers_all_expected_kinds`
/// hardcodes the same six tags (tautological against this function), but
/// `tests_mounted_joint_cell::mounted_joint_cell_matches_each_driving_joint_kind`
/// provides a behavioral supplement — it compiles a `.ri` fixture for each
/// driving joint kind and asserts `mounted_joint_cell` returns `Some(...)`,
/// so a kind missed in this list AND covered by that test WOULD produce a failure.
fn is_motion_joint_cell_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::StructureRef(name) if matches!(
            name.as_str(),
            "Revolute" | "Prismatic" | "Cylindrical" | "Planar" | "Spherical" | "Fixed"
        )
    )
}

/// The outcome of a per-scope relate-solve ([`solve_relate_scope`]).
///
/// Carries the solved assembly pose for each `at auto` sub plus the DOF accounting
/// and the post-solve verification diagnostics. The Resolution-node build pass
/// (ζ step-18) writes each [`poses`](Self::poses) entry back as the auto sub's pose
/// value (placement reuses the existing `eval_sub_pose`→`ApplyTransform` path) and
/// surfaces [`diagnostics`](Self::diagnostics) — an `Error` fails the build.
#[derive(Debug, Clone, Default)]
pub struct RelateSolution {
    /// The solved [`Value::Frame`] per `at auto` sub, keyed by sub-instance name
    /// (e.g. `"bolt"`). Empty when the scope has no auto subs, or when the driving
    /// set was infeasible (no placement — see [`diagnostics`](Self::diagnostics)).
    pub poses: HashMap<String, Value>,
    /// DOF spent by the driving set = its combined Jacobian rank (exact codimension).
    pub spent: u32,
    /// Residual DOF left free = `6 − spent` (the Frame freedoms the relations leave
    /// open, e.g. spin about a shared axis).
    pub free: u32,
    /// Number of relations in the driving set (the maximal independent subset that
    /// was handed to the solver).
    pub driving: usize,
    /// Number of relations in the redundant remainder (verified post-solve as
    /// geometry-backed assertions, never solved).
    pub redundant: usize,
    /// Verification diagnostics: a redundant-remainder relation violated at the
    /// solved placement (step-14), or a driving-set conflict (the solver's
    /// `Infeasible` report; step-16 refines it into a minimal conflict set). An
    /// `Error` here fails the build.
    pub diagnostics: Vec<Diagnostic>,
    /// The per-relation consumption ledger of a ZERO-AUTO scope's static
    /// verification (DIC α, task 5415), or `None` when this solution came from the
    /// auto-ful solve path.
    ///
    /// `None` vs `Some(StaticRelateFacts { verified: 0, .. })` is a real
    /// distinction and the reason this is an `Option`: the first means "static
    /// verification did not run here", the second "it ran and decided nothing".
    /// `Default` leaves it `None`, so every existing `..Default::default()` site
    /// and the whole auto-ful path are byte-identical (invariant V1).
    pub static_facts: Option<StaticRelateFacts>,
}

/// The consumption ledger for one zero-auto relate scope's static verification
/// (DIC α, task 5415) — how many of its declared relations were actually decided,
/// and how.
///
/// Every relation in the scope lands in exactly one bucket, so
/// `verified + violated + unverifiable` always equals the scope's relation count.
/// That total is the point: it is what lets ζ's declared-intent ledger (#5420)
/// report consumption without re-deriving it, and what makes a silently-skipped
/// relation impossible to hide — a relation that fell through every arm would show
/// up as a missing count rather than as nothing at all
/// (`docs/legibility/design-invariants.md` INV-SF-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StaticRelateFacts {
    /// Relations measured and found SATISFIED within the assertion tolerance.
    /// Reported silently — a held assertion raises no diagnostic.
    pub verified: usize,
    /// Relations measured and found VIOLATED beyond the assertion tolerance.
    /// Aggregated into one [`DiagnosticCode::RelateStaticViolated`] Error.
    pub violated: usize,
    /// Relations whose satisfaction could not be DECIDED — no residual model, an
    /// operand that did not realize, or an operand on a concretely-posed sub whose
    /// placement is not composed into the local datums. Aggregated into one
    /// [`DiagnosticCode::RelateStaticUnverifiable`] Warning. Never counted as
    /// verified: that is the false green this arm exists to kill.
    pub unverifiable: usize,
}

// ── Trace-to-ground / global float (η, B6) ───────────────────────────────────

/// Add an undirected edge `a — b` to the connectivity adjacency.
fn link(adj: &mut HashMap<String, Vec<String>>, a: &str, b: &str) {
    adj.entry(a.to_string()).or_default().push(b.to_string());
    adj.entry(b.to_string()).or_default().push(a.to_string());
}

/// The datum name of a `self.*` intrinsic-datum operand — the anchor reference a
/// `ground(sub)` desugar (`fasten(sub.frame, self.frame)`) carries — or `None` when
/// `expr` is not one.
///
/// A `self.<datum>` projection lowers to `MethodCall { object: ValueRef(__self :
/// StructureRef), method, [] }` (η step-8/10) — distinct from a `<sub>.<member>`
/// `IndexAccess` (which [`decode_operand`] owns). So a no-arg `MethodCall` on a
/// `StructureRef`-typed receiver is the `self` anchor operand. Mirrors the
/// self-datum discriminator the compiler's `classify` / eval's
/// `try_eval_self_datum_projection` use.
///
/// The member name is carried rather than a bare `bool` so the zero-auto verifier
/// can NAME the operand (`self.frame`) it could not compare against; the two
/// callers that only need presence read `.is_some()`.
fn self_anchor_member(expr: &CompiledExpr) -> Option<&str> {
    match &expr.kind {
        CompiledExprKind::MethodCall { object, method, args }
            if args.is_empty() && matches!(object.result_type, Type::StructureRef(_)) =>
        {
            Some(method)
        }
        _ => None,
    }
}

/// Trace each `at auto` sub in `scope` to the grounded anchor over the relation
/// operand graph; return the (sorted) names of the auto subs that DON'T reach it —
/// the floating subs that leave the assembly globally under-grounded (B6).
///
/// Kernel-free and purely structural: connectivity is a property of the compiled
/// relation operands, independent of realized geometry, so it fires at `reify build`
/// even without the OCCT kernel (unit-testable on collected scopes). The graph nodes
/// are the `at auto` subs ∪ a synthetic ground anchor; for each relation:
///  - every `<sub>.<member>` datum operand ([`decode_operand`]) contributes its sub,
///    and the relation unions all the auto subs it references together;
///  - a relation referencing a GROUND sub (a non-auto anchor) or any `self.*` datum
///    operand ([`self_anchor_member`]) unions its auto subs into the anchor.
///
/// An auto sub not in the anchor's connected component is floating.
pub fn trace_to_ground(scope: &RelateScope) -> Vec<String> {
    let auto: HashSet<&str> = scope.auto_unknowns.iter().map(|a| a.sub.as_str()).collect();
    let ground: HashSet<&str> = scope.ground.iter().map(|g| g.as_str()).collect();

    // Adjacency among auto-sub names + a synthetic anchor node (the empty string,
    // never a valid sub name). A BFS from the anchor reaches every grounded auto sub.
    const ANCHOR: &str = "";
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for rel in &scope.relations {
        let CompiledExprKind::FunctionCall { args, .. } = &rel.kind else {
            continue;
        };
        let mut rel_autos: Vec<String> = Vec::new();
        let mut touches_anchor = false;
        for arg in args {
            if let Some(opref) = decode_operand(arg) {
                if ground.contains(opref.sub.as_str()) {
                    touches_anchor = true;
                } else if auto.contains(opref.sub.as_str()) {
                    rel_autos.push(opref.sub);
                }
            } else if self_anchor_member(arg).is_some() {
                touches_anchor = true;
            }
        }
        // Chain the relation's auto subs together (one relation couples them).
        for pair in rel_autos.windows(2) {
            link(&mut adj, &pair[0], &pair[1]);
        }
        // A relation touching the anchor grounds every auto sub it references.
        if touches_anchor {
            for s in &rel_autos {
                link(&mut adj, ANCHOR, s);
            }
        }
    }

    // BFS from the anchor; auto subs not reached are floating.
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec![ANCHOR.to_string()];
    while let Some(n) = stack.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(neighbors) = adj.get(&n) {
            for nb in neighbors {
                if !seen.contains(nb) {
                    stack.push(nb.clone());
                }
            }
        }
    }

    let mut floating: Vec<String> = scope
        .auto_unknowns
        .iter()
        .map(|a| a.sub.clone())
        .filter(|s| !seen.contains(s))
        .collect();
    floating.sort();
    floating
}

/// The B6 global-float [`Diagnostic`] (code [`DiagnosticCode::AssemblyGlobalFloat`])
/// for the floating auto subs — they have no path to a grounded anchor or `self`, so
/// the assembly retains all 6 DOF in `self`. Names the floating subs and guides the
/// fix (ground a part).
fn global_float_diagnostic(floating: &[String]) -> Diagnostic {
    let subs = floating.join(", ");
    let example = floating.first().map(String::as_str).unwrap_or("part");
    Diagnostic::error(format!(
        "6 DOF — the assembly floats in `self`: ground a part. No relation grounds \
         {{{subs}}} to a fixed anchor or `self.frame` (e.g. add `ground({example})`)."
    ))
    .with_code(DiagnosticCode::AssemblyGlobalFloat)
}

/// The un-consumable-relate-member [`Diagnostic`] (code
/// [`DiagnosticCode::RelateExpectsRelation`], task 7050) for a `relate {}` member
/// that type-checks to `Type::Relation` — so the compiler's own
/// `check_relate_relations` accepted it — yet is not a direct geometric-relation
/// call the solve can build a [`RelationInstance`] from (e.g. an alias to a
/// `let`-bound Relation, an `if`/`match` yielding one, or a call to a user-defined
/// `fn ... -> Relation` wrapper). The code is shared with the compiler's own
/// relate-member type-mismatch diagnostic; a dedicated code is deferred to #7494.
/// `source` is the member's 0-based index into `scope.relations`; the message
/// names its 1-based DECLARATION position — the diagnostic's only localiser. It
/// does not also name a sub: which sub(s) a member touches can't generally be
/// read back out of a non-`FunctionCall` expr the way [`operand_refs`] reads a
/// driving relation's operand references.
///
/// Only call this for a `Type::Relation` member (see [`build_relation_instances`]):
/// a member the compiler already flagged as NOT `Type::Relation` gets an accurate
/// diagnostic there (`crates/reify-compiler/src/entity.rs`'s
/// `check_relate_relations`), and must not also draw this one — a second,
/// differently-worded `RelateExpectsRelation` diagnostic would contradict the first
/// (anti-cascade, mirroring `check_relate_relations`'s own `Type::Error` skip).
///
/// Replaces a SILENT drop with the INV-SF-3 diagnostic
/// (`docs/legibility/design-invariants.md:130`: a declaration is either consumed by
/// a solve/verify pass this run, or generates a diagnostic naming why not) — but
/// only for scopes that reach this function at all. A relate scope with no `at
/// auto` sub never calls it (`solve_scopes`'s qualifying filter, and this file's
/// own early return, both skip such a scope before any relation is inspected), so
/// the same authoring mistake there is still silently dropped; closing that gap
/// belongs to the compiler's `check_relate_relations`, which sees every relate
/// block regardless of auto subs (deferred to #7495).
fn unconsumable_relation_diagnostic(source: usize) -> Diagnostic {
    let position = source + 1;
    Diagnostic::error(format!(
        "relate-block member {position} is not a direct call to a geometric \
         relation, so the relate-solve cannot verify it: every `relate {{}}` \
         member must call a geometric relation directly — a name or expression \
         that merely evaluates to one, such as a `fn ... -> Relation` wrapper, is \
         not itself a relation call."
    ))
    .with_code(DiagnosticCode::RelateExpectsRelation)
}

/// Run the per-scope relate-solve over already-realized LOCAL datums (ζ steps
/// 14/16/18): rank-partition the relations into a driving set + a redundant
/// remainder, solve ONLY the driving set for the `at auto` Frame, then verify the
/// remainder post-solve as geometry-backed assertions.
///
/// `scope` is the collected scope ([`collect_relate_scope`]); `realized` is its
/// operand datums realized single-shot in each sub's own frame
/// ([`realize_operand_datums`]). This function is **pure** (kernel-free) given
/// `realized` — the OCCT-dependent realization is the caller's job — so it is unit-
/// testable without a geometry kernel and reused verbatim by the build pass.
///
/// ## Pipeline (PRD §7.1 steps 2/3/5)
///
/// 1. **Build** a [`RelationInstance`] per relation over the realized datums,
///    tagging each operand with the sub it belongs to (so the partition/solve know
///    which datums MOVE with the auto Frame vs which are fixed anchors).
/// 2. **Partition** at the seed witness into driving + redundant
///    ([`partition_driving_set`]); report `spent`/`free`.
/// 3. **Solve** the driving set for the auto Frame ([`solve_frame`]). On
///    [`SolveResult::Solved`] the solved Frame is recorded in `poses`; on
///    [`SolveResult::Infeasible`] the driving set is inconsistent — a
///    [`minimal_infeasible_subset`] is derived and rendered as a geometric
///    [`conflict_diagnostic`] (minimal conflict set + conflicting magnitudes +
///    newest-declared primary), never the solver's internal message (step-16).
/// 4. **Verify** each redundant-remainder relation against the SOLVED placement
///    within the assertion tolerance — satisfied ⇒ silent (B2). A *violated*
///    remainder relation that shares a datum operand with a driving relation pins
///    the same geometry to a different value: a genuine CONFLICT, rendered as the
///    same geometric [`conflict_diagnostic`] (B3). A violated remainder that shares
///    no driving operand is a lone assertion failure. This unified-DAG predicate
///    path — NOT a solver constraint — is what makes a *consistent* redundant
///    relation pass silently while an *inconsistent* one fails loud.
///
/// ## Grounding model (ζ scope)
///
/// ζ's named scope has exactly one `at auto` unknown traced to a grounded anchor
/// (a non-auto sub fixed at identity); `self`-anchor / construction-datum / global-
/// float grounding is η (#4387). This solves the single auto unknown against the
/// fixed anchors. A scope with no auto unknown returns an empty solution.
pub fn solve_relate_scope(scope: &RelateScope, realized: &RealizedDatums) -> RelateSolution {
    // ζ scope: exactly one `at auto` unknown. No auto sub ⇒ nothing to solve.
    let Some(auto) = scope.auto_unknowns.first() else {
        return RelateSolution::default();
    };

    // 1. Build a RelationInstance per relation over the realized datums, paired
    //    with the source relation each was built from (see `ScopeInstances`).
    //    Hoisted ahead of the B6 trace-to-ground short-circuit below so an
    //    un-consumable relate-block member is diagnosed on EVERY exit path — never
    //    masked by a floating assembly (task 7050) — even though the float path
    //    below then discards the built instances unused.
    let built = build_relation_instances(scope, realized);
    let instances = &built.instances;

    // A relate-block member the solve could not consume (not a geometric relation
    // call) is diagnosed here — never silently dropped (INV-SF-3, task 7050).
    let skip_diagnostics: Vec<Diagnostic> = built
        .skipped
        .iter()
        .copied()
        .map(unconsumable_relation_diagnostic)
        .collect();

    // η trace-to-ground (B6): any `at auto` sub with no path (over the relation
    // operand graph) to a grounded anchor or `self.*` makes the assembly globally
    // float in `self`. Emit the precise global-float error PRE-SOLVE and short-circuit
    // — the SolveSpace solve would otherwise return an opaque under-determined result
    // instead of "ground a part". Kernel-free: connectivity is purely structural. The
    // skip diagnostics above ride along on this exit path too (task 7050) — a
    // floating assembly must not mask an un-consumable relate-block member.
    let floating = trace_to_ground(scope);
    if !floating.is_empty() {
        let mut diagnostics = skip_diagnostics;
        diagnostics.push(global_float_diagnostic(&floating));
        return RelateSolution { diagnostics, ..RelateSolution::default() };
    }

    let frame_unknown = FrameUnknown {
        sub: auto.sub.clone(),
        free: auto.free,
    };

    // The single kernel-defaulted tolerance knob governs the whole hierarchy
    // (kernel_local ≤ solver_convergence ≤ assertion/dedup); PRD §7.1 coherence law.
    let tol = RelateTolerance::kernel_default();

    // The seed witness. ζ's e2e scopes (§1/B2/B3) carry no `auto(seed=…)` params, so
    // the seed is identity; evaluating `seed_params` into a non-identity seed Pose is
    // a refinement not exercised by ζ's named e2e set (B5's seed bias is covered at
    // the constraints layer with an explicit seed Pose). The grounded anchor's local
    // datums already encode the target, so identity is the correct witness here.
    let seed = Pose::identity();

    // 2. Partition at the witness into driving + redundant; the rank-revealing
    //    tolerance is tied to the solver-convergence tol (design §4).
    let partition =
        partition_driving_set(instances, &frame_unknown, &seed, tol.solver_convergence());

    let mut solution = RelateSolution {
        spent: partition.spent,
        free: partition.free,
        driving: partition.driving.len(),
        redundant: partition.redundant.len(),
        diagnostics: skip_diagnostics,
        ..RelateSolution::default()
    };

    // 3. Solve ONLY the driving set for the auto Frame.
    let driving_rels: Vec<RelationInstance> =
        partition.driving.iter().map(|&i| instances[i].clone()).collect();
    let result = solve_frame(&driving_rels, &frame_unknown, &seed, tol.solver_convergence());

    let solved_pose = match result {
        // The solver's `unique` flag is intentionally not consumed here. For strict
        // `at auto` a residual non-gauge DOF surfaces as `unique:false` — the
        // under-determined signal — but ζ does NOT fail the build on it: the §1 bolt
        // leaves a residual spin DOF about the shank axis and must still build (the
        // gauge DOF is seeded to a concrete value). ζ's contract is to PRODUCE the
        // data — the residual DOF count is already carried in `solution.free` — and
        // θ (#4388) renders the `W_UNDERDETERMINED` ledger from it.
        SolveResult::Solved { values, .. } => {
            // Record each solved Frame as the auto sub's pose (keyed by sub-instance
            // name); the build pass (step-18) writes these back for placement.
            for (cell, frame) in &values {
                solution.poses.insert(cell.entity.clone(), frame.clone());
            }
            solution
                .poses
                .get(&frame_unknown.sub)
                .and_then(pose_from_frame)
        }
        SolveResult::Infeasible { .. } => {
            // The driving set is geometrically inconsistent. Derive the MINIMAL
            // inconsistent subset (the smallest set of driving relations that is
            // still infeasible) and render a geometric conflict diagnostic — naming
            // the conflict set, its conflicting magnitudes, and the newest-declared
            // member as primary. The solver's own message is discarded: ζ speaks
            // geometry, never libslvs internals. No placement, so the remainder is
            // not verified.
            let conflict = minimal_infeasible_subset(
                instances,
                &partition.driving,
                &frame_unknown,
                &seed,
                tol.solver_convergence(),
            );
            solution.diagnostics.push(conflict_diagnostic(&conflict, &built, &frame_unknown.sub));
            None
        }
        SolveResult::NoProgress { reason } => {
            // `solve_frame` maps non-convergence to `Infeasible`, so this arm is
            // defensive. Emit the geometric "seed too far" guidance (the wrong-root /
            // under-determined `seed:` ledger is θ's surface, #4388).
            solution.diagnostics.push(Diagnostic::error(format!(
                "the relations on `{}` did not converge to a placement ({reason}); the \
                 seed configuration may be too far from a solution — try an \
                 `auto(seed = …)` nearer the intended placement",
                frame_unknown.sub
            )));
            None
        }
    };

    // 4. Verify each redundant-remainder relation against the SOLVED placement.
    if let Some(pose) = solved_pose {
        for &i in &partition.redundant {
            let rel = &instances[i];
            let resid =
                max_relation_residual(std::slice::from_ref(rel), &frame_unknown, &pose);
            if resid <= tol.assertion() {
                // Satisfied within the assertion tolerance ⇒ silent (a consistent
                // redundant relation, B2): an opt-in lint hook only, no diagnostic.
                continue;
            }

            // Violated. If it shares a datum operand with a DRIVING relation, the two
            // pin the SAME geometry to different values — a genuine CONFLICT (B3), not
            // a lone assertion. Render the minimal conflict set + geometric magnitudes
            // + newest-primary (step-16), the same diagnostic an infeasible driving set
            // produces. Otherwise it is a standalone assertion the geometry violates.
            let r_ops = operand_refs(built.relation(i));
            let colocated: Vec<usize> = partition
                .driving
                .iter()
                .copied()
                .filter(|&d| operand_refs(built.relation(d)).iter().any(|o| r_ops.contains(o)))
                .collect();

            if colocated.is_empty() {
                // A lone violated assertion — no driving relation pins the same
                // geometry. Geometric, no solver internals.
                solution.diagnostics.push(Diagnostic::error(format!(
                    "relation `{}` on `{}` is not satisfied at the solved placement: it is \
                     redundant with the driving relations but inconsistent with the geometry \
                     they produce",
                    rel.name, frame_unknown.sub,
                )));
            } else {
                let mut conflict = colocated;
                conflict.push(i);
                solution.diagnostics.push(conflict_diagnostic(
                    &conflict,
                    &built,
                    &frame_unknown.sub,
                ));
            }
        }
    }

    solution
}

// ── Zero-auto static verification (DIC α, task 5415) ─────────────────────────

/// One relation's static verdict.
enum StaticVerdict {
    /// Measured, and satisfied within the assertion tolerance — silent.
    Verified,
    /// Measured, and violated. Carries the DOMINANT residual row — value AND unit
    /// — so the aggregate can say by how much in the unit that was actually
    /// measured. A bare `f64` here is what let the renderer print `fmt_mm` over a
    /// dimensionless direction/dot/cosine row and state a fabricated length; a
    /// residual row vector is not dimensionally homogeneous (`concentric` alone
    /// mixes two tilt rows with two metre rows), so the unit has to ride along.
    ///
    /// The measured [`RelationInstance`] rides along too, so the renderer derives
    /// the demand phrase from the instance ALREADY IN HAND. Rebuilding it there
    /// re-walked the args and deep-cloned every operand `Value`, and needed an
    /// `unwrap_or_else` fallback that this arm's own precondition makes
    /// unreachable — dead code that, if ever reached, would have rendered
    /// "`concentric` requires … satisfied — off by 30 mm".
    Violated(ResidualRow, RelationInstance),
    /// Not DECIDED. Carries the reader-facing reason, which is the whole value of
    /// this arm: an undecided relation must say why, not fall silent.
    Unverifiable(String),
}

/// Statically verify a relate scope that has NO `at auto` subs.
///
/// # Why this exists
///
/// A `relate { }` block whose subs are all fixed has nothing to solve for, so
/// [`solve_scopes`] used to skip it outright. The consequence was that a
/// geometrically FALSE relate block was a total silent no-op — `reify eval` said
/// nothing and `reify check` printed "All constraints satisfied." A declared
/// intent was neither consumed nor its non-consumption reported, which
/// `docs/legibility/design-invariants.md` INV-SF-3 forbids
/// (`docs/prds/v0_6/declared-intent-consumption-accounting.md` §3 decision 1).
///
/// Nothing moves here: there is no pose to determine, only a verdict to render on
/// the datums as they already sit. So no solver, no partition, no Jacobian — just
/// [`static_relation_residuals`] per relation.
///
/// # Every relation lands in exactly one bucket
///
/// Walked in SOURCE order, each relation is tried against these arms in turn, and
/// the FIRST that matches wins:
///
/// 1. not a relation call ⇒ unverifiable (never dropped — a member that vanished
///    silently would be the same class of failure as the no-op itself);
/// 2. an operand on a sub in [`RelateScope::posed`] ⇒ unverifiable. Realized datums
///    are each structure's LOCAL datum in its own identity frame, and a declared
///    `at <pose>` is never composed into them, so any residual measured here would
///    be measured at the WRONG configuration. This arm runs BEFORE the measurement
///    precisely so a coincidentally-zero residual cannot be mistaken for a pass;
/// 3. an operand that did not realize (`Value::Undef` / absent) ⇒ unverifiable,
///    naming the operand;
/// 4. fewer than two datum operands to compare ⇒ unverifiable, naming the
///    `self.*` anchor when that is the cause. The reachable shape is the
///    `ground(sub)` / `fix(sub)` desugar `fasten(sub.frame, self.frame)`, whose
///    `self.frame` is the enclosing structure's own datum and so is not among the
///    realized sub datums arm 3 walks;
/// 5. an EMPTY residual row vector ⇒ unverifiable — no residual model for this
///    relation name / operand-kind combination. Never folded into "satisfied".
///    Arm 4 runs first so this arm's reason is only ever given for a genuinely
///    UNMODELLED combination;
/// 6. otherwise each row against the assertion rung for ITS OWN unit
///    ([`assertion_rung`]): every row within ⇒ verified (silent), any row beyond ⇒
///    violated.
///
/// The counts therefore always sum to the scope's relation count, which is what
/// makes a silently-skipped relation impossible to hide.
///
/// # Rendering
///
/// At most ONE Error and at most ONE Warning per scope, each naming its full set.
/// Aggregation is load-bearing rather than stylistic: `dedup_diagnostics`
/// (`crates/reify-cli/src/main.rs`) short-circuits on `code.is_some()`, so a CODED
/// per-relation diagnostic would reach the user uncollapsed, one line per relation
/// (the #5014 collateral-observability shape).
///
/// A wholly satisfied scope is SILENT. The placement-relations belt's δ leaf would
/// have warned on every zero-auto block regardless of the verdict; it was dropped
/// at decompose and superseded by this task (ratified 2026-07-25). Satisfied
/// relations are counted in [`StaticRelateFacts::verified`] for ζ's ledger instead.
///
/// Diagnostics speak geometry — via the same [`describe_demand`] /
/// [`describe_operands`] / [`fmt_mm`] helpers the auto-ful conflict path uses —
/// never solver internals. The measured magnitude is rendered by
/// [`fmt_residual`] in the unit the residual row carries: mm for a length row,
/// degrees for an orientation row, and a bare number for a direction/dot/cosine
/// row, which has no length reading at all. That same unit tag also selects the
/// rung the row is JUDGED against ([`assertion_rung`]) — rendering a row in one
/// unit while thresholding it in another would leave the category error in place
/// where it does the most damage, since a static violation FAILS the build. [`conflict_diagnostic`] is deliberately NOT reused: it
/// needs an auto sub and a driving/redundant partition that a zero-auto scope does
/// not have.
pub fn verify_static_scope(scope: &RelateScope, realized: &RealizedDatums) -> RelateSolution {
    let tol = RelateTolerance::kernel_default();
    let mut solution = RelateSolution::default();
    let mut facts = StaticRelateFacts::default();

    // Both lists are built by walking `scope.relations` in order, so the rendering
    // below is deterministic without sorting anything.
    let mut violated: Vec<String> = Vec::new();
    let mut unverifiable: Vec<String> = Vec::new();

    for rel in &scope.relations {
        let refs = operand_refs(rel);
        match static_verdict(rel, &refs, scope, realized, tol) {
            StaticVerdict::Verified => facts.verified += 1,
            StaticVerdict::Violated(row, inst) => {
                facts.violated += 1;
                violated.push(format!(
                    "`{}` requires {} {} — off by {}",
                    relation_name(rel),
                    describe_operands(rel),
                    describe_demand(&inst),
                    fmt_residual(row)
                ));
            }
            StaticVerdict::Unverifiable(reason) => {
                facts.unverifiable += 1;
                unverifiable.push(format!(
                    "`{}` on {} could not be checked: {reason}",
                    relation_name(rel),
                    describe_operands(rel)
                ));
            }
        }
    }

    if !violated.is_empty() {
        solution.diagnostics.push(
            Diagnostic::error(format!(
                "relate: {} not satisfied by the subs' fixed placements: {}",
                plural_relations(violated.len()),
                violated.join("; ")
            ))
            .with_code(DiagnosticCode::RelateStaticViolated),
        );
    }
    if !unverifiable.is_empty() {
        solution.diagnostics.push(
            Diagnostic::warning(format!(
                "relate: {} could not be statically verified: {}",
                plural_relations(unverifiable.len()),
                unverifiable.join("; ")
            ))
            .with_code(DiagnosticCode::RelateStaticUnverifiable),
        );
    }

    // Always `Some`, including the all-verified silent case: the ledger must be able
    // to tell "2 relations verified" from "no relate block here".
    solution.static_facts = Some(facts);
    solution
}

/// `"1 relation"` / `"{n} relations"` — the counted NOUN PHRASE both aggregate
/// headers open with. What it guarantees is number agreement with the count, the
/// one concern the two sites genuinely share.
///
/// The VERB belongs to each call site, which is why none appears here. Folding a
/// copula in is what produced "relate: 1 relation is could not be statically
/// verified": one helper cannot serve both "could not be statically verified"
/// and "not satisfied by …". Splitting into two per-site helpers is the wrong
/// repair — the counted noun phrase stays SPOT.
fn plural_relations(n: usize) -> String {
    if n == 1 {
        "1 relation".to_string()
    } else {
        format!("{n} relations")
    }
}

/// The relation's name as written, or a placeholder for a member that is not a
/// relation call (which arm 1 of [`verify_static_scope`] reports as unverifiable).
fn relation_name(rel: &CompiledExpr) -> String {
    match &rel.kind {
        CompiledExprKind::FunctionCall { function, .. } => function.name.clone(),
        _ => "<non-relation member>".to_string(),
    }
}

/// Decide ONE relation's static verdict. See [`verify_static_scope`] for the arm
/// ordering and why each exists; this function is that list, in that order.
fn static_verdict(
    rel: &CompiledExpr,
    refs: &[(String, String)],
    scope: &RelateScope,
    realized: &RealizedDatums,
    tol: RelateTolerance,
) -> StaticVerdict {
    // 1. Not a relation call. Counted, never dropped.
    let Some(inst) = relation_instance(rel, realized) else {
        return StaticVerdict::Unverifiable(
            "this relate member is not a relation call, so it has no geometric \
             residual to measure"
                .to_string(),
        );
    };

    // 2. An operand on a concretely-posed sub. BEFORE the measurement, so a
    //    coincidentally-zero residual at the wrong configuration cannot pass.
    if let Some((sub, _)) = refs.iter().find(|(sub, _)| scope.posed.contains(sub)) {
        return StaticVerdict::Unverifiable(format!(
            "sub `{sub}` is placed with a concrete `at <pose>`, which is not \
             composed into the local datums this check compares, so any verdict \
             would be measured at the wrong configuration"
        ));
    }

    // 3. An operand that did not realize — absent from the map, or present as
    //    `Undef`. Both mean the same thing to a reader, and both would otherwise
    //    reach `static_relation_residuals` as a missing datum.
    if let Some((sub, member)) = refs
        .iter()
        .find(|(sub, member)| !matches!(realized.get(sub, member), Some(v) if *v != Value::Undef))
    {
        return StaticVerdict::Unverifiable(format!(
            "`{sub}.{member}` did not resolve to a geometric datum"
        ));
    }

    // 4. Fewer than two datum operands, so there is no PAIR to compare. Read from
    //    `comparable_datum_operands` — the very guard `static_relation_residuals`
    //    applies — rather than re-derived here, so the reason cannot drift from the
    //    condition that fired. Arm 3 cannot catch this shape: `operand_refs` lists
    //    only `<sub>.<member>` operands, and the `self.*` anchor a
    //    `ground(sub)`/`fix(sub)` desugar carries is not one.
    if comparable_datum_operands(&inst) < 2 {
        return StaticVerdict::Unverifiable(match self_anchor_operand_name(rel) {
            Some(anchor) => format!(
                "`{anchor}` is the enclosing structure's own datum rather than a \
                 realized sub datum, so this relation has only one operand to \
                 compare"
            ),
            None => format!(
                "only {} of its operands realized as a geometric datum, so there is \
                 nothing to compare it against",
                comparable_datum_operands(&inst)
            ),
        });
    }

    // 5. An EMPTY row vector now means exactly one thing — no residual model for
    //    this name/operand-kind combination — because arm 4 took the other source
    //    (`static_relation_residuals` (iii)). UNVERIFIABLE, never folded into
    //    satisfied.
    let rows = static_relation_residuals(&inst);
    if rows.is_empty() {
        return StaticVerdict::Unverifiable(format!(
            "there is no residual model for `{}` over these operand kinds",
            inst.name
        ));
    }

    // 6. Judge each row against the rung for ITS OWN unit. A single rung applied to
    //    the whole vector was the same category error the `ResidualUnit` tag exists
    //    to prevent, one level up: `concentric` mixes dimensionless tilt rows with
    //    metre rows, and 1e-5 m and 1e-5 rad are not the same claim about geometry.
    //    Because a static violation FAILS a build that previously built silently,
    //    an over-tight rung on the wrong unit is a false Error, not just a cosmetic
    //    mismatch.
    //
    //    The DOMINANT row — the one the diagnostic reports — is the one furthest
    //    beyond its own rung, i.e. the largest exceedance RATIO. Picking it by raw
    //    `|value|` instead would compare a radian against a metre to decide which
    //    to show. `reduce` keeps the FIRST row of a tie, matching the source-order
    //    determinism the rest of this arm relies on.
    let (dominant, exceedance) = rows
        .iter()
        .copied()
        .map(|row| (row, row.value.abs() / assertion_rung(row.unit, &tol)))
        .reduce(|m, r| if r.1 > m.1 { r } else { m })
        .expect("rows is non-empty — the empty case returned Unverifiable above");
    if exceedance <= 1.0 {
        StaticVerdict::Verified
    } else {
        StaticVerdict::Violated(dominant, inst)
    }
}

/// The assertion rung a residual row is judged against — the one for ITS unit.
///
/// The three rungs are all derived from [`RelateTolerance`]'s single base length
/// (see [`RelateTolerance::assertion_angle`]), so they move together under an edit
/// to the hierarchy and cannot drift into three independent hand-picked epsilons.
fn assertion_rung(unit: ResidualUnit, tol: &RelateTolerance) -> f64 {
    match unit {
        ResidualUnit::Length => tol.assertion(),
        ResidualUnit::Angle => tol.assertion_angle(),
        ResidualUnit::Dimensionless => tol.assertion_dimensionless(),
    }
}

/// The name of the `self.*` anchor operand `rel` carries (`"self.frame"` for a
/// `ground(sub)` desugar), or `None` if it has none.
fn self_anchor_operand_name(rel: &CompiledExpr) -> Option<String> {
    let CompiledExprKind::FunctionCall { args, .. } = &rel.kind else {
        return None;
    };
    args.iter()
        .find_map(|arg| self_anchor_member(arg).map(|m| format!("self.{m}")))
}

/// Render a residual row in the unit it was MEASURED in.
///
/// Never launder a dimensionless row through [`fmt_mm`]. `parallel` /
/// `antiparallel` / `coincident`-over-Direction measure a unit-vector difference,
/// `perpendicular` a dot product and `angle` a cosine difference — all pure
/// numbers — while `concentric` / `flush` / `offset` mix dimensionless tilt rows
/// WITH metre rows in one vector. Printing "off by 0.5 mm" for a 0.5 direction
/// residual is a confidently-wrong claim of exactly the kind this whole arm exists
/// to prevent, so the unit tag decides the phrasing.
///
/// The MAGNITUDE is rendered, never the signed row value. A residual row's sign is
/// an artefact of the residual form's operand order and tangent-frame choice — the
/// dominant `concentric` row for the B1 fixture is `−0.03`, which read out as "off
/// by −30 mm" — and "off by" states a distance from satisfaction, which has no
/// direction a reader can act on. Unsigned is also the pre-existing convention on
/// this path: `max_relation_residual` accumulates `r.abs()`, and every other
/// reader-facing magnitude here is a magnitude. Taken ONCE, here, so all three arms
/// agree rather than each remembering.
fn fmt_residual(row: ResidualRow) -> String {
    let magnitude = row.value.abs();
    match row.unit {
        ResidualUnit::Length => fmt_mm(magnitude),
        ResidualUnit::Angle => fmt_deg(magnitude),
        // No length and no angle reading — say the number and what it is, rather
        // than dress it in a unit it does not have.
        ResidualUnit::Dimensionless => {
            format!("{magnitude:.4} (direction residual, dimensionless)")
        }
    }
}

/// An angle magnitude in degrees, trimmed like [`fmt_mm`] so the two read alike.
///
/// The trim cannot eat the whole numeral: `{:.3}` always emits a decimal point and
/// `trim_end_matches('0')` stops there, so the smallest rendering is `"0°"`, never a
/// bare `"°"`. That is a property of the format string rather than a thing to
/// defend with a fallback branch, so it is PINNED
/// (`fmt_deg_keeps_a_numeral_below_the_trim_threshold`) instead.
fn fmt_deg(radians: f64) -> String {
    let s = format!("{:.3}", radians.to_degrees());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}°")
}

/// Process every scope in `module` that declares at least one relation (ζ step-18 —
/// the build-pass entry).
///
/// Each such scope is collected ([`collect_relate_scope`]) and then dispatched on
/// whether it has anything to SOLVE for:
///
/// * **auto-ful** (≥1 `at auto` sub) → the full partition → solve → verify pipeline
///   ([`solve_relate_scope`]), which determines each auto sub's Frame; or
/// * **zero-auto** (every sub fixed) → static verification
///   ([`verify_static_scope`]), which determines nothing and renders a verdict on
///   the datums as they already sit.
///
/// One `(scope_name, RelateSolution)` is returned per processed scope, so the build
/// pass can write each solved Frame back into the value map (keyed by
/// [`auto_pose_cell`]) and surface the diagnostics (an `Error` fails the build). A
/// zero-auto solution carries empty `poses`, so the consumption loop skips the
/// writeback and forwards only the diagnostics — no caller change was needed to
/// surface this arm.
///
/// # Only a scope with NO relations is skipped
///
/// This filter used to also require a non-empty auto set, which meant a `relate { }`
/// block over fixed subs was dropped before any realization and never checked at
/// all. That made a geometrically FALSE relate block a total silent no-op — `reify
/// eval` said nothing and `reify check` printed "All constraints satisfied." A
/// declared intent was neither consumed nor its non-consumption reported, which
/// `docs/legibility/design-invariants.md` INV-SF-3 forbids (fixed by task #5415;
/// `docs/prds/v0_6/declared-intent-consumption-accounting.md` §3 decision 1, §4.4).
///
/// Skipping a scope with no relations at all is kept, and still does the work the
/// old filter was really there for: it keeps the kernel sub-build off the hot path
/// for the overwhelmingly common non-relate scope.
///
/// **One shared realization build.** Rather than clone + rebuild the module once per
/// scope, the operand structures of ALL qualifying scopes are realized together in a
/// SINGLE filtered sub-build ([`realize_structures`]); each scope then resolves only
/// its own operands from the shared value map ([`resolve_operands`]). Local datums
/// are structure-keyed and pose-independent, so the build is shared safely — a
/// structure's datums are identical regardless of which scope references it.
///
/// **Recursion through the sub-build.** That sub-build realizes each referenced
/// structure through `engine`, so its own `solve_scopes` runs over the retained
/// templates — and since the filter is now "≥1 relation", an operand structure that
/// declares a ZERO-AUTO relate block of its own recurses one level further than it
/// used to. Termination is structural: each level retains a strict sub-closure of
/// the last. The nested build's diagnostics are DISCARDED
/// ([`realize_structures`] keeps only `.values`), which costs nothing here because
/// this walk covers every template in the module — so the nested scope is verified
/// by THIS pass in its own right, exactly once. Measured, not assumed, by
/// `a_nested_zero_auto_scope_is_verified_once_by_the_outer_pass` in
/// `harness_engine/relate_static_verification_e2e.rs`; the prose this replaced
/// asserted that operand structures simply carry no relations, which nothing in the
/// compiler enforces.
///
/// Do NOT "fix" the extra level by filtering relation-declaring templates out of
/// `sub_module`: an operand structure may be BOTH (the pin's `Carrier` is), and
/// dropping it would leave the outer scope's operand unrealized — a decidable scope
/// turned unverifiable to save a sub-build.
///
/// The caller MUST invoke this BEFORE the outer build's own state resets so the
/// transient sub-build state is re-established by the main `check()` that follows.
pub fn solve_scopes(
    module: &CompiledModule,
    engine: &mut Engine,
) -> Vec<(String, RelateSolution)> {
    // Collect every scope that declares a relation. A scope with NO relations is
    // skipped before any realization — there is nothing to solve and nothing to
    // verify. A scope WITH relations is always processed, even with zero auto subs:
    // dropping those was the silent no-op INV-SF-3 forbids (#5415).
    let scopes: Vec<(String, RelateScope)> = module
        .templates
        .iter()
        .map(|t| (t.name.clone(), collect_relate_scope(t)))
        .filter(|(_, s)| !s.relations.is_empty())
        .collect();
    if scopes.is_empty() {
        return Vec::new();
    }

    // Realize the UNION of every scope's operand structures in ONE filtered sub-build
    // — no per-scope module.clone()/rebuild. Local datums are pose-independent, so the
    // seed estimate is empty and `solve_relate_scope` witnesses at identity (the
    // grounded anchor's datums encode the target).
    //
    // MIXING THE TWO SCOPE CLASSES IN THIS UNION IS SAFE, and safe for a specific
    // reason worth stating: widening the filter above means `all_refs` now also
    // carries zero-auto scopes' operand structures, so `realize_structures` retains
    // MORE templates than it used to. That cannot perturb an auto-ful scope because
    // each structure is realized STANDALONE in its own identity frame and
    // `resolve_operands` looks datums up by `(structure, member)` — so adding
    // structures adds map entries without altering any existing one.
    //
    // That is reasoning, not evidence, so it is also MEASURED: the V1 pins in
    // `harness_engine/relate_static_verification_e2e.rs` solve the same auto-ful
    // scope with and without a zero-auto companion sharing these leaf structures and
    // compare the DOF partition exactly and the solved pose to the solver's
    // convergence rung. If that ever reds, do NOT repair it by reverting to a
    // per-scope `module.clone()`/rebuild — that discards the single-shared-build
    // property PRD §3 decision 1 / §4.4 require. Split the retained-template set per
    // scope class instead.
    let scope_refs: Vec<Vec<OperandRef>> =
        scopes.iter().map(|(_, s)| scope_operand_refs(s)).collect();
    let all_refs: Vec<OperandRef> = scope_refs.iter().flatten().cloned().collect();
    let values = realize_structures(&all_refs, module, engine);

    // Process each scope against the shared realized datums. Zero-auto scopes join
    // the SAME union sub-build rather than getting one of their own — which is what
    // PRD §3 decision 1 / §4.4 require, and why the arm costs no extra kernel work.
    scopes
        .iter()
        .zip(scope_refs.iter())
        .map(|((name, scope), refs)| {
            let realized = resolve_operands(refs, &values);
            let solution = if scope.auto_unknowns.is_empty() {
                // Nothing to determine — render a verdict on the fixed placements.
                verify_static_scope(scope, &realized)
            } else {
                solve_relate_scope(scope, &realized)
            };
            (name.clone(), solution)
        })
        .collect()
}

/// The [`RelationInstance`]s [`build_relation_instances`] builds from a scope's
/// relations, paired with the `scope.relations` SOURCE index each was built from
/// (task 7050).
///
/// A relate-block member that is `Type::Relation` (the compiler accepted it) but
/// isn't a `CompiledExprKind::FunctionCall` — e.g. a call to a user-defined
/// `fn ... -> Relation` wrapper — is one the solve cannot build an instance for;
/// `build_relation_instances` records its source index in [`skipped`](Self::skipped)
/// instead, so `instances` can be SHORTER than `scope.relations` and the two are
/// addressed by DIFFERENT indices. `sources[i]` is the one place that mapping is
/// recorded, and [`ScopeInstances::relation`] is the ONLY sanctioned crossing from
/// an instance POSITION back to its source relation in `scope.relations` — no other
/// code may index `scope.relations` with an instance position. Borrowing `scope`
/// here, rather than re-taking it as a parameter on every call, ties that crossing
/// structurally to the ONE scope the pairing was built from, so a caller cannot
/// pass a mismatched scope. A member the compiler already flagged as NOT
/// `Type::Relation` is left to THAT diagnostic (anti-cascade) — it lands in neither
/// `sources` nor `skipped`.
struct ScopeInstances<'a> {
    /// The built instances, contiguous and in source order — the shape every
    /// `reify_constraints::relate_solve` entry point (`partition_driving_set`,
    /// `solve_frame`, `max_relation_residual`, `minimal_infeasible_subset`) expects
    /// and returns POSITIONS into.
    instances: Vec<RelationInstance>,
    /// `sources[i]` is the `scope.relations` index `instances[i]` was built from,
    /// strictly increasing in `i` — `build_relation_instances` debug-asserts both
    /// that and its length agreement with `instances`.
    sources: Vec<usize>,
    /// The `scope.relations` source indices of the `Type::Relation` members that
    /// yielded NO instance (not a `FunctionCall`) — disjoint from `sources`. A
    /// non-`Relation` member (already diagnosed by the compiler) is in neither list.
    skipped: Vec<usize>,
    /// The scope every index above is relative to — see the struct doc.
    scope: &'a RelateScope,
}

impl<'a> ScopeInstances<'a> {
    /// The source [`CompiledExpr`] relation `instances[i]` was built from.
    fn relation(&self, i: usize) -> &'a CompiledExpr {
        &self.scope.relations[self.sources[i]]
    }
}

/// Build a [`RelationInstance`] per relation in `scope`, resolving each operand to
/// its realized datum (or trailing scalar magnitude) for the partition / solve.
///
/// Datum operands (`<sub>.<member>`) are decoded ([`decode_operand`]) and looked up
/// in `realized` — tagged with their owning sub so the partition/solve know which
/// datums move with the auto Frame. A trailing scalar operand (the magnitude of a
/// metric DRIVE relation — `distance`/`angle`/`offset`) is carried as a `sub: None`
/// scalar [`Operand`]. Operand order is preserved (the residual forms are
/// order-sensitive). `nominal_delta_dof` is `None`: `reify_compiler::relation_delta_dof`
/// is `pub(crate)`, so the e2e partition stands on its Jacobian-measured rank alone
/// (the γ ΔDOF cross-check is exercised in the kernel-free constraints unit tests).
///
/// A member whose `result_type` isn't `Type::Relation` was already flagged by the
/// compiler's `check_relate_relations` (`crates/reify-compiler/src/entity.rs`) with
/// an accurate message — this pass leaves it alone (anti-cascade) rather than
/// piling a second, potentially contradictory `RelateExpectsRelation` diagnostic on
/// top. See [`ScopeInstances`] for how a `Type::Relation` member this still can't
/// build an instance for is tracked rather than silently misaligning later reads.
fn build_relation_instances<'a>(
    scope: &'a RelateScope,
    realized: &RealizedDatums,
) -> ScopeInstances<'a> {
    let mut instances = Vec::new();
    let mut sources = Vec::new();
    let mut skipped = Vec::new();
    for (source, rel) in scope.relations.iter().enumerate() {
        let Some(instance) = relation_instance(rel, realized) else {
            // Only a `Type::Relation` member is this pass's business (see doc
            // above) — the compiler already diagnosed anything else.
            if rel.result_type == Type::Relation {
                skipped.push(source);
            }
            continue;
        };
        instances.push(instance);
        sources.push(source);
    }
    debug_assert_eq!(instances.len(), sources.len(), "instances/sources built in lockstep");
    debug_assert!(
        sources.windows(2).all(|w| w[0] < w[1]),
        "sources must be strictly increasing (built from one forward walk)"
    );
    ScopeInstances { instances, sources, skipped, scope }
}

/// Build ONE relation's [`RelationInstance`] from its compiled expr + the realized
/// datums, or `None` when the expr is not a relation call.
///
/// Both [`build_relation_instances`] and the zero-auto static arm walk
/// `scope.relations` with `enumerate()` through this, keeping each entry's SOURCE
/// index: a `None` here means the instance list is SHORTER than `scope.relations`,
/// so position in one is not an index into the other.
fn relation_instance(rel: &CompiledExpr, realized: &RealizedDatums) -> Option<RelationInstance> {
    let CompiledExprKind::FunctionCall { function, args } = &rel.kind else {
        return None;
    };
    let mut operands = Vec::new();
    for arg in args {
        if let Some(opref) = decode_operand(arg) {
            let datum = realized
                .get(&opref.sub, &opref.member)
                .cloned()
                .unwrap_or(Value::Undef);
            operands.push(Operand {
                sub: Some(opref.sub),
                datum,
            });
        } else if let Some(scalar) = scalar_operand(arg) {
            operands.push(Operand {
                sub: None,
                datum: scalar,
            });
        }
        // An arg decoding as neither is silently absent from `operands` — a
        // per-operand gap distinct from the whole-member skip in
        // `build_relation_instances`, and not recorded in `skipped`
        // (deferred to #7496).
    }
    Some(RelationInstance {
        name: function.name.clone(),
        operands,
        nominal_delta_dof: None,
    })
}

/// The literal scalar magnitude an operand expr denotes (the trailing metric of a
/// `distance`/`angle`/`offset` DRIVE relation), or `None` if it is not a numeric
/// literal. `Value::Scalar`'s SI magnitude is read by the residual forms via
/// `as_f64`, so a `5mm` literal flows through as `0.005`.
fn scalar_operand(expr: &CompiledExpr) -> Option<Value> {
    match &expr.kind {
        CompiledExprKind::Literal(v @ (Value::Scalar { .. } | Value::Real(_) | Value::Int(_))) => {
            Some(v.clone())
        }
        _ => None,
    }
}

// ── Conflict diagnostics (ζ step-16) ─────────────────────────────────────────

/// The `(sub, member)` datum-operand references a compiled relation expr denotes,
/// in operand order (e.g. `distance(bolt.shank_axis, plate.hole_axis, 5mm)` →
/// `[("bolt","shank_axis"), ("plate","hole_axis")]`). Reuses [`decode_operand`];
/// scalar magnitudes and any non-datum operands are skipped. Two relations *share a
/// datum* — and so pin the same geometry — when their reference sets intersect.
fn operand_refs(rel: &CompiledExpr) -> Vec<(String, String)> {
    match &rel.kind {
        CompiledExprKind::FunctionCall { args, .. } => args
            .iter()
            .filter_map(|a| decode_operand(a).map(|o| (o.sub, o.member)))
            .collect(),
        _ => Vec::new(),
    }
}

/// Render a length (metres) in millimetres for a reader-facing geometric
/// explanation, trimming trailing zeros: `0.005 → "5 mm"`, `0.0055 → "5.5 mm"`.
/// Magnitudes in diagnostics speak geometry (mm), never SI metres or solver units.
fn fmt_mm(meters: f64) -> String {
    let s = format!("{:.3}", meters * 1000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} mm")
}

/// The geometric demand a relation places on its operands — the predicate phrase a
/// conflict explanation reads as "`<name>` requires <subjects> <demand>". Metric
/// DRIVE relations (`distance`/`offset`/`angle`) render their magnitude in mm /
/// degrees from `inst`'s trailing scalar operand; mate relations render their fixed
/// geometric demand (e.g. `concentric` → coincident at 0 mm). Never solver units.
fn describe_demand(inst: &RelationInstance) -> String {
    // Every metric operand, in declaration order. `tangent`'s two-radius combos
    // (`tangent(a, b, r1, r2)`) carry BOTH radii, so the whole list is collected
    // rather than the first match. The single-metric relations below read
    // `scalars.first()`, which for them is the value the previous `.find(..)`
    // returned — they each carry exactly one metric operand, so first and only
    // coincide and no existing phrasing moves.
    let scalars: Vec<f64> = inst
        .operands
        .iter()
        .filter(|o| o.sub.is_none())
        .filter_map(|o| o.datum.as_f64())
        .collect();
    let scalar = scalars.first().copied();
    match inst.name.as_str() {
        "concentric" | "coincident" => "coincident (0 mm apart)".to_string(),
        "flush" => "coplanar (flush, 0 mm offset)".to_string(),
        "parallel" => "parallel".to_string(),
        "antiparallel" => "anti-parallel".to_string(),
        "perpendicular" => "perpendicular".to_string(),
        "on" => "incident".to_string(),
        // `tangent` is metric like distance/offset — its radii travel as trailing
        // scalar operands (task 5540), so the phrase must name them. The bare word
        // "tangent" told an author whose tangency could not be satisfied nothing at
        // all about WHAT separation was demanded. The centre-distance target the
        // residual actually drives is `|r1 + r2|`, so it is spelled out too: that is
        // the number a conflicting placement is being measured against, and under the
        // signed-radius convention (a negative radius = internal tangency) it is not
        // inferable from the two radii by eye.
        "tangent" => match scalars.as_slice() {
            [r] => format!("tangent ({} radius)", fmt_mm(*r)),
            [r1, r2] => format!(
                "tangent ({} and {} radii — {} between axes)",
                fmt_mm(*r1),
                fmt_mm(*r2),
                fmt_mm((r1 + r2).abs())
            ),
            _ => "tangent".to_string(),
        },
        "distance" => match scalar {
            Some(d) => format!("{} apart", fmt_mm(d)),
            None => "a fixed distance apart".to_string(),
        },
        "offset" => match scalar {
            Some(d) => format!("offset by {}", fmt_mm(d)),
            None => "offset".to_string(),
        },
        "angle" => match scalar {
            Some(theta) => format!("at {:.1}°", theta.to_degrees()),
            None => "at a fixed angle".to_string(),
        },
        other => other.to_string(),
    }
}

/// Render a relation's datum operands as `sub.member and sub.member` — the geometric
/// subjects of the relation, reader-facing (the conflict explanation's "what").
fn describe_operands(rel: &CompiledExpr) -> String {
    let parts: Vec<String> = operand_refs(rel)
        .iter()
        .map(|(s, m)| format!("{s}.{m}"))
        .collect();
    match parts.len() {
        0 => "its operands".to_string(),
        1 => parts[0].clone(),
        _ => {
            let last = &parts[parts.len() - 1];
            let head = parts[..parts.len() - 1].join(", ");
            format!("{head} and {last}")
        }
    }
}

/// Build the geometric conflict [`Diagnostic`] for a minimal conflict set (ζ step-16).
///
/// `conflict` are INSTANCE positions (into `built.instances` — what
/// [`minimal_infeasible_subset`] and the redundant-remainder `colocated` check both
/// return) of the mutually-inconsistent relations. The NEWEST-declared member — the
/// greatest SOURCE index ([`ScopeInstances::relation`]'s `built.sources`), since the
/// flat relation set preserves source/declaration order — is flagged as the
/// **primary** conflict (PRD §7.1: newest member is the likely culprit). The
/// explanation is purely geometric — each relation's demand + its magnitude in mm /
/// degrees — and never mentions the solver or libslvs (ζ's diagnostics speak
/// geometry; θ #4388 renders the polished `reify explain` ledger / spans / badge from
/// the same data).
fn conflict_diagnostic(conflict: &[usize], built: &ScopeInstances, auto_sub: &str) -> Diagnostic {
    // primary = newest-declared = greatest SOURCE index among the conflict set.
    let primary = conflict.iter().copied().max_by_key(|&i| built.sources[i]).unwrap_or(0);
    let mut others: Vec<usize> = conflict.iter().copied().filter(|&i| i != primary).collect();
    others.sort_unstable();

    let primary_name = &built.instances[primary].name;
    let primary_subjects = describe_operands(built.relation(primary));
    let primary_demand = describe_demand(&built.instances[primary]);

    let mut msg = format!(
        "conflicting relations on `{auto_sub}`: `{primary_name}` requires \
         {primary_subjects} {primary_demand}"
    );
    for &o in &others {
        msg.push_str(&format!(
            ", but `{}` requires them {}",
            built.instances[o].name,
            describe_demand(&built.instances[o]),
        ));
    }
    msg.push_str(&format!(
        " — these cannot both be satisfied. `{primary_name}` is the newest-declared \
         relation, flagged as the primary conflict; remove or relax it."
    ));

    Diagnostic::error(msg)
}

/// Derive a MINIMAL inconsistent subset of the driving relations (ζ step-16).
///
/// Tries each driving PAIR (the smallest non-trivial conflict) via a re-solve: the
/// first pair that is still [`SolveResult::Infeasible`] on its own is returned as the
/// minimal conflict set. If no pair is infeasible (a conflict that genuinely needs
/// ≥3 relations), falls back to the whole driving set. Returns INSTANCE positions
/// (into `instances`), the same shape [`conflict_diagnostic`] expects. Bounded by
/// the driving-set size, which is `≤ 6` for a single Frame unknown — the pairwise
/// search is cheap.
fn minimal_infeasible_subset(
    instances: &[RelationInstance],
    driving: &[usize],
    unknown: &FrameUnknown,
    seed: &Pose,
    tol: f64,
) -> Vec<usize> {
    for (a, &i) in driving.iter().enumerate() {
        for &j in &driving[a + 1..] {
            let pair = [instances[i].clone(), instances[j].clone()];
            if matches!(
                solve_frame(&pair, unknown, seed, tol),
                SolveResult::Infeasible { .. }
            ) {
                return vec![i, j];
            }
        }
    }
    driving.to_vec()
}

// ── Unit tests for mounted_joint_cell (step-1, task #4399) ───────────────────

#[cfg(test)]
mod tests_mounted_joint_cell {
    //! Kernel-free unit tests for [`mounted_joint_cell`].
    //!
    //! Compiles a small structure that has:
    //!   - `sub base : Base` (grounded anchor)
    //!   - `sub link : Link` (no `at auto` / no relate block — not needed by `mounted_joint_cell`)
    //!   - `let j = revolute(link.hub_point, 0rad..2.094rad)` — first arg is a `link` sub datum (DD1 match)
    //!   - `let k = revolute(vec3(0, 0, 1), 0rad..2.094rad)` — literal vec3 arg (B9: no match)
    //!
    //! Assertions:
    //!   (a) `mounted_joint_cell("Mech", "link", &module) == Some(ValueCellId::new("Mech", "j"))` — RED until impl
    //!   (b) `mounted_joint_cell("Mech", "link", &module) != Some(ValueCellId::new("Mech", "k"))` — B9-consistency
    //!   (c) `mounted_joint_cell("Mech", "nonexistent", &module) == None` — unknown sub → None
    //!   (d) `mounted_joint_cell("DoesNotExist", "link", &module) == None` — unknown scope → None

    use reify_core::ValueCellId;
    use reify_test_support::compile_source_with_stdlib;

    use super::mounted_joint_cell;

    /// The test fixture: a Mech structure with a relate-mounted link sub and two
    /// revolute joints — `j` (axis refs `link`, should match) and `k` (literal
    /// axis, should NOT match).
    // Kernel-free fixture: uses only `point3` (a plain multi-component constructor the
    // compiler types without geometry) — avoids `axis_z` whose return type the compiler
    // doesn't register statically (axis_z is a runtime geometry builtin). The relate
    // block is omitted because `mounted_joint_cell` scans `template.value_cells`, not
    // `template.relations`; the sub-datum reference in `j`'s arg is all that matters.
    const SOURCE: &str = r#"
structure Base {
    let mount_point : Point = point3(50mm, 0mm, 0mm)
}

structure Link {
    let hub_point : Point = point3(0mm, 0mm, 0mm)
}

structure Mech {
    sub base : Base
    sub link : Link
    // j: first arg is `link.hub_point` — a cross-sub IndexAccess (OperandRef sub="link")
    // → DD1 match: mounted_joint_cell("Mech", "link") must return Some(cell_id("Mech","j"))
    let j = revolute(link.hub_point, 0rad..2.094rad)
    // k: literal vec3 arg — no sub datum reference → B9 non-match → None for sub="link"
    let k = revolute(vec3(0, 0, 1), 0rad..2.094rad)
}
"#;

    fn compiled_module() -> reify_compiler::CompiledModule {
        let module = compile_source_with_stdlib(SOURCE);
        // Sanity: must compile without errors.
        assert!(
            module.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
            "test fixture must compile without errors; got: {:#?}",
            module.diagnostics
        );
        module
    }

    /// (a) RED test — mounted_joint_cell returns the joint cell that references the
    /// mounted sub (the `j` cell whose first operand is `link.hub_point`).
    ///
    /// RED: the stub returns `None` for every pair; this will fail until step-2
    /// implements the operand-reference scan.
    #[test]
    fn mounted_joint_cell_returns_cell_for_mounted_sub() {
        let module = compiled_module();
        let expected = ValueCellId::new("Mech", "j");
        let result = mounted_joint_cell("Mech", "link", &module);
        assert_eq!(
            result,
            Some(expected),
            "mounted_joint_cell(\"Mech\", \"link\") must return Some(\"Mech\".\"j\") \
             (the joint whose axis arg references link), got {result:?}"
        );
    }

    /// (b) B9 consistency — the literal-axis joint `k` must NOT be returned;
    /// the sub-referencing joint `j` must be returned instead.
    ///
    /// Strengthened from the original exclusion-only form (`result != Some(k)`,
    /// which passes trivially against the old None stub):
    ///   1. Asserts `k` EXISTS in the module (exclusion is non-vacuous).
    ///   2. Asserts `result == Some(j)` (correct answer, not just "not k").
    #[test]
    fn mounted_joint_cell_excludes_literal_axis_joint() {
        let module = compiled_module();
        let j_id = ValueCellId::new("Mech", "j");
        let k_id = ValueCellId::new("Mech", "k");
        // Precondition: `k` must exist in the compiled module so the exclusion is
        // non-vacuous (not just "result != Some(id-that-doesn't-exist)").
        assert!(
            module
                .templates
                .iter()
                .any(|t| t.name == "Mech" && t.value_cells.iter().any(|c| c.id == k_id)),
            "test precondition: literal-axis joint `k` must exist in the Mech template"
        );
        let result = mounted_joint_cell("Mech", "link", &module);
        assert_eq!(
            result,
            Some(j_id),
            "mounted_joint_cell must return Some(j) (sub-referencing joint), \
             not Some(k) (literal-axis, B9 non-match) or None"
        );
    }

    /// First-match semantics: when two joint cells both reference the same sub,
    /// `mounted_joint_cell` returns the FIRST one in declaration order.
    ///
    /// Pins the first-match behavior documented in [`mounted_joint_cell`]: only
    /// the first matching cell is returned; subsequent matches are silently skipped.
    #[test]
    fn mounted_joint_cell_first_match_returns_declaration_order_first() {
        const MULTI_JOINT_SOURCE: &str = r#"
structure Link2 {
    let hub_point : Point = point3(0mm, 0mm, 0mm)
}

structure Mech2 {
    sub link : Link2
    let j  = revolute(link.hub_point, 0rad..1rad)
    let j2 = revolute(link.hub_point, 0rad..2rad)
}
"#;
        let module = compile_source_with_stdlib(MULTI_JOINT_SOURCE);
        assert!(
            module.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
            "multi-joint fixture must compile without errors; got: {:#?}",
            module.diagnostics
        );
        let j_id = ValueCellId::new("Mech2", "j");
        let result = mounted_joint_cell("Mech2", "link", &module);
        assert_eq!(
            result,
            Some(j_id),
            "when two joints reference the same sub, mounted_joint_cell must \
             return the FIRST in declaration order (first-match semantics)"
        );
    }

    /// `is_motion_joint_cell_type` recognises every driving-joint StructureRef
    /// kind and rejects non-motion kinds.
    ///
    /// **Note — tautological coupling**: both this test and `is_motion_joint_cell_type`
    /// hardcode the same six tags.  A new driving-joint kind added to
    /// `joint_ctor_result_type` in `reify-compiler` would be missed by BOTH
    /// (no test failure on its own).  The companion behavioral test
    /// `mounted_joint_cell_matches_each_driving_joint_kind` provides an
    /// independent guard: it compiles a real fixture per kind and asserts
    /// `mounted_joint_cell` returns `Some(...)`, catching any drift between the
    /// compiler's type assignment and this function's match arms once the
    /// new-kind fixture is added there.
    #[test]
    fn is_motion_joint_cell_type_covers_all_expected_kinds() {
        use reify_core::Type;

        use super::is_motion_joint_cell_type;

        // Driving-joint StructureRef tags — all must match.
        for tag in &["Revolute", "Prismatic", "Cylindrical", "Planar", "Spherical", "Fixed"] {
            assert!(
                is_motion_joint_cell_type(&Type::StructureRef((*tag).to_string())),
                "is_motion_joint_cell_type must return true for StructureRef({tag:?})"
            );
        }
        // Non-motion StructureRef tags — none must match.
        // `JacobianColumn` is the joint_jacobian result tag since task 6102;
        // `Twist` stays — it is still the transform_log / transform_exp tag.
        // Both must be non-motion: a Jacobian column and a spatial velocity are
        // neither of them a joint kind.
        for tag in &[
            "Coupling",
            "Mechanism",
            "Snapshot",
            "JointBinding",
            "BodyId",
            "SweepDim",
            "Twist",
            "JacobianColumn",
        ] {
            assert!(
                !is_motion_joint_cell_type(&Type::StructureRef((*tag).to_string())),
                "is_motion_joint_cell_type must return false for StructureRef({tag:?})"
            );
        }
        // Non-StructureRef types must not match.
        assert!(
            !is_motion_joint_cell_type(&Type::dimensionless_scalar()),
            "is_motion_joint_cell_type must return false for a non-StructureRef type"
        );
    }

    /// (c) Unknown sub → None.
    #[test]
    fn mounted_joint_cell_returns_none_for_unknown_sub() {
        let module = compiled_module();
        assert_eq!(
            mounted_joint_cell("Mech", "nonexistent", &module),
            None,
            "mounted_joint_cell must return None for a sub name that does not exist"
        );
    }

    /// (d) Unknown scope → None.
    #[test]
    fn mounted_joint_cell_returns_none_for_unknown_scope() {
        let module = compiled_module();
        assert_eq!(
            mounted_joint_cell("DoesNotExist", "link", &module),
            None,
            "mounted_joint_cell must return None when the scope template is not found"
        );
    }

    /// Behavioral coverage: `mounted_joint_cell` matches a sub-datum-referencing
    /// joint cell for EACH driving-joint kind, not just revolute.
    ///
    /// This is the independent complement to
    /// `is_motion_joint_cell_type_covers_all_expected_kinds` (which is tautological:
    /// it hardcodes the same tags as `is_motion_joint_cell_type` itself).  Here the
    /// expected result is derived from the COMPILER's type assignment: each fixture
    /// uses a real joint-constructor call that the compiler resolves to
    /// `Type::StructureRef("<Kind>")`, so a kind present in `joint_ctor_result_type`
    /// but absent from `is_motion_joint_cell_type` WOULD produce a failure once a
    /// fixture for the new kind is added here.
    ///
    /// **`fixed` is excluded**: `fixed()` takes no args and cannot reference a sub
    /// datum by construction; `mounted_joint_cell` always returns `None` for fixed
    /// joints (verified in the sub-test at the end).
    ///
    /// **Compiler leniency**: joint-constructor builtins return
    /// `Type::StructureRef(...)` regardless of arg types (name-only dispatch in
    /// `joint_ctor_result_type`). The fixtures intentionally use `hub_point : Point`
    /// for all constructor arg positions — even where a Vector or Range is expected
    /// at runtime — because this test targets the static association scan (not
    /// runtime evaluation). The compiler accepts these without Error diagnostics.
    #[test]
    fn mounted_joint_cell_matches_each_driving_joint_kind() {
        /// Compile a structure with `sub link : Hub` and one joint of the given
        /// `ctor_call`, then assert `mounted_joint_cell("Mech", "link")` returns
        /// `Some(ValueCellId::new("Mech", "j"))`.
        fn check_kind(ctor_call: &str, kind_label: &str) {
            // Use a fresh structure name per kind to avoid name collisions when
            // all fixtures are compiled in the same module.
            let mech_name = format!("Mech{kind_label}");
            let source = format!(
                r#"
structure Hub{kind_label} {{
    let hub_point : Point = point3(0mm, 0mm, 0mm)
}}
structure {mech_name} {{
    sub link : Hub{kind_label}
    let j = {ctor_call}
}}
"#
            );
            let module = compile_source_with_stdlib(&source);
            assert!(
                module.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
                "fixture for {kind_label} must compile without Error diagnostics; got: {:#?}",
                module.diagnostics
            );
            let j_id = ValueCellId::new(&mech_name, "j");
            let result = mounted_joint_cell(&mech_name, "link", &module);
            assert_eq!(
                result,
                Some(j_id),
                "mounted_joint_cell must return Some(j) for a {kind_label} joint \
                 whose arg references the mounted sub (StructureRef({kind_label:?}) \
                 must be in is_motion_joint_cell_type)"
            );
        }

        // ── Revolute (2-arg): already covered by other tests, included for completeness.
        check_kind("revolute(link.hub_point, 0rad..1rad)", "Revolute");
        // ── Prismatic (2-arg: axis, length_range).
        check_kind("prismatic(link.hub_point, 0mm..100mm)", "Prismatic");
        // ── Cylindrical (3-arg: axis, translation_range, rotation_range).
        check_kind("cylindrical(link.hub_point, 0mm..100mm, 0rad..1rad)", "Cylindrical");
        // ── Planar (5-arg: axis_x, axis_y, range_x, range_y, range_theta).
        // Uses hub_point for both axis args; DD1 matches on the first arg.
        check_kind(
            "planar(link.hub_point, link.hub_point, 0mm..100mm, 0mm..100mm, 0rad..1rad)",
            "Planar",
        );
        // ── Spherical (1-arg: angle_range). Spherical takes only a range arg
        // (no axis/center), so hub_point is used as the arg — semantically wrong
        // at runtime but the compiler assigns StructureRef("Spherical") regardless
        // of arg types (name-only dispatch). This tests that `is_motion_joint_cell_type`
        // includes "Spherical" and that decode_operand finds the sub-datum reference.
        check_kind("spherical(link.hub_point)", "Spherical");

        // ── Fixed: 0 args — cannot reference a sub datum by construction.
        {
            let source = r#"
structure HubFixed {
    let hub_point : Point = point3(0mm, 0mm, 0mm)
}
structure MechFixed {
    sub link : HubFixed
    let j = fixed()
}
"#;
            let module = compile_source_with_stdlib(source);
            assert!(
                module.diagnostics.iter().all(|d| d.severity != reify_core::Severity::Error),
                "fixed() fixture must compile without Error diagnostics"
            );
            let result = mounted_joint_cell("MechFixed", "link", &module);
            assert_eq!(
                result,
                None,
                "fixed() takes no args so mounted_joint_cell must return None — \
                 fixed joints cannot reference a sub datum by construction"
            );
        }
    }
}

#[cfg(test)]
mod tests_static_verification {
    //! Kernel-free unit tests for the ZERO-AUTO static-verification arm — DIC α
    //! (task 5415).
    //!
    //! In-file rather than an integration test (mirroring
    //! [`super::tests_mounted_joint_cell`]) because these reach crate-private
    //! internals — `resolve_operands`, `RealizedDatums`' inner map — that an
    //! external test crate cannot see. They compile `.ri` source strings and
    //! read structure off the resulting templates; no geometry kernel is
    //! involved.

    use std::collections::HashMap;

    use reify_core::{DiagnosticCode, Severity};
    use reify_ir::Value;
    use reify_test_support::compile_source_with_stdlib;

    use super::{
        RealizedDatums, RelateScope, StaticRelateFacts, collect_relate_scope,
        verify_static_scope,
    };

    /// Three scopes covering the whole `posed` classification:
    ///
    /// * `PosedScope` — one sub with a concrete `at <pose>`, one with no `at`
    ///   clause at all;
    /// * `PoseFreeScope` — the shape both DIC fixtures use: no `at` anywhere;
    /// * `AutoScope` — an `at auto` sub, whose placement is solver-determined.
    ///
    /// Kernel-free: the leaf structures carry only `point3`, a plain
    /// multi-component constructor the compiler types without geometry (the same
    /// restriction `tests_mounted_joint_cell`'s fixture works under).
    const SOURCE: &str = r#"
structure Bushing {
    let p = point3(0mm, 0mm, 0mm)
}

structure Plate {
    let p = point3(0mm, 0mm, 0mm)
}

structure PosedScope {
    sub a : Bushing at transform3(orient_identity(), vec3(30mm, 20mm, 5mm))
    sub b : Plate
}

structure PoseFreeScope {
    sub a : Bushing
    sub b : Plate
}

structure AutoScope {
    sub a : Bushing at auto
    sub b : Plate
}

structure BushingS {
    let bore = cylinder(4mm, 12mm)
    let bore_axis : Axis = bore.axis
    let seat = rectangle(10mm, 10mm)
    let seat_plane : Plane = seat.plane
}

structure PlateS {
    let boss = cylinder(4mm, 12mm)
    let boss_axis : Axis = boss.axis
    let top = rectangle(10mm, 10mm)
    let top_plane : Plane = top.plane
}

structure StaticScope {
    sub bush : BushingS
    sub plate : PlateS

    relate {
        concentric(bush.bore_axis, plate.boss_axis)
        flush(bush.seat_plane, plate.top_plane)
    }
}

structure SingleRelationScope {
    sub bush : BushingS
    sub plate : PlateS

    relate {
        concentric(bush.bore_axis, plate.boss_axis)
    }
}

structure PosedOperandScope {
    sub bush : BushingS
    sub plate : PlateS at transform3(orient_identity(), vec3(30mm, 20mm, 5mm))

    relate {
        concentric(bush.bore_axis, plate.boss_axis)
    }
}

structure GroundedScope {
    sub bush : BushingS
    sub plate : PlateS

    relate {
        ground(bush)
    }
}

structure FastenScope {
    sub bush : BushingS
    sub plate : PlateS

    relate {
        fasten(bush.frame, plate.frame)
    }
}

structure PerpendicularScope {
    sub bush : BushingS
    sub plate : PlateS

    relate {
        perpendicular(bush.bore_axis, plate.boss_axis)
    }
}
"#;

    /// Collect the named scope, panicking with the available template names on a
    /// miss so a fixture rename fails legibly rather than as `unwrap` on `None`.
    fn scope(name: &str) -> RelateScope {
        let module = compile_source_with_stdlib(SOURCE);
        let template = module
            .templates
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "fixture template `{name}` must compile; got {:?}",
                    module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
                )
            });
        collect_relate_scope(template)
    }

    /// A sub declared with a concrete `at <pose>` is recorded in `posed`, in
    /// sub-declaration order — AND still appears in `ground`.
    ///
    /// `posed` is an ADDITIONAL classification, not a partition of `ground`: a
    /// posed sub is genuinely a fixed (non-auto) anchor, so removing it from
    /// `ground` would change the auto-ful solve path's inputs and break V1. The
    /// two lists overlap on purpose.
    ///
    /// # Why the classification is needed at all
    ///
    /// `resolve_operands` keys realized datums by `(structure, member)` — each
    /// structure's LOCAL datum in its OWN identity frame — and a declared
    /// `SubComponentDecl.pose` is never composed into them. A static verdict
    /// computed over a posed sub's datums would therefore be evaluated at the
    /// WRONG configuration, producing a confidently wrong answer in either
    /// direction. The zero-auto arm uses this list to classify such a scope's
    /// relations UNVERIFIABLE instead (PRD §10 open question 5).
    #[test]
    fn collect_relate_scope_records_concretely_posed_subs() {
        let s = scope("PosedScope");
        assert_eq!(
            s.posed,
            vec!["a".to_string()],
            "only the sub carrying a concrete `at <pose>` belongs in `posed`"
        );
        assert_eq!(
            s.ground,
            vec!["a".to_string(), "b".to_string()],
            "a posed sub is STILL a non-auto ground anchor — `posed` is an \
             additional classification, not a partition of `ground`"
        );
        assert!(
            s.auto_unknowns.is_empty(),
            "no sub in this scope is `at auto`"
        );
    }

    /// A scope whose subs carry no `at` clause has an EMPTY `posed` list.
    ///
    /// This is the shape of both DIC fixtures
    /// (`dic_relate_static_{violated,ok}.ri`), so it is the shape that must be
    /// statically VERIFIABLE — an over-eager `posed` would classify them
    /// unverifiable and leave the false green in place under a new name.
    #[test]
    fn collect_relate_scope_leaves_posed_empty_without_at_clauses() {
        let s = scope("PoseFreeScope");
        assert!(
            s.posed.is_empty(),
            "no sub carries an `at` clause, so `posed` must be empty; got {:?}",
            s.posed
        );
        assert_eq!(s.ground, vec!["a".to_string(), "b".to_string()]);
    }

    /// An `at auto` sub never appears in `posed`.
    ///
    /// Guaranteed structurally, not incidentally: `SubComponentDecl` documents
    /// that `auto_pose.is_some()` implies `pose.is_none()` — the placement is
    /// solver-determined, not a compiled pose expression — so the two
    /// classifications cannot both fire for one sub.
    #[test]
    fn collect_relate_scope_never_files_an_auto_sub_as_posed() {
        let s = scope("AutoScope");
        assert!(
            s.posed.is_empty(),
            "`at auto` is solver-determined, not a concrete pose; got {:?}",
            s.posed
        );
        assert_eq!(
            s.auto_unknowns.len(),
            1,
            "the `at auto` sub must still be collected as a Frame unknown"
        );
        assert_eq!(s.auto_unknowns[0].sub, "a");
    }

    // ── static verification (`verify_static_scope`) — DIC α steps 5/6 ────────
    //
    // These build `RealizedDatums` BY HAND rather than through a kernel build, so
    // the whole verdict surface — satisfied, violated, and both flavours of
    // unverifiable — is reachable without OCCT and without contriving `.ri`
    // geometry that lands on each one. The datum magnitudes are the measured
    // `dic_relate_static_violated` split (30, 20, 5 mm), so the kernel-free unit
    // and the OCCT e2e are pinning the same numbers.

    fn point3v(x: f64, y: f64, z: f64) -> Value {
        Value::Point(vec![Value::length(x), Value::length(y), Value::length(z)])
    }

    fn vec3v(x: f64, y: f64, z: f64) -> Value {
        Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
    }

    fn axis_v(o: (f64, f64, f64), d: (f64, f64, f64)) -> Value {
        Value::Axis {
            origin: Box::new(point3v(o.0, o.1, o.2)),
            direction: Box::new(vec3v(d.0, d.1, d.2)),
        }
    }

    fn plane_v(o: (f64, f64, f64), n: (f64, f64, f64)) -> Value {
        Value::Plane {
            origin: Box::new(point3v(o.0, o.1, o.2)),
            normal: Box::new(vec3v(n.0, n.1, n.2)),
        }
    }

    /// A `Frame` at `o` whose basis is a rotation of `deg` about `+z`. The basis is
    /// a unit quaternion, which is the only shape `frame_coincidence_residual`
    /// reads; `deg = 0` is the identity orientation.
    fn frame_v(o: (f64, f64, f64), deg: f64) -> Value {
        let half = deg.to_radians() / 2.0;
        Value::Frame {
            origin: Box::new(point3v(o.0, o.1, o.2)),
            basis: Box::new(Value::Orientation {
                w: half.cos(),
                x: 0.0,
                y: 0.0,
                z: half.sin(),
            }),
        }
    }

    /// Hand-build a [`RealizedDatums`] — its map is crate-private, which is the
    /// reason this module lives in-file rather than in `tests/`.
    fn realized(entries: &[(&str, &str, Value)]) -> RealizedDatums {
        let mut operands = HashMap::new();
        for (sub, member, v) in entries {
            operands.insert(((*sub).to_string(), (*member).to_string()), v.clone());
        }
        RealizedDatums { operands }
    }

    /// The `dic_relate_static_violated` split, in metres.
    const SPLIT: (f64, f64, f64) = (0.030, 0.020, 0.005);

    /// Both subs' datums colocated at `SPLIT` — every relation TRUE.
    fn colocated_datums() -> RealizedDatums {
        realized(&[
            ("bush", "bore_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ("bush", "seat_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
            ("plate", "top_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
        ])
    }

    /// The bushing at the origin, the plate at `SPLIT` — every relation FALSE.
    ///
    /// The resulting magnitudes are derived, not tuned: `concentric` measures the
    /// 30 mm in-plane split (an axis constrains only the two components
    /// perpendicular to itself), `flush` the 5 mm along-normal offset. Both are
    /// 500×–3000× the 1e-5 m assertion tolerance, so no verdict here is sensitive
    /// to that constant's exact value.
    fn split_datums() -> RealizedDatums {
        realized(&[
            ("bush", "bore_axis", axis_v((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ("bush", "seat_plane", plane_v((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ("plate", "top_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
        ])
    }

    /// (a) A zero-auto scope whose relations all HOLD is completely silent, and
    /// reports both as verified.
    ///
    /// The silence is the contract, not an accident of implementation. The
    /// placement-relations belt's δ leaf would have emitted a `W_RELATE_NO_AUTO`
    /// warning on every zero-auto relate block regardless of whether the assertion
    /// held; that leaf was DROPPED at decompose and superseded by this task
    /// (ratified 2026-07-25). So this asserts the diagnostic list is empty
    /// outright — not merely free of Errors — which is what would fail if
    /// `W_RELATE_NO_AUTO` were ever reintroduced.
    ///
    /// `static_facts` is `Some` even here, so ζ's ledger can tell "2 relations
    /// verified" from "no relate block at all" — the difference between consumed
    /// and absent.
    #[test]
    fn verify_static_scope_is_silent_when_every_relation_holds() {
        let s = scope("StaticScope");
        let solution = verify_static_scope(&s, &colocated_datums());

        assert!(
            solution.diagnostics.is_empty(),
            "a satisfied zero-auto relate block must be SILENT — no error, no \
             warning, no info. Got {:?}. A warning here would be the dropped \
             `W_RELATE_NO_AUTO` resurfacing.",
            solution
                .diagnostics
                .iter()
                .map(|d| (d.severity, d.code, d.message.clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 2,
                violated: 0,
                unverifiable: 0,
            }),
            "facts must be reported even when the scope is silent, so the ledger \
             can distinguish `verified: 2` from the absence of a relate block"
        );
    }

    /// (b) A zero-auto scope with two VIOLATED relations emits EXACTLY ONE
    /// aggregated Error naming both.
    ///
    /// The count is load-bearing, not stylistic. `dedup_diagnostics`
    /// (`crates/reify-cli/src/main.rs`) short-circuits on `d.code.is_some()`, so a
    /// CODED diagnostic is never collapsed at the CLI — a per-relation diagnostic
    /// would reach the user unfiltered, one line per relation. Hence the #5014
    /// collateral-observability shape: one Error per relate block naming the full
    /// violated set.
    #[test]
    fn verify_static_scope_aggregates_violations_into_one_error() {
        let s = scope("StaticScope");
        let solution = verify_static_scope(&s, &split_datums());

        assert_eq!(
            solution.diagnostics.len(),
            1,
            "two violated relations must produce ONE aggregated Error, never one \
             each — coded diagnostics bypass the CLI's dedup. Got {:?}",
            solution
                .diagnostics
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
        );
        let d = &solution.diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::RelateStaticViolated));
        // Content, not wording: WHICH relations were reported is the aggregation
        // contract. The phrasing is deliberately not pinned.
        assert!(
            d.message.contains("concentric") && d.message.contains("flush"),
            "the aggregate must name BOTH violated relations; got {:?}",
            d.message
        );
        // The measured figures are MAGNITUDES. Both dominant rows here are
        // negative as measured — `concentric`'s tangent-frame projection of the
        // −30 mm split, `flush`'s −5 mm along-normal offset — and the renderer used
        // to pass the signed value straight to `fmt_mm`, so the Error read "off by
        // −30 mm". A residual's sign is an artefact of operand order and tangent-
        // frame choice; "off by" is a distance from satisfaction and has no
        // direction to carry. `max_relation_residual` is the sibling convention
        // (it accumulates `r.abs()`).
        assert!(
            d.message.contains("off by 30 mm") && d.message.contains("off by 5 mm"),
            "both measured magnitudes must render unsigned, in mm; got {:?}",
            d.message
        );
        assert!(
            !d.message.contains("off by -"),
            "no `off by` clause may carry a sign; got {:?}",
            d.message
        );
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 0,
                violated: 2,
                unverifiable: 0,
            })
        );
    }

    /// (b′) A violation whose DOMINANT residual row is angular must not be
    /// rendered as a length.
    ///
    /// A residual row vector is not dimensionally homogeneous:
    /// `axis_coincidence_residual` returns two dimensionless TILT rows followed by
    /// two metre POSITION rows. Two axes that are CO-LOCATED but tilted are
    /// therefore violated entirely in the dimensionless block, and the earlier
    /// renderer — `fmt_mm(max |row|)` — turned that pure number into a confident
    /// millimetre figure that no measurement supports. Fabricating a length is
    /// precisely the class of claim this whole arm exists to kill, so it is pinned
    /// here rather than left to the reviewer who happens to read the output.
    ///
    /// Every other violated-path test uses axis-aligned datums, where the dominant
    /// row happens to be the metre one — which is why the bug survived them all.
    #[test]
    fn verify_static_scope_reports_an_angular_violation_without_claiming_mm() {
        let s = scope("StaticScope");
        // Axes share an origin and differ only in TILT (45°); planes are left
        // genuinely flush, so `concentric` is the only violated relation.
        let datums = realized(&[
            ("bush", "bore_axis", axis_v(SPLIT, (1.0, 0.0, 1.0))),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ("bush", "seat_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
            ("plate", "top_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
        ]);

        let solution = verify_static_scope(&s, &datums);

        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 1,
                violated: 1,
                unverifiable: 0,
            }),
            "tilted co-located axes violate `concentric` while the flush planes \
             still hold; got {:?}",
            solution.diagnostics
        );
        let d = solution
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
            .expect("a tilted concentric pair is a violation");

        // The measured magnitude is whatever follows "off by ". The DEMAND clause
        // legitimately says "0 mm" (that is the target, not a measurement), so the
        // assertion is scoped to the measured figure alone.
        let measured = d
            .message
            .split("off by ")
            .nth(1)
            .expect("a violated relation reports what it measured")
            .trim_end_matches(['"', '.']);
        assert!(
            !measured.contains("mm"),
            "the dominant row here is a dimensionless tilt component, which has no \
             length reading — reporting it in mm states a magnitude nothing \
             measured. Got {measured:?} from {:?}",
            d.message
        );
        assert!(
            measured.contains("dimensionless"),
            "an angular/direction violation must say what it actually measured; \
             got {measured:?}"
        );
    }

    /// (b″) A violation whose dominant row is an ORIENTATION delta renders in
    /// DEGREES — the only path that reaches `fmt_deg`.
    ///
    /// `fasten` (= `coincident` over Frame) is the one residual form that emits
    /// Angle rows: three metre origin-delta rows followed by three radian
    /// exponential-map rows. Two frames sharing a BIT-IDENTICAL origin and
    /// differing only in orientation therefore violate entirely in the angular
    /// block. Every other violated-path test lands on a metre or a dimensionless
    /// row, so without this one the degrees rendering never runs at all.
    ///
    /// `fasten` is reachable in a zero-auto scope both as written here and as the
    /// `ground(sub)` / `fix(sub)` desugar, so this is a user-visible rendering, not
    /// a defensive arm.
    #[test]
    fn verify_static_scope_reports_an_orientation_violation_in_degrees() {
        let s = scope("FastenScope");
        // Same origin (bitwise), bases 5° apart about +z.
        let datums = realized(&[
            ("bush", "frame", frame_v(SPLIT, 0.0)),
            ("plate", "frame", frame_v(SPLIT, 5.0)),
        ]);

        let solution = verify_static_scope(&s, &datums);

        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 0,
                violated: 1,
                unverifiable: 0,
            }),
            "co-located frames 5° apart violate `fasten`; got {:?}",
            solution.diagnostics
        );
        let d = solution
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
            .expect("a 5° orientation split is a violation");
        let measured = d
            .message
            .split("off by ")
            .nth(1)
            .expect("a violated relation reports what it measured")
            .trim_end_matches(['"', '.']);
        assert!(
            measured.starts_with("5°"),
            "the dominant row is a radian orientation delta of exactly 5°, so it \
             must render in degrees; got {measured:?} from {:?}",
            d.message
        );
        assert!(
            !measured.contains("mm") && !measured.contains("dimensionless"),
            "an orientation residual has neither a length reading nor a bare-number \
             one; got {measured:?}"
        );
    }

    /// The degree rendering always keeps a numeral — `"0°"`, never a bare `"°"`.
    ///
    /// [`super::fmt_deg`] formats to three decimals and then trims trailing zeros
    /// and a trailing point, which LOOKS like it could eat the whole numeral for a
    /// magnitude below 0.0005°. It cannot: `{:.3}` always emits a decimal point, so
    /// `trim_end_matches('0')` stops there and leaves the integer part. Measured
    /// rather than argued, and measured rather than "fixed" with a fallback branch
    /// that could never run — dead code whose behaviour nobody checks is how the
    /// `unwrap_or_else("satisfied")` fallback on the violated path came to render
    /// nonsense.
    #[test]
    fn fmt_deg_keeps_a_numeral_below_the_trim_threshold() {
        for radians in [0.0, 1e-9, 1e-6, 0.0004_f64.to_radians()] {
            let rendered = super::fmt_deg(radians);
            assert_eq!(
                rendered, "0°",
                "a sub-0.0005° magnitude must render as a zero with its unit, never \
                 as a bare unit; got {rendered:?} for {radians} rad"
            );
        }
        assert_eq!(super::fmt_deg(5.0_f64.to_radians()), "5°");
        assert_eq!(super::fmt_deg(0.25), "14.324°");
    }

    /// A correct `perpendicular` over NON-UNIT direction operands is VERIFIED, not
    /// failed.
    ///
    /// `perpendicular`'s residual is the dot product of its two operands, and
    /// `dir_of` does not normalize — an `Axis` carries whatever direction vector
    /// realization produced. Unnormalized, the row scales with BOTH magnitudes: the
    /// exactly-perpendicular pair below reads `10 × 10 = 100` against a 1e-5 rung
    /// and fails a build whose geometry is exactly right. The zero set is
    /// magnitude-invariant, so the solve path never noticed; the static arm turns
    /// the same row into a build-FAILING Error, which is what made this reachable.
    ///
    /// Normalizing first (as `angle` already did) makes the row the cosine of the
    /// misalignment — the scale-free number the dimensionless rung is defined
    /// against.
    #[test]
    fn verify_static_scope_holds_a_perpendicular_over_non_unit_directions() {
        let s = scope("PerpendicularScope");
        let datums = realized(&[
            ("bush", "bore_axis", axis_v(SPLIT, (10.0, 0.0, 0.0))),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 10.0))),
        ]);

        let solution = verify_static_scope(&s, &datums);

        assert!(
            solution.diagnostics.is_empty(),
            "exactly perpendicular axes are SATISFIED however long their direction \
             vectors are; got {:?}",
            solution
                .diagnostics
                .iter()
                .map(|d| (d.severity, d.message.clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 1,
                violated: 0,
                unverifiable: 0,
            })
        );
    }

    /// (c) UNVERIFIABLE, source 1 — an operand that did not realize.
    ///
    /// A Warning, not an Error: an unverifiable relation is *not consumed*, which
    /// is a different claim from *proven violated*. Erroring would fail builds
    /// that are green today on the strength of a measurement never taken — the
    /// never-false-Error hygiene mirroring R2's never-false-Inert. Whether the
    /// ledger escalates it is ζ (#5420)'s call, not this arm's.
    #[test]
    fn verify_static_scope_warns_when_an_operand_did_not_realize() {
        let s = scope("SingleRelationScope");
        let datums = realized(&[
            ("bush", "bore_axis", Value::Undef),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
        ]);
        let solution = verify_static_scope(&s, &datums);

        assert_eq!(solution.diagnostics.len(), 1);
        let d = &solution.diagnostics[0];
        assert_eq!(
            d.severity,
            Severity::Warning,
            "unverifiable is `not consumed`, not `proven violated`"
        );
        assert_eq!(d.code, Some(DiagnosticCode::RelateStaticUnverifiable));
        assert!(
            d.message
                .starts_with("relate: 1 relation could not be statically verified"),
            "the header must read grammatically at the SINGULAR cardinality. \
             Anchored at the start on purpose: a `contains` probe passes on \
             \"1 relation is could not be statically verified\" too. Got {:?}",
            d.message
        );
        assert!(
            d.message.contains("concentric") && d.message.contains("bush.bore_axis"),
            "the warning must name the relation AND the operand that did not \
             resolve, or the reader cannot act on it; got {:?}",
            d.message
        );
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 0,
                violated: 0,
                unverifiable: 1,
            })
        );
    }

    /// (c′) UNVERIFIABLE, source 1b — a relation with only ONE realized sub datum
    /// says so, and names the operand it could not compare against.
    ///
    /// The reachable shape is the `ground(sub)` / `fix(sub)` sugar, which the
    /// compiler desugars to `fasten(sub.frame, self.frame)`. `self.frame` is the
    /// ENCLOSING structure's own datum, not a sub datum: it lowers to a no-arg
    /// `MethodCall` that `decode_operand` rejects, so it never appears in
    /// `operand_refs` and the did-not-realize arm cannot see it. The relation
    /// reaches `static_relation_residuals` with one datum operand, trips its arity
    /// guard, and comes back with an empty row vector.
    ///
    /// Reading that emptiness as "there is no residual model for `fasten` over
    /// these operand kinds" was FALSE — `coincident_residual`'s Frame branch models
    /// exactly this pair, and would measure it the moment a second Frame were
    /// available. An unverifiable verdict's whole value is its reason, so a wrong
    /// reason undercuts the honest-non-consumption contract this arm exists to
    /// uphold (INV-SF-3).
    #[test]
    fn verify_static_scope_names_the_self_anchor_it_cannot_compare() {
        let s = scope("GroundedScope");
        let datums = realized(&[("bush", "frame", frame_v(SPLIT, 0.0))]);

        let solution = verify_static_scope(&s, &datums);

        assert_eq!(solution.diagnostics.len(), 1);
        let d = &solution.diagnostics[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::RelateStaticUnverifiable));
        assert!(
            d.message.contains("`self.frame`"),
            "the reason must name the operand that is not a realized sub datum; got \
             {:?}",
            d.message
        );
        assert!(
            !d.message.contains("no residual model"),
            "`fasten` over two Frames IS modelled — this relation simply has one \
             operand, not two. Reserving that wording for the genuinely unmodelled \
             case is the point of the arm; got {:?}",
            d.message
        );
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 0,
                violated: 0,
                unverifiable: 1,
            })
        );
    }

    /// (d) UNVERIFIABLE, source 2 — an operand belonging to a concretely-posed
    /// sub, even when the raw local-datum residual reads as SATISFIED.
    ///
    /// This is the sharpest test in the module, and the reason `RelateScope.posed`
    /// exists. The datums here are bit-identical, so a naive arm would measure
    /// zero and report the scope verified. But realized datums are each
    /// structure's LOCAL datum in its OWN identity frame, and `plate`'s declared
    /// `at transform3(…)` is never composed into them — so that zero was measured
    /// at the wrong configuration and means nothing. Reporting it as verified
    /// would be a confidently wrong verdict.
    ///
    /// Honest non-consumption beats a false verdict (INV-SF-3): say it is
    /// unverifiable, and say why (PRD §10 open question 5). Composing declared
    /// poses is the follow-up that would make this genuinely decidable.
    #[test]
    fn verify_static_scope_will_not_judge_a_posed_sub_even_when_it_would_pass() {
        let s = scope("PosedOperandScope");
        assert_eq!(
            s.posed,
            vec!["plate".to_string()],
            "fixture guard: this scope must actually carry a posed sub"
        );

        // Bit-identical operands: the RAW residual is exactly zero here.
        let datums = realized(&[
            ("bush", "bore_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
        ]);
        let solution = verify_static_scope(&s, &datums);

        assert_eq!(solution.diagnostics.len(), 1);
        let d = &solution.diagnostics[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::RelateStaticUnverifiable));
        assert!(
            d.message.contains("plate"),
            "the warning must name the posed sub whose placement is not composed \
             into the datums; got {:?}",
            d.message
        );
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 0,
                violated: 0,
                unverifiable: 1,
            }),
            "a posed-sub relation is neither verified nor violated — the residual \
             that would have read as satisfied was measured at the wrong \
             configuration and must not be counted"
        );
    }

    /// (e) A scope carrying BOTH a violation and an unverifiable relation emits
    /// exactly TWO diagnostics — one Error aggregate and one Warning aggregate —
    /// never one per relation.
    #[test]
    fn verify_static_scope_emits_one_aggregate_per_severity() {
        let s = scope("StaticScope");
        // concentric: violated by the 30 mm split. flush: bush's plane never
        // realized ⇒ unverifiable.
        let datums = realized(&[
            ("bush", "bore_axis", axis_v((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ("bush", "seat_plane", Value::Undef),
            ("plate", "top_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
        ]);
        let solution = verify_static_scope(&s, &datums);

        assert_eq!(
            solution.diagnostics.len(),
            2,
            "one Error aggregate + one Warning aggregate; got {:?}",
            solution
                .diagnostics
                .iter()
                .map(|d| (d.severity, d.message.clone()))
                .collect::<Vec<_>>()
        );
        let errors: Vec<_> = solution
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        let warnings: Vec<_> = solution
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert_eq!(errors[0].code, Some(DiagnosticCode::RelateStaticViolated));
        assert_eq!(
            warnings[0].code,
            Some(DiagnosticCode::RelateStaticUnverifiable)
        );
        assert!(errors[0].message.contains("concentric"));
        assert!(warnings[0].message.contains("flush"));
        assert_eq!(
            solution.static_facts,
            Some(StaticRelateFacts {
                verified: 0,
                violated: 1,
                unverifiable: 1,
            })
        );
    }

    /// Both aggregate headers read grammatically at BOTH cardinalities.
    ///
    /// Nothing else in this module can catch a malformed header: every other
    /// assertion here is a `contains("concentric")` / `contains("plate")`
    /// substring probe and the e2e filters on [`DiagnosticCode`], so word ORDER
    /// is entirely unpinned. That is how the unverifiable site came to emit
    /// "relate: 1 relation is could not be statically verified: …" unnoticed.
    /// These pins are `starts_with` on the header prefix rather than `contains`:
    /// only an ANCHORED prefix fails on a doubled or mis-ordered verb.
    ///
    /// Four pins across this test and (c), not one on the site that broke,
    /// because the defect class is "one shared helper is asked to serve two
    /// different verb forms" — pinning only the broken site lets the other drift
    /// back in unobserved.
    ///
    /// The expected strings are the HEADER PREFIXES of the canonical message forms
    /// documented on [`DiagnosticCode::RelateStaticViolated`] and
    /// [`DiagnosticCode::RelateStaticUnverifiable`], so no third wording is
    /// invented here. What is pinned is ONLY that prefix, up to the colon — the
    /// per-item tail those doc examples go on to show is not under test. Saying so
    /// is the point: the earlier claim that this "pins the code TO the doc" was
    /// read as a doc↔code binding, and under it BOTH examples drifted from the
    /// emitted text and stayed stale until a reader compared them by hand.
    #[test]
    fn verify_static_scope_headers_agree_in_number_at_both_sites() {
        // Each case below renders exactly one aggregate, so the sole message IS
        // the header under test.
        let sole_message = |solution: &super::RelateSolution| -> String {
            assert_eq!(
                solution.diagnostics.len(),
                1,
                "each case here must render exactly ONE aggregate, or the message \
                 picked below is not the one under test; got {:?}",
                solution
                    .diagnostics
                    .iter()
                    .map(|d| d.message.clone())
                    .collect::<Vec<_>>()
            );
            solution.diagnostics[0].message.clone()
        };

        // (b) PLURAL unverifiable. No existing fixture yields two unverifiable
        // relations, so both of `StaticScope`'s relations lose their `bush`
        // operand here.
        let plural_unverifiable = sole_message(&verify_static_scope(
            &scope("StaticScope"),
            &realized(&[
                ("bush", "bore_axis", Value::Undef),
                ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
                ("bush", "seat_plane", Value::Undef),
                ("plate", "top_plane", plane_v(SPLIT, (0.0, 0.0, 1.0))),
            ]),
        ));
        assert!(
            plural_unverifiable
                .starts_with("relate: 2 relations could not be statically verified"),
            "PLURAL unverifiable header must agree in number and carry no second \
             verb; got {plural_unverifiable:?}"
        );

        // (c) SINGULAR violated — `SingleRelationScope`'s one relation, measured
        // across the 30 mm split.
        let singular_violated = sole_message(&verify_static_scope(
            &scope("SingleRelationScope"),
            &realized(&[
                ("bush", "bore_axis", axis_v((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                ("plate", "boss_axis", axis_v(SPLIT, (0.0, 0.0, 1.0))),
            ]),
        ));
        assert!(
            singular_violated
                .starts_with("relate: 1 relation not satisfied by the subs' fixed placements"),
            "SINGULAR violated header; got {singular_violated:?}"
        );

        // (d) PLURAL violated — the split-datum pair, both relations false.
        let plural_violated =
            sole_message(&verify_static_scope(&scope("StaticScope"), &split_datums()));
        assert!(
            plural_violated
                .starts_with("relate: 2 relations not satisfied by the subs' fixed placements"),
            "PLURAL violated header; got {plural_violated:?}"
        );
    }

    /// (f) The rendering is deterministic and follows SOURCE order.
    ///
    /// Two invocations over the same inputs must produce byte-identical messages
    /// — the aggregate is built by walking `scope.relations` in order, never by
    /// iterating a `HashMap`. Source order also decides the listing order within
    /// an aggregate, so `concentric` (declared first) precedes `flush`.
    #[test]
    fn verify_static_scope_renders_deterministically_in_source_order() {
        let s = scope("StaticScope");
        let first = verify_static_scope(&s, &split_datums());
        let second = verify_static_scope(&s, &split_datums());

        let msgs = |sol: &super::RelateSolution| {
            sol.diagnostics
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            msgs(&first),
            msgs(&second),
            "two invocations over identical inputs must render identically — any \
             difference means a HashMap iteration order leaked into the message"
        );

        let m = &first.diagnostics[0].message;
        let c = m.find("concentric").expect("concentric must be named");
        let f = m.find("flush").expect("flush must be named");
        assert!(
            c < f,
            "relations must be listed in SOURCE order (concentric is declared \
             first); got {m:?}"
        );
    }
}
