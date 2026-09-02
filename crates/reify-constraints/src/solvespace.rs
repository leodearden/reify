//! SolveSpace geometric constraint solver integration.
//!
//! Implements `ConstraintSolver` using the SolveSpace libslvs C library
//! via hand-written FFI bindings with newtype-wrapped handles.  Creates
//! a fresh solver system per call (stateless), making it trivially
//! Send + Sync.

#[cfg(slvs_not_found)]
compile_error!(
    "libslvs not found. Install with: sudo apt install libslvs1-dev \
     or set SLVS_LIB_DIR to the directory containing libslvs.so"
);

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, SourceSpan, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, ResolutionProblem,
    SolveResult, Value, ValueMap,
};

use crate::sketch::{
    SketchBuildError, SketchConstraint, SketchConstraintDef, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchHandleMap, SketchSolveResult, SketchSystem,
};
use crate::slvs_sys::{
    self, SLVS_C_ANGLE, SLVS_C_ARC_LINE_TANGENT, SLVS_C_AT_MIDPOINT, SLVS_C_CURVE_CURVE_TANGENT,
    SLVS_C_DIAMETER, SLVS_C_EQUAL_LENGTH_LINES, SLVS_C_EQUAL_RADIUS, SLVS_C_HORIZONTAL,
    SLVS_C_PARALLEL, SLVS_C_PERPENDICULAR, SLVS_C_POINTS_COINCIDENT, SLVS_C_PT_ON_CIRCLE,
    SLVS_C_PT_ON_LINE, SLVS_C_PT_PT_DISTANCE, SLVS_C_SYMMETRIC_LINE, SLVS_C_VERTICAL,
    SLVS_C_WHERE_DRAGGED, SLVS_FREE_IN_3D, SLVS_RESULT_DIDNT_CONVERGE, SLVS_RESULT_INCONSISTENT,
    SLVS_RESULT_OKAY, SLVS_RESULT_TOO_MANY_UNKNOWNS, Slvs_Constraint, Slvs_Entity, Slvs_Param,
    Slvs_System, Slvs_hConstraint, Slvs_hEntity, Slvs_hGroup, Slvs_hParam,
};

/// Global mutex to serialize access to the libslvs solver.
///
/// SolveSpace's library code uses global mutable state internally
/// (e.g. the `SS` static sketch object), so concurrent calls to
/// `Slvs_Solve` cause data races and crashes. This mutex ensures
/// only one solve runs at a time.
static SLVS_LOCK: Mutex<()> = Mutex::new(());

/// Geometric constraint solver backed by SolveSpace's libslvs.
///
/// Solves geometric constraints (point distances, angles, parallelism,
/// coincidence, etc.) by mapping Reify's `ResolutionProblem` to libslvs
/// entities and constraints, solving, then reading back results.
///
/// A fresh `Slvs_System` is created per `solve()` call — no internal
/// mutable state — so this type is `Send + Sync`. Thread safety is
/// ensured by a global mutex around `Slvs_Solve` calls, since libslvs
/// uses internal global state.
pub struct SolveSpaceSolver;

// ---------------------------------------------------------------------------
// Pattern recognition
// ---------------------------------------------------------------------------

/// A recognized geometric constraint pattern extracted from a `CompiledExpr` tree.
#[derive(Debug)]
enum GeometricPattern {
    /// distance(point_a, point_b) == value_si
    PtPtDistance {
        pt_a: PointRef,
        pt_b: PointRef,
        distance_si: f64,
    },
    /// angle(line_a, line_b) == angle_deg
    ///
    /// `angle_deg` is POST-CONVERSION DEGREES, not radians. The pattern
    /// deliberately stores the SolveSpace-side unit so the value that reaches
    /// `add_constraint_wrkpl` needs no further thought — see the marshalling
    /// boundary in [`try_angle_eq`] for the crossing itself (#6184).
    Angle {
        line_a: LineRef,
        line_b: LineRef,
        angle_deg: f64,
    },
    /// parallel(line_a, line_b)
    Parallel { line_a: LineRef, line_b: LineRef },
    /// perpendicular(line_a, line_b)
    Perpendicular { line_a: LineRef, line_b: LineRef },
    /// coincident(point_a, point_b)  OR  distance(point_a, point_b) == 0
    Coincident { pt_a: PointRef, pt_b: PointRef },
}

/// A point reference: either auto params or fixed coordinates.
#[derive(Debug, Clone)]
enum PointRef {
    /// Auto params: (x_cell_id, y_cell_id, z_cell_id_or_fixed)
    Auto {
        x: Option<ValueCellId>,
        y: Option<ValueCellId>,
        z: Option<ValueCellId>,
    },
    /// Fixed literal coordinates in SI units.
    Fixed { x: f64, y: f64, z: f64 },
}

impl PointRef {
    /// Returns true if this is a 2D point (Auto with z=None, or Fixed with z=0).
    fn is_2d(&self) -> bool {
        match self {
            PointRef::Auto { z, .. } => z.is_none(),
            PointRef::Fixed { z, .. } => *z == 0.0,
        }
    }
}

/// A line reference: two points.
#[derive(Debug, Clone)]
struct LineRef {
    start: PointRef,
    end: PointRef,
}

/// Try to recognize a geometric constraint pattern from an expression tree.
///
/// **Superseded for 2D sketches only** (`docs/prds/v0_6/constrained-2d-sketch.md`,
/// D8).  A sketch no longer arrives as an expression tree to be pattern-matched:
/// it arrives as a typed `SketchSystem` and is lowered declaration-by-declaration
/// by `add_sketch` behind `solve_sketch`.  Guessing a constraint's meaning back
/// out of an expression is inherently partial — an unrecognized shape reports
/// `NoProgress` — where direct lowering cannot fail to understand what the caller
/// declared.
///
/// This route stays live regardless: it serves the registry's auto-param path
/// (`ConstraintSolver::solve` over a `ResolutionProblem`), whose input really is
/// expressions and which has no `SketchSystem` to hand over.  Nothing here is
/// deprecated, and nothing here changed.
///
/// Consolidating the three geometric solvers now in the crate — relate-solve's
/// Gauss–Newton, this pattern path, and the sketch path — is explicitly out of
/// scope (PRD §11, "Solver consolidation"): breadcrumbs only, no unification.
fn recognize_pattern(expr: &CompiledExpr, auto_params: &[AutoParam]) -> Option<GeometricPattern> {
    match &expr.kind {
        // eq(distance_call, literal) or eq(literal, distance_call)
        CompiledExprKind::BinOp {
            op: BinOp::Eq,
            left,
            right,
        } => {
            // Try: left is fn call, right is literal
            if let Some(pat) = try_distance_eq(left, right, auto_params) {
                return Some(pat);
            }
            // Try: right is fn call, left is literal
            if let Some(pat) = try_distance_eq(right, left, auto_params) {
                return Some(pat);
            }
            // Try angle eq
            if let Some(pat) = try_angle_eq(left, right, auto_params) {
                return Some(pat);
            }
            if let Some(pat) = try_angle_eq(right, left, auto_params) {
                return Some(pat);
            }
            None
        }
        // Top-level function call (boolean constraints like parallel, perpendicular, coincident)
        CompiledExprKind::FunctionCall { function, args } => {
            let qn = &function.qualified_name;
            if qn.contains("parallel") {
                try_line_pair_constraint(args, auto_params).map(|(a, b)| {
                    GeometricPattern::Parallel {
                        line_a: a,
                        line_b: b,
                    }
                })
            } else if qn.contains("perpendicular") {
                try_line_pair_constraint(args, auto_params).map(|(a, b)| {
                    GeometricPattern::Perpendicular {
                        line_a: a,
                        line_b: b,
                    }
                })
            } else if qn.contains("coincident") {
                if args.len() == 2 {
                    let pt_a = extract_point_ref(&args[0], auto_params)?;
                    let pt_b = extract_point_ref(&args[1], auto_params)?;
                    Some(GeometricPattern::Coincident { pt_a, pt_b })
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Try to match: fn_call == scalar_literal as a distance constraint.
fn try_distance_eq(
    fn_expr: &CompiledExpr,
    val_expr: &CompiledExpr,
    auto_params: &[AutoParam],
) -> Option<GeometricPattern> {
    if let CompiledExprKind::FunctionCall { function, args } = &fn_expr.kind {
        let qn = &function.qualified_name;
        if (qn.contains("distance") || qn.contains("pt_pt_distance")) && args.len() == 2 {
            let pt_a = extract_point_ref(&args[0], auto_params)?;
            let pt_b = extract_point_ref(&args[1], auto_params)?;
            let distance_si = extract_scalar_si(val_expr)?;
            // Exact zero in SI metres — NOT a geometric tolerance.
            // Any non-zero distance, however small, uses PtPtDistance.
            if distance_si.abs() < 1e-15 {
                return Some(GeometricPattern::Coincident { pt_a, pt_b });
            }
            return Some(GeometricPattern::PtPtDistance {
                pt_a,
                pt_b,
                distance_si,
            });
        }
    }
    None
}

/// Try to match: fn_call == scalar_literal as an angle constraint.
fn try_angle_eq(
    fn_expr: &CompiledExpr,
    val_expr: &CompiledExpr,
    auto_params: &[AutoParam],
) -> Option<GeometricPattern> {
    if let CompiledExprKind::FunctionCall { function, args } = &fn_expr.kind {
        let qn = &function.qualified_name;
        if qn.contains("angle") && args.len() == 2 {
            let line_a = extract_line_ref(&args[0], auto_params)?;
            let line_b = extract_line_ref(&args[1], auto_params)?;
            // THE ANGULAR MARSHALLING BOUNDARY (INV-AD-4; #6184).
            //
            // Everything UPSTREAM of this line is SI RADIANS: `extract_scalar_si`
            // yields the SI-coherent magnitude of an Angle scalar, and reify's
            // DSL/IR are radians throughout (rad = 1 by SI coherence).
            //
            // Precisely: only ONE of `extract_scalar_si`'s three arms is
            // dimensioned. A `Value::Real`/`Value::Int` RHS (`angle(a, b) == 0.5`)
            // carries no `DimensionVector` at all, so nothing about it is
            // SI-coherent by construction — it is taken verbatim and therefore
            // INTERPRETED as radians here, which is the same convention the rest
            // of the tree applies to an undimensioned angular magnitude. Whether
            // the type checker admits a dimensionless RHS against an
            // `Angle`-typed `angle(...)` call is deliberately NOT relied on: if
            // it does, the value is radians by that rule; if it does not, those
            // two arms are simply unreachable on this path. Either way the
            // boundary below reads radians.
            //
            // Everything DOWNSTREAM is DEGREES: libslvs' `SLVS_C_ANGLE` reads
            // its `valA` in degrees, which is the one genuine degree crossing in
            // the tree — every other IO boundary reify has is SI.
            //
            // So `to_degrees()` here IS the boundary, and the `angle_rad` ->
            // `angle_deg` rename across it is the contract made visible in the
            // names. Nothing downstream is in radians; nothing upstream is in
            // degrees.
            //
            // Same convention, already declared, at the two sibling crossings:
            // the `SketchConstraint::Angle` arm below ("Degrees, not radians:
            // `SLVS_C_ANGLE` reads `valA` in degrees"), and the
            // `SketchConstraint` enum doc in `crates/reify-constraints/src/sketch.rs`.
            let angle_rad = extract_scalar_si(val_expr)?;
            let angle_deg = angle_rad.to_degrees();
            return Some(GeometricPattern::Angle {
                line_a,
                line_b,
                angle_deg,
            });
        }
    }
    None
}

/// Try to extract a line pair for parallel/perpendicular constraints.
fn try_line_pair_constraint(
    args: &[CompiledExpr],
    auto_params: &[AutoParam],
) -> Option<(LineRef, LineRef)> {
    if args.len() == 2 {
        let line_a = extract_line_ref(&args[0], auto_params)?;
        let line_b = extract_line_ref(&args[1], auto_params)?;
        Some((line_a, line_b))
    } else {
        None
    }
}

/// Extract a PointRef from an expression.
///
/// Handles:
/// - FunctionCall("point3d", [x, y, z]): extracts coords from args
/// - ValueRef to an auto param: treats as a single-dimension point (x only)
fn extract_point_ref(expr: &CompiledExpr, auto_params: &[AutoParam]) -> Option<PointRef> {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            let qn = &function.qualified_name;
            if (qn.contains("point3d") || qn.contains("point")) && args.len() >= 2 {
                let x = extract_coord(&args[0], auto_params)?;
                let y = extract_coord(&args[1], auto_params)?;
                let z = if args.len() >= 3 {
                    extract_coord(&args[2], auto_params)?
                } else {
                    CoordRef::Fixed(0.0) // 2D point: z defaults to 0
                };
                return Some(make_point_ref(x, y, z));
            }
            None
        }
        // A bare ValueRef could be a point auto param
        CompiledExprKind::ValueRef(id) => {
            if is_auto_param(id, auto_params) {
                Some(PointRef::Auto {
                    x: Some(id.clone()),
                    y: None,
                    z: None,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract a LineRef from a line_segment expression or two-point expression.
fn extract_line_ref(expr: &CompiledExpr, auto_params: &[AutoParam]) -> Option<LineRef> {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            let qn = &function.qualified_name;
            if (qn.contains("line") || qn.contains("line_segment")) && args.len() == 2 {
                let start = extract_point_ref(&args[0], auto_params)?;
                let end = extract_point_ref(&args[1], auto_params)?;
                return Some(LineRef { start, end });
            }
            // Also handle direct point pair for angle constraints
            if args.len() == 2
                && let (Some(start), Some(end)) = (
                    extract_point_ref(&args[0], auto_params),
                    extract_point_ref(&args[1], auto_params),
                )
            {
                return Some(LineRef { start, end });
            }
            None
        }
        _ => None,
    }
}

/// A single coordinate is either a ValueRef (auto param) or a literal.
enum CoordRef {
    Auto(ValueCellId),
    Fixed(f64),
}

fn extract_coord(expr: &CompiledExpr, auto_params: &[AutoParam]) -> Option<CoordRef> {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) if is_auto_param(id, auto_params) => {
            Some(CoordRef::Auto(id.clone()))
        }
        CompiledExprKind::Literal(val) => Some(CoordRef::Fixed(val.as_f64()?)),
        _ => None,
    }
}

fn make_point_ref(x: CoordRef, y: CoordRef, z: CoordRef) -> PointRef {
    match (&x, &y, &z) {
        (CoordRef::Fixed(fx), CoordRef::Fixed(fy), CoordRef::Fixed(fz)) => PointRef::Fixed {
            x: *fx,
            y: *fy,
            z: *fz,
        },
        _ => PointRef::Auto {
            x: match x {
                CoordRef::Auto(id) => Some(id),
                CoordRef::Fixed(_) => None,
            },
            y: match y {
                CoordRef::Auto(id) => Some(id),
                CoordRef::Fixed(_) => None,
            },
            z: match z {
                CoordRef::Auto(id) => Some(id),
                CoordRef::Fixed(_) => None,
            },
        },
    }
}

/// Extract a scalar SI value from a literal expression.
fn extract_scalar_si(expr: &CompiledExpr) -> Option<f64> {
    match &expr.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => Some(*si_value),
        CompiledExprKind::Literal(Value::Real(v)) => Some(*v),
        CompiledExprKind::Literal(Value::Int(v)) => Some(*v as f64),
        _ => None,
    }
}

fn is_auto_param(id: &ValueCellId, auto_params: &[AutoParam]) -> bool {
    auto_params.iter().any(|ap| ap.id == *id)
}

// ---------------------------------------------------------------------------
// Solver core
// ---------------------------------------------------------------------------

/// Allocator for slvs handles (params, entities, constraints).
///
/// Each handle type is a distinct newtype, preventing accidental mixing
/// of param handles with entity handles at compile time.
struct HandleAlloc {
    next_param: Slvs_hParam,
    next_entity: Slvs_hEntity,
    next_constraint: Slvs_hConstraint,
    next_group: Slvs_hGroup,
}

impl HandleAlloc {
    fn new() -> Self {
        Self {
            next_param: Slvs_hParam(1),
            next_entity: Slvs_hEntity(1),
            next_constraint: Slvs_hConstraint(1),
            // 1 and 2 are reserved for FIXED_GROUP and the legacy SOLVE_GROUP,
            // so the legacy route's group numbering is untouched.
            next_group: Slvs_hGroup(3),
        }
    }

    /// Allocate a fresh solve group.
    ///
    /// `Slvs_Solve(sys, hg)` varies only params whose group is `hg` and treats
    /// every other param as a constant, so a group is the unit of "what this
    /// solve is allowed to move".  Allocation is sequential, which keeps group
    /// ids — like entity and constraint handles — a deterministic function of
    /// the input.
    fn group(&mut self) -> Slvs_hGroup {
        let h = self.next_group;
        self.next_group.0 += 1;
        h
    }

    fn param(&mut self) -> Slvs_hParam {
        let h = self.next_param;
        self.next_param.0 += 1;
        h
    }

    fn entity(&mut self) -> Slvs_hEntity {
        let h = self.next_entity;
        self.next_entity.0 += 1;
        h
    }

    fn constraint(&mut self) -> Slvs_hConstraint {
        let h = self.next_constraint;
        self.next_constraint.0 += 1;
        h
    }
}

/// Maps between Reify ValueCellIds and slvs parameter handles.
struct ParamMapping {
    /// ValueCellId -> slvs param handle
    cell_to_param: HashMap<ValueCellId, Slvs_hParam>,
    /// slvs param handle -> ValueCellId
    param_to_cell: HashMap<Slvs_hParam, ValueCellId>,
}

impl ParamMapping {
    fn new() -> Self {
        Self {
            cell_to_param: HashMap::new(),
            param_to_cell: HashMap::new(),
        }
    }

    fn insert(&mut self, cell_id: ValueCellId, param_h: Slvs_hParam) {
        self.cell_to_param.insert(cell_id.clone(), param_h);
        self.param_to_cell.insert(param_h, cell_id);
    }

    fn get_param(&self, cell_id: &ValueCellId) -> Option<Slvs_hParam> {
        self.cell_to_param.get(cell_id).copied()
    }
}

/// Error produced by the internal builder call chain
/// (`add_auto_coord` → `add_point` → `add_pattern_to_builder`).
///
/// Carries the `cell_id` as a structured field so it can be logged
/// separately by the `solve()` call site, and a human-readable `message`.
/// Implements `std::error::Error` so it can be propagated with `?` or
/// wrapped by any conforming error-aggregation library.
// DO NOT derive Clone — ValueCellId holds two String fields and nothing clones BuilderError.
#[derive(Debug)]
struct BuilderError {
    cell_id: ValueCellId,
    message: String,
}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BuilderError {}

/// Builder that accumulates slvs params/entities/constraints.
///
/// Uses two groups:
/// - `FIXED_GROUP` (1): for fixed/reference params and entities that shouldn't be varied
/// - `SOLVE_GROUP` (2): for auto params, their entities, and constraints to solve
struct SystemBuilder {
    alloc: HandleAlloc,
    params: Vec<Slvs_Param>,
    entities: Vec<Slvs_Entity>,
    constraints: Vec<Slvs_Constraint>,
    mapping: ParamMapping,
    /// Track which entities are already created for points.
    point_entities: HashMap<PointKey, Slvs_hEntity>,
    /// Lazily-created XY workplane entity handle for 2D constraints.
    workplane: Option<Slvs_hEntity>,
    /// Lazily-created in-plane normal, shared by every circle and arc.
    ///
    /// libslvs requires a normal entity per circle/arc, but in a 2D sketch they
    /// all lie in the one workplane, so one instance serves all of them.
    sketch_normal: Option<Slvs_hEntity>,
    /// The group `solve()` hands to `Slvs_Solve` — i.e. the params it may vary.
    ///
    /// Defaults to `SOLVE_GROUP` so the legacy pattern-recognition route is
    /// unchanged; `add_sketch` repoints it at the group it allocated for the
    /// sketch.
    solve_group: Slvs_hGroup,
    /// The sketch declaration currently being lowered, if any.
    ///
    /// Held on the builder rather than threaded through each emit call so that
    /// attribution is automatic: every slvs constraint allocated while this is
    /// set is recorded against that declaration, including the several a single
    /// `Fix` on a line expands into.  A per-call-site `record(...)` would be one
    /// forgotten line away from an unattributable failure.
    ///
    /// `None` outside `add_sketch`, which is what keeps the legacy route's
    /// constraints out of the map entirely.
    attributing: Option<(SketchConstraintId, SourceSpan)>,
    /// Reverse index from emitted slvs constraint handle to the sketch
    /// declaration it came from.  Handed to the `SketchHandleMap` on the way
    /// out of `add_sketch`.
    constraint_attribution: HashMap<Slvs_hConstraint, (SketchConstraintId, SourceSpan)>,
}

const FIXED_GROUP: Slvs_hGroup = Slvs_hGroup(1);
const SOLVE_GROUP: Slvs_hGroup = Slvs_hGroup(2);

/// Key to deduplicate point entities.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum PointKey {
    Auto(
        Option<ValueCellId>,
        Option<ValueCellId>,
        Option<ValueCellId>,
    ),
    Fixed(u64, u64, u64), // f64 bits for hashing
}

/// Return type of [`SystemBuilder::add_line_pair`].
///
/// Uses named fields rather than a bare tuple so callers cannot
/// accidentally swap the two handles (which are the same type and
/// therefore indistinguishable positionally).
#[derive(Debug, Clone, Copy)]
struct LinePairEntities {
    line_a: Slvs_hEntity,
    line_b: Slvs_hEntity,
}

impl SystemBuilder {
    fn new() -> Self {
        Self {
            alloc: HandleAlloc::new(),
            params: Vec::new(),
            entities: Vec::new(),
            constraints: Vec::new(),
            mapping: ParamMapping::new(),
            point_entities: HashMap::new(),
            workplane: None,
            sketch_normal: None,
            solve_group: SOLVE_GROUP,
            attributing: None,
            constraint_attribution: HashMap::new(),
        }
    }

    /// Add or retrieve a point entity from a PointRef.
    ///
    /// # Errors
    ///
    /// Returns `Err(BuilderError)` if any coordinate cell_id is a non-auto param
    /// absent from `current_values` (propagated from `add_auto_coord`).
    fn add_point(
        &mut self,
        pt: &PointRef,
        auto_params: &[AutoParam],
        current_values: &ValueMap,
    ) -> Result<Slvs_hEntity, BuilderError> {
        let key = point_key(pt);
        if let Some(&h) = self.point_entities.get(&key) {
            return Ok(h);
        }

        match pt {
            PointRef::Fixed { x, y, z } => {
                // Fixed points go in FIXED_GROUP so solver won't vary them
                let px = self.alloc.param();
                let py = self.alloc.param();
                let pz = self.alloc.param();
                self.params.push(Slvs_Param::new(px, FIXED_GROUP, *x));
                self.params.push(Slvs_Param::new(py, FIXED_GROUP, *y));
                self.params.push(Slvs_Param::new(pz, FIXED_GROUP, *z));
                let eh = self.alloc.entity();
                self.entities
                    .push(Slvs_Entity::point_3d(eh, FIXED_GROUP, px, py, pz));
                self.point_entities.insert(key, eh);
                Ok(eh)
            }
            PointRef::Auto {
                x: x_id,
                y: y_id,
                z: z_id,
            } => {
                let px = self.add_auto_coord(x_id, auto_params, current_values)?;
                let py = self.add_auto_coord(y_id, auto_params, current_values)?;
                let eh = self.alloc.entity();
                if z_id.is_none() {
                    // 2D point: use POINT_IN_2D on the XY workplane.
                    // This has only 2 params (u, v) — z is implicitly 0,
                    // so the solver cannot vary it.
                    let wp = self.get_workplane();
                    self.entities
                        .push(Slvs_Entity::point_2d(eh, SOLVE_GROUP, wp, px, py));
                } else {
                    let pz = self.add_auto_coord(z_id, auto_params, current_values)?;
                    self.entities
                        .push(Slvs_Entity::point_3d(eh, SOLVE_GROUP, px, py, pz));
                }
                self.point_entities.insert(key, eh);
                Ok(eh)
            }
        }
    }

    /// Add a param for an auto coordinate. If the cell_id is Some and is an auto param,
    /// map it to SOLVE_GROUP; otherwise add a param in SOLVE_GROUP with its fixed value.
    ///
    /// All params within Auto points go into SOLVE_GROUP to avoid mixed-group
    /// Jacobian rank issues in libslvs. "Fixed" coordinates (no cell_id or
    /// non-auto cell_id) are initialized to their value but not mapped, so
    /// their solved values are ignored in the output.
    ///
    /// For 2D points (z=None), `add_point` uses POINT_IN_2D entities that
    /// have only 2 params; this method is called only for x and y in that case.
    ///
    /// # Errors
    ///
    /// Returns `Err(BuilderError)` if `cell_id` is `Some(id)`, `id` is not an
    /// auto param, and `id` is absent from `current_values`. This indicates the
    /// eval pass did not complete — a logic error per the project's noisy-error
    /// convention. The `BuilderError` carries the missing `cell_id` as a
    /// structured field for use in tracing.
    fn add_auto_coord(
        &mut self,
        cell_id: &Option<ValueCellId>,
        auto_params: &[AutoParam],
        current_values: &ValueMap,
    ) -> Result<Slvs_hParam, BuilderError> {
        if let Some(id) = cell_id {
            // Check if already mapped
            if let Some(h) = self.mapping.get_param(id) {
                return Ok(h);
            }
            // Check if it's truly an auto param
            if auto_params.iter().any(|ap| ap.id == *id) {
                let h = self.alloc.param();
                let initial = current_values
                    .get(id)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.01); // small nonzero default for lengths
                self.params.push(Slvs_Param::new(h, SOLVE_GROUP, initial));
                self.mapping.insert(id.clone(), h);
                return Ok(h);
            }
            // Not an auto param — put in SOLVE_GROUP with current value
            // (avoids mixed-group Jacobian issues, but not mapped so value is ignored).
            // If the value is missing, the eval pass didn't complete — this is a logic
            // error that must not be silently swallowed.
            match current_values.get(id).and_then(|v| v.as_f64()) {
                Some(val) => {
                    let h = self.alloc.param();
                    self.params.push(Slvs_Param::new(h, SOLVE_GROUP, val));
                    Ok(h)
                }
                None => Err(BuilderError {
                    cell_id: id.clone(),
                    message: format!("non-auto parameter {id} missing from current_values"),
                }),
            }
        } else {
            // No cell_id — a fixed coordinate not backed by a cell.
            // This path is reached when a 3D point has a literal coordinate
            // that isn't an auto param (e.g. x=literal in a mixed auto/fixed
            // point). Put in SOLVE_GROUP to match the entity group and avoid
            // mixed-group Jacobian issues. Not mapped, so the value is ignored.
            let h = self.alloc.param();
            self.params.push(Slvs_Param::new(h, SOLVE_GROUP, 0.0));
            Ok(h)
        }
    }

    /// Add a line segment entity from two point entities.
    fn add_line_segment(&mut self, pt_a: Slvs_hEntity, pt_b: Slvs_hEntity) -> Slvs_hEntity {
        let eh = self.alloc.entity();
        self.entities
            .push(Slvs_Entity::line_segment(eh, SOLVE_GROUP, pt_a, pt_b));
        eh
    }

    /// Adds up to 4 point entities and 2 line segment entities for a pair of lines.
    ///
    /// Extracts the start/end points of `line_a` and `line_b`, creates point
    /// entities for each via [`add_point`], then creates two
    /// [`SLVS_E_LINE_SEGMENT`] entities from those points.  Returns a
    /// [`LinePairEntities`] with the two segment handles.
    ///
    /// [`add_point`]: SystemBuilder::add_point
    /// [`SLVS_E_LINE_SEGMENT`]: crate::slvs_sys::SLVS_E_LINE_SEGMENT
    ///
    /// ## Point deduplication
    ///
    /// [`add_point`] maintains a `PointKey`-based cache, so shared endpoints
    /// are reused rather than duplicated:
    ///
    /// - **Fixed** points dedup when coordinates are bit-equal (`f64::to_bits`).
    /// - **Auto** points dedup when all three `ValueCellId` components are equal.
    ///
    /// If two or more of the four corner points are identical, the actual number
    /// of new entities is between 5 (one shared endpoint: 3 points + 2 lines)
    /// and 6 (all distinct: 4 points + 2 lines).
    ///
    /// # Partial mutation
    ///
    /// On `Err`, point entities created by earlier successful [`add_point`]
    /// calls are **not** rolled back.  They remain in `builder.point_entities`
    /// and `builder.entities`.  Callers that abandon the builder on `Err`
    /// (such as [`solve`]) are unaffected, but callers that reuse the builder
    /// after an error must account for the pre-existing partial state.
    ///
    /// [`solve`]: crate::solvespace::solve
    ///
    /// # Errors
    ///
    /// Returns `Err(BuilderError)` if any point entity cannot be created —
    /// specifically, when a non-auto coordinate `cell_id` is absent from
    /// `current_values`.  The error carries the offending `cell_id` so the
    /// caller can surface a precise diagnostic.
    fn add_line_pair(
        &mut self,
        line_a: &LineRef,
        line_b: &LineRef,
        auto_params: &[AutoParam],
        current_values: &ValueMap,
    ) -> Result<LinePairEntities, BuilderError> {
        let la_start = self.add_point(&line_a.start, auto_params, current_values)?;
        let la_end = self.add_point(&line_a.end, auto_params, current_values)?;
        let lb_start = self.add_point(&line_b.start, auto_params, current_values)?;
        let lb_end = self.add_point(&line_b.end, auto_params, current_values)?;
        let line_a_e = self.add_line_segment(la_start, la_end);
        let line_b_e = self.add_line_segment(lb_start, lb_end);
        Ok(LinePairEntities {
            line_a: line_a_e,
            line_b: line_b_e,
        })
    }

    /// Get or create the default XY workplane.
    ///
    /// Some constraints (parallel, perpendicular, angle) require a workplane
    /// in SolveSpace. We create an XY workplane at the origin in FIXED_GROUP.
    fn get_workplane(&mut self) -> Slvs_hEntity {
        if let Some(wp) = self.workplane {
            return wp;
        }

        // Origin point for workplane (at 0,0,0)
        let ox = self.alloc.param();
        let oy = self.alloc.param();
        let oz = self.alloc.param();
        self.params.push(Slvs_Param::new(ox, FIXED_GROUP, 0.0));
        self.params.push(Slvs_Param::new(oy, FIXED_GROUP, 0.0));
        self.params.push(Slvs_Param::new(oz, FIXED_GROUP, 0.0));
        let origin_e = self.alloc.entity();
        self.entities
            .push(Slvs_Entity::point_3d(origin_e, FIXED_GROUP, ox, oy, oz));

        // Normal for XY plane: quaternion (1, 0, 0, 0) = identity rotation
        let nw = self.alloc.param();
        let nx = self.alloc.param();
        let ny = self.alloc.param();
        let nz = self.alloc.param();
        self.params.push(Slvs_Param::new(nw, FIXED_GROUP, 1.0));
        self.params.push(Slvs_Param::new(nx, FIXED_GROUP, 0.0));
        self.params.push(Slvs_Param::new(ny, FIXED_GROUP, 0.0));
        self.params.push(Slvs_Param::new(nz, FIXED_GROUP, 0.0));
        let normal_e = self.alloc.entity();
        let mut normal_entity =
            Slvs_Entity::zeroed_with(normal_e, FIXED_GROUP, slvs_sys::SLVS_E_NORMAL_IN_3D);
        normal_entity.param = [nw, nx, ny, nz];
        self.entities.push(normal_entity);

        // Workplane entity
        let wp_e = self.alloc.entity();
        let mut wp_entity = Slvs_Entity::zeroed_with(wp_e, FIXED_GROUP, slvs_sys::SLVS_E_WORKPLANE);
        wp_entity.point[0] = origin_e;
        wp_entity.normal = normal_e;
        self.entities.push(wp_entity);

        self.workplane = Some(wp_e);
        wp_e
    }

    /// Get or create the in-plane normal that circles and arcs point at.
    ///
    /// Lives in `FIXED_GROUP` alongside the workplane it names: it carries no
    /// params, and a normal that the solver was free to move would be a datum
    /// that drifts.
    fn get_sketch_normal(&mut self) -> Slvs_hEntity {
        if let Some(n) = self.sketch_normal {
            return n;
        }
        let wrkpl = self.get_workplane();
        let n = self.alloc.entity();
        self.entities
            .push(Slvs_Entity::normal_in_2d(n, FIXED_GROUP, wrkpl));
        self.sketch_normal = Some(n);
        n
    }

    /// Add a constraint on a specific workplane (or `SLVS_FREE_IN_3D` for 3D).
    ///
    /// The constraint lands in `self.solve_group`, which is what makes it
    /// visible to the subsequent solve: libslvs only generates equations for
    /// constraints whose group matches the group `Slvs_Solve` was called with.
    /// For the legacy pattern-recognition route `solve_group` is still
    /// `SOLVE_GROUP`, so nothing there changes.
    #[allow(clippy::too_many_arguments)]
    fn add_constraint_wrkpl(
        &mut self,
        type_: std::os::raw::c_int,
        wrkpl: Slvs_hEntity,
        val_a: f64,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
        entity_a: Slvs_hEntity,
        entity_b: Slvs_hEntity,
    ) {
        // Both endpoint selectors zero: every constraint but the tangency pair
        // ignores them entirely.
        self.add_constraint_wrkpl_other(type_, wrkpl, val_a, pt_a, pt_b, entity_a, entity_b, 0, 0);
    }

    /// Add a workplane constraint that also selects *which end* of each curve it
    /// is talking about.
    ///
    /// slvs carries that choice in `other` / `other2`: 0 names a curve's start
    /// point, 1 its end point.  Only the tangency constraints read them, which
    /// is why [`Self::add_constraint_wrkpl`] — the form every other constraint
    /// uses — is this function with both selectors zero rather than a second
    /// copy of the push.
    #[allow(clippy::too_many_arguments)]
    fn add_constraint_wrkpl_other(
        &mut self,
        type_: std::os::raw::c_int,
        wrkpl: Slvs_hEntity,
        val_a: f64,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
        entity_a: Slvs_hEntity,
        entity_b: Slvs_hEntity,
        other: std::os::raw::c_int,
        other2: std::os::raw::c_int,
    ) {
        let ch = self.alloc.constraint();
        let group = self.solve_group;
        // Every handle allocated while a sketch declaration is being lowered is
        // attributed to it, whether the declaration produced one constraint or
        // several.  Outside `add_sketch` this is a no-op.
        if let Some(origin) = self.attributing {
            self.constraint_attribution.insert(ch, origin);
        }
        self.constraints.push(
            Slvs_Constraint::new(
                ch, group, type_, wrkpl, val_a, pt_a, pt_b, entity_a, entity_b,
            )
            .with_other(other, other2),
        );
    }

    /// Lower a [`SketchSystem`] directly into this builder's slvs system.
    ///
    /// "Directly" is the point: nothing here inspects a `CompiledExpr` or tries
    /// to recognise a pattern.  The caller has already decided what the sketch
    /// means; this walks the typed declarations and emits the corresponding slvs
    /// entities and constraints one for one.
    ///
    /// The sketch gets its own freshly allocated group and this builder's
    /// `solve_group` is repointed at it, so the subsequent `solve()` varies the
    /// sketch's params and holds the datum geometry (workplane origin, normal)
    /// fixed.  One `add_sketch` per builder.
    ///
    /// # Errors
    ///
    /// Returns `Err(SketchBuildError)` for a malformed `SketchSystem`.
    fn add_sketch(&mut self, system: &SketchSystem) -> Result<SketchHandleMap, SketchBuildError> {
        // Before anything is emitted, so a malformed system is refused whole
        // instead of being lowered with the offending declaration dropped — a
        // partial lowering still solves, and returns a plausible answer to a
        // question the caller never asked.
        system.validate()?;

        let wrkpl = self.get_workplane();
        let group = self.alloc.group();
        self.solve_group = group;

        let mut handles = SketchHandleMap::new();
        let mut emitted: HashMap<SketchEntityId, EmittedEntity> = HashMap::new();

        // Pass 1: points.  Every other entity kind is defined by the points it
        // references, so points must exist as slvs entities before anything can
        // name them.
        for def in &system.entities {
            if let SketchEntity::Point { x, y } = def.entity {
                let px = self.alloc.param();
                let py = self.alloc.param();
                self.params.push(Slvs_Param::new(px, group, x));
                self.params.push(Slvs_Param::new(py, group, y));
                let eh = self.alloc.entity();
                self.entities
                    .push(Slvs_Entity::point_2d(eh, group, wrkpl, px, py));
                emitted.insert(
                    def.id,
                    EmittedEntity::Point {
                        entity: eh,
                        params: (px, py),
                    },
                );
            }
        }

        // Pass 2: composite entities, and the readback map for everything.
        //
        // Walked in declaration order, and the readback entry is pushed for
        // points here too rather than in pass 1, so the solved output order is
        // the declaration order (O9) instead of "points first, then the rest".
        for def in &system.entities {
            match def.entity {
                SketchEntity::Point { .. } => {
                    if let Some((_, (px, py))) =
                        emitted.get(&def.id).and_then(EmittedEntity::as_point)
                    {
                        handles.push_point(def.id, px, py);
                    }
                }
                SketchEntity::Line { start, end } => {
                    let ends = emitted
                        .get(&start)
                        .and_then(EmittedEntity::as_point)
                        .zip(emitted.get(&end).and_then(EmittedEntity::as_point));
                    let Some(((ae, ap), (be, bp))) = ends else {
                        unresolved_entity_ref(def.id, start, end);
                        continue;
                    };
                    let eh = self.alloc.entity();
                    self.entities
                        .push(Slvs_Entity::line_segment_2d(eh, group, wrkpl, ae, be));
                    emitted.insert(
                        def.id,
                        EmittedEntity::Line {
                            entity: eh,
                            start,
                            end,
                        },
                    );
                    handles.push_line(def.id, ap, bp);
                }
                SketchEntity::Circle { center, radius } => {
                    let Some((ce, cp)) = emitted.get(&center).and_then(EmittedEntity::as_point)
                    else {
                        unresolved_entity_ref(def.id, center, center);
                        continue;
                    };
                    // libslvs has no radius param: a circle points at a
                    // *distance entity*, which is what holds the param.
                    let rp = self.alloc.param();
                    self.params.push(Slvs_Param::new(rp, group, radius));
                    let rc = self.alloc.entity();
                    self.entities
                        .push(Slvs_Entity::distance(rc, group, wrkpl, rp));
                    let normal = self.get_sketch_normal();
                    let eh = self.alloc.entity();
                    self.entities
                        .push(Slvs_Entity::circle(eh, group, wrkpl, ce, normal, rc));
                    emitted.insert(def.id, EmittedEntity::Circle { entity: eh });
                    handles.push_circle(def.id, cp, rp);
                }
                SketchEntity::Arc { center, start, end } => {
                    let pts = [center, start, end]
                        .iter()
                        .map(|id| emitted.get(id).and_then(EmittedEntity::as_point))
                        .collect::<Option<Vec<_>>>();
                    let Some(pts) = pts else {
                        unresolved_entity_ref(def.id, start, end);
                        continue;
                    };
                    let [(ce, cp), (se, sp), (ee, ep)] = pts[..] else {
                        unresolved_entity_ref(def.id, start, end);
                        continue;
                    };
                    // No radius param of its own: an arc's radius is implied by
                    // centre→start, and libslvs supplies |c-s| = |c-e| itself.
                    //
                    // That implicit equation is why DOF is never recomputed on
                    // this side of the FFI.  An arc's three points are six
                    // params but only five degrees of freedom, and nothing here
                    // declared the equation that removes the sixth.  `solve`
                    // reports whatever `Slvs_Solve` reports, so the arc is
                    // accounted for correctly without this emit site having to
                    // publish its own rank contribution.
                    let normal = self.get_sketch_normal();
                    let eh = self.alloc.entity();
                    self.entities.push(Slvs_Entity::arc_of_circle(
                        eh, group, wrkpl, normal, ce, se, ee,
                    ));
                    emitted.insert(def.id, EmittedEntity::Arc { entity: eh });
                    handles.push_arc(def.id, cp, sp, ep);
                }
            }
        }

        // Pass 3: constraints.
        //
        // The declaration under emission is announced on the builder rather than
        // passed down, so every slvs handle allocated below it is attributed
        // without the emit sites having to remember to say so.
        for def in &system.constraints {
            self.attributing = Some((def.id, def.span));
            self.emit_sketch_constraint(def, wrkpl, &emitted);
        }
        self.attributing = None;

        handles.set_attribution(std::mem::take(&mut self.constraint_attribution));
        Ok(handles)
    }

    /// Emit the slvs constraints for one sketch constraint declaration.
    ///
    /// One declaration can expand into more than one slvs constraint — `Fix` on
    /// a line anchors each endpoint separately — so this pushes rather than
    /// returning a handle.
    fn emit_sketch_constraint(
        &mut self,
        def: &SketchConstraintDef,
        wrkpl: Slvs_hEntity,
        emitted: &HashMap<SketchEntityId, EmittedEntity>,
    ) {
        /// The "no entity" sentinel for the slots a given constraint leaves empty.
        const NONE: Slvs_hEntity = Slvs_hEntity(0);

        /// Resolve `id` to a point entity handle, or report and yield `None`.
        fn point(
            emitted: &HashMap<SketchEntityId, EmittedEntity>,
            def: &SketchConstraintDef,
            id: SketchEntityId,
        ) -> Option<Slvs_hEntity> {
            match emitted.get(&id).and_then(EmittedEntity::as_point) {
                Some((entity, _)) => Some(entity),
                None => {
                    unresolved_constraint_ref(def, id, "point");
                    None
                }
            }
        }

        /// Resolve `id` to a curve (circle or arc) handle, or report and yield
        /// `None`.
        fn curve(
            emitted: &HashMap<SketchEntityId, EmittedEntity>,
            def: &SketchConstraintDef,
            id: SketchEntityId,
        ) -> Option<Slvs_hEntity> {
            match emitted.get(&id).and_then(EmittedEntity::as_curve) {
                Some(entity) => Some(entity),
                None => {
                    unresolved_constraint_ref(def, id, "circle or arc");
                    None
                }
            }
        }

        /// Resolve `id` to an arc handle, or report and yield `None`.
        ///
        /// Deliberately stricter than [`curve`]: the tangency constraints read
        /// the *endpoints* of the curves they name, and a circle has none —
        /// `Slvs_Entity::circle` leaves `point[1]`/`point[2]` at zero.  Handing
        /// libslvs a circle here would have it resolve point handles the entity
        /// does not carry, which is a C-side lookup on a null handle rather than
        /// anything this binding can catch afterwards.  Hence a kind check
        /// before the call, not a hope about what happens after it.
        fn arc(
            emitted: &HashMap<SketchEntityId, EmittedEntity>,
            def: &SketchConstraintDef,
            id: SketchEntityId,
        ) -> Option<Slvs_hEntity> {
            match emitted.get(&id) {
                Some(EmittedEntity::Arc { entity }) => Some(*entity),
                _ => {
                    unresolved_constraint_ref(def, id, "arc");
                    None
                }
            }
        }

        /// slvs' endpoint selector for a curve: 0 = its start, 1 = its end.
        fn endpoint(at_end: bool) -> std::os::raw::c_int {
            if at_end { 1 } else { 0 }
        }

        /// Resolve `id` to a line entity handle, or report and yield `None`.
        fn line(
            emitted: &HashMap<SketchEntityId, EmittedEntity>,
            def: &SketchConstraintDef,
            id: SketchEntityId,
        ) -> Option<Slvs_hEntity> {
            match emitted.get(&id) {
                Some(EmittedEntity::Line { entity, .. }) => Some(*entity),
                _ => {
                    unresolved_constraint_ref(def, id, "line");
                    None
                }
            }
        }

        match def.constraint {
            SketchConstraint::Fix(id) => {
                // Anchoring is per point: a line is anchored by anchoring both
                // of its endpoints, which is two slvs constraints for one
                // declaration.
                let anchored: Vec<SketchEntityId> = match emitted.get(&id) {
                    Some(EmittedEntity::Point { .. }) => vec![id],
                    Some(EmittedEntity::Line { start, end, .. }) => vec![*start, *end],
                    _ => {
                        unresolved_constraint_ref(def, id, "point or line");
                        return;
                    }
                };
                for pt in anchored {
                    let Some(pe) = point(emitted, def, pt) else {
                        continue;
                    };
                    self.add_constraint_wrkpl(
                        SLVS_C_WHERE_DRAGGED,
                        wrkpl,
                        0.0,
                        pe,
                        NONE,
                        NONE,
                        NONE,
                    );
                }
            }
            SketchConstraint::Coincident { a, b } => {
                let (Some(ae), Some(be)) = (point(emitted, def, a), point(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_POINTS_COINCIDENT, wrkpl, 0.0, ae, be, NONE, NONE);
            }
            SketchConstraint::Distance { a, b, value } => {
                let (Some(ae), Some(be)) = (point(emitted, def, a), point(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_PT_PT_DISTANCE, wrkpl, value, ae, be, NONE, NONE);
            }
            SketchConstraint::Horizontal(id) => {
                let Some(le) = line(emitted, def, id) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_HORIZONTAL, wrkpl, 0.0, NONE, NONE, le, NONE);
            }
            SketchConstraint::Vertical(id) => {
                let Some(le) = line(emitted, def, id) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_VERTICAL, wrkpl, 0.0, NONE, NONE, le, NONE);
            }
            SketchConstraint::Parallel { a, b } => {
                let (Some(ae), Some(be)) = (line(emitted, def, a), line(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_PARALLEL, wrkpl, 0.0, NONE, NONE, ae, be);
            }
            SketchConstraint::Perpendicular { a, b } => {
                let (Some(ae), Some(be)) = (line(emitted, def, a), line(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_PERPENDICULAR, wrkpl, 0.0, NONE, NONE, ae, be);
            }
            SketchConstraint::Angle { a, b, degrees } => {
                let (Some(ae), Some(be)) = (line(emitted, def, a), line(emitted, def, b)) else {
                    return;
                };
                // Degrees, not radians: `SLVS_C_ANGLE` reads `valA` in degrees,
                // which is also the convention the legacy pattern route uses.
                self.add_constraint_wrkpl(SLVS_C_ANGLE, wrkpl, degrees, NONE, NONE, ae, be);
            }
            SketchConstraint::Diameter { circle, value } => {
                let Some(ce) = curve(emitted, def, circle) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_DIAMETER, wrkpl, value, NONE, NONE, ce, NONE);
            }
            SketchConstraint::Radius { circle, value } => {
                let Some(ce) = curve(emitted, def, circle) else {
                    return;
                };
                // libslvs exposes no radius constraint — `SLVS_C_DIAMETER` is
                // the whole radius family — so the doubling happens here, at the
                // single emit site, rather than being pushed onto callers. That
                // keeps `radius` meaning radius in the surface vocabulary.
                self.add_constraint_wrkpl(
                    SLVS_C_DIAMETER,
                    wrkpl,
                    2.0 * value,
                    NONE,
                    NONE,
                    ce,
                    NONE,
                );
            }
            SketchConstraint::PtOnCircle { pt, circle } => {
                let (Some(pe), Some(ce)) = (point(emitted, def, pt), curve(emitted, def, circle))
                else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_PT_ON_CIRCLE, wrkpl, 0.0, pe, NONE, ce, NONE);
            }
            SketchConstraint::EqualRadius { a, b } => {
                let (Some(ae), Some(be)) = (curve(emitted, def, a), curve(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_EQUAL_RADIUS, wrkpl, 0.0, NONE, NONE, ae, be);
            }
            // Tangency, both arms.
            //
            // These constrain *directions* only: they say the line is square to
            // the arc's radius at the chosen end, or that two arcs' radii there
            // are parallel.  Neither makes the curves touch — a caller who wants
            // them to meet pairs the tangency with a `Coincident` on the two
            // endpoints, which is what the fixtures do.
            SketchConstraint::ArcLineTangent {
                arc: arc_id,
                line: line_id,
                at_end,
            } => {
                let (Some(ae), Some(le)) = (arc(emitted, def, arc_id), line(emitted, def, line_id))
                else {
                    return;
                };
                self.add_constraint_wrkpl_other(
                    SLVS_C_ARC_LINE_TANGENT,
                    wrkpl,
                    0.0,
                    NONE,
                    NONE,
                    ae,
                    le,
                    endpoint(at_end),
                    0,
                );
            }
            SketchConstraint::CurveCurveTangent {
                a,
                a_at_end,
                b,
                b_at_end,
            } => {
                let (Some(ae), Some(be)) = (arc(emitted, def, a), arc(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl_other(
                    SLVS_C_CURVE_CURVE_TANGENT,
                    wrkpl,
                    0.0,
                    NONE,
                    NONE,
                    ae,
                    be,
                    endpoint(a_at_end),
                    endpoint(b_at_end),
                );
            }
            SketchConstraint::PtOnLine { pt, line: line_id } => {
                let (Some(pe), Some(le)) = (point(emitted, def, pt), line(emitted, def, line_id))
                else {
                    return;
                };
                // The line's *infinite extension*: this says the point is on the
                // line, not that it lies between the endpoints, so it leaves the
                // point free to slide.
                self.add_constraint_wrkpl(SLVS_C_PT_ON_LINE, wrkpl, 0.0, pe, NONE, le, NONE);
            }
            SketchConstraint::AtMidpoint { pt, line: line_id } => {
                let (Some(pe), Some(le)) = (point(emitted, def, pt), line(emitted, def, line_id))
                else {
                    return;
                };
                self.add_constraint_wrkpl(SLVS_C_AT_MIDPOINT, wrkpl, 0.0, pe, NONE, le, NONE);
            }
            SketchConstraint::SymmetricLine { a, b, about } => {
                let (Some(ae), Some(be), Some(me)) = (
                    point(emitted, def, a),
                    point(emitted, def, b),
                    line(emitted, def, about),
                ) else {
                    return;
                };
                // The mirrored pair goes in the point slots and the mirror in the
                // entity slot — the asymmetry of the arguments is the whole
                // difference between "mirror a about b" and "mirror b about a".
                self.add_constraint_wrkpl(SLVS_C_SYMMETRIC_LINE, wrkpl, 0.0, ae, be, me, NONE);
            }
            SketchConstraint::EqualLengthLines { a, b } => {
                let (Some(ae), Some(be)) = (line(emitted, def, a), line(emitted, def, b)) else {
                    return;
                };
                self.add_constraint_wrkpl(
                    SLVS_C_EQUAL_LENGTH_LINES,
                    wrkpl,
                    0.0,
                    NONE,
                    NONE,
                    ae,
                    be,
                );
            }
        }
    }

    /// Solve the system and return the result.
    ///
    /// Checks for Vec-length overflow when casting to `c_int` (i32) and
    /// performs bounds-checked access on the `faileds` field returned by
    /// `Slvs_Solve`.
    fn solve(mut self) -> SlvsSolveResult {
        let solve_group = self.solve_group;

        // Every system goes through `Slvs_Solve`, including one with no
        // constraints at all, so `dof` is always libslvs' own number.
        //
        // There is no shortcut here for the zero-constraint case, and
        // deliberately so.  Computing `dof` in Rust as "count the free params
        // in the solve group" requires knowing every equation libslvs
        // generates on its own behalf, and that knowledge cannot be kept
        // honest: an `SLVS_E_ARC_OF_CIRCLE` silently contributes
        // `|c-s| = |c-e|`, so rank is not 0 even with zero declared
        // constraints, and an unconstrained arc's true dof is 5 rather than
        // its six params.  Correcting the arithmetic per entity kind would
        // only re-derive libslvs' internal rules on this side of the FFI,
        // where they go stale the moment another entity kind grows an
        // implicit equation.  Asking the library is correct by construction.
        //
        // Nothing is lost by the removal: the legacy `recognize_pattern`
        // route cannot reach `solve` with an empty constraint list at all —
        // `ConstraintSolver::solve` bails with `NoProgress` unless a pattern
        // was recognized, and every recognized pattern contributes at least
        // one constraint — and it discards `dof` regardless.

        // --- Overflow checks for vec lengths → c_int (i32) ---
        // Return TooLarge instead of panicking — panics here would
        // unwind through callers, corrupt partial state, and poison
        // SLVS_LOCK.
        let n_params = match i32::try_from(self.params.len()) {
            Ok(n) => n,
            Err(_) => return SlvsSolveResult::TooLarge,
        };
        let n_entities = match i32::try_from(self.entities.len()) {
            Ok(n) => n,
            Err(_) => return SlvsSolveResult::TooLarge,
        };
        let n_constraints = match i32::try_from(self.constraints.len()) {
            Ok(n) => n,
            Err(_) => return SlvsSolveResult::TooLarge,
        };

        // At least one slot even for a constraint-free system: an empty `Vec`'s
        // `as_mut_ptr()` is a dangling (aligned, unallocated) pointer, and now
        // that such a system reaches the FFI it would be handed straight to C.
        // `faileds` below still reports the *true* allocated length, so the
        // readback's bounds check stays sound.
        let mut failed: Vec<Slvs_hConstraint> =
            vec![Slvs_hConstraint(0); self.constraints.len().max(1)];
        let n_failed_buf = match i32::try_from(failed.len()) {
            Ok(n) => n,
            Err(_) => return SlvsSolveResult::TooLarge,
        };

        let mut sys = Slvs_System {
            param: self.params.as_mut_ptr(),
            params: n_params,
            entity: self.entities.as_mut_ptr(),
            entities: n_entities,
            constraint: self.constraints.as_mut_ptr(),
            constraints: n_constraints,
            dragged: [Slvs_hParam(0); 4],
            calculateFaileds: 1,
            failed: failed.as_mut_ptr(),
            faileds: n_failed_buf,
            dof: 0,
            result: 0,
        };

        // Lock the global mutex — libslvs uses internal global state and
        // is not safe to call concurrently.
        //
        // If the lock is poisoned (prior panic while holding it), refuse
        // to proceed: the C++ global state is in an indeterminate condition
        // and recovering would risk undefined behavior.
        let _guard = match SLVS_LOCK.lock() {
            Ok(guard) => guard,
            Err(_poisoned) => return SlvsSolveResult::LockPoisoned,
        };

        unsafe {
            slvs_sys::Slvs_Solve(&mut sys, solve_group);
        }

        // Drop guard after solve completes
        drop(_guard);

        match sys.result {
            SLVS_RESULT_OKAY => SlvsSolveResult::Ok {
                params: self.params,
                mapping: self.mapping,
                dof: sys.dof,
            },
            SLVS_RESULT_INCONSISTENT => {
                // --- Bounds check on faileds (c_int → usize) ---
                let n_failed = if sys.faileds < 0 {
                    0usize
                } else {
                    (sys.faileds as usize).min(failed.len())
                };
                let failed_ids = failed[..n_failed].to_vec();
                SlvsSolveResult::Inconsistent { failed_ids }
            }
            SLVS_RESULT_DIDNT_CONVERGE => SlvsSolveResult::DidntConverge,
            SLVS_RESULT_TOO_MANY_UNKNOWNS => SlvsSolveResult::TooManyUnknowns,
            code => SlvsSolveResult::UnknownError(code),
        }
    }
}

enum SlvsSolveResult {
    Ok {
        params: Vec<Slvs_Param>,
        mapping: ParamMapping,
        /// libslvs' degrees-of-freedom count for the solved group.
        dof: i32,
    },
    Inconsistent {
        failed_ids: Vec<Slvs_hConstraint>,
    },
    DidntConverge,
    TooManyUnknowns,
    /// Vec lengths exceeded i32::MAX — can't pass to the C API.
    TooLarge,
    /// The global SLVS_LOCK mutex was poisoned by a prior panic.
    LockPoisoned,
    UnknownError(i32),
}

// ---------------------------------------------------------------------------
// Constrained-2D-sketch entry point
// ---------------------------------------------------------------------------

/// What `add_sketch` emitted for one sketch entity, indexed by its sketch id.
///
/// The emit-side counterpart of `SketchHandleMap`: constraints name their
/// operands by entity handle and are picky about kind (`SLVS_C_HORIZONTAL`
/// wants a line, `SLVS_C_WHERE_DRAGGED` wants a point), so this records the
/// handle *and* enough structure to reject a slot handed the wrong thing.
enum EmittedEntity {
    Point {
        entity: Slvs_hEntity,
        /// The two params carrying the point's coordinates, kept so a composite
        /// entity can register its readback without re-deriving them.
        params: (Slvs_hParam, Slvs_hParam),
    },
    Line {
        entity: Slvs_hEntity,
        start: SketchEntityId,
        end: SketchEntityId,
    },
    Circle {
        entity: Slvs_hEntity,
    },
    Arc {
        entity: Slvs_hEntity,
    },
}

impl EmittedEntity {
    /// The slvs entity handle and coordinate params, if this is a point.
    ///
    /// `None` for every other kind, which is what makes "this slot wants a
    /// point" a check rather than an assumption.
    fn as_point(&self) -> Option<(Slvs_hEntity, (Slvs_hParam, Slvs_hParam))> {
        match self {
            EmittedEntity::Point { entity, params } => Some((*entity, *params)),
            _ => None,
        }
    }

    /// The slvs entity handle, if this is a curve — a circle or an arc.
    ///
    /// The radius family and `PtOnCircle` accept either: libslvs treats an arc
    /// as a circle with two ends for all of them.
    fn as_curve(&self) -> Option<Slvs_hEntity> {
        match self {
            EmittedEntity::Circle { entity } | EmittedEntity::Arc { entity } => Some(*entity),
            _ => None,
        }
    }
}

/// Report a composite entity whose defining points could not be resolved.
///
/// Unreachable for any input: `SketchSystem::validate` runs before emission
/// starts and rejects a dangling id or a non-point endpoint as a typed
/// `SketchBuildError::BadEntityRef`.  What remains here is a guard against
/// *this crate* drifting — the validator's slot table and the emit path's
/// per-kind lookups describe the same rule in two places, and if they ever
/// disagree the entity would otherwise vanish from the readback in silence.
/// Hence a loud report and a debug assertion, not a warning.
fn unresolved_entity_ref(owner: SketchEntityId, start: SketchEntityId, end: SketchEntityId) {
    tracing::error!(
        entity = owner.0,
        start = start.0,
        end = end.0,
        "a validated sketch entity is defined by ids that are not emitted points; \
         the validator's slot table and the emit path disagree"
    );
    debug_assert!(
        false,
        "unresolvable entity ref after validation: entity {} references {}/{}",
        owner.0, start.0, end.0
    );
}

/// Report a constraint operand that could not be resolved to an entity of the
/// kind that slot requires.
///
/// Same provenance as [`unresolved_entity_ref`]: `SketchSystem::validate` has
/// already rejected every input that could reach this, so arriving here means
/// the validator and the emit path disagree about a slot's kind.
fn unresolved_constraint_ref(def: &SketchConstraintDef, entity: SketchEntityId, expected: &str) {
    tracing::error!(
        constraint = def.id.0,
        entity = entity.0,
        expected,
        "a validated sketch constraint operand is not an emitted entity of the \
         expected kind; the validator's slot table and the emit path disagree"
    );
    debug_assert!(
        false,
        "unresolvable constraint operand after validation: constraint {} wants {} at entity {}",
        def.id.0, expected, entity.0
    );
}

/// Solve a 2D constrained sketch through a real `Slvs_Solve` call.
///
/// This is the crate's only sketch-solving seam.  `SystemBuilder` and
/// `add_sketch` stay private on purpose: the builder holds raw `Slvs_*` structs,
/// and publishing it would push the whole slvs vocabulary across the crate
/// boundary and make every future FFI change a breaking change for consumers.
/// A free function that owns the whole build → solve → read-back round trip
/// gives callers exactly one thing to depend on.
///
/// What comes back is libslvs' own report, not a judgement about it: the raw
/// `dof`, the resolved failing set, the raw non-OK result codes.  Classifying
/// that into diagnostics needs to know which degrees of freedom were declared
/// `auto`, which this layer cannot see.
pub fn solve_sketch(system: &SketchSystem) -> SketchSolveResult {
    let mut builder = SystemBuilder::new();

    let handles = match builder.add_sketch(system) {
        Ok(handles) => handles,
        // Nothing was emitted, so there is nothing to solve and nothing to tear
        // down: the builder is dropped and the typed error goes back verbatim.
        Err(err) => return SketchSolveResult::Malformed(err),
    };

    match builder.solve() {
        SlvsSolveResult::Ok { params, dof, .. } => {
            let values: HashMap<Slvs_hParam, f64> = params.iter().map(|p| (p.h, p.val)).collect();
            match handles.read_back(&values) {
                Ok(entities) => SketchSolveResult::Solved { entities, dof },
                Err(missing) => {
                    // Every param the handle map references was pushed into the
                    // same system that produced `params`, so this cannot happen.
                    // If it somehow does, say so loudly instead of returning
                    // fabricated coordinates — and say the *true* thing: libslvs
                    // reported OKAY and the failure is on this side of the FFI,
                    // so this is its own arm rather than `UnknownError(OKAY)`,
                    // which would blame the C library for a Rust bug.
                    tracing::error!(
                        param = missing.0,
                        "sketch readback referenced a param absent from the solved system"
                    );
                    SketchSolveResult::ReadbackFailed { param: missing.0 }
                }
            }
        }
        SlvsSolveResult::Inconsistent { failed_ids } => SketchSolveResult::Inconsistent {
            // libslvs names the constraints that contradict by slvs handle; the
            // attribution map turns those back into the declarations — and the
            // spans — the author actually wrote, which is the difference between
            // a diagnostic that points at source and a bare count.
            failing: handles.resolve_failing(&failed_ids),
        },
        SlvsSolveResult::DidntConverge => SketchSolveResult::DidntConverge,
        SlvsSolveResult::TooManyUnknowns => SketchSolveResult::TooManyUnknowns,
        SlvsSolveResult::TooLarge => SketchSolveResult::TooLarge,
        SlvsSolveResult::LockPoisoned => SketchSolveResult::LockPoisoned,
        SlvsSolveResult::UnknownError(code) => SketchSolveResult::UnknownError(code),
    }
}

fn point_key(pt: &PointRef) -> PointKey {
    match pt {
        PointRef::Auto { x, y, z } => PointKey::Auto(x.clone(), y.clone(), z.clone()),
        PointRef::Fixed { x, y, z } => PointKey::Fixed(x.to_bits(), y.to_bits(), z.to_bits()),
    }
}

// ---------------------------------------------------------------------------
// ConstraintSolver implementation
// ---------------------------------------------------------------------------

impl ConstraintSolver for SolveSpaceSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        if problem.auto_params.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
                unique: true,
            };
        }

        let mut builder = SystemBuilder::new();
        let mut recognized_any = false;

        for (_cn_id, expr) in &problem.constraints {
            match recognize_pattern(expr, &problem.auto_params) {
                Some(pattern) => {
                    recognized_any = true;
                    if let Err(err) = add_pattern_to_builder(
                        &mut builder,
                        &pattern,
                        &problem.auto_params,
                        &problem.current_values,
                    ) {
                        tracing::warn!(
                            cell_id = %err.cell_id,
                            reason = %err.message,
                            "constraint pattern builder failed"
                        );
                        return SolveResult::NoProgress {
                            reason: err.message,
                        };
                    }
                }
                None => {
                    return SolveResult::NoProgress {
                        reason: "unrecognized geometric constraint pattern".to_string(),
                    };
                }
            }
        }

        if !recognized_any {
            return SolveResult::NoProgress {
                reason: "no geometric constraint patterns recognized".to_string(),
            };
        }

        // Solve
        match builder.solve() {
            SlvsSolveResult::Ok {
                params, mapping, ..
            } => {
                // Extract solved values
                let mut values: HashMap<ValueCellId, Value> = HashMap::new();
                for param in &params {
                    if let Some(cell_id) = mapping.param_to_cell.get(&param.h) {
                        // Find the dimension from auto_params
                        let dim = problem
                            .auto_params
                            .iter()
                            .find(|ap| ap.id == *cell_id)
                            .map(|ap| dimension_of(&ap.param_type))
                            .unwrap_or(DimensionVector::DIMENSIONLESS);
                        values.insert(
                            cell_id.clone(),
                            Value::Scalar {
                                si_value: param.val,
                                dimension: dim,
                            },
                        );
                    }
                }
                SolveResult::Solved {
                    values,
                    unique: true,
                }
            }
            SlvsSolveResult::Inconsistent { failed_ids } => SolveResult::Infeasible {
                diagnostics: vec![
                    Diagnostic::error(format!(
                        "geometric constraints are inconsistent ({} failed)",
                        failed_ids.len()
                    ))
                    .with_code(DiagnosticCode::ConstraintUnsatisfiable),
                ],
            },
            SlvsSolveResult::DidntConverge => SolveResult::NoProgress {
                reason: "SolveSpace solver did not converge".to_string(),
            },
            SlvsSolveResult::TooManyUnknowns => SolveResult::NoProgress {
                reason: "too many unknowns for SolveSpace solver".to_string(),
            },
            SlvsSolveResult::TooLarge => SolveResult::NoProgress {
                reason: "constraint system too large for SolveSpace (exceeds i32::MAX entities)"
                    .to_string(),
            },
            SlvsSolveResult::LockPoisoned => SolveResult::NoProgress {
                reason:
                    "solver lock poisoned by earlier panic — libslvs global state may be corrupted"
                        .to_string(),
            },
            SlvsSolveResult::UnknownError(code) => SolveResult::NoProgress {
                reason: format!("SolveSpace solver returned unknown error code {}", code),
            },
        }
    }
}

/// Add a recognized pattern to the system builder.
///
/// # Errors
///
/// Returns `Err(BuilderError)` if any point contains a non-auto coordinate
/// cell_id that is missing from `current_values` (propagated from
/// `add_point` → `add_auto_coord`). The `BuilderError` carries the missing
/// `cell_id` as a structured field for the `solve()` tracing log.
fn add_pattern_to_builder(
    builder: &mut SystemBuilder,
    pattern: &GeometricPattern,
    auto_params: &[AutoParam],
    current_values: &ValueMap,
) -> Result<(), BuilderError> {
    let e_none = Slvs_hEntity(0);

    match pattern {
        GeometricPattern::PtPtDistance {
            pt_a,
            pt_b,
            distance_si,
        } => {
            let ea = builder.add_point(pt_a, auto_params, current_values)?;
            let eb = builder.add_point(pt_b, auto_params, current_values)?;
            // Use the workplane for 2D points so the constraint operates in 2D.
            let wrkpl = if pt_a.is_2d() && pt_b.is_2d() {
                builder.get_workplane()
            } else {
                SLVS_FREE_IN_3D
            };
            builder.add_constraint_wrkpl(
                SLVS_C_PT_PT_DISTANCE,
                wrkpl,
                *distance_si,
                ea,
                eb,
                e_none,
                e_none,
            );
        }
        GeometricPattern::Angle {
            line_a,
            line_b,
            angle_deg,
        } => {
            let LinePairEntities {
                line_a: line_a_e,
                line_b: line_b_e,
            } = builder.add_line_pair(line_a, line_b, auto_params, current_values)?;
            // Angle constraints require a workplane in SolveSpace.
            // Degrees, not radians: `SLVS_C_ANGLE` reads `valA` in degrees, and
            // `angle_deg` was already converted at the marshalling boundary in
            // `try_angle_eq` — no conversion is owed here (#6184).
            let wp = builder.get_workplane();
            builder.add_constraint_wrkpl(
                SLVS_C_ANGLE,
                wp,
                *angle_deg,
                e_none,
                e_none,
                line_a_e,
                line_b_e,
            );
        }
        GeometricPattern::Parallel { line_a, line_b } => {
            let LinePairEntities {
                line_a: line_a_e,
                line_b: line_b_e,
            } = builder.add_line_pair(line_a, line_b, auto_params, current_values)?;
            // Parallel/perpendicular require a workplane in SolveSpace
            let wp = builder.get_workplane();
            builder.add_constraint_wrkpl(
                SLVS_C_PARALLEL,
                wp,
                0.0,
                e_none,
                e_none,
                line_a_e,
                line_b_e,
            );
        }
        GeometricPattern::Perpendicular { line_a, line_b } => {
            let LinePairEntities {
                line_a: line_a_e,
                line_b: line_b_e,
            } = builder.add_line_pair(line_a, line_b, auto_params, current_values)?;
            let wp = builder.get_workplane();
            builder.add_constraint_wrkpl(
                SLVS_C_PERPENDICULAR,
                wp,
                0.0,
                e_none,
                e_none,
                line_a_e,
                line_b_e,
            );
        }
        GeometricPattern::Coincident { pt_a, pt_b } => {
            let ea = builder.add_point(pt_a, auto_params, current_values)?;
            let eb = builder.add_point(pt_b, auto_params, current_values)?;
            // Use the workplane for 2D points so the constraint operates in 2D.
            let wrkpl = if pt_a.is_2d() && pt_b.is_2d() {
                builder.get_workplane()
            } else {
                SLVS_FREE_IN_3D
            };
            builder.add_constraint_wrkpl(
                SLVS_C_POINTS_COINCIDENT,
                wrkpl,
                0.0,
                ea,
                eb,
                e_none,
                e_none,
            );
        }
    }
    Ok(())
}

/// Extract the DimensionVector from a Type.
fn dimension_of(ty: &Type) -> DimensionVector {
    match ty {
        Type::Scalar { dimension } => *dimension,
        _ => DimensionVector::DIMENSIONLESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_test_support::{single_auto_param, vcid};

    // ── Shared test helpers ────────────────────────────────────────────────

    /// Returns a standard "missing non-auto coord" setup tuple:
    /// `(builder, cell_id, auto_params, current_values)` where:
    /// - `builder` is a fresh `SystemBuilder` with no params
    /// - `cell_id` is `vcid(entity, field)`
    /// - `auto_params` is an empty `Vec<AutoParam>` (cell_id is non-auto)
    /// - `current_values` is an empty `ValueMap` (cell_id is absent → triggers Err)
    fn missing_coord_setup(
        entity: &str,
        field: &str,
    ) -> (SystemBuilder, ValueCellId, Vec<AutoParam>, ValueMap) {
        let builder = SystemBuilder::new();
        let cell_id = vcid(entity, field);
        let auto_params: Vec<AutoParam> = vec![];
        let current_values = ValueMap::new();
        (builder, cell_id, auto_params, current_values)
    }

    /// Asserts that `result` is an `Err(BuilderError)` whose `cell_id`
    /// matches and whose `message` contains `"missing"`.  Generic over
    /// the `Ok` type so it works for `Result<Slvs_hParam, BuilderError>`,
    /// `Result<Slvs_hEntity, BuilderError>`, and `Result<(), BuilderError>` alike.
    /// `context` is prepended to every assertion failure message so the
    /// call site is visible without inspecting the stack trace.
    #[track_caller]
    fn assert_missing_err<T: std::fmt::Debug>(
        result: Result<T, BuilderError>,
        cell_id: &ValueCellId,
        context: &str,
    ) {
        match result {
            Err(BuilderError {
                cell_id: id,
                message,
            }) => {
                assert_eq!(
                    id, *cell_id,
                    "{context}: BuilderError cell_id should match the expected ValueCellId"
                );
                assert!(
                    message.contains("missing"),
                    "{context}: BuilderError message should contain 'missing', got: {}",
                    message
                );
            }
            Ok(v) => panic!("{context}: expected Err for missing non-auto coord, got Ok({v:?})"),
        }
    }

    /// `fixed_line` helper: constructs a fully-Fixed `LineRef` from six coordinates.
    /// Drives step-1 TDD cycle.
    #[test]
    fn fixed_line_helper_produces_expected_line_ref() {
        let line = fixed_line(0.0, 1.0, 2.0, 3.0, 4.0, 5.0);
        match line.start {
            PointRef::Fixed { x, y, z } => {
                assert_eq!(x, 0.0);
                assert_eq!(y, 1.0);
                assert_eq!(z, 2.0);
            }
            other => panic!("expected Fixed start, got {other:?}"),
        }
        match line.end {
            PointRef::Fixed { x, y, z } => {
                assert_eq!(x, 3.0);
                assert_eq!(y, 4.0);
                assert_eq!(z, 5.0);
            }
            other => panic!("expected Fixed end, got {other:?}"),
        }
    }

    /// Shorthand for `PointRef::Fixed { x, y, z }`.
    fn fixed_point(x: f64, y: f64, z: f64) -> PointRef {
        PointRef::Fixed { x, y, z }
    }

    /// Shorthand for `LineRef { start, end }`.
    fn line(start: PointRef, end: PointRef) -> LineRef {
        LineRef { start, end }
    }

    /// Constructs a fully-Fixed `LineRef` from six coordinates.
    /// Reduces boilerplate in tests that use all-fixed line segments.
    fn fixed_line(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> LineRef {
        line(fixed_point(x0, y0, z0), fixed_point(x1, y1, z1))
    }

    /// Non-auto param with a value present in current_values should succeed
    /// and use the provided value. Regression guard for the non-auto happy path.
    #[test]
    fn add_auto_coord_succeeds_for_non_auto_with_value() {
        let mut builder = SystemBuilder::new();
        let cell_id = vcid("Test", "x");
        // cell_id is NOT in auto_params — it's a non-auto param
        let auto_params: Vec<AutoParam> = vec![];
        // But it IS in current_values
        let mut current_values = ValueMap::new();
        current_values.insert(
            cell_id.clone(),
            Value::Scalar {
                si_value: 42.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        );

        let result = builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        let h = result.expect("expected Ok for non-auto param present in current_values");
        // Verify the param was created with the correct value
        let param = builder
            .params
            .iter()
            .find(|p| p.h == h)
            .expect("param not found in builder");
        assert_eq!(
            param.val, 42.0,
            "param value should match current_values entry"
        );
    }

    /// Auto param not yet in current_values should get the 0.01 default.
    /// Regression guard: the documented auto-param default must not be changed.
    #[test]
    fn add_auto_coord_auto_param_default_preserved() {
        let mut builder = SystemBuilder::new();
        let cell_id = vcid("Test", "x");
        // cell_id IS in auto_params
        let auto_params = vec![single_auto_param(cell_id.clone())];
        // But NOT in current_values — should use 0.01 default
        let current_values = ValueMap::new();

        let result = builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        let h = result.expect("expected Ok for auto param");
        let param = builder
            .params
            .iter()
            .find(|p| p.h == h)
            .expect("param not found in builder");
        assert_eq!(
            param.val, 0.01,
            "auto param without current value should use 0.01 default"
        );
    }

    /// None cell_id (fixed literal coordinate) should return Ok with 0.0.
    /// Regression guard: the fixed-coordinate placeholder must remain 0.0.
    #[test]
    fn add_auto_coord_no_cell_id_uses_zero() {
        let mut builder = SystemBuilder::new();
        let auto_params: Vec<AutoParam> = vec![];
        let current_values = ValueMap::new();

        let result = builder.add_auto_coord(&None, &auto_params, &current_values);

        let h = result.expect("expected Ok for None cell_id");
        let param = builder
            .params
            .iter()
            .find(|p| p.h == h)
            .expect("param not found in builder");
        assert_eq!(
            param.val, 0.0,
            "None cell_id should produce param with value 0.0"
        );
    }

    /// `add_line_pair` should create 4 point entities and 2 line segment entities,
    /// returning two distinct handles as Ok.
    #[test]
    fn add_line_pair_returns_two_line_entities() {
        let mut builder = SystemBuilder::new();
        let auto_params: Vec<AutoParam> = vec![];
        let current_values = ValueMap::new();

        let la = line(fixed_point(0.0, 0.0, 0.0), fixed_point(1.0, 0.0, 0.0));
        let lb = line(fixed_point(0.0, 1.0, 0.0), fixed_point(1.0, 1.0, 0.0));

        let result = builder.add_line_pair(&la, &lb, &auto_params, &current_values);

        let entities = result.expect("add_line_pair should return Ok");
        assert_ne!(
            entities.line_a, entities.line_b,
            "line entities should be distinct handles"
        );
        // 4 Fixed points (each creates 1 entity) + 2 line segments = 6 entities
        assert_eq!(
            builder.entities.len(),
            6,
            "expected 4 point + 2 line entities"
        );
    }

    /// BuilderError Display must embed the cell_id and the word "missing" so
    /// log messages and SolveResult::NoProgress reasons are human-readable.
    /// Also verifies the type implements `std::error::Error` so it can be
    /// propagated with `?` or wrapped by any conforming error-aggregation library.
    #[test]
    fn builder_error_display_contains_cell_id() {
        let cell_id = vcid("Test", "x");
        let err = BuilderError {
            cell_id: cell_id.clone(),
            message: format!("non-auto parameter {cell_id} missing from current_values"),
        };

        let display = err.to_string();
        assert!(
            display.contains("missing"),
            "Display should contain 'missing', got: {display}"
        );
        assert!(
            display.contains(&cell_id.to_string()),
            "Display should contain cell_id '{}', got: {display}",
            cell_id
        );

        // Verify it satisfies std::error::Error via trait-object coercion.
        let _: &dyn std::error::Error = &err;
    }

    /// Non-auto param whose cell_id is missing from current_values should return
    /// Err(BuilderError) — a logic error (eval pass incomplete) that must not be
    /// silently swallowed per the project's noisy-error convention.
    #[test]
    fn add_auto_coord_errors_on_missing_non_auto_value() {
        let (mut builder, cell_id, auto_params, current_values) = missing_coord_setup("Test", "x");

        let result = builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_auto_coord");
    }

    /// Error from add_auto_coord should propagate through add_point and
    /// add_pattern_to_builder back to the caller. This verifies the error
    /// propagation chain used by solve()'s Err(reason) arm, exercised via
    /// a hand-crafted GeometricPattern (the path is unreachable via
    /// recognize_pattern because it guards non-auto coords at line 299).
    #[test]
    fn add_pattern_to_builder_propagates_coord_error() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("Test", "bad_coord");

        // Craft a Coincident pattern whose pt_a references the missing cell_id.
        // pt_b is a fixed point so it won't contribute any error.
        let pattern = GeometricPattern::Coincident {
            pt_a: PointRef::Auto {
                x: Some(cell_id.clone()),
                y: None,
                z: None,
            },
            pt_b: PointRef::Fixed {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };

        let result = add_pattern_to_builder(&mut builder, &pattern, &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_pattern_to_builder");
    }

    /// Exercises the `?` on line 1077 (`eb = builder.add_point(pt_b, ...)?`).
    /// pt_a is Fixed (no error), pt_b is Auto with a missing cell_id.
    #[test]
    fn add_pattern_to_builder_propagates_coincident_pt_b_error() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("Test", "bad_pt_b");

        let pattern = GeometricPattern::Coincident {
            pt_a: PointRef::Fixed {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            pt_b: PointRef::Auto {
                x: Some(cell_id.clone()),
                y: None,
                z: None,
            },
        };

        let result = add_pattern_to_builder(&mut builder, &pattern, &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_pattern_to_builder (pt_b path)");
    }

    /// Exercises the `?` on the Parallel arm (line 1046,
    /// `builder.add_line_pair(line_a, line_b, ...)?`).
    /// line_a is fully fixed; line_b.start is Auto with a missing cell_id
    /// (the 3rd add_point call inside add_line_pair).
    #[test]
    fn add_pattern_to_builder_propagates_parallel_line_error() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("Test", "bad_lb_start");

        let pattern = GeometricPattern::Parallel {
            line_a: fixed_line(0.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            line_b: line(
                PointRef::Auto {
                    x: Some(cell_id.clone()),
                    y: None,
                    z: None,
                },
                fixed_point(2.0, 1.0, 0.0),
            ),
        };

        let result = add_pattern_to_builder(&mut builder, &pattern, &auto_params, &current_values);

        assert_missing_err(
            result,
            &cell_id,
            "add_pattern_to_builder (parallel line_b.start path)",
        );
    }

    /// Calling add_auto_coord twice with the same auto-param cell_id must
    /// return the same Slvs_hParam handle and must NOT grow params on the second call.
    #[test]
    fn add_auto_coord_cache_hit_idempotency() {
        let mut builder = SystemBuilder::new();
        let cell_id = vcid("Test", "x");
        let auto_params = vec![single_auto_param(cell_id.clone())];
        let current_values = ValueMap::new();
        let initial_len = builder.params.len();

        // First call — creates the param and inserts into the mapping
        let h1 = builder
            .add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values)
            .expect("first call should succeed");
        let len_after_first = builder.params.len();
        assert_eq!(
            len_after_first,
            initial_len + 1,
            "first call should insert exactly one param"
        );

        // Second call — should hit the cache and return the same handle
        let h2 = builder
            .add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values)
            .expect("second call should succeed");

        assert_eq!(h1, h2, "second call should return the same cached handle");
        assert_eq!(
            builder.params.len(),
            len_after_first,
            "params.len() should not grow on the second (cache-hit) call"
        );
    }

    /// When an auto-param cell_id is present in current_values, add_auto_coord
    /// must use that value as the warm-start initial value instead of the 0.01 default.
    #[test]
    fn add_auto_coord_auto_param_warm_start() {
        let mut builder = SystemBuilder::new();
        let cell_id = vcid("Test", "x");
        let auto_params = vec![single_auto_param(cell_id.clone())];
        let mut current_values = ValueMap::new();
        current_values.insert(
            cell_id.clone(),
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH,
            },
        );

        let h = builder
            .add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values)
            .expect("expected Ok for auto param with current value");

        let param = builder
            .params
            .iter()
            .find(|p| p.h == h)
            .expect("param not found in builder");
        assert_eq!(
            param.val, 5.0,
            "auto param with current value should use that value as warm-start initial"
        );
    }

    /// BuilderError must expose cell_id and message fields, and Display must
    /// output only the message (cell_id is logged as a separate structured field).
    #[test]
    fn builder_error_has_cell_id_and_display() {
        let cell_id = vcid("Test", "x");
        let message = "non-auto parameter Test.x missing from current_values".to_string();
        let err = BuilderError {
            cell_id: cell_id.clone(),
            message: message.clone(),
        };

        assert_eq!(
            err.cell_id, cell_id,
            "cell_id field should match the provided ValueCellId"
        );
        assert_eq!(
            err.message, message,
            "message field should match the provided string"
        );
        assert_eq!(
            err.to_string(),
            message,
            "Display should output only the message, not the cell_id separately"
        );
    }

    /// assert_missing_err must panic when the error's cell_id does not match the
    /// expected cell_id.  This is a negative test for the helper: it verifies the
    /// mismatch-detection path rather than only the happy-path.
    #[test]
    #[should_panic(expected = "panics_on_wrong_cell_id: BuilderError cell_id")]
    fn assert_missing_err_panics_on_wrong_cell_id() {
        let actual_id = vcid("A", "x");
        let expected_id = vcid("B", "y");
        let result: Result<(), BuilderError> = Err(BuilderError {
            cell_id: actual_id.clone(),
            message: format!(
                "non-auto parameter {} missing from current_values",
                actual_id
            ),
        });
        // Passes `expected_id` ("B","y") but the error carries `actual_id` ("A","x") — must panic.
        assert_missing_err(result, &expected_id, "panics_on_wrong_cell_id");
    }

    /// add_point must propagate the Err returned by add_auto_coord when the
    /// x-coordinate cell_id is a non-auto param absent from current_values.
    /// This covers the `?` operator in add_point's PointRef::Auto arm.
    /// Also strengthened to check contains("missing"), consistent with the
    /// other two error-path tests.
    #[test]
    fn add_point_propagates_missing_value_error() {
        let (mut builder, cell_id, auto_params, current_values) = missing_coord_setup("Fixed", "x");

        let pt = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let result = builder.add_point(&pt, &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_point");
    }

    // ── add_line_pair: additional error-propagation tests (S6) ────────────────

    /// `add_line_pair` must propagate Err when `line_a.end` contains a non-auto
    /// cell_id missing from current_values.  The `?` on the second `add_point`
    /// call (`la_end`) must surface the error.
    #[test]
    fn add_line_pair_propagates_error_at_line_a_end() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("LineAEnd", "x");

        let bad_end = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let line_a = line(fixed_point(0.0, 0.0, 0.0), bad_end);
        let line_b = line(fixed_point(0.0, 1.0, 0.0), fixed_point(1.0, 1.0, 0.0));

        let result = builder.add_line_pair(&line_a, &line_b, &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_line_pair (line_a.end)");
    }

    /// `add_line_pair` must propagate Err when `line_b.start` contains a non-auto
    /// cell_id missing from current_values.  The `?` on the third `add_point`
    /// call (`lb_start`) must surface the error.
    #[test]
    fn add_line_pair_propagates_error_at_line_b_start() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("LineBStart", "x");

        let bad_start = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let line_a = line(fixed_point(0.0, 0.0, 0.0), fixed_point(1.0, 0.0, 0.0));
        let line_b = line(bad_start, fixed_point(1.0, 1.0, 0.0));

        let result = builder.add_line_pair(&line_a, &line_b, &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_line_pair (line_b.start)");
    }

    /// `add_line_pair` must propagate Err when `line_b.end` contains a non-auto
    /// cell_id missing from current_values.  The `?` on the fourth `add_point`
    /// call (`lb_end`) must surface the error.
    #[test]
    fn add_line_pair_propagates_error_at_line_b_end() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("LineBEnd", "x");

        let bad_end = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let line_a = line(fixed_point(0.0, 0.0, 0.0), fixed_point(1.0, 0.0, 0.0));
        let line_b = line(fixed_point(0.0, 1.0, 0.0), bad_end);

        let result = builder.add_line_pair(&line_a, &line_b, &auto_params, &current_values);

        assert_missing_err(result, &cell_id, "add_line_pair (line_b.end)");
    }

    // ── add_line_pair: partial-mutation contract test (S3) ────────────────────

    /// When `add_line_pair` fails at `lb_start` (line_b.start has a missing
    /// non-auto coord), the two points for line_a have ALREADY been inserted
    /// into `builder.point_entities`.  This documents *observed* behaviour, not
    /// a hard requirement — a future refactor that adds rollback would be a
    /// defensible improvement.
    #[test]
    fn add_line_pair_partial_mutation_on_error() {
        let (mut builder, cell_id, auto_params, current_values) =
            missing_coord_setup("LineBStart", "x");

        // line_a is fully Fixed — both points will be created before the error
        let bad_start = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let line_a = line(fixed_point(0.0, 0.0, 0.0), fixed_point(1.0, 0.0, 0.0));
        // line_b.start triggers the error; line_b.end is never reached
        let line_b = line(bad_start, fixed_point(1.0, 1.0, 0.0));

        let result = builder.add_line_pair(&line_a, &line_b, &auto_params, &current_values);

        assert!(result.is_err(), "expected Err when lb_start is missing");
        // The two line_a points were inserted before the error — no rollback.
        // Exactly 2: both Fixed, distinct coords (no dedup), line_b never reached.
        assert_eq!(
            builder.point_entities.len(),
            2,
            "builder.point_entities should contain exactly the 2 line_a points \
             (len={}) — add_line_pair has no rollback on Err",
            builder.point_entities.len()
        );
    }

    // ── add_line_pair: dedup shared-endpoint test (S1) ───────────────────────

    /// When two lines share an endpoint (line_a.end == line_b.start as Fixed
    /// coordinates), `add_point` returns the cached entity handle on the second
    /// call.  Only 3 point entities + 2 line entities = 5 total are created,
    /// not 6.  This pins the PointKey::Fixed dedup contract.
    #[test]
    fn add_line_pair_dedups_shared_endpoint() {
        let mut builder = SystemBuilder::new();
        let auto_params: Vec<AutoParam> = vec![];
        let current_values = ValueMap::new();

        // line_a: (0,0,0) → (1,0,0)
        // line_b: (1,0,0) → (2,0,0)  — shares the (1,0,0) endpoint with line_a
        let la = line(fixed_point(0.0, 0.0, 0.0), fixed_point(1.0, 0.0, 0.0));
        let lb = line(fixed_point(1.0, 0.0, 0.0), fixed_point(2.0, 0.0, 0.0));

        let entities = builder
            .add_line_pair(&la, &lb, &auto_params, &current_values)
            .expect("add_line_pair should return Ok");
        // 3 unique Fixed points (deduped) + 2 line segments = 5 entities
        assert_eq!(
            builder.entities.len(),
            5,
            "expected 3 unique point entities + 2 line segment entities when one endpoint \
             is shared"
        );
        assert_eq!(
            builder.point_entities.len(),
            3,
            "PointKey cache should contain exactly 3 unique entries for the shared-endpoint case"
        );
        // Verify the shared endpoint handle propagates into the Slvs_Entity arrays:
        // segment_a.point[1] (la_end = (1,0,0)) must equal segment_b.point[0] (lb_start = (1,0,0))
        let segment_a = builder
            .entities
            .iter()
            .find(|e| e.h == entities.line_a)
            .expect("line_a entity must be present in builder.entities");
        let segment_b = builder
            .entities
            .iter()
            .find(|e| e.h == entities.line_b)
            .expect("line_b entity must be present in builder.entities");
        assert_eq!(
            segment_a.point[1], segment_b.point[0],
            "shared endpoint handle must be identical in both segment Slvs_Entity.point arrays: \
             segment_a.point[1] (la_end) should equal segment_b.point[0] (lb_start)"
        );
    }

    // ── add_line_pair: entity-type guard test (S4) ────────────────────────────

    /// The two handles returned by `add_line_pair` must refer to
    /// `SLVS_E_LINE_SEGMENT` entities, not point entities or any other type.
    /// This guards against a future regression where point handles are
    /// accidentally returned in the wrong positions.
    #[test]
    fn add_line_pair_returns_line_segment_entities() {
        let mut builder = SystemBuilder::new();
        let auto_params: Vec<AutoParam> = vec![];
        let current_values = ValueMap::new();

        let la = line(fixed_point(0.0, 0.0, 0.0), fixed_point(1.0, 0.0, 0.0));
        let lb = line(fixed_point(0.0, 1.0, 0.0), fixed_point(1.0, 1.0, 0.0));

        let entities = builder
            .add_line_pair(&la, &lb, &auto_params, &current_values)
            .expect("add_line_pair should return Ok");

        for (label, handle) in [("line_a", entities.line_a), ("line_b", entities.line_b)] {
            let entity = builder
                .entities
                .iter()
                .find(|e| e.h == handle)
                .unwrap_or_else(|| panic!("{label} handle not found in builder.entities"));
            assert_eq!(
                entity.type_,
                slvs_sys::SLVS_E_LINE_SEGMENT,
                "{label} entity type should be SLVS_E_LINE_SEGMENT"
            );
        }
    }

    /// `add_line_pair` must propagate Err from all four `?` sites inside the function
    /// (line_a.start, line_a.end, line_b.start, line_b.end). Each position is tested
    /// independently to ensure no site swallows errors.
    #[test]
    fn add_line_pair_propagates_error_from_each_position() {
        let (_, cell_id, auto_params, current_values) = missing_coord_setup("Test", "bad");

        // Helper: a Fixed point that always succeeds
        let good = || fixed_point(0.0, 0.0, 0.0);
        // Helper: an Auto point with a non-auto cell_id absent from current_values → Err
        let bad = || PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };

        let positions: &[(&str, LineRef, LineRef)] = &[
            ("line_a.start", line(bad(), good()), line(good(), good())),
            ("line_a.end", line(good(), bad()), line(good(), good())),
            ("line_b.start", line(good(), good()), line(bad(), good())),
            ("line_b.end", line(good(), good()), line(good(), bad())),
        ];

        for (position_name, line_a, line_b) in positions {
            let mut builder = SystemBuilder::new();
            let result = builder.add_line_pair(line_a, line_b, &auto_params, &current_values);
            assert_missing_err(result, &cell_id, position_name);
        }
    }

    /// Documents the *observed* behaviour, **not** a design requirement:
    /// when `add_line_pair` returns Err at the second `?` site (line_a.end),
    /// the first point (line_a.start) has already been registered and is not
    /// rolled back.  A future refactor that adds rollback would be a
    /// defensible improvement — if that happens, update this test rather
    /// than treating the new behaviour as a regression.
    #[test]
    fn add_line_pair_currently_does_not_rollback_on_err() {
        let (_, cell_id, auto_params, current_values) = missing_coord_setup("Test", "bad");
        let mut builder = SystemBuilder::new();

        let initial_entity_count = builder.entities.len();
        let initial_point_count = builder.point_entities.len();

        // line_a.start = Fixed (succeeds, registers 1 point entity)
        // line_a.end = erroring Auto (fails at second ? site)
        let bad_end = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let la = line(fixed_point(0.0, 0.0, 0.0), bad_end);
        let lb = line(fixed_point(1.0, 0.0, 0.0), fixed_point(2.0, 0.0, 0.0));

        let result = builder.add_line_pair(&la, &lb, &auto_params, &current_values);

        assert!(result.is_err(), "expected Err due to erroring Auto point");
        // line_a.start was registered before the failure — at most 1 point entity remains.
        // A future rollback implementation may reduce this to 0.
        assert!(
            builder.entities.len() <= initial_entity_count + 1,
            "at most 1 point entity (line_a.start) should remain after the Err"
        );
        assert!(
            builder.point_entities.len() <= initial_point_count + 1,
            "at most 1 entry in point_entities cache should remain after the Err"
        );
    }
}
