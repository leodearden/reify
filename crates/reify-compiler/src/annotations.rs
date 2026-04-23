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
            reify_types::DEPRECATED_ANNOTATION => {
                // Valid on any context — no warning.
            }
            reify_types::TEST_ANNOTATION => {
                if !matches!(
                    context,
                    "structure" | "occurrence" | "function" | "constraint_def"
                ) {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "annotation @test is not valid on {context} declarations"
                        ))
                        .with_label(DiagnosticLabel::new(ann.span, "@test")),
                    );
                }
            }
            reify_types::OPTIMIZED_ANNOTATION => {
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
                } else if context == "constraint_def" && !is_valid_optimized(ann) {
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
            reify_types::SOLVER_HINT_ANNOTATION => {
                if !matches!(context, "structure" | "occurrence" | "param" | "let") {
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
            if is_valid_optimized(ann) {
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

/// Return `true` if `ann` is a well-formed `@optimized("target")` annotation —
/// i.e. its name is `"optimized"` and its first argument is a string literal.
pub(crate) fn is_valid_optimized(ann: &reify_types::Annotation) -> bool {
    ann.name == reify_types::OPTIMIZED_ANNOTATION
        && matches!(
            ann.args.first(),
            Some(reify_types::AnnotationArg::String(_))
        )
}

/// Pragmas valid on block-level declarations (structures, occurrences, traits, purposes, etc.).
pub(crate) const KNOWN_BLOCK_PRAGMAS: &[&str] = &["precision", "solver", "kernel"];

/// Pragmas valid only at module level; not valid on any block-level declaration.
pub(crate) const MODULE_ONLY_PRAGMAS: &[&str] = &["no_prelude", "version"];

/// Returns `true` if `name` is a known block-level pragma.
pub(crate) fn is_known_block_pragma(name: &str) -> bool {
    KNOWN_BLOCK_PRAGMAS.contains(&name)
}

/// Returns `true` if `name` is a module-only pragma (valid at module level, not on blocks).
pub(crate) fn is_module_only_pragma(name: &str) -> bool {
    MODULE_ONLY_PRAGMAS.contains(&name)
}

/// Returns `true` if `name` is any recognized pragma (block or module-only).
///
/// The module-level pragma set is `KNOWN_BLOCK_PRAGMAS ∪ MODULE_ONLY_PRAGMAS`,
/// which structurally enforces the subset relation: the block list is always a
/// subset of the module list by construction rather than by hand-maintenance.
pub(crate) fn is_known_module_pragma(name: &str) -> bool {
    is_known_block_pragma(name) || is_module_only_pragma(name)
}

/// Validate block-level pragmas on a compiled declaration, emitting warnings for unknown or
/// misplaced pragma names.
///
/// Known block-level pragmas: `#precision`, `#solver`, `#kernel` — no warning.
/// Module-only pragmas (`#no_prelude`, `#version`) on a block: context-aware "only valid at
/// module level" warning.
/// All other pragma names: generic `"unknown pragma #<name>"` warning.
pub(crate) fn validate_pragmas(
    pragmas: &[reify_syntax::Pragma],
    context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for pragma in pragmas {
        if is_known_block_pragma(&pragma.name) {
            // Valid here — no warning.
        } else if is_module_only_pragma(&pragma.name) {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "pragma #{} is only valid at module level, not on {}",
                    pragma.name, context
                ))
                .with_label(DiagnosticLabel::new(
                    pragma.span,
                    "module-only pragma on block",
                )),
            );
        } else {
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
pub(crate) fn deprecation_message(annotations: &[reify_types::Annotation]) -> Option<&str> {
    for ann in annotations {
        if ann.name == reify_types::DEPRECATED_ANNOTATION {
            return Some(match ann.args.first() {
                Some(reify_types::AnnotationArg::String(s)) => s.as_str(),
                _ => "",
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
        if ann.name == reify_types::OPTIMIZED_ANNOTATION
            && let Some(first) = ann.args.first()
            && let reify_syntax::ExprKind::StringLiteral(s) = &first.kind
        {
            return Some(s.clone());
        }
    }
    None
}

/// Extract solver hints from compiled annotations.
///
/// Iterates all `@solver_hint` annotations, parsing the first arg as a hint kind
/// string ("discrete_set" or "prefer_stock") and the second arg as an identifier
/// naming the collection. Emits warnings for unrecognized kinds or missing args.
pub(crate) fn extract_solver_hints(
    annotations: &[reify_types::Annotation],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SolverHint> {
    let mut hints = Vec::new();
    for ann in annotations {
        if ann.name != reify_types::SOLVER_HINT_ANNOTATION {
            continue;
        }
        // First arg: string literal for hint kind
        let kind = match ann.args.first() {
            Some(reify_types::AnnotationArg::String(s)) => match s.as_str() {
                "discrete_set" => SolverHintKind::DiscreteSet,
                "prefer_stock" => SolverHintKind::PreferStock,
                other => {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "unknown solver hint kind '{other}'; expected 'discrete_set' or 'prefer_stock'"
                        ))
                        .with_label(DiagnosticLabel::new(ann.span, "unknown kind")),
                    );
                    continue;
                }
            },
            _ => {
                diagnostics.push(
                    Diagnostic::warning(
                        "@solver_hint requires a string literal kind as first argument, \
                         e.g. @solver_hint(\"discrete_set\", collection)"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(ann.span, "missing kind")),
                );
                continue;
            }
        };
        // Second arg: identifier for collection name
        let collection = match ann.args.get(1) {
            Some(reify_types::AnnotationArg::Ident(name)) => name.clone(),
            _ => {
                diagnostics.push(
                    Diagnostic::warning(
                        "@solver_hint requires a collection reference as second argument, \
                         e.g. @solver_hint(\"discrete_set\", bolt_lengths)"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(ann.span, "missing collection")),
                );
                continue;
            }
        };
        hints.push(SolverHint {
            kind,
            collection,
            span: ann.span,
        });
    }
    hints
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
    diagnostics
        .push(Diagnostic::warning(text).with_label(DiagnosticLabel::new(span, "deprecated")));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(name: &str, args: Vec<reify_types::AnnotationArg>) -> reify_types::Annotation {
        reify_types::Annotation {
            name: name.to_string(),
            args,
            span: reify_types::SourceSpan::empty(0),
        }
    }

    #[test]
    fn is_valid_optimized_true_for_string_arg() {
        let a = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String(
                "kernel::foo".to_string(),
            )],
        );
        assert!(is_valid_optimized(&a));
    }

    #[test]
    fn is_valid_optimized_false_for_no_args() {
        let a = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        assert!(!is_valid_optimized(&a));
    }

    #[test]
    fn is_valid_optimized_false_for_int_arg() {
        let a = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::Int(123)],
        );
        assert!(!is_valid_optimized(&a));
    }

    #[test]
    fn is_valid_optimized_false_for_wrong_name() {
        let a = ann(
            "other",
            vec![reify_types::AnnotationArg::String("foo".to_string())],
        );
        assert!(!is_valid_optimized(&a));
    }

    /// Documents that only the *first* arg is tested — extra trailing args are ignored.
    #[test]
    fn is_valid_optimized_true_for_string_first_arg_with_trailing_args() {
        let a = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![
                reify_types::AnnotationArg::String("kernel::foo".to_string()),
                reify_types::AnnotationArg::Int(42),
            ],
        );
        assert!(is_valid_optimized(&a));
    }

    #[test]
    fn is_valid_optimized_false_for_bool_arg() {
        let a = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::Bool(true)],
        );
        assert!(!is_valid_optimized(&a));
    }
}
