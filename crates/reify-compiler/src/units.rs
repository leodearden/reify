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
    "thicken",
    "draft",
    "chamfer",
    "fillet",
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
    "line_segment",
    "arc",
    "helix",
    "interp",
    "bezier",
    "nurbs",
    "tube",
    "pipe",
    "box_centered",
    "cylinder_centered",
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
/// fn center_of_mass(solid: Solid, density: Real) -> Point3<Length>
/// fn moment_of_inertia(solid: Solid, density: Real) -> Tensor<2, 3, MomentOfInertia>
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
/// - `edges(solid)`                          → `Type::List(Geometry)`
/// - `faces(solid)`                          → `Type::List(Geometry)`
/// - `edges_by_length(solid, range)`         → `Type::List(Geometry)`
/// - `faces_by_area(solid, range)`           → `Type::List(Geometry)`
/// - `faces_by_normal(solid, dir, tol)`      → `Type::List(Geometry)`
/// - `edges_parallel_to(solid, dir, tol)`    → `Type::List(Geometry)`
/// - `edges_at_height(solid, h, tol)`        → `Type::List(Geometry)`
/// - `adjacent_faces(solid, face)`           → `Type::List(Geometry)`
/// - `shared_edges(face1, face2)`            → `Type::List(Geometry)`
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
        // Task 2699 — compile-time type wiring; eval dispatch is task 2691
        "edges" | "faces" | "edges_by_length" | "faces_by_area" | "faces_by_normal"
        | "edges_parallel_to" | "edges_at_height" | "adjacent_faces" | "shared_edges" => {
            Type::List(Box::new(Type::Geometry))
        }
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
                Some(reify_core::Type::Real)
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
/// **`curvature` overload note**: only the `Curve`→`Scalar<Curvature>`
/// overload is registered in Phase 1. The `Surface`→`Matrix<2,2,
/// Curvature>` overload requires arg-type-aware dispatch and is deferred
/// to a later phase.
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
/// - `curvature(curve, t)`   → `Scalar<Curvature>` (Curve overload only;
///   Surface overload deferred — see [`GEOMETRY_QUERY_NAMES`])
///
/// KGQ-ζ Phase 6 addition (task 3615):
/// - `normal(surface, point)` → `Vector3<Dimensionless>` (`Type::vec3(Type::Real)`)
///   The quantity is `Type::Real` (dimensionless), NOT a `Scalar` dimension,
///   matching the `Value::Vector(vec![Value::Real(_); 3])` shape that
///   `dispatch_normal_vector3` constructs and that `Value.infer_type()` maps
///   back to `Type::Vector { n: 3, quantity: Box::new(Type::Real) }`.
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
        // Type::Real (not a Scalar dimension) is the quantity so that the
        // dispatched Value::Vector(vec![Value::Real(_);3]).infer_type() == this.
        "normal" => Type::vec3(Type::Real),
        _ => return None,
    })
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
    fn compile_geometry_draft_recognized() {
        assert!(is_geometry_function("draft"));
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
            (
                "edges",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "faces",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "edges_by_length",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "faces_by_area",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "faces_by_normal",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "edges_parallel_to",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
            (
                "edges_at_height",
                reify_core::Type::List(Box::new(reify_core::Type::Geometry)),
            ),
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
    /// `Some(Type::vec3(Type::Real))` — a dimensionless 3D vector, i.e.
    /// `Type::Vector { n: 3, quantity: Box::new(Type::Real) }`.
    ///
    /// This is the exact type that `Value::Vector(vec![Value::Real(_); 3]).infer_type()`
    /// produces (verified: value.rs `try_infer_type` for Vector sets quantity =
    /// first component's `try_infer_type()`, and `Value::Real → Type::Real`).
    #[test]
    fn geometry_query_result_type_for_normal_is_vec3_real() {
        use reify_core::Type;
        assert_eq!(
            geometry_query_result_type("normal"),
            Some(Type::vec3(Type::Real)),
            "geometry_query_result_type(\"normal\") must be Some(Type::vec3(Type::Real)) \
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
            Some(reify_core::Type::Real)
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
            affine_map_algebra_result_type("determinant", Some(&reify_core::Type::Real)),
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
}
