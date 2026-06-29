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
//!
//! Unknown-field and payload-type checks, plus real `Value::Enum` assembly, are
//! layered on in later steps.

use std::collections::HashSet;

use reify_core::ty::Type;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};
use reify_ir::{CompiledExpr, EnumDef, Value, VariantPayload};

use crate::expr::make_poison_literal;
use crate::type_compat::type_compatible;

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
) -> CompiledExpr {
    // Resolve the enum that declares a variant named `variant_name`.
    let resolved = enum_defs.iter().find_map(|e| {
        e.variants
            .iter()
            .find(|v| v.name == variant_name)
            .map(|v| (e.name.as_str(), v))
    });
    let (enum_name, variant_def) = match resolved {
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

    // Declared fields (declaration order). A bare/Unit variant declares none,
    // so its declared set is empty.
    let declared_fields: &[(String, Type)] = match &variant_def.payload {
        VariantPayload::Named(fields) => fields,
        VariantPayload::Unit => &[],
    };

    let supplied: HashSet<&str> = compiled_fields.iter().map(|(n, _)| n.as_str()).collect();

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

    // Payload-type check: each supplied field that IS declared must carry a
    // value whose compiled type is compatible with the declared field type.
    // Skip Type::Error declared types (an unresolvable declared type already
    // drew a diagnostic in resolve_enum_variant_payloads — anti-cascade); an
    // unknown supplied field is not declared, so it never reaches this check.
    for (field_name, value) in compiled_fields {
        if let Some((_, declared_ty)) = declared_fields.iter().find(|(n, _)| n == field_name) {
            if declared_ty.is_error() {
                continue;
            }
            if !type_compatible(declared_ty, &value.result_type) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "field '{}' of variant '{}' expects type {}, got {}",
                        field_name, variant_name, declared_ty, value.result_type
                    ))
                    .with_code(DiagnosticCode::VariantPayloadType)
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!("expected {}, got {}", declared_ty, value.result_type),
                    )),
                );
            }
        }
    }

    // Placeholder value — real `Value::Enum` payload assembly lands in a later
    // step. The result type is known (`Type::Enum`), so this does not cascade
    // type errors at the binding site.
    CompiledExpr::literal(Value::Undef, Type::Enum(enum_name.to_string()))
}
