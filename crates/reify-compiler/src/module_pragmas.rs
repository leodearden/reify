//! Module-pragma post-pass.
//!
//! Runs after [`crate::compile_builder::ctx::CompilationCtx::into_compiled_module`]
//! has assembled the [`CompiledModule`]. Extracts semantics from KNOWN module-level
//! pragmas (currently only `#precision`) into typed fields on the module, and emits
//! diagnostics for malformed or out-of-scope forms.
//!
//! The complementary pre-pass `validate_module_pragmas`
//! ([`crate::compile_builder::pre_pass`]) only warns on UNKNOWN module-pragma names;
//! the two passes run at different phases and emit non-overlapping diagnostic sets.

use reify_syntax::{ParsedModule, PragmaArg, PragmaValue};
use reify_types::{Diagnostic, DiagnosticLabel, DimensionVector, Value};

use crate::types::CompiledModule;
use crate::units::unit_to_scalar;

/// Apply every known module-level pragma to the assembled `CompiledModule`,
/// mutating typed fields and pushing diagnostics in place.
pub(crate) fn apply_module_pragmas(parsed: &ParsedModule, module: &mut CompiledModule) {
    apply_precision_pragma(parsed, module);
}

/// Process the first well-formed module-level `#precision(<Length-quantity>)` pragma:
/// store its SI-metres value on `module.default_tolerance`. All other shapes emit a
/// warning (or info, for the legacy `#precision(float64)` form) and leave
/// `default_tolerance` unset. Subsequent `#precision` pragmas (regardless of arg
/// shape) emit a "subsequent pragma ignored; first one wins" warning.
fn apply_precision_pragma(parsed: &ParsedModule, module: &mut CompiledModule) {
    let mut first_seen = false;
    for pragma in &parsed.pragmas {
        if pragma.name != "precision" {
            continue;
        }

        if first_seen {
            module.diagnostics.push(
                Diagnostic::warning("subsequent #precision pragma ignored; first one wins")
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
            );
            continue;
        }
        first_seen = true;

        // First-seen pragma: interpret its args.
        match pragma.args.as_slice() {
            [PragmaArg::Bare(PragmaValue::Quantity { value, unit })] => {
                match unit_to_scalar(*value, unit) {
                    Some((Value::Scalar { si_value, dimension }, _))
                        if dimension == DimensionVector::LENGTH =>
                    {
                        // Defensive sanity check: the grammar's `number_literal`
                        // regex (`\d+(\.\d+)?`) currently produces only non-negative
                        // finite f64 values, so this branch is unreachable from
                        // source today. Kept so a future grammar relaxation cannot
                        // silently push garbage tolerance into the kernel.
                        if si_value.is_finite() && si_value >= 0.0 {
                            module.default_tolerance = Some(si_value);
                        }
                    }
                    Some(_) => {
                        // unit_to_scalar matched, but the dimension is not LENGTH
                        // (e.g. `0.001s`).
                        module.diagnostics.push(
                            Diagnostic::warning(
                                "#precision: expected a Length quantity (e.g. 0.001m); ignored",
                            )
                            .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                        );
                    }
                    None => {
                        // Unrecognised unit (e.g. `1foo`).
                        module.diagnostics.push(
                            Diagnostic::warning(format!(
                                "#precision: unrecognised unit '{unit}'; v0.1 supports m/mm/cm/in"
                            ))
                            .with_label(DiagnosticLabel::new(pragma.span, "unrecognised unit")),
                        );
                    }
                }
            }
            [PragmaArg::Bare(PragmaValue::Ident(s))] if s == "float64" => {
                module.diagnostics.push(
                    Diagnostic::info(
                        "#precision(float64) recognised but ignored \u{2014} v0.1 always uses \
                         float64; use a Length literal (e.g. 0.001m) to set the default tolerance",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored legacy form")),
                );
            }
            [PragmaArg::Bare(PragmaValue::Number(_))] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#precision: expected a Length literal (e.g. 0.001m), got a bare \
                         number; ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            [PragmaArg::KeyValue { .. }] => {
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#precision: expected a Length literal (e.g. 0.001m); key=value form \
                         not recognised in v0.1; ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
            _ => {
                // Catch-all for any other shape: zero args, multiple args, a
                // bare String/Bool, or an Ident other than `float64`. All emit
                // the same generic "expected a Length literal" warning and
                // leave default_tolerance unset.
                module.diagnostics.push(
                    Diagnostic::warning(
                        "#precision: expected a Length literal (e.g. 0.001m); ignored",
                    )
                    .with_label(DiagnosticLabel::new(pragma.span, "ignored")),
                );
            }
        }
    }
}
