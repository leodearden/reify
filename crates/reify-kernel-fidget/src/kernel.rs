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

use reify_ir::{BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE, TessError, Value};

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
    ///
    /// Uses `checked_add` so the "BTreeMap would OOM first" invariant is
    /// load-bearing in code rather than only in prose: if we ever reach
    /// `u64::MAX` allocations the panic message points back here.
    fn allocate_id(&mut self) -> GeometryHandleId {
        let id = GeometryHandleId(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect(
            "FidgetKernel handle id overflow — handle BTreeMap would have \
             OOM'd long before this; if you see this panic, the invariant \
             was wrong",
        );
        id
    }

    /// Insert a Tree against a fresh id and return the corresponding
    /// [`GeometryHandle`].
    ///
    /// `repr` is `None`: Fidget's symbolic SDF [`Tree`] belongs to the
    /// [`ReprKind::Sdf`] family — an SDF is `f(x,y,z) → distance`, not a
    /// topology, so there is no meaningful B-rep sub-shape classification.
    /// `repr` carries `None` per task 3179's architectural decision (option
    /// (b)). See also task 3093 review esc-3093-33, which first identified
    /// the semantic abuse.
    fn insert_tree(&mut self, tree: Tree) -> GeometryHandle {
        let id = self.allocate_id();
        self.trees.insert(id, tree);
        GeometryHandle { id, repr: None }
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
    ///
    /// # Precondition
    ///
    /// Callers must pass finite positive extents (`w`, `h`, `d` all satisfy
    /// `value.is_finite() && value > 0.0`). Input validation is enforced at
    /// the `execute(Box)` boundary before this method is called.
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
        let outside_part = (qx_pos.square() + qy_pos.square() + qz_pos.square()).sqrt();

        // inside_part = min(max(qx, qy, qz), 0)
        // qy, qz are not used after this expression — move them in directly.
        let inside_part = qx.max(qy).max(qz).min(0.0);

        outside_part + inside_part
    }

    /// Public SDF evaluation entry point.
    ///
    /// Builds a `JitShape::from(tree.clone())`, requests a
    /// `ez_point_tape()`, and runs `eval(&tape, x, y, z)`. Per-call JIT
    /// compilation is acceptable for v0.2 — a `Mutex<HashMap<GeometryHandleId,
    /// JitShape>>` cache layer is a non-breaking optimisation later.
    ///
    /// TODO(jit-cache): callers that evaluate the same handle many times
    /// (e.g. per-pixel raster sampling) currently pay one full JIT
    /// compilation per call. A per-handle `JitShape` cache (keyed on the
    /// `GeometryHandleId`, invalidated when the Tree changes — which it
    /// never does today since handles are immutable post-insert) is
    /// non-breaking and should be the next optimisation. File a follow-up
    /// task before any caller starts hot-looping this path.
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

/// Returns `true` iff `v` is both finite and strictly positive.
///
/// Centralises the `v.is_finite() && v > 0.0` predicate so that
/// `validate_positive_finite` and the `Box` arm's combined-dimension check
/// share a single definition; if the "positive-finite" contract ever tightens
/// (e.g. to exclude subnormals) only this site needs updating.
#[inline]
fn is_positive_finite(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

impl GeometryKernel for FidgetKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        match op {
            // Input validation runs at the boundary so `sphere_tree` can
            // assume a finite positive radius.
            GeometryOp::Sphere { radius } => {
                let r = extract_f64(radius)?;
                if !is_positive_finite(r) {
                    return Err(GeometryError::OperationFailed(
                        SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE.into(),
                    ));
                }
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
                // Combined check: all three dimensions validated together so
                // a single shared const covers any failure.  Using
                // `BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE` (from
                // `reify_types`) makes the error string byte-identical to
                // OCCT's emission — structural, not just conventional.
                if !(is_positive_finite(w) && is_positive_finite(h) && is_positive_finite(d)) {
                    return Err(GeometryError::OperationFailed(
                        BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE.into(),
                    ));
                }
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
            // The catch-all message names (a) the rejected op, (b) the repr
            // family (Sdf), and (c) the kernel identity (Fidget) so readers
            // can attribute the failure. The
            // fidget_kernel_execute_unsupported_op_names_op_in_message test
            // pins this format over "Fillet" and "Translate".
            other => Err(GeometryError::OperationFailed(format!(
                "Fidget SDF kernel: {} not yet supported on Sdf representation",
                other.kind_name()
            ))),
        }
    }

    /// Note on handle validation: this method does NOT check whether the
    /// handle on the query refers to a registered Tree before returning the
    /// "not yet supported" error. That's a small inconsistency with
    /// `execute(Boolean { ... })` — which surfaces `InvalidReference` for
    /// unknown handles — but it's deliberate: every Sdf query is uniformly
    /// unsupported (queries require meshing first), so reporting the
    /// handle-validity status would be both misleading (the operator's
    /// problem isn't the handle, it's the unsupported op) and wasteful
    /// (the lookup adds cost for no diagnostic value). When the
    /// SDF→Mesh meshing follow-up lands and queries become available,
    /// the handle-validity check moves to the front of this method.
    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        // The catch-all message names (a) the rejected query (via kind_name()),
        // (b) the repr family (Sdf), and (c) the kernel identity (Fidget) so
        // readers can attribute the failure. The
        // fidget_kernel_query_export_tessellate_each_emit_op_specific_message test
        // pins this format over GeometryQuery::Volume.
        Err(QueryError::QueryFailed(format!(
            "Fidget SDF kernel: {} queries on Sdf require meshing — see arch §10.8 \
             (SDF→Mesh follow-up task)",
            query.kind_name(),
        )))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(format!(
            "Fidget SDF kernel: {format:?} export from an Sdf representation is not \
             supported — Sdf→BRep conversion is a v0.3 follow-up",
        )))
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
    /// returns a fresh handle with `repr: None`.
    ///
    /// # Architectural context
    ///
    /// A Fidget SDF [`Tree`] belongs to the [`ReprKind::Sdf`] family — an SDF
    /// is `f(x,y,z) → distance`, not a topology, so there is no meaningful
    /// B-rep sub-shape classification. `repr` must be `None` per task 3179's
    /// architectural decision (option (b)).
    ///
    /// - **Task 3093 review esc-3093-33**: The original semantic-abuse
    ///   acknowledgement — `insert_tree` once carried an inline comment "the
    ///   closest fine-grained classifier for 'implicit-surface-defined solid'",
    ///   explicitly noting the misclassification.
    /// - **Architectural rule**: `BRepKind` is a *B-rep sub-shape classifier
    ///   for OCCT handles*. Non-B-rep kernels (Mesh/Sdf/Voxel/VolumeMesh
    ///   families per [`ReprKind`]) have no B-rep sub-shape. `None` is
    ///   structurally honest and guards against re-filing `ReprKind::Sdf`
    ///   handles under a B-rep variant.
    #[test]
    fn fidget_kernel_execute_sphere_returns_handle_with_unclassified_repr() {
        let mut kernel = FidgetKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(1.0),
        });
        let handle = result.expect("Sphere execution must succeed on FidgetKernel");
        assert!(handle.repr.is_none());
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
        assert!(handle.repr.is_none());
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
        assert!(union.repr.is_none());
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
        assert!(diff.repr.is_none());
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
        assert!(inter.repr.is_none());
        assert_ne!(inter.id, GeometryHandleId::INVALID);
        assert_ne!(inter.id, left);
        assert_ne!(inter.id, right);
    }

    /// Catch-all messages must name (a) the rejected op, (b) the repr
    /// family Fidget answers on, and (c) the kernel's identity — so a
    /// regression that drops the op-token interpolation is caught here.
    #[test]
    fn fidget_kernel_execute_unsupported_op_names_op_in_message() {
        let mut kernel = FidgetKernel::new();

        let err = kernel
            .execute(&GeometryOp::Fillet {
                target: GeometryHandleId(1),
                radius: Value::Real(0.1),
            })
            .expect_err("Fillet must be rejected on Sdf");
        match err {
            GeometryError::OperationFailed(msg) => {
                assert!(msg.contains("Fillet"), "{msg:?}");
                assert!(msg.contains("Sdf"), "{msg:?}");
                assert!(msg.contains("Fidget"), "{msg:?}");
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }

        let err = kernel
            .execute(&GeometryOp::Translate {
                target: GeometryHandleId(1),
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            })
            .expect_err("Translate must be rejected on Sdf");
        match err {
            GeometryError::OperationFailed(msg) => {
                assert!(msg.contains("Translate"), "{msg:?}");
                assert!(msg.contains("Sdf"), "{msg:?}");
                assert!(msg.contains("Fidget"), "{msg:?}");
            }
            other => panic!("expected OperationFailed, got {other:?}"),
        }
    }

    /// Sphere SDF must match the analytical formula to within 1e-5 at
    /// canonical sample points (origin → −r, on-surface → 0, outside → +d).
    #[test]
    fn fidget_kernel_evaluate_sdf_at_sphere_matches_analytical() {
        let mut kernel = FidgetKernel::new();
        let sphere = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(1.0),
            })
            .expect("Sphere build");
        let h = sphere.id;

        let cases: &[(f32, f32, f32, f32)] = &[
            (0.0, 0.0, 0.0, -1.0),
            (1.0, 0.0, 0.0, 0.0),
            (2.0, 0.0, 0.0, 1.0),
            (0.5, 0.5, 0.5, (0.75_f32).sqrt() - 1.0),
        ];
        for &(x, y, z, expected) in cases {
            let got = kernel
                .evaluate_sdf_at(h, x, y, z)
                .expect("eval must succeed");
            assert!(
                (got - expected).abs() < 1e-5,
                "sphere SDF({x},{y},{z}): expected {expected}, got {got}",
            );
        }
    }

    /// Box SDF must match the analytical formula on canonical axis points
    /// for the unit cube (full extents 2×2×2, half-extents 1).
    #[test]
    fn fidget_kernel_evaluate_sdf_at_box_matches_analytical() {
        let mut kernel = FidgetKernel::new();
        let cube = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(2.0),
                height: Value::Real(2.0),
                depth: Value::Real(2.0),
            })
            .expect("Box build");
        let h = cube.id;

        let cases: &[(f32, f32, f32, f32)] = &[
            // origin: deepest interior point — distance to each face = 1
            (0.0, 0.0, 0.0, -1.0),
            // on +X face
            (1.0, 0.0, 0.0, 0.0),
            // 1 unit beyond +X face
            (2.0, 0.0, 0.0, 1.0),
            // on +Y face
            (0.0, 1.0, 0.0, 0.0),
            // on +Z face
            (0.0, 0.0, 1.0, 0.0),
        ];
        for &(x, y, z, expected) in cases {
            let got = kernel
                .evaluate_sdf_at(h, x, y, z)
                .expect("eval must succeed");
            assert!(
                (got - expected).abs() < 1e-5,
                "box SDF({x},{y},{z}): expected {expected}, got {got}",
            );
        }
    }

    /// `query`/`export`/`tessellate` must each emit op-specific error
    /// messages that name the kernel (`Fidget`), the repr family (`Sdf`),
    /// and reference the architecture pointer for SDF→Mesh deferral
    /// (`§10.8`) so the diagnostic explains the deferral rather than
    /// looking like a generic catch-all.
    #[test]
    fn fidget_kernel_query_export_tessellate_each_emit_op_specific_message() {
        use reify_ir::GeometryQuery;

        let kernel = FidgetKernel::new();

        // (a) query
        let err = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .expect_err("query must error on Sdf");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(msg.contains("Volume"), "{msg:?}");
                assert!(msg.contains("Fidget"), "{msg:?}");
                assert!(msg.contains("Sdf"), "{msg:?}");
                assert!(msg.contains("§10.8"), "{msg:?}");
            }
            other => panic!("expected QueryFailed, got {other:?}"),
        }

        // (b) export
        let mut sink: Vec<u8> = Vec::new();
        let err = kernel
            .export(GeometryHandleId(1), ExportFormat::Step, &mut sink)
            .expect_err("export must error on Sdf");
        match err {
            ExportError::FormatError(msg) => {
                assert!(msg.contains("Step"), "{msg:?}");
                assert!(msg.contains("Fidget"), "{msg:?}");
                assert!(msg.contains("Sdf"), "{msg:?}");
            }
            other => panic!("expected FormatError, got {other:?}"),
        }

        // (c) tessellate
        let err = kernel
            .tessellate(GeometryHandleId(1), 0.1)
            .expect_err("tessellate must error on Sdf");
        match err {
            TessError::TessellationFailed(msg) => {
                assert!(msg.contains("Fidget"), "{msg:?}");
                assert!(msg.contains("§10.8"), "{msg:?}");
            }
            other => panic!("expected TessellationFailed, got {other:?}"),
        }
    }

    /// `evaluate_sdf_at` on an unknown handle must surface
    /// `QueryError::QueryFailed` (not `QueryError::InvalidHandle` — the
    /// trait's existing query path uses `QueryFailed` for invalid lookups,
    /// staying within the established error vocabulary). The test name
    /// mirrors the actual error variant so a future reader grepping for
    /// `QueryFailed` finds this pin directly.
    #[test]
    fn fidget_kernel_evaluate_sdf_at_unknown_handle_returns_query_failed() {
        let kernel = FidgetKernel::new();
        let err = kernel
            .evaluate_sdf_at(GeometryHandleId(999), 0.0, 0.0, 0.0)
            .expect_err("unknown handle must error");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(msg.contains("invalid"), "{msg:?}");
                assert!(msg.contains("999"), "{msg:?}");
            }
            other => panic!("expected QueryFailed, got {other:?}"),
        }
    }

    /// `execute(Sphere)` must reject every non-positive or non-finite radius
    /// with `GeometryError::OperationFailed` whose message contains both
    /// `"sphere radius"` and `"finite positive"`.  The integer-coercion path
    /// (`Value::Int(-1)`) is included to confirm that the check runs on the
    /// `f64` produced by `extract_f64`, not on the raw `Value` tag.
    ///
    /// A valid `Value::Real(1.0)` sanity case at the end is a regression
    /// guard that the helper does not over-reject.
    #[test]
    fn fidget_kernel_execute_sphere_rejects_invalid_radius() {
        let bad_radii: &[Value] = &[
            Value::Real(-1.0),
            Value::Real(0.0),
            Value::Real(f64::NAN),
            Value::Real(f64::INFINITY),
            Value::Real(f64::NEG_INFINITY),
            Value::Int(-1),
        ];

        for radius in bad_radii {
            let mut kernel = FidgetKernel::new();
            let result = kernel.execute(&GeometryOp::Sphere {
                radius: radius.clone(),
            });
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert_eq!(
                        msg.as_str(),
                        SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
                        "sphere-radius rejection message must be byte-identical to the shared const; radius={radius:?}, got {msg:?}",
                    );
                }
                Ok(handle) => panic!(
                    "execute(Sphere) with radius={radius:?} must fail, but returned Ok({handle:?})",
                ),
                Err(other) => panic!(
                    "execute(Sphere) with radius={radius:?} must return \
                     OperationFailed, got {other:?}",
                ),
            }
        }

        // Sanity: a valid radius must still succeed (regression guard).
        let mut kernel = FidgetKernel::new();
        kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(1.0),
            })
            .expect("execute(Sphere) with radius=1.0 must succeed");
    }

    /// `execute(Box)` must reject every triple that contains a non-positive or
    /// non-finite dimension with `GeometryError::OperationFailed` whose
    /// message contains both `"box dimensions"` and `"finite positive"`.
    ///
    /// Each bad-axis case exercises a single invalid dimension while keeping
    /// the other two valid, plus one all-bad case. A sanity triple
    /// `(2.0, 2.0, 2.0)` at the end confirms the helper does not
    /// over-reject.
    #[test]
    fn fidget_kernel_execute_box_rejects_invalid_dimensions() {
        // (width, height, depth) triples — each has at least one bad axis.
        let bad_triples: &[(Value, Value, Value)] = &[
            // negative width
            (Value::Real(-1.0), Value::Real(1.0), Value::Real(1.0)),
            // zero height
            (Value::Real(1.0), Value::Real(0.0), Value::Real(1.0)),
            // NaN depth
            (Value::Real(1.0), Value::Real(1.0), Value::Real(f64::NAN)),
            // +Inf width
            (
                Value::Real(f64::INFINITY),
                Value::Real(1.0),
                Value::Real(1.0),
            ),
            // -Inf height
            (
                Value::Real(1.0),
                Value::Real(f64::NEG_INFINITY),
                Value::Real(1.0),
            ),
            // all bad
            (Value::Real(-1.0), Value::Real(-2.0), Value::Real(-3.0)),
        ];

        for (width, height, depth) in bad_triples {
            let mut kernel = FidgetKernel::new();
            let result = kernel.execute(&GeometryOp::Box {
                width: width.clone(),
                height: height.clone(),
                depth: depth.clone(),
            });
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert_eq!(
                        msg.as_str(),
                        BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE,
                        "box-dimensions rejection message must be byte-identical to the shared const; triple=({width:?},{height:?},{depth:?}), got {msg:?}",
                    );
                }
                Ok(handle) => panic!(
                    "execute(Box) with ({width:?},{height:?},{depth:?}) must fail, but returned Ok({handle:?})",
                ),
                Err(other) => panic!(
                    "execute(Box) with ({width:?},{height:?},{depth:?}) must return \
                     OperationFailed, got {other:?}",
                ),
            }
        }

        // Sanity: valid dimensions must still succeed (regression guard).
        let mut kernel = FidgetKernel::new();
        kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(2.0),
                height: Value::Real(2.0),
                depth: Value::Real(2.0),
            })
            .expect("execute(Box) with valid dimensions must succeed");
    }

    /// `iso_mesh` on a sphere SDF must produce a non-empty mesh with valid
    /// geometry: at least one vertex and one triangle, with vertex count and
    /// index count both divisible by 3.
    #[test]
    fn fidget_kernel_iso_mesh_sphere_produces_nonempty_mesh() {
        use crate::IsoMeshOptions;
        let mut kernel = FidgetKernel::new();
        let sphere = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(1.0),
            })
            .expect("Sphere build");
        let result = kernel.iso_mesh(sphere.id, &IsoMeshOptions::default());
        let mesh = result.expect("iso_mesh on sphere must succeed");
        assert!(
            mesh.vertices.len() > 0,
            "iso_mesh must produce at least one vertex; got {} vertices",
            mesh.vertices.len(),
        );
        assert_eq!(
            mesh.vertices.len() % 3,
            0,
            "vertex count must be divisible by 3 (flat xyz layout); got {}",
            mesh.vertices.len(),
        );
        assert!(
            mesh.indices.len() > 0,
            "iso_mesh must produce at least one index; got {} indices",
            mesh.indices.len(),
        );
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "index count must be divisible by 3 (triangle list); got {}",
            mesh.indices.len(),
        );
    }

    /// `iso_mesh` on an unknown handle must return `Err(TessError::InvalidHandle(_))`.
    #[test]
    fn fidget_kernel_iso_mesh_unknown_handle_errors() {
        use crate::IsoMeshOptions;
        let kernel = FidgetKernel::new();
        let result = kernel.iso_mesh(GeometryHandleId(999), &IsoMeshOptions::default());
        match result {
            Err(TessError::InvalidHandle(id)) => {
                assert_eq!(id, GeometryHandleId(999), "must report the invalid handle id");
            }
            Ok(_) => panic!("iso_mesh on unknown handle must fail"),
            Err(other) => panic!("expected InvalidHandle, got {:?}", other),
        }
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
