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
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, Diagnostic,
    DimensionVector, ResolutionProblem, Severity, SolveResult, Type, Value, ValueCellId, ValueMap,
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
#[derive(Debug, Clone)]
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
                        return SolveResult::NoProgress { reason: err.message };
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
                SolveResult::Solved { values }
            }
            SlvsSolveResult::Inconsistent { failed_ids } => SolveResult::Infeasible {
                diagnostics: vec![Diagnostic {
                    severity: Severity::Error,
                    message: format!(
                        "geometric constraints are inconsistent ({} failed)",
                        failed_ids.len()
                    ),
                    labels: vec![],
                }],
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
            let la_start = builder.add_point(&line_a.start, auto_params, current_values)?;
            let la_end = builder.add_point(&line_a.end, auto_params, current_values)?;
            let lb_start = builder.add_point(&line_b.start, auto_params, current_values)?;
            let lb_end = builder.add_point(&line_b.end, auto_params, current_values)?;
            let line_a_e = builder.add_line_segment(la_start, la_end);
            let line_b_e = builder.add_line_segment(lb_start, lb_end);
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
            let la_start = builder.add_point(&line_a.start, auto_params, current_values)?;
            let la_end = builder.add_point(&line_a.end, auto_params, current_values)?;
            let lb_start = builder.add_point(&line_b.start, auto_params, current_values)?;
            let lb_end = builder.add_point(&line_b.end, auto_params, current_values)?;
            let line_a_e = builder.add_line_segment(la_start, la_end);
            let line_b_e = builder.add_line_segment(lb_start, lb_end);
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
            let la_start = builder.add_point(&line_a.start, auto_params, current_values)?;
            let la_end = builder.add_point(&line_a.end, auto_params, current_values)?;
            let lb_start = builder.add_point(&line_b.start, auto_params, current_values)?;
            let lb_end = builder.add_point(&line_b.end, auto_params, current_values)?;
            let line_a_e = builder.add_line_segment(la_start, la_end);
            let line_b_e = builder.add_line_segment(lb_start, lb_end);
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

    /// Non-auto param with a value present in current_values should succeed
    /// and use the provided value. Regression guard for the non-auto happy path.
    #[test]
    fn add_auto_coord_succeeds_for_non_auto_with_value() {
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Test", "x");
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

        let result =
            builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        let h = result.expect("expected Ok for non-auto param present in current_values");
        // Verify the param was created with the correct value
        let param = builder.params.iter().find(|p| p.h == h).expect("param not found in builder");
        assert_eq!(param.val, 42.0, "param value should match current_values entry");
    }

    /// Auto param not yet in current_values should get the 0.01 default.
    /// Regression guard: the documented auto-param default must not be changed.
    #[test]
    fn add_auto_coord_auto_param_default_preserved() {
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Test", "x");
        // cell_id IS in auto_params
        let auto_params = vec![AutoParam {
            id: cell_id.clone(),
            param_type: Type::length(),
            bounds: None,
        }];
        // But NOT in current_values — should use 0.01 default
        let current_values = ValueMap::new();

        let result =
            builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        let h = result.expect("expected Ok for auto param");
        let param = builder.params.iter().find(|p| p.h == h).expect("param not found in builder");
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
        let param = builder.params.iter().find(|p| p.h == h).expect("param not found in builder");
        assert_eq!(param.val, 0.0, "None cell_id should produce param with value 0.0");
    }

    /// BuilderError Display must embed the cell_id and the word "missing" so
    /// log messages and SolveResult::NoProgress reasons are human-readable.
    /// Also verifies the type satisfies std::error::Error so it can be used
    /// in ? chains with anyhow / thiserror in the future.
    #[test]
    fn builder_error_display_contains_cell_id() {
        let cell_id = ValueCellId::new("Test", "x");
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
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Test", "x");
        // cell_id is NOT in auto_params — it's a non-auto param
        let auto_params: Vec<AutoParam> = vec![];
        // cell_id is also NOT in current_values — logic error
        let current_values = ValueMap::new();

        let result =
            builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        assert!(
            result.is_err(),
            "expected Err for non-auto param missing from current_values, got Ok"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.cell_id, cell_id,
            "BuilderError cell_id should match the ValueCellId passed to add_auto_coord"
        );
        assert!(
            err.message.contains("missing"),
            "BuilderError message should contain 'missing', got: {}",
            err.message
        );
        // Verify Display produces the same human-readable message
        let display = err.to_string();
        assert!(
            display.contains(&cell_id.to_string()),
            "Display should contain cell_id '{}', got: {display}",
            cell_id
        );
    }

    /// add_auto_coord must return a BuilderError carrying the original
    /// ValueCellId when a non-auto cell_id is absent from current_values,
    /// preserving the id as typed data for downstream consumers.
    #[test]
    fn add_auto_coord_returns_builder_error_with_cell_id() {
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Test", "x");
        let auto_params: Vec<AutoParam> = vec![];
        let current_values = ValueMap::new();

        let result =
            builder.add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values);

        let err = result.expect_err("expected Err, got Ok");
        assert_eq!(err.cell_id, cell_id, "error should carry the original cell_id");
    }

    /// Error from add_auto_coord should propagate through add_point and
    /// add_pattern_to_builder back to the caller. This verifies the error
    /// propagation chain used by solve()'s Err(reason) arm, exercised via
    /// a hand-crafted GeometricPattern (the path is unreachable via
    /// recognize_pattern because it guards non-auto coords at line 299).
    #[test]
    fn add_pattern_to_builder_propagates_coord_error() {
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Test", "bad_coord");
        // cell_id is NOT in auto_params (empty) → non-auto treatment
        let auto_params: Vec<AutoParam> = vec![];
        // cell_id is also NOT in current_values → triggers Err in add_auto_coord
        let current_values = ValueMap::new();

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

        assert!(
            result.is_err(),
            "expected Err when coord cell_id is missing from current_values, got Ok"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.cell_id, cell_id,
            "propagated BuilderError cell_id should match the PointRef::Auto coordinate cell_id"
        );
        assert!(
            err.message.contains("missing"),
            "propagated BuilderError message should contain 'missing', got: {}",
            err.message
        );
        assert!(
            err.to_string().contains(&cell_id.to_string()),
            "propagated error Display should contain cell_id, got: {}",
            err.to_string()
        );
    }

    /// Calling add_auto_coord twice with the same auto-param cell_id must
    /// return the same Slvs_hParam handle and must NOT grow params on the second call.
    #[test]
    fn add_auto_coord_cache_hit_idempotency() {
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Test", "x");
        let auto_params = vec![AutoParam {
            id: cell_id.clone(),
            param_type: Type::length(),
            bounds: None,
        }];
        let current_values = ValueMap::new();

        // First call — creates the param and inserts into the mapping
        let h1 = builder
            .add_auto_coord(&Some(cell_id.clone()), &auto_params, &current_values)
            .expect("first call should succeed");
        let len_after_first = builder.params.len();

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
        let cell_id = ValueCellId::new("Test", "x");
        let auto_params = vec![AutoParam {
            id: cell_id.clone(),
            param_type: Type::length(),
            bounds: None,
        }];
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
        let cell_id = ValueCellId::new("Test", "x");
        let message = "non-auto parameter Test.x missing from current_values".to_string();
        let err = BuilderError { cell_id: cell_id.clone(), message: message.clone() };

        assert_eq!(err.cell_id, cell_id, "cell_id field should match the provided ValueCellId");
        assert_eq!(err.message, message, "message field should match the provided string");
        assert_eq!(
            err.to_string(),
            message,
            "Display should output only the message, not the cell_id separately"
        );
    }

    /// add_point must propagate the Err returned by add_auto_coord when the
    /// x-coordinate cell_id is a non-auto param absent from current_values.
    /// This covers the `?` operator on line 489 of add_point.
    #[test]
    fn add_point_propagates_missing_value_error() {
        let mut builder = SystemBuilder::new();
        let cell_id = ValueCellId::new("Fixed", "y");
        // cell_id is NOT in auto_params (non-auto)
        let auto_params: Vec<AutoParam> = vec![];
        // cell_id is also NOT in current_values — triggers the Err branch
        let current_values = ValueMap::new();

        let pt = PointRef::Auto {
            x: Some(cell_id.clone()),
            y: None,
            z: None,
        };
        let result = builder.add_point(&pt, &auto_params, &current_values);

        assert!(
            result.is_err(),
            "add_point should propagate the Err from add_auto_coord, got Ok"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.cell_id, cell_id,
            "propagated BuilderError cell_id should match the coord cell_id"
        );
    }
}
