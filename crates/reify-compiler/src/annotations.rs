use super::*;

pub(crate) mod schema;

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
                        ExprKind::NumberLiteral { value, is_real } => {
                            // Int/Real classification (incl. integer-form overflow fallback) is
                            // shared with `compile_expr_guarded` via
                            // reify_syntax::classify_number_literal so the two sites cannot drift.
                            Some(match reify_syntax::classify_number_literal(*value, *is_real) {
                                reify_syntax::NumberClass::Int(i) => reify_types::AnnotationArg::Int(i),
                                reify_syntax::NumberClass::Real(f) => reify_types::AnnotationArg::Real(f),
                                // Mirror site: compile_expr_guarded in expr.rs handles LossyReal the same way.
                                reify_syntax::NumberClass::LossyReal(f) => {
                                    diagnostics.push(crate::diagnostics::lossy_real_warning(expr.span));
                                    reify_types::AnnotationArg::Real(f)
                                }
                            })
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
/// Dispatches to [`schema::validate_via_schema`], which consults the
/// `schema::ANNOTATION_REGISTRY` for the authoritative per-annotation
/// valid-context lists and arg-shape rules. See
/// `crates/reify-compiler/src/annotations/schema.rs` for the full listing.
pub(crate) fn validate_annotations(
    annotations: &[reify_types::Annotation],
    context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    schema::validate_via_schema(annotations, context, diagnostics);
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

/// Classification of a pragma name with respect to its validity context.
enum PragmaKind {
    /// Valid on block-level declarations (`#precision`, `#solver`, `#kernel`).
    KnownBlock,
    /// Valid only at module level; misplaced when found on a block (`#no_prelude`, `#version`).
    ModuleOnly,
    /// Not a recognized pragma name.
    Unknown,
}

/// Classify a pragma name for context-aware validation.
fn classify_pragma(name: &str) -> PragmaKind {
    if is_known_block_pragma(name) {
        PragmaKind::KnownBlock
    } else if is_module_only_pragma(name) {
        PragmaKind::ModuleOnly
    } else {
        PragmaKind::Unknown
    }
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
        match classify_pragma(&pragma.name) {
            PragmaKind::KnownBlock => {} // Valid here — no warning.
            PragmaKind::ModuleOnly => {
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
            }
            PragmaKind::Unknown => {
                diagnostics.push(
                    Diagnostic::warning(format!("unknown pragma #{}", pragma.name))
                        .with_label(DiagnosticLabel::new(pragma.span, "unknown pragma")),
                );
            }
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
                "preferred_strategy" => SolverHintKind::PreferredStrategy,
                other => {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "unknown solver hint kind '{other}'; expected 'discrete_set', 'prefer_stock', or 'preferred_strategy'"
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

/// Validate that every `discrete_set` / `prefer_stock` solver hint's collection
/// identifier resolves to a known name in `scope` or `functions`.
///
/// For each hint whose kind is **not** `PreferredStrategy`, the collection
/// identifier is looked up first in the member scope (`scope.resolve`) and
/// then in the module-level function list (`functions.iter().any(|f| f.name == …)`).
/// If neither lookup succeeds an `Error` diagnostic is pushed with the wording
/// `"unresolved name: <ident>"` / label `"not found in scope"`, matching the
/// wording used by `compile_expr`'s `Ident` arm so that the message substring
/// `"unresolved name"` is consistent in error-message assertions.
///
/// **Intentional subset of `compile_expr` resolution:** this validator does *not*
/// check `scope.collection_sub_names` (structural sub-component list names —
/// not valid stock-value-set payloads) or `resolve_builtin_constant` (`pi`/`tau`,
/// which are `Real`-typed scalars and therefore never valid `List`-typed hint
/// payloads).  If either of those becomes a valid hint-payload target in a future
/// PRD the checks should be extended here.
///
/// **Type-checking is not performed:** the validator only confirms that the name
/// exists; it does not verify that the resolved entity is `List`-typed.
/// Type validation is intentionally deferred to a later compiler pass (see
/// follow-up noted in task 2334).
///
/// **Severity — Error vs. Warning:** an unresolved-name diagnostic is escalated
/// to `Error` because solver back-ends cannot recover from a missing collection
/// at run time.  By contrast, an unknown-kind hint emitted by `extract_solver_hints`
/// can be safely dropped and is therefore only a `Warning`.
///
/// `PreferredStrategy` hints are intentionally exempt: spec §12.2 states that
/// any identifier is accepted at compile time and the back-end emits a runtime
/// warning for unrecognised strategy names.
pub(crate) fn validate_solver_hint_collections(
    hints: &[SolverHint],
    scope: &CompilationScope,
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for hint in hints {
        if hint.kind == SolverHintKind::PreferredStrategy {
            continue;
        }
        let name = &hint.collection;
        if scope.resolve(name).is_none() && !functions.iter().any(|f| f.name == *name) {
            diagnostics.push(
                Diagnostic::error(format!("unresolved name: {}", name))
                    .with_code(DiagnosticCode::UnresolvedName)
                    .with_label(DiagnosticLabel::new(hint.span, "not found in scope")),
            );
        }
    }
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

    // ── validate_solver_hint_collections unit tests ──────────────────────────

    fn make_hint(kind: SolverHintKind, collection: &str) -> SolverHint {
        SolverHint {
            kind,
            collection: collection.to_string(),
            span: reify_types::SourceSpan::empty(0),
        }
    }

    /// PreferredStrategy hints are exempt from collection name validation.
    #[test]
    fn validate_collections_skips_preferred_strategy() {
        let hints = vec![make_hint(
            SolverHintKind::PreferredStrategy,
            "bogus_xyz_strategy",
        )];
        let scope = CompilationScope::new("Test");
        let functions: &[CompiledFunction] = &[];
        let mut diagnostics = Vec::new();
        validate_solver_hint_collections(&hints, &scope, functions, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "PreferredStrategy should produce no diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// Any name registered in scope is accepted regardless of the resolved type.
    ///
    /// The validator checks name presence only — it does not inspect the
    /// resolved type at all.  Using a non-`List` type (`Type::Real`) here makes
    /// that contract visually obvious: if the type were inspected, this test
    /// would produce a diagnostic and fail.
    #[test]
    fn validate_collections_accepts_name_in_scope() {
        let hints = vec![make_hint(SolverHintKind::DiscreteSet, "my_collection")];
        let mut scope = CompilationScope::new("Test");
        scope.register("my_collection", Type::Real);
        let functions: &[CompiledFunction] = &[];
        let mut diagnostics = Vec::new();
        validate_solver_hint_collections(&hints, &scope, functions, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "name in scope should produce no diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// A name resolvable only via the `functions` list (not in scope) is accepted.
    ///
    /// Exercises the second lookup branch in `validate_solver_hint_collections`:
    /// `scope.resolve` returns `None` (scope is empty) but
    /// `functions.iter().any(|f| f.name == *name)` succeeds because the function
    /// list contains an entry whose name matches the hint collection.  This
    /// confirms that the scope and function lookups are independent fallbacks.
    #[test]
    fn validate_collections_accepts_name_via_functions() {
        let hints = vec![make_hint(SolverHintKind::DiscreteSet, "fn_collection")];
        let scope = CompilationScope::new("Test");
        let stub_fn = CompiledFunction {
            name: "fn_collection".to_string(),
            is_pub: false,
            params: vec![],
            param_defaults: Vec::new(),
            return_type: Type::Real,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: reify_types::CompiledExpr::literal(
                    reify_types::Value::Real(0.0),
                    Type::Real,
                ),
            },
            content_hash: reify_types::ContentHash::of_str("fn_collection_stub"),
            annotations: vec![],
            optimized_target: None,
        };
        let functions = &[stub_fn];
        let mut diagnostics = Vec::new();
        validate_solver_hint_collections(&hints, &scope, functions, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "name in functions should produce no diagnostics, got: {:?}",
            diagnostics
        );
    }

    /// An unresolvable discrete_set collection name emits an Error diagnostic.
    #[test]
    fn validate_collections_errors_on_unresolvable_name() {
        let hints = vec![make_hint(SolverHintKind::DiscreteSet, "ghost_collection")];
        let scope = CompilationScope::new("Test");
        let functions: &[CompiledFunction] = &[];
        let mut diagnostics = Vec::new();
        validate_solver_hint_collections(&hints, &scope, functions, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "expected exactly 1 diagnostic");
        let d = &diagnostics[0];
        assert_eq!(d.severity, reify_types::Severity::Error, "should be Error");
        assert!(
            d.message.contains("unresolved name") && d.message.contains("ghost_collection"),
            "unexpected message: {}",
            d.message
        );
    }

}
