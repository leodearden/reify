//! Declarative annotation schema registry for `validate_annotations`.
//!
//! This module defines the `AnnotationSchema` type, a lazy-initialized registry
//! of all known annotations, and the `validate_via_schema` dispatcher that
//! replaces the per-annotation match-arm in `annotations.rs`.

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
