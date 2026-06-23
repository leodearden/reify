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
use reify_core::{Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, ExportFormat, Value};

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
