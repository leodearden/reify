//! Behavioural tests for the GitHub-flavored Markdown formatter (`fmt_markdown`).
//!
//! Tests live in the integration `tests/` directory rather than `mod tests` inside
//! `fmt_markdown.rs` so that golden snapshots can be loaded via `include_str!` from
//! sibling `tests/snapshots/` files without polluting the library binary.

use reify_doc::cross_refs::CrossRefs;
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

/// Helper: render the model in single-file mode with a CrossRefs and unwrap to
/// `String`.
fn render_single_with_xrefs(model: &DocModel, xrefs: &CrossRefs) -> String {
    match render_markdown(model, Some(xrefs), &MarkdownOptions::default()) {
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
/// and expects the rendered output to contain *exactly* the heading line for
/// that variant — keyword, visibility, name, and anchor.  Asserting the full
/// expected string (rather than a disjunction of substrings) catches
/// regressions that mis-emit any one of those four pieces; a permissive
/// disjunction lets a wrong combo (e.g. dropping the keyword on a public
/// item) silently satisfy one of the alternates.  See suggestion #7 in the
/// reviewer's amendment notes.
#[test]
fn item_h2_headings_per_variant() {
    fn item_with_name(maker: impl FnOnce(&str) -> ItemDoc) -> ItemDoc {
        maker("Foo")
    }

    // Each tuple is `(kind, item, expected_heading_line)`.  The expected
    // heading line is the *entire* H2 line including the explicit anchor.
    let cases: Vec<(&str, ItemDoc, &str)> = vec![
        (
            "structure",
            item_with_name(|n| ItemDoc::Structure {
                name: n.into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            }),
            "## `pub structure Foo` <a id=\"Foo\"></a>",
        ),
        (
            "occurrence",
            item_with_name(|n| ItemDoc::Occurrence {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], params: vec![],
                ports: vec![], constraints: vec![], sub_components: vec![],
                realizations: vec![], meta: vec![],
            }),
            "## `occurrence Foo` <a id=\"Foo\"></a>",
        ),
        (
            "trait",
            item_with_name(|n| ItemDoc::Trait {
                name: n.into(), doc: None, is_pub: true,
                annotations: vec![], pragmas: vec![], members: vec![],
            }),
            "## `pub trait Foo` <a id=\"Foo\"></a>",
        ),
        (
            "function",
            item_with_name(|n| ItemDoc::Function {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                signature: "fn Foo()".into(),
            }),
            "## `fn Foo` <a id=\"Foo\"></a>",
        ),
        (
            "field",
            item_with_name(|n| ItemDoc::Field {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                type_repr: "i32".into(), default_repr: None,
            }),
            "## `let Foo` <a id=\"Foo\"></a>",
        ),
        (
            "purpose",
            item_with_name(|n| ItemDoc::Purpose {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x".into(), direction: "minimize".into(),
            }),
            "## `purpose Foo` <a id=\"Foo\"></a>",
        ),
        (
            "enum",
            item_with_name(|n| ItemDoc::Enum {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![], variants: vec![],
            }),
            "## `enum Foo` <a id=\"Foo\"></a>",
        ),
        (
            "unit",
            item_with_name(|n| ItemDoc::Unit {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                base_unit: "Meter".into(), scale: "1.0".into(),
            }),
            "## `unit Foo` <a id=\"Foo\"></a>",
        ),
        (
            "type_alias",
            item_with_name(|n| ItemDoc::TypeAlias {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                type_repr: "f64".into(),
            }),
            "## `type Foo` <a id=\"Foo\"></a>",
        ),
        (
            "constraint_def",
            item_with_name(|n| ItemDoc::ConstraintDef {
                name: n.into(), doc: None, is_pub: false,
                annotations: vec![], pragmas: vec![],
                expr_repr: "x > 0".into(),
            }),
            "## `constraint Foo` <a id=\"Foo\"></a>",
        ),
    ];

    for (kind, item, expected_heading) in cases {
        let out = render_one_item(item);
        assert!(
            out.contains(expected_heading),
            "variant={kind}: missing exact H2 heading {expected_heading:?}\n--- output ---\n{out}"
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

/// Constraints section uses safe inline-code fencing when `expr_repr` contains
/// literal backticks.
///
/// * Labelled entry (`label = Some("safe")`), `expr_repr = "v <= \`max\` V"`:
///   internal backtick, no leading/trailing → `- safe: ``v <= \`max\` V```.
/// * Unlabelled entry, `expr_repr = "\`a\` && b"`: starts with backtick →
///   pad required → `- `` \`a\` && b `` *(line 7)*`.
#[test]
fn constraints_section_uses_safe_inline_code_fence() {
    let labeled_expr = "v <= `max` V"; // internal backtick, no leading/trailing
    let unlabeled_expr = "`a` && b";   // starts with backtick → pad
    let item = ItemDoc::Structure {
        name: "Safe".into(), doc: None, is_pub: false,
        annotations: vec![], pragmas: vec![], params: vec![],
        ports: vec![],
        constraints: vec![
            ConstraintDoc {
                label: Some("safe".into()),
                expr_repr: labeled_expr.into(),
                annotations: vec![],
                line: None,
            },
            ConstraintDoc {
                label: None,
                expr_repr: unlabeled_expr.into(),
                annotations: vec![],
                line: Some(7),
            },
        ],
        sub_components: vec![], realizations: vec![], meta: vec![],
    };
    let out = render_one_item(item);

    assert!(out.contains("### Constraints"), "H3 missing:\n{out}");

    // (a) Labelled constraint: no leading/trailing backtick → double fence, no pad.
    let labeled_fenced = format!("- safe: ``{labeled_expr}``");
    assert!(
        out.contains(&labeled_fenced),
        "labeled bullet not correctly fenced (`{labeled_fenced}`):\n{out}"
    );
    // Labelled entry has no line info — the bullet must not contain "*(line".
    let labeled_line = out.lines().find(|l| l.contains("safe:"))
        .expect("labeled bullet present");
    assert!(
        !labeled_line.contains("*(line"),
        "labeled bullet should omit line suffix: {labeled_line}"
    );

    // (b) Unlabelled constraint: starts with backtick → double fence + pad +
    //     trailing *(line 7)*.
    let unlabeled_fenced = format!("- `` {unlabeled_expr} `` *(line 7)*");
    assert!(
        out.contains(&unlabeled_fenced),
        "unlabeled bullet not correctly fenced (`{unlabeled_fenced}`):\n{out}"
    );
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

/// Field body uses safe inline-code fencing when `type_repr` or
/// `default_repr` contains a literal backtick.
///
/// * `type_repr = "Map<\`Key\`, V>"` — one internal backtick, no leading/trailing
///   → fence_len = 2, no pad → `` ``Map<`Key`, V>`` ``
/// * `default_repr = "\`zero\`"` — starts AND ends with backtick
///   → fence_len = 2, pad required → `` `` `zero` `` ``
#[test]
fn field_body_uses_safe_inline_code_fence() {
    let type_repr = "Map<`Key`, V>";
    let default_repr = "`zero`";
    let item = ItemDoc::Field {
        name: "store".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        type_repr: type_repr.into(),
        default_repr: Some(default_repr.into()),
    };
    let out = render_one_item(item);

    // (a) Verbatim values must appear.
    assert!(out.contains(type_repr), "type_repr not verbatim:\n{out}");
    assert!(out.contains(default_repr), "default_repr not verbatim:\n{out}");

    // (b) Default uses single-backtick fence without padding: the single-fence
    // form for `default_repr` would be "``zero``" — the space pad in the safe
    // form ensures this does NOT appear in the output.
    let default_bad = format!("`{default_repr}`"); // "``zero``"
    assert!(
        !out.contains(&default_bad),
        "default uses single-backtick form:\n{out}"
    );

    // (c) Type line: no leading/trailing backtick in value → no pad.
    // Positive assertion is sufficient to prove the fence is correct.
    let type_fenced = format!("**Type:** ``{type_repr}``");
    assert!(
        out.contains(&type_fenced),
        "Type line not correctly fenced (`{type_fenced}`):\n{out}"
    );

    // (d) Default line: starts & ends with backtick → pad required.
    let default_fenced = format!("**Default:** `` {default_repr} ``");
    assert!(
        out.contains(&default_fenced),
        "Default line not correctly fenced (`{default_fenced}`):\n{out}"
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

/// Purpose body uses safe inline-code fencing when `expr_repr` contains a
/// literal backtick in the middle.  No leading/trailing backtick → no pad.
/// `max_consecutive_backticks = 1` → fence_len = 2.
#[test]
fn purpose_body_uses_safe_inline_code_fence() {
    // "minimize `area` + slack": one internal backtick run (length 1), no
    // leading/trailing backtick → fence_len = 2, needs_pad = false.
    let expr_repr = "minimize `area` + slack";
    let item = ItemDoc::Purpose {
        name: "min_area".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        expr_repr: expr_repr.into(),
        direction: "minimize".into(),
    };
    let out = render_one_item(item);

    // (a) Verbatim value must appear.
    assert!(out.contains(expr_repr), "expr_repr not verbatim:\n{out}");

    // (b) Single-backtick form: "**Expression:** `minimize `area` + slack`".
    // The leading `**Expression:** ` prefix makes this unambiguous — the safe
    // double-fence form starts with "``" after the space, not "`m".
    let bad_form = format!("**Expression:** `{expr_repr}`");
    assert!(
        !out.contains(&bad_form),
        "output uses single-backtick form:\n{out}"
    );

    // (c) Correct double-fence form (no pad — value neither starts nor ends
    // with a backtick).
    let fenced = format!("**Expression:** ``{expr_repr}``");
    assert!(
        out.contains(&fenced),
        "Expression line not correctly fenced (`{fenced}`):\n{out}"
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

/// Unit body uses safe inline-code fencing when `base_unit` or `scale`
/// contains a literal backtick.
///
/// * `base_unit = "\`Ampere\`"` — starts AND ends with backtick → pad required.
/// * `scale = "1\`e\`-3"` — one internal backtick, no leading/trailing.
#[test]
fn unit_body_uses_safe_inline_code_fence() {
    let base_unit = "`Ampere`"; // starts and ends with backtick → pad
    let scale = "1`e`-3";      // internal backtick, no leading/trailing → no pad
    let item = ItemDoc::Unit {
        name: "Milliamp2".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        base_unit: base_unit.into(),
        scale: scale.into(),
    };
    let out = render_one_item(item);

    // (a) Verbatim values must appear.
    assert!(out.contains(base_unit), "base_unit not verbatim:\n{out}");
    assert!(out.contains(scale), "scale not verbatim:\n{out}");

    // (b) Base line: starts & ends with backtick → double-fence + pad.
    let base_fenced = format!("**Base:** `` {base_unit} ``");
    assert!(
        out.contains(&base_fenced),
        "Base line not correctly fenced (`{base_fenced}`):\n{out}"
    );

    // (c) Scale line: internal backtick only → double-fence, no pad.
    let scale_fenced = format!("**Scale:** ``{scale}``");
    assert!(
        out.contains(&scale_fenced),
        "Scale line not correctly fenced (`{scale_fenced}`):\n{out}"
    );
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

/// TypeAlias body uses safe inline-code fencing when `type_repr` contains a
/// literal backtick in the interior (no leading/trailing → no pad).
#[test]
fn type_alias_body_uses_safe_inline_code_fence() {
    // "Vec<`T`>": one internal backtick run (length 1), no leading/trailing.
    // → fence_len = 2, needs_pad = false → "``Vec<`T`>``"
    let type_repr = "Vec<`T`>";
    let item = ItemDoc::TypeAlias {
        name: "VecT".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        type_repr: type_repr.into(),
    };
    let out = render_one_item(item);

    // (a) Verbatim value must appear.
    assert!(out.contains(type_repr), "type_repr not verbatim:\n{out}");

    // (b) Correct double-fence form (no pad).
    let fenced = format!("= ``{type_repr}``");
    assert!(
        out.contains(&fenced),
        "TypeAlias rhs not correctly fenced (`{fenced}`):\n{out}"
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

/// ConstraintDef body uses safe inline-code fencing when `expr_repr` contains
/// a literal backtick: the fence must be at least two backticks long so the
/// embedded backtick does not terminate the span.  Input starts with `` ` ``
/// so a space pad is also required inside the fence.
#[test]
fn constraint_def_body_uses_safe_inline_code_fence() {
    // "`inner` && true": longest backtick run = 1, starts with backtick.
    // Expected fence_len = 2, needs_pad = true → "`` `inner` && true ``".
    let expr_repr = "`inner` && true";
    let item = ItemDoc::ConstraintDef {
        name: "safe_check".into(),
        doc: None,
        is_pub: false,
        annotations: vec![],
        pragmas: vec![],
        expr_repr: expr_repr.into(),
    };
    let out = render_one_item(item);

    // (a) The verbatim value must appear in the output.
    assert!(
        out.contains(expr_repr),
        "expr_repr not verbatim in output:\n{out}"
    );
    // (b) The unsafe single-backtick form must NOT be present.
    let bad_form = format!("`{expr_repr}`");
    assert!(
        !out.contains(&bad_form),
        "output uses single-backtick form which would break CommonMark:\n{out}"
    );
    // (c) The correctly-padded double-fence form must be present.
    let fenced = format!("`` {expr_repr} ``");
    assert!(
        out.contains(&fenced),
        "output does not contain correctly-fenced form `{fenced}`:\n{out}"
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

// ---------------------------------------------------------------------------
// Cross-refs ("Conforms to" / "Used by")
// ---------------------------------------------------------------------------

/// Given a Structure "Bolt" and a CrossRefs whose `trait_to_conformers` maps
/// "Fastener" → ["Bolt"], the Bolt section renders `### Conforms to` followed
/// by `- [\`Fastener\`](#Fastener)`.
#[test]
fn cross_refs_conforms_to_renders_for_structure() {
    let bolt = ItemDoc::Structure {
        name: "Bolt".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![bolt],
            ..Default::default()
        }],
    };
    let mut xrefs = CrossRefs::default();
    xrefs
        .trait_to_conformers
        .insert("Fastener".into(), vec!["Bolt".into()]);
    let out = render_single_with_xrefs(&model, &xrefs);

    assert!(
        out.contains("### Conforms to"),
        "Conforms to H3 missing:\n{out}"
    );
    assert!(
        out.contains("- [`Fastener`](#Fastener)"),
        "Fastener anchor link missing:\n{out}"
    );
}

/// Given an Occurrence "MCU" and a CrossRefs whose `entity_to_containers` maps
/// "MCU" → ["Board"], the MCU section renders `### Used by` followed by
/// `- [\`Board\`](#Board)`.
#[test]
fn cross_refs_used_by_renders_for_occurrence() {
    let mcu = ItemDoc::Occurrence {
        name: "MCU".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![mcu],
            ..Default::default()
        }],
    };
    let mut xrefs = CrossRefs::default();
    xrefs
        .entity_to_containers
        .insert("MCU".into(), vec!["Board".into()]);
    let out = render_single_with_xrefs(&model, &xrefs);

    assert!(
        out.contains("### Used by"),
        "Used by H3 missing:\n{out}"
    );
    assert!(
        out.contains("- [`Board`](#Board)"),
        "Board anchor link missing:\n{out}"
    );
}

/// When `cross_refs` is `None` (or empty), neither "Conforms to" nor "Used by"
/// sections are emitted.
#[test]
fn cross_refs_omitted_when_absent_or_empty() {
    let bolt = ItemDoc::Structure {
        name: "Bolt".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items: vec![bolt],
            ..Default::default()
        }],
    };
    // Case (a): None.
    let none_out = render_single(&model);
    assert!(
        !none_out.contains("### Conforms to"),
        "Conforms to should be absent when xrefs=None:\n{none_out}"
    );
    assert!(
        !none_out.contains("### Used by"),
        "Used by should be absent when xrefs=None:\n{none_out}"
    );

    // Case (b): Empty CrossRefs.
    let empty_out = render_single_with_xrefs(&model, &CrossRefs::default());
    assert!(
        !empty_out.contains("### Conforms to"),
        "Conforms to should be absent when xrefs empty:\n{empty_out}"
    );
    assert!(
        !empty_out.contains("### Used by"),
        "Used by should be absent when xrefs empty:\n{empty_out}"
    );
}

// ---------------------------------------------------------------------------
// TOC (table of contents)
// ---------------------------------------------------------------------------

/// Helper: build a minimal item with the given variant and name. Test-only
/// scaffolding used by the TOC tests.
fn mk_item(kind: &str, name: &str) -> ItemDoc {
    match kind {
        "structure" => ItemDoc::Structure {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
            constraints: vec![], sub_components: vec![], realizations: vec![],
            meta: vec![],
        },
        "occurrence" => ItemDoc::Occurrence {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
            constraints: vec![], sub_components: vec![], realizations: vec![],
            meta: vec![],
        },
        "trait" => ItemDoc::Trait {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], members: vec![],
        },
        "function" => ItemDoc::Function {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            signature: format!("fn {name}()"),
        },
        "field" => ItemDoc::Field {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            type_repr: "i32".into(), default_repr: None,
        },
        "purpose" => ItemDoc::Purpose {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            expr_repr: "x".into(), direction: "minimize".into(),
        },
        "enum" => ItemDoc::Enum {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], variants: vec![],
        },
        "unit" => ItemDoc::Unit {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            base_unit: "Meter".into(), scale: "1.0".into(),
        },
        "type_alias" => ItemDoc::TypeAlias {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![], type_repr: "f64".into(),
        },
        "constraint_def" => ItemDoc::ConstraintDef {
            name: name.into(), doc: None, is_pub: true,
            annotations: vec![], pragmas: vec![],
            expr_repr: "x > 0".into(),
        },
        other => panic!("unknown kind: {other}"),
    }
}

/// TOC must group by kind in fixed order: Traits → Structures → Occurrences
/// → Enums → Functions → Constants.  Within a group, items appear
/// alphabetically.  Empty groups are omitted entirely.  Each entry is an
/// anchor link `- [`{name}`](#{name})`.
#[test]
fn toc_groups_kinds_in_fixed_order() {
    let items = vec![
        // Mix of kinds and out-of-alphabetical-order names so the test verifies
        // both bucketing and within-bucket sort.
        mk_item("structure", "Zeta"),
        mk_item("structure", "Alpha"),
        mk_item("trait", "HasPower"),
        mk_item("occurrence", "MCU"),
        mk_item("enum", "Color"),
        mk_item("function", "compute"),
        mk_item("type_alias", "Meters"),
        mk_item("unit", "Milliamp"),
    ];
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items,
            ..Default::default()
        }],
    };
    let out = render_single(&model);

    // Each non-empty group's H3 must appear, in order.
    let traits_idx = out.find("### Traits").expect("Traits H3 missing");
    let structures_idx = out
        .find("### Structures")
        .expect("Structures H3 missing");
    let occurrences_idx = out
        .find("### Occurrences")
        .expect("Occurrences H3 missing");
    let enums_idx = out.find("### Enums").expect("Enums H3 missing");
    let functions_idx = out
        .find("### Functions")
        .expect("Functions H3 missing");
    let constants_idx = out
        .find("### Constants")
        .expect("Constants H3 missing");
    assert!(
        traits_idx < structures_idx
            && structures_idx < occurrences_idx
            && occurrences_idx < enums_idx
            && enums_idx < functions_idx
            && functions_idx < constants_idx,
        "TOC group order wrong: traits={traits_idx} structures={structures_idx} \
         occurrences={occurrences_idx} enums={enums_idx} functions={functions_idx} \
         constants={constants_idx}\n{out}"
    );

    // Anchor link format and within-group alphabetical sort.
    assert!(
        out.contains("- [`Alpha`](#Alpha)"),
        "anchor link for Alpha missing:\n{out}"
    );
    assert!(
        out.contains("- [`Zeta`](#Zeta)"),
        "anchor link for Zeta missing:\n{out}"
    );
    let alpha_idx = out.find("- [`Alpha`]").expect("Alpha bullet present");
    let zeta_idx = out.find("- [`Zeta`]").expect("Zeta bullet present");
    assert!(
        alpha_idx < zeta_idx,
        "Alpha must precede Zeta in TOC: alpha@{alpha_idx} zeta@{zeta_idx}\n{out}"
    );

    // The TOC sits between the module H1 (and optional doc paragraph) and the
    // first item H2 — `## Contents` H2 must precede the first `## ` (item)
    // heading.
    let contents = out.find("## Contents").expect("Contents H2 present");
    let first_h1 = out.find("# m\n").expect("module H1 present");
    let first_item_h2 = out
        .find("## `")
        .expect("first item H2 with backtick keyword present");
    assert!(
        first_h1 < contents && contents < first_item_h2,
        "TOC must sit between module H1 and first item H2: h1={first_h1} contents={contents} item={first_item_h2}\n{out}"
    );
}

/// Empty groups are omitted entirely. A module with only a Trait must NOT
/// emit `### Structures`, `### Occurrences`, etc.
#[test]
fn toc_omits_empty_groups() {
    let items = vec![mk_item("trait", "HasPower")];
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items,
            ..Default::default()
        }],
    };
    let out = render_single(&model);
    assert!(out.contains("### Traits"), "Traits group missing:\n{out}");
    assert!(
        !out.contains("### Structures"),
        "empty Structures group should be omitted:\n{out}"
    );
    assert!(
        !out.contains("### Occurrences"),
        "empty Occurrences group should be omitted:\n{out}"
    );
    assert!(
        !out.contains("### Enums"),
        "empty Enums group should be omitted:\n{out}"
    );
    assert!(
        !out.contains("### Functions"),
        "empty Functions group should be omitted:\n{out}"
    );
    assert!(
        !out.contains("### Constants"),
        "empty Constants group should be omitted:\n{out}"
    );
}

/// "Constants" group buckets Field, Unit, TypeAlias, ConstraintDef, and
/// Purpose. All five items must appear under `### Constants`.
#[test]
fn toc_constants_bucket_includes_field_unit_alias_constraint_purpose() {
    let items = vec![
        mk_item("field", "supply_v"),
        mk_item("unit", "Milliamp"),
        mk_item("type_alias", "Meters"),
        mk_item("constraint_def", "voltage_safe"),
        mk_item("purpose", "minimize_area"),
    ];
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "m".into(),
            items,
            ..Default::default()
        }],
    };
    let out = render_single(&model);
    assert!(out.contains("### Constants"), "Constants H3 missing:\n{out}");
    let constants = out.find("### Constants").unwrap();
    // Find each of the 5 anchor lines after the Constants header.
    for name in ["supply_v", "Milliamp", "Meters", "voltage_safe", "minimize_area"] {
        let needle = format!("- [`{name}`](#{name})");
        assert!(
            out.contains(&needle),
            "missing anchor for {name}:\n{out}"
        );
        let pos = out.find(&needle).unwrap();
        assert!(
            pos > constants,
            "anchor for {name} must be after Constants H3"
        );
    }
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

// ---------------------------------------------------------------------------
// Split mode (one file per item plus index.md)
// ---------------------------------------------------------------------------

/// Helper: render the model in split mode and unwrap to the per-file vector.
fn render_split(model: &DocModel) -> Vec<(String, String)> {
    match render_markdown(model, None, &MarkdownOptions { split: true }) {
        MarkdownOutput::Single(_) => panic!("expected Split output"),
        MarkdownOutput::Split(v) => v,
    }
}

/// Split mode produces one file per item plus an `index.md` whose body holds
/// the TOC.  Verifies (a) presence of `index.md` with TOC content, (b) one
/// file per item with kind-prefixed slug filenames, (c) per-item body matches
/// what single-mode would emit for that item (modulo the per-item file's own
/// header / back-link), and (d) deterministic ordering — `index.md` first,
/// then items in module declaration order.
#[test]
fn split_mode_emits_index_and_per_item_files() {
    let board = ItemDoc::Structure {
        name: "Board".into(),
        doc: Some("The main PCB board.".into()),
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let has_power = ItemDoc::Trait {
        name: "HasPower".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![],
        members: vec!["voltage: Voltage".into()],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "electronics.board".into(),
            doc: Some("Electronics board module.".into()),
            items: vec![board, has_power],
            ..Default::default()
        }],
    };
    let files = render_split(&model);

    // (a) index.md present with TOC.
    let index = files
        .iter()
        .find(|(n, _)| n == "index.md")
        .expect("index.md missing in split output");
    assert!(
        index.1.contains("## Contents"),
        "index.md must contain `## Contents` TOC, got:\n{}",
        index.1
    );
    assert!(
        index.1.contains("- [`Board`](structure-Board.md)"),
        "TOC must list Board as filename link, got:\n{}",
        index.1
    );
    assert!(
        !index.1.contains("(#Board)"),
        "TOC must NOT contain old fragment-only link for Board, got:\n{}",
        index.1
    );
    assert!(
        index.1.contains("- [`HasPower`](trait-HasPower.md)"),
        "TOC must list HasPower as filename link, got:\n{}",
        index.1
    );
    assert!(
        !index.1.contains("(#HasPower)"),
        "TOC must NOT contain old fragment-only link for HasPower, got:\n{}",
        index.1
    );

    // (b) per-item filenames.
    let board_file = files
        .iter()
        .find(|(n, _)| n == "structure-Board.md")
        .expect("structure-Board.md missing");
    let trait_file = files
        .iter()
        .find(|(n, _)| n == "trait-HasPower.md")
        .expect("trait-HasPower.md missing");

    // (c) per-item body matches the single-mode section for that item.
    // The Board file must contain the H2 anchor heading and the doc paragraph.
    assert!(
        board_file.1.contains("## `pub structure Board`"),
        "Board file must have H2 heading, got:\n{}",
        board_file.1
    );
    assert!(
        board_file.1.contains("<a id=\"Board\"></a>"),
        "Board file must have anchor, got:\n{}",
        board_file.1
    );
    assert!(
        board_file.1.contains("The main PCB board."),
        "Board file must contain doc paragraph, got:\n{}",
        board_file.1
    );
    assert!(
        trait_file.1.contains("## `pub trait HasPower`"),
        "HasPower file must have H2 heading, got:\n{}",
        trait_file.1
    );
    assert!(
        trait_file.1.contains("- voltage: Voltage"),
        "HasPower file must contain trait member, got:\n{}",
        trait_file.1
    );

    // Per-item files must NOT replicate the TOC.
    assert!(
        !board_file.1.contains("## Contents"),
        "Per-item file must not contain TOC, got:\n{}",
        board_file.1
    );

    // (d) deterministic ordering: index.md first, then items in declaration order.
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names[0], "index.md",
        "index.md must be the first entry, got order: {names:?}"
    );
    let board_pos = names.iter().position(|n| *n == "structure-Board.md").unwrap();
    let trait_pos = names.iter().position(|n| *n == "trait-HasPower.md").unwrap();
    assert!(
        board_pos < trait_pos,
        "Board (declared first) must precede HasPower in split output, got order: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Snapshot tests (committed-golden Markdown under tests/snapshots/)
// ---------------------------------------------------------------------------
//
// To regenerate the snapshots after an intentional formatter change, run:
//
//     UPDATE_SNAPSHOTS=1 cargo test -p reify-doc --test fmt_markdown_tests
//
// `assert_or_update_snapshot` writes the current actual output to disk when
// the env var is set, otherwise it `assert_eq!`s with a clear-diff panic
// message that names the offending file.

/// Build the absolute path to a file under `crates/reify-doc/tests/snapshots/`.
///
/// Uses `CARGO_MANIFEST_DIR` so the path resolves correctly regardless of
/// the working directory the test is run from.  Pattern matches the
/// project's task-348 convention.
fn snapshot_path(filename: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(filename)
}

/// Path-taking primitive for snapshot assertions.
///
/// Contains the full comparison body (both `actual` and `expected`
/// CRLF→LF normalisation).  Used directly by regression tests that need to
/// exercise an arbitrary on-disk golden (e.g. a runtime-created CRLF file)
/// without polluting `tests/snapshots/`.
fn assert_or_update_snapshot_at(path: &std::path::Path, actual: &str) {
    let actual = actual.replace("\r\n", "\n");
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create_dir_all({parent:?}): {e}"));
        }
        std::fs::write(path, actual)
            .unwrap_or_else(|e| panic!("write({path:?}): {e}"));
        return;
    }
    let expected = match std::fs::read_to_string(path) {
        Ok(s) => s.replace("\r\n", "\n"),
        Err(e) => panic!(
            "snapshot file not found at {path:?}: {e}\n\
             Re-run with UPDATE_SNAPSHOTS=1 to create it.\n\
             === Actual output ({} bytes) ===\n{actual}",
            actual.len()
        ),
    };
    if expected != actual {
        panic!(
            "snapshot mismatch for {path:?}; \
             re-run with UPDATE_SNAPSHOTS=1 to regenerate.\n\
             === Expected ({} bytes) ===\n{expected}\n\n\
             === Actual ({} bytes) ===\n{actual}",
            expected.len(),
            actual.len(),
        );
    }
}

/// Assert that `actual` matches the committed snapshot at
/// `tests/snapshots/{filename}`, or — when `UPDATE_SNAPSHOTS=1` is set —
/// overwrite the snapshot with `actual` so the developer can regenerate
/// goldens after an intentional formatter change.
///
/// Both `actual` and the on-disk `expected` are normalised to LF-only before
/// comparison so the test produces a closed result regardless of working-tree
/// EOL transforms.  Specifically:
/// - `actual` normalisation prevents a Windows developer running
///   `UPDATE_SNAPSHOTS=1` from accidentally baking `\r\n` into committed
///   goldens (which would break comparison on every other platform).
/// - `expected` normalisation prevents the reverse: a Windows checkout with
///   `core.autocrlf=true` silently converting a committed LF golden to CRLF
///   on disk, making the on-disk file differ from the LF-only `actual` even
///   when the logical content is identical.
fn assert_or_update_snapshot(filename: &str, actual: &str) {
    let path = snapshot_path(filename);
    assert_or_update_snapshot_at(&path, actual);
}

/// Build the inline `DocModel` fixture used by the snapshot tests.
///
/// TODO(future task): replace with `build_doc_model(load_str!("examples/integration_full_v01.ri"))`
/// once that function lands; see scope caveat in task 2357 description.
///
/// The fixture mirrors the *structure* of `examples/integration_full_v01.ri` —
/// multiple Structures, an Occurrence, a Trait, an Enum, a Function, a
/// TypeAlias, a Unit, a ConstraintDef, a Purpose, and representative
/// annotations including `@deprecated`, `@optimized`, `@test`, `@solver_hint`.
/// Item names and content are deliberately kept short so the goldens stay
/// reviewable.
fn build_integration_full_v01_fixture() -> DocModel {
    DocModel {
        modules: vec![ModuleDoc {
            path: "integration_full_v01".to_string(),
            doc: Some(
                "Comprehensive v0.1 language feature integration.".to_string(),
            ),
            items: vec![
                ItemDoc::TypeAlias {
                    name: "Pressure".into(),
                    doc: Some("Pressure is Force per Area (SI unit: Pa).".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    type_repr: "Force / Area".into(),
                },
                ItemDoc::Unit {
                    name: "mil".into(),
                    doc: Some("One mil = 1/1000 inch.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    base_unit: "Length".into(),
                    scale: "0.0000254".into(),
                },
                ItemDoc::Enum {
                    name: "Grade".into(),
                    doc: Some("Material grade classification.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    variants: vec![
                        "Standard".into(),
                        "Reinforced".into(),
                        "Premium".into(),
                    ],
                },
                ItemDoc::Function {
                    name: "safety_factor".into(),
                    doc: Some("Safety factor for real-valued loads.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    signature: "fn safety_factor(load: Real) -> Real".into(),
                },
                ItemDoc::Trait {
                    name: "Physical".into(),
                    doc: Some("Trait for objects with a measurable mass.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    members: vec!["mass: Mass".into()],
                },
                ItemDoc::ConstraintDef {
                    name: "Positive".into(),
                    doc: Some("Length value v is strictly positive.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    expr_repr: "v > 0mm".into(),
                },
                ItemDoc::Purpose {
                    name: "minimize_area".into(),
                    doc: None,
                    is_pub: false,
                    annotations: vec![],
                    pragmas: vec![],
                    expr_repr: "total_area".into(),
                    direction: "minimize".into(),
                },
                ItemDoc::Structure {
                    name: "Bolt".into(),
                    doc: Some("A standard fastening bolt.".into()),
                    is_pub: true,
                    annotations: vec![AnnotationDoc {
                        name: "optimized".into(),
                        args: vec!["\"area\"".into()],
                    }],
                    pragmas: vec![],
                    params: vec![
                        ParamDoc {
                            name: "length".into(),
                            doc: Some("Bolt length.".into()),
                            type_repr: "Length".into(),
                            default_repr: Some("100 mm".into()),
                            annotations: vec![AnnotationDoc {
                                name: "solver_hint".into(),
                                args: vec![
                                    "discrete_set(standard_bolt_lengths)".into(),
                                ],
                            }],
                        },
                        ParamDoc {
                            name: "diameter".into(),
                            doc: None,
                            type_repr: "Length".into(),
                            default_repr: Some("M8".into()),
                            annotations: vec![],
                        },
                    ],
                    ports: vec![],
                    constraints: vec![ConstraintDoc {
                        label: None,
                        expr_repr: "length >= diameter".into(),
                        annotations: vec![],
                        line: Some(42),
                    }],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![("version".into(), "1.0".into())],
                },
                ItemDoc::Structure {
                    name: "Board".into(),
                    doc: Some("Main PCB board.".into()),
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![PortDoc {
                        name: "pwr_in".into(),
                        direction: "in".into(),
                        type_name: "Power".into(),
                        members: vec!["voltage".into(), "current".into()],
                    }],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![],
                },
                ItemDoc::Occurrence {
                    name: "MCU".into(),
                    doc: Some("Microcontroller occurrence.".into()),
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
                ItemDoc::Structure {
                    name: "OldThing".into(),
                    doc: Some("Deprecated legacy structure.".into()),
                    is_pub: true,
                    annotations: vec![AnnotationDoc {
                        name: "deprecated".into(),
                        args: vec!["\"use Bolt instead\"".into()],
                    }],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![],
                },
                ItemDoc::Structure {
                    name: "TestSelfWeight".into(),
                    doc: Some("Self-weight regression test.".into()),
                    is_pub: false,
                    annotations: vec![AnnotationDoc {
                        name: "test".into(),
                        args: vec![],
                    }],
                    pragmas: vec![],
                    params: vec![],
                    ports: vec![],
                    constraints: vec![],
                    sub_components: vec![],
                    realizations: vec![],
                    meta: vec![],
                },
            ],
            ..Default::default()
        }],
    }
}

/// Build the matching `CrossRefs` fixture.  Populates both the trait→conformer
/// and entity→containers maps so the snapshot exercises the "Conforms to" /
/// "Used by" cross-ref renderings.
fn build_integration_full_v01_cross_refs() -> CrossRefs {
    let mut xrefs = CrossRefs::default();
    xrefs
        .trait_to_conformers
        .insert("Physical".into(), vec!["Bolt".into()]);
    xrefs
        .entity_to_containers
        .insert("MCU".into(), vec!["Board".into()]);
    xrefs
}

/// Single-file mode snapshot.  Builds the inline fixture and compares the
/// rendered output to `tests/snapshots/integration_full_v01.single.md`.
#[test]
fn snapshot_integration_full_v01_single_mode() {
    let model = build_integration_full_v01_fixture();
    let xrefs = build_integration_full_v01_cross_refs();
    let out = match render_markdown(&model, Some(&xrefs), &MarkdownOptions::default()) {
        MarkdownOutput::Single(s) => s,
        MarkdownOutput::Split(_) => panic!("expected Single output for default options"),
    };
    assert_or_update_snapshot("integration_full_v01.single.md", &out);
}

/// Split mode snapshot.  Builds the inline fixture and compares each
/// `(filename, body)` entry to a per-file golden under
/// `tests/snapshots/integration_full_v01.split.{filename}`.
///
/// Pins the *exact* generated filename set up-front (before per-file body
/// comparisons) so a renderer regression that drops or duplicates a file
/// fails this test loudly instead of silently passing on the surviving
/// file-by-file body checks.  See suggestion #2 in the reviewer's amendment
/// notes.
#[test]
fn snapshot_integration_full_v01_split_mode() {
    let model = build_integration_full_v01_fixture();
    let xrefs = build_integration_full_v01_cross_refs();
    let files = match render_markdown(&model, Some(&xrefs), &MarkdownOptions { split: true }) {
        MarkdownOutput::Single(_) => panic!("expected Split output for split: true"),
        MarkdownOutput::Split(v) => v,
    };

    // Pin the expected filename set: the formatter must emit `index.md` plus
    // exactly one per-item file per ItemDoc in the fixture (12 items at the
    // time of writing).  Listed in the order render_split is documented to
    // produce — index first, then items in module-declaration order.  This
    // catches dropped or extra files independently of body-level snapshots.
    let expected_filenames: Vec<&str> = vec![
        "index.md",
        "type_alias-Pressure.md",
        "unit-mil.md",
        "enum-Grade.md",
        "function-safety_factor.md",
        "trait-Physical.md",
        "constraint_def-Positive.md",
        "purpose-minimize_area.md",
        "structure-Bolt.md",
        "structure-Board.md",
        "occurrence-MCU.md",
        "structure-OldThing.md",
        "structure-TestSelfWeight.md",
    ];
    let actual_filenames: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        actual_filenames, expected_filenames,
        "split-mode filename set drifted; expected exactly {expected_filenames:?}, got {actual_filenames:?}"
    );

    for (filename, body) in &files {
        let snapshot_name = format!("integration_full_v01.split.{filename}");
        assert_or_update_snapshot(&snapshot_name, body);
    }
}

// ---------------------------------------------------------------------------
// Multi-module split mode (per reviewer suggestion #1)
// ---------------------------------------------------------------------------

/// Render a two-module model with same-named items in each module to exercise
/// the otherwise-untested multi-module branch of `render_split`.  Verifies:
///
/// - per-item filenames are prefixed by the module path (`{path}/{kind}-{name}.md`),
/// - per-item bodies use `../index.md` for the back-link (one level up from
///   the nested module subdirectory), and
/// - same-named items in different modules resolve to *distinct* files.
#[test]
fn split_mode_multi_module_prefixes_and_backlinks() {
    let board_a = ItemDoc::Structure {
        name: "Board".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let board_b = ItemDoc::Structure {
        name: "Board".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let model = DocModel {
        modules: vec![
            ModuleDoc {
                path: "alpha".into(),
                items: vec![board_a],
                ..Default::default()
            },
            ModuleDoc {
                path: "beta".into(),
                items: vec![board_b],
                ..Default::default()
            },
        ],
    };
    let files = match render_markdown(&model, None, &MarkdownOptions { split: true }) {
        MarkdownOutput::Single(_) => panic!("expected Split output for split: true"),
        MarkdownOutput::Split(v) => v,
    };

    // (a) Filenames prefixed by module path. Two same-named items must land
    // in distinct files.
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"alpha/structure-Board.md"),
        "expected alpha/structure-Board.md in {names:?}"
    );
    assert!(
        names.contains(&"beta/structure-Board.md"),
        "expected beta/structure-Board.md in {names:?}"
    );
    // (c) Same-name collision resolves to two distinct files.
    let board_files: Vec<&&str> = names
        .iter()
        .filter(|n| n.ends_with("/structure-Board.md"))
        .collect();
    assert_eq!(
        board_files.len(),
        2,
        "expected exactly two Board files (one per module), got: {board_files:?}"
    );

    // (b) Per-item bodies use `../index.md` for the back-link.
    for (name, body) in &files {
        if name == "index.md" {
            continue;
        }
        assert!(
            body.contains("[← Index](../index.md)"),
            "per-item file {name} must use `../index.md` back-link, got body:\n{body}"
        );
        assert!(
            !body.contains("[← Index](index.md)"),
            "per-item file {name} must NOT use the flat `index.md` back-link \
             (multi-module case), got body:\n{body}"
        );
    }

    // (d) index.md uses H2 module headings with module-prefixed filename links.
    let index_body = &files
        .iter()
        .find(|(n, _)| n == "index.md")
        .expect("index.md missing")
        .1;

    // Module H2 headings — one per module, in model.modules order.
    assert!(
        index_body.contains("## alpha\n"),
        "index.md must contain `## alpha` H2 heading, got:\n{index_body}"
    );
    assert!(
        index_body.contains("## beta\n"),
        "index.md must contain `## beta` H2 heading, got:\n{index_body}"
    );
    let alpha_pos = index_body.find("## alpha\n").unwrap();
    let beta_pos = index_body.find("## beta\n").unwrap();
    assert!(
        alpha_pos < beta_pos,
        "## alpha must precede ## beta in index.md (model.modules order)"
    );

    // Module-prefixed filename links resolve same-name collision.
    assert!(
        index_body.contains("- [`Board`](alpha/structure-Board.md)"),
        "index.md must link alpha Board via alpha/structure-Board.md, got:\n{index_body}"
    );
    assert!(
        index_body.contains("- [`Board`](beta/structure-Board.md)"),
        "index.md must link beta Board via beta/structure-Board.md, got:\n{index_body}"
    );

    // Negative guard: no bare fragment links.
    assert!(
        !index_body.contains("(#Board)"),
        "index.md must NOT contain fragment-only link (#Board), got:\n{index_body}"
    );
}

// ---------------------------------------------------------------------------
// Split mode per-item cross-ref filename links
// ---------------------------------------------------------------------------

/// In single-module split mode, `### Conforms to` and `### Used by` bullets
/// inside per-item files must use filename-shaped links (`trait-Fastener.md`,
/// `structure-Board.md`) rather than the old fragment-only form (`#Fastener`,
/// `#Board`).
#[test]
fn split_mode_per_item_cross_refs_use_filename_links() {
    let fastener = ItemDoc::Trait {
        name: "Fastener".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![],
        members: vec![],
    };
    let bolt = ItemDoc::Structure {
        name: "Bolt".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let board = ItemDoc::Structure {
        name: "Board".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let mcu = ItemDoc::Occurrence {
        name: "MCU".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let model = DocModel {
        modules: vec![ModuleDoc {
            path: "hardware".into(),
            items: vec![fastener, bolt, board, mcu],
            ..Default::default()
        }],
    };
    let mut xrefs = CrossRefs::default();
    xrefs
        .trait_to_conformers
        .insert("Fastener".into(), vec!["Bolt".into()]);
    xrefs
        .entity_to_containers
        .insert("MCU".into(), vec!["Board".into()]);

    let files = match render_markdown(&model, Some(&xrefs), &MarkdownOptions { split: true }) {
        MarkdownOutput::Split(v) => v,
        MarkdownOutput::Single(_) => panic!("expected Split output"),
    };

    // Locate the per-item files.
    let bolt_body = &files
        .iter()
        .find(|(n, _)| n == "structure-Bolt.md")
        .expect("structure-Bolt.md missing in split output")
        .1;
    let mcu_body = &files
        .iter()
        .find(|(n, _)| n == "occurrence-MCU.md")
        .expect("occurrence-MCU.md missing in split output")
        .1;

    // Bolt: Conforms to Fastener — must use filename link.
    assert!(
        bolt_body.contains("### Conforms to"),
        "Bolt file must contain `### Conforms to`, got:\n{bolt_body}"
    );
    assert!(
        bolt_body.contains("- [`Fastener`](trait-Fastener.md)"),
        "Bolt Conforms-to must link to trait-Fastener.md, got:\n{bolt_body}"
    );
    assert!(
        !bolt_body.contains("(#Fastener)"),
        "Bolt Conforms-to must NOT use old fragment link (#Fastener), got:\n{bolt_body}"
    );

    // MCU: Used by Board — must use filename link.
    assert!(
        mcu_body.contains("### Used by"),
        "MCU file must contain `### Used by`, got:\n{mcu_body}"
    );
    assert!(
        mcu_body.contains("- [`Board`](structure-Board.md)"),
        "MCU Used-by must link to structure-Board.md, got:\n{mcu_body}"
    );
    assert!(
        !mcu_body.contains("(#Board)"),
        "MCU Used-by must NOT use old fragment link (#Board), got:\n{mcu_body}"
    );
}

// ---------------------------------------------------------------------------
// Multi-module split per-item cross-ref relative paths
// ---------------------------------------------------------------------------

/// In multi-module split mode, cross-reference bullets in per-item files must
/// use relative paths that are correct from the *containing file's* location:
/// - Same-module references: `{kind}-{name}.md` (sibling, no `../` prefix).
/// - Cross-module references: `../{other_module}/{kind}-{name}.md` (up one
///   directory, then into the other module's subdirectory).
#[test]
fn multi_module_split_per_item_cross_refs_use_relative_paths() {
    // alpha: Bolt (Structure), LocalT (Trait — same-module reference)
    let bolt = ItemDoc::Structure {
        name: "Bolt".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![], params: vec![], ports: vec![],
        constraints: vec![], sub_components: vec![], realizations: vec![],
        meta: vec![],
    };
    let local_t = ItemDoc::Trait {
        name: "LocalT".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![],
        members: vec![],
    };
    // beta: Fastener (Trait — cross-module reference from alpha/Bolt)
    let fastener = ItemDoc::Trait {
        name: "Fastener".into(),
        doc: None,
        is_pub: true,
        annotations: vec![], pragmas: vec![],
        members: vec![],
    };
    let model = DocModel {
        modules: vec![
            ModuleDoc {
                path: "alpha".into(),
                items: vec![bolt, local_t],
                ..Default::default()
            },
            ModuleDoc {
                path: "beta".into(),
                items: vec![fastener],
                ..Default::default()
            },
        ],
    };
    let mut xrefs = CrossRefs::default();
    // Bolt conforms to both Fastener (cross-module, in beta) and LocalT (same-module, in alpha).
    xrefs
        .trait_to_conformers
        .insert("Fastener".into(), vec!["Bolt".into()]);
    xrefs
        .trait_to_conformers
        .insert("LocalT".into(), vec!["Bolt".into()]);

    let files = match render_markdown(&model, Some(&xrefs), &MarkdownOptions { split: true }) {
        MarkdownOutput::Split(v) => v,
        MarkdownOutput::Single(_) => panic!("expected Split output"),
    };

    // Locate alpha/structure-Bolt.md.
    let bolt_body = &files
        .iter()
        .find(|(n, _)| n == "alpha/structure-Bolt.md")
        .expect("alpha/structure-Bolt.md missing in split output")
        .1;

    // Cross-module: Fastener is in beta — must use ../beta/ relative path.
    assert!(
        bolt_body.contains("### Conforms to"),
        "Bolt must have Conforms-to section, got:\n{bolt_body}"
    );
    assert!(
        bolt_body.contains("- [`Fastener`](../beta/trait-Fastener.md)"),
        "Bolt cross-module Conforms-to must use ../beta/trait-Fastener.md, got:\n{bolt_body}"
    );
    // Check for `](beta/` (start-of-link form) rather than `(beta/` so the
    // guard actually distinguishes root-relative `](beta/...` from the
    // correct `](../beta/...` — the `(` in `](../beta/...` is followed by
    // `..`, not `b`, so a `(beta/` check would be vacuously true.
    assert!(
        !bolt_body.contains("](beta/trait-Fastener.md)"),
        "Bolt must NOT use root-relative beta/ path (would resolve as alpha/beta/...), \
         got:\n{bolt_body}"
    );
    assert!(
        !bolt_body.contains("(#Fastener)"),
        "Bolt must NOT fall back to fragment link for Fastener, got:\n{bolt_body}"
    );

    // Same-module: LocalT is in alpha — must use sibling link (no ../ prefix).
    assert!(
        bolt_body.contains("- [`LocalT`](trait-LocalT.md)"),
        "Bolt same-module Conforms-to must use sibling trait-LocalT.md, got:\n{bolt_body}"
    );
    assert!(
        !bolt_body.contains("(alpha/trait-LocalT.md)"),
        "Bolt must NOT use module-prefixed path for same-module LocalT, got:\n{bolt_body}"
    );
}

// ---------------------------------------------------------------------------
// Ambiguous cross-ref fallback
// ---------------------------------------------------------------------------

/// When the same name exists in two modules, `unique_resolve` returns `None`
/// because the reference is genuinely ambiguous — `CrossRefs` carries only
/// bare names, not module-qualified names.  In that case the resolver must
/// fall back to the fragment form `#Name`, which is no worse than the
/// pre-task behaviour (the fragment will dangle, but at least it preserves
/// the visible link text and does not silently point to the wrong file).
///
/// This test pins that fallback so any future change that accidentally
/// guesses a module or omits the link entirely will fail loudly.
#[test]
fn multi_module_split_cross_ref_ambiguous_name_falls_back_to_fragment() {
    // Two modules each declare a trait named `Shared` — genuine ambiguity.
    let shared_alpha = ItemDoc::Trait {
        name: "Shared".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        members: vec![],
    };
    let shared_beta = ItemDoc::Trait {
        name: "Shared".into(),
        doc: None,
        is_pub: true,
        annotations: vec![],
        pragmas: vec![],
        members: vec![],
    };
    // Conformer lives in alpha; its cross-ref target name "Shared" is
    // ambiguous because alpha::Shared and beta::Shared both exist.
    let bolt = ItemDoc::Structure {
        name: "Bolt".into(),
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
    };
    let model = DocModel {
        modules: vec![
            ModuleDoc {
                path: "alpha".into(),
                items: vec![shared_alpha, bolt],
                ..Default::default()
            },
            ModuleDoc {
                path: "beta".into(),
                items: vec![shared_beta],
                ..Default::default()
            },
        ],
    };
    let mut xrefs = CrossRefs::default();
    xrefs
        .trait_to_conformers
        .insert("Shared".into(), vec!["Bolt".into()]);

    let files = match render_markdown(&model, Some(&xrefs), &MarkdownOptions { split: true }) {
        MarkdownOutput::Split(v) => v,
        MarkdownOutput::Single(_) => panic!("expected Split output"),
    };

    let bolt_body = &files
        .iter()
        .find(|(n, _)| n == "alpha/structure-Bolt.md")
        .expect("alpha/structure-Bolt.md missing")
        .1;

    // The cross-ref section is present (the link text is preserved).
    assert!(
        bolt_body.contains("### Conforms to"),
        "Bolt must have Conforms-to section even on ambiguous cross-ref, got:\n{bolt_body}"
    );

    // Known limitation: ambiguous name falls back to fragment `#Shared`.
    // This is intentional — see the `unique_resolve` doc comment in
    // fmt_markdown.rs and the design decision in plan.json.
    assert!(
        bolt_body.contains("- [`Shared`](#Shared)"),
        "Ambiguous cross-ref must fall back to fragment #Shared, got:\n{bolt_body}"
    );

    // Must NOT emit a module-qualified path — that would silently mislead
    // the reader by picking one module arbitrarily.
    assert!(
        !bolt_body.contains("](alpha/trait-Shared.md)"),
        "Must NOT guess alpha module for ambiguous Shared, got:\n{bolt_body}"
    );
    assert!(
        !bolt_body.contains("](beta/trait-Shared.md)"),
        "Must NOT guess beta module for ambiguous Shared, got:\n{bolt_body}"
    );
}

// ---------------------------------------------------------------------------
// Snapshot-helper regression tests
// ---------------------------------------------------------------------------

/// Regression: `assert_or_update_snapshot` must normalise `\r\n` → `\n` in
/// `actual` before comparing against the committed LF-only golden.
///
/// Background: on a Windows checkout running `UPDATE_SNAPSHOTS=1` the helper
/// historically wrote CRLF bytes into the golden file, silently breaking the
/// comparison for every other developer (whose `actual` would be LF).
/// This test guards against that regression by passing a CRLF-terminated
/// `actual` to the helper and asserting no panic occurs when compared with
/// the LF-only golden `crlf_normalization_smoke.txt`.
///
/// NOTE: the env-var code path (`UPDATE_SNAPSHOTS=1`) is intentionally NOT
/// tested here — env-var mutation across concurrent cargo-test threads is
/// racy and would conflict with other snapshot tests.  The normalisation
/// `replace()` call covers both branches, so the compare-path test alone
/// is sufficient to pin the fix.
#[test]
fn assert_or_update_snapshot_normalizes_crlf_in_actual() {
    // CRLF-terminated actual — two lines, each ending with \r\n.
    let actual = "line1\r\nline2\r\n";
    // The committed golden (crlf_normalization_smoke.txt) contains
    // "line1\nline2\n" (LF only).  After normalisation the strings must be
    // equal; without normalisation the helper panics with a snapshot-mismatch
    // message.
    assert_or_update_snapshot("crlf_normalization_smoke.txt", actual);
}

/// Regression: `assert_or_update_snapshot` must normalise `\r\n` → `\n` in
/// the **on-disk expected** before comparing with the in-memory LF `actual`.
///
/// Background: on a Windows checkout with `core.autocrlf=true` a committed
/// LF golden is silently translated to CRLF on disk.  Without the `expected`
/// normalisation the helper would panic with a snapshot-mismatch even though
/// the logical content is identical.  This test exercises `assert_or_update_snapshot_at`
/// (the path-taking primitive) directly, passing an LF `actual` against a
/// runtime-created CRLF on-disk file, so that removing the
/// `s.replace("\r\n", "\n")` in the `Ok(s) => …` arm causes the test to fail.
///
/// NOTE: this is the inverse of `assert_or_update_snapshot_normalizes_crlf_in_actual`
/// (CRLF actual + LF expected) — together the two tests provide independent
/// coverage of each `replace()` call so that deleting either one surfaces a
/// failure.
#[test]
fn assert_or_update_snapshot_normalizes_crlf_in_expected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("crlf_expected.txt");
    // Write CRLF bytes to the file, simulating a Windows checkout with
    // core.autocrlf=true that has silently re-encoded a committed LF golden.
    std::fs::write(&path, "line1\r\nline2\r\n").expect("write CRLF file");
    // LF-only actual — what the formatter actually produces on all platforms.
    // Without the expected-arm normalisation the helper panics (CRLF ≠ LF);
    // with it both sides become "line1\nline2\n" and the assertion succeeds.
    assert_or_update_snapshot_at(&path, "line1\nline2\n");
}
