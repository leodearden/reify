//! JSON formatter for [`crate::model::DocModel`].
//!
//! See [`render_json`] for the public entry point.

use crate::model::DocModel;

/// Render a [`DocModel`] as JSON.
///
/// When `compact == false`, output is pretty-printed (multi-line, two-space
/// indentation) via [`serde_json::to_string_pretty`].  When `compact == true`,
/// output is the single-line `serde_json::to_string` form.
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
