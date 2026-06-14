//! Compiler signatures for the geometric-relation vocabulary (geometric-
//! relations γ, task 4383) — the §7.3/§9 relation contract.
//!
//! Holds the single source of truth for the **pure** relation builtin name
//! family ([`RELATION_FN_NAMES`]), the name-only classification predicate
//! ([`is_relation_typed_fn`]), the arg-aware name→`Type::Relation` resolver
//! ([`relation_fn_result_type`]), the ΔDOF (degree-of-freedom-removal)
//! inference ([`relation_delta_dof`]), and the hover/contract surfacing
//! ([`relation_contract_string`]).
//!
//! Mirrors the established name-keyed signature-family modules
//! (`joint_signatures.rs`, `math_signatures.rs`): a `NAMES` slice as the single
//! source of truth, an `is_*_typed_fn` predicate, a `*_result_type` resolver,
//! and an in-module test suite with an independent `EXPECTED_NAMES` fixture.
//!
//! ## Relations are directives, not truths
//!
//! A relation type-checks to `Type::Relation` (a DOF-removal directive — no
//! truth value, distinct from `Bool`) but evaluates to `Value::Undef` until ζ
//! supplies the relate-solve (the geometry-query Phase-1 precedent). γ provides
//! the type + vocabulary; the `relate`-block `Relation`-vs-`Bool` enforcement is
//! δ's job, and the relate-solve / `ApplyTransform` placement is ζ's.
//!
//! ## `angle` / `distance` are arity-gated shared verbs
//!
//! `angle` and `distance` are NOT in [`RELATION_FN_NAMES`] — `units.rs`
//! `GEOMETRY_QUERY_NAMES` already owns their arity-2 DERIVE forms
//! (`angle`→Angle, `distance`→Scalar<Length>). Their arity-3 DRIVE forms
//! (`angle(a, b, θ)` / `distance(a, b, δ)`) are relations: [`relation_fn_result_type`]
//! claims them only at arity 3 and returns `None` at arity 2 so the call falls
//! through to the geometry-query arm. The pure family stays disjoint from every
//! sibling family (pinned by the `units.rs` disjointness test).

use crate::datum_projection::DatumProjectionResolution;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, SourceSpan, Type};
use reify_ir::{CompiledExpr, CompiledExprKind};

/// The complete set of **pure** geometric-relation builtin names recognised by
/// the compiler. Single source of truth — imported into the `units.rs` test
/// module to pin disjointness from all sibling families (mirrors
/// `JOINT_TYPED_FN_NAMES` / `MATH_CONSTRUCTION_NAMES`).
///
/// **9 names**:
/// - **Primitives** (5): `coincident` (datum-coincidence), `on` (incidence),
///   `parallel`, `antiparallel`, `perpendicular` (orientation).
/// - **Named compounds** (4): `concentric`, `flush`, `offset`, `tangent`.
///
/// `angle` and `distance` are deliberately EXCLUDED — they are arity-gated
/// shared verbs whose arity-2 forms belong to `units.rs` `GEOMETRY_QUERY_NAMES`;
/// only their arity-3 DRIVE forms are relations (handled in
/// [`relation_fn_result_type`] / [`relation_delta_dof`]).
///
/// Case-sensitive: Reify function names are snake_case.
pub const RELATION_FN_NAMES: &[&str] = &[
    // Primitives (5).
    "coincident",
    "on",
    "parallel",
    "antiparallel",
    "perpendicular",
    // Named compounds (4).
    "concentric",
    "flush",
    "offset",
    "tangent",
];

/// Is `name` a **pure** relation builtin? Name-only classification — a
/// `.contains` over the single-source-of-truth slice [`RELATION_FN_NAMES`].
/// Case-sensitive. Excludes the arity-gated shared verbs `angle`/`distance`.
///
/// `pub` so reify-lsp's hover provider can gate its relation-contract branch on
/// the relation vocabulary without re-deriving the name family.
pub fn is_relation_typed_fn(name: &str) -> bool {
    RELATION_FN_NAMES.contains(&name)
}

/// Arg-aware result-type resolver for the relation vocabulary. Returns
/// `Some(Type::Relation)` for every pure relation name (regardless of operand
/// shape) and for the arity-3 DRIVE forms of `angle`/`distance`; `None`
/// otherwise — including the arity-2 `angle`/`distance` DERIVE forms, which then
/// fall through to the geometry-query arm in `expr.rs` (mirrors the arg-aware
/// `selector_composition_result_type` fall-through idiom).
pub(crate) fn relation_fn_result_type(name: &str, args: &[CompiledExpr]) -> Option<Type> {
    if RELATION_FN_NAMES.contains(&name) {
        return Some(Type::Relation);
    }
    // Shared-verb DRIVE forms: arity-3 `angle`/`distance` are relations; the
    // arity-2 DERIVE forms fall through (None) to geometry-query.
    if matches!(name, "angle" | "distance") && args.len() == 3 {
        return Some(Type::Relation);
    }
    None
}

/// The ΔDOF (degrees of freedom removed) a relation publishes — the exact
/// codimension of its constraint manifold, NOT a tolerance (design §3.1/§3.4;
/// PRD §12 confirms the G6 numeric-floor branch does not fire). Returns `None`
/// for names/operand shapes outside the curated vocabulary.
///
/// The integers are first-principles codimension counts:
/// - `coincident(D, D)` removes `codim(D)`: a `Direction` pins 2 angular DOF; a
///   `Point` pins 3 translational DOF; a `Plane` pins 1 translation + 2 tilt =
///   3; an `Axis` pins 2 translation + 2 tilt = 4; a `Frame` pins all 6.
/// - `on(Point, host)` removes `3 − dim(host)` (the point keeps `dim(host)`
///   freedoms sliding within the host): `Plane`(dim 2)→1, `Axis`(dim 1)→2,
///   `Point`(dim 0)→3.
/// - Metric primitives `angle`/`distance` (arity-3 DRIVE form) each pin 1 scalar.
/// - Orientation primitives: `parallel`/`antiparallel` pin 2 angular DOF;
///   `perpendicular` pins 1.
/// - Named compounds publish their summed-body nominal codim: `concentric` = a
///   coincident axis (4); `flush` = a coincident plane (3); `offset` = parallel
///   (2) + on (1) = 3; `tangent` = 2.
pub(crate) fn relation_delta_dof(name: &str, args: &[CompiledExpr]) -> Option<u32> {
    let arg_ty = |i: usize| args.get(i).map(|a: &CompiledExpr| &a.result_type);
    match name {
        // coincident(D, D): codim of the datum kind D.
        "coincident" => match arg_ty(0)? {
            Type::Direction => Some(2),
            Type::Point { .. } => Some(3),
            Type::Plane => Some(3),
            Type::Axis => Some(4),
            Type::Frame(_) => Some(6),
            _ => None,
        },
        // on(Point, host) = 3 − dim(host).
        "on" => match arg_ty(1)? {
            Type::Plane => Some(1),
            Type::Axis => Some(2),
            Type::Point { .. } => Some(3),
            _ => None,
        },
        // Metric primitives — only the arity-3 DRIVE form removes a DOF.
        "angle" | "distance" => {
            if args.len() == 3 {
                Some(1)
            } else {
                None
            }
        }
        // Orientation primitives.
        "parallel" | "antiparallel" => Some(2),
        "perpendicular" => Some(1),
        // Named compounds (nominal summed-body codim).
        "concentric" => Some(4),
        "flush" => Some(3),
        "offset" => Some(3),
        "tangent" => Some(2),
        _ => None,
    }
}

/// The ΔDOF contract string surfaced by `reify-lsp` hover:
/// `name(ArgTys) -> Relation removes N`. The metric operand is rendered by its
/// dimension name (`Length`/`Angle`), not `Scalar[m]`, to match the §4 signature
/// vocabulary. If the ΔDOF is unknown (uncurated operand shape) the count is
/// rendered as `?`.
pub(crate) fn relation_contract_string(name: &str, args: &[CompiledExpr]) -> String {
    let arg_tys: Vec<String> = args
        .iter()
        .map(|a| format_relation_arg_ty(&a.result_type))
        .collect();
    let removes = relation_delta_dof(name, args)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    format!(
        "{}({}) -> Relation removes {}",
        name,
        arg_tys.join(","),
        removes
    )
}

/// Render an operand type for the contract string: a metric scalar by its
/// dimension name (`Length`/`Angle`), datum kinds by their short name
/// (`Point`/`Frame` collapse the dimensional suffix; `Plane`/`Axis`/`Direction`
/// already Display short).
fn format_relation_arg_ty(ty: &Type) -> String {
    match ty {
        Type::Scalar { dimension } => dimension.canonical_name().unwrap_or("Real").to_string(),
        Type::Point { .. } => "Point".to_string(),
        Type::Frame(_) => "Frame".to_string(),
        other => format!("{}", other),
    }
}

/// Find a relation call named `name` in `module` and return its ΔDOF contract
/// string (`name(ArgTys) -> Relation removes N`), or `None` if no such call is
/// present. This is the compiler-side traversal backing reify-lsp's hover: it
/// keeps `CompiledExprKind` matching inside reify-compiler so the LSP crate need
/// not depend on the IR's expression internals.
///
/// `enclosing_decl` scopes the search to a single template (the structure /
/// occurrence the hover cursor sits in); `None` searches every template. Each
/// candidate cell's compiled `default_expr` is walked in pre-order; the first
/// `FunctionCall` whose `function.name` is `name` AND which `relation_fn_result_type`
/// confirms is a relation (so the arity-2 `angle`/`distance` DERIVE forms are
/// excluded) supplies the operand `result_type`s the contract is rendered from.
///
/// For the single-call hover snippets this name+scope match is unambiguous;
/// span-level disambiguation of multiple same-name calls is a noted refinement,
/// not required for the user-observable signal.
pub fn relation_contract_for_call(
    module: &crate::CompiledModule,
    name: &str,
    enclosing_decl: Option<&str>,
) -> Option<String> {
    for template in &module.templates {
        if let Some(decl) = enclosing_decl
            && template.name != decl
        {
            continue;
        }
        // Top-level value cells plus guarded-group members (where/else), mirroring
        // the cell traversal in reify-lsp's `AnalysisContext::find_member_decl`.
        let guarded = template
            .guarded_groups
            .iter()
            .flat_map(|g| g.members.iter().chain(g.else_members.iter()));
        for vc in template.value_cells.iter().chain(guarded) {
            let Some(expr) = &vc.default_expr else {
                continue;
            };
            let mut found: Option<String> = None;
            expr.walk(&mut |node| {
                if found.is_some() {
                    return;
                }
                if let CompiledExprKind::FunctionCall { function, args } = &node.kind
                    && function.name == name
                    && relation_fn_result_type(name, args).is_some()
                {
                    found = Some(relation_contract_string(name, args));
                }
            });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

// ── The three policing layers (design §3.2) ─────────────────────────────────
//
// `check_relation_arg_types` is a pure diagnostic side-effect mirroring
// `builtin_signatures::check_builtin_arg_types`: it pushes diagnostics for
// DEFINITE static violations only and never changes inference or the emitted IR
// node. It composes the §3.2 layers:
//   (a) UNIT       — the metric slot's dimension (θ:Angle, δ:Length).
//   (b) KIND/PROJ  — operands must project to the named datum, "implicit
//                    projection iff unique" (reuses β's projection semantics +
//                    `DatumProjectionUnavailable`/`Ambiguous` codes).
//   (c) CURATION   — only unconditionally-well-defined signatures exist; a
//                    `distance` call on a `Plane` is redirected to `offset`.
// PRD decision-6 gradualism: a `Type::Error` (poison) or `Type::TypeParam`
// (unresolved) slot is skipped silently.

/// The datum kind a relation's operands must project to (the §3.3 lift target).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedDatum {
    Direction,
    Axis,
    Plane,
    Point,
}

impl ExpectedDatum {
    /// The short datum name used in projection diagnostics.
    fn datum_name(self) -> &'static str {
        match self {
            ExpectedDatum::Direction => "Direction",
            ExpectedDatum::Axis => "Axis",
            ExpectedDatum::Plane => "Plane",
            ExpectedDatum::Point => "Point",
        }
    }
}

/// The checkable metric slot for a relation, if any: `(index, dimension,
/// type_name)`. Only the arity-3 metric-DRIVE relations have one (`angle`→Angle,
/// `distance`/`offset`→Length). A name without a metric slot — or a call too
/// short to reach the slot index — is not unit-checked (the arity-2 `angle`/
/// `distance` DERIVE forms reach `compiled_args.get(2) == None` and fall out).
fn relation_metric_slot(name: &str) -> Option<(usize, DimensionVector, &'static str)> {
    match name {
        "angle" => Some((2, DimensionVector::ANGLE, "Angle")),
        "distance" | "offset" => Some((2, DimensionVector::LENGTH, "Length")),
        _ => None,
    }
}

/// The datum kind a relation's two operand slots (indices 0 and 1) must project
/// to, or `None` for names that are not projection-policed in γ.
///
/// The shared verbs `angle`/`distance` are policed ONLY in their arity-3 DRIVE
/// form; their arity-2 DERIVE forms are geometry queries and return `None` here
/// (so the relation checker is a no-op for them). `coincident`/`on`/`tangent`
/// are intentionally `None`: `coincident` is kind-generic (any same-kind datum
/// pair), `on` mixes operand kinds (Point + host), and `tangent` is surface-
/// conditional — none has a single fixed operand datum to police in γ.
fn relation_operand_datum(name: &str, args: &[CompiledExpr]) -> Option<ExpectedDatum> {
    match name {
        // Orientation primitives (arity-2): operands are directions.
        "parallel" | "antiparallel" | "perpendicular" => Some(ExpectedDatum::Direction),
        // Named compounds with a fixed operand datum.
        "concentric" => Some(ExpectedDatum::Axis),
        "flush" | "offset" => Some(ExpectedDatum::Plane),
        // Shared verbs: only the arity-3 DRIVE form is a relation.
        "angle" if args.len() == 3 => Some(ExpectedDatum::Direction),
        "distance" if args.len() == 3 => Some(ExpectedDatum::Point),
        _ => None,
    }
}

/// Resolve whether an operand `actual` lifts to the `expected` datum under the
/// §3.3 "implicit projection iff unique" rule, reusing β's
/// [`DatumProjectionResolution`]:
/// - same-datum → `Resolved`;
/// - `Axis`→`Direction` (via `.dir`) and `Plane`→`Direction` (via `.normal`) →
///   `Resolved` (the unique direction);
/// - `Frame`→`Direction` → `Ambiguous` (any basis axis — suggest `.x/.y/.z`);
/// - anything else (e.g. `Point`→`Direction`, `Direction`→`Point`) →
///   `Unavailable`.
fn lift_to_datum(actual: &Type, expected: ExpectedDatum) -> DatumProjectionResolution {
    use DatumProjectionResolution::*;
    match (expected, actual) {
        // Same-datum: always the identity projection.
        (ExpectedDatum::Direction, Type::Direction) => Resolved(Type::Direction),
        (ExpectedDatum::Axis, Type::Axis) => Resolved(Type::Axis),
        (ExpectedDatum::Plane, Type::Plane) => Resolved(Type::Plane),
        (ExpectedDatum::Point, Type::Point { .. }) => Resolved(actual.clone()),
        // Implicit projection iff unique → Direction.
        (ExpectedDatum::Direction, Type::Axis) => Resolved(Type::Direction), // .dir
        (ExpectedDatum::Direction, Type::Plane) => Resolved(Type::Direction), // .normal
        (ExpectedDatum::Direction, Type::Frame(_)) => Ambiguous {
            suggestions: vec!["x", "y", "z"],
        },
        // No unique projection to the named datum.
        _ => Unavailable,
    }
}

/// Check a relation call's arguments against the §3.2 policing layers, pushing
/// reused diagnostic codes (`ArgTypeMismatch` for the unit layer; β's
/// `DatumProjectionUnavailable`/`DatumProjectionAmbiguous` for the kind/projection
/// and curation layers). A pure side-effect on `diagnostics`; mirrors
/// [`crate::builtin_signatures::check_builtin_arg_types`].
pub(crate) fn check_relation_arg_types(
    name: &str,
    compiled_args: &[CompiledExpr],
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // (a) UNIT layer — the metric slot's physical dimension.
    if let Some((idx, expected_dim, type_name)) = relation_metric_slot(name) {
        if let Some(metric) = compiled_args.get(idx) {
            match &metric.result_type {
                // Gradualism: poison / unresolved pass silently.
                Type::Error | Type::TypeParam(_) => {}
                // Dimensioned scalar: mismatch only when the dimension differs.
                Type::Scalar { dimension } if *dimension == expected_dim => {}
                other => emit_unit_mismatch(name, type_name, other, call_span, diagnostics),
            }
        }
    }

    // (b) KIND/PROJECTION + (c) CURATION layers — operand slots 0 and 1.
    if let Some(expected) = relation_operand_datum(name, compiled_args) {
        for idx in 0..2 {
            let Some(operand) = compiled_args.get(idx) else {
                break; // call too short — arity errors handled elsewhere.
            };
            let actual = &operand.result_type;

            // Gradualism: skip poison / unresolved operands silently.
            if matches!(actual, Type::Error | Type::TypeParam(_)) {
                continue;
            }

            // (c) CURATION: there is no bare plane-to-plane `distance`; the
            // well-defined plane-separation relation is `offset`.
            if name == "distance" && matches!(actual, Type::Plane) {
                emit_curation_use_offset(name, call_span, diagnostics);
                break;
            }

            // (b) KIND/PROJECTION: operand must lift to the named datum.
            // Anti-cascade: stop at the first failing operand.
            match lift_to_datum(actual, expected) {
                DatumProjectionResolution::Resolved(_) => {}
                DatumProjectionResolution::Unavailable => {
                    emit_projection_unavailable(name, expected, actual, call_span, diagnostics);
                    break;
                }
                DatumProjectionResolution::Ambiguous { suggestions } => {
                    emit_projection_ambiguous(name, expected, &suggestions, call_span, diagnostics);
                    break;
                }
            }
        }
    }
}

/// Emit a B10 unit-layer `ArgTypeMismatch` for a metric slot whose dimension is
/// wrong. Wording mirrors `builtin_signatures::emit_mismatch` for consistency.
fn emit_unit_mismatch(
    name: &str,
    type_name: &str,
    actual: &Type,
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let msg = format!("{name}: metric argument expects {type_name}, got {actual}");
    let label = format!("expected {type_name}, got {actual}");
    diagnostics.push(
        Diagnostic::error(msg)
            .with_code(DiagnosticCode::ArgTypeMismatch)
            .with_label(DiagnosticLabel::new(call_span, label)),
    );
}

/// Emit a B9 kind/projection-layer `DatumProjectionUnavailable` for an operand
/// with no unique projection to the named datum (reuses β's code/wording).
fn emit_projection_unavailable(
    name: &str,
    expected: ExpectedDatum,
    actual: &Type,
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let datum = expected.datum_name();
    let msg = format!("{name}: operand {actual} has no {datum} projection");
    let label = format!("no {datum} projection");
    diagnostics.push(
        Diagnostic::error(msg)
            .with_code(DiagnosticCode::DatumProjectionUnavailable)
            .with_label(DiagnosticLabel::new(call_span, label)),
    );
}

/// Emit a `DatumProjectionAmbiguous` for an operand whose projection to the named
/// datum is non-unique (e.g. `Frame`→`Direction`), naming the disambiguators.
fn emit_projection_ambiguous(
    name: &str,
    expected: ExpectedDatum,
    suggestions: &[&str],
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let datum = expected.datum_name();
    let hints = suggestions
        .iter()
        .map(|s| format!(".{s}"))
        .collect::<Vec<_>>()
        .join(", ");
    let msg = format!("{name}: ambiguous {datum} projection; specify one of {hints}");
    let label = format!("ambiguous {datum} projection");
    diagnostics.push(
        Diagnostic::error(msg)
            .with_code(DiagnosticCode::DatumProjectionAmbiguous)
            .with_label(DiagnosticLabel::new(call_span, label)),
    );
}

/// Emit the curation redirect for a `distance` call with a `Plane` operand:
/// there is no bare plane-to-plane distance, so point the author to `offset`,
/// which bundles its own parallelism precondition (design §3.2(c)).
fn emit_curation_use_offset(
    name: &str,
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let msg = format!(
        "{name}: no plane-to-plane distance; use offset(a, b, δ) for plane separation"
    );
    diagnostics.push(
        Diagnostic::error(msg)
            .with_code(DiagnosticCode::DatumProjectionUnavailable)
            .with_label(DiagnosticLabel::new(call_span, "use offset(a, b, δ)")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DiagnosticCode, SourceSpan, Type};
    use reify_ir::{CompiledExpr, Value};

    /// Independent fixture — the 9 pure relation names. Deliberately does NOT
    /// reference [`RELATION_FN_NAMES`] so a drift in that slice is caught against
    /// this independent list (mirrors `joint_signatures::tests::EXPECTED_NAMES`).
    const EXPECTED_NAMES: [&str; 9] = [
        // Primitive relations.
        "coincident",
        "on",
        "parallel",
        "antiparallel",
        "perpendicular",
        // Named compounds.
        "concentric",
        "flush",
        "offset",
        "tangent",
    ];

    /// Build a typed argument placeholder. Only `result_type` matters for the
    /// signature checks; the value is `Value::Undef` (relations stay Undef in γ).
    fn arg(ty: Type) -> CompiledExpr {
        CompiledExpr::literal(Value::Undef, ty)
    }

    // ── Name-family contract (step-3 RED / step-4 GREEN) ─────────────────────

    /// `is_relation_typed_fn` recognises every pure relation name.
    #[test]
    fn is_relation_typed_fn_recognises_all_pure_names() {
        for name in EXPECTED_NAMES {
            assert!(
                is_relation_typed_fn(name),
                "is_relation_typed_fn({name:?}) must be true (pure relation family)"
            );
        }
    }

    /// `is_relation_typed_fn` rejects sibling-family names, the empty name, and
    /// unknown names. `angle`/`distance` are deliberately NOT pure relation names
    /// (they are arity-gated shared verbs), so they too must be rejected here.
    #[test]
    fn is_relation_typed_fn_rejects_other_family_and_unknown_names() {
        // Geometry-query family.
        assert!(!is_relation_typed_fn("volume"), "must reject geometry-query 'volume'");
        // Math-linalg family.
        assert!(!is_relation_typed_fn("vec"), "must reject math-linalg 'vec'");
        // Joint-constructor family.
        assert!(!is_relation_typed_fn("prismatic"), "must reject joint 'prismatic'");
        // Shared-verb names are NOT pure relation names.
        assert!(!is_relation_typed_fn("angle"), "must reject shared-verb 'angle'");
        assert!(!is_relation_typed_fn("distance"), "must reject shared-verb 'distance'");
        // Empty / unknown.
        assert!(!is_relation_typed_fn(""), "must reject empty name");
        assert!(!is_relation_typed_fn("does_not_exist"), "must reject unrelated name");
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so the
    /// PascalCase forms must not match.
    #[test]
    fn is_relation_typed_fn_is_case_sensitive() {
        assert!(!is_relation_typed_fn("Coincident"), "PascalCase must not match");
        assert!(!is_relation_typed_fn("Offset"), "PascalCase must not match");
        assert!(!is_relation_typed_fn("Concentric"), "PascalCase must not match");
    }

    /// `RELATION_FN_NAMES` is exactly the 9 expected names: correct count, every
    /// expected name present, and no extra entry.
    #[test]
    fn relation_fn_names_are_exactly_the_nine() {
        assert_eq!(
            RELATION_FN_NAMES.len(),
            EXPECTED_NAMES.len(),
            "RELATION_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            RELATION_FN_NAMES
        );
        for name in EXPECTED_NAMES {
            assert!(
                RELATION_FN_NAMES.contains(&name),
                "RELATION_FN_NAMES must contain {name:?}"
            );
        }
        for name in RELATION_FN_NAMES {
            assert!(
                EXPECTED_NAMES.contains(name),
                "RELATION_FN_NAMES has unexpected entry {name:?} not in the fixture"
            );
        }
    }

    // ── Result-type resolution (step-3 RED / step-4 GREEN) ───────────────────

    /// Every pure relation name resolves to `Some(Type::Relation)` (arg-aware,
    /// but pure names always claim Relation regardless of operand shape).
    #[test]
    fn relation_fn_result_type_pure_names_are_relation() {
        let two = [arg(Type::Axis), arg(Type::Axis)];
        for name in EXPECTED_NAMES {
            assert_eq!(
                relation_fn_result_type(name, &two),
                Some(Type::Relation),
                "{name} must resolve to Type::Relation"
            );
        }
    }

    /// The shared-verb DRIVE forms `angle`/`distance` at arity 3 resolve to
    /// `Some(Type::Relation)`.
    #[test]
    fn relation_fn_result_type_angle_distance_arity3_is_relation() {
        let angle3 = [arg(Type::Axis), arg(Type::Axis), arg(Type::angle())];
        let dist3 = [
            arg(Type::point3(Type::length())),
            arg(Type::point3(Type::length())),
            arg(Type::length()),
        ];
        assert_eq!(
            relation_fn_result_type("angle", &angle3),
            Some(Type::Relation),
            "angle/3 is the metric DRIVE relation form"
        );
        assert_eq!(
            relation_fn_result_type("distance", &dist3),
            Some(Type::Relation),
            "distance/3 is the metric DRIVE relation form"
        );
    }

    /// The shared-verb DERIVE forms `angle`/`distance` at arity 2 resolve to
    /// `None` so the call falls through to the geometry-query arm.
    #[test]
    fn relation_fn_result_type_angle_distance_arity2_is_none() {
        let two = [arg(Type::Axis), arg(Type::Axis)];
        assert_eq!(
            relation_fn_result_type("angle", &two),
            None,
            "angle/2 must fall through to geometry-query (Angle)"
        );
        assert_eq!(
            relation_fn_result_type("distance", &two),
            None,
            "distance/2 must fall through to geometry-query (Scalar<Length>)"
        );
    }

    /// Unknown / sibling-family names resolve to `None`.
    #[test]
    fn relation_fn_result_type_unknown_is_none() {
        let two = [arg(Type::Axis), arg(Type::Axis)];
        assert_eq!(relation_fn_result_type("volume", &two), None);
        assert_eq!(relation_fn_result_type("", &two), None);
    }

    // ── ΔDOF (codim-law) inference (step-3 RED / step-4 GREEN) ────────────────

    /// `coincident(D, D)` removes the codimension of the datum kind `D`
    /// (design §3.1/§3.4): Direction=2, Point=3, Plane=3, Axis=4, Frame=6.
    #[test]
    fn relation_delta_dof_coincident() {
        assert_eq!(
            relation_delta_dof("coincident", &[arg(Type::Axis), arg(Type::Axis)]),
            Some(4)
        );
        assert_eq!(
            relation_delta_dof("coincident", &[arg(Type::Plane), arg(Type::Plane)]),
            Some(3)
        );
        assert_eq!(
            relation_delta_dof(
                "coincident",
                &[arg(Type::point3(Type::length())), arg(Type::point3(Type::length()))]
            ),
            Some(3)
        );
        assert_eq!(
            relation_delta_dof("coincident", &[arg(Type::Direction), arg(Type::Direction)]),
            Some(2)
        );
        assert_eq!(
            relation_delta_dof("coincident", &[arg(Type::Frame(3)), arg(Type::Frame(3))]),
            Some(6)
        );
    }

    /// `on(Point, host)` removes `3 − dim(host)`: Plane(dim 2)=1, Axis(dim 1)=2,
    /// Point(dim 0)=3.
    #[test]
    fn relation_delta_dof_on() {
        let pt = || arg(Type::point3(Type::length()));
        assert_eq!(relation_delta_dof("on", &[pt(), arg(Type::Plane)]), Some(1));
        assert_eq!(relation_delta_dof("on", &[pt(), arg(Type::Axis)]), Some(2));
        assert_eq!(relation_delta_dof("on", &[pt(), pt()]), Some(3));
    }

    /// Metric primitives remove 1; orientation primitives remove 2/2/1; named
    /// compounds publish their summed-body nominal codim.
    #[test]
    fn relation_delta_dof_primitives_and_compounds() {
        let aa = [arg(Type::Axis), arg(Type::Axis)];
        let aa_theta = [arg(Type::Axis), arg(Type::Axis), arg(Type::angle())];
        let pp_delta = [
            arg(Type::point3(Type::length())),
            arg(Type::point3(Type::length())),
            arg(Type::length()),
        ];
        // Orientation primitives.
        assert_eq!(relation_delta_dof("parallel", &aa), Some(2));
        assert_eq!(relation_delta_dof("antiparallel", &aa), Some(2));
        assert_eq!(relation_delta_dof("perpendicular", &aa), Some(1));
        // Metric primitives (arity-3 DRIVE form).
        assert_eq!(relation_delta_dof("angle", &aa_theta), Some(1));
        assert_eq!(relation_delta_dof("distance", &pp_delta), Some(1));
        // Named compounds (nominal).
        assert_eq!(relation_delta_dof("concentric", &aa), Some(4));
        assert_eq!(relation_delta_dof("flush", &[arg(Type::Plane), arg(Type::Plane)]), Some(3));
        assert_eq!(
            relation_delta_dof("offset", &[arg(Type::Plane), arg(Type::Plane), arg(Type::length())]),
            Some(3)
        );
        assert_eq!(relation_delta_dof("tangent", &aa), Some(2));
    }

    // ── Contract string (step-3 RED / step-4 GREEN) ──────────────────────────

    /// The contract string renders `name(ArgTys) -> Relation removes N`, with the
    /// metric operand rendered by its dimension name (`Length`, not `Scalar[m]`).
    #[test]
    fn relation_contract_string_offset() {
        let offset = [arg(Type::Plane), arg(Type::Plane), arg(Type::length())];
        assert_eq!(
            relation_contract_string("offset", &offset),
            "offset(Plane,Plane,Length) -> Relation removes 3"
        );
    }

    // ── Policing layers: check_relation_arg_types (step-5 RED / step-6 GREEN) ─
    //
    // A pure side-effect on `diagnostics` mirroring
    // `builtin_signatures::check_builtin_arg_types`: the three §3.2 policing
    // layers (unit / kind-projection / curation) plus PRD decision-6 gradualism.
    // RED here: `check_relation_arg_types` does not exist yet, so this module
    // fails to compile — the joint_signatures.rs RED convention.

    /// A span for the call site under test (offsets are irrelevant to the checks).
    fn span() -> SourceSpan {
        SourceSpan::new(0, 10)
    }

    // (a) UNIT layer — the metric slot's dimension (θ:Angle, δ:Length, §3.2(a)).

    /// `angle(Axis, Axis, Length)` — the metric must be an `Angle`; a `Length`
    /// metric is a B10 unit error (`ArgTypeMismatch` naming "Angle"). The `Axis`
    /// operands lift to `Direction`, so the metric mismatch is the ONLY diagnostic.
    #[test]
    fn check_unit_angle_with_length_metric_is_mismatch() {
        let args = [arg(Type::Axis), arg(Type::Axis), arg(Type::length())];
        let mut diags = Vec::new();
        check_relation_arg_types("angle", &args, span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {diags:?}");
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("Angle"),
            "B10 message must name the expected dimension 'Angle': {}",
            diags[0].message
        );
    }

    /// `distance(Point, Point, Angle)` — the metric must be a `Length`; an `Angle`
    /// metric is a B10 unit error naming "Length". `Point` operands are valid for
    /// distance, so the metric mismatch is the only diagnostic.
    #[test]
    fn check_unit_distance_with_angle_metric_is_mismatch() {
        let pt = || arg(Type::point3(Type::length()));
        let args = [pt(), pt(), arg(Type::angle())];
        let mut diags = Vec::new();
        check_relation_arg_types("distance", &args, span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {diags:?}");
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("Length"),
            "B10 message must name the expected dimension 'Length': {}",
            diags[0].message
        );
    }

    /// `angle(Axis, Axis, Angle)` — correct metric dimension, operands lift to
    /// `Direction` → no diagnostics.
    #[test]
    fn check_unit_angle_with_correct_metric_is_clean() {
        let args = [arg(Type::Axis), arg(Type::Axis), arg(Type::angle())];
        let mut diags = Vec::new();
        check_relation_arg_types("angle", &args, span(), &mut diags);
        assert!(diags.is_empty(), "correct angle call must be clean, got: {diags:?}");
    }

    // (b) KIND/PROJECTION layer — operands must project to the named datum,
    //     "implicit projection iff unique" (§3.2(b)/§3.3, reuses β codes).

    /// `angle(Point, Point, Angle)` — a `Point` has no `Direction` projection, so
    /// the operand fails to lift: B9 `DatumProjectionUnavailable`. Exactly one
    /// projection diagnostic (anti-cascade: stop at the first failing operand).
    #[test]
    fn check_projection_angle_on_points_is_unavailable() {
        let pt = || arg(Type::point3(Type::length()));
        let args = [pt(), pt(), arg(Type::angle())];
        let mut diags = Vec::new();
        check_relation_arg_types("angle", &args, span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {diags:?}");
        assert_eq!(diags[0].code, Some(DiagnosticCode::DatumProjectionUnavailable));
    }

    /// `parallel(Frame, Frame)` — a bare directional projection on a `Frame` is
    /// ambiguous (could be any basis axis): `DatumProjectionAmbiguous`. The code is
    /// the stable contract; the message suggests the disambiguating `.x/.y/.z`.
    #[test]
    fn check_projection_parallel_on_frames_is_ambiguous() {
        let args = [arg(Type::Frame(3)), arg(Type::Frame(3))];
        let mut diags = Vec::new();
        check_relation_arg_types("parallel", &args, span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {diags:?}");
        assert_eq!(diags[0].code, Some(DiagnosticCode::DatumProjectionAmbiguous));
    }

    /// Operands that already are (or lift uniquely to) the named datum are clean:
    /// `angle`(Axis→Direction), `flush`(Plane same-kind), `concentric`(Axis same-kind).
    #[test]
    fn check_projection_clean_when_operands_lift() {
        // angle: Axis → Direction via .dir
        let mut d1 = Vec::new();
        check_relation_arg_types(
            "angle",
            &[arg(Type::Axis), arg(Type::Axis), arg(Type::angle())],
            span(),
            &mut d1,
        );
        assert!(d1.is_empty(), "angle(Axis,Axis,Angle) must be clean, got: {d1:?}");

        // flush: Plane same-kind
        let mut d2 = Vec::new();
        check_relation_arg_types("flush", &[arg(Type::Plane), arg(Type::Plane)], span(), &mut d2);
        assert!(d2.is_empty(), "flush(Plane,Plane) must be clean, got: {d2:?}");

        // concentric: Axis same-kind
        let mut d3 = Vec::new();
        check_relation_arg_types(
            "concentric",
            &[arg(Type::Axis), arg(Type::Axis)],
            span(),
            &mut d3,
        );
        assert!(d3.is_empty(), "concentric(Axis,Axis) must be clean, got: {d3:?}");
    }

    // (c) CURATION layer — only unconditionally-well-defined signatures ship
    //     (§3.2(c)): there is no bare plane-to-plane `distance`; use `offset`.

    /// `distance(Plane, Plane, Length)` — there is no bare plane-to-plane distance;
    /// the well-defined relation is `offset`. A `Plane` operand on `distance` emits
    /// a single kind diagnostic hinting "use offset". (`offset` itself bundles its
    /// own parallelism precondition.)
    #[test]
    fn check_curation_distance_on_planes_hints_use_offset() {
        let args = [arg(Type::Plane), arg(Type::Plane), arg(Type::length())];
        let mut diags = Vec::new();
        check_relation_arg_types("distance", &args, span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {diags:?}");
        assert!(
            diags[0].message.contains("offset"),
            "curation diagnostic must hint 'use offset': {}",
            diags[0].message
        );
    }

    /// `offset(Plane, Plane, Length)` — the curated plane-separation relation: clean.
    #[test]
    fn check_curation_offset_on_planes_is_clean() {
        let args = [arg(Type::Plane), arg(Type::Plane), arg(Type::length())];
        let mut diags = Vec::new();
        check_relation_arg_types("offset", &args, span(), &mut diags);
        assert!(diags.is_empty(), "offset(Plane,Plane,Length) must be clean, got: {diags:?}");
    }

    // GRADUALISM (PRD decision-6) — poison/unresolved args pass silently.

    /// A `Type::Error` (poison) or `Type::TypeParam` (unresolved) operand or metric
    /// suppresses the relevant relation arg diagnostic (anti-cascade gradualism),
    /// mirroring `check_builtin_arg_types`.
    #[test]
    fn check_gradualism_error_and_type_param_pass_silently() {
        // metric poison: angle metric = Error → unit check skipped (operands lift OK).
        let mut d1 = Vec::new();
        check_relation_arg_types(
            "angle",
            &[arg(Type::Axis), arg(Type::Axis), arg(Type::Error)],
            span(),
            &mut d1,
        );
        assert!(d1.is_empty(), "Error metric must be skipped, got: {d1:?}");

        // operand poison: angle operands = Error → projection check skipped.
        let mut d2 = Vec::new();
        check_relation_arg_types(
            "angle",
            &[arg(Type::Error), arg(Type::Error), arg(Type::angle())],
            span(),
            &mut d2,
        );
        assert!(d2.is_empty(), "Error operands must be skipped, got: {d2:?}");

        // unresolved type params everywhere → all checks skipped.
        let mut d3 = Vec::new();
        let tp = || arg(Type::TypeParam("T".to_string()));
        check_relation_arg_types("distance", &[tp(), tp(), tp()], span(), &mut d3);
        assert!(d3.is_empty(), "TypeParam args must be skipped, got: {d3:?}");
    }

    // Arity-gating + unknown-name no-ops — the checker must not fire spuriously.

    /// The shared verbs `angle`/`distance` are policed ONLY in their arity-3 DRIVE
    /// form. The arity-2 DERIVE forms are geometry queries, so the relation checker
    /// must be a no-op for them — otherwise a valid `angle(p1, p2)` query would
    /// draw a spurious projection diagnostic. (Pure relation names have no arity gate.)
    #[test]
    fn check_arity2_shared_verbs_are_noop() {
        let pts = [arg(Type::point3(Type::length())), arg(Type::point3(Type::length()))];
        let mut d1 = Vec::new();
        check_relation_arg_types("angle", &pts, span(), &mut d1);
        assert!(d1.is_empty(), "arity-2 angle must not be policed as a relation, got: {d1:?}");

        let mut d2 = Vec::new();
        check_relation_arg_types("distance", &pts, span(), &mut d2);
        assert!(d2.is_empty(), "arity-2 distance must not be policed as a relation, got: {d2:?}");
    }

    /// An unrecognized / sibling-family name draws no relation diagnostics.
    #[test]
    fn check_unrecognized_name_is_noop() {
        let mut diags = Vec::new();
        check_relation_arg_types("volume", &[arg(Type::Geometry)], span(), &mut diags);
        assert!(diags.is_empty(), "unrecognized name must be a no-op, got: {diags:?}");
    }
}
