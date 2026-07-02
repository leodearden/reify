//! Field-set + payload-type checking and value assembly for brace-form
//! enum-variant construction `Variant { field: value, ... }` (task δ #3942).
//!
//! # Why a brace-only, variant-only resolution
//!
//! The construction surface is the BRACE form (F2-a, Leo-ratified 2026-05-27):
//! Reify structures and functions are instantiated/called with PARENS
//! (`Name(field: value)` / `Name(args)`), so `Name { field: value }` is
//! unambiguously a variant construction — there is no structure/fn collision to
//! disambiguate. The enum is therefore resolved purely by searching `enum_defs`
//! for the (first) enum that declares a variant named `name` (§11 Q3: the rare
//! two-enum same-variant-name collision resolves first-match; no fixture hits
//! it).
//!
//! # Checks
//!
//! - **Missing field** ([`DiagnosticCode::VariantMissingField`]): a field the
//!   variant declares was not supplied.
//! - **Unknown field** ([`DiagnosticCode::VariantUnknownField`]): a supplied
//!   field the variant does not declare (a bare/`Unit` variant declares none,
//!   so any supplied field is unknown).
//! - **Payload type** ([`DiagnosticCode::VariantPayloadType`]): a supplied
//!   field's value type is incompatible with the declared field type.
//!
//! # Value assembly
//!
//! When the field-set is valid and every field type-checks, the construction
//! compiles to a literal `Value::Enum { type_name, variant, payload }` whose
//! payload is assembled in the variant's DECLARATION order (PRD D6/Q4 — so
//! `content_hash`/`PartialEq`/`Ord` of the produced value are stable regardless
//! of construction-site field order). Field values must be compile-time
//! literals; a non-constant payload field is out of v1 scope (the runtime
//! constructor node, paralleling `StructureInstanceCtor`, is a deferred
//! refinement) and draws a diagnostic.

use std::collections::{HashMap, HashSet};

use reify_core::ty::Type;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};
use reify_ir::{CompiledExpr, CompiledExprKind, EnumDef, Value, VariantPayload};

use crate::expr::make_poison_literal;
use crate::type_compat::{enum_payload_compatible, type_carries_type_param, type_compatible, unify};
use crate::type_resolution::substitute_type_params;

/// Resolve, field-check, and build a brace-form variant construction
/// `variant_name { compiled_fields }` into a [`CompiledExpr`].
///
/// `compiled_fields` are the already-compiled field value expressions in source
/// order (the recursion context lives in [`crate::expr`]); this helper resolves
/// the declaring enum and checks the supplied fields against the variant's
/// declared payload, emitting diagnostics on `diagnostics`.
pub(crate) fn compile_variant_construct(
    variant_name: &str,
    compiled_fields: &[(String, CompiledExpr)],
    enum_defs: &[EnumDef],
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
    expected_type: Option<&Type>,
) -> CompiledExpr {
    // Resolve the enum that declares a variant named `variant_name`.
    let resolved = enum_defs.iter().find_map(|e| {
        e.variants
            .iter()
            .find(|v| v.name == variant_name)
            .map(|v| (e, v))
    });
    let (enum_def, variant_def) = match resolved {
        Some(pair) => pair,
        None => {
            // Anti-cascade (mirrors the EnumAccess unknown-enum arm): no enum in
            // scope declares this variant — poison to suppress follow-on errors.
            return make_poison_literal(
                diagnostics,
                Diagnostic::error(format!(
                    "unknown variant '{}': no enum in scope declares it",
                    variant_name
                ))
                .with_label(DiagnosticLabel::new(span, "unknown variant")),
            );
        }
    };
    let enum_name = enum_def.name.as_str();

    // Declared fields (declaration order). A bare/Unit variant declares none,
    // so its declared set is empty.
    let declared_fields: &[(String, Type)] = match &variant_def.payload {
        VariantPayload::Named(fields) => fields,
        VariantPayload::Unit => &[],
    };

    let supplied: HashSet<&str> = compiled_fields.iter().map(|(n, _)| n.as_str()).collect();

    // Baseline diagnostic count: any push by the field-set/type checks below
    // means THIS construction is invalid and the value must not be assembled.
    let checks_start = diagnostics.len();

    // Duplicate-field check: the grammar permits a repeated field name
    // syntactically (`Rect { width: 20mm, width: 10mm, height: 5mm }`), but the
    // value-assembly loop below takes the FIRST occurrence in declaration order
    // and would otherwise silently DROP the rest — a quiet correctness footgun
    // for a typo'd repeat. Flag every extra (2nd, 3rd, …) occurrence so a
    // duplicate is a hard error rather than a silent drop. Counted in the
    // checks region, so a duplicate also suppresses value assembly (the
    // construction is invalid).
    let mut seen_fields: HashSet<&str> = HashSet::new();
    for (field_name, _value) in compiled_fields {
        if !seen_fields.insert(field_name.as_str()) {
            diagnostics.push(
                Diagnostic::error(format!(
                    "variant '{}' has duplicate field '{}'",
                    variant_name, field_name
                ))
                .with_code(DiagnosticCode::VariantDuplicateField)
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("duplicate field '{}'", field_name),
                )),
            );
        }
    }

    // Missing-field check: every declared field must be supplied.
    for (decl_name, _decl_ty) in declared_fields {
        if !supplied.contains(decl_name.as_str()) {
            diagnostics.push(
                Diagnostic::error(format!(
                    "variant '{}' is missing field '{}'",
                    variant_name, decl_name
                ))
                .with_code(DiagnosticCode::VariantMissingField)
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("missing field '{}'", decl_name),
                )),
            );
        }
    }

    // Unknown-field check: every supplied field must be declared. A bare/Unit
    // variant has an empty declared set, so any supplied field is unknown
    // (handles `Point { x: 1mm }`). Missing + unknown can co-occur (e.g.
    // `Circle { diameter: 5mm }` is missing `radius` AND has unknown `diameter`).
    let declared_names: HashSet<&str> =
        declared_fields.iter().map(|(n, _)| n.as_str()).collect();
    for (field_name, _value) in compiled_fields {
        if !declared_names.contains(field_name.as_str()) {
            diagnostics.push(
                Diagnostic::error(format!(
                    "variant '{}' has no field '{}'",
                    variant_name, field_name
                ))
                .with_code(DiagnosticCode::VariantUnknownField)
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("no field '{}'", field_name),
                )),
            );
        }
    }

    // Pinned-annotation override (task γ #4031, PRD §5 D3 pinned-annotation
    // check): when the construction site carries an `expected_type` that
    // resolves to THIS enum applied to exactly `enum_def.type_params.len()`
    // args (e.g. `param r : Result<Force, String> = Ok { value: 5mm }`), the
    // annotation authoritatively PINS every type param positionally — this
    // subst OVERRIDES payload-driven inference for the payload-type check
    // below. Computed FIRST (before the inference/conflict pass) so that pass
    // can be gated on its presence — see the next comment. Falls back to the
    // payload-inferred `subst` when `expected_type` is absent, unresolved
    // (e.g. `Type::Error` from an upstream diagnostic), for a different enum,
    // or arity-mismatched.
    let pin_subst: Option<HashMap<String, Type>> = expected_type.and_then(|ty| match ty {
        Type::Applied { name, args } if name == enum_name && args.len() == enum_def.type_params.len() => {
            Some(
                enum_def
                    .type_params
                    .iter()
                    .zip(args.iter())
                    .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                    .collect(),
            )
        }
        _ => None,
    });

    // Type-argument inference (task γ #4031, PRD §5 D3): bind each supplied
    // field's declared type-param leaves to the corresponding concrete value
    // type via the reused `unify` machinery (the same conservative, single-pass
    // structural unification generic function-call inference uses for
    // `FnTypeArgConflict`, task 4231). Conservative + payload-driven: a
    // structural mismatch binds nothing (unify's contract) and is caught by
    // the payload-type check below once substituted; a same-param
    // double-binding conflict (`Err`) is a genuine construction-site error —
    // e.g. `enum Pair<T> { Both { a: T, b: T } }` constructed
    // `Both { a: 1mm, b: 1N }` binds `T` to `Length` from `a`, then conflicts
    // with `Force` from `b`. At most one diagnostic per param (`conflicted_params`
    // de-dup) — a 3rd+ field binding the same already-conflicted param would
    // otherwise cascade a diagnostic per extra field. Field iteration stays in
    // declaration/source order (`compiled_fields` is source order) for
    // deterministic "first conflict wins" attribution.
    //
    // Skipped entirely when a pin is present (`pin_subst.is_some()`): the
    // annotation already authoritatively resolves every type param, so this
    // payload-only cross-field conflict pass would double-report the same
    // root cause the pinned payload-type check below already reports
    // per-field (and more precisely, naming the exact mismatching field) as
    // `VariantPayloadType` — e.g. `param p : Pair<Length> = Both { a: 1mm,
    // b: 1N }` would otherwise emit BOTH `EnumTypeArgConflict` (from this
    // pass, since `a` and `b` disagree with each other) AND
    // `VariantPayloadType` for field `b` (from the pinned check, since `b`
    // disagrees with the pin) for the same single user error. `subst` is also
    // never consulted once a pin is present (`final_subst` below always
    // prefers `pin_subst`), so skipping the loop is a pure no-op for the
    // pinned case beyond suppressing the redundant diagnostic.
    let mut subst: HashMap<String, Type> = HashMap::new();
    let mut conflicted_params: HashSet<String> = HashSet::new();
    if pin_subst.is_none() {
        for (field_name, value) in compiled_fields {
            if let Some((_, declared_ty)) = declared_fields.iter().find(|(n, _)| n == field_name) {
                if declared_ty.is_error() {
                    continue;
                }
                if let Err(conflict) = unify(declared_ty, &value.result_type, &mut subst)
                    && conflicted_params.insert(conflict.param.clone())
                {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "type parameter '{}' bound to both {} and {}",
                            conflict.param, conflict.existing, conflict.incoming
                        ))
                        .with_code(DiagnosticCode::EnumTypeArgConflict)
                        .with_label(DiagnosticLabel::new(
                            span,
                            format!(
                                "conflicting type argument for '{}': {} vs {}",
                                conflict.param, conflict.existing, conflict.incoming
                            ),
                        )),
                    );
                }
            }
        }
    }
    let final_subst = pin_subst.as_ref().unwrap_or(&subst);

    // Payload-type check: each supplied field that IS declared must carry a
    // value whose compiled type is compatible with the declared field type,
    // AFTER substituting any type parameters bound by inference (or pinned by
    // annotation) above (task γ #4031). Skip Type::Error declared types (an
    // unresolvable declared type already drew a diagnostic in
    // resolve_enum_variant_payloads — anti-cascade); an unknown supplied field
    // is not declared, so it never reaches this check. A substituted type
    // that still carries an unbound type param is conservatively skipped
    // (INV-3) — inference never guesses, so an unmentioned/unpinned param
    // must not spuriously fail this check. When there is NO pin, a field
    // whose declared type IS a conflicted param (`conflicted_params` above)
    // is also skipped — anti-cascade: `subst`'s binding for that param is an
    // arbitrary first-writer-wins artifact once conflicted, so checking
    // against it would emit a second, misleading diagnostic for the same root
    // cause already reported as `EnumTypeArgConflict`. A PIN's binding is
    // never arbitrary (it is the user's explicit annotation), so this skip
    // does not apply once a pin overrides the substitution.
    // Non-generic enums are unaffected: no `Type::TypeParam` leaves means
    // `subst` stays empty and `substitute_type_params` is the identity, so
    // this is byte-for-byte the pre-γ check (INV-6).
    //
    // Recursive/applied-field tolerance (task γ #4031 step-8): a CONCRETE
    // substituted type that is enum-shaped (`Type::Enum(n)` or
    // `Type::Applied { name: n, .. }`) is additionally checked against
    // [`enum_payload_compatible`], which accepts a supplied `Type::Enum(n)`
    // of the SAME base name. This covers the pinned-recursive case (e.g. a
    // `Tree<Length>`-pinned `Node` field substitutes to concrete
    // `Type::Applied { "Tree", [Length] }`) where the supplied child's
    // erased `result_type` (`Type::Enum("Tree")`, D1/F-Mono erasure — no
    // value ever carries type args) would otherwise spuriously fail raw
    // `type_compatible` (no Applied-vs-Enum rule there). The unpinned case is
    // already handled above by the unbound-type-param skip (the substituted
    // type still carries `TypeParam` and never reaches this call), so this
    // is a confirming belt-and-braces layer for when substitution DOES fully
    // resolve an enum-shaped declared type.
    for (field_name, value) in compiled_fields {
        if let Some((_, declared_ty)) = declared_fields.iter().find(|(n, _)| n == field_name) {
            if declared_ty.is_error() {
                continue;
            }
            if pin_subst.is_none()
                && let Type::TypeParam(p) = declared_ty
                && conflicted_params.contains(p)
            {
                continue;
            }
            let substituted = substitute_type_params(declared_ty, final_subst);
            if type_carries_type_param(&substituted) {
                continue;
            }
            if !type_compatible(&substituted, &value.result_type)
                && !enum_payload_compatible(&substituted, &value.result_type)
            {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "field '{}' of variant '{}' expects type {}, got {}",
                        field_name, variant_name, substituted, value.result_type
                    ))
                    .with_code(DiagnosticCode::VariantPayloadType)
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!("expected {}, got {}", substituted, value.result_type),
                    )),
                );
            }
        }
    }

    // If any field-set/type check above failed for THIS construction, the value
    // cannot be assembled. The variant IS resolved, so the result type is known
    // (`Type::Enum`) — return a typed placeholder (not a `Type::Error` poison)
    // so the field-check diagnostics carry the signal without cascading a type
    // mismatch at the binding site.
    if diagnostics.len() > checks_start {
        return CompiledExpr::literal(Value::Undef, Type::Enum(enum_name.to_string()));
    }

    // Valid construction: assemble the payload in the variant's DECLARATION
    // order (PRD D6/Q4 — normalize construction-site field order so the value's
    // content-hash / PartialEq / Ord are order-stable). A valid field-set
    // guarantees every declared field is supplied exactly once.
    let mut payload: Vec<(String, Value)> = Vec::with_capacity(declared_fields.len());
    for (decl_name, _decl_ty) in declared_fields {
        let (_, compiled) = compiled_fields
            .iter()
            .find(|(n, _)| n == decl_name)
            .expect("valid field-set guarantees every declared field is supplied");
        match &compiled.kind {
            CompiledExprKind::Literal(value) => payload.push((decl_name.clone(), value.clone())),
            _ => {
                // Non-constant payload value (e.g. a runtime param reference,
                // which is plausible user code — not a typo): the runtime
                // constructor node is out of v1 scope (deferred follow-up). The
                // variant IS resolved and its type is known, so return a TYPED
                // placeholder (`Value::Undef` @ `Type::Enum`) — mirroring the
                // failed-field-check arm above — rather than a `Type::Error`
                // poison. This keeps the "not yet supported" signal from
                // cascading a spurious type mismatch at the binding site.
                diagnostics.push(
                    Diagnostic::error(format!(
                        "non-constant payload value for field '{}' of variant '{}' is not yet supported",
                        decl_name, variant_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        "non-constant variant payload field",
                    )),
                );
                return CompiledExpr::literal(Value::Undef, Type::Enum(enum_name.to_string()));
            }
        }
    }

    CompiledExpr::literal(
        Value::Enum {
            type_name: enum_name.to_string(),
            variant: variant_name.to_string(),
            payload,
        },
        Type::Enum(enum_name.to_string()),
    )
}
