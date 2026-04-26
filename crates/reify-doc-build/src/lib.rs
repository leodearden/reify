//! Lowering passes that consume `reify_compiler` outputs and produce `reify_doc`
//! model artefacts.
//!
//! Kept separate from `reify-doc` so that crate's serde-only embeddability surface
//! is preserved — downstream consumers that only need the data model types do not
//! need to pull in the full compiler stack.
//!
//! This crate is the natural home for future compiler→doc-model transforms
//! (e.g., `build_doc_model`, formatter/CLI lowering stages).

pub mod cross_refs;
