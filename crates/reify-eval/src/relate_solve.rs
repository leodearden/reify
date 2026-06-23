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
    FrameUnknown, Operand, Pose, RelateTolerance, RelationInstance, max_relation_residual,
    partition_driving_set, pose_from_frame, solve_frame,
};
use reify_core::{Diagnostic, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, ExportFormat, SolveResult, Value};

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
    /// `false` for bare `at auto` (strict — a residual DOF is an error);
    /// `true` for `at auto(free)` (a residual DOF is gauge-seeded, not an error).
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
/// * [`relations`](Self::relations) — the flat, source-ordered relation set
///   (each a `FunctionCall` retaining its name + operand exprs); and
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
/// No solve is performed here — this is pure structural classification.
pub fn collect_relate_scope(template: &TopologyTemplate) -> RelateScope {
    let mut auto_unknowns = Vec::new();
    let mut ground = Vec::new();

    for sub in &template.sub_components {
        match &sub.auto_pose {
            Some(spec) => auto_unknowns.push(AutoUnknown {
                sub: sub.name.clone(),
                free: spec.free,
                seed_params: spec.params.clone(),
            }),
            None => ground.push(sub.name.clone()),
        }
    }

    RelateScope {
        auto_unknowns,
        relations: template.relations.clone(),
        ground,
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

    // 1. Decode every relation operand into its (sub, structure, member) ref.
    let refs: Vec<OperandRef> = scope
        .relations
        .iter()
        .flat_map(|rel| match &rel.kind {
            CompiledExprKind::FunctionCall { args, .. } => {
                args.iter().filter_map(decode_operand).collect::<Vec<_>>()
            }
            _ => Vec::new(),
        })
        .collect();

    if refs.is_empty() {
        return RealizedDatums::default();
    }

    // 2. Build the union closure of all referenced structures ONCE (single-shot):
    //    a filtered module carrying just those structures (+ their sub-structure
    //    deps), built standalone so each structure's local datums realize in its
    //    own identity frame — independent of any assembly pose.
    let mut keep = HashSet::new();
    for r in &refs {
        keep.extend(structure_closure(&r.structure, module));
    }
    let mut sub_module = module.clone();
    sub_module.templates.retain(|t| keep.contains(&t.name));
    let values = engine.build(&sub_module, ExportFormat::Step).values;

    // 3. Resolve each operand to its realized local datum. A structure's local
    //    datum cell is keyed `ValueCellId { entity: <Struct>, member: <datum> }`.
    let mut operands = HashMap::new();
    for r in refs {
        let datum = values
            .get(&ValueCellId::new(&r.structure, &r.member))
            .cloned()
            .unwrap_or(Value::Undef);
        operands.insert((r.sub, r.member), datum);
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

    // 1. Build a RelationInstance per relation over the realized datums.
    let instances = build_relation_instances(scope, realized);

    // 2. Partition at the witness into driving + redundant; the rank-revealing
    //    tolerance is tied to the solver-convergence tol (design §4).
    let partition =
        partition_driving_set(&instances, &frame_unknown, &seed, tol.solver_convergence());

    let mut solution = RelateSolution {
        spent: partition.spent,
        free: partition.free,
        driving: partition.driving.len(),
        redundant: partition.redundant.len(),
        ..RelateSolution::default()
    };

    // 3. Solve ONLY the driving set for the auto Frame.
    let driving_rels: Vec<RelationInstance> =
        partition.driving.iter().map(|&i| instances[i].clone()).collect();
    let result = solve_frame(&driving_rels, &frame_unknown, &seed, tol.solver_convergence());

    let solved_pose = match result {
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
                &instances,
                &partition.driving,
                &frame_unknown,
                &seed,
                tol.solver_convergence(),
            );
            solution.diagnostics.push(conflict_diagnostic(
                &conflict,
                scope,
                &instances,
                &frame_unknown.sub,
            ));
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
            let r_ops = operand_refs(&scope.relations[i]);
            let colocated: Vec<usize> = partition
                .driving
                .iter()
                .copied()
                .filter(|&d| {
                    operand_refs(&scope.relations[d]).iter().any(|o| r_ops.contains(o))
                })
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
                    scope,
                    &instances,
                    &frame_unknown.sub,
                ));
            }
        }
    }

    solution
}

/// Run the per-scope relate-solve for every scope in `module` that has at least
/// one `at auto` sub AND at least one relation (ζ step-18 — the build-pass entry).
///
/// For each qualifying scope this collects ([`collect_relate_scope`]), realizes
/// its operand datums single-shot ([`realize_operand_datums`]), and runs the full
/// partition → solve → verify pipeline ([`solve_relate_scope`]); it returns one
/// `(scope_name, RelateSolution)` per solved scope so the build pass can write each
/// solved Frame back into the value map (keyed by [`auto_pose_cell`]) and surface
/// the verification diagnostics (an `Error` fails the build).
///
/// Scopes with no `at auto` sub OR no relation are skipped before any realization
/// — nothing to solve, and the skip keeps a kernel sub-build off the hot path for
/// the overwhelmingly common non-relate scope.
///
/// **Single-level recursion.** Realization sub-builds each referenced structure
/// through `engine` ([`realize_operand_datums`] filters the module to the operand
/// structures' closure). ζ's grounding model keeps those leaf structures free of
/// `at auto` / relations, so the sub-build's own `solve_scopes` finds nothing and
/// does not recurse further. The caller MUST invoke this BEFORE the outer build's
/// own state resets so the transient sub-build state is re-established by the main
/// `check()` that follows.
pub fn solve_scopes(
    module: &CompiledModule,
    engine: &mut Engine,
) -> Vec<(String, RelateSolution)> {
    let mut out = Vec::new();
    for template in &module.templates {
        let scope = collect_relate_scope(template);
        if scope.auto_unknowns.is_empty() || scope.relations.is_empty() {
            continue;
        }
        // Local datums are pose-independent (single-shot), so the seed estimate is
        // empty here — `realize_operand_datums` ignores it and `solve_relate_scope`
        // witnesses at identity (the grounded anchor's datums encode the target).
        let seeds = HashMap::new();
        let realized = realize_operand_datums(&scope, module, engine, &seeds);
        let solution = solve_relate_scope(&scope, &realized);
        out.push((template.name.clone(), solution));
    }
    out
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
fn build_relation_instances(
    scope: &RelateScope,
    realized: &RealizedDatums,
) -> Vec<RelationInstance> {
    scope
        .relations
        .iter()
        .filter_map(|rel| {
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
            }
            Some(RelationInstance {
                name: function.name.clone(),
                operands,
                nominal_delta_dof: None,
            })
        })
        .collect()
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
    let scalar = inst
        .operands
        .iter()
        .find(|o| o.sub.is_none())
        .and_then(|o| o.datum.as_f64());
    match inst.name.as_str() {
        "concentric" | "coincident" => "coincident (0 mm apart)".to_string(),
        "flush" => "coplanar (flush, 0 mm offset)".to_string(),
        "parallel" => "parallel".to_string(),
        "antiparallel" => "anti-parallel".to_string(),
        "perpendicular" => "perpendicular".to_string(),
        "on" => "incident".to_string(),
        "tangent" => "tangent".to_string(),
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
/// `conflict` are the source indices (into `scope.relations` / `instances`) of the
/// mutually-inconsistent relations. The NEWEST-declared member — the highest source
/// index, since the flat relation set preserves source/declaration order — is flagged
/// as the **primary** conflict (PRD §7.1: newest member is the likely culprit). The
/// explanation is purely geometric — each relation's demand + its magnitude in mm /
/// degrees — and never mentions the solver or libslvs (ζ's diagnostics speak
/// geometry; θ #4388 renders the polished `reify explain` ledger / spans / badge from
/// the same data).
fn conflict_diagnostic(
    conflict: &[usize],
    scope: &RelateScope,
    instances: &[RelationInstance],
    auto_sub: &str,
) -> Diagnostic {
    // primary = newest-declared = highest source index.
    let primary = conflict.iter().copied().max().unwrap_or(0);
    let mut others: Vec<usize> = conflict.iter().copied().filter(|&i| i != primary).collect();
    others.sort_unstable();

    let primary_name = &instances[primary].name;
    let primary_subjects = describe_operands(&scope.relations[primary]);
    let primary_demand = describe_demand(&instances[primary]);

    let mut msg = format!(
        "conflicting relations on `{auto_sub}`: `{primary_name}` requires \
         {primary_subjects} {primary_demand}"
    );
    for &o in &others {
        msg.push_str(&format!(
            ", but `{}` requires them {}",
            instances[o].name,
            describe_demand(&instances[o]),
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
/// ≥3 relations), falls back to the whole driving set. Returns source indices (into
/// `scope.relations` / `instances`). Bounded by the driving-set size, which is `≤ 6`
/// for a single Frame unknown — the pairwise search is cheap.
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
