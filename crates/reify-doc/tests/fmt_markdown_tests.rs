//! Behavioural tests for the GitHub-flavored Markdown formatter (`fmt_markdown`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_markdown.rs` so that golden snapshots can be loaded via `include_str!` from
//! sibling `tests/snapshots/` files without polluting the library binary.

use reify_doc::fmt_markdown::{render_markdown, MarkdownOptions, MarkdownOutput};
use reify_doc::model::{DocModel, ItemDoc, ModuleDoc};

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
