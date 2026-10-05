//! What the compiler makes of one COMPLETE module, split by the layer that
//! rejected it — the binary's one definition of a doc sample that "compiles
//! clean": it parses, and compiling it yields zero `Severity::Error`
//! diagnostics. No warning ever counts.

use reify_compiler::parse_with_stdlib;
use reify_core::ModulePath;
use reify_test_support::{compile_source_with_stdlib_allow_parse_errors, errors_only};

/// What the compiler made of a module, split by the LAYER that rejected it.
///
/// `errors_only` alone cannot make this distinction: parse errors are folded
/// into the same `.diagnostics` list as compile-layer ones, so "did not parse"
/// and "parsed and then failed type checking" arrive indistinguishable. A
/// caller that must tell them apart — the fence gate's ```` ```reify-invalid ````
/// check — needs [`ModuleCompile::ParseRejected`] on its own.
pub(crate) enum ModuleCompile {
    /// Parsed, compiled, zero `Severity::Error` diagnostics.
    Clean,
    /// The PARSER rejected the source, so it is not reify source at all. Any
    /// compile-layer diagnostics downstream of a broken AST describe the
    /// wreckage rather than the source, which is why this arm carries only the
    /// parse messages.
    ParseRejected(Vec<String>),
    /// Parsed cleanly, then produced at least one `Severity::Error`.
    SemanticErrors(Vec<String>),
}

impl ModuleCompile {
    /// The diagnostics of whichever layer rejected the module; `None` when it
    /// compiled clean.
    pub(crate) fn rejection(self) -> Option<Vec<String>> {
        match self {
            ModuleCompile::Clean => None,
            ModuleCompile::ParseRejected(messages) | ModuleCompile::SemanticErrors(messages) => {
                Some(messages)
            }
        }
    }

    /// The rendered diagnostics, one indented bullet per line.
    pub(crate) fn rendered(messages: &[String]) -> String {
        messages
            .iter()
            .map(|message| format!("    - {message}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Compile `source` VERBATIM as a complete module and report which layer, if
/// any, rejected it.
///
/// # Why `_allow_parse_errors`
///
/// `compile_source_with_stdlib` PANICS on parse errors. One malformed sample
/// would then abort a whole corpus gate with a backtrace naming no file and no
/// fence — defeating the "names file + fence ordinal + diagnostics" contract
/// at exactly the moment it matters most. The
/// `compile_source_with_stdlib_allow_parse_errors` variant returns parse
/// errors in `.diagnostics` at Error severity, forwarded by the compiler
/// itself, so a malformed sample is reported as a normal, fully-attributed
/// violation.
///
/// The extra `parse_with_stdlib` call is what separates the two layers. It is
/// the SAME parse the helper performs internally, repeated rather than
/// threaded out, because the helper's signature returns only a
/// `CompiledModule`; a string match on the diagnostic text would be the
/// alternative, and an ad-hoc parser over a message is what heuristic 12 exists
/// to forbid.
pub(crate) fn compile_module(source: &str) -> ModuleCompile {
    let parsed = parse_with_stdlib(source, ModulePath::single("module_compile"));
    if !parsed.errors.is_empty() {
        return ModuleCompile::ParseRejected(
            parsed.errors.iter().map(|e| e.message.clone()).collect(),
        );
    }
    let compiled = compile_source_with_stdlib_allow_parse_errors(source);
    let errors = errors_only(&compiled);
    if errors.is_empty() {
        ModuleCompile::Clean
    } else {
        ModuleCompile::SemanticErrors(
            errors
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// Hermetic controls — synthetic sources only.
// ---------------------------------------------------------------------------

#[test]
fn compile_module_splits_its_verdict_by_the_rejecting_layer() {
    assert!(
        compile_module("structure def S {\n    let b = box(1mm, 1mm, 1mm)\n}\n")
            .rejection()
            .is_none(),
        "a module that parses and compiles with zero Error diagnostics is Clean — the \
         missing-`module` WARNING never counts"
    );
    assert!(
        matches!(
            compile_module("structure def S {\n    let p = Point {}\n}\n"),
            ModuleCompile::ParseRejected(messages) if !messages.is_empty()
        ),
        "empty-brace construction is a grammar-level error, so the PARSER rejects it"
    );
    assert!(
        matches!(
            compile_module("structure def S {\n    let o = some(1mm, 2mm)\n}\n"),
            ModuleCompile::SemanticErrors(messages) if !messages.is_empty()
        ),
        "a source that parses and then fails the compile layer is SemanticErrors"
    );
}
