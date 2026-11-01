//! Every formatter renders an expression-valued field the same way: source text
//! as code, and a missing source as `ExprText`'s one placeholder, outside code.

use std::collections::BTreeSet;

use reify_doc::expr_text::{ExprText, UnavailableReason};
use reify_doc::fmt_html::render_html;
use reify_doc::fmt_json::render_json;
use reify_doc::fmt_markdown::{MarkdownOptions, MarkdownOutput, render_markdown};
use reify_doc::model::{
    ConstraintDoc, DocModel, ItemDoc, ItemHeader, ItemKind, ModuleDoc, ParamDoc,
};

const PLACEHOLDER_PREFIX: &str = "source unavailable (";

/// How many fields of [`fixture_model`] carry each unavailable reason.
const EXPECTED_PLACEHOLDER_COUNTS: [(UnavailableReason, usize); 3] = [
    (UnavailableReason::DeclarationWithoutValue, 1),
    (UnavailableReason::SpanOutsideSource, 2),
    (UnavailableReason::NoSpan, 2),
];

fn header(name: &str) -> ItemHeader {
    ItemHeader {
        name: name.into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
    }
}

fn param(name: &str, default_repr: Option<ExprText>) -> ParamDoc {
    ParamDoc {
        name: name.into(),
        doc: None,
        type_repr: "Length".into(),
        default_repr,
        annotations: vec![],
    }
}

fn constraint(expr_repr: ExprText) -> ConstraintDoc {
    ConstraintDoc {
        label: None,
        expr_repr,
        annotations: vec![],
        line: None,
    }
}

fn fixture_model() -> DocModel {
    let structure = ItemDoc {
        header: header("S"),
        kind: ItemKind::Structure {
            params: vec![
                param("w", Some(ExprText::Source("10 mm".into()))),
                param(
                    "h",
                    Some(ExprText::Unavailable(
                        UnavailableReason::DeclarationWithoutValue,
                    )),
                ),
            ],
            ports: vec![],
            constraints: vec![
                constraint(ExprText::Source("w > h".into())),
                constraint(ExprText::Unavailable(UnavailableReason::SpanOutsideSource)),
            ],
            sub_components: vec![],
            realizations: vec![],
            meta: vec![],
        },
    };
    let field = ItemDoc {
        header: header("f"),
        kind: ItemKind::Field {
            type_repr: "Length".into(),
            default_repr: Some(ExprText::Unavailable(UnavailableReason::NoSpan)),
        },
    };
    let purpose = ItemDoc {
        header: header("p"),
        kind: ItemKind::Purpose {
            expr_repr: ExprText::Unavailable(UnavailableReason::NoSpan),
            direction: "minimize".into(),
        },
    };
    let unusable_constraint_def = ItemDoc {
        header: header("c"),
        kind: ItemKind::ConstraintDef {
            expr_repr: ExprText::Unavailable(UnavailableReason::SpanOutsideSource),
        },
    };
    let constraint_def = ItemDoc {
        header: header("ok"),
        kind: ItemKind::ConstraintDef {
            expr_repr: ExprText::Source("x".into()),
        },
    };
    DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![
                structure,
                field,
                purpose,
                unusable_constraint_def,
                constraint_def,
            ],
            ..Default::default()
        }],
    }
}

fn single_markdown(model: &DocModel) -> String {
    match render_markdown(model, None, &MarkdownOptions::default()) {
        MarkdownOutput::Single(body) => body,
        MarkdownOutput::Split(_) => panic!("default MarkdownOptions must render a single file"),
    }
}

/// Every placeholder in `rendered`, read as the text between `open` and
/// `close` around each occurrence of the shared placeholder prefix.  Panics
/// when a placeholder is not wrapped in exactly that markup.
fn wrapped_placeholders(rendered: &str, open: &str, close: &str) -> Vec<String> {
    rendered
        .match_indices(PLACEHOLDER_PREFIX)
        .map(|(start, _)| {
            assert!(
                rendered[..start].ends_with(open),
                "placeholder at byte {start} is not preceded by {open:?}:\n{rendered}"
            );
            let rest = &rendered[start..];
            let end = rest.find(close).unwrap_or_else(|| {
                panic!("placeholder at byte {start} is not closed by {close:?}:\n{rendered}")
            });
            rest[..end].to_string()
        })
        .collect()
}

#[test]
fn html_renders_source_as_code_and_unavailable_as_the_shared_placeholder() {
    let html = render_html(&fixture_model(), None);

    for source in [
        "<td><code>10 mm</code></td>",
        "<code>w &gt; h</code>",
        "<p><code>x</code></p>",
    ] {
        assert!(html.contains(source), "missing {source:?} in:\n{html}");
    }
    for (reason, expected) in EXPECTED_PLACEHOLDER_COUNTS {
        let placeholder = ExprText::Unavailable(reason).to_string();
        assert_eq!(
            html.matches(&format!("<em>{placeholder}</em>")).count(),
            expected,
            "occurrences of the {reason:?} placeholder in:\n{html}"
        );
        assert!(
            !html.contains(&format!("<code>{placeholder}")),
            "the {reason:?} placeholder must never render as code:\n{html}"
        );
    }
}

#[test]
fn markdown_renders_source_as_code_and_unavailable_as_the_shared_placeholder() {
    let markdown = single_markdown(&fixture_model());

    let w_row = markdown
        .lines()
        .find(|line| line.starts_with("| `w` |"))
        .unwrap_or_else(|| panic!("no params row for `w` in:\n{markdown}"));
    assert!(
        w_row.contains("`10 mm`"),
        "w's default is not code: {w_row}"
    );
    for (reason, expected) in EXPECTED_PLACEHOLDER_COUNTS {
        let placeholder = ExprText::Unavailable(reason).to_string();
        assert_eq!(
            markdown.matches(&format!("*{placeholder}*")).count(),
            expected,
            "occurrences of the {reason:?} placeholder in:\n{markdown}"
        );
        assert!(
            !markdown.contains(&format!("`{placeholder}")),
            "the {reason:?} placeholder must never render as code:\n{markdown}"
        );
    }
    let h_row = markdown
        .lines()
        .find(|line| line.starts_with("| `h` |"))
        .unwrap_or_else(|| panic!("no params row for `h` in:\n{markdown}"));
    assert_eq!(
        h_row.matches('|').count(),
        6,
        "the placeholder must keep the params row at five cells: {h_row}"
    );
}

#[test]
fn json_carries_source_as_string_and_unavailable_as_typed_reason() {
    let model = fixture_model();
    let json = render_json(&model, true);

    for fragment in [
        "\"default_repr\":\"10 mm\"",
        "{\"unavailable\":\"declaration_without_value\"}",
        "{\"unavailable\":\"span_outside_source\"}",
        "{\"unavailable\":\"no_span\"}",
    ] {
        assert!(json.contains(fragment), "missing {fragment:?} in:\n{json}");
    }
    assert!(
        !json.contains(PLACEHOLDER_PREFIX),
        "JSON carries the typed reason, never the display text:\n{json}"
    );
    let back: DocModel = serde_json::from_str(&json).expect("rendered JSON must decode");
    assert_eq!(back, model);
}

#[test]
fn every_formatter_uses_the_same_placeholder_text() {
    let model = fixture_model();
    let expected: BTreeSet<String> = EXPECTED_PLACEHOLDER_COUNTS
        .iter()
        .map(|(reason, _)| ExprText::Unavailable(*reason).to_string())
        .collect();

    let html: BTreeSet<String> = wrapped_placeholders(&render_html(&model, None), "<em>", "</em>")
        .into_iter()
        .collect();
    let markdown: BTreeSet<String> = wrapped_placeholders(&single_markdown(&model), "*", "*")
        .into_iter()
        .collect();

    assert_eq!(
        html, expected,
        "html placeholders differ from ExprText's Display"
    );
    assert_eq!(
        markdown, expected,
        "markdown placeholders differ from ExprText's Display"
    );
}
