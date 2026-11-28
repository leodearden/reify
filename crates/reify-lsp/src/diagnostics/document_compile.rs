//! Compile one LSP document: as a program when it is a file-backed entry that
//! follows an import, otherwise exactly as the single-file stdlib compile.

use std::path::Path;
use std::sync::Arc;

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program::{self, CompiledProgram};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ContentHash, SourceSpan};
use reify_eval::LoadedProgram;
use tower_lsp::lsp_types::{Range, Url};

use crate::convert;

/// A compiled document, immutable once built.
pub(super) struct CompiledDocument {
    /// The module the document is evaluated as: a program's evaluation module.
    module: Arc<CompiledModule>,
    program: Option<LoadedProgram>,
    eval_cache_key: ContentHash,
    /// How many of `module.templates`, from the front, the document itself
    /// declares (its program entry's templates); the rest are imported, and
    /// their spans index another file.
    declared_templates: usize,
}

impl CompiledDocument {
    pub(super) fn single_file(module: CompiledModule) -> Self {
        Self {
            eval_cache_key: module.content_hash,
            declared_templates: module.templates.len(),
            module: Arc::new(module),
            program: None,
        }
    }

    pub(super) fn module(&self) -> &CompiledModule {
        &self.module
    }

    /// This document as `engine` evaluates it. A program is loaded into
    /// `engine`, which returns the document's module: the only one that
    /// resolves in the program's environment there.
    pub(super) fn loaded_into(self, engine: &mut reify_eval::Engine) -> Self {
        match &self.program {
            Some(program) => Self {
                module: engine.load(program.clone()),
                ..self
            },
            None => self,
        }
    }

    /// Equal for two compiles only when a warm re-evaluation of the earlier
    /// one is sound for the later one; an edit eval cannot observe, such as a
    /// comment in an import, may still change it.
    pub(super) fn eval_cache_key(&self) -> ContentHash {
        self.eval_cache_key
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
            let loaded = LoadedProgram::new(program::compile_program_with_parsed_entry(
                &entry_path,
                source,
                parsed,
                &entry_dir_resolver(&entry_path),
                cfg,
                &SimpleConstraintChecker,
            ));
            CompiledDocument {
                eval_cache_key: program_eval_cache_key(&parsed.path.0.join("."), loaded.program()),
                declared_templates: loaded.program().entry().templates.len(),
                module: Arc::clone(loaded.evaluated()),
                program: Some(loaded),
            }
        }
        _ => CompiledDocument::single_file(reify_compiler::compile_with_stdlib_checked(
            parsed,
            &SimpleConstraintChecker,
        )),
    }
}

/// The entry's content hash covers its own text and its import paths only, so
/// the key also covers the source of every other module of `program`; the
/// entry is the module keyed `entry_key`, wherever `program.sources` lists it.
fn program_eval_cache_key(entry_key: &str, program: &CompiledProgram) -> ContentHash {
    let imported = program
        .sources
        .iter()
        .filter(|module| module.module_key != entry_key);
    ContentHash::combine_all(
        std::iter::once(program.entry().content_hash).chain(imported.map(|module| {
            ContentHash::of_str(&module.module_key).combine(ContentHash::of_str(&module.source))
        })),
    )
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

    /// A file-backed document that follows no import compiles exactly as the
    /// strict single-file compile does: no module-header diagnostic, the same
    /// content hash.
    #[test]
    fn a_document_that_follows_no_import_compiles_single_file() {
        let body = "structure def Plate {\n    param w : Length = 10mm\n}\n";
        let cases = [
            ("headerless", body.to_string()),
            (
                "cfg-gated-out user import",
                format!("#cfg(target = \"wasm\")\nimport ghost\n{body}"),
            ),
        ];
        let uri = Url::parse("file:///reify-lsp-routing/solo.ri").expect("uri");
        let cfg = CfgSet::host_default();
        for (label, source) in cases {
            let parsed = parse("solo", &source);

            let document = compile_document(&parsed, &source, &uri, &cfg);
            let single_file =
                program::compile_single_file(&parsed, &cfg, &SimpleConstraintChecker);

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

    const GPA_BODY: &str = "structure def Plate {\n    param e : Pressure = 200GPa\n}\n";

    fn errors(document: &CompiledDocument) -> Vec<&reify_core::Diagnostic> {
        document
            .module()
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    /// Strict visibility (stdlib-namespace κ #5503): an import-free file
    /// writing a std unit draws the NS-V2 fix-it Error.
    #[test]
    fn an_import_free_document_writing_a_std_unit_gets_the_fixit() {
        let uri = Url::parse("file:///reify-lsp-routing/plate.ri").expect("uri");
        let parsed = parse("plate", GPA_BODY);

        let document = compile_document(&parsed, GPA_BODY, &uri, &CfgSet::host_default());

        assert!(
            errors(&document)
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::StdNameNotImported)),
            "{:#?}",
            document.module().diagnostics
        );
    }

    /// A file-backed document whose only import is a std module follows it
    /// as a program and is clean.
    #[test]
    fn a_document_importing_its_std_unit_module_compiles_as_a_clean_program() {
        let guard = reify_test_support::prefixed_tempdir("reify-lsp-doc-compile-");
        let source = format!("module plate\nimport std.si_units\n{GPA_BODY}");
        let uri = Url::from_file_path(guard.path().join("plate.ri")).expect("uri");
        let parsed = parse("plate", &source);

        let document = compile_document(&parsed, &source, &uri, &CfgSet::host_default());

        assert!(
            document.program.is_some(),
            "a std import is followed, so the document is a program"
        );
        assert!(
            errors(&document).is_empty(),
            "{:#?}",
            document.module().diagnostics
        );
    }

    /// A document with no file follows its std imports and never a user
    /// import: `import std.prelude` brings `GPa` in, and `import parts` is
    /// not resolved, so neither draws an Error.
    #[test]
    fn an_untitled_document_follows_only_its_std_imports() {
        let uri = Url::parse("untitled:Untitled-1").expect("uri");
        let source = format!("import std.prelude\nimport parts\n{GPA_BODY}");
        let parsed = parse("untitled", &source);

        let document = compile_document(&parsed, &source, &uri, &CfgSet::host_default());

        assert!(
            errors(&document).is_empty(),
            "{:#?}",
            document.module().diagnostics
        );
    }

    /// The evaluation module holds an imported template the entry does not;
    /// only the entry's own templates' spans index the document.
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

        let program = document
            .program
            .as_ref()
            .expect("an importing document compiles as a program")
            .program();
        assert!(
            !program.entry().templates.iter().any(|t| t.name == "Pulley"),
            "the entry holds only its own templates"
        );
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
