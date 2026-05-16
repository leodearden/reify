//! Declarative annotation schema registry for `validate_annotations`.
//!
//! This module defines the [`AnnotationSchema`] type, a const-initialized slice
//! of all known annotations (`SCHEMAS`), and the [`validate_via_schema`] dispatcher
//! that replaces the per-annotation match-arm in `annotations.rs`.
//!
//! ## Phase-1 hybrid: registry + per-annotation helpers
//!
//! The schema struct is pure declarative data (valid contexts, staged fields for
//! later phases). Per-annotation arg-shape rules live in three private helper
//! functions (`check_optimized_args`, `check_shell_args`, `check_solid_args`)
//! stored as fn-pointer fields (`arg_check`) on each schema entry and dispatched
//! via `if let Some(check) = schema.arg_check` in [`validate_via_schema`]. This is
//! a deliberate Phase-1 trade-off: the registry centralises cross-cutting metadata
//! while the helpers preserve bit-for-bit wording from the legacy match-arm.
//! Later phases (δ, ζ, η) can migrate rules into the schema struct.
//!
//! See `docs/prds/annotation-args.md` §4 (Phase-1 scope) for the full rationale.

use reify_types::{Annotation, Diagnostic, DiagnosticLabel};

// ─── Schema types ────────────────────────────────────────────────────────────

/// Evaluation time of an annotation argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Phase-1 scaffold; consumed in later phases (δ, ζ, ι).
pub(crate) enum EvalTime {
    /// Argument must be a compile-time constant expression.
    CompileConst,
    /// Argument may be deferred to materialization time.
    AtMaterialization,
}

/// Policy for extra positional arguments beyond those declared in `args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Phase-1 scaffold; `Error` variant consumed in later phases.
pub(crate) enum ExtraArgsPolicy {
    /// Extra arguments are an error.
    Error,
    /// Extra arguments emit a warning and are ignored.
    WarnIgnore,
}

/// Argument type for a declared annotation parameter.
///
/// Phase-1 subset per PRD §4. `Field<X,Y>` is deferred to task ι.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Phase-1 scaffold; consumed in later phases (δ, ζ, ι).
pub(crate) enum ArgType {
    String,
    Int,
    Real,
    Bool,
    /// Length-typed numeric (placeholder — dispatched as `Int | Real` until ι).
    Length,
    Any,
}

/// Declaration of a single named annotation parameter.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase-1 scaffold; fields consumed in later phases (δ, ζ, ι).
pub(crate) struct ArgSchema {
    /// Parameter name (for named-arg syntax in future phases).
    pub(crate) name: &'static str,
    /// 0-based positional index.
    pub(crate) positional_index: usize,
    /// Whether this argument is required.
    pub(crate) required: bool,
    /// Expected argument type.
    pub(crate) ty: ArgType,
    /// When this argument is evaluated.
    pub(crate) eval_time: EvalTime,
}

/// Registry entry for a single known annotation.
///
/// The `name` field carries the canonical lowercase spelling (e.g. `"test"`).
/// `valid_contexts` lists the `context` strings (passed to `validate_annotations`)
/// on which this annotation is allowed.
#[derive(Debug)]
#[allow(dead_code)] // `args`, `flag_set`, `on_extra` are Phase-1 scaffold; used in later phases.
pub(crate) struct AnnotationSchema {
    /// Canonical annotation name (e.g. `"test"`, `"deprecated"`).
    pub(crate) name: &'static str,
    /// Contexts in which this annotation is valid.
    pub(crate) valid_contexts: &'static [&'static str],
    /// Declared argument schemas (Phase-1 mostly empty — arg-shape rules live
    /// in per-annotation helpers until later phases migrate them here).
    pub(crate) args: &'static [ArgSchema],
    /// Optional flag-set (e.g. for `@allow(shadowing)` in task β). `None` in Phase 1.
    pub(crate) flag_set: Option<&'static [&'static str]>,
    /// Policy for extra positional arguments beyond `args`.
    pub(crate) on_extra: ExtraArgsPolicy,
    /// `@<name>` label string used in context-mismatch diagnostics.
    /// Stored as a `&'static str` literal to eliminate the `format!("@{}", name)`
    /// formatting and to pin the label to a single source of truth alongside `name`.
    pub(crate) label: &'static str,
    /// Per-annotation arg-shape checker. `None` for annotations with no arg rules.
    /// Unified signature `fn(&Annotation, &str, &mut Vec<Diagnostic>)` so all helpers
    /// share a single fn-pointer type; helpers that don't need `context` accept `_context`.
    pub(crate) arg_check: Option<fn(&Annotation, &str, &mut Vec<Diagnostic>)>,
}

// ─── Registry ────────────────────────────────────────────────────────────────

/// All contexts that `validate_annotations` can be called with, as observed
/// across the 9 call-sites in entity.rs, traits.rs, functions.rs, guards.rs,
/// and compile_builder/defs_phase.rs.
///
/// Used as the `valid_contexts` for `@deprecated`, which is valid everywhere.
const ALL_VALID_CONTEXTS: &[&str] = &[
    "structure",
    "occurrence",
    "function",
    "constraint_def",
    "trait",
    "purpose",
    "param",
    "let",
    "field",
];

/// Const-initialized slice of all known annotation schemas.
///
/// Linear scan over n=6 entries is faster than HashMap probing at this scale
/// (no hash computation, no OnceLock barrier, cache-friendly layout). Adding a
/// new annotation requires only a new entry here — no separate `build_registry`
/// edit needed.
const SCHEMAS: &[AnnotationSchema] = &[
    // @test — valid on structure, occurrence, function, constraint_def
    AnnotationSchema {
        name: reify_types::TEST_ANNOTATION,
        label: "@test",
        valid_contexts: &["structure", "occurrence", "function", "constraint_def"],
        args: &[],
        flag_set: None,
        on_extra: ExtraArgsPolicy::WarnIgnore,
        arg_check: None,
    },
    // @deprecated — valid on any context
    AnnotationSchema {
        name: reify_types::DEPRECATED_ANNOTATION,
        label: "@deprecated",
        valid_contexts: ALL_VALID_CONTEXTS,
        args: &[],
        flag_set: None,
        on_extra: ExtraArgsPolicy::WarnIgnore,
        arg_check: None,
    },
    // @optimized — valid on structure, occurrence, constraint_def, function
    AnnotationSchema {
        name: reify_types::OPTIMIZED_ANNOTATION,
        label: "@optimized",
        valid_contexts: &["structure", "occurrence", "constraint_def", "function"],
        args: &[],
        flag_set: None,
        on_extra: ExtraArgsPolicy::WarnIgnore,
        arg_check: Some(check_optimized_args),
    },
    // @solver_hint — valid on structure, occurrence, param, let
    AnnotationSchema {
        name: reify_types::SOLVER_HINT_ANNOTATION,
        label: "@solver_hint",
        valid_contexts: &["structure", "occurrence", "param", "let"],
        args: &[],
        flag_set: None,
        on_extra: ExtraArgsPolicy::WarnIgnore,
        arg_check: None,
    },
    // @shell — valid on structure, occurrence (zero or one Length-typed thickness arg)
    AnnotationSchema {
        name: reify_types::SHELL_ANNOTATION,
        label: "@shell",
        valid_contexts: &["structure", "occurrence"],
        args: &[],
        flag_set: None,
        on_extra: ExtraArgsPolicy::WarnIgnore,
        arg_check: Some(check_shell_args),
    },
    // @solid — valid on structure, occurrence (bare marker — no args)
    AnnotationSchema {
        name: reify_types::SOLID_ANNOTATION,
        label: "@solid",
        valid_contexts: &["structure", "occurrence"],
        args: &[],
        flag_set: None,
        // WarnIgnore matches the Warning severity emitted by check_solid_args.
        // Error is reserved for a future phase that intentionally upgrades severity.
        on_extra: ExtraArgsPolicy::WarnIgnore,
        arg_check: Some(check_solid_args),
    },
];

/// Look up the schema for a known annotation by its canonical name.
///
/// Returns `None` for names that are not registered (i.e. unknown annotations).
/// Linear scan over the 6-entry `SCHEMAS` const slice — faster than HashMap
/// at this scale (no hashing, no OnceLock barrier).
pub(crate) fn lookup_schema(name: &str) -> Option<&'static AnnotationSchema> {
    SCHEMAS.iter().find(|s| s.name == name)
}

// ─── Per-annotation arg-shape helpers ────────────────────────────────────────

/// Check @optimized arg shape on contexts where the target string is consumed
/// (`constraint_def` and `function`). Mirrors annotations.rs:114-133 verbatim.
fn check_optimized_args(ann: &Annotation, context: &str, diagnostics: &mut Vec<Diagnostic>) {
    if matches!(context, "constraint_def" | "function") && !super::is_valid_optimized(ann) {
        diagnostics.push(
            Diagnostic::warning(
                "annotation @optimized requires a string literal target, \
                 e.g. @optimized(\"kernel::foo\")"
                    .to_string(),
            )
            .with_label(DiagnosticLabel::new(ann.span, "@optimized missing target")),
        );
    }
}

/// Check @shell arg shape. Mirrors annotations.rs:153-188 verbatim.
///
/// Only called when context is valid (structure/occurrence); the caller's
/// `else` branch enforces the short-circuit so this never fires on wrong-context.
///
/// `_context` is unused but required for the uniform fn-pointer signature
/// `fn(&Annotation, &str, &mut Vec<Diagnostic>)`.
fn check_shell_args(ann: &Annotation, _context: &str, diagnostics: &mut Vec<Diagnostic>) {
    match ann.args.as_slice() {
        [] => {} // bare @shell — defer thickness to medial analysis.
        [
            reify_types::AnnotationArg::Int(_)
            | reify_types::AnnotationArg::Real(_),
        ] => {}
        [_] => {
            diagnostics.push(
                Diagnostic::warning(
                    "@shell thickness argument must be a numeric literal, \
                     e.g. @shell(0.5)"
                        .to_string(),
                )
                .with_label(DiagnosticLabel::new(ann.span, "non-numeric thickness")),
            );
        }
        _ => {
            diagnostics.push(
                Diagnostic::warning(
                    "@shell accepts at most one argument (thickness); \
                     extra arguments will be ignored"
                        .to_string(),
                )
                .with_label(DiagnosticLabel::new(ann.span, "too many arguments")),
            );
        }
    }
}

/// Check @solid arg shape. Mirrors annotations.rs:198-205 verbatim.
///
/// Only called when context is valid (structure/occurrence); the caller's
/// `else` branch enforces the short-circuit so this never fires on wrong-context.
///
/// `_context` is unused but required for the uniform fn-pointer signature
/// `fn(&Annotation, &str, &mut Vec<Diagnostic>)`.
fn check_solid_args(ann: &Annotation, _context: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !ann.args.is_empty() {
        diagnostics.push(
            Diagnostic::warning(
                "@solid takes no arguments; force-tet is unconditional".to_string(),
            )
            .with_label(DiagnosticLabel::new(ann.span, "@solid takes no arguments")),
        );
    }
}

/// Slice-level duplicate-@optimized pass. Mirrors annotations.rs:236-252 verbatim.
///
/// Only fires in `constraint_def` and `function` contexts (the two that consume
/// `optimized_target` downstream). Tracks the first *valid* @optimized seen;
/// every subsequent valid @optimized emits a "duplicate" warning. Malformed
/// entries (those where `is_valid_optimized` returns `false`) are excluded from
/// the "seen" count so they don't trigger contradictory diagnostics.
fn duplicate_optimized_check(
    annotations: &[Annotation],
    context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !matches!(context, "constraint_def" | "function") {
        return;
    }
    let mut seen_valid_optimized = false;
    for ann in annotations {
        if super::is_valid_optimized(ann) {
            if seen_valid_optimized {
                diagnostics.push(
                    Diagnostic::warning(
                        "multiple @optimized annotations on the same declaration \
                         — only the first well-formed one is used"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(ann.span, "duplicate @optimized")),
                );
            }
            seen_valid_optimized = true;
        }
    }
}

// ─── Dispatcher ──────────────────────────────────────────────────────────────

/// Validate a slice of compiled annotations against the schema registry.
///
/// For each annotation:
/// - If the name has no registry entry, emit an "unknown annotation @<name>" warning.
/// - Else if the context is not in the schema's `valid_contexts`, emit a
///   context-mismatch warning with the byte-identical wording from the legacy
///   match-arm in `annotations.rs`.
/// - Otherwise, dispatch into per-annotation arg-shape helpers (added in later steps).
///
/// After the per-annotation loop, run the slice-level duplicate-@optimized pass
/// (added in step-8).
pub(crate) fn validate_via_schema(
    annotations: &[Annotation],
    context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for ann in annotations {
        match lookup_schema(&ann.name) {
            None => {
                // Unknown annotation — mirrors legacy `other` arm (annotations.rs:207-212).
                diagnostics.push(
                    Diagnostic::warning(format!("unknown annotation @{}", ann.name))
                        .with_label(DiagnosticLabel::new(ann.span, "unknown annotation")),
                );
            }
            Some(schema) => {
                if !schema.valid_contexts.contains(&context) {
                    // Context mismatch — mirrors each legacy annotation's context arm.
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "annotation @{} is not valid on {context} declarations",
                            schema.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            ann.span,
                            schema.label, // &'static str — eliminates format!("@{}", name) per warning
                        )),
                    );
                } else {
                    // Valid context — dispatch into per-annotation arg-shape helper via
                    // fn-pointer field. The `else` enforces the short-circuit: arg-shape
                    // warnings must not fire when the context is wrong (mirrors `else if`
                    // in legacy arms).
                    if let Some(check) = schema.arg_check {
                        check(ann, context, diagnostics);
                    }
                }
            }
        }
    }
    // Slice-level duplicate-@optimized pass (mirrors annotations.rs:236-252).
    duplicate_optimized_check(annotations, context, diagnostics);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test helpers ─────────────────────────────────────────────────────────

    fn ann(name: &str, args: Vec<reify_types::AnnotationArg>) -> reify_types::Annotation {
        ann_at(name, args, 0)
    }

    fn ann_at(
        name: &str,
        args: Vec<reify_types::AnnotationArg>,
        offset: u32,
    ) -> reify_types::Annotation {
        reify_types::Annotation {
            name: name.to_string(),
            args,
            span: reify_types::SourceSpan::empty(offset),
        }
    }

    // ── Registry lookup tests ────────────────────────────────────────────────

    #[test]
    fn lookup_test_annotation() {
        let schema = lookup_schema("test").expect("expected Some for 'test'");
        let valid = schema.valid_contexts;
        assert!(valid.contains(&"structure"), "structure missing");
        assert!(valid.contains(&"occurrence"), "occurrence missing");
        assert!(valid.contains(&"function"), "function missing");
        assert!(valid.contains(&"constraint_def"), "constraint_def missing");
        assert_eq!(valid.len(), 4, "expected exactly 4 valid contexts");
    }

    #[test]
    fn lookup_deprecated_annotation() {
        let schema = lookup_schema("deprecated").expect("expected Some for 'deprecated'");
        let valid = schema.valid_contexts;
        // @deprecated is valid on any context; verify each known call-site context
        for ctx in &[
            "structure",
            "occurrence",
            "function",
            "constraint_def",
            "trait",
            "purpose",
            "param",
            "let",
            "field",
        ] {
            assert!(
                valid.contains(ctx),
                "context '{}' missing from @deprecated",
                ctx
            );
        }
        assert!(!valid.is_empty(), "valid_contexts should not be empty");
    }

    #[test]
    fn lookup_optimized_annotation() {
        let schema = lookup_schema("optimized").expect("expected Some for 'optimized'");
        let valid = schema.valid_contexts;
        assert!(valid.contains(&"structure"), "structure missing");
        assert!(valid.contains(&"occurrence"), "occurrence missing");
        assert!(valid.contains(&"constraint_def"), "constraint_def missing");
        assert!(valid.contains(&"function"), "function missing");
        assert_eq!(valid.len(), 4, "expected exactly 4 valid contexts");
    }

    #[test]
    fn lookup_solver_hint_annotation() {
        let schema = lookup_schema("solver_hint").expect("expected Some for 'solver_hint'");
        let valid = schema.valid_contexts;
        assert!(valid.contains(&"structure"), "structure missing");
        assert!(valid.contains(&"occurrence"), "occurrence missing");
        assert!(valid.contains(&"param"), "param missing");
        assert!(valid.contains(&"let"), "let missing");
        assert_eq!(valid.len(), 4, "expected exactly 4 valid contexts");
    }

    #[test]
    fn lookup_shell_annotation() {
        let schema = lookup_schema("shell").expect("expected Some for 'shell'");
        let valid = schema.valid_contexts;
        assert!(valid.contains(&"structure"), "structure missing");
        assert!(valid.contains(&"occurrence"), "occurrence missing");
        assert_eq!(valid.len(), 2, "expected exactly 2 valid contexts");
    }

    #[test]
    fn lookup_solid_annotation() {
        let schema = lookup_schema("solid").expect("expected Some for 'solid'");
        let valid = schema.valid_contexts;
        assert!(valid.contains(&"structure"), "structure missing");
        assert!(valid.contains(&"occurrence"), "occurrence missing");
        assert_eq!(valid.len(), 2, "expected exactly 2 valid contexts");
    }

    #[test]
    fn lookup_nonexistent_returns_none() {
        assert!(
            lookup_schema("nonexistent_xyz").is_none(),
            "expected None for unknown annotation"
        );
    }

    // ── validate_via_schema: context-mismatch tests ──────────────────────────

    /// @test on an invalid context emits exactly one warning with the
    /// byte-identical message from the legacy match-arm.
    #[test]
    fn validate_test_on_invalid_context_emits_warning() {
        let a = ann(reify_types::TEST_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "param", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert_eq!(
            diags[0].message,
            "annotation @test is not valid on param declarations",
            "unexpected message"
        );
        assert_eq!(diags[0].labels.len(), 1);
        assert_eq!(diags[0].labels[0].message, "@test", "unexpected label");
        assert_eq!(diags[0].labels[0].span, a.span, "label span must equal ann span");
    }

    /// @optimized on an invalid context emits the same wording as the legacy arm.
    #[test]
    fn validate_optimized_on_invalid_context_emits_warning() {
        let a = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "param", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert_eq!(
            diags[0].message,
            "annotation @optimized is not valid on param declarations"
        );
        assert_eq!(diags[0].labels[0].message, "@optimized");
        assert_eq!(diags[0].labels[0].span, a.span);
    }

    /// @solver_hint on "function" (not in its valid set) emits one warning.
    #[test]
    fn validate_solver_hint_on_invalid_context_emits_warning() {
        let a = ann(reify_types::SOLVER_HINT_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "function", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert_eq!(
            diags[0].message,
            "annotation @solver_hint is not valid on function declarations"
        );
        assert_eq!(diags[0].labels[0].message, "@solver_hint");
        assert_eq!(diags[0].labels[0].span, a.span);
    }

    /// @shell on "function" emits one context-mismatch warning.
    #[test]
    fn validate_shell_on_invalid_context_emits_warning() {
        let a = ann(reify_types::SHELL_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "function", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert_eq!(
            diags[0].message,
            "annotation @shell is not valid on function declarations"
        );
        assert_eq!(diags[0].labels[0].message, "@shell");
        assert_eq!(diags[0].labels[0].span, a.span);
    }

    /// @solid on "function" emits one context-mismatch warning.
    #[test]
    fn validate_solid_on_invalid_context_emits_warning() {
        let a = ann(reify_types::SOLID_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "function", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert_eq!(
            diags[0].message,
            "annotation @solid is not valid on function declarations"
        );
        assert_eq!(diags[0].labels[0].message, "@solid");
        assert_eq!(diags[0].labels[0].span, a.span);
    }

    /// Empty annotation slice produces zero diagnostics.
    #[test]
    fn validate_empty_slice_produces_no_diagnostics() {
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(&[], "structure", &mut diags);
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    /// @deprecated on any context produces zero diagnostics.
    #[test]
    fn validate_deprecated_on_any_context_produces_no_diagnostics() {
        for ctx in ["structure", "occurrence", "function", "constraint_def", "param", "let"] {
            let a = ann(reify_types::DEPRECATED_ANNOTATION, vec![]);
            let mut diags: Vec<reify_types::Diagnostic> = vec![];
            validate_via_schema(std::slice::from_ref(&a), ctx, &mut diags);
            assert!(
                diags.is_empty(),
                "context={ctx}: expected no diagnostics, got: {:?}",
                diags
            );
        }
    }

    /// Unknown annotation name emits "unknown annotation @<name>" warning.
    #[test]
    fn validate_unknown_annotation_emits_warning() {
        let a = ann("future_annotation", vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert_eq!(
            diags[0].message,
            "unknown annotation @future_annotation",
            "unexpected message"
        );
        assert_eq!(diags[0].labels[0].message, "unknown annotation");
        assert_eq!(diags[0].labels[0].span, a.span);
    }

    /// @test on a valid context (structure) produces zero diagnostics.
    #[test]
    fn validate_test_on_valid_context_produces_no_diagnostics() {
        let a = ann(reify_types::TEST_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert!(
            diags.is_empty(),
            "expected no diagnostics on valid context, got: {:?}",
            diags
        );
    }

    // ── validate_via_schema: @optimized arg-shape tests ─────────────────────

    /// @optimized on constraint_def with no args → missing-target warning.
    #[test]
    fn validate_optimized_no_args_on_constraint_def_warns() {
        let a = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "constraint_def", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("requires a string literal target"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "@optimized missing target");
    }

    /// @optimized on function with no args → missing-target warning.
    #[test]
    fn validate_optimized_no_args_on_function_warns() {
        let a = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "function", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("requires a string literal target"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "@optimized missing target");
    }

    /// @optimized on structure/occurrence with no args → zero diagnostics.
    #[test]
    fn validate_optimized_no_args_on_structure_occurrence_produces_no_diagnostics() {
        for ctx in ["structure", "occurrence"] {
            let a = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
            let mut diags: Vec<reify_types::Diagnostic> = vec![];
            validate_via_schema(std::slice::from_ref(&a), ctx, &mut diags);
            assert!(
                diags.is_empty(),
                "context={ctx}: expected no diagnostics for bare @optimized, got: {:?}",
                diags
            );
        }
    }

    /// @optimized with [String("k::f")] on constraint_def → zero diagnostics.
    #[test]
    fn validate_optimized_with_string_arg_on_constraint_def_produces_no_diagnostics() {
        let a = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("k::f".to_string())],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "constraint_def", &mut diags);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for well-formed @optimized, got: {:?}",
            diags
        );
    }

    // ── validate_via_schema: @shell arg-shape tests ──────────────────────────

    /// @shell on structure with [] → 0 diagnostics.
    #[test]
    fn validate_shell_bare_on_structure_produces_no_diagnostics() {
        let a = ann(reify_types::SHELL_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert!(diags.is_empty(), "expected no diags, got: {:?}", diags);
    }

    /// @shell with [Real(0.5)] → 0 diagnostics.
    #[test]
    fn validate_shell_real_arg_produces_no_diagnostics() {
        let a = ann(
            reify_types::SHELL_ANNOTATION,
            vec![reify_types::AnnotationArg::Real(0.5)],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert!(diags.is_empty(), "expected no diags, got: {:?}", diags);
    }

    /// @shell with [Int(2)] → 0 diagnostics.
    #[test]
    fn validate_shell_int_arg_produces_no_diagnostics() {
        let a = ann(
            reify_types::SHELL_ANNOTATION,
            vec![reify_types::AnnotationArg::Int(2)],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "occurrence", &mut diags);
        assert!(diags.is_empty(), "expected no diags, got: {:?}", diags);
    }

    /// @shell with [String("thick")] → 1 warning containing "must be a numeric literal".
    #[test]
    fn validate_shell_non_numeric_arg_warns() {
        let a = ann(
            reify_types::SHELL_ANNOTATION,
            vec![reify_types::AnnotationArg::String("thick".to_string())],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("must be a numeric literal"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "non-numeric thickness");
    }

    /// @shell with [Real(0.5), Real(0.6)] → 1 warning containing "at most one argument".
    #[test]
    fn validate_shell_extra_args_warn() {
        let a = ann(
            reify_types::SHELL_ANNOTATION,
            vec![
                reify_types::AnnotationArg::Real(0.5),
                reify_types::AnnotationArg::Real(0.6),
            ],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("at most one argument"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "too many arguments");
    }

    /// @shell on invalid context with arg → exactly one diagnostic (context-mismatch only).
    /// The arg-shape check must NOT fire when context is wrong — short-circuit verified.
    #[test]
    fn validate_shell_on_invalid_context_with_arg_emits_only_context_mismatch() {
        let a = ann(
            reify_types::SHELL_ANNOTATION,
            vec![reify_types::AnnotationArg::String("x".to_string())],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "function", &mut diags);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly 1 diagnostic (context-mismatch only), got: {:?}",
            diags
        );
        assert!(
            diags[0].message.contains("@shell is not valid on function"),
            "expected context-mismatch message, got: {}",
            diags[0].message
        );
    }

    // ── validate_via_schema: @solid arg-shape tests ──────────────────────────

    /// @solid on structure with [] → 0 diagnostics.
    #[test]
    fn validate_solid_bare_on_structure_produces_no_diagnostics() {
        let a = ann(reify_types::SOLID_ANNOTATION, vec![]);
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
        assert!(diags.is_empty(), "expected no diags, got: {:?}", diags);
    }

    /// Any arg passed to @solid on a valid context emits "takes no arguments".
    #[test]
    fn validate_solid_with_any_arg_on_valid_context_warns() {
        let arg_shapes: &[(&str, Vec<reify_types::AnnotationArg>)] = &[
            ("Real(0.5)", vec![reify_types::AnnotationArg::Real(0.5)]),
            ("Int(2)", vec![reify_types::AnnotationArg::Int(2)]),
            (
                "String(foo)",
                vec![reify_types::AnnotationArg::String("foo".into())],
            ),
            ("Bool(true)", vec![reify_types::AnnotationArg::Bool(true)]),
            (
                "Ident(id)",
                vec![reify_types::AnnotationArg::Ident("ident".into())],
            ),
            (
                "two reals",
                vec![
                    reify_types::AnnotationArg::Real(0.5),
                    reify_types::AnnotationArg::Real(0.6),
                ],
            ),
        ];
        for (label, args) in arg_shapes {
            let a = ann(reify_types::SOLID_ANNOTATION, args.clone());
            let mut diags: Vec<reify_types::Diagnostic> = vec![];
            validate_via_schema(std::slice::from_ref(&a), "structure", &mut diags);
            assert_eq!(
                diags.len(),
                1,
                "arg shape {label}: expected exactly 1 diagnostic, got: {:?}",
                diags
            );
            assert!(
                diags[0].message.contains("takes no arguments"),
                "arg shape {label}: unexpected message: {}",
                diags[0].message
            );
            assert_eq!(
                diags[0].labels[0].message,
                "@solid takes no arguments",
                "arg shape {label}: unexpected label"
            );
        }
    }

    /// @solid on invalid context with arg → exactly one diagnostic (context-mismatch only).
    #[test]
    fn validate_solid_on_invalid_context_with_arg_emits_only_context_mismatch() {
        let a = ann(
            reify_types::SOLID_ANNOTATION,
            vec![reify_types::AnnotationArg::Real(0.5)],
        );
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(std::slice::from_ref(&a), "function", &mut diags);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly 1 diagnostic (context-mismatch only), got: {:?}",
            diags
        );
        assert!(
            diags[0].message.contains("@solid is not valid on function"),
            "expected context-mismatch message, got: {}",
            diags[0].message
        );
    }

    // ── validate_via_schema: duplicate @optimized slice-level pass ───────────

    /// Two valid @optimized on constraint_def → exactly one duplicate warning
    /// attached to the SECOND annotation's span.
    #[test]
    fn duplicate_valid_optimized_on_constraint_def_warns_on_second() {
        let a1 = ann_at(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("a".to_string())],
            0,
        );
        let a2 = ann_at(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("b".to_string())],
            10,
        );
        let anns = vec![a1, a2.clone()];
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(&anns, "constraint_def", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("multiple @optimized annotations"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "duplicate @optimized");
        assert_eq!(
            diags[0].labels[0].span,
            a2.span,
            "duplicate warning must be on the second annotation's span"
        );
    }

    /// Two valid @optimized on function → exactly one duplicate warning on the second.
    #[test]
    fn duplicate_valid_optimized_on_function_warns_on_second() {
        let a1 = ann_at(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("a".to_string())],
            0,
        );
        let a2 = ann_at(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("b".to_string())],
            10,
        );
        let anns = vec![a1, a2.clone()];
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(&anns, "function", &mut diags);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("multiple @optimized annotations"),
            "unexpected message: {}",
            diags[0].message
        );
        assert_eq!(diags[0].labels[0].message, "duplicate @optimized");
        assert_eq!(diags[0].labels[0].span, a2.span);
    }

    /// Two valid @optimized on structure → zero duplicate warnings
    /// (slice-level pass does NOT fire outside constraint_def/function).
    #[test]
    fn duplicate_valid_optimized_on_structure_produces_no_duplicate_warning() {
        let a1 = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("a".to_string())],
        );
        let a2 = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("b".to_string())],
        );
        let anns = vec![a1, a2];
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(&anns, "structure", &mut diags);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for @optimized on structure, got: {:?}",
            diags
        );
    }

    /// Malformed @optimized() then valid @optimized("b") on constraint_def →
    /// exactly 1 missing-target warning, zero duplicate warnings.
    /// Malformed entries don't count toward seen_valid.
    #[test]
    fn malformed_then_valid_optimized_no_duplicate_warning() {
        let a_malformed = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        let a_valid = ann(
            reify_types::OPTIMIZED_ANNOTATION,
            vec![reify_types::AnnotationArg::String("b".to_string())],
        );
        let anns = vec![a_malformed, a_valid];
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(&anns, "constraint_def", &mut diags);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly 1 diagnostic (missing-target only), got: {:?}",
            diags
        );
        assert!(
            diags[0].message.contains("requires a string literal target"),
            "unexpected message: {}",
            diags[0].message
        );
    }

    /// Two malformed @optimized() on constraint_def → exactly 2 missing-target
    /// warnings, zero duplicate warnings.
    #[test]
    fn two_malformed_optimized_produces_two_missing_target_no_dup() {
        let a1 = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        let a2 = ann(reify_types::OPTIMIZED_ANNOTATION, vec![]);
        let anns = vec![a1, a2];
        let mut diags: Vec<reify_types::Diagnostic> = vec![];
        validate_via_schema(&anns, "constraint_def", &mut diags);
        assert_eq!(
            diags.len(),
            2,
            "expected exactly 2 missing-target diagnostics, got: {:?}",
            diags
        );
        for d in &diags {
            assert!(
                d.message.contains("requires a string literal target"),
                "unexpected message: {}",
                d.message
            );
        }
        // No duplicate warning should be present
        for d in &diags {
            assert!(
                !d.message.contains("multiple @optimized"),
                "unexpected duplicate warning: {}",
                d.message
            );
        }
    }

}
