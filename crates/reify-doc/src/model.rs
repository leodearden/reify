//! Pure-data DocModel surface for the `reify doc` tool.
//!
//! All types here are serde-serializable value objects with no dependency on
//! `reify-compiler`, `reify-syntax`, or `reify-types`. String fields carry
//! rendered/printable representations rather than typed AST nodes.

use serde::{Deserialize, Serialize};

/// Root documentation model for a set of compiled Reify modules.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocModel {
    pub modules: Vec<ModuleDoc>,
}

/// Documentation for a single compiled Reify module (fields expanded in later cycles).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModuleDoc {}

/// Documentation for a single `@annotation(...)` attached to a declaration.
///
/// Arguments are stored as rendered/printable strings — not typed AST values —
/// so `reify-doc` remains free of any dependency on `reify-syntax` or `reify-types`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnnotationDoc {
    /// The annotation name (e.g. `"deprecated"`, `"units"`).
    pub name: String,
    /// Rendered argument values (e.g. `["\"use foo instead\"", "since = \"1.0\""]`).
    pub args: Vec<String>,
}

/// Documentation for a single `#pragma(...)` attached to a declaration.
///
/// Like `AnnotationDoc`, arguments are rendered strings to avoid
/// coupling to `reify-syntax`'s `PragmaArg`/`PragmaValue` types.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PragmaDoc {
    /// The pragma name (e.g. `"inline"`, `"layout"`).
    pub name: String,
    /// Rendered argument values (e.g. `["always"]`, `["row", "3"]`).
    pub args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_model_serde_round_trip() {
        let model = DocModel { modules: Vec::new() };
        let json = serde_json::to_string(&model).expect("serialize");
        let back: DocModel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(model, back);
        assert!(back.modules.is_empty());
    }

    #[test]
    fn annotation_doc_serde_round_trip() {
        let ann = AnnotationDoc {
            name: "deprecated".to_string(),
            args: vec!["\"use foo instead\"".to_string(), "since = \"1.0\"".to_string()],
        };
        let json = serde_json::to_string(&ann).expect("serialize");
        let back: AnnotationDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ann, back);
        assert_eq!(back.args.len(), 2);
    }

    #[test]
    fn pragma_doc_serde_round_trip() {
        let pragma = PragmaDoc {
            name: "inline".to_string(),
            args: vec!["always".to_string()],
        };
        let json = serde_json::to_string(&pragma).expect("serialize");
        let back: PragmaDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(pragma, back);
        assert_eq!(back.args.len(), 1);
    }
}
