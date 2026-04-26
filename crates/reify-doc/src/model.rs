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

/// Documentation for a parameter declaration on a structure or occurrence.
///
/// Field values are rendered/printable strings — `type_repr` holds a textual
/// rendering of the parameter's type (e.g. `"Length"`), and `default_repr`
/// holds a rendering of the default expression if present.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamDoc {
    /// Parameter name.
    pub name: String,
    /// Optional doc-comment extracted from the source.
    pub doc: Option<String>,
    /// Rendered type of the parameter (e.g. `"Length"`, `"Voltage"`).
    pub type_repr: String,
    /// Rendered default value expression, if any.
    pub default_repr: Option<String>,
    /// Annotations attached to this parameter declaration.
    pub annotations: Vec<AnnotationDoc>,
}

/// Documentation for a port declaration on a structure or occurrence.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PortDoc {
    /// Port name.
    pub name: String,
    /// Direction of the port (e.g. `"in"`, `"out"`, `"inout"`).
    pub direction: String,
    /// Rendered type name for the port interface (e.g. `"Power"`, `"Signal"`).
    pub type_name: String,
    /// Names of the port's member signals/nets, if any.
    pub members: Vec<String>,
}

/// Documentation for a constraint expression on a topology template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConstraintDoc {
    /// Optional user-given label for the constraint.
    pub label: Option<String>,
    /// Rendered constraint expression (e.g. `"voltage >= 3.0 V && voltage <= 5.5 V"`).
    pub expr_repr: String,
    /// Annotations attached to this constraint.
    pub annotations: Vec<AnnotationDoc>,
}

/// Documentation for a sub-component instantiation inside a topology template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubComponentDoc {
    /// Instance name within the parent template.
    pub name: String,
    /// Name of the structure or occurrence being instantiated.
    pub structure_name: String,
    /// Rendered argument expressions passed to the sub-component (e.g. `["flash = 512 kB"]`).
    pub args: Vec<String>,
    /// Annotations attached to this sub-component instantiation.
    pub annotations: Vec<AnnotationDoc>,
}

/// Documentation for a realization block on a topology template.
///
/// A realization describes how abstract topology maps to a concrete view
/// (e.g. schematic placement, layout, simulation). `op_summaries` holds
/// one rendered string per operation inside the realization body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RealizationDoc {
    /// Realization name (e.g. `"SchematicView"`, `"PCBLayout"`).
    pub name: String,
    /// Rendered summaries of each operation in the realization body.
    pub op_summaries: Vec<String>,
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

    #[test]
    fn item_doc_variants_serde_round_trip() {
        // Structure variant — rich with children
        let structure_item = ItemDoc::Structure {
            name: "Board".to_string(),
            doc: Some("Main PCB board.".to_string()),
            is_pub: true,
            annotations: vec![AnnotationDoc { name: "deprecated".to_string(), args: vec![] }],
            pragmas: vec![PragmaDoc { name: "layout".to_string(), args: vec!["row".to_string()] }],
            params: vec![ParamDoc {
                name: "width".to_string(),
                doc: None,
                type_repr: "Length".to_string(),
                default_repr: Some("100 mm".to_string()),
                annotations: vec![],
            }],
            ports: vec![PortDoc {
                name: "pwr".to_string(),
                direction: "in".to_string(),
                type_name: "Power".to_string(),
                members: vec![],
            }],
            constraints: vec![ConstraintDoc {
                label: None,
                expr_repr: "width > 0 mm".to_string(),
                annotations: vec![],
            }],
            sub_components: vec![SubComponentDoc {
                name: "cpu".to_string(),
                structure_name: "MCU".to_string(),
                args: vec![],
                annotations: vec![],
            }],
            realizations: vec![RealizationDoc {
                name: "Schematic".to_string(),
                op_summaries: vec!["place cpu".to_string()],
            }],
            meta: vec![("version".to_string(), "1.0".to_string())],
        };
        let json = serde_json::to_string(&structure_item).expect("serialize");
        // Confirm the tagged shape has "kind": "structure"
        assert!(json.contains("\"kind\":\"structure\""), "tag present in JSON: {json}");
        let back: ItemDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(structure_item, back);

        // Function variant — simple
        let fn_item = ItemDoc::Function {
            name: "compute".to_string(),
            doc: None,
            is_pub: false,
            annotations: vec![],
            pragmas: vec![],
            signature: "fn compute(x: f64) -> f64".to_string(),
        };
        let json = serde_json::to_string(&fn_item).expect("serialize");
        assert!(json.contains("\"kind\":\"function\""), "tag present: {json}");
        let back: ItemDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fn_item, back);

        // Enum variant
        let enum_item = ItemDoc::Enum {
            name: "Color".to_string(),
            doc: Some("Color choices.".to_string()),
            is_pub: true,
            annotations: vec![],
            pragmas: vec![],
            variants: vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
        };
        let json = serde_json::to_string(&enum_item).expect("serialize");
        assert!(json.contains("\"kind\":\"enum\""), "tag present: {json}");
        let back: ItemDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(enum_item, back);

        // TypeAlias variant
        let alias_item = ItemDoc::TypeAlias {
            name: "Meters".to_string(),
            doc: None,
            is_pub: true,
            annotations: vec![],
            pragmas: vec![],
            type_repr: "f64".to_string(),
        };
        let json = serde_json::to_string(&alias_item).expect("serialize");
        assert!(json.contains("\"kind\":\"type_alias\""), "tag present: {json}");
        let back: ItemDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(alias_item, back);
    }

    #[test]
    fn module_doc_with_items_serde_round_trip() {
        let module = ModuleDoc {
            path: "electronics.board".to_string(),
            doc: Some("Electronics board module.".to_string()),
            items: vec![
                ItemDoc::Structure {
                    name: "Board".to_string(),
                    doc: None,
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![],
                },
                ItemDoc::Occurrence {
                    name: "Connector".to_string(),
                    doc: None,
                    is_pub: false,
                    annotations: vec![],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![],
                },
                ItemDoc::Trait {
                    name: "HasPower".to_string(),
                    doc: None,
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    members: vec!["voltage: Voltage".to_string()],
                },
                ItemDoc::Field {
                    name: "supply_voltage".to_string(),
                    doc: None,
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    type_repr: "Voltage".to_string(),
                    default_repr: None,
                },
                ItemDoc::Purpose {
                    name: "minimize_area".to_string(),
                    doc: None,
                    is_pub: false,
                    annotations: vec![],
                    pragmas: vec![],
                    expr_repr: "total_area".to_string(),
                    direction: "minimize".to_string(),
                },
                ItemDoc::Unit {
                    name: "Milliamp".to_string(),
                    doc: None,
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    base_unit: "Ampere".to_string(),
                    scale: "0.001".to_string(),
                },
                ItemDoc::ConstraintDef {
                    name: "voltage_safe".to_string(),
                    doc: None,
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    expr_repr: "v <= 5.5 V".to_string(),
                },
            ],
            annotations: vec![AnnotationDoc { name: "version".to_string(), args: vec!["\"1.0\"".to_string()] }],
            pragmas: vec![PragmaDoc { name: "stability".to_string(), args: vec!["stable".to_string()] }],
        };
        let model = DocModel { modules: vec![module.clone()] };
        let json = serde_json::to_string(&model).expect("serialize");
        let back: DocModel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.modules.len(), 1);
        assert_eq!(back.modules[0], module);
        assert_eq!(back.modules[0].items.len(), 7);
    }
}
