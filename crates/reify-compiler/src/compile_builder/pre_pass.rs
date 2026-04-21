//! Pre-pass helpers for [`crate::compile_with_prelude_refs`]: parse-error
//! forwarding, module-pragma validation, and `#no_prelude` shadowing.
//!
//! Each helper operates on `&mut CompilationCtx`, pushing diagnostics but
//! otherwise performing no compilation work — that happens in the later
//! phase modules.

use reify_syntax::ParsedModule;
use reify_types::{Diagnostic, DiagnosticLabel};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;

/// Module-level pragmas recognized by the compiler. Unknown pragmas produce
/// a warning diagnostic; they are otherwise ignored.
const KNOWN_MODULE_PRAGMAS: &[&str] = &["no_prelude", "precision", "solver", "kernel", "version"];

/// Forward every entry in `parsed.errors` as a warning diagnostic on `ctx`.
///
/// Parse errors are reported as warnings (not errors) because the parser has
/// already produced a (partial) tree — compilation continues best-effort and
/// any hard failures surface as errors from the phase that can't resolve the
/// malformed declaration.
pub(crate) fn forward_parse_errors(ctx: &mut CompilationCtx, parsed: &ParsedModule) {
    for err in &parsed.errors {
        ctx.diagnostics.push(
            Diagnostic::warning(format!("parse error: {}", err.message))
                .with_label(DiagnosticLabel::new(err.span, "parse error")),
        );
    }
}

/// Validate module-level pragmas against [`KNOWN_MODULE_PRAGMAS`], emitting
/// a warning diagnostic for each unrecognized name.
pub(crate) fn validate_module_pragmas(ctx: &mut CompilationCtx, parsed: &ParsedModule) {
    for pragma in &parsed.pragmas {
        if !KNOWN_MODULE_PRAGMAS.contains(&pragma.name.as_str()) {
            ctx.diagnostics.push(
                Diagnostic::warning(format!("unknown pragma #{}", pragma.name))
                    .with_label(DiagnosticLabel::new(pragma.span, "unknown pragma")),
            );
        }
    }
}

/// Apply the `#no_prelude` module pragma: if present, return an empty prelude
/// slice (shadowing the caller's prelude); otherwise return the caller's
/// prelude unchanged.
///
/// This suppresses ALL prelude-dependent behavior — unit seeding, trait/enum/
/// function resolution, and constraint def imports — by funneling every phase
/// through the returned slice.
pub(crate) fn effective_prelude<'a>(
    parsed: &ParsedModule,
    prelude: &'a [&'a CompiledModule],
) -> &'a [&'a CompiledModule] {
    let has_no_prelude = parsed.pragmas.iter().any(|p| p.name == "no_prelude");
    if has_no_prelude { &[] } else { prelude }
}
