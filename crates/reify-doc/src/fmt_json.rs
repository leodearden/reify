//! JSON formatter for [`crate::model::DocModel`].
//!
//! See [`render_json`] for the public entry point.

#[cfg(test)]
mod tests {
    use crate::fmt_json::render_json;
    use crate::model::{DocModel, ItemDoc, ModuleDoc};

    /// Build a small `DocModel` with one `ModuleDoc` containing a single
    /// `Trait` item.  Used by all formatter tests below.
    fn fixture_model() -> DocModel {
        DocModel {
            modules: vec![ModuleDoc {
                path: "m".to_string(),
                doc: None,
                items: vec![ItemDoc::Trait {
                    name: "T".to_string(),
                    doc: None,
                    is_pub: true,
                    annotations: vec![],
                    pragmas: vec![],
                    members: vec![],
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
}
