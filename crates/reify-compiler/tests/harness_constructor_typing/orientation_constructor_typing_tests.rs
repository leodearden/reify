//! Compiler typing tests for the orientation / transform / frame constructor
//! family (task 5344): every member must resolve its call-site cell type to
//! `Type::Orientation(3)` / `Type::Frame(3)` / `Type::Transform(3)` rather than
//! falling through `expr.rs`'s `NoUserFunctions` ladder to the first-arg
//! fallback, and the three zero-arg members must NOT trip the "cannot infer
//! return type of zero-arg function" warning.
//!
//! Structurally mirrors `tests/affine_constructor_typing_tests.rs`, the sibling
//! family's template. The family's membership rationale (why an explicit list
//! and never a prefix rule; why `frame_at` and the four decomposers are
//! excluded) is stated ONCE, on
//! `reify_compiler::orientation_signatures::ORIENTATION_TYPED_FN_NAMES` — the
//! docs here name only what each individual test pins.
//!
//! Concretely, the fallback mistyped these call sites:
//! - `orient_identity()` / `frame3_identity()` / `transform3_identity()` — zero
//!   args, so the fallback's `unwrap_or_else` fired: typed `Real`, plus one
//!   warning per call site (25 of them in `prj/printer_v01/printer.ri`);
//! - `orient_axis_angle(axis, angle)` — silently `Vector{3}`, adopted from the
//!   rotation-AXIS first argument;
//! - `transform3(orient, vec)` — silently whatever the orientation typed as;
//! - `frame3(point, orient)` — silently `Point`.
//!
//! RED until the `is_orientation_typed_fn` arm is wired into the ladder.

use reify_compiler::CompiledModule;
use reify_core::{Severity, Type};
use reify_test_support::{compile_source, compile_source_with_stdlib, get_let_expr_in};

/// `result_type` of value cell `cell_name` in template `template_name` of an
/// ALREADY-COMPILED module.
///
/// Takes `&CompiledModule` rather than a `&str` source so each test compiles its
/// fixture exactly ONCE and then looks up every cell against that one module —
/// the per-cell-recompile shape this file previously used cost O(cells) full
/// stdlib compiles for O(1) work, against a harness-consolidation PRD whose
/// whole point is cutting merge-gate compile cost. Traversal itself is
/// delegated to `reify_test_support::get_let_expr_in` (templates.find →
/// value_cells.find → default_expr), which is the shared utility for exactly
/// this lookup.
fn cell_type(module: &CompiledModule, template_name: &str, cell_name: &str) -> Type {
    get_let_expr_in(module, template_name, cell_name)
        .result_type
        .clone()
}

/// Every "cannot infer return type" diagnostic in `module`, in source order.
/// Empty is the contract for all of this family's call sites.
fn infer_warnings(module: &CompiledModule) -> Vec<&str> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("cannot infer return type"))
        .map(|d| d.message.as_str())
        .collect()
}

/// Dimensionless numeric args keep these sources independent of the stdlib unit
/// registry (`compile_source` has no `unit` decls in scope). The constructor
/// result types are arg-agnostic, so this does not weaken the assertions — for
/// `orient_axis_angle` the axis argument still types as
/// `Vector{3, dimensionless}`, which is exactly the wrong first-arg type the old
/// fallback adopted.
const ORIENT_HOST: &str = r#"
    structure OrientHost {
        let identity    = orient_identity()
        let quaternion  = orient_quaternion(1.0, 0.0, 0.0, 0.0)
        let euler       = orient_euler("xyz", 0.0, 0.0, 0.0)
        let basis       = orient_basis(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0))
        let look_at     = orient_look_at(vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0))
        let axis_angle  = orient_axis_angle(vec3(0.0, 0.0, 1.0), 90.0)
        let exp         = orient_exp(vec3(0.0, 0.0, 0.0))
        let inverse     = orient_inverse(orient_identity())
        let compose     = orient_compose(orient_identity(), orient_identity())
        let slerp       = orient_slerp(orient_identity(), orient_identity(), 0.5)
    }
"#;

/// All ten Orientation producers type as `Type::Orientation(3)`.
#[test]
fn orientation_constructors_type_as_orientation_3() {
    let module = compile_source(ORIENT_HOST);
    for cell in [
        "identity",
        "quaternion",
        "euler",
        "basis",
        "look_at",
        "axis_angle",
        "exp",
        "inverse",
        "compose",
        "slerp",
    ] {
        assert_eq!(
            cell_type(&module, "OrientHost", cell),
            Type::Orientation(3),
            "cell '{cell}' must type as Orientation(3)"
        );
    }
}

/// Both Frame producers type as `Type::Frame(3)`.
#[test]
fn frame_constructors_type_as_frame_3() {
    let source = r#"
        structure FrameHost {
            let f  = frame3(point3(0.0, 0.0, 0.0), orient_identity())
            let fi = frame3_identity()
        }
    "#;
    let module = compile_source(source);
    for cell in ["f", "fi"] {
        assert_eq!(
            cell_type(&module, "FrameHost", cell),
            Type::Frame(3),
            "cell '{cell}' must type as Frame(3)"
        );
    }
}

/// All six Transform producers type as `Type::Transform(3)` — explicitly
/// including `frame_to_frame`, whose `frame_` prefix is the trap documented on
/// `ORIENTATION_TYPED_FN_NAMES`.
#[test]
fn transform_constructors_type_as_transform_3() {
    let source = r#"
        structure TransformHost {
            let t        = transform3(orient_identity(), vec3(0.0, 0.0, 0.0))
            let ti       = transform3_identity()
            let compose  = transform_compose(transform3_identity(), transform3_identity())
            let inverse  = transform_inverse(transform3_identity())
            let exp      = transform_exp(transform_log(transform3_identity()))
            let f2f      = frame_to_frame(frame3_identity(), frame3_identity())
        }
    "#;
    let module = compile_source(source);
    for cell in ["t", "ti", "compose", "inverse", "exp", "f2f"] {
        assert_eq!(
            cell_type(&module, "TransformHost", cell),
            Type::Transform(3),
            "cell '{cell}' must type as Transform(3)"
        );
    }
    // Guard the prefix trap specifically: frame_to_frame must NOT be grouped
    // with the Frame producers.
    assert_ne!(
        cell_type(&module, "TransformHost", "f2f"),
        Type::Frame(3),
        "frame_to_frame must type as Transform(3), not Frame(3)"
    );
}

/// The task's named acceptance test, at the call-site level: the rotation AXIS
/// is the first argument and types as `Vector{3}`, so the old first-arg fallback
/// silently gave the whole call that Vector type.
#[test]
fn orient_axis_angle_does_not_type_as_vector() {
    let module = compile_source(ORIENT_HOST);
    let ty = cell_type(&module, "OrientHost", "axis_angle");
    assert_eq!(
        ty,
        Type::Orientation(3),
        "orient_axis_angle(axis, angle) must type as Orientation(3)"
    );
    assert!(
        !matches!(ty, Type::Vector { .. }),
        "orient_axis_angle must NOT adopt the first-arg (axis) Vector type, got {ty:?}"
    );
}

/// The three zero-arg members are the only names in the family that can trip the
/// zero-arg warning — it is emitted solely from the first-arg fallback's
/// `unwrap_or_else` branch, reached only when `compiled_args.first()` is `None`.
#[test]
fn zero_arg_orientation_constructors_emit_no_infer_warning() {
    let source = r#"
        structure ZeroArgHost {
            let o = orient_identity()
            let f = frame3_identity()
            let t = transform3_identity()
        }
    "#;
    let module = compile_source(source);
    let warnings = infer_warnings(&module);
    assert!(
        warnings.is_empty(),
        "zero-arg orientation constructors must not emit the infer warning, got: {warnings:#?}"
    );
}

/// The `prj/printer_v01/printer.ri` shape: a zero-arg `orient_identity()` nested
/// inside `transform3`. Under the old fallback the inner call typed `Real`, and
/// the outer `transform3` then adopted that `Real` through the first-arg chain —
/// so this pins that the fix composes through nesting, not just at leaf sites.
#[test]
fn nested_transform3_over_orient_identity_types_as_transform_3() {
    let source = r#"
        structure NestedHost {
            let t = transform3(orient_identity(), vec3(0.0, 0.0, 0.0))
        }
    "#;
    let module = compile_source(source);
    let ty = cell_type(&module, "NestedHost", "t");
    assert_eq!(
        ty,
        Type::Transform(3),
        "transform3(orient_identity(), vec3(..)) must type as Transform(3)"
    );
    assert!(
        !matches!(ty, Type::Scalar { .. }),
        "nested transform3 must not inherit the inner call's fallback Real type, got {ty:?}"
    );
}

/// The real `prj/printer_v01/printer.ri`, resolved from this crate's manifest
/// dir. Mirrors the `DEV_CAPSTAN` constant in
/// `crates/reify-eval/tests/harness_sweep/capstan_groove_e2e.rs`, the existing
/// precedent for gating on a REAL design file rather than a reconstruction.
const PRINTER_RI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../prj/printer_v01/printer.ri"
);

/// The task's headline acceptance criterion, pinned against the ACTUAL file it
/// was measured on: compiling `prj/printer_v01/printer.ri` emits ZERO "cannot
/// infer return type" diagnostics (it emitted 25 before this family existed).
///
/// The synthetic `printer_shaped_source_emits_zero_infer_warnings` below stays
/// as the focused per-shape lock — it pins WHICH constructor shapes resolve and
/// to what. This test is the criterion itself, and unlike the synthetic proxy it
/// cannot silently unlock when printer.ri grows a new zero-arg builtin call.
///
/// Failure here does NOT necessarily mean this family regressed: a newly-added
/// unclaimed zero-arg builtin anywhere in printer.ri fails it too. That is the
/// point — the criterion is a property of the file, not of one name list — so
/// the assertion prints every offending diagnostic.
#[test]
fn real_printer_ri_emits_zero_infer_warnings() {
    let source = std::fs::read_to_string(PRINTER_RI)
        .unwrap_or_else(|e| panic!("cannot read {PRINTER_RI}: {e}"));
    let module = compile_source_with_stdlib(&source);
    let warnings = infer_warnings(&module);
    assert!(
        warnings.is_empty(),
        "compiling the real prj/printer_v01/printer.ri must emit ZERO 'cannot \
         infer return type' warnings (was 25); got {}: {warnings:#?}",
        warnings.len()
    );
}

/// The per-shape lock behind the headline criterion, over a reconstruction of
/// printer.ri's usage shapes — repeated `orient_identity()` bindings,
/// `orient_axis_angle(axis, angle)`, `transform3(orient_identity(), vec3(..))`,
/// `frame3(point3(..), orient_identity())`, and a nested
/// `transform_compose(transform3(..), transform3(..))`. It asserts BOTH:
///
/// (a) ZERO "cannot infer return type" diagnostics for these specific shapes —
///     that warning is emitted solely from the first-arg fallback's
///     `unwrap_or_else` branch;
/// (b) the nested cells carry the right type END-TO-END, proving the fix
///     composes through nesting rather than only at leaf call sites — which the
///     real-file test above cannot show, since it only counts diagnostics.
///
/// Uses `compile_source_with_stdlib` so the dimensioned literals (`1m`, `90deg`,
/// `0mm`) printer.ri actually passes resolve — the arguments are the real thing,
/// not dimensionless stand-ins.
#[test]
fn printer_shaped_source_emits_zero_infer_warnings() {
    let source = r#"
        structure PrinterShaped {
            let rot_to_y = orient_axis_angle(vec3(1m, 0mm, 0mm), 90deg)
            let a_upper = transform3(rot_to_y, vec3(0mm, 0mm, 10mm))
            let a_lower = transform3(orient_identity(), vec3(0mm, 0mm, 20mm))
            let b_upper = transform3(orient_identity(), vec3(0mm, 0mm, 30mm))
            let b_lower = transform3(orient_identity(), vec3(0mm, 0mm, 40mm))
            let stacked = transform_compose(
                transform3(orient_identity(), vec3(0mm, 0mm, 50mm)),
                transform3(orient_identity(), vec3(0mm, 0mm, 60mm))
            )
            let mount = frame3(point3(0mm, 0mm, 0mm), orient_identity())
        }
    "#;
    let module = compile_source_with_stdlib(source);

    // (a) no call site in these shapes reaches the zero-arg fallback.
    let warnings = infer_warnings(&module);
    assert!(
        warnings.is_empty(),
        "printer-shaped source must emit ZERO 'cannot infer return type' \
         warnings (was 25 across prj/printer_v01/printer.ri); got: {warnings:#?}"
    );

    // (b) the types compose through nesting, not just at leaf call sites.
    for cell in ["a_upper", "a_lower", "b_upper", "b_lower", "stacked"] {
        assert_eq!(
            cell_type(&module, "PrinterShaped", cell),
            Type::Transform(3),
            "cell '{cell}' must type as Transform(3) end-to-end"
        );
    }
    assert_eq!(
        cell_type(&module, "PrinterShaped", "rot_to_y"),
        Type::Orientation(3),
        "orient_axis_angle(axis, angle) must type as Orientation(3), not the \
         axis argument's Vector type"
    );
    assert_eq!(
        cell_type(&module, "PrinterShaped", "mount"),
        Type::Frame(3),
        "frame3(point3(..), orient_identity()) must type as Frame(3), not the \
         point argument's type"
    );
}

// ── Datum-neighbour audit (the same defect, one resolver over) ────────────────
//
// `plane_xy`/`plane_xz`/`plane_yz` and `axis_x`/`axis_y`/`axis_z` share the
// orientation family's first-arg-fallback defect but belong to the DATUM
// vocabulary, so they are claimed by `units.rs::datum_constructor_result_type`
// (alongside `midplane`/`plane_through`/`axis_through`/`frame_at`) rather than
// by `ORIENTATION_TYPED_FN_NAMES` — the two families stay cleanly disjoint.
//
// These six take exactly ONE argument, not zero: `reify_stdlib::geometry`'s
// `make_plane` (:1112) and `make_axis` (:1152) both hard-check `args.len() != 1`.
// So unlike the three zero-arg orientation constructors they mistype SILENTLY —
// they never trip the "cannot infer return type" warning, which is precisely why
// the defect went unnoticed here. Do NOT take the LSP completion table
// (`crates/reify-lsp/src/completion.rs`) as the source of truth: it declares
// `plane_xy() -> Frame` and `axis_x() -> Vector`, wrong in BOTH arity and return
// type. The evaluator is the source of truth.
//
// These use `compile_source_with_stdlib` so dimensioned literals (`10mm`)
// resolve — the wrong types the fallback adopted (`Scalar<Length>` /
// `Point3<Length>`) are only reproducible with real dimensioned arguments.

/// `plane_xy(10mm)` types as `Type::Plane`, NOT the `Scalar<Length>` of its
/// offset argument.
#[test]
fn axis_aligned_plane_constructors_type_as_plane() {
    let source = r#"
        structure PlaneHost {
            let pxy = plane_xy(10mm)
            let pxz = plane_xz(0mm)
            let pyz = plane_yz(5mm)
        }
    "#;
    let module = compile_source_with_stdlib(source);
    for cell in ["pxy", "pxz", "pyz"] {
        let ty = cell_type(&module, "PlaneHost", cell);
        assert_eq!(
            ty,
            Type::Plane,
            "cell '{cell}' must type as Plane, not the offset argument's type"
        );
        assert!(
            !matches!(ty, Type::Scalar { .. }),
            "cell '{cell}' must not adopt the offset argument's Scalar<Length> \
             (the old first-arg fallback), got {ty:?}"
        );
    }
}

/// `axis_x(point3(0mm, 0mm, 0mm))` types as `Type::Axis`, NOT a type derived
/// from its origin argument.
///
/// Measured pre-fix type: `Scalar<Length>`, not `Point3<Length>` — the fallback
/// chained, because `point3` was itself unclaimed and adopted `0mm`'s
/// `Scalar<Length>` first. (`point3`/`vec3` are now claimed by
/// `math_signatures::MATH_CONSTRUCTION_NAMES`, so the inner call types as a real
/// `Point3<Length>`; that is the esc-5344-4 grant, and it does not change what
/// `axis_x` itself must resolve to.) The assertion below is therefore written
/// against `Type::Axis` plus a negative on `Type::Point`, both of which hold
/// regardless of which link in that chain the wrong type came from.
#[test]
fn axis_aligned_axis_constructors_type_as_axis() {
    let source = r#"
        structure AxisHost {
            let ax = axis_x(point3(0mm, 0mm, 0mm))
            let ay = axis_y(point3(0mm, 0mm, 0mm))
            let az = axis_z(point3(0mm, 0mm, 0mm))
        }
    "#;
    let module = compile_source_with_stdlib(source);
    for cell in ["ax", "ay", "az"] {
        let ty = cell_type(&module, "AxisHost", cell);
        assert_eq!(
            ty,
            Type::Axis,
            "cell '{cell}' must type as Axis, not the origin argument's type"
        );
        assert!(
            !matches!(ty, Type::Point { .. } | Type::Scalar { .. }),
            "cell '{cell}' must not inherit an argument type through the old \
             first-arg fallback chain, got {ty:?}"
        );
    }
}

// ── The four DECOMPOSERS (task 6306) ─────────────────────────────────────────
//
// The sibling half of the family above: the same two eval dispatchers, reached
// by the same name-based dispatch, but returning the COORDINATES of a rigid
// motion (`Value::Vector` / `List` / `Map`) rather than the motion itself. They
// are claimed by `ORIENTATION_DECOMPOSER_FN_NAMES` / the
// `is_orientation_decomposer_fn` ladder arm; membership and every result-type
// ruling live ONCE there, so the docs here name only what each test pins.
//
// Their defect is SHARPER than the producers'. All four take at least one
// argument, so the first-arg fallback never reaches its zero-arg "cannot infer
// return type" warning — the mistyping is entirely SILENT. And task 5344 made it
// worse rather than better for three of them: now that `orient_identity()` types
// as a real `Orientation(3)`, `orient_log(q)` inherits `Orientation(3)` where it
// once inherited the fallback's `Real`.

/// Dimensionless literals keep this source independent of the stdlib unit
/// registry, so it can use `compile_source` — the same reason `ORIENT_HOST`
/// above does. All four result types are argument-independent, so this weakens
/// nothing.
///
/// TASK 6082 NOTE: it flips `orient_to_euler` to subject-first
/// `orient_to_euler(q, convention)`. When that lands, ONLY the fixture line
/// below changes — every assertion in this section survives verbatim, because
/// the result type never depended on the argument order (that independence is
/// exactly what these tests pin).
const DECOMPOSER_HOST: &str = r#"
    structure DecomposerHost {
        let q     = orient_identity()
        let log   = orient_log(q)
        let euler = orient_to_euler("xyz", q)
        let aa    = orient_to_axis_angle(q)
        let twist = transform_log(transform3_identity())
    }
"#;

/// All four decomposer call sites resolve to their documented result types.
#[test]
fn decomposer_call_sites_type_as_their_result_types() {
    let module = compile_source(DECOMPOSER_HOST);
    assert_eq!(
        cell_type(&module, "DecomposerHost", "log"),
        Type::vec3(Type::dimensionless_scalar()),
        "orient_log(q) is the rotation vector — three untagged Value::Real \
         components, hence vec3<DIMENSIONLESS>"
    );
    assert_eq!(
        cell_type(&module, "DecomposerHost", "euler"),
        Type::List(Box::new(Type::angle())),
        "orient_to_euler(..) returns three Angles as a Value::List"
    );
    assert_eq!(
        cell_type(&module, "DecomposerHost", "aa"),
        Type::StructureRef("AxisAngle".to_string()),
        "orient_to_axis_angle(q) returns Value::Map{{angle, axis}}"
    );
    assert_eq!(
        cell_type(&module, "DecomposerHost", "twist"),
        Type::StructureRef("Twist".to_string()),
        "transform_log(t) returns the Value::Map{{angular, linear}} twist"
    );
}

/// `orient_log(q)` must not adopt its argument's `Orientation(3)`.
///
/// This is the negative that task 5344 SHARPENED: before it, `orient_identity()`
/// itself typed `Real`, so `orient_log(q)` inherited `Real`; after it, `q` is a
/// genuine `Orientation(3)` and the fallback hands that straight through. Either
/// way the call site never carried the vector it actually evaluates to.
///
/// The second negative mirrors the unit-level dimension pin: the components are
/// untagged `Value::Real`, and `TWIST_ANGULAR_DIM` (`geometry.rs:216`) already
/// rules this same ω quantity DIMENSIONLESS on a live eval gate.
#[test]
fn orient_log_call_site_is_not_its_first_arg_orientation() {
    let module = compile_source(DECOMPOSER_HOST);
    let ty = cell_type(&module, "DecomposerHost", "log");
    assert_ne!(
        ty,
        Type::Orientation(3),
        "orient_log DECOMPOSES an Orientation — it must not adopt its \
         argument's Orientation(3) type via the first-arg fallback"
    );
    assert_ne!(
        ty,
        Type::vec3(Type::angle()),
        "orient_log's components are untagged Value::Real; typing them Angle \
         would contradict the live TWIST_ANGULAR_DIM = DIMENSIONLESS gate"
    );
}

/// The two `Value::Map` call sites must not adopt their first-argument types.
///
/// Both are 1-arg, so today's mistyping is SILENT — no diagnostic marks it.
#[test]
fn map_decomposer_call_sites_are_not_their_first_arg_types() {
    let module = compile_source(DECOMPOSER_HOST);
    assert_ne!(
        cell_type(&module, "DecomposerHost", "aa"),
        Type::Orientation(3),
        "orient_to_axis_angle(q) must not adopt q's Orientation(3)"
    );
    assert_ne!(
        cell_type(&module, "DecomposerHost", "twist"),
        Type::Transform(3),
        "transform_log(t) must not adopt t's Transform(3)"
    );
}

/// The new arm is a pure improvement, not a new diagnostic source.
///
/// None of the four could trip the zero-arg warning even before this change
/// (they all take arguments), so `infer_warnings` was already empty here. This
/// pins that claiming them did not perturb diagnostics in either direction.
#[test]
fn decomposer_call_sites_emit_no_infer_warning() {
    let module = compile_source(DECOMPOSER_HOST);
    let warnings = infer_warnings(&module);
    assert!(
        warnings.is_empty(),
        "decomposer call sites must emit ZERO 'cannot infer return type' \
         warnings, got: {warnings:#?}"
    );
}

/// NO-REGRESSION LOCK: typing `orient_to_axis_angle` as
/// `StructureRef("AxisAngle")` must not make a member access on its result a
/// HARD `UnknownMember` error.
///
/// Unlike its neighbours this is not a RED→GREEN test — it is green before AND
/// after the ladder arm is wired, and its whole job is to STAY green.
///
/// MEASURED BASELINE (recorded because it contradicts the assumption this lock
/// was designed from). `orient_to_axis_angle(q).angle` is NOT diagnostic-free
/// today: with the receiver typed `Orientation(3)` the access falls through
/// `expr.rs`'s per-kind member handlers to the residual tail
/// `member_access_aggregation_or_unsupported`, which emits the GENERIC
/// `Severity::Error` "member access not yet supported: .angle" (expr.rs:6617).
/// That diagnostic is a receiver-type-independent limitation of member-access
/// support — it predates this task, this task neither introduces nor removes it,
/// and this test deliberately does NOT forbid it.
///
/// What this test DOES forbid is the one failure mode design decision 4 exists
/// to prevent, identified by its distinct message. `member_path::resolve_hop` is
/// asymmetric:
/// - a `StructureRef(name)` ABSENT from `template_registry` → SOFT
///   `Indeterminate(TemplateNotInRegistry)`, whose `to_diagnostic` returns
///   `None`;
/// - a REGISTERED structure MISSING the field → HARD `UnknownMember`, rendered
///   as `structure '<name>' has no member '<member>'`
///   (member_path.rs:622-628).
///
/// `AxisAngle` is deliberately left UNDECLARED (see
/// `orientation_decomposer_result_type`'s doc), so the hop stays on the soft
/// branch. This test fails loudly if anyone later "helpfully" adds an empty
/// `structure def AxisAngle { }` marker — the natural-looking move, since
/// `Twist`/`BodyId`/`JointBinding` are exactly that — which would flip
/// `tests/prd-gate/fixtures/orient_axis_angle_member_parses.ri:12` into that
/// hard error. Declaring it WITH its `{angle, axis}` fields is the PRD's
/// τ2-orientation post-state and is the only correct way to make it appear.
///
/// Uses `compile_source_with_stdlib` so a template registry is genuinely
/// present: with no registry the hop would be soft for the unrelated
/// `NoTemplateRegistry` reason and the test would pass vacuously.
#[test]
fn orient_to_axis_angle_member_access_still_compiles() {
    let source = r#"
        structure AxisAngleMemberHost {
            let q = orient_identity()
            let ang = orient_to_axis_angle(q).angle
        }
    "#;
    let module = compile_source_with_stdlib(source);
    let unknown_member: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("has no member"))
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        unknown_member.is_empty(),
        "`orient_to_axis_angle(q).angle` must stay a SOFT \
         Indeterminate(TemplateNotInRegistry) hop, never a hard UnknownMember. \
         A \"structure 'AxisAngle' has no member 'angle'\" here means someone \
         declared an EMPTY `structure def AxisAngle {{ }}` — declare it WITH its \
         {{angle, axis}} fields or not at all, or \
         tests/prd-gate/fixtures/orient_axis_angle_member_parses.ri:12 regresses. \
         Got: {unknown_member:#?}"
    );
}
