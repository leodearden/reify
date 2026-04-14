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
/// - `@optimized`: valid on structure, occurrence, constraint_def
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
                // @optimized is accepted on structure and occurrence even though
                // optimized_target is only consumed in constraint_def context
                // (entity.rs reads it when lowering a ConstraintDef to a
                // CompiledConstraint). Those two contexts remain in the allow-list
                // to avoid a breaking change; a follow-up may add a distinct
                // 'annotation has no effect here' warning or remove them entirely.
                if !matches!(context, "structure" | "occurrence" | "constraint_def") {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "annotation @optimized is not valid on {context} declarations"
                        ))
                        .with_label(DiagnosticLabel::new(ann.span, "@optimized")),
                    );
                } else if context == "constraint_def"
                    && !matches!(
                        ann.args.first(),
                        Some(reify_types::AnnotationArg::String(_))
                    )
                {
                    // @optimized without a string-literal target on a constraint_def
                    // silently routes to the language-level checker, which confuses
                    // users who think they wired up an optimized impl. Warn explicitly.
                    //
                    // The target is only consumed in constraint_def context: entity.rs
                    // reads optimized_target when lowering a ConstraintDef to a
                    // CompiledConstraint. On structure/occurrence contexts the annotation
                    // is stored but nothing downstream reads the target string, so warning
                    // there would tell the user to add a string that nothing uses.
                    diagnostics.push(
                        Diagnostic::warning(
                            "annotation @optimized requires a string literal target, e.g. @optimized(\"kernel::foo\")"
                                .to_string(),
                        )
                        .with_label(DiagnosticLabel::new(ann.span, "@optimized missing target")),
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

    // Duplicate-annotation checks. These only apply in constraint_def context
    // because `optimized_target` (the extractor that stamps
    // `CompiledConstraint::optimized_target`) is only called by entity.rs when
    // lowering constraint defs. On structure/occurrence contexts, multiple
    // @optimized annotations have no consumer downstream, so warning that one
    // "shadows" another would be misleading — there is nothing being shadowed.
    //
    // Within constraint_def, `optimized_target` uses first-valid-wins semantics:
    // it skips malformed @optimized entries (those without a string-literal arg)
    // and returns the first well-formed one. Warn on every *valid* @optimized
    // past the first valid one so the user knows their shadowed entry is ignored:
    //   @optimized("new_target")
    //   @optimized("legacy_target")   // ← valid but shadowed; warn here
    //
    // Malformed entries are intentionally excluded from the "seen" count.
    // They already generate a separate missing-target warning, and counting
    // them here would produce contradictory diagnostics: e.g. warning that
    // annotation #1 is malformed and then warning that annotation #2 is
    // shadowed by annotation #1.
    if context == "constraint_def" {
        let mut seen_valid_optimized = false;
        for ann in annotations {
            if ann.name == "optimized"
                && matches!(ann.args.first(), Some(reify_types::AnnotationArg::String(_)))
            {
                if seen_valid_optimized {
                    diagnostics.push(
                        Diagnostic::warning(
                            "multiple @optimized annotations on the same declaration — only the first well-formed one is used"
                                .to_string(),
                        )
                        .with_label(DiagnosticLabel::new(ann.span, "duplicate @optimized")),
                    );
                }
                seen_valid_optimized = true;
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

/// Extract the optimization target from a parsed annotation list.
///
/// Returns `Some(target)` for the first `@optimized("target")` annotation with a
/// `StringLiteral` first arg, or `None` if no such annotation is found. Malformed
/// `@optimized` entries (no args, or a non-string arg) are skipped so that a later
/// valid `@optimized("target")` in the list is still returned. This matches the
/// validator's "first-valid wins" contract while ensuring a malformed earlier
/// sibling does not silently drop a valid later one.
///
/// Operates on parsed `reify_syntax::Annotation`s because the helper runs against
/// `ConstraintDef.annotations` before lowering.
pub(crate) fn optimized_target(annotations: &[reify_syntax::Annotation]) -> Option<String> {
    for ann in annotations {
        if ann.name == "optimized"
            && let Some(first) = ann.args.first()
            && let reify_syntax::ExprKind::StringLiteral(s) = &first.kind
        {
            return Some(s.clone());
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

