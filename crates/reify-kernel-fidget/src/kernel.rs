//! `FidgetKernel` — Tree-backed SDF kernel wired to fidget 0.4's pure-Rust JIT.
//!
//! Each successful `execute(...)` call allocates a fresh
//! [`GeometryHandleId`] and stores a symbolic [`fidget::context::Tree`]
//! against it. SDF point evaluation goes through
//! [`FidgetKernel::evaluate_sdf_at`] which builds a `JitShape` per call
//! (a per-handle `JitShape` cache is a non-breaking optimisation for a
//! follow-up task; see step-10's design note in plan.json).
//!
//! Storing the symbolic `Tree` (not a compiled `JitShape`) keeps the kernel
//! cheap for Boolean composition: Union/Difference/Intersection are
//! Tree-level ops (`min`/`max` on the symbolic graph) and never need to
//! JIT-compile the operands.
//!
//! # v0.2 scope
//!
//! Wired in this task:
//! - `execute(Sphere)` and `execute(Box)` — SDF primitives needed to build
//!   test inputs. Kernel-only; not added to `CapabilityDescriptor` per the
//!   task spec (descriptor side is unchanged).
//! - `execute(Union | Difference | Intersection)` — the three SDF Booleans
//!   the descriptor already claims.
//! - `evaluate_sdf_at(handle, x, y, z)` — JIT-compiled point evaluation
//!   (arch §10.8 "JIT compilation as the production-performance deliverable").
//!
//! Deferred (named follow-up tasks):
//! - `tessellate` (SDF→Mesh feature-preserving meshing — arch §10.8 / lib.rs).
//! - `query` / `export` on Sdf reps (require meshing first).
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/lib.rs:140-143` — `extract_f64` helper
//! pattern for `Value` → `f64` conversion at the GeometryOp boundary.

use std::collections::BTreeMap;

use fidget::context::Tree;
use fidget::shape::EzShape;

use reify_types::{
    BRepKind, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

/// Tree-backed Fidget SDF kernel.
///
/// Internal handle ids start at `1` and increment per allocation; `0` and
/// `u64::MAX` (the [`GeometryHandleId::INVALID`] sentinel) are never returned.
///
/// `Tree` is `Send + Sync` (it wraps `Arc<TreeOp>`), `BTreeMap<K, V>` is
/// auto-`Send + Sync` when `K` and `V` are, and `u64` is trivially both —
/// so `FidgetKernel` is `Send + Sync` without any `unsafe impl`.
pub struct FidgetKernel {
    trees: BTreeMap<GeometryHandleId, Tree>,
    /// Next handle id to hand out. Starts at `1`; INVALID = `u64::MAX`
    /// is structurally unreachable since we'd OOM on `BTreeMap` insertion
    /// long before reaching it.
    next_id: u64,
}

impl FidgetKernel {
    /// Construct a new `FidgetKernel` with no allocated handles.
    pub fn new() -> Self {
        Self {
            trees: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Allocate a fresh handle id (post-increment).
    fn allocate_id(&mut self) -> GeometryHandleId {
        let id = GeometryHandleId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Insert a Tree against a fresh id and return the corresponding
    /// [`GeometryHandle`] with `BRepKind::Solid` repr (the closest
    /// fine-grained classifier for "implicit-surface-defined solid";
    /// see plan.json design decisions).
    fn insert_tree(&mut self, tree: Tree) -> GeometryHandle {
        let id = self.allocate_id();
        self.trees.insert(id, tree);
        GeometryHandle {
            id,
            repr: BRepKind::Solid,
        }
    }

    /// Look up two handles, cloning the underlying Trees. Errors with
    /// `InvalidReference(left)` first (left is checked before right) — the
    /// stable contract pinned by
    /// `fidget_kernel_execute_boolean_with_unknown_handle_returns_invalid_reference`.
    fn lookup_pair(
        &self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(Tree, Tree), GeometryError> {
        let a = self
            .trees
            .get(&left)
            .ok_or(GeometryError::InvalidReference(left))?
            .clone();
        let b = self
            .trees
            .get(&right)
            .ok_or(GeometryError::InvalidReference(right))?
            .clone();
        Ok((a, b))
    }

    /// Build the SDF of a sphere of radius `r` centred at the origin:
    /// `sqrt(x² + y² + z²) − r`.
    fn sphere_tree(r: f64) -> Tree {
        let x = Tree::x();
        let y = Tree::y();
        let z = Tree::z();
        // (x² + y² + z²).sqrt() − r
        let r_sq = x.square() + y.square() + z.square();
        r_sq.sqrt() - r
    }

    /// Build the standard Inigo-Quilez axis-aligned-box SDF for a box
    /// centred at the origin with full extents `(w, h, d)`. Half-extents
    /// `b = (w/2, h/2, d/2)`:
    ///
    /// ```text
    /// q = abs(p) − b
    /// length(max(q, 0)) + min(max(q.x, q.y, q.z), 0)
    /// ```
    ///
    /// The first term measures distance outside the box; the second term
    /// measures depth inside the box (negative).
    fn box_tree(w: f64, h: f64, d: f64) -> Tree {
        let bx = w * 0.5;
        let by = h * 0.5;
        let bz = d * 0.5;

        // q = |p| − b
        let qx = Tree::x().abs() - bx;
        let qy = Tree::y().abs() - by;
        let qz = Tree::z().abs() - bz;

        // outside_part = sqrt(max(qx,0)² + max(qy,0)² + max(qz,0)²)
        let qx_pos = qx.max(0.0);
        let qy_pos = qy.max(0.0);
        let qz_pos = qz.max(0.0);
        let outside_part =
            (qx_pos.square() + qy_pos.square() + qz_pos.square()).sqrt();

        // inside_part = min(max(qx, qy, qz), 0)
        let inside_part = qx.max(qy.clone()).max(qz.clone()).min(0.0);

        outside_part + inside_part
    }

    /// Public SDF evaluation entry point.
    ///
    /// Builds a `JitShape::from(tree.clone())`, requests a
    /// `ez_point_tape()`, and runs `eval(&tape, x, y, z)`. Per-call JIT
    /// compilation is acceptable for v0.2 — a `Mutex<HashMap<GeometryHandleId,
    /// JitShape>>` cache layer is a non-breaking optimisation later.
    ///
    /// `f32` mirrors fidget's native float width; reify's `f64` callers
    /// should cast at the boundary.
    pub fn evaluate_sdf_at(
        &self,
        handle: GeometryHandleId,
        x: f32,
        y: f32,
        z: f32,
    ) -> Result<f32, QueryError> {
        let tree = self.trees.get(&handle).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "Fidget SDF kernel: invalid handle {} (no Tree registered)",
                handle.0
            ))
        })?;

        let shape = fidget::jit::JitShape::from(tree.clone());
        let mut eval = fidget::jit::JitShape::new_point_eval();
        let tape = shape.ez_point_tape();
        let (value, _trace) = eval
            .eval(&tape, x, y, z)
            .map_err(|e| QueryError::QueryFailed(format!("Fidget SDF eval failed: {e}")))?;
        Ok(value)
    }
}

impl Default for FidgetKernel {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract an `f64` from a `Value` (Int/Real/Scalar). Mirrors the OCCT
/// adapter's `extract_f64` (`crates/reify-kernel-occt/src/lib.rs:140-143`).
fn extract_f64(v: &Value) -> Result<f64, GeometryError> {
    v.as_f64()
        .ok_or_else(|| GeometryError::OperationFailed("expected numeric value".into()))
}

/// Stable static label for a `GeometryOp` variant — used in error
/// messages so the format string interpolates a stable token rather than
/// the full `Debug` print.
fn op_kind_name(op: &GeometryOp) -> &'static str {
    match op {
        GeometryOp::Box { .. } => "Box",
        GeometryOp::Cylinder { .. } => "Cylinder",
        GeometryOp::Sphere { .. } => "Sphere",
        GeometryOp::Tube { .. } => "Tube",
        GeometryOp::Union { .. } => "Union",
        GeometryOp::Difference { .. } => "Difference",
        GeometryOp::Intersection { .. } => "Intersection",
        GeometryOp::Fillet { .. } => "Fillet",
        GeometryOp::Chamfer { .. } => "Chamfer",
        GeometryOp::Translate { .. } => "Translate",
        GeometryOp::Rotate { .. } => "Rotate",
        GeometryOp::Scale { .. } => "Scale",
        GeometryOp::RotateAround { .. } => "RotateAround",
        GeometryOp::LinearPattern { .. } => "LinearPattern",
        GeometryOp::CircularPattern { .. } => "CircularPattern",
        GeometryOp::Mirror { .. } => "Mirror",
        GeometryOp::LinearPattern2D { .. } => "LinearPattern2D",
        GeometryOp::ArbitraryPattern { .. } => "ArbitraryPattern",
        GeometryOp::Loft { .. } => "Loft",
        GeometryOp::Extrude { .. } => "Extrude",
        GeometryOp::Revolve { .. } => "Revolve",
        GeometryOp::Sweep { .. } => "Sweep",
        GeometryOp::Pipe { .. } => "Pipe",
        GeometryOp::ExtrudeSymmetric { .. } => "ExtrudeSymmetric",
        GeometryOp::SweepGuided { .. } => "SweepGuided",
        GeometryOp::LoftGuided { .. } => "LoftGuided",
        GeometryOp::LineSegment { .. } => "LineSegment",
        GeometryOp::Arc { .. } => "Arc",
        GeometryOp::Helix { .. } => "Helix",
        GeometryOp::InterpCurve { .. } => "InterpCurve",
        GeometryOp::BezierCurve { .. } => "BezierCurve",
        GeometryOp::NurbsCurve { .. } => "NurbsCurve",
        GeometryOp::Draft { .. } => "Draft",
        GeometryOp::Thicken { .. } => "Thicken",
        GeometryOp::Shell { .. } => "Shell",
    }
}

impl GeometryKernel for FidgetKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        match op {
            GeometryOp::Sphere { radius } => {
                let r = extract_f64(radius)?;
                let tree = Self::sphere_tree(r);
                Ok(self.insert_tree(tree))
            }
            GeometryOp::Box {
                width,
                height,
                depth,
            } => {
                let w = extract_f64(width)?;
                let h = extract_f64(height)?;
                let d = extract_f64(depth)?;
                let tree = Self::box_tree(w, h, d);
                Ok(self.insert_tree(tree))
            }
            GeometryOp::Union { left, right } => {
                let (a, b) = self.lookup_pair(*left, *right)?;
                let tree = a.min(b);
                Ok(self.insert_tree(tree))
            }
            GeometryOp::Intersection { left, right } => {
                let (a, b) = self.lookup_pair(*left, *right)?;
                let tree = a.max(b);
                Ok(self.insert_tree(tree))
            }
            GeometryOp::Difference { left, right } => {
                // Difference (left − right) is `max(a, neg(b))` — the
                // half-space outside `right` intersected with `left`.
                let (a, b) = self.lookup_pair(*left, *right)?;
                let tree = a.max(b.neg());
                Ok(self.insert_tree(tree))
            }
            other => Err(GeometryError::OperationFailed(format!(
                "Fidget SDF kernel: {} not yet supported on Sdf representation",
                op_kind_name(other)
            ))),
        }
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(
            "Fidget SDF kernel: queries not yet supported on Sdf representation".into(),
        ))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(
            "Fidget SDF kernel: export not yet supported on Sdf representation".into(),
        ))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(
            "Fidget SDF kernel: SDF→Mesh feature-preserving meshing is the v0.2 \
             follow-up named in arch §10.8 / docs/prds/v0_2/multi-kernel.md \
             (deferred from this task by design)"
                .into(),
        ))
    }
    // extract_edges, extract_faces, execute_with_history, query_many all use
    // the trait defaults — they error in the standard "not supported" fashion.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trait-conformance pin: `FidgetKernel` must be `Send + Sync` and
    /// upcastable to `Box<dyn GeometryKernel>` (the dyn-safe trait surface
    /// `KernelRegistration::factory` returns).
    ///
    /// Replaces the `assert_stub_kernel_errors!(FidgetKernel::new, "Fidget")`
    /// macro invocation: that macro asserted every op returns `Err`, which
    /// is exactly what the wired-in implementation contradicts (Sphere/Box
    /// and the SDF Booleans now succeed).
    #[test]
    fn fidget_kernel_is_send_sync_and_object_safe() {
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        let kernel = FidgetKernel::new();
        assert_send_sync(&kernel);
        let _boxed: Box<dyn GeometryKernel> = Box::new(FidgetKernel::new());
    }

    /// Pins the contract that `execute(GeometryOp::Sphere { radius })`
    /// returns a fresh handle with `BRepKind::Solid` (the closest
    /// fine-grained classifier for "implicit-surface-defined solid"; see
    /// design decision in plan).
    #[test]
    fn fidget_kernel_execute_sphere_returns_handle_with_solid_repr() {
        let mut kernel = FidgetKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(1.0),
        });
        let handle = result.expect("Sphere execution must succeed on FidgetKernel");
        assert_eq!(handle.repr, BRepKind::Solid);
        assert_ne!(
            handle.id,
            GeometryHandleId::INVALID,
            "FidgetKernel must allocate a real handle id, not the INVALID sentinel",
        );
    }

    /// Pins the Box-primitive SDF construction. The body in step-2 still
    /// rejects Box via the catch-all "not yet supported" branch, so this
    /// test fails until step-4 wires the standard Inigo-Quilez box SDF.
    #[test]
    fn fidget_kernel_execute_box_returns_handle() {
        let mut kernel = FidgetKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(2.0),
            height: Value::Real(2.0),
            depth: Value::Real(2.0),
        });
        let handle = result.expect("Box execution must succeed on FidgetKernel");
        assert_eq!(handle.repr, BRepKind::Solid);
        assert_ne!(handle.id, GeometryHandleId::INVALID);
    }

    /// Helper: build two unit-radius spheres and return their handles.
    fn two_spheres(kernel: &mut FidgetKernel) -> (GeometryHandleId, GeometryHandleId) {
        let a = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(1.0),
            })
            .expect("first Sphere");
        let b = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(1.0),
            })
            .expect("second Sphere");
        (a.id, b.id)
    }

    #[test]
    fn fidget_kernel_execute_union_composes_two_spheres() {
        let mut kernel = FidgetKernel::new();
        let (left, right) = two_spheres(&mut kernel);
        let union = kernel
            .execute(&GeometryOp::Union { left, right })
            .expect("Union must succeed on FidgetKernel");
        assert_eq!(union.repr, BRepKind::Solid);
        assert_ne!(union.id, GeometryHandleId::INVALID);
        assert_ne!(union.id, left);
        assert_ne!(union.id, right);
    }

    #[test]
    fn fidget_kernel_execute_difference_composes_two_spheres() {
        let mut kernel = FidgetKernel::new();
        let (left, right) = two_spheres(&mut kernel);
        let diff = kernel
            .execute(&GeometryOp::Difference { left, right })
            .expect("Difference must succeed on FidgetKernel");
        assert_eq!(diff.repr, BRepKind::Solid);
        assert_ne!(diff.id, GeometryHandleId::INVALID);
        assert_ne!(diff.id, left);
        assert_ne!(diff.id, right);
    }

    #[test]
    fn fidget_kernel_execute_intersection_composes_two_spheres() {
        let mut kernel = FidgetKernel::new();
        let (left, right) = two_spheres(&mut kernel);
        let inter = kernel
            .execute(&GeometryOp::Intersection { left, right })
            .expect("Intersection must succeed on FidgetKernel");
        assert_eq!(inter.repr, BRepKind::Solid);
        assert_ne!(inter.id, GeometryHandleId::INVALID);
        assert_ne!(inter.id, left);
        assert_ne!(inter.id, right);
    }

    /// Pins the stable contract that the FIRST missing handle is the one
    /// named in `InvalidReference` — `left` is checked before `right`.
    #[test]
    fn fidget_kernel_execute_boolean_with_unknown_handle_returns_invalid_reference() {
        let mut kernel = FidgetKernel::new();
        let bogus_left = GeometryHandleId(999);
        let bogus_right = GeometryHandleId(1000);
        let result = kernel.execute(&GeometryOp::Union {
            left: bogus_left,
            right: bogus_right,
        });
        match result {
            Err(GeometryError::InvalidReference(id)) => {
                assert_eq!(id, bogus_left, "first missing handle must be named");
            }
            other => panic!(
                "expected Err(InvalidReference({:?})), got {:?}",
                bogus_left, other
            ),
        }
    }
}
