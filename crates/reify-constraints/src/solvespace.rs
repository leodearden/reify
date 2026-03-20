//! SolveSpace geometric constraint solver integration.
//!
//! Implements `ConstraintSolver` using the SolveSpace libslvs C library
//! via hand-written FFI bindings. Creates a fresh solver system per call
//! (stateless), making it trivially Send + Sync.

use std::collections::HashMap;
use std::sync::Mutex;

use reify_types::{
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, Diagnostic,
    DimensionVector, ResolutionProblem, SolveResult, Severity, Type, Value, ValueCellId, ValueMap,
};

use crate::slvs_sys::{
    self, Slvs_Constraint, Slvs_Entity, Slvs_Param, Slvs_System, Slvs_hConstraint, Slvs_hEntity,
    Slvs_hGroup, Slvs_hParam, SLVS_C_ANGLE, SLVS_C_PARALLEL, SLVS_C_PERPENDICULAR,
    SLVS_C_POINTS_COINCIDENT, SLVS_C_PT_PT_DISTANCE, SLVS_FREE_IN_3D, SLVS_RESULT_DIDNT_CONVERGE,
    SLVS_RESULT_INCONSISTENT, SLVS_RESULT_OKAY, SLVS_RESULT_TOO_MANY_UNKNOWNS,
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

/// A line reference: two points.
#[derive(Debug, Clone)]
struct LineRef {
    start: PointRef,
    end: PointRef,
}

/// Try to recognize a geometric constraint pattern from an expression tree.
fn recognize_pattern(
    expr: &CompiledExpr,
    auto_params: &[AutoParam],
) -> Option<GeometricPattern> {
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
                try_line_pair_constraint(args, auto_params)
                    .map(|(a, b)| GeometricPattern::Parallel {
                        line_a: a,
                        line_b: b,
                    })
            } else if qn.contains("perpendicular") {
                try_line_pair_constraint(args, auto_params)
                    .map(|(a, b)| GeometricPattern::Perpendicular {
                        line_a: a,
                        line_b: b,
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
        if qn.contains("distance") || qn.contains("pt_pt_distance") {
            if args.len() == 2 {
                let pt_a = extract_point_ref(&args[0], auto_params)?;
                let pt_b = extract_point_ref(&args[1], auto_params)?;
                let distance_si = extract_scalar_si(val_expr)?;
                // distance == 0 is a coincident constraint
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
        if qn.contains("angle") {
            if args.len() == 2 {
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
fn extract_point_ref(
    expr: &CompiledExpr,
    auto_params: &[AutoParam],
) -> Option<PointRef> {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            let qn = &function.qualified_name;
            if qn.contains("point3d") || qn.contains("point") {
                if args.len() >= 2 {
                    let x = extract_coord(&args[0], auto_params);
                    let y = extract_coord(&args[1], auto_params);
                    let z = if args.len() >= 3 {
                        extract_coord(&args[2], auto_params)
                    } else {
                        CoordRef::Fixed(0.0)
                    };
                    return Some(make_point_ref(x, y, z));
                }
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
fn extract_line_ref(
    expr: &CompiledExpr,
    auto_params: &[AutoParam],
) -> Option<LineRef> {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            let qn = &function.qualified_name;
            if qn.contains("line") || qn.contains("line_segment") {
                if args.len() == 2 {
                    let start = extract_point_ref(&args[0], auto_params)?;
                    let end = extract_point_ref(&args[1], auto_params)?;
                    return Some(LineRef { start, end });
                }
            }
            // Also handle direct point pair for angle constraints
            if args.len() == 2 {
                if let (Some(start), Some(end)) = (
                    extract_point_ref(&args[0], auto_params),
                    extract_point_ref(&args[1], auto_params),
                ) {
                    return Some(LineRef { start, end });
                }
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

fn extract_coord(expr: &CompiledExpr, auto_params: &[AutoParam]) -> CoordRef {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) if is_auto_param(id, auto_params) => {
            CoordRef::Auto(id.clone())
        }
        CompiledExprKind::Literal(val) => CoordRef::Fixed(val.as_f64().unwrap_or(0.0)),
        _ => CoordRef::Fixed(0.0),
    }
}

fn make_point_ref(x: CoordRef, y: CoordRef, z: CoordRef) -> PointRef {
    match (&x, &y, &z) {
        (CoordRef::Fixed(fx), CoordRef::Fixed(fy), CoordRef::Fixed(fz)) => {
            PointRef::Fixed {
                x: *fx,
                y: *fy,
                z: *fz,
            }
        }
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
struct HandleAlloc {
    next_param: Slvs_hParam,
    next_entity: Slvs_hEntity,
    next_constraint: Slvs_hConstraint,
}

impl HandleAlloc {
    fn new() -> Self {
        Self {
            next_param: 1,
            next_entity: 1,
            next_constraint: 1,
        }
    }

    fn param(&mut self) -> Slvs_hParam {
        let h = self.next_param;
        self.next_param += 1;
        h
    }

    fn entity(&mut self) -> Slvs_hEntity {
        let h = self.next_entity;
        self.next_entity += 1;
        h
    }

    fn constraint(&mut self) -> Slvs_hConstraint {
        let h = self.next_constraint;
        self.next_constraint += 1;
        h
    }
}

/// Maps between Reify ValueCellIds and slvs parameter handles.
struct ParamMapping {
    /// ValueCellId → slvs param handle
    cell_to_param: HashMap<ValueCellId, Slvs_hParam>,
    /// slvs param handle → ValueCellId
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

const FIXED_GROUP: Slvs_hGroup = 1;
const SOLVE_GROUP: Slvs_hGroup = 2;

/// Key to deduplicate point entities.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum PointKey {
    Auto(Option<ValueCellId>, Option<ValueCellId>, Option<ValueCellId>),
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
    fn add_point(
        &mut self,
        pt: &PointRef,
        auto_params: &[AutoParam],
        current_values: &ValueMap,
    ) -> Slvs_hEntity {
        let key = point_key(pt);
        if let Some(&h) = self.point_entities.get(&key) {
            return h;
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
                eh
            }
            PointRef::Auto {
                x: x_id,
                y: y_id,
                z: z_id,
            } => {
                let px = self.add_auto_coord(x_id, auto_params, current_values);
                let py = self.add_auto_coord(y_id, auto_params, current_values);
                let pz = self.add_auto_coord(z_id, auto_params, current_values);
                let eh = self.alloc.entity();
                self.entities
                    .push(Slvs_Entity::point_3d(eh, SOLVE_GROUP, px, py, pz));
                self.point_entities.insert(key, eh);
                eh
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
    fn add_auto_coord(
        &mut self,
        cell_id: &Option<ValueCellId>,
        auto_params: &[AutoParam],
        current_values: &ValueMap,
    ) -> Slvs_hParam {
        if let Some(id) = cell_id {
            // Check if already mapped
            if let Some(h) = self.mapping.get_param(id) {
                return h;
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
                return h;
            }
            // Not an auto param — put in SOLVE_GROUP with current value
            // (avoids mixed-group Jacobian issues, but not mapped so value is ignored)
            let val = current_values
                .get(id)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let h = self.alloc.param();
            self.params.push(Slvs_Param::new(h, SOLVE_GROUP, val));
            h
        } else {
            // No cell_id — put in SOLVE_GROUP at 0 (not mapped, value ignored)
            let h = self.alloc.param();
            self.params.push(Slvs_Param::new(h, SOLVE_GROUP, 0.0));
            h
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
        let mut normal_entity = Slvs_Entity::zeroed_with(normal_e, FIXED_GROUP, slvs_sys::SLVS_E_NORMAL_IN_3D);
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

    /// Add a constraint, optionally on a workplane.
    fn add_constraint(
        &mut self,
        type_: std::os::raw::c_int,
        val_a: f64,
        pt_a: Slvs_hEntity,
        pt_b: Slvs_hEntity,
        entity_a: Slvs_hEntity,
        entity_b: Slvs_hEntity,
    ) {
        self.add_constraint_wrkpl(type_, SLVS_FREE_IN_3D, val_a, pt_a, pt_b, entity_a, entity_b);
    }

    /// Add a constraint on a specific workplane.
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
    fn solve(mut self) -> SlvsSolveResult {
        if self.constraints.is_empty() {
            return SlvsSolveResult::Ok {
                params: self.params,
                mapping: self.mapping,
                dof: 0,
            };
        }

        let mut failed: Vec<Slvs_hConstraint> = vec![0; self.constraints.len()];

        let mut sys = Slvs_System {
            param: self.params.as_mut_ptr(),
            params: self.params.len() as i32,
            entity: self.entities.as_mut_ptr(),
            entities: self.entities.len() as i32,
            constraint: self.constraints.as_mut_ptr(),
            constraints: self.constraints.len() as i32,
            dragged: [0; 4],
            calculateFaileds: 1,
            failed: failed.as_mut_ptr(),
            faileds: failed.len() as i32,
            dof: 0,
            result: 0,
        };

        // Lock the global mutex — libslvs uses internal global state and
        // is not safe to call concurrently.
        let _guard = SLVS_LOCK.lock().unwrap_or_else(|e| e.into_inner());

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
                let n_failed = sys.faileds as usize;
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
        dof: i32,
    },
    Inconsistent {
        failed_ids: Vec<Slvs_hConstraint>,
    },
    DidntConverge,
    TooManyUnknowns,
    UnknownError(i32),
}

fn point_key(pt: &PointRef) -> PointKey {
    match pt {
        PointRef::Auto { x, y, z } => PointKey::Auto(x.clone(), y.clone(), z.clone()),
        PointRef::Fixed { x, y, z } => {
            PointKey::Fixed(x.to_bits(), y.to_bits(), z.to_bits())
        }
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
                    add_pattern_to_builder(
                        &mut builder,
                        &pattern,
                        &problem.auto_params,
                        &problem.current_values,
                    );
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
            SlvsSolveResult::UnknownError(code) => SolveResult::NoProgress {
                reason: format!("SolveSpace solver returned unknown error code {}", code),
            },
        }
    }
}

/// Add a recognized pattern to the system builder.
fn add_pattern_to_builder(
    builder: &mut SystemBuilder,
    pattern: &GeometricPattern,
    auto_params: &[AutoParam],
    current_values: &ValueMap,
) {
    match pattern {
        GeometricPattern::PtPtDistance {
            pt_a,
            pt_b,
            distance_si,
        } => {
            let ea = builder.add_point(pt_a, auto_params, current_values);
            let eb = builder.add_point(pt_b, auto_params, current_values);
            builder.add_constraint(SLVS_C_PT_PT_DISTANCE, *distance_si, ea, eb, 0, 0);
        }
        GeometricPattern::Angle {
            line_a,
            line_b,
            angle_deg,
        } => {
            let la_start = builder.add_point(&line_a.start, auto_params, current_values);
            let la_end = builder.add_point(&line_a.end, auto_params, current_values);
            let lb_start = builder.add_point(&line_b.start, auto_params, current_values);
            let lb_end = builder.add_point(&line_b.end, auto_params, current_values);
            let line_a_e = builder.add_line_segment(la_start, la_end);
            let line_b_e = builder.add_line_segment(lb_start, lb_end);
            builder.add_constraint(SLVS_C_ANGLE, *angle_deg, 0, 0, line_a_e, line_b_e);
        }
        GeometricPattern::Parallel { line_a, line_b } => {
            let la_start = builder.add_point(&line_a.start, auto_params, current_values);
            let la_end = builder.add_point(&line_a.end, auto_params, current_values);
            let lb_start = builder.add_point(&line_b.start, auto_params, current_values);
            let lb_end = builder.add_point(&line_b.end, auto_params, current_values);
            let line_a_e = builder.add_line_segment(la_start, la_end);
            let line_b_e = builder.add_line_segment(lb_start, lb_end);
            // Parallel/perpendicular require a workplane in SolveSpace
            let wp = builder.get_workplane();
            builder.add_constraint_wrkpl(SLVS_C_PARALLEL, wp, 0.0, 0, 0, line_a_e, line_b_e);
        }
        GeometricPattern::Perpendicular { line_a, line_b } => {
            let la_start = builder.add_point(&line_a.start, auto_params, current_values);
            let la_end = builder.add_point(&line_a.end, auto_params, current_values);
            let lb_start = builder.add_point(&line_b.start, auto_params, current_values);
            let lb_end = builder.add_point(&line_b.end, auto_params, current_values);
            let line_a_e = builder.add_line_segment(la_start, la_end);
            let line_b_e = builder.add_line_segment(lb_start, lb_end);
            let wp = builder.get_workplane();
            builder.add_constraint_wrkpl(SLVS_C_PERPENDICULAR, wp, 0.0, 0, 0, line_a_e, line_b_e);
        }
        GeometricPattern::Coincident { pt_a, pt_b } => {
            let ea = builder.add_point(pt_a, auto_params, current_values);
            let eb = builder.add_point(pt_b, auto_params, current_values);
            builder.add_constraint(SLVS_C_POINTS_COINCIDENT, 0.0, ea, eb, 0, 0);
        }
    }
}

/// Extract the DimensionVector from a Type.
fn dimension_of(ty: &Type) -> DimensionVector {
    match ty {
        Type::Scalar { dimension } => *dimension,
        _ => DimensionVector::DIMENSIONLESS,
    }
}

