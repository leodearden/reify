use super::*;

/// Lower parsed syntax annotations to compiled annotation types.
pub(crate) fn lower_annotations(
    parsed: &[reify_syntax::Annotation],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<reify_types::Annotation> {
    parsed
        .iter()
        .map(|ann| {
            let args = ann
                .args
                .iter()
                .filter_map(|expr| {
                    use reify_syntax::ExprKind;
                    match &expr.kind {
                        ExprKind::NumberLiteral(value) => {
                            if *value == value.floor() && value.abs() < i64::MAX as f64 {
                                Some(reify_types::AnnotationArg::Int(*value as i64))
                            } else {
                                Some(reify_types::AnnotationArg::Real(*value))
                            }
                        }
                        ExprKind::StringLiteral(s) => {
                            Some(reify_types::AnnotationArg::String(s.clone()))
                        }
                        ExprKind::BoolLiteral(b) => Some(reify_types::AnnotationArg::Bool(*b)),
                        ExprKind::Ident(name) => {
                            Some(reify_types::AnnotationArg::Ident(name.clone()))
                        }
                        _ => {
                            diagnostics.push(
                                Diagnostic::warning(format!(
                                    "unsupported expression in annotation @{} argument; only literals and identifiers are allowed",
                                    ann.name
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "complex expression")),
                            );
                            None
                        }
                    }
                })
                .collect();
            reify_types::Annotation {
                name: ann.name.clone(),
                args,
                span: ann.span,
            }
        })
        .collect()
}

/// Validate annotations against known annotation rules and context.
///
/// Known annotations and their valid contexts:
/// - `@test`: valid on structure, occurrence, function, constraint_def
/// - `@optimized`: valid on structure, occurrence
/// - `@solver_hint`: valid on structure, occurrence
/// - `@deprecated`: valid on any context
pub(crate) fn validate_annotations(
    annotations: &[reify_types::Annotation],
    context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for ann in annotations {
        match ann.name.as_str() {
            "deprecated" => {
                // Valid on any context — no warning.
            }
            "test" => {
                if !matches!(context, "structure" | "occurrence" | "function" | "constraint_def") {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "annotation @test is not valid on {context} declarations"
                        ))
                        .with_label(DiagnosticLabel::new(ann.span, "@test")),
                    );
                }
            }
            "optimized" => {
                if !matches!(context, "structure" | "occurrence") {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "annotation @optimized is not valid on {context} declarations"
                        ))
                        .with_label(DiagnosticLabel::new(ann.span, "@optimized")),
                    );
                }
            }
            "solver_hint" => {
                if !matches!(context, "structure" | "occurrence") {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "annotation @solver_hint is not valid on {context} declarations"
                        ))
                        .with_label(DiagnosticLabel::new(ann.span, "@solver_hint")),
                    );
                }
            }
            other => {
                diagnostics.push(
                    Diagnostic::warning(format!("unknown annotation @{other}"))
                        .with_label(DiagnosticLabel::new(ann.span, "unknown annotation")),
                );
            }
        }
    }
}

/// Validate block-level pragmas on a compiled declaration, emitting warnings for unknown names.
///
/// Known block-level pragmas: `#precision`, `#solver`, `#kernel`.
/// Unknown pragmas emit a `Severity::Warning` diagnostic.
pub(crate) fn validate_pragmas(
    pragmas: &[reify_syntax::Pragma],
    _context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    const KNOWN_BLOCK_PRAGMAS: &[&str] = &["precision", "solver", "kernel"];
    for pragma in pragmas {
        if !KNOWN_BLOCK_PRAGMAS.contains(&pragma.name.as_str()) {
            diagnostics.push(
                Diagnostic::warning(format!("unknown pragma #{}", pragma.name))
                    .with_label(DiagnosticLabel::new(pragma.span, "unknown pragma")),
            );
        }
    }
}

// ─── Deprecation-on-use helpers ─────────────────────────────────────────────

/// Extract the deprecation message from an annotation list.
///
/// Returns `Some(message)` if there is an `@deprecated("message")` annotation with a
/// `String` first arg, `Some("")` if `@deprecated` has no args, or `None` if there
/// is no `@deprecated` annotation at all.
pub(crate) fn deprecation_message(annotations: &[reify_types::Annotation]) -> Option<String> {
    for ann in annotations {
        if ann.name == "deprecated" {
            return Some(match ann.args.first() {
                Some(reify_types::AnnotationArg::String(s)) => s.clone(),
                _ => String::new(),
            });
        }
    }
    None
}

/// Emit a deprecation warning for a use-site reference to a deprecated entity.
///
/// Format: `use of deprecated <kind> '<name>': <message>` (with message)
///         `use of deprecated <kind> '<name>'` (without message)
pub(crate) fn emit_deprecation_warning(
    entity_kind: &str,
    entity_name: &str,
    message: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let text = if message.is_empty() {
        format!("use of deprecated {entity_kind} '{entity_name}'")
    } else {
        format!("use of deprecated {entity_kind} '{entity_name}': {message}")
    };
    diagnostics.push(
        Diagnostic::warning(text)
            .with_label(DiagnosticLabel::new(span, "deprecated")),
    );
}

