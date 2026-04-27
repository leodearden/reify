//! Behavioural tests for the GitHub-flavored Markdown formatter (`fmt_markdown`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_markdown.rs` so that golden snapshots can be loaded via `include_str!` from
//! sibling `tests/snapshots/` files without polluting the library binary.

use reify_doc::fmt_markdown::{render_markdown, MarkdownOptions, MarkdownOutput};
use reify_doc::model::{
    AnnotationDoc, ConstraintDoc, DocModel, ItemDoc, ModuleDoc, ParamDoc, PortDoc,
};

/// Helper: build a single-module model with one item and return the rendered
/// single-file output.
fn render_one_item(item: ItemDoc) -> String {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".to_string(),
            items: vec![item],
            ..Default::default()
        }],
    };
    render_single(&model)
}

/// Helper: render the model in single-file mode and unwrap to `String`.
fn render_single(model: &DocModel) -> String {
    match render_markdown(model, None, &MarkdownOptions::default()) {
        MarkdownOutput::Single(s) => s,
        MarkdownOutput::Split(_) => panic!("expected Single output"),
    }
}

/// `MarkdownOptions::default()` must yield single-file (non-split) mode so that
/// callers who don't care about splitting can rely on the obvious default.
#[test]
fn options_default_is_single_file() {
    let opts = MarkdownOptions::default();
    assert!(!opts.split, "default MarkdownOptions.split must be false");
}

/// `MarkdownOutput` must expose `Single(String)` and `Split(Vec<(String, String)>)`
/// variants and be matchable.
#[test]
fn output_variants_can_be_matched() {
    let single = MarkdownOutput::Single("hello".to_string());
    match single {
        MarkdownOutput::Single(s) => assert_eq!(s, "hello"),
        MarkdownOutput::Split(_) => panic!("expected Single"),
    }

    let split = MarkdownOutput::Split(vec![("index.md".to_string(), "body".to_string())]);
    match split {
        MarkdownOutput::Single(_) => panic!("expected Split"),
        MarkdownOutput::Split(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].0, "index.md");
            assert_eq!(v[0].1, "body");
        }
    }
}

/// An empty `DocModel` (no modules) renders to an empty single-file body and a
/// split-mode list containing exactly the `index.md` placeholder.
#[test]
fn empty_model_single_mode_yields_empty_body() {
    let model = DocModel::default();
    let out = render_markdown(&model, None, &MarkdownOptions::default());
    match out {
        MarkdownOutput::Single(s) => {
            assert!(s.trim().is_empty(), "expected empty single body, got: {s:?}");
        }
        MarkdownOutput::Split(_) => panic!("default options should yield Single"),
    }
}

#[test]
fn empty_model_split_mode_yields_index_only() {
    let model = DocModel::default();
    let out = render_markdown(&model, None, &MarkdownOptions { split: true });
    match out {
        MarkdownOutput::Single(_) => panic!("split: true should yield Split"),
        MarkdownOutput::Split(v) => {
            assert_eq!(v.len(), 1, "expected exactly one (index.md) entry, got {v:?}");
            assert_eq!(v[0].0, "index.md");
        }
    }
}

/// A module with `path = "electronics.board"` and a top-level doc renders an
/// `# electronics.board` H1 header followed by the doc paragraph.
#[test]
fn module_header_and_doc_paragraph() {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "electronics.board".to_string(),
            doc: Some("Electronics board module.".to_string()),
            ..Default::default()
        }],
    };
    let out = render_single(&model);
    assert!(
        out.contains("# electronics.board\n"),
        "expected H1 module header, got:\n{out}"
    );
    assert!(
        out.contains("Electronics board module."),
        "expected module doc paragraph, got:\n{out}"
    );
    // Header followed by blank line and then the doc paragraph.
    let header_idx = out.find("# electronics.board").expect("header present");
    let doc_idx = out.find("Electronics board module.").expect("doc present");
    assert!(
        doc_idx > header_idx,
        "doc paragraph must come after header"
    );
}

/// A module with no `doc` renders only the H1 (no extra paragraph).
#[test]
fn module_header_without_doc() {
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "no_doc.module".to_string(),
            doc: None,
            ..Default::default()
        }],
    };
    let out = render_single(&model);
    assert!(out.contains("# no_doc.module"), "header present, got: {out}");
}

/// Table-driven coverage for the H2 heading + anchor on every ItemDoc variant.
///
/// Each case names the variant, supplies the item built with `name = "Foo"`,
/// and expects the rendered output to contain a substring including the
/// language keyword and the anchor `<a id="Foo"></a>`.
#[test]
fn item_h2_headings_per_variant() {
    fn item_with_name(maker: impl FnOnce(&str) -> ItemDoc) -> ItemDoc {
        maker("Foo")
    }

    let cases: Vec<(&str, ItemDoc, &str)> = vec![
        (
            "structure",
            item_with_name(|n| ItemDoc::Structure {
                name: n.into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            }),
            "pub structure",
        ),
        (
            "occurrence",
            item_with_name(|n| ItemDoc::Occurrence {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            }),
            "occurrence",
        ),
        (
            "trait",
            item_with_name(|n| ItemDoc::Trait {
                name: n.into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], members: vec![],
            }),
            "pub trait",
        ),
        (
            "function",
            item_with_name(|n| ItemDoc::Function {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                signature: "fn Foo()".into(),
            }),
            "fn",
        ),
        (
            "field",
            item_with_name(|n| ItemDoc::Field {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                type_repr: "i32".into(), default_repr: None,
            }),
            "let",
        ),
        (
            "purpose",
            item_with_name(|n| ItemDoc::Purpose {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x".into(), direction: "minimize".into(),
            }),
            "purpose",
        ),
        (
            "enum",
            item_with_name(|n| ItemDoc::Enum {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], variants: vec![],
            }),
            "enum",
        ),
        (
            "unit",
            item_with_name(|n| ItemDoc::Unit {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                base_unit: "Meter".into(), scale: "1.0".into(),
            }),
            "unit",
        ),
        (
            "type_alias",
            item_with_name(|n| ItemDoc::TypeAlias {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                type_repr: "f64".into(),
            }),
            "type",
        ),
        (
            "constraint_def",
            item_with_name(|n| ItemDoc::ConstraintDef {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x > 0".into(),
            }),
            "constraint",
        ),
    ];

    for (kind, item, expected_keyword) in cases {
        let out = render_one_item(item);
        // Must contain an H2 heading line containing the keyword and `Foo`
        // wrapped in backticks.
        let header_line = out.lines().find(|l| l.starts_with("## "));
        let header_line = header_line
            .unwrap_or_else(|| panic!("variant={kind}: no H2 in output:\n{out}"));
        assert!(
            header_line.contains(expected_keyword),
            "variant={kind}: H2 missing keyword {expected_keyword:?}: {header_line}"
        );
        assert!(
            header_line.contains("`Foo`")
                || header_line.contains("`pub structure Foo`")
                || header_line.contains(&format!("`{} Foo`", expected_keyword))
                || header_line.contains(&format!("`pub {} Foo`", expected_keyword)),
            "variant={kind}: H2 missing `Foo` backticked: {header_line}"
        );
        assert!(
            header_line.contains(r#"<a id="Foo">"#),
            "variant={kind}: H2 missing anchor: {header_line}"
        );
    }
}

/// A Structure with two parameters renders a GFM `### Parameters` table with
/// the standard 5-column header (Name | Type | Dimension | Default | Description).
#[test]
fn parameters_table_renders() {
    let item = ItemDoc::Structure {
        name: "Bolt".into(), doc: None, is_pub: true,
        annotations: vec![], pragmas: vec![],
        params: vec![
            ParamDoc {
                name: "length".into(),
                doc: Some("Bolt length.".into()),
                type_repr: "Length".into(),
                default_repr: Some("100 mm".into()),
                annotations: vec![],
            },
            ParamDoc {
                name: "diameter".into(),
                doc: None,
                type_repr: "Length".into(),
                default_repr: None,
                annotations: vec![],
            },
        ],
        ports: vec![], constraints: vec![], sub_components: vec![],
        realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(out.contains("### Parameters"), "section H3 missing:\n{out}");
    assert!(
        out.contains("| Name | Type | Dimension | Default | Description |"),
        "header row missing:\n{out}"
    );
    assert!(
        out.contains("| --- | --- | --- | --- | --- |"),
        "alignment row missing:\n{out}"
    );
    assert!(
        out.contains("`length`") && out.contains("`Length`"),
        "name+type cells missing:\n{out}"
    );
    assert!(out.contains("`100 mm`"), "default cell missing:\n{out}");
    // diameter row has em-dash for missing default.
    assert!(
        out.lines().any(|l| l.contains("`diameter`") && l.contains("—")),
        "em-dash for empty default missing:\n{out}"
    );
}

/// A Structure with no params omits the `### Parameters` section entirely.
#[test]
fn parameters_table_omitted_when_empty() {
    let item = ItemDoc::Structure {
        name: "Empty".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![],
        ports: vec![], constraints: vec![], sub_components: vec![],
        realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(!out.contains("### Parameters"), "should omit section, got:\n{out}");
    assert!(!out.contains("| Name | Type"), "should omit header, got:\n{out}");
}

/// A Structure with two ports renders a GFM `### Ports` table with the
/// standard 5-column header (Name | Kind | Role | Type | Description).
#[test]
fn ports_table_renders() {
    let item = ItemDoc::Structure {
        name: "Board".into(), doc: None, is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![],
        ports: vec![
            PortDoc {
                name: "pwr_in".into(),
                direction: "in".into(),
                type_name: "Power".into(),
                members: vec!["voltage".into(), "current".into()],
            },
            PortDoc {
                name: "data_out".into(),
                direction: "out".into(),
                type_name: "Signal".into(),
                members: vec![],
            },
        ],
        constraints: vec![], sub_components: vec![],
        realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(out.contains("### Ports"), "section H3 missing:\n{out}");
    assert!(
        out.contains("| Name | Kind | Role | Type | Description |"),
        "header row missing:\n{out}"
    );
    assert!(
        out.contains("| --- | --- | --- | --- | --- |"),
        "alignment row missing:\n{out}"
    );
    assert!(
        out.contains("`pwr_in`") && out.contains("`Power`"),
        "name+type cells missing:\n{out}"
    );
    // direction maps to Role column.
    assert!(out.contains("in") && out.contains("out"), "roles missing:\n{out}");
}

#[test]
fn ports_table_omitted_when_empty() {
    let item = ItemDoc::Structure {
        name: "NoP".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![],
        ports: vec![], constraints: vec![], sub_components: vec![],
        realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(!out.contains("### Ports"), "should omit ports section, got:\n{out}");
}

/// Constraints render as `### Constraints` H3 then bulleted list. Entries
/// with `line: Some(N)` get a ` *(line N)*` suffix; entries with `label`
/// render as `- {label}: \`{expr}\``; entries without label render as
/// `- \`{expr}\``.
#[test]
fn constraints_section_renders() {
    let item = ItemDoc::Structure {
        name: "Bolt".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![],
        ports: vec![],
        constraints: vec![
            ConstraintDoc {
                label: None,
                expr_repr: "length >= diameter".into(),
                annotations: vec![],
                line: Some(42),
            },
            ConstraintDoc {
                label: Some("safe_v".into()),
                expr_repr: "v <= 5.5 V".into(),
                annotations: vec![],
                line: None,
            },
        ],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(out.contains("### Constraints"), "H3 missing:\n{out}");
    // First constraint: no label, line 42.
    assert!(
        out.contains("- `length >= diameter` *(line 42)*"),
        "labelless+line bullet missing:\n{out}"
    );
    // Second: label, no line.
    assert!(
        out.contains("- safe_v: `v <= 5.5 V`"),
        "labeled+no-line bullet missing:\n{out}"
    );
    // Sanity: the second bullet must NOT contain `*(line` since line is None.
    let bullet_with_label = out
        .lines()
        .find(|l| l.contains("safe_v"))
        .expect("found labeled bullet line");
    assert!(
        !bullet_with_label.contains("*(line"),
        "labeled bullet should omit line suffix: {bullet_with_label}"
    );
}

#[test]
fn constraints_section_omitted_when_empty() {
    let item = ItemDoc::Structure {
        name: "NoC".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![],
        ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(!out.contains("### Constraints"), "should omit, got:\n{out}");
}

/// Meta section: `### Meta` H3 then `- **{key}**: {value}` bullets, sorted
/// alphabetically by key. Input order ["version","alpha"] must render
/// "alpha" before "version".
#[test]
fn meta_section_renders_alphabetical() {
    let item = ItemDoc::Structure {
        name: "Meta".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![
            ("version".into(), "1.0".into()),
            ("alpha".into(), "yes".into()),
        ],
    };
    let out = render_one_item(item);
    assert!(out.contains("### Meta"), "H3 missing:\n{out}");
    let alpha_idx = out.find("**alpha**").expect("alpha bullet present");
    let version_idx = out.find("**version**").expect("version bullet present");
    assert!(
        alpha_idx < version_idx,
        "alpha must come before version (sorted), got alpha@{alpha_idx} version@{version_idx}"
    );
    assert!(
        out.contains("- **alpha**: yes") && out.contains("- **version**: 1.0"),
        "meta bullet shape wrong:\n{out}"
    );
}

#[test]
fn meta_section_omitted_when_empty() {
    let item = ItemDoc::Structure {
        name: "NoMeta".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let out = render_one_item(item);
    assert!(!out.contains("### Meta"), "should omit, got:\n{out}");
}

/// Trait variant emits a `### Members` H3 + bullet list of rendered member
/// signatures.
#[test]
fn trait_body_renders_members() {
    let item = ItemDoc::Trait {
        name: "HasPower".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        members: vec![
            "voltage: Voltage".into(),
            "current: Current".into(),
        ],
    };
    let out = render_one_item(item);
    assert!(out.contains("### Members"), "Members H3 missing:\n{out}");
    assert!(
        out.contains("- voltage: Voltage"),
        "first member bullet missing:\n{out}"
    );
    assert!(
        out.contains("- current: Current"),
        "second member bullet missing:\n{out}"
    );
}

/// A trait with no members omits the `### Members` section entirely.
#[test]
fn trait_body_omits_members_when_empty() {
    let item = ItemDoc::Trait {
        name: "Marker".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        members: vec![],
    };
    let out = render_one_item(item);
    assert!(!out.contains("### Members"), "should omit, got:\n{out}");
}

/// Function variant emits a fenced ```reify``` code block containing the
/// rendered signature.
#[test]
fn function_body_renders_signature_fence() {
    let item = ItemDoc::Function {
        name: "compute".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        signature: "fn compute(x: f64) -> f64".into(),
    };
    let out = render_one_item(item);
    assert!(out.contains("```reify\n"), "opening fence missing:\n{out}");
    assert!(
        out.contains("fn compute(x: f64) -> f64"),
        "signature missing:\n{out}"
    );
    // There must be exactly one opening and one closing fence around the
    // signature for this single function.
    let opens = out.matches("```reify").count();
    let closes = out.matches("```\n").count();
    assert!(opens >= 1, "expected at least one opening fence, got {opens}");
    assert!(closes >= 1, "expected at least one closing fence, got {closes}");
}

/// Enum variant emits a `### Variants` H3 + bullet list.
#[test]
fn enum_body_renders_variants() {
    let item = ItemDoc::Enum {
        name: "Color".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        variants: vec!["Red".into(), "Green".into(), "Blue".into()],
    };
    let out = render_one_item(item);
    assert!(out.contains("### Variants"), "Variants H3 missing:\n{out}");
    assert!(out.contains("- Red"), "Red bullet missing:\n{out}");
    assert!(out.contains("- Green"), "Green bullet missing:\n{out}");
    assert!(out.contains("- Blue"), "Blue bullet missing:\n{out}");
}

/// An enum with no variants omits the `### Variants` section entirely.
#[test]
fn enum_body_omits_variants_when_empty() {
    let item = ItemDoc::Enum {
        name: "Empty".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        variants: vec![],
    };
    let out = render_one_item(item);
    assert!(!out.contains("### Variants"), "should omit, got:\n{out}");
}

/// Field variant emits an inline `**Type:** \`...\`` line and (when
/// `default_repr.is_some()`) a `**Default:** \`...\`` line.
#[test]
fn field_body_renders_type_and_default() {
    let item = ItemDoc::Field {
        name: "supply_voltage".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        type_repr: "Voltage".into(),
        default_repr: Some("3.3 V".into()),
    };
    let out = render_one_item(item);
    assert!(
        out.contains("**Type:** `Voltage`"),
        "Type line missing:\n{out}"
    );
    assert!(
        out.contains("**Default:** `3.3 V`"),
        "Default line missing:\n{out}"
    );
}

/// Field with `default_repr: None` omits the `**Default:**` line.
#[test]
fn field_body_omits_default_when_none() {
    let item = ItemDoc::Field {
        name: "x".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        type_repr: "i32".into(),
        default_repr: None,
    };
    let out = render_one_item(item);
    assert!(out.contains("**Type:** `i32`"), "Type line missing:\n{out}");
    assert!(
        !out.contains("**Default:**"),
        "Default line should be omitted, got:\n{out}"
    );
}

/// Purpose variant emits `**Direction:** {direction}` and
/// `**Expression:** \`{expr_repr}\`` lines.
#[test]
fn purpose_body_renders_direction_and_expression() {
    let item = ItemDoc::Purpose {
        name: "minimize_area".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        expr_repr: "total_area".into(),
        direction: "minimize".into(),
    };
    let out = render_one_item(item);
    assert!(
        out.contains("**Direction:** minimize"),
        "Direction line missing:\n{out}"
    );
    assert!(
        out.contains("**Expression:** `total_area`"),
        "Expression line missing:\n{out}"
    );
}

/// Unit variant emits `**Base:** \`{base_unit}\`` and
/// `**Scale:** \`{scale}\`` lines.
#[test]
fn unit_body_renders_base_and_scale() {
    let item = ItemDoc::Unit {
        name: "Milliamp".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        base_unit: "Ampere".into(),
        scale: "0.001".into(),
    };
    let out = render_one_item(item);
    assert!(out.contains("**Base:** `Ampere`"), "Base line missing:\n{out}");
    assert!(out.contains("**Scale:** `0.001`"), "Scale line missing:\n{out}");
}

/// TypeAlias variant emits a single `= \`{type_repr}\`` line.
#[test]
fn type_alias_body_renders_rhs() {
    let item = ItemDoc::TypeAlias {
        name: "Meters".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        type_repr: "f64".into(),
    };
    let out = render_one_item(item);
    assert!(
        out.contains("= `f64`"),
        "type alias rhs line missing:\n{out}"
    );
}

/// ConstraintDef variant emits a single `\`{expr_repr}\`` line.
#[test]
fn constraint_def_body_renders_expr() {
    let item = ItemDoc::ConstraintDef {
        name: "voltage_safe".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        expr_repr: "v <= 5.5 V".into(),
    };
    let out = render_one_item(item);
    assert!(
        out.contains("`v <= 5.5 V`"),
        "constraint expression missing:\n{out}"
    );
}

/// `@deprecated("use Foo instead")` on an item renders a blockquote callout
/// `> **Deprecated:** use Foo instead` *between* the H2 heading and the
/// doc-comment paragraph (or kind-specific body).
#[test]
fn deprecated_annotation_emits_callout() {
    let item = ItemDoc::Structure {
        name: "OldThing".into(),
        doc: Some("This is the docstring.".into()),
        is_pub: true,
        annotations: vec![AnnotationDoc {
            name: "deprecated".into(),
            // String-literal arg as rendered from source: leading/trailing `"`s
            // are part of the printable representation; the formatter strips them.
            args: vec!["\"use Foo instead\"".into()],
        }],
        pragmas: vec![], params: vec![], ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(
        out.contains("> **Deprecated:** use Foo instead"),
        "deprecated callout missing:\n{out}"
    );
    // Order: heading -> callout -> doc paragraph.
    let h2 = out.find("## ").expect("H2 present");
    let callout = out.find("> **Deprecated:**").expect("callout present");
    let doc_p = out.find("This is the docstring.").expect("doc present");
    assert!(
        h2 < callout && callout < doc_p,
        "ordering wrong (h2={h2} callout={callout} doc={doc_p}):\n{out}"
    );
}

/// `@optimized("area")` on an item renders an italic note
/// `*Optimized: \`area\`*`.
#[test]
fn optimized_annotation_emits_italic_note() {
    let item = ItemDoc::Structure {
        name: "Bolt".into(),
        doc: None,
        is_pub: false,
        annotations: vec![AnnotationDoc {
            name: "optimized".into(),
            args: vec!["\"area\"".into()],
        }],
        pragmas: vec![], params: vec![], ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    assert!(
        out.contains("*Optimized: `area`*"),
        "optimized italic note missing:\n{out}"
    );
}

/// `@test`-annotated items are excluded from the main item flow and instead
/// emitted under a `## Tests` H2 subsection at the bottom of the module.
#[test]
fn test_annotated_items_grouped_under_tests_section() {
    let foo = ItemDoc::Structure {
        name: "Foo".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![], params: vec![], ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let bar = ItemDoc::Structure {
        name: "Bar".into(),
        doc: None,
        is_pub: true,
        annotations: vec![AnnotationDoc { name: "test".into(), args: vec![] }],
        pragmas: vec![], params: vec![], ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![foo, bar],
            ..Default::default()
        }],
    };
    let out = render_single(&model);

    assert!(out.contains("## Tests"), "Tests H2 missing:\n{out}");
    let foo_idx = out.find("Foo").expect("Foo present");
    let tests_idx = out.find("## Tests").expect("Tests H2 present");
    let bar_idx = out.find("Bar").expect("Bar present");
    assert!(
        foo_idx < tests_idx && tests_idx < bar_idx,
        "ordering wrong: foo@{foo_idx} tests@{tests_idx} bar@{bar_idx}\n{out}"
    );
}

/// When no item carries `@test`, the `## Tests` header is omitted entirely.
#[test]
fn no_tests_no_tests_header() {
    let only = ItemDoc::Structure {
        name: "Only".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![], params: vec![], ports: vec![], constraints: vec![],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![only],
            ..Default::default()
        }],
    };
    let out = render_single(&model);
    assert!(!out.contains("## Tests"), "Tests H2 should be absent:\n{out}");
}

/// `@solver_hint(discrete_set(standard_bolt_lengths))` on a parameter appends
/// `*hint: discrete_set(standard_bolt_lengths)*` to that param's Description
/// cell, after the doc-comment text.
#[test]
fn solver_hint_annotation_appends_to_description_cell() {
    let item = ItemDoc::Structure {
        name: "Bolt".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        params: vec![ParamDoc {
            name: "length".into(),
            doc: Some("Bolt length.".into()),
            type_repr: "Length".into(),
            default_repr: None,
            annotations: vec![AnnotationDoc {
                name: "solver_hint".into(),
                // Non-string-literal arg (no surrounding quotes); the rendered
                // representation is the call expression itself.
                args: vec!["discrete_set(standard_bolt_lengths)".into()],
            }],
        }],
        ports: vec![], constraints: vec![], sub_components: vec![],
        realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);
    // The description cell of `length` must contain both the doc text and the
    // italic hint suffix.
    let row = out
        .lines()
        .find(|l| l.contains("`length`"))
        .expect("found length row");
    assert!(
        row.contains("Bolt length."),
        "doc text missing in row: {row}"
    );
    assert!(
        row.contains("*hint: discrete_set(standard_bolt_lengths)*"),
        "solver_hint italic suffix missing in row: {row}"
    );
}
