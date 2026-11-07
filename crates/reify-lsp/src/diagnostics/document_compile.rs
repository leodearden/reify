//! Compile one LSP document: as a program when it is a file-backed entry that
//! follows an import, otherwise exactly as the single-file stdlib compile.

use std::path::Path;

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program;
use reify_constraints::SimpleConstraintChecker;
use reify_core::SourceSpan;
use tower_lsp::lsp_types::{Range, Url};

use crate::convert;

/// A compiled document, immutable once built.
pub(super) struct CompiledDocument {
    module: CompiledModule,
    /// How many of `module.templates`, from the front, the document declares;
    /// the rest are imported, and their spans index another file.
    declared_templates: usize,
}

impl CompiledDocument {
    pub(super) fn single_file(module: CompiledModule) -> Self {
        Self {
            declared_templates: module.templates.len(),
            module,
        }
    }

    pub(super) fn module(&self) -> &CompiledModule {
        &self.module
    }

    /// The range of `span`, a span of `module().templates[template_index]`, in
    /// `source`; `Range::default()` when that template is imported.
    pub(super) fn template_span_range(
        &self,
        source: &str,
        template_index: usize,
        span: SourceSpan,
    ) -> Range {
        if template_index < self.declared_templates {
            convert::span_to_range(source, span)
        } else {
            Range::default()
        }
    }
}

/// Compile `parsed`, the parse of `source` addressed by `uri`, under `cfg`.
pub(super) fn compile_document(
    parsed: &ParsedModule,
    source: &str,
    uri: &Url,
    cfg: &CfgSet,
) -> CompiledDocument {
    match uri.to_file_path() {
        Ok(entry_path) if program::entry_follows_imports(parsed, cfg) => {
            let compiled = program::compile_program_with_parsed_entry(
                &entry_path,
                source,
                parsed,
                &entry_dir_resolver(&entry_path),
                cfg,
                &SimpleConstraintChecker,
            );
            CompiledDocument {
                declared_templates: compiled.entry_declared_templates().len(),
                module: compiled.entry,
            }
        }
        _ => CompiledDocument::single_file(reify_compiler::compile_with_stdlib_checked(
            parsed,
            &SimpleConstraintChecker,
        )),
    }
}

/// User imports resolve from the entry file's directory, as `reify check` and
/// the GUI resolve them.
fn entry_dir_resolver(entry_path: &Path) -> ModuleResolver {
    let dir = entry_path.parent().unwrap_or_else(|| Path::new("."));
    ModuleResolver::new(dir, dir.join("crates/reify-compiler/stdlib"))
}

#[cfg(test)]
mod tests {
    use reify_core::{DiagnosticCode, ModulePath, Severity};

    use super::*;

    type DiagShape = (Severity, Option<DiagnosticCode>, String, Vec<SourceSpan>);

    fn diag_shapes(module: &CompiledModule) -> Vec<DiagShape> {
        module
            .diagnostics
            .iter()
            .map(|d| {
                (
                    d.severity,
                    d.code,
                    d.message.clone(),
                    d.labels.iter().map(|l| l.span).collect(),
                )
            })
            .collect()
    }

    fn parse(stem: &str, source: &str) -> ParsedModule {
        reify_compiler::parse_with_stdlib(source, ModulePath::single(stem))
    }

    /// A document that follows no import compiles exactly as the single-file
    /// compile does: no module-header diagnostic, the same content hash.
    #[test]
    fn a_document_that_follows_no_import_compiles_single_file() {
        let body = "structure def Plate {\n    param w : Length = 10mm\n}\n";
        let cases = [
            (
                "headerless",
                "file:///reify-lsp-routing/solo.ri",
                body.to_string(),
            ),
            (
                "std-only import",
                "file:///reify-lsp-routing/solo.ri",
                format!("import std.units\n{body}"),
            ),
            (
                "cfg-gated-out user import",
                "file:///reify-lsp-routing/solo.ri",
                format!("#cfg(target = \"wasm\")\nimport ghost\n{body}"),
            ),
            (
                "not file-backed",
                "untitled:Untitled-1",
                format!("import parts\n{body}"),
            ),
        ];
        for (label, uri, source) in cases {
            let uri = Url::parse(uri).expect("uri");
            let parsed = parse("solo", &source);

            let document = compile_document(&parsed, &source, &uri, &CfgSet::host_default());
            let single_file =
                reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);

            assert_eq!(
                diag_shapes(document.module()),
                diag_shapes(&single_file),
                "{label}: diagnostics"
            );
            assert_eq!(
                document.module().content_hash,
                single_file.content_hash,
                "{label}: content hash"
            );
        }
    }

    #[test]
    fn template_span_range_only_trusts_declared_templates() {
        let guard = reify_test_support::prefixed_tempdir("reify-lsp-doc-compile-");
        std::fs::write(
            guard.path().join("parts.ri"),
            "module parts\npub structure def Pulley {\n    param d : Length = 10mm\n}\n",
        )
        .expect("write parts.ri");
        let source = "module main\nimport parts\nstructure def Rig {\n    sub p = Pulley()\n    constraint p.d > 5mm\n}\n";
        let uri = Url::from_file_path(guard.path().join("main.ri")).expect("uri");
        let parsed = parse("main", source);

        let document = compile_document(&parsed, source, &uri, &CfgSet::host_default());

        let templates = &document.module().templates;
        let index_of = |name: &str| {
            templates
                .iter()
                .position(|t| t.name == name)
                .unwrap_or_else(|| panic!("no template {name}"))
        };
        let rig = index_of("Rig");
        let pulley = index_of("Pulley");
        let rig_span = templates[rig].constraints[0].span;
        assert_eq!(
            document.template_span_range(source, rig, rig_span),
            convert::span_to_range(source, rig_span)
        );
        assert_eq!(
            document.template_span_range(source, pulley, templates[pulley].value_cells[0].span),
            Range::default()
        );
    }
}
