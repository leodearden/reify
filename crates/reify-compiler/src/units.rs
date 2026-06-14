use reify_core::field_calculus::{DifferentialOp, differential_codomain};
use super::*;

/// The complete set of stdlib geometry constructor names recognised by the
/// compiler. This is the **source of truth** for both [`is_geometry_function`]
/// (derived via `.contains(&name)`) and the dispatch coverage test in
/// `crates/reify-compiler/tests/geometry_traits_inference_tests.rs`.
///
/// # Maintenance contract
///
/// When adding a new geometry function name here, you **must** also add an
/// explicit arm for it in `infer_traits_for_function_call` (or its `try_*`
/// companion) in `crates/reify-compiler/src/geometry_traits_inference.rs`.
/// The test `every_geometry_function_name_has_explicit_dispatch_arm` will
/// fail loudly if a name is added here without a matching dispatch arm —
/// turning the previously-silent `_ => InferredTraits::all()` fallback into
/// a compile-time-traceable assertion failure.
///
/// Order matches the original `matches!` in the pre-refactor `is_geometry_function`
/// for diff readability. Case-sensitive: Reify function names are snake_case.
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
    "linear_pattern",
    "linear_pattern_2d",
    "circular_pattern",
    "mirror",
    "arbitrary_pattern",
    "loft",
    "loft_guided",
    "extrude",
    "revolve",
    "revolve_full",
    "shell",
    "shell_open",
    "thicken",
    "offset_solid",
    "draft",
    "chamfer",
    "fillet",
    "fillet_all",
    "union",
    "intersection",
    "difference",
    "union_all",
    "intersection_all",
    "sweep",
    "sweep_guided",
    "extrude_symmetric",
    "translate",
    "rotate",
    "scale",
    "rotate_around",
    "apply_transform",
    "line_segment",
    "arc",
    "helix",
    "interp",
    "bezier",
    "nurbs",
    "tube",
    "torus",
    "pipe",
    "box_centered",
    "cylinder_centered",
    "cone",
    "wedge",
    "rectangle",
    "circle",
    "polygon",
    "ellipse",
    "zone_slab",
    "zone_cylinder",
    "zone_annulus",
    "zone_profile",
];

pub(crate) fn is_geometry_function(name: &str) -> bool {
    GEOMETRY_FUNCTION_NAMES.contains(&name)
}

/// The complete set of stdlib geometry **query-helper** names recognised by
/// the compiler. Sibling to [`GEOMETRY_FUNCTION_NAMES`] — these helpers
/// produce a `Type::Bool` and are dispatched at eval-time by
/// `reify_eval::geometry_ops::try_eval_conformance_query`, which routes to a
/// `GeometryQuery::Is{Watertight,Manifold,Orientable}` against the kernel.
///
/// Kept distinct from `GEOMETRY_FUNCTION_NAMES` because these helpers do not
/// lower to a `CompiledGeometryOp` and must be classified as
/// non-geometry-producing by `is_geometry_let`. Case-sensitive: Reify
/// function names are snake_case.
pub const GEOMETRY_QUERY_HELPER_NAMES: &[&str] = &["is_watertight", "is_manifold", "is_orientable"];

pub(crate) fn is_geometry_query_helper(name: &str) -> bool {
    GEOMETRY_QUERY_HELPER_NAMES.contains(&name)
}

/// The complete set of stdlib **kinematic-query** helper names recognised by
/// the compiler. Sibling to [`GEOMETRY_QUERY_HELPER_NAMES`] — these helpers
/// consume a Snapshot Map and dispatch at eval-time via
/// `reify_eval::geometry_ops::try_eval_kinematic_query`, which queries the
/// geometry kernel using `GeometryQuery::Distance` per body pair.
///
/// Unlike the conformance-query helpers (which all return `Type::Bool`), the
/// kinematic helpers have three distinct result types — see
/// [`kinematic_query_result_type`]. They share this list only for
/// classification (recognised vs. fallback) — per-name type dispatch lives
/// alongside the call site in `expr.rs`.
///
/// Case-sensitive: Reify function names are snake_case.
pub const GEOMETRY_KINEMATIC_QUERY_NAMES: &[&str] =
    &["interferes", "interferes_with", "min_clearance"];

pub(crate) fn is_geometry_kinematic_query(name: &str) -> bool {
    GEOMETRY_KINEMATIC_QUERY_NAMES.contains(&name)
}

/// Result type per kinematic-query helper. Matches the `Value` shape produced
/// by `reify_eval::geometry_ops::try_eval_kinematic_query`:
///
/// - `interferes(snapshot)`        → `Type::List(Type::Map(...))`
/// - `interferes_with(s, a, b)`    → `Type::Bool`
/// - `min_clearance(s, a, b)`      → `Type::length()`
///
/// Returns `None` for any other name (caller falls through to its default
/// type-inference path).
pub(crate) fn kinematic_query_result_type(name: &str) -> Option<reify_core::Type> {
    use reify_core::Type;
    Some(match name {
        // List of pair Maps `{ "a": Int, "b": Int }`. We deliberately type as
        // List of generic Map (Type::String → Type::Int) rather than a
        // dedicated record type because Reify's surface Type lacks structural
        // record syntax; the per-key contract is documented in the
        // `try_eval_kinematic_query` Some(List(...)) arm.
        "interferes" => Type::List(Box::new(Type::Map(
            Box::new(Type::String),
            Box::new(Type::Int),
        ))),
        "interferes_with" => Type::Bool,
        "min_clearance" => Type::length(),
        _ => return None,
    })
}

/// The complete set of stdlib **topology-selector** helper names recognised by
/// the compiler. Sibling to [`GEOMETRY_KINEMATIC_QUERY_NAMES`] — these helpers
/// produce a per-name typed result and dispatch at eval-time via
/// `reify_eval::geometry_ops::try_eval_topology_selector`.
///
/// Per `docs/prds/topology-selectors.md` §3.9 these are the v0.1 names
/// (3 wired by task 2324 + 11 wired by task 2699):
///
/// ```text
/// // Task 2324 — eval dispatch fully implemented
/// fn closest_point<G: Geometry>(point: Point3<Length>, geometry: G) -> Point3<Length>
/// fn is_on<G: Geometry>(point: Point3<Length>, geometry: G) -> Bool
/// fn angle_between_surfaces(a: Surface, b: Surface) -> Angle
///
/// // Task 2699 — compile-time type wiring only; eval dispatch is task 2691
/// fn edges(solid: Solid) -> List<Geometry>
/// fn faces(solid: Solid) -> List<Geometry>
/// fn edges_by_length(solid: Solid, range: Range<Length>) -> List<Geometry>
/// fn faces_by_area(solid: Solid, range: Range<Area>) -> List<Geometry>
/// fn faces_by_normal(solid: Solid, dir: Vec3, tol: Angle) -> List<Geometry>
/// fn edges_parallel_to(solid: Solid, dir: Vec3, tol: Angle) -> List<Geometry>
/// fn edges_at_height(solid: Solid, h: Length, tol: Length) -> List<Geometry>
/// fn adjacent_faces(solid: Solid, face: Geometry) -> List<Geometry>
/// fn shared_edges(face1: Geometry, face2: Geometry) -> List<Geometry>
/// fn center_of_mass(solid: Solid, density: Density) -> Point3<Length>
/// fn moment_of_inertia(solid: Solid, density: Density) -> Tensor<2, 3, MomentOfInertia>
/// ```
///
/// Like the kinematic-query helpers, these names share this list only for
/// classification — per-name type dispatch lives in
/// [`topology_selector_result_type`] and the eval-side post-process
/// [`reify_eval::engine_build::post_process_topology_selectors`].
///
/// For the 11 task-2699 names, eval-side dispatch in
/// `reify_eval::geometry_ops::try_eval_topology_selector` falls through to
/// the `_ => return None` arm, leaving cells at `Value::Undef`.
/// `value_type_kind_matches` accepts `Value::Undef` for any type
/// (`reify_eval::lib:196`), so the cell typechecks at compile-time and stays
/// `Undef` at runtime until task 2691 wires the dispatch arms.
///
/// Case-sensitive: Reify function names are snake_case.
///
/// ## Compile-time arg-type enforcement (task 4493 ζ)
///
/// The dimensioned-scalar argument slots for members of this family are
/// statically enforced by [`crate::builtin_signatures::check_builtin_arg_types`]:
/// `center_of_mass`/`moment_of_inertia` arg 1 (`density: Density` →
/// `MASS_DENSITY`); `faces_by_normal`/`edges_parallel_to` arg 2 (`tol: Angle`
/// → `ANGLE`); `edges_at_height` args 1 and 2 (`h`/`tol: Length` → `LENGTH`).
/// A [`reify_core::DiagnosticCode::ArgTypeMismatch`] error is emitted for any
/// definite static dimension mismatch at call-site compile time.
pub const GEOMETRY_TOPOLOGY_SELECTOR_NAMES: &[&str] = &[
    // Task 2324 — eval dispatch fully implemented
    "closest_point",
    "is_on",
    "angle_between_surfaces",
    // Task 2699 — compile-time type wiring; eval dispatch is task 2691
    "edges",
    "faces",
    "edges_by_length",
    "faces_by_area",
    "faces_by_normal",
    "edges_parallel_to",
    "edges_at_height",
    "adjacent_faces",
    "shared_edges",
    "center_of_mass",
    "moment_of_inertia",
    // Task 4190 — split(solid, plane) -> List<Solid> via BRepAlgoAPI_Splitter.
    // Joins the topology-selector family (not GEOMETRY_FUNCTION_NAMES) because
    // it returns a multi-output List<Geometry>, matching the topology-selector
    // eval path (try_eval_topology_selector / execute_split).
    "split",
    // Task 4119 δ — Named-leaf constructors for the three SelectorKind variants.
    // `face(geometry, name) -> Selector(Face)`, `edge(geometry, name) ->
    // Selector(Edge)`, `solid_body(geometry, name) -> Selector(Body)`.
    // These join the topology-selector family (not GEOMETRY_FUNCTION_NAMES) so
    // they route through the value-typing path (selector_composition_result_type
    // / topology_selector_result_type) and are excluded from CSG geometry-let
    // routing by `is_selector_expr` in geometry.rs.
    // NOTE: `body` is intentionally absent — it is the RBD mechanism constructor
    // in JOINT_TYPED_FN_NAMES (→ StructureRef("Mechanism")).  `solid_body` is
    // the PRD §11.1 alternative name, verified free across all family lists.
    "face",
    "edge",
    "solid_body",
];

pub(crate) fn is_geometry_topology_selector(name: &str) -> bool {
    GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name)
}

/// Result type per topology-selector helper. Sets the cell's `result_type`
/// so the post-processor does not fall back to the first-arg type (which
/// would be `Type::Geometry` — rejected by `is_representable_cell_type`).
///
/// Task 2324 names — `Value` shape matches eval dispatch:
/// - `closest_point(point, geometry)`        → `Type::point3(Type::length())`
/// - `is_on(point, geometry)`                → `Type::Bool`
/// - `angle_between_surfaces(a, b)`          → `Type::angle()`
///
/// Task 2699 names — compile-time type only; eval dispatch is task 2691.
/// Until then, cells hold `Value::Undef`, which `value_type_kind_matches`
/// accepts for any type (`reify_eval::lib:196`):
/// Task 4118 (γ) — the 7 predicate/all selector constructors evaluate to a
/// typed `Value::Selector(kind)`, so their compile-time result type is
/// `Type::Selector(kind)`. The compiler inserts a `ResolveSelector` coercion
/// node to bridge `Selector → List<Geometry>` at the three consumption sites
/// (param-binding, single()/list-helper, IndexAccess-object):
/// - `edges(solid)`                          → `Type::Selector(Edge)`
/// - `faces(solid)`                          → `Type::Selector(Face)`
/// - `edges_by_length(solid, range)`         → `Type::Selector(Edge)`
/// - `faces_by_area(solid, range)`           → `Type::Selector(Face)`
/// - `faces_by_normal(solid, dir, tol)`      → `Type::Selector(Face)`
/// - `edges_parallel_to(solid, dir, tol)`    → `Type::Selector(Edge)`
/// - `edges_at_height(solid, h, tol)`        → `Type::Selector(Edge)`
/// - `adjacent_faces(solid, face)`           → `Type::List(Geometry)` (relational)
/// - `shared_edges(face1, face2)`            → `Type::List(Geometry)` (relational)
/// - `center_of_mass(solid, density)`        → `Type::point3(Type::length())`
/// - `moment_of_inertia(solid, density)`     → `Type::tensor(2, 3, MomentOfInertia)`
///
/// Returns `None` for any other name (caller falls through to its default
/// type-inference path).
pub(crate) fn topology_selector_result_type(name: &str) -> Option<reify_core::Type> {
    use reify_core::Type;
    Some(match name {
        // Task 2324 — eval dispatch fully implemented
        "closest_point" => Type::point3(Type::length()),
        "is_on" => Type::Bool,
        "angle_between_surfaces" => Type::angle(),
        // Task 4118 (γ) — the 7 predicate/all selector constructors are typed
        // `Type::Selector(kind)` (Edge / Face per the constructor). The compiler
        // bridges `Selector → List<Geometry>` via a `ResolveSelector` coercion
        // node at the three consumption sites.
        "edges" => Type::Selector(reify_core::ty::SelectorKind::Edge),
        "faces" => Type::Selector(reify_core::ty::SelectorKind::Face),
        "edges_by_length" => Type::Selector(reify_core::ty::SelectorKind::Edge),
        "faces_by_area" => Type::Selector(reify_core::ty::SelectorKind::Face),
        "faces_by_normal" => Type::Selector(reify_core::ty::SelectorKind::Face),
        "edges_parallel_to" => Type::Selector(reify_core::ty::SelectorKind::Edge),
        "edges_at_height" => Type::Selector(reify_core::ty::SelectorKind::Edge),
        // Task 2699 — relational selectors stay List<Geometry>: adjacent_faces /
        // shared_edges have no `LeafQuery` representation (4117's LeafQuery =
        // {Named,All,ByNormal,ByArea,ByLength,ByHeight,ByParallel}), so they are
        // out of scope for the Selector re-type.
        "adjacent_faces" | "shared_edges" => Type::List(Box::new(Type::Geometry)),
        // Task 4190 — split(solid, plane) -> List<Solid>. Same List<Geometry>
        // result type as the edge/face selectors; eval dispatch via
        // TopologySelectorHelper::Split in try_eval_topology_selector.
        "split" => Type::List(Box::new(Type::Geometry)),
        // Task 4119 δ — Named-leaf constructors (PRD §11.1).
        // `face(geometry, name)` / `edge(geometry, name)` / `solid_body(geometry, name)`
        // each return the per-kind Selector type.  `body` is NOT listed here —
        // it remains the RBD mechanism constructor (StructureRef("Mechanism")).
        "face" => Type::Selector(reify_core::ty::SelectorKind::Face),
        "edge" => Type::Selector(reify_core::ty::SelectorKind::Edge),
        "solid_body" => Type::Selector(reify_core::ty::SelectorKind::Body),
        "center_of_mass" => Type::point3(Type::length()),
        "moment_of_inertia" => Type::tensor(
            2,
            3,
            Type::Scalar {
                dimension: reify_core::DimensionVector::MOMENT_OF_INERTIA,
            },
        ),
        _ => return None,
    })
}

/// Classify `union`/`intersect`/`difference` calls whose operands are
/// `Type::Selector(kind)` — the selector-composition algebra (task 4119 δ).
///
/// Enforces the **K1 kind-closure invariant** at compile time: all operands must
/// carry the SAME `reify_core::ty::SelectorKind`.  On mismatch emits exactly one
/// [`DiagnosticCode::SelectorKindMismatch`] (`E_SELECTOR_KIND_MISMATCH`) diagnostic
/// naming both kinds (using `SelectorKind`'s `Display` impl, e.g. `"FaceSelector"`
/// and `"EdgeSelector"`); returns `Some(Type::Selector(first_kind))` as the
/// anti-cascade result so downstream type-checks receive a valid selector type and
/// do not cascade.
///
/// Returns `None` for:
/// - any name other than `union`, `intersect`, or `difference` (caller falls through)
/// - calls whose selector-type operands are ALL non-`Selector` types (the CSG
///   `union(box, box)` / `difference(box, box)` cases — caller falls through to
///   `is_geometry_function`)
///
/// **Arity** — `difference` is strictly binary (exactly 2 operands); passing the
/// wrong arity emits an `E_SELECTOR_KIND_MISMATCH` error at compile time.
/// `union` and `intersect` are variadic (≥ 2 operands); the ≥-2 floor is enforced
/// at eval time by the `args.len() < 2` gate in `try_eval_topology_selector` (the
/// compile-time path cannot see a sub-2-arity call that passes kind-checking, so
/// no compile-time guard is needed for them).
///
/// Inserted as a ladder arm **before** `is_geometry_function` in
/// `crates/reify-compiler/src/expr.rs` so that selector compositions are given their
/// correct `Type::Selector(k)` result type before the function-name-based fallback
/// assigns `Type::dimensionless_scalar()`.
pub(crate) fn selector_composition_result_type(
    name: &str,
    compiled_args: &[CompiledExpr],
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    if !matches!(name, "union" | "intersect" | "difference") {
        return None;
    }

    // Collect the `reify_core::ty::SelectorKind` from every selector-typed arg.
    // If no arg is a Selector type, this is a CSG call — return None so the caller
    // falls through to `is_geometry_function`.
    let selector_kinds: Vec<reify_core::ty::SelectorKind> = compiled_args
        .iter()
        .filter_map(|arg| match &arg.result_type {
            Type::Selector(k) => Some(*k),
            _ => None,
        })
        .collect();

    if selector_kinds.is_empty() {
        return None;
    }

    let first_kind = selector_kinds[0];

    // Arity gate for `difference`: it is strictly binary (exactly 2 operands).
    // Emitting here (before the kind check) ensures the user sees an actionable
    // error rather than a silent Undef at eval when the arity gate in
    // `try_eval_topology_selector` returns None.  `union`/`intersect` are variadic
    // (≥ 2) — their sub-2 floor is eval-gated and documented in the rustdoc above.
    if name == "difference" && compiled_args.len() != 2 {
        let n = compiled_args.len();
        diagnostics.push(
            Diagnostic::error(format!(
                "selector `difference` requires exactly 2 operands, got {n}"
            ))
            .with_code(DiagnosticCode::SelectorKindMismatch)
            .with_label(DiagnosticLabel::new(
                call_span,
                format!("expected 2 operands, got {n}"),
            )),
        );
        return Some(Type::Selector(first_kind)); // anti-cascade
    }

    // Reject non-Selector operands mixed with Selector operands.  A call like
    // `union(faces(b), box(…))` routes here (because faces(b) is_selector_expr),
    // but `box(…)` has a non-Selector type — that is always a user error.  Without
    // this check the function would return Some(Selector(Face)) from the K1 path
    // below, compile silently, and only fail at eval when reconstruct_selector_value
    // returns None and the cell is left at Undef.  Emit E_SELECTOR_KIND_MISMATCH at
    // compile time instead.
    let non_selector_count = compiled_args
        .iter()
        .filter(|arg| !matches!(&arg.result_type, Type::Selector(_)))
        .count();
    if non_selector_count > 0 {
        diagnostics.push(
            Diagnostic::error(format!(
                "selector composition requires all operands to be selectors; \
                 {non_selector_count} non-selector operand(s) found",
            ))
            .with_code(DiagnosticCode::SelectorKindMismatch)
            .with_label(DiagnosticLabel::new(
                call_span,
                "non-selector operand in selector composition",
            )),
        );
        return Some(Type::Selector(first_kind)); // anti-cascade
    }

    // Check K1: all selector operands must share the same kind.
    if let Some(&mismatch_kind) = selector_kinds.iter().find(|&&k| k != first_kind) {
        // Emit exactly ONE E_SELECTOR_KIND_MISMATCH naming both kinds.
        diagnostics.push(
            Diagnostic::error(format!(
                "selector composition kind mismatch: cannot compose {} and {}",
                first_kind, mismatch_kind,
            ))
            .with_code(DiagnosticCode::SelectorKindMismatch)
            .with_label(DiagnosticLabel::new(
                call_span,
                "mixed-kind selector composition",
            )),
        );
        // Anti-cascade: return first_kind's type so downstream checks receive a valid
        // (if wrong-kind) selector type and do not cascade.
        return Some(Type::Selector(first_kind));
    }

    // All selector operands have the same kind — valid K1 composition.
    Some(Type::Selector(first_kind))
}

/// The complete set of AffineMap **constructor** free-function names recognised
/// by the compiler (PRD §4.2, task β). A sibling classifier list to the geometry
/// families above. Unlike [`GEOMETRY_FUNCTION_NAMES`] (geometry-handle producers
/// carrying the geometry-traits-inference dispatch contract), these constructors
/// produce a first-class `Value::AffineMap` value, so they live in their own
/// list and lower through the ordinary builtin path
/// (`reify_stdlib::geometry::eval_geometry`), NOT the geometry-op path.
///
/// Every name resolves to the SAME result type — `Type::AffineMap(3)` — via
/// [`affine_map_constructor_result_type`]; the list exists for call-site
/// classification in `expr.rs` (the `is_affine_map_constructor` arm, before the
/// first-arg fallback). Registering them here replaces the wrong first-arg
/// fallback type (e.g. `affine_scale(...)` → Real) and silences the zero-arg
/// "cannot infer return type" warning for `affine_identity()`.
///
/// Case-sensitive: Reify function names are snake_case.
pub const AFFINE_MAP_CONSTRUCTOR_NAMES: &[&str] = &[
    "affine_scale",
    "affine_shear_xy",
    "affine_shear_xz",
    "affine_shear_yx",
    "affine_shear_yz",
    "affine_shear_zx",
    "affine_shear_zy",
    "affine_translate",
    "affine_identity",
    "affine_map",
    "affine_from_transform",
];

pub(crate) fn is_affine_map_constructor(name: &str) -> bool {
    AFFINE_MAP_CONSTRUCTOR_NAMES.contains(&name)
}

/// Result type for every AffineMap constructor: `Type::AffineMap(3)`.
///
/// All 11 constructors share one result type, so this is a single membership
/// check rather than a per-name match. Returns `None` for any name not in
/// [`AFFINE_MAP_CONSTRUCTOR_NAMES`] (caller falls through to its default
/// type-inference path).
pub(crate) fn affine_map_constructor_result_type(name: &str) -> Option<reify_core::Type> {
    if is_affine_map_constructor(name) {
        Some(reify_core::Type::AffineMap(3))
    } else {
        None
    }
}

/// Tolerancing **marker** builtins (η/4480, PRD
/// docs/prds/v0_6/gdt-geometric-zones-and-containment.md task η, contract C3).
///
/// `nominal()` is a zero-arg builtin returning an inert `Geometry`-typed marker
/// — an INVALID-handle `Value::GeometryHandle`, evaluated in
/// `reify_stdlib::tolerancing::eval_tolerancing`. It is the default for
/// `Conforms.actual` (`param actual : Geometry = nominal()`): param defaults
/// compile in a *neutral scope* (functions.rs:106-130), so a
/// `= tolerance.feature` default cannot evaluate — an inert marker is the only
/// way to keep the param `Geometry`-typed while the constraint body ignores it.
///
/// Single-name family, structurally parallel to the sibling builtin-name
/// families above ([`AFFINE_MAP_CONSTRUCTOR_NAMES`], etc.). It exists for
/// call-site classification in `expr.rs` (the `is_tolerancing_marker` arm in
/// the `NoUserFunctions` ladder, before the zero-arg fallback). Registering it
/// here replaces the wrong fallback — `nominal()` would otherwise reach the
/// zero-arg fallback, typed `Real` with a "cannot infer return type of zero-arg
/// function" warning.
///
/// The marker flows nowhere: the Conforms predicate never reads `actual`, and
/// the η `measure_gdt_conformance` pass keys on an *explicit* `actual` binding,
/// never this default — so the INVALID-handle sentinel never reaches a kernel.
///
/// Case-sensitive: Reify function names are snake_case.
pub const TOLERANCING_MARKER_NAMES: &[&str] = &["nominal"];

pub(crate) fn is_tolerancing_marker(name: &str) -> bool {
    TOLERANCING_MARKER_NAMES.contains(&name)
}

/// Result type for tolerancing marker builtins: `Type::Geometry`.
///
/// Returns `None` for any name not in [`TOLERANCING_MARKER_NAMES`] (caller
/// falls through to its default type-inference path). Mirrors
/// [`affine_map_constructor_result_type`] — uniform result type, single
/// membership check.
pub(crate) fn tolerancing_marker_result_type(name: &str) -> Option<reify_core::Type> {
    if is_tolerancing_marker(name) {
        Some(reify_core::Type::Geometry)
    } else {
        None
    }
}

/// Return-type inference for the AffineMap **algebra** free-functions (task γ,
/// PRD §4.3).  Separate from [`affine_map_constructor_result_type`] because
/// (a) `affine_inverse` returns `Option<AffineMap(3)>` and `determinant`
/// returns `Real` — three different result types across three names — and
/// (b) an existing test pins `!is_affine_map_constructor("affine_compose")`.
///
/// `first_arg_type` is used to disambiguate `determinant`: when the first arg
/// is an `AffineMap(3)` the result is `Real`; when it is something else
/// (e.g. a Matrix) we return `None` and let the caller fall through to the
/// existing first-arg fallback, preserving the matrix-determinant behaviour.
///
/// Returns `None` for any name / arg-type combination not in scope here.
pub(crate) fn affine_map_algebra_result_type(
    name: &str,
    first_arg_type: Option<&reify_core::Type>,
) -> Option<reify_core::Type> {
    match name {
        "affine_compose" => Some(reify_core::Type::AffineMap(3)),
        "affine_inverse" => {
            Some(reify_core::Type::Option(Box::new(reify_core::Type::AffineMap(3))))
        }
        "determinant" => {
            // Only override when the first arg is an AffineMap; otherwise fall
            // through to the existing matrix-determinant first-arg fallback.
            if matches!(first_arg_type, Some(reify_core::Type::AffineMap(_))) {
                Some(reify_core::Type::dimensionless_scalar())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The complete set of stdlib **geometry-query** helper names recognised by
/// the compiler. Fifth geometry-name family, structurally parallel to the
/// existing four ([`GEOMETRY_FUNCTION_NAMES`],
/// [`GEOMETRY_QUERY_HELPER_NAMES`], [`GEOMETRY_KINEMATIC_QUERY_NAMES`],
/// [`GEOMETRY_TOPOLOGY_SELECTOR_NAMES`]).
///
/// Per the PRD §1 frozen list (docs/prds/v0_3/geometry-handle-runtime.md):
///
/// ```text
/// fn volume(g: Solid)                   -> Scalar<Volume>
/// fn area(g: Surface)                   -> Scalar<Area>
/// fn length(g: Curve)                   -> Scalar<Length>
/// fn perimeter(g: Surface)              -> Scalar<Length>
/// fn centroid(g: Geometry)              -> Point3<Length>
/// fn bounding_box(g: Geometry)          -> BoundingBox
/// fn distance(a: Geometry, b: Geometry) -> Scalar<Length>
/// fn contains(a: Geometry, b: Geometry) -> Bool
/// fn intersects(a: Geometry, b: Geometry) -> Bool
/// fn geo_equiv(a: Geometry, b: Geometry) -> Bool
/// fn angle(a: Vec3, b: Vec3)            -> Scalar<Angle>
/// fn curvature(c: Curve, t: Real)       -> Scalar<Curvature>
/// ```
///
/// **Disjointness contract**: this list MUST remain disjoint from the four
/// other families. Cell classification could double-fire if a name appeared
/// in two families (the geometry-query arm in `expr.rs::infer_type` is
/// dispatched AFTER the topology-selector / kinematic-query / conformance-
/// query arms — a name living in both would silently win at the earlier
/// arm). Pinned by both directions: the
/// `is_geometry_query_rejects_other_family_names` test (other → not in
/// geometry-query) AND the `geometry_query_names_are_disjoint_from_other_families`
/// test (every entry of `GEOMETRY_QUERY_NAMES` is absent from the four
/// sibling slices).
///
/// **Maintenance contract**: adding a name here REQUIRES a parallel entry
/// in [`geometry_query_result_type`]. Mirrors the documented contract on
/// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`. Pinned by the
/// `geometry_query_names_each_have_a_result_type` test, which iterates this
/// slice directly (not a hand-maintained fixture vec) so a new entry
/// without a parallel result-type arm fails at test time rather than
/// `.expect()`-panicking in production (`expr.rs::infer_type` calls
/// `geometry_query_result_type(name).expect("is_geometry_query implies …")`).
///
/// **Phase 1 trade**: compile-time return-type wiring only. Eval-time
/// dispatch arrives in Phase 6 (GHR-ζ). Until then, cells hold
/// `Value::Undef`, which `value_type_kind_matches` accepts for any type
/// (`reify_eval::lib:196`), so the cell typechecks at compile-time and
/// stays `Undef` at runtime.
///
/// **`curvature` overload note**: the default `Curve`→`Scalar<Curvature>`
/// overload is registered here.  The `Surface`→`Matrix<2,2,Curvature>`
/// overload is handled by the arg-aware structural dispatcher
/// [`geometry_query_arg_aware_result_type`] (task 4315): when the first
/// compiled arg is an inline `faces(...)[i]` IndexAccess, that fn returns
/// `Some(Matrix{2,2,Curvature})` before this table is consulted.  A
/// curvature call whose surface arg arrives via a let-binding or parameter
/// still falls through to this `Scalar<Curvature>` default.
///
/// Call-site dispatch is in `expr.rs::infer_type` (the `else if
/// is_geometry_query(name)` arm, immediately after the topology-selector
/// arm).
///
/// Case-sensitive: Reify function names are snake_case.
pub const GEOMETRY_QUERY_NAMES: &[&str] = &[
    "volume",
    "area",
    "length",
    "perimeter",
    "centroid",
    "bounding_box",
    "distance",
    "contains",
    "intersects",
    "geo_equiv",
    "angle",
    "curvature",
    // KGQ-ζ (task 3615, Phase 6): at-point surface normal.
    // normal(surface: Surface, point: Point3<Length>) -> Vector3<Dimensionless>
    // GHR-α (task 3603) registered the original 12 Phase-1 names above and
    // omitted `normal` (it fell through the gap between the two PRDs). KGQ-ζ
    // absorbs the registration since 3603 is already done.
    "normal",
    // ζ / C4 (task 4479): max deviation between actual and nominal geometries.
    // max_deviation(actual: Geometry, nominal: Geometry) -> Scalar<Length>
    // 2-arg Length-returning query; mirrors `distance`. BRepOnly capability
    // (both operands require OCCT) registered in GeometryQuery::capability_kind().
    "max_deviation",
];

pub(crate) fn is_geometry_query(name: &str) -> bool {
    GEOMETRY_QUERY_NAMES.contains(&name)
}

/// The complete set of stdlib **dynamics-query** helper names recognised by
/// the compiler (RBD-β, task 3829). Sixth name family, structurally parallel
/// to the five geometry families above ([`GEOMETRY_FUNCTION_NAMES`],
/// [`GEOMETRY_QUERY_HELPER_NAMES`], [`GEOMETRY_KINEMATIC_QUERY_NAMES`],
/// [`GEOMETRY_TOPOLOGY_SELECTOR_NAMES`], [`GEOMETRY_QUERY_NAMES`]).
///
/// `body_mass_props(body, density?)` is a name-recognised builtin — NOT a
/// `pub fn` — so that a call site lowers to a `CompiledExprKind::FunctionCall`
/// (which `reify_eval::dynamics_ops::try_eval_body_mass_props` dispatches as a
/// build post-process) rather than a `UserFunctionCall`. This mirrors the
/// geometry-query-helper convention exactly: keeping it a builtin (a) makes the
/// eval-side `FunctionCall` dispatch reachable for every real call site, and
/// (b) avoids the `OverloadResolution::NoMatch` default-padding path, so a
/// 1-arg `body_mass_props(body)` call stays 1-arg and the "no explicit
/// density" rung (and thus the `W_DynamicsDefaultDensity` warning) stays
/// reachable. Declaring it as a `pub fn` with an optional `density` default
/// would route to `UserFunctionCall` AND pad the call to 2 args — defeating
/// both — which is why the steward decision reversed the original `pub fn`
/// plan in favour of this builtin registration.
///
/// **Disjointness contract**: like the geometry families, this list MUST
/// remain disjoint from all five geometry families so a name cannot satisfy
/// two classification predicates. Pinned by
/// `dynamics_query_names_are_disjoint_from_other_families` (and the converse
/// extension to `geometry_query_names_are_disjoint_from_other_families`).
///
/// **Result type**: every entry resolves to `Type::StructureRef("MassProperties")`,
/// set up-front in `expr.rs::infer_type`'s `NoUserFunctions` ladder (the
/// `is_dynamics_query` arm, immediately before the first-arg fallback) so the
/// cell typechecks; eval-time dispatch overwrites the `Value::Undef` left by
/// the pure `eval_expr` path. Because the type is uniform there is no per-name
/// result-type table (mirrors `GEOMETRY_QUERY_HELPER_NAMES → Type::Bool`).
///
/// Case-sensitive: Reify function names are snake_case.
pub const DYNAMICS_QUERY_NAMES: &[&str] = &["body_mass_props"];

pub(crate) fn is_dynamics_query(name: &str) -> bool {
    DYNAMICS_QUERY_NAMES.contains(&name)
}

/// Dynamics-constructor builtins: `point_mass(mass)` and
/// `mass_properties(mass, com, inertia)` (task 4278, v0.3 flexures
/// uniform-mass substrate).
///
/// These are name-recognised eval-builtins dispatched in
/// `reify_stdlib::dynamics::eval_dynamics` — NOT `.ri` declarations
/// (body_mass_props / DYNAMICS_QUERY_NAMES precedent). The
/// `is_dynamics_constructor` arm in `expr.rs::infer_type`'s
/// `NoUserFunctions` ladder sets the result type to
/// `Type::StructureRef("MassProperties")` **up-front**, which is
/// LOAD-BEARING: without it the first-arg fallback would infer
/// `Scalar<Mass>` for `point_mass(2.5kg)`, tripping
/// `value_type_kind_matches` at eval time. Uniform `StructureRef`
/// result type — no per-name table (mirrors DYNAMICS_QUERY_NAMES).
///
/// **Disjointness contract**: every entry must be absent from every
/// other classification family; pinned by
/// `dynamics_constructor_names_are_disjoint_from_other_families` (and
/// the converse asserts added to the sibling disjointness tests).
///
/// Case-sensitive: Reify function names are snake_case.
pub const DYNAMICS_CONSTRUCTOR_NAMES: &[&str] = &["mass_properties", "point_mass"];

pub(crate) fn is_dynamics_constructor(name: &str) -> bool {
    DYNAMICS_CONSTRUCTOR_NAMES.contains(&name)
}

/// The complete set of stdlib **field-op** names recognised by the compiler
/// (std.fields α, task 4219). A dedicated name family, structurally parallel
/// to the sibling classification families above.
///
/// Per PRD docs/prds/v0_6/std-fields-api.md §5.1:
///
/// ```text
/// fn fn_field(f: Function)                    -> Field<D, C>
/// fn from_samples(pts: List<D>, vals: List<C>, method) -> Field<D, C>
/// fn restrict(f: Field<D,C>, region: Geometry) -> Field<D, C>
/// fn compose(f: Field<B,C>, g: Field<A,B>)    -> Field<A, C>
/// fn sample(f: Field<D,C>, p: D)              -> C   (THE FIX: codomain)
/// fn gradient(f: Field<D,C>)                  -> Field<D, …>
/// fn divergence(f: Field<D,C>)                -> Field<D, …>
/// fn curl(f: Field<D,C>)                      -> Field<D, …>
/// fn laplacian(f: Field<D,C>)                 -> Field<D, …>
/// ```
///
/// **Disjointness contract**: this list MUST remain disjoint from all six
/// sibling families. A name living in two families would silently route
/// through whichever arm is dispatched first in `expr.rs`'s
/// `NoUserFunctions` ladder. Pinned by both directions:
/// `is_field_op_recognises_all_field_op_names` (membership) AND
/// `field_op_names_are_disjoint_from_other_families` (forward) plus the
/// `!FIELD_OP_NAMES.contains(name)` asserts in the six sibling
/// disjointness tests (reverse).
///
/// **Maintenance contract**: adding a name here REQUIRES a parallel arm in
/// [`field_op_result_type`].  Pinned by
/// `field_op_names_each_have_a_result_type`, which iterates this slice
/// directly (not a hand-maintained fixture) and asserts every entry has a
/// matching arm.  Without this test, a name added to the slice without a
/// parallel arm would silently return `None` and fall through to the
/// first-arg fallback — exactly the mistyping the family was created to fix.
///
/// **Phase Tier-1 trade**: compile-time return-type wiring only.
/// `fn_field` / `from_samples` / `restrict` / `compose` eval-time dispatch
/// arrives in tasks β/γ/δ; `sample`/`gradient`/`divergence`/`curl`/
/// `laplacian` already dispatch in `reify-expr`.
///
/// Case-sensitive: Reify function names are snake_case.
pub const FIELD_OP_NAMES: &[&str] = &[
    "fn_field",
    "from_samples",
    "restrict",
    "compose",
    "sample",
    "gradient",
    "divergence",
    "curl",
    "laplacian",
];

pub(crate) fn is_field_op(name: &str) -> bool {
    FIELD_OP_NAMES.contains(&name)
}

/// Compile-time return type for a field-op call, per PRD §5.1.
///
/// Returns `Some(Type)` when `name` is a recognised field-op name AND the
/// argument shape matches (i.e. the call is well-typed for this arm).
/// Returns `None` when:
/// - `name` is not a field-op name, OR
/// - the argument shape doesn't match (e.g. `arg_types[0]` is not a `Field`
///   for `sample`/`gradient`/`restrict`/`compose`, or not a `Function` for
///   `fn_field`).
///
/// A `None` result means the caller should fall through to its default
/// first-arg-fallback type-inference, preserving zero regression for existing
/// code that passes mis-shaped arguments.
///
/// # Relationship to eval-time dispatch
///
/// This function sets the **compile-time** `result_type` on the cell; it does
/// NOT perform eval.  At eval time `reify-expr` dispatches the same names.
/// The two layers must agree on the result type for well-typed calls — the
/// table test `field_op_result_type_matches_prd_5_1_table` (+ the gradient
/// variant) pins the contract.
pub(crate) fn field_op_result_type(
    name: &str,
    arg_types: &[reify_core::Type],
) -> Option<reify_core::Type> {
    // Fast rejection: if the name is not in the field-op family, return None
    // immediately.  This also makes `is_field_op` / `FIELD_OP_NAMES` reachable
    // from production code (the `_ => None` wildcard below handles the same
    // case, but the early return here satisfies the dead_code lint).
    if !is_field_op(name) {
        return None;
    }
    use reify_core::Type;

    match name {
        // fn_field(f: Function{params, return_type}) → Field<params[0], return_type>
        "fn_field" => {
            if let Some(Type::Function { params, return_type }) = arg_types.first() {
                let domain = params.first()?.clone();
                Some(Type::Field {
                    domain: Box::new(domain),
                    codomain: return_type.clone(),
                })
            } else {
                None
            }
        }

        // from_samples(List<D>, List<C>, method) → Field<D, C>
        "from_samples" => {
            if arg_types.len() < 2 {
                return None;
            }
            if let (Type::List(d), Type::List(c)) = (&arg_types[0], &arg_types[1]) {
                Some(Type::Field {
                    domain: d.clone(),
                    codomain: c.clone(),
                })
            } else {
                None
            }
        }

        // restrict(Field<D,C>, region) → Field<D,C>  (type unchanged)
        "restrict" => {
            if let Some(Type::Field { domain, codomain }) = arg_types.first() {
                Some(Type::Field {
                    domain: domain.clone(),
                    codomain: codomain.clone(),
                })
            } else {
                None
            }
        }

        // compose(Field<B,C>, Field<A,B>) → Field<A,C>
        //
        // Requires the shared middle type B: arg[0].domain must equal arg[1].codomain.
        // Returns None if they differ — a mis-composed call must not be silently typed
        // (4219-S2 reviewer note).
        //
        // Reachability note (4219-S2): once the user `.ri` compose fn (task 4224 ζ) is
        // in stdlib/fields.ri, real compose(...) calls resolve via
        // resolve_function_overload → UserFunctionCall BEFORE the NoUserFunctions
        // field-op branch in expr.rs, so this arm is MOOT for real compose(...) calls.
        // It is retained as a defensive, correct-in-isolation typing helper and
        // regression guard exercised by the units.rs table tests.
        "compose" => {
            if arg_types.len() < 2 {
                return None;
            }
            if let (
                Type::Field { domain: b0, codomain: c },
                Type::Field { domain: a, codomain: b1 },
            ) = (&arg_types[0], &arg_types[1])
            {
                // Middle type B must be consistent: arg[0].domain == arg[1].codomain.
                if b0 != b1 {
                    return None;
                }
                Some(Type::Field {
                    domain: a.clone(),
                    codomain: c.clone(),
                })
            } else {
                None
            }
        }

        // sample(Field<D,C>, p) → C   THE §5.1 FIX: codomain, not Field
        "sample" => {
            if let Some(Type::Field { codomain, .. }) = arg_types.first() {
                Some(*codomain.clone())
            } else {
                None
            }
        }

        // gradient(Field<D,C>) → Field<D, result_codomain>
        // 1D (scalar D): result_codomain = gradient_quantity (scalar or Real)
        // nD (Point{n, scalar} D): result_codomain = Vector{n, gradient_quantity}
        "gradient" => {
            if let Some(Type::Field { domain, codomain }) = arg_types.first() {
                differential_codomain(DifferentialOp::Gradient, domain, codomain)
                    .map(|cod| Type::Field {
                        domain: domain.clone(),
                        codomain: Box::new(cod),
                    })
            } else {
                None
            }
        }

        // divergence(Field<Point{n,scalar}, Vector{n,scalar}>) → Field<D, scalar_codomain>
        "divergence" => {
            if let Some(Type::Field { domain, codomain }) = arg_types.first() {
                differential_codomain(DifferentialOp::Divergence, domain, codomain)
                    .map(|cod| Type::Field {
                        domain: domain.clone(),
                        codomain: Box::new(cod),
                    })
            } else {
                None
            }
        }

        // curl(Field<Point{3,scalar}, Vector{3,scalar}>) → Field<D, Vector{3,result}>
        "curl" => {
            if let Some(Type::Field { domain, codomain }) = arg_types.first() {
                differential_codomain(DifferentialOp::Curl, domain, codomain)
                    .map(|cod| Type::Field {
                        domain: domain.clone(),
                        codomain: Box::new(cod),
                    })
            } else {
                None
            }
        }

        // laplacian(Field<D, scalar_codomain>) → Field<D, scalar_codomain/domain²>
        "laplacian" => {
            if let Some(Type::Field { domain, codomain }) = arg_types.first() {
                differential_codomain(DifferentialOp::Laplacian, domain, codomain)
                    .map(|cod| Type::Field {
                        domain: domain.clone(),
                        codomain: Box::new(cod),
                    })
            } else {
                None
            }
        }

        // Unknown name — not a field op.
        _ => None,
    }
}

/// Result type per geometry-query helper. Sets the cell's `result_type` so
/// that downstream `value_type_kind_matches` accepts the post-process
/// `Value` (which is `Value::Undef` until GHR-ζ Phase 6 wires kernel
/// dispatch). Falling through to the first-arg type would mismatch — the
/// first arg is typically a `Geometry`, `Solid`, or `Vec3`, not the helper's
/// actual return type.
///
/// Per PRD §1 frozen list (Phase 1 wiring):
/// - `volume(solid)`         → `Scalar<Volume>`
/// - `area(surface)`         → `Scalar<Area>`
/// - `length(curve)`         → `Scalar<Length>`
/// - `perimeter(surface)`    → `Scalar<Length>`
/// - `centroid(geometry)`    → `Point3<Length>`
/// - `bounding_box(geometry)` → `BoundingBox` (the first-class
///   `Type::BoundingBox` variant; pairs with the existing
///   `Value::BoundingBox { min, max }` value variant at
///   `reify_types::value.rs:547` whose value→type inference at
///   `value.rs:1299` returns `Type::BoundingBox`)
/// - `distance(a, b)`        → `Scalar<Length>`
/// - `contains(a, b)`        → `Bool`
/// - `intersects(a, b)`      → `Bool`
/// - `geo_equiv(a, b)`       → `Bool`
/// - `angle(a, b)`           → `Scalar<Angle>`
/// - `curvature(curve, t)`   → `Scalar<Curvature>` (default; Surface overload
///   `curvature(faces(...)[i], pt)` → `Matrix<2,2,Curvature>` handled by
///   [`geometry_query_arg_aware_result_type`] — task 4315)
///
/// KGQ-ζ Phase 6 addition (task 3615):
/// - `normal(surface, point)` → `Vector3<Dimensionless>` (`Type::vec3(Type::dimensionless_scalar())`)
///   The quantity is `Type::dimensionless_scalar()` (dimensionless), NOT a `Scalar` dimension,
///   matching the `Value::Vector(vec![Value::Real(_); 3])` shape that
///   `dispatch_normal_vector3` constructs and that `Value.infer_type()` maps
///   back to `Type::Vector { n: 3, quantity: Box::new(Type::dimensionless_scalar()) }`.
///
/// Returns `None` for any other name (caller falls through to its default
/// type-inference path). Mirrors the contract of the sibling
/// [`topology_selector_result_type`].
pub(crate) fn geometry_query_result_type(name: &str) -> Option<reify_core::Type> {
    use reify_core::{DimensionVector, Type};
    Some(match name {
        "volume" => Type::Scalar {
            dimension: DimensionVector::VOLUME,
        },
        "area" => Type::Scalar {
            dimension: DimensionVector::AREA,
        },
        "length" => Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "perimeter" => Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "centroid" => Type::point3(Type::length()),
        "bounding_box" => Type::bounding_box(),
        "distance" => Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "contains" => Type::Bool,
        "intersects" => Type::Bool,
        "geo_equiv" => Type::Bool,
        "angle" => Type::angle(),
        "curvature" => Type::Scalar {
            dimension: DimensionVector::CURVATURE,
        },
        // KGQ-ζ (task 3615, Phase 6): at-point surface normal.
        // Returns a dimensionless unit vector — Value::Vector([Real,Real,Real]).
        // Type::dimensionless_scalar() (not a Scalar dimension) is the quantity so that the
        // dispatched Value::Vector(vec![Value::Real(_);3]).infer_type() == this.
        "normal" => Type::vec3(Type::dimensionless_scalar()),
        // ζ / C4 (task 4479): max deviation between actual and nominal.
        // max_deviation(actual: Geometry, nominal: Geometry) -> Scalar<Length>
        // Mirrors the `distance` arm: 2-arg geometry query, Length result.
        "max_deviation" => Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        _ => return None,
    })
}

/// The set of face-producing topology-selector names: calling one returns a
/// `List<Geometry>` of **surface** sub-handles.
///
/// This is the Surface-yielding subset of [`GEOMETRY_TOPOLOGY_SELECTOR_NAMES`]
/// (edges / edges_by_length / edges_parallel_to / edges_at_height / shared_edges
/// are excluded — they yield curve sub-handles). Used by the structural surface
/// detector in [`geometry_query_arg_aware_result_type`].
///
/// **Invariant:** every entry here MUST also appear in
/// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`; the converse need not hold (the curve
/// selectors are legitimately absent). Structurally enforced by the
/// `face_producing_selector_names_is_subset_of_geometry_topology_selector_names`
/// test.
const FACE_PRODUCING_SELECTOR_NAMES: &[&str] =
    &["faces", "faces_by_area", "faces_by_normal", "adjacent_faces"];

/// Returns `true` iff `arg.kind` is an `IndexAccess` whose `object` resolves to
/// a `FunctionCall` whose `function.name` is in [`FACE_PRODUCING_SELECTOR_NAMES`].
///
/// Structural surface detection — mirrors the `math_fn_result_type` precedent
/// of inspecting the COMPILED-ARG STRUCTURE rather than the undifferentiated
/// first-arg type (which would be `Type::Geometry` for both faces and edges
/// after index access, offering no discrimination). Called by
/// [`geometry_query_arg_aware_result_type`].
///
/// **Two object shapes (task 4118 γ).** After step-12 inserts the
/// `Selector → List<Geometry>` coercion, a re-typed face selector is wrapped in
/// a `ResolveSelector` coercion node between the `IndexAccess` object and the
/// selector `FunctionCall`:
/// `IndexAccess{ object: ResolveSelector{ FunctionCall{faces} } }`. The detector
/// unwraps that node to reach the inner `FunctionCall`. Still-`List<Geometry>`
/// selectors (e.g. `adjacent_faces`, which has no `Selector` re-typing and so no
/// coercion) keep the bare `IndexAccess{ FunctionCall }` shape, handled by the
/// fallback that inspects `object` directly.
fn is_surface_producing_arg(arg: &reify_ir::CompiledExpr) -> bool {
    use reify_ir::CompiledExprKind;
    let CompiledExprKind::IndexAccess { object, .. } = &arg.kind else {
        return false;
    };
    // Unwrap a ResolveSelector coercion node (re-typed face selectors); else
    // inspect the object directly (still-List selectors like adjacent_faces).
    let inner = match &object.kind {
        CompiledExprKind::ResolveSelector { selector } => selector.as_ref(),
        _ => object.as_ref(),
    };
    if let CompiledExprKind::FunctionCall { function, .. } = &inner.kind {
        return FACE_PRODUCING_SELECTOR_NAMES.contains(&function.name.as_str());
    }
    false
}

/// Arg-aware return-type override for geometry-query functions.
///
/// Currently handles only the **curvature Surface→Matrix<2,2,Curvature>**
/// overload: when `name == "curvature"` AND the first compiled arg is an
/// inline `faces(...)[i]` form (detected via structural inspection by
/// [`is_surface_producing_arg`]), returns
/// `Some(Type::Matrix{m:2, n:2, quantity:Scalar<Curvature>})`.
///
/// For every other `(name, arg)` combination — including curvature with a
/// let-bound solid, a curve IndexAccess, or no arg at all — returns `None`
/// so the caller falls through to [`geometry_query_result_type`]'s default.
///
/// ## Design — why structural inspection?
///
/// There is no `Type`-level Surface/Curve distinction:
/// `topology_selector_result_type` returns `Type::List(Geometry)` for BOTH
/// `faces()` and `edges()`, so a type-based discriminator would see identical
/// types for surface and curve sub-handles. The structural approach (checking
/// which selector the IndexAccess wraps) is the only available signal at the
/// `CompiledExpr` dispatch point, exactly as `math_fn_result_type` uses
/// `CompiledExprKind::ListLiteral` length rather than the undifferentiated
/// `Type::List` to recover the vector dimension.
///
/// ## Scope boundary
///
/// Only the **inline** `faces(...)[i]` form is detected. A curvature call
/// whose surface arg arrives via a let-binding or parameter (a `ValueRef`)
/// degrades to `Scalar<Curvature>`. Threading `Type::Surface` / `Type::Curve`
/// through the type system is a large cross-cutting change deferred to a
/// future task.
///
/// ## Wiring
///
/// Called in `expr.rs`'s `is_geometry_query` arm via
/// `geometry_query_arg_aware_result_type(name, compiled_args.first())
///     .or_else(|| geometry_query_result_type(name))`;
/// [`geometry_query_result_type`] stays **unchanged** (preserving the
/// maintenance-contract tests that iterate it).
pub(crate) fn geometry_query_arg_aware_result_type(
    name: &str,
    first_arg: Option<&reify_ir::CompiledExpr>,
) -> Option<reify_core::Type> {
    use reify_core::{DimensionVector, Type};
    if name == "curvature" && first_arg.is_some_and(is_surface_producing_arg) {
        Some(Type::Matrix {
            m: 2,
            n: 2,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::CURVATURE,
            }),
        })
    } else {
        None
    }
}

// --- Unit conversion ---

/// Convert a unit string and value to an SI-based `Value::Scalar`.
/// Returns `None` if the unit is unrecognized.
pub(crate) fn unit_to_scalar(value: f64, unit: &str) -> Option<(Value, DimensionVector)> {
    match unit {
        "mm" => Some((
            Value::Scalar {
                si_value: value * 0.001,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "cm" => Some((
            Value::Scalar {
                si_value: value * 0.01,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "m" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "in" => Some((
            Value::Scalar {
                si_value: value * 0.0254,
                dimension: DimensionVector::LENGTH,
            },
            DimensionVector::LENGTH,
        )),
        "deg" => Some((
            Value::Scalar {
                si_value: value * std::f64::consts::PI / 180.0,
                dimension: DimensionVector::ANGLE,
            },
            DimensionVector::ANGLE,
        )),
        "rad" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::ANGLE,
            },
            DimensionVector::ANGLE,
        )),
        "kg" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::MASS,
            },
            DimensionVector::MASS,
        )),
        "g" => Some((
            Value::Scalar {
                si_value: value * 0.001,
                dimension: DimensionVector::MASS,
            },
            DimensionVector::MASS,
        )),
        "s" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::TIME,
            },
            DimensionVector::TIME,
        )),
        // Kelvin needs a hardcoded fallback because `std.units` itself uses
        // `1K` in `BOLTZMANN_CONSTANT()`s body — fn bodies in std.units load
        // with no unit_registry seeded, so the K declared at units.ri can't
        // satisfy the same file's own quantity literals. Mirrors the kg/s/m
        // self-bootstrap entries above.
        "K" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::TEMPERATURE,
            },
            DimensionVector::TEMPERATURE,
        )),
        // Bare SI base units completing the standard set (factor 1.0).
        // A/mol/cd are the SI bases for Current/AmountOfSubstance/LuminousIntensity;
        // they need the same hardcoded fallback as kg/s/K so that stdlib fn bodies
        // and other unseeded-registry scopes can resolve these unit literals
        // (PRD §2.2 / decision D5).
        "A" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::CURRENT,
            },
            DimensionVector::CURRENT,
        )),
        "mol" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::AMOUNT_OF_SUBSTANCE,
            },
            DimensionVector::AMOUNT_OF_SUBSTANCE,
        )),
        "cd" => Some((
            Value::Scalar {
                si_value: value,
                dimension: DimensionVector::LUMINOUS_INTENSITY,
            },
            DimensionVector::LUMINOUS_INTENSITY,
        )),
        _ => None,
    }
}

// --- Unit registry ---

/// Internal unit entry — stored in the registry during compilation.
#[derive(Debug, Clone)]
pub struct UnitEntry {
    pub name: String,
    pub dimension: DimensionVector,
    /// SI conversion factor: si_value = value * factor.
    pub factor: f64,
    /// Additive offset for affine units (e.g., °C→K): si_value = value * factor + offset.
    pub offset: Option<f64>,
    pub is_pub: bool,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
    /// Display path of the module that introduced this unit via prelude seeding,
    /// e.g. "std/units" or "dep". `None` for units declared in the current module.
    pub source_module: Option<String>,
}

impl UnitEntry {
    /// Construct a `UnitEntry` for prelude-seeded units.
    ///
    /// Bakes in `SourceSpan::prelude()` (so `is_prelude()` checks and
    /// diagnostic labels behave correctly) and the originating module's
    /// display path. The six shared fields are copied from `cu`.
    pub fn from_compiled_for_prelude(cu: &CompiledUnit, source_module: String) -> UnitEntry {
        UnitEntry {
            name: cu.name.clone(),
            dimension: cu.dimension,
            factor: cu.factor,
            offset: cu.offset,
            is_pub: cu.is_pub,
            span: SourceSpan::prelude(),
            content_hash: cu.content_hash,
            source_module: Some(source_module),
        }
    }
}

/// Registry mapping unit names to compiled unit entries.
/// Built incrementally during the unit pre-pass so later units can reference earlier ones.
pub struct UnitRegistry {
    entries: HashMap<String, UnitEntry>,
}

impl UnitRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        UnitRegistry {
            entries: HashMap::new(),
        }
    }

    /// Register a unit entry. Returns `Err(entry)` if the name is already registered.
    pub fn register(&mut self, entry: UnitEntry) -> Result<(), Box<UnitEntry>> {
        if self.entries.contains_key(&entry.name) {
            Err(Box::new(entry))
        } else {
            self.entries.insert(entry.name.clone(), entry);
            Ok(())
        }
    }

    /// Seed a prelude unit entry into the registry (overwrite semantics).
    ///
    /// Used to pre-populate the registry with units from prelude modules
    /// before processing module-local declarations. Duplicate prelude entries
    /// resolve by load order (last wins).
    pub fn seed_prelude_unit(&mut self, entry: UnitEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Look up a unit by name.
    pub fn lookup(&self, name: &str) -> Option<&UnitEntry> {
        self.entries.get(name)
    }
}

impl Default for UnitRegistry {
    fn default() -> Self {
        UnitRegistry::new()
    }
}

// --- UnitExpr resolver (task 3803 / PRD §4.2) ---

/// Error returned by [`resolve_unit_expr`] when a `UnitExpr` cannot be folded.
///
/// All variants carry the *use-site* `span` that was threaded into the call —
/// the `UnitExpr` AST nodes carry no spans of their own, and the §3.1
/// contiguity invariant means the entire unit-expression is one source region.
#[derive(Debug, PartialEq)]
pub enum UnitResolveError {
    /// No entry for this unit name in the registry.
    UnknownUnit { name: String, span: SourceSpan },
    /// The unit has an additive offset (`UnitEntry.offset.is_some()`) and
    /// therefore cannot be used inside a compound expression.  Route bare
    /// affine literals (e.g. `20degC`) through the existing
    /// `lookup_unit_in_registry` / `unit_to_scalar` standalone path.
    AffineUnitInCompound { name: String, span: SourceSpan },
    /// The exponent in a `Pow` node is outside the `i8` range accepted by
    /// [`DimensionVector::pow`].  Realistic unit exponents are ≤ ±10; values
    /// beyond `i8::MIN..=i8::MAX` (±127) are rejected rather than silently
    /// wrapping or panicking.
    ExponentOutOfRange { exponent: i32, span: SourceSpan },
}

/// Convert a [`UnitResolveError`] into a user-facing [`Diagnostic`].
///
/// Centralises diagnostic messaging for compound unit resolution failures so
/// that both call sites (`expr.rs` and `type_resolution.rs`) emit consistent
/// error messages, and a future fourth `UnitResolveError` variant is handled in
/// exactly one place.
///
/// The span embedded in each error variant is used as the diagnostic label
/// location — it equals the `expr.span` threaded into `resolve_unit_expr` at
/// the call site, which covers the entire quantity literal source region.
pub(crate) fn unit_resolve_error_to_diagnostic(err: &UnitResolveError) -> Diagnostic {
    match err {
        UnitResolveError::UnknownUnit { name, span } => Diagnostic::error(format!(
            "unknown unit: {}",
            name
        ))
        .with_label(DiagnosticLabel::new(*span, "unrecognized unit")),

        UnitResolveError::AffineUnitInCompound { name, span } => Diagnostic::error(format!(
            "affine (offset) unit '{}' cannot be used in a compound unit expression",
            name
        ))
        .with_label(DiagnosticLabel::new(*span, "affine unit in compound")),

        UnitResolveError::ExponentOutOfRange { exponent, span } => Diagnostic::error(format!(
            "unit exponent {} out of range",
            exponent
        ))
        .with_label(DiagnosticLabel::new(*span, "exponent out of range")),
    }
}

/// Fold a `UnitExpr` AST node against `registry`, returning the combined
/// SI conversion factor and [`DimensionVector`] for the expression.
///
/// # Parameters
/// - `expr`     — the unit expression to evaluate (no spans stored in AST nodes)
/// - `registry` — the registry to look up atom names in
/// - `span`     — the use-site source span (stamped into any error)
///
/// # Returns
/// `Ok((factor, dimension))` where `si_value = numeric_value * factor`.
///
/// # Errors
/// - [`UnitResolveError::UnknownUnit`]          — atom name not in registry
/// - [`UnitResolveError::AffineUnitInCompound`] — affine unit in compound context
/// - [`UnitResolveError::ExponentOutOfRange`]   — `Pow` exponent outside `i8` range
///
/// # Standalone affine path
/// Bare affine literals like `20degC` must be routed through
/// `lookup_unit_in_registry` / `unit_to_scalar` (which applies the offset).
/// The ε integration task wires the bare-vs-compound routing in `expr.rs`.
pub fn resolve_unit_expr(
    expr: &reify_ast::UnitExpr,
    registry: &UnitRegistry,
    span: SourceSpan,
) -> Result<(f64, DimensionVector), UnitResolveError> {
    match expr {
        reify_ast::UnitExpr::Unit(name) => match registry.lookup(name) {
            None => Err(UnitResolveError::UnknownUnit {
                name: name.clone(),
                span,
            }),
            Some(entry) => {
                if entry.offset.is_some() {
                    return Err(UnitResolveError::AffineUnitInCompound {
                        name: name.clone(),
                        span,
                    });
                }
                Ok((entry.factor, entry.dimension))
            }
        },
        reify_ast::UnitExpr::Mul(a, b) => {
            let (fa, da) = resolve_unit_expr(a, registry, span)?;
            let (fb, db) = resolve_unit_expr(b, registry, span)?;
            Ok((fa * fb, da.mul(&db)))
        }
        reify_ast::UnitExpr::Div(a, b) => {
            let (fa, da) = resolve_unit_expr(a, registry, span)?;
            let (fb, db) = resolve_unit_expr(b, registry, span)?;
            Ok((fa / fb, da.div(&db)))
        }
        reify_ast::UnitExpr::Pow(a, n) => {
            let (fa, da) = resolve_unit_expr(a, registry, span)?;
            // `f64::powi` takes i32 natively — no narrowing needed for the factor.
            // `DimensionVector::pow` takes i8, so the exponent must be narrowed.
            // Guard the conversion so an out-of-range exponent surfaces as a
            // resolve error rather than a panic or silent wrap.  Realistic unit
            // exponents are ≤ ±10 (per the UnitExpr::Pow doc-comment).
            let n_i8 = i8::try_from(*n)
                .map_err(|_| UnitResolveError::ExponentOutOfRange { exponent: *n, span })?;
            Ok((fa.powi(*n), da.pow(n_i8)))
        }
    }
}

// --- Type alias registry ---

#[cfg(test)]
mod tests {
    use super::*;
    // Math-linalg construction family (task 4179) — single source of truth in
    // `crate::math_signatures`, imported here to pin disjointness from the five
    // geometry families and the dynamics-query family (both directions).
    use crate::math_signatures::MATH_CONSTRUCTION_NAMES;
    // Math-linalg operation/function family (task 4182 δ) — sibling slice in
    // `crate::math_signatures`, imported here to pin disjointness from the five
    // geometry families, the dynamics-query family, AND the construction family.
    use crate::math_signatures::MATH_OPERATION_NAMES;
    // §1.2 trig/transcendental family (task 4352) — third math sibling slice in
    // `crate::math_signatures`, imported here to pin disjointness from every
    // other name family (both directions).
    use crate::math_signatures::MATH_TRANSCENDENTAL_NAMES;
    // §13 joint-constructor family (mechanism β, task 4311) — single source of
    // truth in `crate::joint_signatures`, imported here to pin disjointness from
    // all eight sibling families (regression-lock: catches any future colliding
    // name added to EITHER the joint slice or a sibling slice).
    use crate::joint_signatures::JOINT_TYPED_FN_NAMES;
    // FEA stress-analysis reduction family (FEA-5, task 2884) — single source
    // of truth in `crate::analysis_signatures`, imported here to pin
    // disjointness from all sibling families.
    use crate::analysis_signatures::ANALYSIS_FN_NAMES;
    // Geometric-relation vocabulary (geometric-relations γ, task 4383) — single
    // source of truth in `crate::relation_signatures`, imported here to pin the
    // PURE relation family disjoint from every sibling family. The shared-verb
    // names `angle`/`distance` are deliberately NOT in this slice (they stay in
    // GEOMETRY_QUERY_NAMES and are arity-gated into relations in expr.rs).
    use crate::relation_signatures::RELATION_FN_NAMES;

    // Local fixtures for name families that have no pub single-source slice —
    // they are hardcoded match arms in `affine_map_algebra_result_type` and
    // `infer_list_helper_return_type`. Keep in sync with those functions.
    // Hoisted to module level so both the operation and transcendental
    // disjointness tests share a single copy, and a future addition to either
    // family only requires one edit here.
    const AFFINE_ALGEBRA_NAMES: &[&str] = &["affine_compose", "affine_inverse", "determinant"];
    const LIST_HELPER_NAMES: &[&str] = &["single", "flat_map"];

    // --- Step 21: Verify new geometry function names are recognized ---

    #[test]
    fn compile_geometry_linear_pattern_recognized() {
        assert!(is_geometry_function("linear_pattern"));
    }

    #[test]
    fn compile_geometry_circular_pattern_recognized() {
        assert!(is_geometry_function("circular_pattern"));
    }

    #[test]
    fn compile_geometry_mirror_recognized() {
        assert!(is_geometry_function("mirror"));
    }

    #[test]
    fn compile_geometry_loft_recognized() {
        assert!(is_geometry_function("loft"));
    }

    #[test]
    fn compile_geometry_shell_recognized() {
        assert!(is_geometry_function("shell"));
    }

    #[test]
    fn compile_geometry_thicken_recognized() {
        assert!(is_geometry_function("thicken"));
    }

    #[test]
    fn compile_geometry_offset_solid_recognized() {
        assert!(is_geometry_function("offset_solid"));
    }

    #[test]
    fn compile_geometry_fillet_all_recognized() {
        assert!(is_geometry_function("fillet_all"));
    }

    #[test]
    fn compile_geometry_draft_recognized() {
        assert!(is_geometry_function("draft"));
    }

    /// `shell_open` must be a recognised geometry function (step-1 RED).
    ///
    /// RED until step-2 adds "shell_open" to GEOMETRY_FUNCTION_NAMES.
    #[test]
    fn compile_geometry_shell_open_recognized() {
        assert!(
            is_geometry_function("shell_open"),
            "is_geometry_function(\"shell_open\") must be true after step-2 registration"
        );
    }

    // --- Boolean function recognition tests (step-1) ---

    #[test]
    fn compile_geometry_union_recognized() {
        assert!(is_geometry_function("union"));
    }

    #[test]
    fn compile_geometry_intersection_recognized() {
        assert!(is_geometry_function("intersection"));
    }

    #[test]
    fn compile_geometry_difference_recognized() {
        assert!(is_geometry_function("difference"));
    }

    #[test]
    fn compile_geometry_union_all_recognized() {
        assert!(is_geometry_function("union_all"));
    }

    #[test]
    fn compile_geometry_intersection_all_recognized() {
        assert!(is_geometry_function("intersection_all"));
    }

    #[test]
    fn compile_geometry_linear_pattern_2d_recognized() {
        assert!(is_geometry_function("linear_pattern_2d"));
    }

    #[test]
    fn compile_geometry_arbitrary_pattern_recognized() {
        assert!(is_geometry_function("arbitrary_pattern"));
    }

    // --- Sweep (pipe) compiler tests (task-310 step-13) ---

    #[test]
    fn is_geometry_function_sweep() {
        assert!(is_geometry_function("sweep"));
    }

    // --- Tube and pipe compound-shape tests (task-324 step-3) ---

    #[test]
    fn is_geometry_function_tube_recognized() {
        assert!(is_geometry_function("tube"));
    }

    #[test]
    fn is_geometry_function_pipe_recognized() {
        assert!(is_geometry_function("pipe"));
    }

    // --- Torus primitive (task-4157 step-5) ---

    #[test]
    fn is_geometry_function_torus_recognized() {
        assert!(is_geometry_function("torus"));
    }

    // --- Centred primitives (task-4159) ---

    #[test]
    fn is_geometry_function_box_centered_recognized() {
        assert!(is_geometry_function("box_centered"));
    }

    #[test]
    fn is_geometry_function_cylinder_centered_recognized() {
        assert!(is_geometry_function("cylinder_centered"));
    }

    // --- Cone (task-4156) ---

    #[test]
    fn is_geometry_function_cone_recognized() {
        // RED until step-6 adds "cone" to GEOMETRY_FUNCTION_NAMES.
        assert!(is_geometry_function("cone"));
    }

    // --- Geometry query helpers (task 2320 step-1) ---
    //
    // Sibling list to `GEOMETRY_FUNCTION_NAMES` for the three monomorphic
    // conformance-query helpers that return `Type::Bool` and dispatch at
    // eval-time via `reify_eval::geometry_ops::try_eval_conformance_query`.

    #[test]
    fn is_geometry_query_helper_recognises_is_watertight() {
        assert!(is_geometry_query_helper("is_watertight"));
    }

    #[test]
    fn is_geometry_query_helper_recognises_is_manifold() {
        assert!(is_geometry_query_helper("is_manifold"));
    }

    #[test]
    fn is_geometry_query_helper_recognises_is_orientable() {
        assert!(is_geometry_query_helper("is_orientable"));
    }

    #[test]
    fn is_geometry_query_helper_rejects_constructor_names() {
        // `box` is a constructor in `GEOMETRY_FUNCTION_NAMES` — it must NOT
        // satisfy the query-helper predicate, otherwise the two lists would
        // overlap and `is_geometry_let` would misclassify the let-binding.
        assert!(!is_geometry_query_helper("box"));
    }

    #[test]
    fn is_geometry_query_helper_rejects_unrelated_names() {
        // `volume` happens not to be a member of either list today; this
        // pins the negative answer so a future addition to the helpers does
        // not silently widen the predicate.
        assert!(!is_geometry_query_helper("volume"));
    }

    #[test]
    fn is_geometry_query_helper_rejects_empty_name() {
        assert!(!is_geometry_query_helper(""));
    }

    #[test]
    fn is_geometry_query_helper_is_case_sensitive() {
        // Reify function names are snake_case; PascalCase variants must not
        // match (mirrors the `GEOMETRY_FUNCTION_NAMES` case-sensitivity
        // contract documented above).
        assert!(!is_geometry_query_helper("IsWatertight"));
    }

    // --- Geometry topology-selector helpers (task 2324 step-8) ---
    //
    // Sibling list to `GEOMETRY_KINEMATIC_QUERY_NAMES` for the three
    // topology-selector helpers per `docs/prds/topology-selectors.md` §3.9:
    //   - `closest_point(point, geometry) -> Point3<Length>`
    //   - `is_on(point, geometry) -> Bool`
    //   - `angle_between_surfaces(a, b) -> Angle`
    // Eval-time dispatch is in
    // `reify_eval::geometry_ops::try_eval_topology_selector`, which routes to
    // `GeometryQuery::ClosestPointOnShape` / `PointOnShape` / `SurfaceAngle`.

    #[test]
    fn is_geometry_topology_selector_recognises_closest_point() {
        assert!(is_geometry_topology_selector("closest_point"));
    }

    #[test]
    fn is_geometry_topology_selector_recognises_is_on() {
        assert!(is_geometry_topology_selector("is_on"));
    }

    #[test]
    fn is_geometry_topology_selector_recognises_angle_between_surfaces() {
        assert!(is_geometry_topology_selector("angle_between_surfaces"));
    }

    #[test]
    fn is_geometry_topology_selector_rejects_conformance_query_names() {
        // `is_watertight` belongs to the conformance-query family; the lists
        // must remain disjoint so cell classification doesn't double-fire.
        assert!(!is_geometry_topology_selector("is_watertight"));
    }

    #[test]
    fn is_geometry_topology_selector_rejects_kinematic_query_names() {
        // `interferes` belongs to the kinematic-query family; these lists are
        // disjoint per the `GEOMETRY_KINEMATIC_QUERY_NAMES` doc-comment.
        assert!(!is_geometry_topology_selector("interferes"));
    }

    #[test]
    fn is_geometry_topology_selector_rejects_constructor_names() {
        // `box` is a constructor in `GEOMETRY_FUNCTION_NAMES` and must not
        // overlap with the topology-selector predicate.
        assert!(!is_geometry_topology_selector("box"));
    }

    #[test]
    fn is_geometry_topology_selector_rejects_empty_name() {
        assert!(!is_geometry_topology_selector(""));
    }

    #[test]
    fn is_geometry_topology_selector_is_case_sensitive() {
        // Reify function names are snake_case; PascalCase variants must not
        // match.
        assert!(!is_geometry_topology_selector("ClosestPoint"));
    }

    #[test]
    fn topology_selector_result_type_closest_point_is_point3_length() {
        assert_eq!(
            topology_selector_result_type("closest_point"),
            Some(reify_core::Type::point3(reify_core::Type::length()))
        );
    }

    #[test]
    fn topology_selector_result_type_is_on_is_bool() {
        assert_eq!(
            topology_selector_result_type("is_on"),
            Some(reify_core::Type::Bool)
        );
    }

    #[test]
    fn topology_selector_result_type_angle_between_surfaces_is_angle() {
        assert_eq!(
            topology_selector_result_type("angle_between_surfaces"),
            Some(reify_core::Type::angle())
        );
    }

    #[test]
    fn topology_selector_result_type_returns_none_for_unrecognised_names() {
        assert_eq!(topology_selector_result_type("is_watertight"), None);
        assert_eq!(topology_selector_result_type("interferes"), None);
        assert_eq!(topology_selector_result_type(""), None);
        // Defensive: names that will be added by task 2699 but are not in scope
        // for any other recognised family.
        assert_eq!(topology_selector_result_type("single"), None);
        assert_eq!(topology_selector_result_type("flat_map"), None);
    }

    // --- Task 2699 topology-selector registry — table-driven coverage ---
    //
    // Single source of truth for the 11 names wired by task 2699 (PRD §3.9).
    // Two test functions iterate this table: one asserts the predicate
    // `is_geometry_topology_selector`, the other asserts
    // `topology_selector_result_type`. Adding a 12th task-2699 name is a
    // one-line table edit — no per-name boilerplate.
    fn task_2699_topology_selector_cases() -> Vec<(&'static str, reify_core::Type)> {
        vec![
            // Task 4118 (γ): the 7 predicate/all selector constructors now
            // evaluate to a typed `Value::Selector(kind)` and so are typed
            // `Type::Selector(kind)` at compile time (not `List<Geometry>`).
            // The compiler inserts a `ResolveSelector` coercion node at the
            // three consumption sites (param-binding, single()/list-helper,
            // IndexAccess-object) to bridge `Selector → List<Geometry>`.
            (
                "edges",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
            ),
            (
                "faces",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
            ),
            (
                "edges_by_length",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
            ),
            (
                "faces_by_area",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
            ),
            (
                "faces_by_normal",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Face),
            ),
            (
                "edges_parallel_to",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
            ),
            (
                "edges_at_height",
                reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge),
            ),
            // adjacent_faces / shared_edges remain List<Geometry>: they are
            // RELATIONAL queries with no `LeafQuery` representation (4117's
            // LeafQuery = {Named,All,ByNormal,ByArea,ByLength,ByHeight,
            // ByParallel}), so they are out of scope for the Selector re-type.
            (
                "adjacent_faces",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "shared_edges",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "center_of_mass",
                reify_core::Type::point3(reify_core::Type::length()),
            ),
            (
                "moment_of_inertia",
                reify_core::Type::tensor(
                    2,
                    3,
                    reify_core::Type::Scalar {
                        dimension: reify_core::DimensionVector::MOMENT_OF_INERTIA,
                    },
                ),
            ),
        ]
    }

    #[test]
    fn is_geometry_topology_selector_recognises_all_task_2699_names() {
        for (name, _) in task_2699_topology_selector_cases() {
            assert!(
                is_geometry_topology_selector(name),
                "is_geometry_topology_selector({name:?}) must be true (task 2699 §3.9)"
            );
        }
    }

    #[test]
    fn topology_selector_result_type_for_task_2699_names_matches_table() {
        for (name, expected) in task_2699_topology_selector_cases() {
            assert_eq!(
                topology_selector_result_type(name),
                Some(expected.clone()),
                "topology_selector_result_type({name:?}) must equal {expected:?} (task 2699 §3.9)"
            );
        }
    }

    // Task 4118 (γ): the 7 predicate/all selector constructors are typed
    // `Type::Selector(kind)` (Edge / Face per the constructor), NOT
    // `List<Geometry>`. The compiler bridges `Selector → List<Geometry>` via a
    // `ResolveSelector` coercion node at the three consumption sites.
    #[test]
    fn topology_selector_result_type_for_re_typed_selectors_is_typed_selector() {
        use reify_core::Type;
        use reify_core::ty::SelectorKind;
        // edges/faces (All) and the predicate selectors.
        assert_eq!(
            topology_selector_result_type("faces"),
            Some(Type::Selector(SelectorKind::Face))
        );
        assert_eq!(
            topology_selector_result_type("edges"),
            Some(Type::Selector(SelectorKind::Edge))
        );
        assert_eq!(
            topology_selector_result_type("faces_by_normal"),
            Some(Type::Selector(SelectorKind::Face))
        );
        assert_eq!(
            topology_selector_result_type("faces_by_area"),
            Some(Type::Selector(SelectorKind::Face))
        );
        assert_eq!(
            topology_selector_result_type("edges_by_length"),
            Some(Type::Selector(SelectorKind::Edge))
        );
        assert_eq!(
            topology_selector_result_type("edges_at_height"),
            Some(Type::Selector(SelectorKind::Edge))
        );
        assert_eq!(
            topology_selector_result_type("edges_parallel_to"),
            Some(Type::Selector(SelectorKind::Edge))
        );
        // Relational selectors stay List<Geometry> (out of scope for the
        // Selector re-type — no LeafQuery representation).
        assert_eq!(
            topology_selector_result_type("adjacent_faces"),
            Some(Type::List(Box::new(Type::Geometry)))
        );
        assert_eq!(
            topology_selector_result_type("shared_edges"),
            Some(Type::List(Box::new(Type::Geometry)))
        );
    }

    // --- Task 3603 / GHR-α — geometry-query registry (PRD §1 Phase 1) ---
    //
    // The fifth geometry-name family, structurally parallel to the existing four
    // (`GEOMETRY_FUNCTION_NAMES`, `GEOMETRY_QUERY_HELPER_NAMES`,
    // `GEOMETRY_KINEMATIC_QUERY_NAMES`, `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`).
    // Per PRD §1 frozen list:
    //   volume / area / length / perimeter / centroid / bounding_box /
    //   distance / contains / intersects / geo_equiv / angle / curvature
    //
    // Phase 1 registers compile-time return types only; eval-time dispatch
    // arrives in Phase 6 (GHR-ζ). Until then, cells hold `Value::Undef`, which
    // `value_type_kind_matches` accepts for any type.
    fn phase1_geometry_query_cases() -> Vec<(&'static str, reify_core::Type)> {
        use reify_core::{DimensionVector, Type};
        vec![
            (
                "volume",
                Type::Scalar {
                    dimension: DimensionVector::VOLUME,
                },
            ),
            (
                "area",
                Type::Scalar {
                    dimension: DimensionVector::AREA,
                },
            ),
            (
                "length",
                Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                },
            ),
            (
                "perimeter",
                Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                },
            ),
            ("centroid", Type::point3(Type::length())),
            ("bounding_box", Type::bounding_box()),
            (
                "distance",
                Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                },
            ),
            ("contains", Type::Bool),
            ("intersects", Type::Bool),
            ("geo_equiv", Type::Bool),
            ("angle", Type::angle()),
            (
                "curvature",
                Type::Scalar {
                    dimension: DimensionVector::CURVATURE,
                },
            ),
        ]
    }

    #[test]
    fn is_geometry_query_recognises_all_phase1_names() {
        for (name, _) in phase1_geometry_query_cases() {
            assert!(
                is_geometry_query(name),
                "is_geometry_query({name:?}) must be true (GHR-α PRD §1)"
            );
        }
    }

    #[test]
    fn geometry_query_result_type_for_all_phase1_names_matches_table() {
        for (name, expected) in phase1_geometry_query_cases() {
            assert_eq!(
                geometry_query_result_type(name),
                Some(expected.clone()),
                "geometry_query_result_type({name:?}) must equal {expected:?} (GHR-α PRD §1)"
            );
        }
    }

    /// Disjointness invariant: `is_geometry_query` must reject names from the
    /// four other geometry-name families. Without this, cell classification
    /// could double-fire when adding the fifth dispatch arm in expr.rs.
    #[test]
    fn is_geometry_query_rejects_other_family_names() {
        // Constructor family (`GEOMETRY_FUNCTION_NAMES`).
        assert!(!is_geometry_query("box"), "must reject constructor 'box'");
        // Conformance-query family (`GEOMETRY_QUERY_HELPER_NAMES`).
        assert!(
            !is_geometry_query("is_watertight"),
            "must reject conformance-query 'is_watertight'"
        );
        // Kinematic-query family (`GEOMETRY_KINEMATIC_QUERY_NAMES`).
        assert!(
            !is_geometry_query("interferes"),
            "must reject kinematic-query 'interferes'"
        );
        // Topology-selector family (`GEOMETRY_TOPOLOGY_SELECTOR_NAMES`).
        assert!(
            !is_geometry_query("closest_point"),
            "must reject topology-selector 'closest_point'"
        );
        // Empty / unrelated.
        assert!(!is_geometry_query(""), "must reject empty name");
        assert!(
            !is_geometry_query("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case. The
    /// PascalCase / camelCase form must not match (mirrors the other four
    /// family case-sensitivity contracts).
    #[test]
    fn is_geometry_query_is_case_sensitive() {
        assert!(!is_geometry_query("Volume"));
        assert!(!is_geometry_query("BoundingBox"));
        assert!(!is_geometry_query("boundingBox"));
    }

    /// `geometry_query_result_type` returns `None` for any name not in the
    /// Phase-1 frozen list — matches the contract of the sibling
    /// `topology_selector_result_type`.
    #[test]
    fn geometry_query_result_type_returns_none_for_unrecognised_names() {
        assert_eq!(geometry_query_result_type("box"), None);
        assert_eq!(geometry_query_result_type("is_watertight"), None);
        assert_eq!(geometry_query_result_type("closest_point"), None);
        assert_eq!(geometry_query_result_type(""), None);
    }

    /// Disjointness invariant — forward direction (each `GEOMETRY_QUERY_NAMES`
    /// entry must NOT appear in any of the four sibling family slices).
    /// Complements `is_geometry_query_rejects_other_family_names` (the inverse
    /// direction); together they pin the doc-comment disjointness contract.
    /// Without this, a name added to `GEOMETRY_QUERY_NAMES` that also lived in
    /// e.g. `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` would silently route through
    /// the topology-selector arm (dispatched first in `expr.rs::infer_type`)
    /// and the geometry-query arm would be dead code.
    #[test]
    fn geometry_query_names_are_disjoint_from_other_families() {
        for name in GEOMETRY_QUERY_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            assert!(
                !FIELD_OP_NAMES.contains(name),
                "GEOMETRY_QUERY_NAMES entry {name:?} must NOT also be in \
                 FIELD_OP_NAMES (field-op family, task 4219)"
            );
        }
    }

    /// Disjointness invariant for the RBD-β dynamics-query family (task 3829).
    /// Every `DYNAMICS_QUERY_NAMES` entry (`body_mass_props`) must be absent
    /// from all five geometry families, so a name can satisfy at most one
    /// classification predicate in `expr.rs::infer_type`'s `NoUserFunctions`
    /// ladder. Sibling to `geometry_query_names_are_disjoint_from_other_families`
    /// (extended above with the converse `DYNAMICS_QUERY_NAMES` assert) — the
    /// pair pins disjointness in both directions.
    #[test]
    fn dynamics_query_names_are_disjoint_from_other_families() {
        for name in DYNAMICS_QUERY_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            assert!(
                !FIELD_OP_NAMES.contains(name),
                "DYNAMICS_QUERY_NAMES entry {name:?} must NOT also be in \
                 FIELD_OP_NAMES (field-op family, task 4219)"
            );
        }
    }

    /// Disjointness invariant for the math-linalg construction family (task
    /// 4179). Every `MATH_CONSTRUCTION_NAMES` entry (`vec` / `matrix` / `diag`
    /// / `identity`) must be absent from all five geometry families AND the
    /// dynamics-query family, so a name can satisfy at most one classification
    /// predicate in `expr.rs::resolve_function_overload`'s `NoUserFunctions`
    /// ladder. Forward sibling to `is_math_typed_fn_rejects_other_family_…`
    /// (the predicate-level reverse direction lives in `math_signatures.rs`);
    /// the converse asserts in the geometry / dynamics disjointness tests above
    /// pin the other direction. Because the names are pinned disjoint from
    /// every other family, the new arm's position in the ladder is unobservable.
    #[test]
    fn math_typed_fn_names_are_disjoint_from_other_families() {
        for name in MATH_CONSTRUCTION_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ — \
                 constructors and operations are disjoint slices)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            assert!(
                !FIELD_OP_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 FIELD_OP_NAMES (field-op family, task 4219)"
            );
            assert!(
                !MATH_TRANSCENDENTAL_NAMES.contains(name),
                "MATH_CONSTRUCTION_NAMES entry {name:?} must NOT also be in \
                 MATH_TRANSCENDENTAL_NAMES (§1.2 trig/transcendental family, task 4352 — \
                 constructors and transcendentals are disjoint slices)"
            );
        }
    }

    /// Disjointness invariant for the math-linalg OPERATION family (task 4182
    /// δ). Every `MATH_OPERATION_NAMES` entry (`sqrt` / `dot` / `determinant` /
    /// `eigenvalues` / `complex` / …) must be absent from all five geometry
    /// families, the dynamics-query family, AND the math-linalg CONSTRUCTION
    /// family — so a name can satisfy at most one classification predicate in
    /// `expr.rs::resolve_function_overload`'s `NoUserFunctions` ladder, and the
    /// operation/construction split stays a partition (never overlapping
    /// slices). Sibling to `math_typed_fn_names_are_disjoint_from_other_families`
    /// (which pins the construction family); the converse asserts added to the
    /// geometry / dynamics / construction disjointness tests above pin the other
    /// direction.
    ///
    /// Also pins disjointness from the two EARLIER arms in the same
    /// `NoUserFunctions` ladder that this six-family set didn't cover (amendment:
    /// reviewer test_coverage): the affine constructor/algebra families
    /// (`AFFINE_MAP_CONSTRUCTOR_NAMES` + `affine_map_algebra_result_type`'s arms)
    /// and the list-helper family (`infer_list_helper_return_type`'s arms). A
    /// math op sharing a name with an earlier arm would be silently shadowed and
    /// produce a wrong cell type with no failing test. `determinant` is the one
    /// DELIBERATE overlap with affine-algebra — the affine arm fires only for an
    /// `AffineMap` first arg and otherwise falls through to the math arm (arg-type
    /// disambiguated), so it is the documented exception.
    #[test]
    fn math_operation_fn_names_are_disjoint_from_other_families() {
        // AFFINE_ALGEBRA_NAMES / LIST_HELPER_NAMES are hoisted to module level
        // (shared with `math_transcendental_fn_names_are_disjoint_from_other_families`).
        for name in MATH_OPERATION_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179 — \
                 operations and constructors are disjoint slices)"
            );
            // Affine constructor family — an EARLIER arm in expr.rs's
            // NoUserFunctions ladder; a same-named math op would be shadowed.
            assert!(
                !AFFINE_MAP_CONSTRUCTOR_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 AFFINE_MAP_CONSTRUCTOR_NAMES (affine constructor family — earlier \
                 arm in the NoUserFunctions ladder would shadow it)"
            );
            // Affine ALGEBRA free-fns — also an earlier arm. `determinant` is the
            // ONE intentional overlap (arg-type disambiguated: the affine arm
            // fires only for an AffineMap first arg, else falls through to the
            // math arm), so it is the documented exception; every other
            // affine-algebra name would UNCONDITIONALLY shadow a same-named math op.
            assert!(
                !AFFINE_ALGEBRA_NAMES.contains(name) || *name == "determinant",
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be an affine-algebra \
                 free-fn (except the intentional, arg-type-disambiguated `determinant` \
                 overlap)"
            );
            // List-helper family — also an earlier ladder arm; a collision would
            // shadow the math arm.
            assert!(
                !LIST_HELPER_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be a list-helper \
                 (`single` / `flat_map` — earlier arm in the NoUserFunctions ladder \
                 would shadow it)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            assert!(
                !FIELD_OP_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 FIELD_OP_NAMES (field-op family, task 4219)"
            );
            assert!(
                !MATH_TRANSCENDENTAL_NAMES.contains(name),
                "MATH_OPERATION_NAMES entry {name:?} must NOT also be in \
                 MATH_TRANSCENDENTAL_NAMES (§1.2 trig/transcendental family, task 4352 — \
                 operations and transcendentals are disjoint slices)"
            );
        }
    }

    /// Disjointness invariant for the §1.2 trig/transcendental family (task
    /// 4352). Every `MATH_TRANSCENDENTAL_NAMES` entry (`sin` / `cos` / `asin` /
    /// `exp` / `log` / …) must be absent from every other name family — the five
    /// geometry families, the dynamics-query family, the two other math families
    /// (construction + operation), the joint / dynamics-constructor / analysis /
    /// field-op / relation families, and the affine constructor / algebra /
    /// list-helper ladder arms — so a name satisfies at most one classification
    /// predicate in `expr.rs::resolve_function_overload`'s `NoUserFunctions`
    /// ladder, and the transcendental/operation/construction split stays a
    /// partition. This forward sweep is the substantive guard (it catches a
    /// colliding name added to EITHER side); the converse asserts in the two
    /// math-sibling tests pin the math partition's documented both-ways story.
    /// Mirrors `math_operation_fn_names_are_disjoint_from_other_families`.
    #[test]
    fn math_transcendental_fn_names_are_disjoint_from_other_families() {
        for name in MATH_TRANSCENDENTAL_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !JOINT_TYPED_FN_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 JOINT_TYPED_FN_NAMES (§13 joint family, task 4311)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            assert!(
                !FIELD_OP_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 FIELD_OP_NAMES (field-op family, task 4219)"
            );
            assert!(
                !RELATION_FN_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 RELATION_FN_NAMES (geometric-relation family, task 4383)"
            );
            assert!(
                !AFFINE_MAP_CONSTRUCTOR_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be in \
                 AFFINE_MAP_CONSTRUCTOR_NAMES (affine constructor family — earlier \
                 arm in the NoUserFunctions ladder would shadow it)"
            );
            assert!(
                !AFFINE_ALGEBRA_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be an \
                 affine-algebra free-fn (earlier arm would shadow it; unlike \
                 `determinant`, no trig name has an intentional affine-algebra overlap)"
            );
            assert!(
                !LIST_HELPER_NAMES.contains(name),
                "MATH_TRANSCENDENTAL_NAMES entry {name:?} must NOT also be a \
                 list-helper (`single` / `flat_map` — earlier arm would shadow it)"
            );
        }
    }

    /// Maintenance invariant — iterates `GEOMETRY_QUERY_NAMES` *directly*
    /// (not a hand-maintained fixture vec) and asserts every entry has a
    /// corresponding result-type arm in `geometry_query_result_type`. Without
    /// this, a 13th name added to the slice but missing a result-type arm
    /// would pass the table-driven test (which iterates the fixture, not the
    /// slice) and `.expect()`-panic in production when first called via the
    /// `expr.rs::infer_type` dispatch
    /// (`geometry_query_result_type(name).expect("is_geometry_query implies result type")`).
    #[test]
    fn geometry_query_names_each_have_a_result_type() {
        for name in GEOMETRY_QUERY_NAMES {
            assert!(
                geometry_query_result_type(name).is_some(),
                "GEOMETRY_QUERY_NAMES entry {name:?} has no matching arm in \
                 geometry_query_result_type — adding a name to the slice \
                 REQUIRES adding a parallel arm (or expr.rs will panic at \
                 runtime)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 3615 — KGQ-ζ: `normal` compiler registration
    // -----------------------------------------------------------------------

    /// `is_geometry_query("normal")` must return true once the KGQ-ζ
    /// registration lands in step-4. Fails until then.
    #[test]
    fn is_geometry_query_recognises_normal() {
        assert!(
            is_geometry_query("normal"),
            "is_geometry_query(\"normal\") must be true after KGQ-ζ step-4 registration"
        );
    }

    /// `geometry_query_result_type("normal")` must return
    /// `Some(Type::vec3(Type::dimensionless_scalar()))` — a dimensionless 3D vector, i.e.
    /// `Type::Vector { n: 3, quantity: Box::new(Type::dimensionless_scalar()) }`.
    ///
    /// This is the exact type that `Value::Vector(vec![Value::Real(_); 3]).infer_type()`
    /// produces (verified: value.rs `try_infer_type` for Vector sets quantity =
    /// first component's `try_infer_type()`, and `Value::Real → Type::dimensionless_scalar()`).
    #[test]
    fn geometry_query_result_type_for_normal_is_vec3_real() {
        use reify_core::Type;
        assert_eq!(
            geometry_query_result_type("normal"),
            Some(Type::vec3(Type::dimensionless_scalar())),
            "geometry_query_result_type(\"normal\") must be Some(Type::vec3(Type::dimensionless_scalar())) \
             after KGQ-ζ step-4 registration"
        );
    }

    // -----------------------------------------------------------------------
    // Task 3803 — resolve_unit_expr: registry-folding evaluator for UnitExpr
    // -----------------------------------------------------------------------
    //
    // Inline fixture registry shared by step-1 through step-10 tests.
    // Seeded with a minimal set of SI units sufficient to cover all test cases:
    //   m   → (1.0,    LENGTH)
    //   kN  → (1000.0, FORCE)
    //   mm  → (0.001,  LENGTH)
    //   kg  → (1.0,    MASS)
    //   s   → (1.0,    TIME)
    //   degC → (1.0,   TEMPERATURE, offset=273.15)  ← affine; seeded in step-9
    //
    // All UnitEntry fields are pub → struct literals constructed directly;
    // no test-support helper needed.

    fn make_resolver_registry() -> UnitRegistry {
        use reify_core::{ContentHash, DimensionVector, SourceSpan};
        let mut reg = UnitRegistry::new();
        for (name, dimension, factor, offset) in &[
            ("m", DimensionVector::LENGTH, 1.0_f64, None),
            ("kN", DimensionVector::FORCE, 1000.0_f64, None),
            ("mm", DimensionVector::LENGTH, 0.001_f64, None),
            ("kg", DimensionVector::MASS, 1.0_f64, None),
            ("s", DimensionVector::TIME, 1.0_f64, None),
        ] {
            reg.register(UnitEntry {
                name: name.to_string(),
                dimension: *dimension,
                factor: *factor,
                offset: *offset,
                is_pub: true,
                span: SourceSpan::empty(0),
                content_hash: ContentHash::of_str(name),
                source_module: None,
            })
            .unwrap();
        }
        reg
    }

    // --- Step-1: Unit fold arm (RED — resolve_unit_expr / UnitResolveError absent) ---

    #[test]
    fn resolve_unit_expr_unit_m_returns_length_factor_1() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(10, 11);
        let result = resolve_unit_expr(
            &reify_ast::UnitExpr::Unit("m".to_string()),
            &reg,
            use_span,
        );
        let (factor, dim) = result.expect("m must resolve successfully");
        assert!((factor - 1.0).abs() < 1e-9, "m: factor must ≈ 1.0, got {factor}");
        assert_eq!(dim, DimensionVector::LENGTH, "m: dimension must be LENGTH");
    }

    #[test]
    fn resolve_unit_expr_unit_kn_returns_force_factor_1000() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(10, 12);
        let result = resolve_unit_expr(
            &reify_ast::UnitExpr::Unit("kN".to_string()),
            &reg,
            use_span,
        );
        let (factor, dim) = result.expect("kN must resolve successfully");
        assert!((factor - 1000.0).abs() < 1e-9, "kN: factor must ≈ 1000.0, got {factor}");
        assert_eq!(dim, DimensionVector::FORCE, "kN: dimension must be FORCE");
    }

    #[test]
    fn resolve_unit_expr_unknown_unit_returns_err_with_use_site_span() {
        use reify_core::SourceSpan;
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(20, 23);
        let err = resolve_unit_expr(
            &reify_ast::UnitExpr::Unit("kgg".to_string()),
            &reg,
            use_span,
        )
        .expect_err("kgg must not resolve");
        assert_eq!(
            err,
            UnitResolveError::UnknownUnit {
                name: "kgg".to_string(),
                span: use_span,
            },
            "error must carry the offending name and the use-site span"
        );
    }

    // --- Step-3/4: Mul fold arm ---

    #[test]
    fn resolve_unit_expr_mul_kn_m_returns_torque_factor_1000() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(30, 35);
        // 5kN*m = Mul(Unit("kN"), Unit("m"))
        let expr = reify_ast::UnitExpr::Mul(
            Box::new(reify_ast::UnitExpr::Unit("kN".to_string())),
            Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("kN*m must resolve successfully");
        assert!(
            (factor - 1000.0).abs() < 1e-9,
            "kN*m: factor must ≈ 1000.0 (1000.0 * 1.0), got {factor}"
        );
        // FORCE (kg·m·s⁻²) × LENGTH (m) = kg·m²·s⁻²  (= ENERGY)
        let expected_dim = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);
        assert_eq!(dim, expected_dim, "kN*m: dimension must be FORCE·LENGTH");
    }

    // --- Step-5: Div fold arm (RED — todo!() panics) ---

    #[test]
    fn resolve_unit_expr_div_left_assoc_dynamic_viscosity() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(40, 49);
        // kg/m/s = Div(Div(Unit("kg"), Unit("m")), Unit("s"))
        let expr = reify_ast::UnitExpr::Div(
            Box::new(reify_ast::UnitExpr::Div(
                Box::new(reify_ast::UnitExpr::Unit("kg".to_string())),
                Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
            )),
            Box::new(reify_ast::UnitExpr::Unit("s".to_string())),
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("kg/m/s must resolve successfully");
        // All SI base units → factor = 1.0/1.0/1.0 = 1.0
        assert!(
            (factor - 1.0).abs() < 1e-9,
            "kg/m/s: factor must ≈ 1.0, got {factor}"
        );
        assert_eq!(
            dim,
            DimensionVector::DYNAMIC_VISCOSITY,
            "kg/m/s dimension must be DYNAMIC_VISCOSITY (kg·m⁻¹·s⁻¹)"
        );
    }

    #[test]
    fn resolve_unit_expr_div_factor_divides() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(50, 54);
        // kN/m = Div(Unit("kN"), Unit("m"))
        let expr = reify_ast::UnitExpr::Div(
            Box::new(reify_ast::UnitExpr::Unit("kN".to_string())),
            Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("kN/m must resolve successfully");
        // 1000.0 / 1.0 = 1000.0
        assert!(
            (factor - 1000.0).abs() < 1e-9,
            "kN/m: factor must ≈ 1000.0, got {factor}"
        );
        let expected_dim = DimensionVector::FORCE.div(&DimensionVector::LENGTH);
        assert_eq!(dim, expected_dim, "kN/m: dimension must be FORCE/LENGTH");
    }

    // --- Step-7: Pow fold arm (RED — todo!() panics) ---

    #[test]
    fn resolve_unit_expr_pow_mm_squared_is_area() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(60, 64);
        // mm^2 = Pow(Unit("mm"), 2)
        let expr = reify_ast::UnitExpr::Pow(
            Box::new(reify_ast::UnitExpr::Unit("mm".to_string())),
            2,
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("mm^2 must resolve successfully");
        // 0.001.powi(2) = 1e-6
        assert!(
            (factor - 1e-6).abs() < 1e-15,
            "mm^2: factor must ≈ 1e-6, got {factor}"
        );
        assert_eq!(dim, DimensionVector::AREA, "mm^2: dimension must be AREA");
    }

    #[test]
    fn resolve_unit_expr_pow_negative_exponent_s_minus2() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(70, 74);
        // s^-2 = Pow(Unit("s"), -2)
        let expr = reify_ast::UnitExpr::Pow(
            Box::new(reify_ast::UnitExpr::Unit("s".to_string())),
            -2,
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("s^-2 must resolve successfully");
        assert!(
            (factor - 1.0).abs() < 1e-9,
            "s^-2: factor must ≈ 1.0 (1.0^-2), got {factor}"
        );
        let expected_dim = DimensionVector::TIME.pow(-2);
        assert_eq!(dim, expected_dim, "s^-2: dimension must be TIME^-2");
    }

    #[test]
    fn resolve_unit_expr_pow_zero_exponent_is_dimensionless() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(80, 83);
        // m^0 = Pow(Unit("m"), 0)
        let expr = reify_ast::UnitExpr::Pow(
            Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
            0,
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("m^0 must resolve successfully");
        assert!(
            (factor - 1.0).abs() < 1e-9,
            "m^0: factor must ≈ 1.0 (1.0^0), got {factor}"
        );
        assert_eq!(
            dim,
            DimensionVector::DIMENSIONLESS,
            "m^0: dimension must be DIMENSIONLESS"
        );
    }

    #[test]
    fn resolve_unit_expr_div_pow_kg_per_m3_is_mass_density() {
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_resolver_registry();
        let use_span = SourceSpan::new(90, 96);
        // kg/m^3 = Div(Unit("kg"), Pow(Unit("m"), 3))
        let expr = reify_ast::UnitExpr::Div(
            Box::new(reify_ast::UnitExpr::Unit("kg".to_string())),
            Box::new(reify_ast::UnitExpr::Pow(
                Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
                3,
            )),
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("kg/m^3 must resolve successfully");
        // factor = 1.0 / 1.0^3 = 1.0
        assert!(
            (factor - 1.0).abs() < 1e-9,
            "kg/m^3: factor must ≈ 1.0, got {factor}"
        );
        assert_eq!(
            dim,
            DimensionVector::MASS_DENSITY,
            "kg/m^3: dimension must be MASS_DENSITY"
        );
    }

    // --- Step-9: affine-rejection tests (RED — AffineUnitInCompound variant absent) ---

    fn make_affine_registry() -> UnitRegistry {
        use reify_core::{ContentHash, DimensionVector, SourceSpan};
        let mut reg = make_resolver_registry();
        // degC is an affine unit: si_value = value * 1.0 + 273.15
        reg.register(UnitEntry {
            name: "degC".to_string(),
            dimension: DimensionVector::TEMPERATURE,
            factor: 1.0,
            offset: Some(273.15),
            is_pub: true,
            span: SourceSpan::empty(0),
            content_hash: ContentHash::of_str("degC"),
            source_module: None,
        })
        .unwrap();
        reg
    }

    #[test]
    fn resolve_unit_expr_affine_in_compound_div_rejected() {
        use reify_core::SourceSpan;
        let reg = make_affine_registry();
        let use_span = SourceSpan::new(100, 108);
        // 5degC/m = Div(Unit("degC"), Unit("m"))
        let expr = reify_ast::UnitExpr::Div(
            Box::new(reify_ast::UnitExpr::Unit("degC".to_string())),
            Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
        );
        let err = resolve_unit_expr(&expr, &reg, use_span)
            .expect_err("degC/m must be rejected");
        assert_eq!(
            err,
            UnitResolveError::AffineUnitInCompound {
                name: "degC".to_string(),
                span: use_span,
            },
            "error must be AffineUnitInCompound with offending name and use-site span"
        );
    }

    #[test]
    fn resolve_unit_expr_bare_affine_unit_also_rejected() {
        // Even a bare `Unit("degC")` returns AffineUnitInCompound — the fold
        // never silently drops an offset. Bare affine literals must use the
        // standalone lookup_unit_in_registry / unit_to_scalar path.
        use reify_core::SourceSpan;
        let reg = make_affine_registry();
        let use_span = SourceSpan::new(110, 114);
        let err = resolve_unit_expr(
            &reify_ast::UnitExpr::Unit("degC".to_string()),
            &reg,
            use_span,
        )
        .expect_err("bare Unit(degC) must be rejected by resolve_unit_expr");
        assert_eq!(
            err,
            UnitResolveError::AffineUnitInCompound {
                name: "degC".to_string(),
                span: use_span,
            },
            "bare affine unit must produce AffineUnitInCompound error"
        );
    }

    #[test]
    fn resolve_unit_expr_non_affine_compound_still_resolves() {
        // Regression: adding the affine guard must not break non-affine units.
        use reify_core::{DimensionVector, SourceSpan};
        let reg = make_affine_registry();
        let use_span = SourceSpan::new(120, 124);
        let expr = reify_ast::UnitExpr::Mul(
            Box::new(reify_ast::UnitExpr::Unit("kg".to_string())),
            Box::new(reify_ast::UnitExpr::Unit("m".to_string())),
        );
        let (factor, dim) = resolve_unit_expr(&expr, &reg, use_span)
            .expect("kg*m must still resolve after affine guard is added");
        assert!((factor - 1.0).abs() < 1e-9);
        assert_eq!(dim, DimensionVector::MASS.mul(&DimensionVector::LENGTH));
    }

    // --- Task 4173: bare SI base units A / mol / cd in unit_to_scalar ---

    #[test]
    fn unit_to_scalar_resolves_bare_si_base_units_a_mol_cd() {
        use reify_core::DimensionVector;

        // Table-driven: (unit string, expected DimensionVector) — all factor 1.0.
        for (unit, expected) in [
            ("A", DimensionVector::CURRENT),
            ("mol", DimensionVector::AMOUNT_OF_SUBSTANCE),
            ("cd", DimensionVector::LUMINOUS_INTENSITY),
        ] {
            let (val, dim) = unit_to_scalar(2.0, unit)
                .unwrap_or_else(|| panic!("bare SI base unit {unit} must resolve"));
            assert_eq!(dim, expected, "{unit}: returned DimensionVector mismatch");
            match val {
                Value::Scalar { si_value, dimension } => {
                    assert!(
                        (si_value - 2.0).abs() < 1e-12,
                        "{unit}: si_value must ≈ 2.0 (factor 1.0), got {si_value}"
                    );
                    assert_eq!(
                        dimension, expected,
                        "{unit}: Value inner dimension mismatch"
                    );
                }
                _ => panic!("{unit}: expected Value::Scalar, got different variant"),
            }
        }
    }

    // ── AffineMap constructor registration tests (step-11, task 3960 β) ────────
    //
    // These tests iterate the production `AFFINE_MAP_CONSTRUCTOR_NAMES` directly
    // (not a local copy), so they track the production list automatically: adding
    // a 12th constructor — or removing one — is exercised here without any test
    // edit, and a name present in the const but unhandled by the classifier/
    // result-type fns fails loudly.

    #[test]
    fn is_affine_map_constructor_recognises_all_constructor_names() {
        for &name in AFFINE_MAP_CONSTRUCTOR_NAMES {
            assert!(
                is_affine_map_constructor(name),
                "{name} must be recognised as an AffineMap constructor"
            );
        }
    }

    #[test]
    fn is_affine_map_constructor_rejects_unrelated_names() {
        // Transform constructors are a distinct family (rigid vs general affine).
        assert!(!is_affine_map_constructor("box"));
        assert!(!is_affine_map_constructor("transform3"));
        assert!(!is_affine_map_constructor("transform3_identity"));
        // affine_apply / affine_compose are out of scope (tasks γ/ζ).
        assert!(!is_affine_map_constructor("affine_apply"));
        assert!(!is_affine_map_constructor(""));
        // Case-sensitive: PascalCase must not match snake_case names.
        assert!(!is_affine_map_constructor("AffineScale"));
    }

    #[test]
    fn affine_map_constructor_result_type_is_affine_map_3_for_all() {
        for &name in AFFINE_MAP_CONSTRUCTOR_NAMES {
            assert_eq!(
                affine_map_constructor_result_type(name),
                Some(reify_core::Type::AffineMap(3)),
                "{name} must resolve to Type::AffineMap(3)"
            );
        }
    }

    // ─── η/4480: tolerancing marker (`nominal()`) registration ────────────────

    #[test]
    fn is_tolerancing_marker_recognises_all_marker_names() {
        for &name in TOLERANCING_MARKER_NAMES {
            assert!(
                is_tolerancing_marker(name),
                "{name} must be recognised as a tolerancing marker builtin"
            );
        }
    }

    #[test]
    fn tolerancing_marker_result_type_is_geometry_for_all() {
        for &name in TOLERANCING_MARKER_NAMES {
            assert_eq!(
                tolerancing_marker_result_type(name),
                Some(reify_core::Type::Geometry),
                "{name} must resolve to Type::Geometry (zero-arg inert marker)"
            );
        }
    }

    #[test]
    fn is_tolerancing_marker_rejects_unrelated_names() {
        // Geometry constructors / queries are distinct families.
        assert!(!is_tolerancing_marker("box"));
        assert!(!is_tolerancing_marker("max_deviation"));
        assert!(!is_tolerancing_marker("effective_tolerance_zone"));
        assert!(!is_tolerancing_marker(""));
        // Case-sensitive: Reify builtin names are snake_case.
        assert!(!is_tolerancing_marker("Nominal"));
        // `nominal` is a common *param* name (DimensionalTolerance.nominal,
        // etc.) — but only the zero-arg call builtin is the marker here.
        assert_eq!(tolerancing_marker_result_type("nominal_diameter"), None);
    }

    /// The marker family must be disjoint from the sibling builtin-name
    /// families so a name cannot satisfy two classification predicates in the
    /// `expr.rs` `NoUserFunctions` ladder (the same invariant the sibling
    /// `*_are_disjoint_from_other_families` tests pin).
    #[test]
    fn tolerancing_marker_names_are_disjoint_from_other_families() {
        for &name in TOLERANCING_MARKER_NAMES {
            assert!(!is_geometry_function(name), "{name} in GEOMETRY_FUNCTION_NAMES");
            assert!(!is_geometry_query(name), "{name} in GEOMETRY_QUERY_NAMES");
            assert!(
                !is_geometry_query_helper(name),
                "{name} in GEOMETRY_QUERY_HELPER_NAMES"
            );
            assert!(
                !is_geometry_topology_selector(name),
                "{name} in GEOMETRY_TOPOLOGY_SELECTOR_NAMES"
            );
            assert!(!is_dynamics_query(name), "{name} in DYNAMICS_QUERY_NAMES");
            assert!(
                !is_dynamics_constructor(name),
                "{name} in DYNAMICS_CONSTRUCTOR_NAMES"
            );
            assert!(
                !is_affine_map_constructor(name),
                "{name} in AFFINE_MAP_CONSTRUCTOR_NAMES"
            );
            assert!(!is_field_op(name), "{name} in FIELD_OP_NAMES");
        }
    }

    #[test]
    fn affine_map_constructor_result_type_returns_none_for_unknown() {
        assert_eq!(affine_map_constructor_result_type("box"), None);
        assert_eq!(affine_map_constructor_result_type("transform3"), None);
        assert_eq!(affine_map_constructor_result_type(""), None);
    }

    // --- affine_map_algebra_result_type tests (step-8) ---

    #[test]
    fn algebra_affine_compose_returns_affine_map_3() {
        assert_eq!(
            affine_map_algebra_result_type("affine_compose", None),
            Some(reify_core::Type::AffineMap(3))
        );
        // First arg type does not change the result for affine_compose.
        assert_eq!(
            affine_map_algebra_result_type(
                "affine_compose",
                Some(&reify_core::Type::AffineMap(3))
            ),
            Some(reify_core::Type::AffineMap(3))
        );
    }

    #[test]
    fn algebra_affine_inverse_returns_option_affine_map_3() {
        assert_eq!(
            affine_map_algebra_result_type("affine_inverse", None),
            Some(reify_core::Type::Option(Box::new(reify_core::Type::AffineMap(3))))
        );
        assert_eq!(
            affine_map_algebra_result_type(
                "affine_inverse",
                Some(&reify_core::Type::AffineMap(3))
            ),
            Some(reify_core::Type::Option(Box::new(reify_core::Type::AffineMap(3))))
        );
    }

    #[test]
    fn algebra_determinant_with_affine_map_arg_returns_real() {
        assert_eq!(
            affine_map_algebra_result_type(
                "determinant",
                Some(&reify_core::Type::AffineMap(3))
            ),
            Some(reify_core::Type::dimensionless_scalar())
        );
    }

    #[test]
    fn algebra_determinant_with_non_affine_arg_returns_none() {
        // When first arg is not AffineMap, fall through to the existing
        // matrix-determinant first-arg behaviour (return None here).
        assert_eq!(
            affine_map_algebra_result_type("determinant", None),
            None
        );
        assert_eq!(
            affine_map_algebra_result_type("determinant", Some(&reify_core::Type::dimensionless_scalar())),
            None
        );
    }

    #[test]
    fn algebra_unknown_names_return_none() {
        assert_eq!(affine_map_algebra_result_type("affine_scale", None), None);
        assert_eq!(affine_map_algebra_result_type("affine_apply", None), None);
        assert_eq!(affine_map_algebra_result_type("box", None), None);
        assert_eq!(affine_map_algebra_result_type("", None), None);
    }

    // --- 2-D profile face producers (task-4160) ---
    // RED until step-6 adds "rectangle" and "circle" to GEOMETRY_FUNCTION_NAMES.

    #[test]
    fn is_geometry_function_rectangle_recognized() {
        // RED until step-6 adds "rectangle" to GEOMETRY_FUNCTION_NAMES.
        assert!(is_geometry_function("rectangle"));
    }

    #[test]
    fn is_geometry_function_circle_recognized() {
        // RED until step-6 adds "circle" to GEOMETRY_FUNCTION_NAMES.
        assert!(is_geometry_function("circle"));
    }

    // --- 2-D profile face producers (task-4161) ---
    // RED until step-6 adds "polygon" and "ellipse" to GEOMETRY_FUNCTION_NAMES.

    #[test]
    fn is_geometry_function_polygon_recognized() {
        // RED until step-6 adds "polygon" to GEOMETRY_FUNCTION_NAMES.
        assert!(is_geometry_function("polygon"));
    }

    #[test]
    fn is_geometry_function_ellipse_recognized() {
        // RED until step-6 adds "ellipse" to GEOMETRY_FUNCTION_NAMES.
        assert!(is_geometry_function("ellipse"));
    }

    // --- split topology selector (task 4190, step-5 RED / step-6 GREEN) ---
    //
    // `split(solid, plane) -> List<Solid>` joins the topology-selector family
    // (GEOMETRY_TOPOLOGY_SELECTOR_NAMES), NOT GEOMETRY_FUNCTION_NAMES, because
    // it returns List<Solid> (multi-output). Family-disjointness invariant: once
    // "split" is added to GEOMETRY_TOPOLOGY_SELECTOR_NAMES, the existing
    // disjointness test `geometry_query_names_are_disjoint_from_other_families`
    // continues to pass because "split" is absent from all other families.

    #[test]
    fn is_geometry_topology_selector_recognises_split() {
        // RED until step-6 adds "split" to GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
        assert!(is_geometry_topology_selector("split"));
    }

    #[test]
    fn topology_selector_result_type_split_is_list_geometry() {
        // RED until step-6 adds the "split" arm to topology_selector_result_type.
        assert_eq!(
            topology_selector_result_type("split"),
            Some(reify_core::Type::List(Box::new(reify_core::Type::Geometry)))
        );
    }

    #[test]
    fn split_is_not_a_geometry_function() {
        // split is in the topology-selector family, NOT the constructor family.
        assert!(!is_geometry_function("split"));
    }

    #[test]
    fn split_is_not_a_geometry_query_helper() {
        assert!(!is_geometry_query_helper("split"));
    }

    #[test]
    fn split_is_not_a_geometry_kinematic_query() {
        assert!(!is_geometry_kinematic_query("split"));
    }

    // --- Named-leaf constructors (task 4119 δ, step-8 GREEN) -----------------
    //
    // `face(geometry, name) -> Selector(Face)`, `edge(geometry, name) ->
    // Selector(Edge)`, `solid_body(geometry, name) -> Selector(Body)` join the
    // topology-selector family.  `body` is intentionally absent (RBD ctor).

    #[test]
    fn is_geometry_topology_selector_recognises_face_edge_solid_body() {
        assert!(is_geometry_topology_selector("face"));
        assert!(is_geometry_topology_selector("edge"));
        assert!(is_geometry_topology_selector("solid_body"));
    }

    #[test]
    fn topology_selector_result_type_named_ctors() {
        assert_eq!(
            topology_selector_result_type("face"),
            Some(reify_core::Type::Selector(reify_core::ty::SelectorKind::Face))
        );
        assert_eq!(
            topology_selector_result_type("edge"),
            Some(reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge))
        );
        assert_eq!(
            topology_selector_result_type("solid_body"),
            Some(reify_core::Type::Selector(reify_core::ty::SelectorKind::Body))
        );
    }

    /// Guard: `body` must NOT be in GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
    /// `body` is the RBD mechanism constructor (JOINT_TYPED_FN_NAMES →
    /// StructureRef("Mechanism")); `solid_body` is the Named-leaf BodySelector
    /// ctor (PRD §11.1).  This test catches any accidental future collision.
    #[test]
    fn body_is_not_a_topology_selector_solid_body_is_the_named_ctor() {
        assert!(
            !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&"body"),
            "`body` must NOT be in GEOMETRY_TOPOLOGY_SELECTOR_NAMES — it is the \
             RBD mechanism constructor (JOINT_TYPED_FN_NAMES); use `solid_body` \
             for the Named-leaf BodySelector ctor (PRD §11.1)"
        );
        assert!(
            is_geometry_topology_selector("solid_body"),
            "`solid_body` must be in GEOMETRY_TOPOLOGY_SELECTOR_NAMES (the Named-leaf \
             BodySelector ctor, PRD §11.1)"
        );
    }

    /// Disjointness regression-lock for the §13 joint-constructor family
    /// (mechanism β, task 4311). Every `JOINT_TYPED_FN_NAMES` entry must be
    /// absent from all eight sibling slices so a name satisfies at most one
    /// classification predicate in `expr.rs::resolve_function_overload`'s
    /// `NoUserFunctions` ladder.
    ///
    /// This test is GREEN on arrival — the 17 joint names are inherently
    /// disjoint from the existing families. It acts as a regression lock:
    /// adding a colliding name to EITHER the joint slice OR a sibling slice
    /// triggers a failure, catching the bug at test time rather than at
    /// production call-time.
    ///
    /// Mirrors `math_typed_fn_names_are_disjoint_from_other_families` and
    /// `dynamics_query_names_are_disjoint_from_other_families`.
    #[test]
    fn joint_typed_fn_names_are_disjoint_from_other_families() {
        for name in JOINT_TYPED_FN_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (geometry-constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
        }
    }

    /// Disjointness regression-lock for the FEA stress-analysis reduction
    /// family (FEA-5, task 2884). Every `ANALYSIS_FN_NAMES` entry must be
    /// absent from all sibling family slices so a name satisfies at most one
    /// classification predicate in `expr.rs::resolve_function_overload`'s
    /// `NoUserFunctions` ladder.
    ///
    /// The 5 analysis names are domain-specific and trivially disjoint — this
    /// is a regression lock, not a behavioural change. Mirrors
    /// `joint_typed_fn_names_are_disjoint_from_other_families`.
    #[test]
    fn analysis_fn_names_are_disjoint_from_other_families() {
        for name in ANALYSIS_FN_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (geometry-constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !JOINT_TYPED_FN_NAMES.contains(name),
                "ANALYSIS_FN_NAMES entry {name:?} must NOT also be in \
                 JOINT_TYPED_FN_NAMES (joint-constructor family, task 4311)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 4315 — geometry_query_arg_aware_result_type unit tests (step-1 RED)
    // -----------------------------------------------------------------------
    //
    // Tests for geometry_query_arg_aware_result_type(name, first_arg):
    //   curvature + inline faces(...)[i] → Some(Matrix{2,2,Curvature})
    //   curvature + edges / ValueRef / bare-faces / None → None
    //   non-curvature name → None
    //   regression: geometry_query_result_type("curvature") unchanged
    //
    // Fixtures are hand-built CompiledExprs (mirroring math_signatures tests).
    // These tests FAIL TO COMPILE until step-2 adds the function — that is the
    // expected RED signal.

    /// Build a bare FunctionCall CompiledExpr for `selector_name` with no args.
    /// Represents the `faces(solid)` / `edges(solid)` / ... call before indexing.
    fn make_selector_call(selector_name: &str) -> reify_ir::CompiledExpr {
        use reify_core::hash::ContentHash;
        use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction};
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: selector_name.to_string(),
                    qualified_name: format!("std::{}", selector_name),
                },
                args: vec![],
            },
            result_type: reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            content_hash: ContentHash::of(selector_name.as_bytes()),
        }
    }

    /// Wrap `object` in `IndexAccess { object, index: Literal(Int(0)) }`.
    /// Result type is `Type::Geometry` (element of the selector's List<Geometry>).
    fn index_0(object: reify_ir::CompiledExpr) -> reify_ir::CompiledExpr {
        use reify_ir::{CompiledExpr, Value};
        let idx = CompiledExpr::literal(Value::Int(0), reify_core::Type::Int);
        CompiledExpr::index_access(object, idx, reify_core::Type::Geometry)
    }

    /// (a) curvature with inline faces(...)[0] → Some(Matrix{2,2,Curvature})
    #[test]
    fn curvature_faces_index_returns_matrix_2x2_curvature() {
        use reify_core::{DimensionVector, Type};
        let surface_arg = index_0(make_selector_call("faces"));
        let expected = Type::Matrix {
            m: 2,
            n: 2,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::CURVATURE,
            }),
        };
        assert_eq!(
            geometry_query_arg_aware_result_type("curvature", Some(&surface_arg)),
            Some(expected),
            "curvature(faces(...)[i], pt) must compile-type as Matrix{{2,2,Curvature}}"
        );
    }

    /// (a-ext) Other face-producing selectors: faces_by_area, faces_by_normal, adjacent_faces
    #[test]
    fn curvature_other_face_selectors_return_matrix_2x2_curvature() {
        use reify_core::{DimensionVector, Type};
        let expected = Type::Matrix {
            m: 2,
            n: 2,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::CURVATURE,
            }),
        };
        for sel in ["faces_by_area", "faces_by_normal", "adjacent_faces"] {
            let surface_arg = index_0(make_selector_call(sel));
            assert_eq!(
                geometry_query_arg_aware_result_type("curvature", Some(&surface_arg)),
                Some(expected.clone()),
                "curvature({sel}(...)[i], pt) must compile-type as Matrix{{2,2,Curvature}}"
            );
        }
    }

    /// (b) curvature with edges(...)[0] → None (curve selector, not surface)
    #[test]
    fn curvature_edges_index_returns_none() {
        let edge_arg = index_0(make_selector_call("edges"));
        assert_eq!(
            geometry_query_arg_aware_result_type("curvature", Some(&edge_arg)),
            None,
            "curvature(edges(...)[i], pt) must return None (falls through to Scalar default)"
        );
    }

    /// (c) curvature with a let-bound ValueRef arg → None
    #[test]
    fn curvature_value_ref_arg_returns_none() {
        use reify_core::identity::ValueCellId;
        use reify_ir::CompiledExpr;
        let solid_arg = CompiledExpr::value_ref(
            ValueCellId::new("S", "my_solid"),
            reify_core::Type::Geometry,
        );
        assert_eq!(
            geometry_query_arg_aware_result_type("curvature", Some(&solid_arg)),
            None,
            "curvature(let_bound_solid, pt) must return None (structural detector sees no inline faces(...)[i])"
        );
    }

    /// (d) curvature with a bare faces() FunctionCall (not wrapped in IndexAccess) → None
    #[test]
    fn curvature_bare_faces_call_returns_none() {
        let bare_faces = make_selector_call("faces");
        assert_eq!(
            geometry_query_arg_aware_result_type("curvature", Some(&bare_faces)),
            None,
            "curvature(faces(...), pt) without index must return None"
        );
    }

    /// (e) Non-curvature name with an inline surface arg → None
    #[test]
    fn non_curvature_name_with_surface_arg_returns_none() {
        let surface_arg = index_0(make_selector_call("faces"));
        assert_eq!(
            geometry_query_arg_aware_result_type("area", Some(&surface_arg)),
            None,
            "non-curvature name 'area' with surface arg must return None"
        );
    }

    /// (f) curvature with no first arg → None
    #[test]
    fn curvature_no_arg_returns_none() {
        use reify_ir::CompiledExpr;
        assert_eq!(
            geometry_query_arg_aware_result_type("curvature", None::<&CompiledExpr>),
            None,
            "curvature with no first arg must return None"
        );
    }

    /// Regression pin: geometry_query_result_type("curvature") stays Scalar<Curvature>
    /// (the arg-aware fn overrides it only for the inline-surface form; the table default
    /// must remain unchanged so the .or_else fallthrough keeps working).
    #[test]
    fn geometry_query_result_type_curvature_unchanged() {
        use reify_core::{DimensionVector, Type};
        assert_eq!(
            geometry_query_result_type("curvature"),
            Some(Type::Scalar {
                dimension: DimensionVector::CURVATURE,
            }),
            "geometry_query_result_type(\"curvature\") must remain Scalar<Curvature> (default table)"
        );
    }

    /// Wrap `selector_call` in `ResolveSelector` then `IndexAccess[0]` — the
    /// NEW shape produced after task 4118 step-12 inserts the coercion node
    /// between the `IndexAccess` object and the selector `FunctionCall`:
    /// `IndexAccess{ object: ResolveSelector{ FunctionCall }, .. }`.
    fn index_0_resolve_selector(
        selector_call: reify_ir::CompiledExpr,
    ) -> reify_ir::CompiledExpr {
        index_0(reify_ir::CompiledExpr::resolve_selector(selector_call))
    }

    /// task 4118 step-13/14 — the detector must see THROUGH a `ResolveSelector`
    /// wrapper. After step-12, inline `faces(s)[0]` lowers to
    /// `IndexAccess{ object: ResolveSelector{ FunctionCall{faces} } }`;
    /// `is_surface_producing_arg` must still return true for the face-producing
    /// selectors and false for the curve selector `edges` wrapped the same way.
    ///
    /// RED until step-14 unwraps the `ResolveSelector` between the `IndexAccess`
    /// object and the selector `FunctionCall`.
    #[test]
    fn is_surface_producing_arg_sees_through_resolve_selector_wrapper() {
        for sel in ["faces", "faces_by_area", "faces_by_normal"] {
            let wrapped = index_0_resolve_selector(make_selector_call(sel));
            assert!(
                is_surface_producing_arg(&wrapped),
                "is_surface_producing_arg must see through ResolveSelector to the \
                 face-producing selector {sel:?}"
            );
        }
        let wrapped_edges = index_0_resolve_selector(make_selector_call("edges"));
        assert!(
            !is_surface_producing_arg(&wrapped_edges),
            "is_surface_producing_arg must return false for a ResolveSelector-wrapped \
             curve selector (edges)"
        );
    }

    /// Regression pin: the bare (un-wrapped) `IndexAccess{ FunctionCall }` shape
    /// must STILL be recognized — `adjacent_faces` stays `List<Geometry>` (no
    /// `ResolveSelector` wrapper), and the hand-built fixtures above depend on
    /// the bare-FunctionCall fallback. (Green before AND after step-14.)
    #[test]
    fn is_surface_producing_arg_still_recognizes_bare_function_call() {
        let bare = index_0(make_selector_call("adjacent_faces"));
        assert!(
            is_surface_producing_arg(&bare),
            "is_surface_producing_arg must still match the bare \
             IndexAccess{{FunctionCall}} shape (adjacent_faces stays List<Geometry>)"
        );
    }

    /// Structural invariant: every name in FACE_PRODUCING_SELECTOR_NAMES must
    /// also appear in GEOMETRY_TOPOLOGY_SELECTOR_NAMES.
    ///
    /// Enforces the documented subset relationship: if a future edit adds a
    /// name to FACE_PRODUCING_SELECTOR_NAMES that is absent from
    /// GEOMETRY_TOPOLOGY_SELECTOR_NAMES, this test fails immediately rather
    /// than silently wiring the structural detector to an unregistered name.
    #[test]
    fn face_producing_selector_names_is_subset_of_geometry_topology_selector_names() {
        for name in FACE_PRODUCING_SELECTOR_NAMES {
            assert!(
                GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "FACE_PRODUCING_SELECTOR_NAMES entry {name:?} must also appear in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (documented subset invariant)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 4278 — DYNAMICS_CONSTRUCTOR_NAMES / is_dynamics_constructor (step-9 RED)
    // -----------------------------------------------------------------------
    //
    // Tests for the new DYNAMICS_CONSTRUCTOR_NAMES family and is_dynamics_constructor
    // predicate:
    //   is_dynamics_constructor("point_mass") → true
    //   is_dynamics_constructor("mass_properties") → true
    //   unrelated names → false
    //   disjointness: every DYNAMICS_CONSTRUCTOR_NAMES entry absent from every
    //     other classification family
    //
    // These tests FAIL TO COMPILE until step-10 adds `DYNAMICS_CONSTRUCTOR_NAMES`
    // and `is_dynamics_constructor` — that is the expected RED signal.

    /// Task 4278 step-9 (RED). `is_dynamics_constructor("point_mass")` and
    /// `is_dynamics_constructor("mass_properties")` must return true. Unrelated
    /// names — "box" (geometry constructor), "body_mass_props" (DYNAMICS_QUERY_NAMES),
    /// and "sqrt" (math-op) — must return false.
    /// RED until step-10 adds `DYNAMICS_CONSTRUCTOR_NAMES` and
    /// `is_dynamics_constructor`.
    #[test]
    fn dynamics_constructor_predicate_recognizes_ctor_names() {
        assert!(
            is_dynamics_constructor("point_mass"),
            "is_dynamics_constructor must recognize 'point_mass' (task 4278)"
        );
        assert!(
            is_dynamics_constructor("mass_properties"),
            "is_dynamics_constructor must recognize 'mass_properties' (task 4278)"
        );
        assert!(
            !is_dynamics_constructor("box"),
            "is_dynamics_constructor must reject geometry name 'box'"
        );
        assert!(
            !is_dynamics_constructor("body_mass_props"),
            "is_dynamics_constructor must reject dynamics-query name 'body_mass_props' \
             (DYNAMICS_QUERY_NAMES is a separate family)"
        );
        assert!(
            !is_dynamics_constructor("sqrt"),
            "is_dynamics_constructor must reject math-op name 'sqrt'"
        );
    }

    /// Task 4278 step-9 (RED). Disjointness invariant for the dynamics-constructor
    /// family. Every `DYNAMICS_CONSTRUCTOR_NAMES` entry (`point_mass` /
    /// `mass_properties`) must be absent from all sibling classification families so
    /// a name can satisfy at most one predicate in `expr.rs::infer_type`'s
    /// `NoUserFunctions` ladder. Sibling to
    /// `dynamics_query_names_are_disjoint_from_other_families` (units.rs).
    /// RED until step-10 adds `DYNAMICS_CONSTRUCTOR_NAMES`.
    #[test]
    fn dynamics_constructor_names_are_disjoint_from_other_families() {
        for name in DYNAMICS_CONSTRUCTOR_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (geometry-constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family — separate slice)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !JOINT_TYPED_FN_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 JOINT_TYPED_FN_NAMES (joint-constructor family, mechanism β task 4311)"
            );
            assert!(
                !AFFINE_MAP_CONSTRUCTOR_NAMES.contains(name),
                "DYNAMICS_CONSTRUCTOR_NAMES entry {name:?} must NOT also be in \
                 AFFINE_MAP_CONSTRUCTOR_NAMES (affine-map constructor family)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Task 4219 — std.fields α: field-op compiler signatures
    // -----------------------------------------------------------------------

    /// `is_field_op` must return `true` for every name in `FIELD_OP_NAMES`
    /// and `false` for unrelated / empty names.
    ///
    /// RED until `FIELD_OP_NAMES` / `is_field_op` are defined in `units.rs`.
    #[test]
    fn is_field_op_recognises_all_field_op_names() {
        for name in &[
            "fn_field",
            "from_samples",
            "restrict",
            "compose",
            "sample",
            "gradient",
            "divergence",
            "curl",
            "laplacian",
        ] {
            assert!(
                is_field_op(name),
                "is_field_op({name:?}) must be true (std.fields α PRD §5.1)"
            );
        }
        // Unrelated names must not be recognised.
        assert!(!is_field_op("box"), "must reject geometry constructor 'box'");
        assert!(!is_field_op("volume"), "must reject geometry query 'volume'");
        assert!(!is_field_op("vec"), "must reject math-linalg 'vec'");
        assert!(!is_field_op(""), "must reject empty name");
        assert!(!is_field_op("SAMPLE"), "must be case-sensitive");
    }

    /// Disjointness invariant — forward direction (each `FIELD_OP_NAMES`
    /// entry must NOT appear in any of the six sibling family slices).
    /// Without this, a field-op name added that also lived in e.g.
    /// `GEOMETRY_QUERY_NAMES` would silently route through the
    /// geometry-query arm (dispatched earlier in the ladder) and the
    /// field-op arm would be dead code.
    ///
    /// RED until `FIELD_OP_NAMES` is defined in `units.rs`.
    #[test]
    fn field_op_names_are_disjoint_from_other_families() {
        // List-helper free-fn names (`infer_list_helper_return_type`'s match
        // arms) have no public single-source slice — they are hardcoded match
        // arms — so this local fixture mirrors them (amendment: reviewer
        // suggestion S3). The list-helper arm sits EARLIER in expr.rs's
        // NoUserFunctions ladder, so a field-op name colliding with one would
        // be silently shadowed and the field-op arm would become dead code.
        const LIST_HELPER_NAMES: &[&str] = &["single", "flat_map"];

        for name in FIELD_OP_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !AFFINE_MAP_CONSTRUCTOR_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 AFFINE_MAP_CONSTRUCTOR_NAMES (affine constructor family — earlier \
                 arm in the NoUserFunctions ladder would shadow it)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            // List-helper family — an EARLIER ladder arm; a collision would
            // shadow the field-op arm (reviewer suggestion S3).
            assert!(
                !LIST_HELPER_NAMES.contains(name),
                "FIELD_OP_NAMES entry {name:?} must NOT also be a list-helper \
                 (`single` / `flat_map` — earlier arm in the NoUserFunctions \
                 ladder would shadow it)"
            );
        }
    }

    /// Disjointness regression-lock for the geometric-relation vocabulary
    /// (geometric-relations γ, task 4383). Every entry of `RELATION_FN_NAMES`
    /// (the PURE relation family) must be absent from all sibling family slices
    /// so a name satisfies at most one classification predicate in
    /// `expr.rs::resolve_function_overload`'s `NoUserFunctions` ladder.
    ///
    /// The shared-verb names `angle`/`distance` are deliberately NOT in
    /// `RELATION_FN_NAMES`: they stay in `GEOMETRY_QUERY_NAMES` (the arity-2
    /// DERIVE form) and are claimed as relations only at arity 3 by the arg-aware
    /// `relation_signatures::relation_fn_result_type` arm placed BEFORE the
    /// geometry-query arm. Pinning the pure family disjoint keeps that single
    /// arity gate the SOLE point where a relation name overlaps a sibling family.
    ///
    /// The 9 relation names are inherently disjoint from the existing families,
    /// so this is GREEN on arrival — a regression lock that fails if a colliding
    /// name is later added to EITHER the relation slice or a sibling slice.
    /// Mirrors `field_op_names_are_disjoint_from_other_families`.
    #[test]
    fn relation_fn_names_are_disjoint_from_other_families() {
        for name in RELATION_FN_NAMES {
            assert!(
                !GEOMETRY_FUNCTION_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_FUNCTION_NAMES (geometry-constructor family)"
            );
            assert!(
                !GEOMETRY_QUERY_HELPER_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_HELPER_NAMES (conformance-query family)"
            );
            assert!(
                !GEOMETRY_KINEMATIC_QUERY_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_KINEMATIC_QUERY_NAMES (kinematic-query family)"
            );
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES (topology-selector family)"
            );
            assert!(
                !GEOMETRY_QUERY_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 GEOMETRY_QUERY_NAMES (geometry-query family — the home of the \
                 arity-2 angle/distance DERIVE forms)"
            );
            assert!(
                !DYNAMICS_QUERY_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_QUERY_NAMES (dynamics-query family, RBD-β task 3829)"
            );
            assert!(
                !MATH_CONSTRUCTION_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 MATH_CONSTRUCTION_NAMES (math-linalg construction family, task 4179)"
            );
            assert!(
                !MATH_OPERATION_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 MATH_OPERATION_NAMES (math-linalg operation family, task 4182 δ)"
            );
            assert!(
                !AFFINE_MAP_CONSTRUCTOR_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 AFFINE_MAP_CONSTRUCTOR_NAMES (affine constructor family)"
            );
            assert!(
                !DYNAMICS_CONSTRUCTOR_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 DYNAMICS_CONSTRUCTOR_NAMES (dynamics-constructor family, task 4278)"
            );
            assert!(
                !ANALYSIS_FN_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 ANALYSIS_FN_NAMES (FEA stress-analysis reduction family, task 2884)"
            );
            assert!(
                !JOINT_TYPED_FN_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 JOINT_TYPED_FN_NAMES (joint-constructor family, task 4311)"
            );
            assert!(
                !FIELD_OP_NAMES.contains(name),
                "RELATION_FN_NAMES entry {name:?} must NOT also be in \
                 FIELD_OP_NAMES (field-op family)"
            );
        }
    }

    /// Per PRD §5.1: `field_op_result_type` must resolve each field-op name
    /// to the expected return type when given well-shaped arguments.
    ///
    /// Mirrors `geometry_query_result_type_for_all_phase1_names_matches_table`
    /// (units.rs:1302). Model: geometry query table test.
    ///
    /// RED until `field_op_result_type` is implemented in units.rs.
    #[test]
    fn field_op_result_type_matches_prd_5_1_table() {
        use reify_core::{DimensionVector, Type};

        // fn_field(Function{params:[Real], return_type:Real}) → Field<Real,Real>
        assert_eq!(
            field_op_result_type(
                "fn_field",
                &[Type::Function {
                    params: vec![Type::dimensionless_scalar()],
                    return_type: Box::new(Type::dimensionless_scalar()),
                }]
            ),
            Some(Type::Field {
                domain: Box::new(Type::dimensionless_scalar()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "fn_field(Function{{[Real]->Real}}) must produce Field<Real,Real> (PRD §5.1)"
        );

        // from_samples(List<Point3<Length>>, List<Scalar<Temperature>>, method)
        //   → Field<Point3<Length>, Scalar<Temperature>>
        let p3l = Type::point3(Type::length());
        let s_temp = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        };
        assert_eq!(
            field_op_result_type(
                "from_samples",
                &[
                    Type::List(Box::new(p3l.clone())),
                    Type::List(Box::new(s_temp.clone())),
                    Type::dimensionless_scalar(), // method arg — ignored for typing
                ]
            ),
            Some(Type::Field {
                domain: Box::new(p3l.clone()),
                codomain: Box::new(s_temp.clone()),
            }),
            "from_samples(List<D>,List<C>,_) must produce Field<D,C> (PRD §5.1)"
        );

        // restrict(Field<Real,Real>, _) → Field<Real,Real>  (domain/codomain unchanged)
        let field_rr = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        assert_eq!(
            field_op_result_type("restrict", &[field_rr.clone(), Type::Geometry]),
            Some(field_rr),
            "restrict(Field<D,C>, _) must return the field type unchanged (PRD §5.1)"
        );

        // compose(Field<B=Real, C=Scalar<Temp>>, Field<A=Point3<Length>, B=Real>)
        //   → Field<A=Point3<Length>, C=Scalar<Temp>>
        let field_b_c = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),       // B
            codomain: Box::new(s_temp.clone()), // C
        };
        let field_a_b = Type::Field {
            domain: Box::new(p3l.clone()), // A
            codomain: Box::new(Type::dimensionless_scalar()), // B
        };
        assert_eq!(
            field_op_result_type("compose", &[field_b_c, field_a_b]),
            Some(Type::Field {
                domain: Box::new(p3l.clone()),      // A
                codomain: Box::new(s_temp.clone()), // C
            }),
            "compose(Field<B,C>, Field<A,B>) must produce Field<A,C> (PRD §5.1)"
        );

        // sample(Field<Real,Real>, Real) → Real   (THE §5.1 FIX: codomain, not Field)
        assert_eq!(
            field_op_result_type(
                "sample",
                &[
                    Type::Field {
                        domain: Box::new(Type::dimensionless_scalar()),
                        codomain: Box::new(Type::dimensionless_scalar()),
                    },
                    Type::dimensionless_scalar(),
                ]
            ),
            Some(Type::dimensionless_scalar()),
            "sample(Field<Real,Real>, Real) must produce Real (codomain), not Field (PRD §5.1 FIX)"
        );

        // sample(Field<Point3<Length>, Scalar<Temperature>>, Point3<Length>)
        //   → Scalar<Temperature>
        assert_eq!(
            field_op_result_type(
                "sample",
                &[
                    Type::Field {
                        domain: Box::new(p3l.clone()),
                        codomain: Box::new(s_temp.clone()),
                    },
                    p3l.clone(),
                ]
            ),
            Some(s_temp.clone()),
            "sample(Field<P3<L>,Sc<T>>, P3<L>) must produce Sc<T> (codomain) (PRD §5.1)"
        );

        // --- divergence/curl/laplacian: dimensional case pins cd/dd^exp quotient ---

        // divergence(Field<Point3<Length>, Vector3<Scalar<Temperature>>)
        //   → Field<Point3<Length>, Scalar<Temperature/Length>>
        // (dimensional quotient cd/dd^1 is asserted — not just dimensionless)
        let v3_temp = Type::vec3(s_temp.clone());
        let div_result_scalar = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH),
        };
        assert_eq!(
            field_op_result_type(
                "divergence",
                &[Type::Field {
                    domain: Box::new(p3l.clone()),
                    codomain: Box::new(v3_temp.clone()),
                }]
            ),
            Some(Type::Field {
                domain: Box::new(p3l.clone()),
                codomain: Box::new(div_result_scalar),
            }),
            "divergence(Field<P3<L>,V3<Temp>>) must produce Field<P3<L>,Sc<Temp/L>> (PRD §5.1)"
        );

        // curl(Field<Point3<Length>, Vector3<Scalar<Temperature>>)
        //   → Field<Point3<Length>, Vector3<Scalar<Temperature/Length>>>
        // (dimensional quotient cd/dd^1 per component; n==3 constraint exercised)
        let curl_component_scalar = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH),
        };
        assert_eq!(
            field_op_result_type(
                "curl",
                &[Type::Field {
                    domain: Box::new(p3l.clone()),
                    codomain: Box::new(v3_temp.clone()),
                }]
            ),
            Some(Type::Field {
                domain: Box::new(p3l.clone()),
                codomain: Box::new(Type::vec3(curl_component_scalar)),
            }),
            "curl(Field<P3<L>,V3<Temp>>) must produce Field<P3<L>,V3<Temp/L>> (PRD §5.1)"
        );

        // laplacian(Field<Point3<Length>, Scalar<Temperature>>)
        //   → Field<Point3<Length>, Scalar<Temperature/Length²>>
        // (domain_exponent=2 is asserted; laplacian-specific path)
        let lap_result_scalar = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE
                .div(&DimensionVector::LENGTH.pow(2)),
        };
        assert_eq!(
            field_op_result_type(
                "laplacian",
                &[Type::Field {
                    domain: Box::new(p3l.clone()),
                    codomain: Box::new(s_temp.clone()),
                }]
            ),
            Some(Type::Field {
                domain: Box::new(p3l.clone()),
                codomain: Box::new(lap_result_scalar),
            }),
            "laplacian(Field<P3<L>,Sc<Temp>>) must produce Field<P3<L>,Sc<Temp/L²>> (PRD §5.1)"
        );
    }

    /// Gradient codomain promotion + dimension quotient — §5.1 invariant.
    ///
    /// 1D case: gradient(Field<Real,Real>) → Field<Real,Real>
    ///   (dimensionless/dimensionless fallback → Real codomain; n=1 keeps scalar)
    ///
    /// nD case: gradient(Field<Point3<Length>, Scalar<Temperature>>)
    ///   → Field<Point3<Length>, Vector{n:3, quantity: Scalar<Temperature/Length>}>
    ///   (mirrors calculus.rs compute_gradient nD branch)
    ///
    /// RED until `field_op_result_type` is implemented in units.rs.
    #[test]
    fn field_op_result_type_gradient_is_codomain_correct() {
        use reify_core::{DimensionVector, Type};

        // 1D case: gradient(Field<Real,Real>) → Field<Real,Real>
        assert_eq!(
            field_op_result_type(
                "gradient",
                &[Type::Field {
                    domain: Box::new(Type::dimensionless_scalar()),
                    codomain: Box::new(Type::dimensionless_scalar()),
                }]
            ),
            Some(Type::Field {
                domain: Box::new(Type::dimensionless_scalar()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "gradient(Field<Real,Real>) must produce Field<Real,Real> (1D dimensionless case)"
        );

        // nD case: gradient(Field<Point3<Length>, Scalar<Temperature>>)
        //   → Field<Point3<Length>, Vector{n:3, Scalar<Temperature/Length>}>
        let p3l = Type::point3(Type::length());
        let gradient_qty = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH),
        };
        let expected = Type::Field {
            domain: Box::new(p3l.clone()),
            codomain: Box::new(Type::Vector {
                n: 3,
                quantity: Box::new(gradient_qty),
            }),
        };
        assert_eq!(
            field_op_result_type(
                "gradient",
                &[Type::Field {
                    domain: Box::new(p3l),
                    codomain: Box::new(Type::Scalar {
                        dimension: DimensionVector::TEMPERATURE,
                    }),
                }]
            ),
            Some(expected),
            "gradient(Field<Point3<Length>,Scalar<Temp>>) must produce \
             Field<Point3<Length>,Vector3<Temp/Length>> (PRD §5.1 gradient codomain)"
        );
    }

    /// Maintenance invariant — iterates `FIELD_OP_NAMES` *directly* (not a
    /// hand-maintained fixture) and asserts every entry returns `Some` from
    /// `field_op_result_type` when given well-shaped arguments.
    ///
    /// Without this test, a name added to `FIELD_OP_NAMES` without a parallel
    /// arm in `field_op_result_type` would silently return `None` and fall
    /// through to the first-arg fallback — exactly the mistyping the family
    /// was created to fix.  The `_ => None` wildcard makes the failure
    /// invisible to table-driven tests that iterate a hand-maintained fixture
    /// rather than the slice itself.
    ///
    /// Mirrors `geometry_query_names_each_have_a_result_type` (units.rs).
    #[test]
    fn field_op_names_each_have_a_result_type() {
        use reify_core::Type;

        // Well-shaped args for each name.  One canonical set is enough;
        // exhaustive shape coverage is the job of the table test.
        let field_rr = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        // divergence / curl require Point{n,scalar} domain + Vector{n,scalar} codomain
        let p3_real = Type::point3(Type::dimensionless_scalar());
        let v3_real = Type::vec3(Type::dimensionless_scalar());
        let field_p3_v3 = Type::Field {
            domain: Box::new(p3_real),
            codomain: Box::new(v3_real),
        };

        for name in FIELD_OP_NAMES {
            let args: Vec<Type> = match *name {
                "fn_field" => vec![Type::Function {
                    params: vec![Type::dimensionless_scalar()],
                    return_type: Box::new(Type::dimensionless_scalar()),
                }],
                "from_samples" => vec![
                    Type::List(Box::new(Type::dimensionless_scalar())),
                    Type::List(Box::new(Type::dimensionless_scalar())),
                    Type::dimensionless_scalar(),
                ],
                "compose" => vec![field_rr.clone(), field_rr.clone()],
                "divergence" | "curl" => vec![field_p3_v3.clone()],
                // restrict / sample / gradient / laplacian all accept Field<Real,Real>
                _ => vec![field_rr.clone()],
            };
            assert!(
                field_op_result_type(name, &args).is_some(),
                "FIELD_OP_NAMES entry {name:?} has no matching arm in \
                 field_op_result_type — adding a name to the slice \
                 REQUIRES a parallel arm in field_op_result_type"
            );
        }
    }

    /// When arg[0] is not a Field (for sample/gradient/restrict/compose) or not
    /// a Function (for fn_field), `field_op_result_type` must return `None` so
    /// the `expr.rs` caller falls through to its first-arg fallback unchanged —
    /// zero regression guarantee for mis-shaped call sites.
    ///
    /// RED until `field_op_result_type` is implemented in units.rs.
    #[test]
    fn field_op_result_type_returns_none_for_mismatched_arg_shapes() {
        use reify_core::Type;

        // sample: arg[0] must be Field
        assert_eq!(
            field_op_result_type("sample", &[Type::dimensionless_scalar(), Type::dimensionless_scalar()]),
            None,
            "sample with non-Field arg[0] must return None (falls through to first-arg fallback)"
        );

        // gradient: arg[0] must be Field
        assert_eq!(
            field_op_result_type("gradient", &[Type::dimensionless_scalar()]),
            None,
            "gradient with non-Field arg[0] must return None"
        );

        // restrict: arg[0] must be Field
        assert_eq!(
            field_op_result_type("restrict", &[Type::dimensionless_scalar(), Type::dimensionless_scalar()]),
            None,
            "restrict with non-Field arg[0] must return None"
        );

        // compose: arg[0] must be Field
        assert_eq!(
            field_op_result_type("compose", &[Type::dimensionless_scalar(), Type::dimensionless_scalar()]),
            None,
            "compose with non-Field args must return None"
        );

        // compose: middle type B must match — arg[0].domain must equal arg[1].codomain
        //
        // compose(Field<B=Real, C=Scalar<Temp>>, Field<A=Point3<Length>, B_actual=Bool>)
        //   middle mismatch: arg[0].domain (Real) != arg[1].codomain (Bool)
        //   → must return None (4219-S2 note: a mis-composed call must not be silently typed)
        //
        // RED on current main: the compose arm ignores middle B entirely and returns
        // Some(Field<Point3<Length>, Scalar<Temperature>>).
        // GREEN after step-4 tightens the arm to require b0 == b1.
        {
            use reify_core::DimensionVector;
            let s_temp = Type::Scalar {
                dimension: DimensionVector::TEMPERATURE,
            };
            let p3l = Type::point3(Type::length());
            // arg[0]: Field<B=Real, C=Scalar<Temp>> — codomain Temp, domain Real
            let arg0 = Type::Field {
                domain: Box::new(Type::dimensionless_scalar()), // B (Real)
                codomain: Box::new(s_temp),                     // C (Scalar<Temp>)
            };
            // arg[1]: Field<A=Point3<Length>, B_actual=Bool> — codomain Bool ≠ Real
            let arg1 = Type::Field {
                domain: Box::new(p3l),   // A (Point3<Length>)
                codomain: Box::new(Type::Bool), // B_actual (Bool ≠ Real → mismatch)
            };
            assert_eq!(
                field_op_result_type("compose", &[arg0, arg1]),
                None,
                "compose with mismatched middle type B (arg[0].domain=Real vs arg[1].codomain=Bool) \
                 must return None — 4219-S2: a mis-composed call must not be silently typed"
            );
        }

        // fn_field: arg[0] must be Function
        assert_eq!(
            field_op_result_type("fn_field", &[Type::dimensionless_scalar()]),
            None,
            "fn_field with non-Function arg[0] must return None"
        );

        // curl: n must be exactly 3; a 2-component domain/codomain must return None
        let p2_real = Type::Point {
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let v2_real = Type::Vector {
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        assert_eq!(
            field_op_result_type(
                "curl",
                &[Type::Field {
                    domain: Box::new(p2_real),
                    codomain: Box::new(v2_real),
                }]
            ),
            None,
            "curl with n=2 (not 3) must return None — n==3 constraint"
        );

        // divergence: codomain must be a Vector; scalar codomain must return None
        let p3_real = Type::point3(Type::dimensionless_scalar());
        let s_real = Type::Scalar {
            dimension: reify_core::DimensionVector::LENGTH,
        };
        assert_eq!(
            field_op_result_type(
                "divergence",
                &[Type::Field {
                    domain: Box::new(p3_real),
                    codomain: Box::new(s_real),
                }]
            ),
            None,
            "divergence with scalar codomain (not Vector) must return None"
        );

        // Empty arg list for any field op must return None (out-of-bounds guard)
        assert_eq!(
            field_op_result_type("sample", &[]),
            None,
            "sample with empty args must return None"
        );
        assert_eq!(
            field_op_result_type("fn_field", &[]),
            None,
            "fn_field with empty args must return None"
        );
    }

    // -----------------------------------------------------------------------
    // Task 4479 — ζ / C4: `max_deviation` compiler registration
    // -----------------------------------------------------------------------

    /// `is_geometry_query("max_deviation")` must return true once the ζ
    /// registration lands in step-6. Fails until then.
    ///
    /// RED until step-6 adds "max_deviation" to GEOMETRY_QUERY_NAMES.
    #[test]
    fn is_geometry_query_recognises_max_deviation() {
        assert!(
            is_geometry_query("max_deviation"),
            "is_geometry_query(\"max_deviation\") must be true after ζ step-6 registration"
        );
    }

    /// `geometry_query_result_type("max_deviation")` must return
    /// `Some(Type::Scalar { dimension: DimensionVector::LENGTH })` — mirroring
    /// the `distance` arm (2-arg Length-returning query, ζ / C4).
    ///
    /// RED until step-6 adds the arm.
    #[test]
    fn geometry_query_result_type_for_max_deviation_is_scalar_length() {
        use reify_core::{DimensionVector, Type};
        let expected = Type::Scalar {
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(
            geometry_query_result_type("max_deviation"),
            Some(expected),
            "geometry_query_result_type(\"max_deviation\") must return \
             Some(Scalar<LENGTH>), mirroring the `distance` arm (ζ / C4)"
        );
    }
}
