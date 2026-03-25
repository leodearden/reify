//! Shared test helpers for reify-compiler integration tests.
//!
//! Each test file declares `mod common;` at the top to include these helpers.
//! The `#![allow(dead_code)]` suppresses warnings for helpers that are only
//! used in a subset of test binaries.

#![allow(dead_code)]

use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_types::{Diagnostic, ModulePath};

/// Parse source and compile, returning the CompiledModule.
pub fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Parse source and compile, returning first template + diagnostics.
pub fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module
        .templates
        .into_iter()
        .next()
        .expect("expected 1 template");
    (template, module.diagnostics)
}
