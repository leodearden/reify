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

use reify_types::{
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, Diagnostic, DiagnosticCode,
    DimensionVector, ResolutionProblem, SolveResult, Type, Value, ValueCellId, ValueMap,
};

use crate::slvs_sys::{
    self, SLVS_C_ANGLE, SLVS_C_PARALLEL, SLVS_C_PERPENDICULAR, SLVS_C_POINTS_COINCIDENT,
    SLVS_C_PT_PT_DISTANCE, SLVS_FREE_IN_3D, SLVS_RESULT_DIDNT_CONVERGE, SLVS_RESULT_INCONSISTENT,
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
}

impl HandleAlloc {
    fn new() -> Self {
        Self {
            next_param: Slvs_hParam(1),
            next_entity: Slvs_hEntity(1),
            next_constraint: Slvs_hConstraint(1),
        }
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

    /// Add a constraint on a specific workplane (or `SLVS_FREE_IN_3D` for 3D).
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
        let ch = self.alloc.constraint();
        self.constraints.push(Slvs_Constraint::new(
            ch,
            SOLVE_GROUP,
            type_,
            wrkpl,
            val_a,
            pt_a,
            pt_b,
            entity_a,
            entity_b,
        ));
    }

    /// Solve the system and return the result.
    ///
    /// Checks for Vec-length overflow when casting to `c_int` (i32) and
    /// performs bounds-checked access on the `faileds` field returned by
    /// `Slvs_Solve`.
    fn solve(mut self) -> SlvsSolveResult {
        if self.constraints.is_empty() {
            return SlvsSolveResult::Ok {
                params: self.params,
                mapping: self.mapping,
                dof: 0,
            };
        }

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

        let mut failed: Vec<Slvs_hConstraint> = vec![Slvs_hConstraint(0); self.constraints.len()];
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
            slvs_sys::Slvs_Solve(&mut sys, SOLVE_GROUP);
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
        #[allow(dead_code)]
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
                diagnostics: vec![Diagnostic::error(format!(
                        "geometric constraints are inconsistent ({} failed)",
                        failed_ids.len()
                    ))
                    .with_code(DiagnosticCode::ConstraintUnsatisfiable)],
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
