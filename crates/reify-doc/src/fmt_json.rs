//! JSON formatter for [`crate::model::DocModel`].
//!
//! See [`render_json`] for the public entry point.

use crate::model::DocModel;

/// Render a [`DocModel`] as JSON.
///
/// When `compact == false`, output is pretty-printed (multi-line, two-space
/// indentation) via [`serde_json::to_string_pretty`].  When `compact == true`,
/// output is the single-line `serde_json::to_string` form.  Field naming is
/// `snake_case` throughout (enforced upstream by `#[serde(rename_all =
/// "snake_case")]` on the [`DocModel`] graph and by `#[serde(tag = "kind",
/// rename_all = "snake_case")]` on `ItemDoc`).
///
/// # Schema stability
///
/// The on-the-wire schema is the [`DocModel`] definition itself.  Per the
/// `reify-doc` PRD (`docs/prds/reify-doc-tool.md` §"JSON"), downstream
/// consumers can rely on the schema being **stable across v0.1 patch
/// releases**.  There is **no backward-compat promise across v0.1 → v0.2** —
/// the source `#version` pragma is the signal that consumers should re-pin.
///
/// # Panics
///
/// `unwrap`s the `serde_json` result.  Every field in the [`DocModel`] graph
/// is plain serde-derived data — no `#[serde(serialize_with = ...)]` adapters,
/// no fallible custom `Serialize` impls — so `to_string` / `to_string_pretty`
/// can only fail on a writer error, and we serialize into a `String` whose
/// writer is infallible.  An unwrap here is therefore "never panics in
/// practice"; we annotate the call with `expect` for diagnosability if a
/// future refactor introduces a fallible adapter.
pub fn render_json(model: &DocModel, compact: bool) -> String {
    if compact {
        serde_json::to_string(model).expect("DocModel serde never fails")
    } else {
        serde_json::to_string_pretty(model).expect("DocModel serde never fails")
    }
}

#[cfg(test)]
mod tests {
    use super::render_json;
    use crate::model::{DocModel, ItemDoc, ItemHeader, ItemKind, ModuleDoc};

    /// Build a small `DocModel` with one `ModuleDoc` containing a single
    /// `Trait` item.  Used by all formatter tests below.
    fn fixture_model() -> DocModel {
        DocModel {
            modules: vec![ModuleDoc {
                path: "m".to_string(),
                doc: None,
                items: vec![ItemDoc {
                    header: ItemHeader {
                        name: "T".to_string(),
                        doc: None,
                        is_pub: true,
                        annotations: vec![],
                        pragmas: vec![],
                    },
                    kind: ItemKind::Trait { members: vec![] },
                }],
                annotations: vec![],
                pragmas: vec![],
                cross_refs: Default::default(),
            }],
        }
    }

    #[test]
    fn render_json_pretty_emits_indented_multiline_output() {
        let model = fixture_model();
        let output = render_json(&model, false);

        // Pretty mode must produce multi-line output with at least one
        // two-space indentation run.  serde_json::to_string_pretty defaults
        // to two-space indentation.
        assert!(
            output.contains('\n'),
            "pretty json must be multi-line, got: {output}"
        );
        assert!(
            output.contains("  "),
            "pretty json must contain two-space indentation, got: {output}"
        );

        // Round-trip: deserializing the pretty form must produce the same
        // model.
        let back: DocModel = serde_json::from_str(&output)
            .unwrap_or_else(|e| panic!("pretty json must round-trip: {e}\njson: {output}"));
        assert_eq!(model, back);
    }

    #[test]
    fn render_json_compact_emits_single_line_output() {
        let model = fixture_model();
        let output = render_json(&model, true);

        // Compact mode must be a single line (no newlines anywhere).
        assert!(
            !output.contains('\n'),
            "compact json must be single-line, got: {output}"
        );

        // Round-trip: deserializing the compact form must produce the same
        // model.
        let back: DocModel = serde_json::from_str(&output)
            .unwrap_or_else(|e| panic!("compact json must round-trip: {e}\njson: {output}"));
        assert_eq!(model, back);
    }

    /// Regression guard: every `ItemDoc` variant must serialize with a
    /// snake_case `"kind"` tag value.  This pins the
    /// `#[serde(tag = "kind", rename_all = "snake_case")]` attribute on
    /// `ItemDoc` against accidental removal — downstream JSON consumers
    /// depend on the snake_case tags (`type_alias`, `constraint_def`).
    #[test]
    fn render_json_uses_snake_case_kind_tags() {
        let model = DocModel {
            modules: vec![ModuleDoc {
                path: "regression".to_string(),
                doc: None,
                items: vec![
                    ItemDoc {
                        header: ItemHeader {
                            name: "Meters".to_string(),
                            doc: None,
                            is_pub: true,
                            annotations: vec![],
                            pragmas: vec![],
                        },
                        kind: ItemKind::TypeAlias {
                            type_repr: "f64".to_string(),
                        },
                    },
                    ItemDoc {
                        header: ItemHeader {
                            name: "voltage_safe".to_string(),
                            doc: None,
                            is_pub: true,
                            annotations: vec![],
                            pragmas: vec![],
                        },
                        kind: ItemKind::ConstraintDef {
                            expr_repr: "v <= 5.5 V".to_string(),
                        },
                    },
                ],
                annotations: vec![],
                pragmas: vec![],
                cross_refs: Default::default(),
            }],
        };

        let output = render_json(&model, true);
        assert!(
            output.contains("\"kind\":\"type_alias\""),
            "expected snake_case `type_alias` kind tag in: {output}"
        );
        assert!(
            output.contains("\"kind\":\"constraint_def\""),
            "expected snake_case `constraint_def` kind tag in: {output}"
        );
        // Negative guards: catch a regression to camelCase / PascalCase.
        assert!(
            !output.contains("typeAlias"),
            "kind tag must not be camelCase, got: {output}"
        );
        assert!(
            !output.contains("constraintDef"),
            "kind tag must not be camelCase, got: {output}"
        );
    }
}
