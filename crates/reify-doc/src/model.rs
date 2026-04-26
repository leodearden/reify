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

    #[test]
    fn param_doc_serde_round_trip() {
        let param = ParamDoc {
            name: "width".to_string(),
            doc: Some("Width of the component.".to_string()),
            type_repr: "Length".to_string(),
            default_repr: Some("100 mm".to_string()),
            annotations: vec![AnnotationDoc {
                name: "units".to_string(),
                args: vec!["mm".to_string()],
            }],
        };
        let json = serde_json::to_string(&param).expect("serialize");
        let back: ParamDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(param, back);
        assert_eq!(back.annotations.len(), 1);
    }

    #[test]
    fn port_doc_serde_round_trip() {
        let port = PortDoc {
            name: "power_in".to_string(),
            direction: "in".to_string(),
            type_name: "Power".to_string(),
            members: vec!["voltage".to_string(), "current".to_string()],
        };
        let json = serde_json::to_string(&port).expect("serialize");
        let back: PortDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(port, back);
        assert_eq!(back.members.len(), 2);
    }

    #[test]
    fn constraint_doc_serde_round_trip() {
        let constraint = ConstraintDoc {
            label: Some("voltage_range".to_string()),
            expr_repr: "voltage >= 3.0 V && voltage <= 5.5 V".to_string(),
            annotations: vec![],
        };
        let json = serde_json::to_string(&constraint).expect("serialize");
        let back: ConstraintDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(constraint, back);
        assert!(back.annotations.is_empty());
    }

    #[test]
    fn sub_component_doc_serde_round_trip() {
        let sub = SubComponentDoc {
            name: "cpu".to_string(),
            structure_name: "MCU".to_string(),
            args: vec!["flash = 512 kB".to_string()],
            annotations: vec![AnnotationDoc {
                name: "supplier".to_string(),
                args: vec!["\"STMicro\"".to_string()],
            }],
        };
        let json = serde_json::to_string(&sub).expect("serialize");
        let back: SubComponentDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(sub, back);
        assert_eq!(back.args.len(), 1);
    }

    #[test]
    fn realization_doc_serde_round_trip() {
        let real = RealizationDoc {
            name: "SchematicView".to_string(),
            op_summaries: vec![
                "place cpu at (10, 20)".to_string(),
                "route power_in -> cpu.vcc".to_string(),
            ],
        };
        let json = serde_json::to_string(&real).expect("serialize");
        let back: RealizationDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(real, back);
        assert_eq!(back.op_summaries.len(), 2);
    }
}
