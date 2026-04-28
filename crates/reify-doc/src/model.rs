//! Pure-data DocModel surface for the `reify doc` tool.
//!
//! All types here are serde-serializable value objects with no dependency on
//! `reify-compiler`, `reify-syntax`, or `reify-types`. String fields carry
//! rendered/printable representations rather than typed AST nodes.

use serde::{Deserialize, Serialize};

/// Root documentation model for a set of compiled Reify modules.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocModel {
    pub modules: Vec<ModuleDoc>,
}

/// Documentation for a single compiled Reify module.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleDoc {
    /// Fully-qualified module path (e.g. `"electronics.board"`).
    pub path: String,
    /// Optional top-level doc-comment for the module.
    pub doc: Option<String>,
    /// Top-level declared items, in source declaration order.
    pub items: Vec<ItemDoc>,
    /// Module-level annotations.
    pub annotations: Vec<AnnotationDoc>,
    /// Module-level pragmas.
    pub pragmas: Vec<PragmaDoc>,
    /// Cross-reference data for this module (referenced modules / items / traits).
    /// Populated by the lowering slice; absent in serialized JSON from earlier slices.
    pub cross_refs: ModuleCrossRefs,
}

/// Documentation for a single `@annotation(...)` attached to a declaration.
///
/// Arguments are stored as rendered/printable strings — not typed AST values —
/// so `reify-doc` remains free of any dependency on `reify-syntax` or `reify-types`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
pub struct ConstraintDoc {
    /// Optional user-given label for the constraint.
    pub label: Option<String>,
    /// Rendered constraint expression (e.g. `"voltage >= 3.0 V && voltage <= 5.5 V"`).
    pub expr_repr: String,
    /// Annotations attached to this constraint.
    pub annotations: Vec<AnnotationDoc>,
    /// 1-indexed source line number of the constraint, if known.
    ///
    /// The struct-level `#[serde(default)]` keeps legacy JSON (serialized
    /// before this field was introduced) deserializing cleanly with
    /// `line == None`.
    pub line: Option<u32>,
}

/// Documentation for a sub-component instantiation inside a topology template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
pub struct RealizationDoc {
    /// Realization name (e.g. `"SchematicView"`, `"PCBLayout"`).
    pub name: String,
    /// Rendered summaries of each operation in the realization body.
    pub op_summaries: Vec<String>,
}

/// Cross-reference information gathered for a module or item.
///
/// Populated by later slices (the lowering pass from `CompiledModule`).
/// In slice 1 the type exists solely so downstream crates can take a
/// dependency on the schema without needing to wait for the lowering pass.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleCrossRefs {
    /// Fully-qualified paths of modules imported or referenced.
    pub referenced_modules: Vec<String>,
    /// Qualified names of items (structures, occurrences, functions, …) referenced.
    pub referenced_items: Vec<String>,
    /// Qualified names of traits referenced.
    pub referenced_traits: Vec<String>,
}

/// A single top-level declaration documented in a module.
///
/// Uses a `"kind"` tag in JSON so downstream consumers can discriminate on
/// declaration type without manual field inspection.  Variants map to the
/// top-level declaration kinds exposed in documentation.
///
/// Note: `Import` declarations from `reify_syntax::Declaration` are
/// intentionally omitted here — imported modules are reflected instead via
/// `ModuleCrossRefs::referenced_modules` and each module's `ModuleDoc.path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemDoc {
    /// A `structure` declaration — topology template with optional children.
    ///
    /// The payload mirrors `ItemDoc::Occurrence` field-for-field.  This
    /// intentional duplication tracks the upstream split in
    /// `reify_syntax::Declaration` between `Declaration::Structure` and
    /// `Declaration::Occurrence`, keeping the future lowering pass (a later
    /// slice) as a near-1-to-1 field walk per variant.
    Structure {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        params: Vec<ParamDoc>,
        ports: Vec<PortDoc>,
        constraints: Vec<ConstraintDoc>,
        sub_components: Vec<SubComponentDoc>,
        realizations: Vec<RealizationDoc>,
        /// Arbitrary key-value metadata (e.g. compiler-generated tags).
        ///
        /// Stored as ordered `(key, value)` pairs so duplicate keys and
        /// source insertion order are both preserved.  Serializes as a JSON
        /// array of two-element arrays: `[["version","1.0"],["tag","alpha"]]`.
        meta: Vec<(String, String)>,
    },
    /// An `occurrence` declaration — like a structure but for occurrence-mode topologies.
    ///
    /// The payload mirrors `ItemDoc::Structure` field-for-field; see the
    /// `Structure` variant doc for the rationale.
    Occurrence {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        params: Vec<ParamDoc>,
        ports: Vec<PortDoc>,
        constraints: Vec<ConstraintDoc>,
        sub_components: Vec<SubComponentDoc>,
        realizations: Vec<RealizationDoc>,
        /// See `ItemDoc::Structure.meta` for serialization shape rationale.
        meta: Vec<(String, String)>,
    },
    /// A `trait` declaration — interface definition.
    Trait {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Rendered member signatures (e.g. `["voltage: Voltage", "current: Current"]`).
        members: Vec<String>,
    },
    /// A `fn` declaration.
    Function {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Full rendered signature (e.g. `"fn compute(x: f64) -> f64"`).
        signature: String,
    },
    /// A `let` (field) declaration at module scope.
    Field {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Rendered field type (e.g. `"Voltage"`).
        type_repr: String,
        /// Rendered default value expression, if any.
        default_repr: Option<String>,
    },
    /// A `purpose` (objective/optimization) declaration.
    Purpose {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Rendered objective expression.
        expr_repr: String,
        /// Optimization direction, e.g. `"minimize"` or `"maximize"`.
        direction: String,
    },
    /// An `enum` declaration.
    Enum {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Rendered variant names.
        variants: Vec<String>,
    },
    /// A `unit` declaration.
    Unit {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Name of the base SI unit (e.g. `"Ampere"`).
        base_unit: String,
        /// Rendered scale factor relative to the base (e.g. `"0.001"`).
        scale: String,
    },
    /// A `type` alias declaration.
    TypeAlias {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Rendered right-hand-side type (e.g. `"f64"`).
        type_repr: String,
    },
    /// A named constraint definition.
    ConstraintDef {
        name: String,
        doc: Option<String>,
        is_pub: bool,
        annotations: Vec<AnnotationDoc>,
        pragmas: Vec<PragmaDoc>,
        /// Rendered constraint expression.
        expr_repr: String,
    },
}

impl ItemDoc {
    /// Lookup the `name` field of any variant.
    pub(crate) fn name(&self) -> &str {
        match self {
            ItemDoc::Structure { name, .. }
            | ItemDoc::Occurrence { name, .. }
            | ItemDoc::Trait { name, .. }
            | ItemDoc::Function { name, .. }
            | ItemDoc::Field { name, .. }
            | ItemDoc::Purpose { name, .. }
            | ItemDoc::Enum { name, .. }
            | ItemDoc::Unit { name, .. }
            | ItemDoc::TypeAlias { name, .. }
            | ItemDoc::ConstraintDef { name, .. } => name,
        }
    }

    /// Lookup the `is_pub` field of any variant.
    pub(crate) fn is_pub(&self) -> bool {
        match self {
            ItemDoc::Structure { is_pub, .. }
            | ItemDoc::Occurrence { is_pub, .. }
            | ItemDoc::Trait { is_pub, .. }
            | ItemDoc::Function { is_pub, .. }
            | ItemDoc::Field { is_pub, .. }
            | ItemDoc::Purpose { is_pub, .. }
            | ItemDoc::Enum { is_pub, .. }
            | ItemDoc::Unit { is_pub, .. }
            | ItemDoc::TypeAlias { is_pub, .. }
            | ItemDoc::ConstraintDef { is_pub, .. } => *is_pub,
        }
    }

    /// Lookup the optional doc-comment of any variant.
    pub(crate) fn doc(&self) -> Option<&str> {
        match self {
            ItemDoc::Structure { doc, .. }
            | ItemDoc::Occurrence { doc, .. }
            | ItemDoc::Trait { doc, .. }
            | ItemDoc::Function { doc, .. }
            | ItemDoc::Field { doc, .. }
            | ItemDoc::Purpose { doc, .. }
            | ItemDoc::Enum { doc, .. }
            | ItemDoc::Unit { doc, .. }
            | ItemDoc::TypeAlias { doc, .. }
            | ItemDoc::ConstraintDef { doc, .. } => doc.as_deref(),
        }
    }

    /// Lookup the annotations attached to any variant.
    pub(crate) fn annotations(&self) -> &[AnnotationDoc] {
        match self {
            ItemDoc::Structure { annotations, .. }
            | ItemDoc::Occurrence { annotations, .. }
            | ItemDoc::Trait { annotations, .. }
            | ItemDoc::Function { annotations, .. }
            | ItemDoc::Field { annotations, .. }
            | ItemDoc::Purpose { annotations, .. }
            | ItemDoc::Enum { annotations, .. }
            | ItemDoc::Unit { annotations, .. }
            | ItemDoc::TypeAlias { annotations, .. }
            | ItemDoc::ConstraintDef { annotations, .. } => annotations,
        }
    }

    /// Language keyword displayed in the H2 heading for each variant.
    ///
    /// Matches the snake_case kind tag used by `#[serde(tag="kind", rename_all="snake_case")]`
    /// on `ItemDoc`, except for variants whose Reify-source keyword differs from the
    /// JSON tag (`Field` → `"let"`, `TypeAlias` → `"type"`, `ConstraintDef` →
    /// `"constraint"`).
    pub(crate) fn keyword(&self) -> &'static str {
        match self {
            ItemDoc::Structure { .. } => "structure",
            ItemDoc::Occurrence { .. } => "occurrence",
            ItemDoc::Trait { .. } => "trait",
            ItemDoc::Function { .. } => "fn",
            ItemDoc::Field { .. } => "let",
            ItemDoc::Purpose { .. } => "purpose",
            ItemDoc::Enum { .. } => "enum",
            ItemDoc::Unit { .. } => "unit",
            ItemDoc::TypeAlias { .. } => "type",
            ItemDoc::ConstraintDef { .. } => "constraint",
        }
    }

    /// Stable TOC group label. `"Constants"` buckets the long tail of
    /// value-like declarations (Field, Unit, TypeAlias, ConstraintDef, Purpose)
    /// per the PRD's six-group TOC.
    pub(crate) fn group(&self) -> &'static str {
        match self {
            ItemDoc::Trait { .. } => "Traits",
            ItemDoc::Structure { .. } => "Structures",
            ItemDoc::Occurrence { .. } => "Occurrences",
            ItemDoc::Enum { .. } => "Enums",
            ItemDoc::Function { .. } => "Functions",
            ItemDoc::Field { .. }
            | ItemDoc::Unit { .. }
            | ItemDoc::TypeAlias { .. }
            | ItemDoc::ConstraintDef { .. }
            | ItemDoc::Purpose { .. } => "Constants",
        }
    }

    /// Snake_case kind tag matching `#[serde(tag="kind", rename_all="snake_case")]`.
    /// Used as the prefix in split-mode filenames so multi-kind name collisions
    /// (e.g. a trait `Board` vs a structure `Board`) stay distinct.
    pub(crate) fn kind_slug(&self) -> &'static str {
        match self {
            ItemDoc::Structure { .. } => "structure",
            ItemDoc::Occurrence { .. } => "occurrence",
            ItemDoc::Trait { .. } => "trait",
            ItemDoc::Function { .. } => "function",
            ItemDoc::Field { .. } => "field",
            ItemDoc::Purpose { .. } => "purpose",
            ItemDoc::Enum { .. } => "enum",
            ItemDoc::Unit { .. } => "unit",
            ItemDoc::TypeAlias { .. } => "type_alias",
            ItemDoc::ConstraintDef { .. } => "constraint_def",
        }
    }
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
            line: None,
        };
        let json = serde_json::to_string(&constraint).expect("serialize");
        let back: ConstraintDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(constraint, back);
        assert!(back.annotations.is_empty());
    }

    /// Round-trip with the new `line` field populated.
    #[test]
    fn constraint_doc_serde_round_trip_with_line() {
        let constraint = ConstraintDoc {
            label: Some("len_ge_diam".to_string()),
            expr_repr: "length >= diameter".to_string(),
            annotations: vec![],
            line: Some(42),
        };
        let json = serde_json::to_string(&constraint).expect("serialize");
        assert!(json.contains("\"line\":42"), "line tag in JSON: {json}");
        let back: ConstraintDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(constraint, back);
        assert_eq!(back.line, Some(42));
    }

    /// Forward-compat guard: legacy JSON without the `line` field must still
    /// deserialize, with `line` defaulting to `None` via the struct-level
    /// `#[serde(default)]`.
    #[test]
    fn constraint_doc_deserializes_without_line() {
        let legacy_json = r#"{"label":"voltage_range","expr_repr":"v <= 5.5 V","annotations":[]}"#;
        let c: ConstraintDoc = serde_json::from_str(legacy_json).expect("deserialize legacy");
        assert_eq!(c.label.as_deref(), Some("voltage_range"));
        assert_eq!(c.expr_repr, "v <= 5.5 V");
        assert!(c.annotations.is_empty());
        assert_eq!(c.line, None);
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
                line: None,
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
            cross_refs: ModuleCrossRefs {
                referenced_modules: vec!["mechanics.base".to_string()],
                referenced_items: vec!["MCU".to_string()],
                referenced_traits: vec![],
            },
        };
        let model = DocModel { modules: vec![module.clone()] };
        let json = serde_json::to_string(&model).expect("serialize");
        let back: DocModel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.modules.len(), 1);
        assert_eq!(back.modules[0], module);
        assert_eq!(back.modules[0].items.len(), 7);
    }

    /// Table-driven test: every `ItemDoc` variant must serialize with the
    /// correct `"kind"` tag value.  Catches snake_case rename surprises (e.g.
    /// `constraint_def` vs `constraintdef`) that would slip past the
    /// round-trip tests while breaking downstream JSON consumers.
    #[test]
    fn item_doc_all_variant_kind_tags() {
        let cases: Vec<(ItemDoc, &str)> = vec![
            (
                ItemDoc::Structure {
                    name: "S".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![], params: vec![],
                    ports: vec![], constraints: vec![], sub_components: vec![],
                    realizations: vec![], meta: vec![],
                },
                "structure",
            ),
            (
                ItemDoc::Occurrence {
                    name: "O".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![], params: vec![],
                    ports: vec![], constraints: vec![], sub_components: vec![],
                    realizations: vec![], meta: vec![],
                },
                "occurrence",
            ),
            (
                ItemDoc::Trait {
                    name: "T".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![], members: vec![],
                },
                "trait",
            ),
            (
                ItemDoc::Function {
                    name: "F".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![],
                    signature: "fn f()".into(),
                },
                "function",
            ),
            (
                ItemDoc::Field {
                    name: "x".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![],
                    type_repr: "i32".into(), default_repr: None,
                },
                "field",
            ),
            (
                ItemDoc::Purpose {
                    name: "P".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![],
                    expr_repr: "cost".into(), direction: "minimize".into(),
                },
                "purpose",
            ),
            (
                ItemDoc::Enum {
                    name: "E".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![], variants: vec![],
                },
                "enum",
            ),
            (
                ItemDoc::Unit {
                    name: "U".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![],
                    base_unit: "Meter".into(), scale: "1.0".into(),
                },
                "unit",
            ),
            (
                ItemDoc::TypeAlias {
                    name: "A".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![],
                    type_repr: "f64".into(),
                },
                "type_alias",
            ),
            (
                ItemDoc::ConstraintDef {
                    name: "C".into(), doc: None, is_pub: false,
                    annotations: vec![], pragmas: vec![],
                    expr_repr: "x > 0".into(),
                },
                "constraint_def",
            ),
        ];

        for (item, expected_kind) in &cases {
            let json = serde_json::to_string(item).expect("serialize");
            let expected_tag = format!("\"kind\":\"{}\"", expected_kind);
            assert!(
                json.contains(&expected_tag),
                "variant={expected_kind}: expected {expected_tag} in serialized JSON: {json}",
            );
        }
    }

    /// Forward-compat guard: JSON serialized before `cross_refs` was added
    /// (and before any future additive field) must still deserialize via
    /// `#[serde(default)]`.  Catches accidental removal of the attribute.
    #[test]
    fn module_doc_deserializes_without_cross_refs() {
        let legacy_json = r#"{"path":"old.module","doc":null,"items":[],"annotations":[],"pragmas":[]}"#;
        let m: ModuleDoc = serde_json::from_str(legacy_json).expect("deserialize legacy");
        assert_eq!(m.path, "old.module");
        assert_eq!(m.cross_refs, ModuleCrossRefs::default());
    }

    #[test]
    fn cross_refs_serde_round_trip() {
        let xrefs = ModuleCrossRefs {
            referenced_modules: vec!["electronics.power".to_string(), "mechanics.base".to_string()],
            referenced_items: vec!["Board".to_string(), "MCU".to_string(), "Connector".to_string()],
            referenced_traits: vec!["HasPower".to_string(), "HasSignal".to_string()],
        };
        let json = serde_json::to_string(&xrefs).expect("serialize");
        let back: ModuleCrossRefs = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(xrefs, back);
        assert_eq!(back.referenced_modules.len(), 2);
        assert_eq!(back.referenced_items.len(), 3);
        assert_eq!(back.referenced_traits.len(), 2);

        // Default round-trip produces an empty value.
        let empty = ModuleCrossRefs::default();
        let json = serde_json::to_string(&empty).expect("serialize empty");
        let back: ModuleCrossRefs = serde_json::from_str(&json).expect("deserialize empty");
        assert_eq!(empty, back);
        assert!(back.referenced_modules.is_empty());
        assert!(back.referenced_items.is_empty());
        assert!(back.referenced_traits.is_empty());
    }

    // -----------------------------------------------------------------------
    // Tests for the seven inherent accessor methods on `ItemDoc`.
    // These are TDD-first: they fail to compile until step 2 adds the methods.
    // -----------------------------------------------------------------------

    /// Returns one of each `ItemDoc` variant with deterministic minimal fields:
    /// `is_pub: false`, `doc: None`, `annotations: []`, `pragmas: []`, and
    /// variant-specific fields set to sensible empty/placeholder values.
    /// Names are the short identifiers used throughout the accessor tests.
    fn sample_items() -> Vec<ItemDoc> {
        vec![
            ItemDoc::Structure {
                name: "S".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            },
            ItemDoc::Occurrence {
                name: "O".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            },
            ItemDoc::Trait {
                name: "T".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], members: vec![],
            },
            ItemDoc::Function {
                name: "F".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                signature: "fn f()".into(),
            },
            ItemDoc::Field {
                name: "x".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                type_repr: "i32".into(), default_repr: None,
            },
            ItemDoc::Purpose {
                name: "P".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "cost".into(), direction: "minimize".into(),
            },
            ItemDoc::Enum {
                name: "E".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], variants: vec![],
            },
            ItemDoc::Unit {
                name: "U".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                base_unit: "Meter".into(), scale: "1.0".into(),
            },
            ItemDoc::TypeAlias {
                name: "A".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                type_repr: "f64".into(),
            },
            ItemDoc::ConstraintDef {
                name: "C".into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x > 0".into(),
            },
        ]
    }

    /// Sets the `is_pub` field of any `ItemDoc` variant to `v`.
    fn set_is_pub(item: &mut ItemDoc, v: bool) {
        match item {
            ItemDoc::Structure { is_pub, .. }
            | ItemDoc::Occurrence { is_pub, .. }
            | ItemDoc::Trait { is_pub, .. }
            | ItemDoc::Function { is_pub, .. }
            | ItemDoc::Field { is_pub, .. }
            | ItemDoc::Purpose { is_pub, .. }
            | ItemDoc::Enum { is_pub, .. }
            | ItemDoc::Unit { is_pub, .. }
            | ItemDoc::TypeAlias { is_pub, .. }
            | ItemDoc::ConstraintDef { is_pub, .. } => *is_pub = v,
        }
    }

    /// Sets the `doc` field of any `ItemDoc` variant to `doc`.
    fn set_doc(item: &mut ItemDoc, doc: Option<String>) {
        match item {
            ItemDoc::Structure { doc: d, .. }
            | ItemDoc::Occurrence { doc: d, .. }
            | ItemDoc::Trait { doc: d, .. }
            | ItemDoc::Function { doc: d, .. }
            | ItemDoc::Field { doc: d, .. }
            | ItemDoc::Purpose { doc: d, .. }
            | ItemDoc::Enum { doc: d, .. }
            | ItemDoc::Unit { doc: d, .. }
            | ItemDoc::TypeAlias { doc: d, .. }
            | ItemDoc::ConstraintDef { doc: d, .. } => *d = doc,
        }
    }

    /// Pushes `ann` onto the `annotations` field of any `ItemDoc` variant.
    fn push_annotation(item: &mut ItemDoc, ann: AnnotationDoc) {
        match item {
            ItemDoc::Structure { annotations, .. }
            | ItemDoc::Occurrence { annotations, .. }
            | ItemDoc::Trait { annotations, .. }
            | ItemDoc::Function { annotations, .. }
            | ItemDoc::Field { annotations, .. }
            | ItemDoc::Purpose { annotations, .. }
            | ItemDoc::Enum { annotations, .. }
            | ItemDoc::Unit { annotations, .. }
            | ItemDoc::TypeAlias { annotations, .. }
            | ItemDoc::ConstraintDef { annotations, .. } => annotations.push(ann),
        }
    }

    #[test]
    fn item_doc_name_returns_variant_name() {
        let expected = ["S", "O", "T", "F", "x", "P", "E", "U", "A", "C"];
        for (item, &exp) in sample_items().iter().zip(expected.iter()) {
            assert_eq!(item.name(), exp);
        }
    }

    #[test]
    fn item_doc_is_pub_returns_variant_visibility() {
        // false cases — all sample_items() have is_pub: false
        for item in &sample_items() {
            assert!(!item.is_pub());
        }
        // true cases — mutate each sample item to set is_pub: true
        for mut item in sample_items() {
            set_is_pub(&mut item, true);
            assert!(item.is_pub());
        }
    }

    #[test]
    fn item_doc_doc_returns_variant_doc_comment() {
        // None cases — all sample_items() have doc: None
        for item in &sample_items() {
            assert_eq!(item.doc(), None);
        }
        // Some cases — mutate each sample item to set doc: Some("doc")
        for mut item in sample_items() {
            set_doc(&mut item, Some("doc".into()));
            assert_eq!(item.doc(), Some("doc"));
        }
    }

    #[test]
    fn item_doc_annotations_returns_variant_annotations() {
        // empty cases — all sample_items() have annotations: []
        for item in &sample_items() {
            assert!(item.annotations().is_empty());
        }
        // one-marker cases — mutate each sample item to add a "marker" annotation
        let marker = AnnotationDoc { name: "marker".to_string(), args: vec![] };
        for mut item in sample_items() {
            push_annotation(&mut item, marker.clone());
            let anns = item.annotations();
            assert_eq!(anns.len(), 1);
            assert_eq!(anns[0].name, "marker");
        }
    }

    #[test]
    fn item_doc_keyword_per_variant() {
        let expected = [
            "structure", "occurrence", "trait", "fn",
            "let", "purpose", "enum", "unit", "type", "constraint",
        ];
        for (item, &exp) in sample_items().iter().zip(expected.iter()) {
            assert_eq!(item.keyword(), exp);
        }
    }

    #[test]
    fn item_doc_group_per_variant() {
        let expected = [
            "Structures", "Occurrences", "Traits", "Functions",
            "Constants", "Constants", "Enums", "Constants", "Constants", "Constants",
        ];
        for (item, &exp) in sample_items().iter().zip(expected.iter()) {
            assert_eq!(item.group(), exp);
        }
    }

    #[test]
    fn item_doc_kind_slug_per_variant() {
        let expected = [
            "structure", "occurrence", "trait", "function",
            "field", "purpose", "enum", "unit", "type_alias", "constraint_def",
        ];
        for (item, &exp) in sample_items().iter().zip(expected.iter()) {
            assert_eq!(item.kind_slug(), exp);
        }
    }

    /// Shape-pinning test: pins the new `ItemDoc { header: ItemHeader, kind: ItemKind }`
    /// struct shape, exercises all seven accessors, and asserts the JSON wire format
    /// flattens header + kind to the historical top-level-field shape.
    ///
    /// Fails to compile against the current enum-shaped `ItemDoc` — step-2 makes it pass.
    #[test]
    fn item_doc_struct_shape_with_header_and_kind_round_trips() {
        let header = ItemHeader {
            name: "Bolt".into(),
            doc: Some("A bolt.".into()),
            is_pub: true,
            annotations: vec![AnnotationDoc { name: "deprecated".into(), args: vec![] }],
            pragmas: vec![PragmaDoc { name: "inline".into(), args: vec![] }],
        };
        let kind = ItemKind::Structure {
            params: vec![],
            ports: vec![],
            constraints: vec![],
            sub_components: vec![],
            realizations: vec![],
            meta: vec![],
        };
        let item = ItemDoc { header: header.clone(), kind: kind.clone() };

        // Direct field access
        assert_eq!(item.header.name, "Bolt");
        assert!(item.header.is_pub);
        assert_eq!(item.kind, kind);

        // All seven accessor methods
        assert_eq!(item.name(), "Bolt");
        assert!(item.is_pub());
        assert_eq!(item.doc(), Some("A bolt."));
        assert_eq!(item.annotations().len(), 1);
        assert_eq!(item.keyword(), "structure");
        assert_eq!(item.group(), "Structures");
        assert_eq!(item.kind_slug(), "structure");

        // JSON wire format: header + kind fields flatten to top-level
        let json = serde_json::to_string(&item).expect("serialize");
        assert!(json.contains("\"kind\":\"structure\""), "kind tag flattened: {json}");
        assert!(json.contains("\"name\":\"Bolt\""), "name flattened: {json}");
        assert!(json.contains("\"is_pub\":true"), "is_pub flattened: {json}");
        assert!(json.contains("\"params\":[]"), "variant payload flattened: {json}");

        // Round-trip
        let back: ItemDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(item, back);
    }
}
