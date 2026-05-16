//! Declarative annotation schema registry for `validate_annotations`.
//!
//! This module defines the `AnnotationSchema` type, a lazy-initialized registry
//! of all known annotations, and the `validate_via_schema` dispatcher that
//! replaces the per-annotation match-arm in `annotations.rs`.

use std::collections::HashMap;
use std::sync::OnceLock;

// ─── Schema types ────────────────────────────────────────────────────────────

/// Evaluation time of an annotation argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvalTime {
    /// Argument must be a compile-time constant expression.
    CompileConst,
    /// Argument may be deferred to materialization time.
    AtMaterialization,
}

/// Policy for extra positional arguments beyond those declared in `args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

// ─── Registry ────────────────────────────────────────────────────────────────

/// Lazy-initialized annotation registry, keyed by canonical annotation name.
///
/// Seeded on first access via `OnceLock::get_or_init`. The pattern mirrors
/// `si_units.rs` and `stdlib_loader.rs` in this crate.
static ANNOTATION_REGISTRY: OnceLock<HashMap<&'static str, AnnotationSchema>> = OnceLock::new();

/// All contexts that `validate_annotations` can be called with, as observed
/// across the 9 call-sites in entity.rs, traits.rs, functions.rs, guards.rs,
/// and compile_builder/defs_phase.rs.
///
/// Used as the `valid_contexts` for `@deprecated`, which is valid everywhere.
static ALL_VALID_CONTEXTS: &[&str] = &[
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

fn build_registry() -> HashMap<&'static str, AnnotationSchema> {
    let mut map = HashMap::new();

    // @test — valid on structure, occurrence, function, constraint_def
    map.insert(
        reify_types::TEST_ANNOTATION,
        AnnotationSchema {
            name: reify_types::TEST_ANNOTATION,
            valid_contexts: &["structure", "occurrence", "function", "constraint_def"],
            args: &[],
            flag_set: None,
            on_extra: ExtraArgsPolicy::WarnIgnore,
        },
    );

    // @deprecated — valid on any context
    map.insert(
        reify_types::DEPRECATED_ANNOTATION,
        AnnotationSchema {
            name: reify_types::DEPRECATED_ANNOTATION,
            valid_contexts: ALL_VALID_CONTEXTS,
            args: &[],
            flag_set: None,
            on_extra: ExtraArgsPolicy::WarnIgnore,
        },
    );

    // @optimized — valid on structure, occurrence, constraint_def, function
    map.insert(
        reify_types::OPTIMIZED_ANNOTATION,
        AnnotationSchema {
            name: reify_types::OPTIMIZED_ANNOTATION,
            valid_contexts: &["structure", "occurrence", "constraint_def", "function"],
            args: &[],
            flag_set: None,
            on_extra: ExtraArgsPolicy::WarnIgnore,
        },
    );

    // @solver_hint — valid on structure, occurrence, param, let
    map.insert(
        reify_types::SOLVER_HINT_ANNOTATION,
        AnnotationSchema {
            name: reify_types::SOLVER_HINT_ANNOTATION,
            valid_contexts: &["structure", "occurrence", "param", "let"],
            args: &[],
            flag_set: None,
            on_extra: ExtraArgsPolicy::WarnIgnore,
        },
    );

    // @shell — valid on structure, occurrence (zero or one Length-typed thickness arg)
    map.insert(
        reify_types::SHELL_ANNOTATION,
        AnnotationSchema {
            name: reify_types::SHELL_ANNOTATION,
            valid_contexts: &["structure", "occurrence"],
            args: &[],
            flag_set: None,
            on_extra: ExtraArgsPolicy::WarnIgnore,
        },
    );

    // @solid — valid on structure, occurrence (bare marker — no args)
    map.insert(
        reify_types::SOLID_ANNOTATION,
        AnnotationSchema {
            name: reify_types::SOLID_ANNOTATION,
            valid_contexts: &["structure", "occurrence"],
            args: &[],
            flag_set: None,
            on_extra: ExtraArgsPolicy::Error,
        },
    );

    map
}

/// Look up the schema for a known annotation by its canonical name.
///
/// Returns `None` for names that are not registered (i.e. unknown annotations).
pub(crate) fn lookup_schema(name: &str) -> Option<&'static AnnotationSchema> {
    ANNOTATION_REGISTRY.get_or_init(build_registry).get(name)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
