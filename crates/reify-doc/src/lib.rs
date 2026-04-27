//! Foundational data model for the `reify doc` tool — pure data, no formatters yet.
//!
//! This crate provides serde-friendly model types that represent the documentation
//! surface of a compiled Reify module. It is intentionally dependency-free beyond
//! `serde`/`serde_json` so it can be embedded in any downstream consumer without
//! pulling in the full compiler stack.

pub mod cross_refs;
pub mod fmt_json;
pub mod fmt_markdown;
pub mod model;
