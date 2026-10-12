//! Contract tests for `reify_compiler::program::compile_program`, the single
//! program-level compile entry point (PRD docs/prds/v0_6/resolution-unification.md
//! §3.1, task #5516).
//!
//! Fixtures live in `tempfile::tempdir()` directories; the resolver's
//! `stdlib_root` is `<dir>/stdlib`, which is absent unless a test creates it.

use std::fs;
use std::path::{Path, PathBuf};

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{ModuleResolver, ModuleSource};
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::{CompiledModule, check_module_path_decl, compile_with_stdlib_checked};
use reify_core::{DiagnosticCode, ModulePath, Severity, SourceSpan};
use reify_ir::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction,
};

/// A local always-indeterminate checker (same contract as the compiler's stub).
struct Indeterminate;

impl ConstraintChecker for Indeterminate {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
                indeterminate_reason: None,
            })
            .collect()
    }
}

fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, source).expect("write fixture");
    path
}

fn resolver_for(dir: &Path) -> ModuleResolver {
    ModuleResolver::new(dir, dir.join("stdlib"))
}

fn compile_entry(dir: &Path, stem: &str, source: &str, cfg: &CfgSet) -> CompiledProgram {
    compile_program(
        &dir.join(format!("{stem}.ri")),
        source,
        &resolver_for(dir),
        cfg,
        &Indeterminate,
    )
}

fn dotted(module: &CompiledModule) -> String {
    module.path.0.join(".")
}

fn import_keys(program: &CompiledProgram) -> Vec<&str> {
    program
        .imports
        .iter()
        .map(|unit| unit.import_path.as_str())
        .collect()
}

fn source_keys(program: &CompiledProgram) -> Vec<&str> {
    program
        .sources
        .iter()
        .map(|s| s.module_key.as_str())
        .collect()
}

fn template_names(module: &CompiledModule) -> Vec<&str> {
    module.templates.iter().map(|t| t.name.as_str()).collect()
}

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

/// What the CLI's single-file `parse_and_compile` produced before
/// `compile_program` existed: the stdlib-aware parse, the stdlib compile, and
/// the module-path declaration diagnostic pushed afterwards.
fn legacy_single_file(stem: &str, source: &str) -> CompiledModule {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(stem));
    let mut compiled = compile_with_stdlib_checked(&parsed, &Indeterminate);
    if let Some(diag) = check_module_path_decl(parsed.declared_module_path.as_ref(), &parsed.path) {
        compiled.diagnostics.push(diag);
    }
    compiled
}

fn assert_kind_names_equal(new: &CompiledModule, old: &CompiledModule, label: &str) {
    fn names<T>(items: &[T], name: impl Fn(&T) -> &str) -> Vec<&str> {
        items.iter().map(name).collect()
    }
    assert_eq!(
        names(&new.templates, |t| &t.name),
        names(&old.templates, |t| &t.name),
        "{label}: templates"
    );
    assert_eq!(
        names(&new.functions, |f| &f.name),
        names(&old.functions, |f| &f.name),
        "{label}: functions"
    );
    assert_eq!(
        names(&new.enum_defs, |e| &e.name),
        names(&old.enum_defs, |e| &e.name),
        "{label}: enum_defs"
    );
    assert_eq!(
        names(&new.trait_defs, |t| &t.name),
        names(&old.trait_defs, |t| &t.name),
        "{label}: trait_defs"
    );
    assert_eq!(
        names(&new.units, |u| &u.name),
        names(&old.units, |u| &u.name),
        "{label}: units"
    );
    assert_eq!(
        names(&new.type_aliases, |a| &a.name),
        names(&old.type_aliases, |a| &a.name),
        "{label}: type_aliases"
    );
    assert_eq!(
        names(&new.constraint_defs, |c| &c.name),
        names(&old.constraint_defs, |c| &c.name),
        "{label}: constraint_defs"
    );
    assert_eq!(
        names(&new.compiled_purposes, |p| &p.name),
        names(&old.compiled_purposes, |p| &p.name),
        "{label}: compiled_purposes"
    );
    assert_eq!(
        names(&new.fields, |f| &f.name),
        names(&old.fields, |f| &f.name),
        "{label}: fields"
    );
}

/// Tracked import-free examples covering units/dimensions, traits + bounds,
/// enum + match, a user fn, an `auto:` type param, a purpose and ports/connect.
const IMPORT_FREE_EXAMPLES: &[&str] = &[
    "bracket.ri",
    "bearing_auto_seal.ri",
    "m5_guarded_enum.ri",
    "m5_user_function.ri",
    "m5_purpose.ri",
    "m8_ports.ri",
    "m9_trait_conformance.ri",
];

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn has_import_decl(source: &str) -> bool {
    let parsed = reify_syntax::parse(source, ModulePath::single("probe"));
    parsed
        .declarations
        .iter()
        .any(|d| matches!(d, reify_ast::Declaration::Import(_)))
}

// ─── (1) I1: single-file equivalence ───────────────────────────────────────

/// For an import-free entry, `compile_program` is observationally identical
/// to the legacy single-file compile (PRD boundary #1, invariant I1).
#[test]
fn compile_program_single_file_matches_legacy_compile() {
    let mut inputs: Vec<(String, String)> = vec![
        (
            "main".to_string(),
            "module main\nstructure def Plate {\n    param w : Length = 10mm\n    constraint w > 1mm\n}\n"
                .to_string(),
        ),
        (
            "undeclared".to_string(),
            "structure def Rod {\n    param r : Length = 2mm\n}\n".to_string(),
        ),
    ];
    for name in IMPORT_FREE_EXAMPLES {
        let path = examples_dir().join(name);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read example {}: {e}", path.display()));
        assert!(
            !has_import_decl(&source),
            "precondition: example {name} must be import-free"
        );
        let stem = name.trim_end_matches(".ri").to_string();
        inputs.push((stem, source));
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = CfgSet::default();
    for (stem, source) in &inputs {
        let legacy = legacy_single_file(stem, source);
        let program = compile_entry(dir.path(), stem, source, &cfg);
        let entry = &program.entry;

        assert_eq!(
            diag_shapes(entry),
            diag_shapes(&legacy),
            "{stem}: diagnostics must match the legacy single-file compile"
        );
        assert_eq!(
            entry.content_hash, legacy.content_hash,
            "{stem}: content_hash must match the legacy single-file compile"
        );
        assert_kind_names_equal(entry, &legacy, stem);
        assert!(
            program.imports.is_empty(),
            "{stem}: import-free entry must have no import units"
        );
        assert_eq!(
            program.sources,
            vec![ModuleSource {
                module_key: stem.clone(),
                path: dir.path().join(format!("{stem}.ri")),
                source: source.clone(),
            }],
            "{stem}: sources must hold exactly the entry"
        );
    }
}

// ─── (2) dirty buffer ──────────────────────────────────────────────────────

/// The entry compiles from the caller-supplied buffer; `entry_path` need not
/// exist on disk, while siblings still resolve from disk.
#[test]
fn compile_program_entry_compiles_from_buffer_not_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.ri",
        "module a\npub structure def Wheel {\n    param r : Real = 1\n}\n",
    );
    let entry_path = dir.path().join("main.ri");
    assert!(!entry_path.exists(), "precondition: entry is not on disk");

    let buffer = "module main\nimport a\nstructure def Cart {\n    sub w = Wheel()\n}\n";
    let program = compile_entry(dir.path(), "main", buffer, &CfgSet::default());

    assert!(
        template_names(&program.entry).contains(&"Cart"),
        "entry must compile from the buffer, got templates {:?}",
        template_names(&program.entry)
    );
    assert_eq!(
        program.sources.last(),
        Some(&ModuleSource {
            module_key: "main".to_string(),
            path: entry_path,
            source: buffer.to_string(),
        }),
        "the entry's recorded source must be the passed buffer"
    );
}

// ─── (3) program shape ─────────────────────────────────────────────────────

/// `imports` holds the direct, followed imports in declaration order (deduped);
/// `sources` holds every compiled user module in topological order, entry
/// last; the entry's merged templates carry the 1-hop pub surface only.
#[test]
fn compile_program_reports_direct_imports_and_topological_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    let c_src = "module c\npub structure def Core {\n    param k : Real = 1\n}\n";
    let a_src = "module a\nimport c\npub structure def Arm {\n    param x : Real = 1\n}\nstructure def ArmPrivate {\n    param y : Real = 2\n}\n";
    let b_src = "module b\npub structure def Base {\n    param z : Real = 3\n}\n";
    write(dir.path(), "c.ri", c_src);
    write(dir.path(), "a.ri", a_src);
    write(dir.path(), "b.ri", b_src);
    let main_src = "module main\nimport a\nimport b\nimport a\nstructure def Robot {\n    param m : Real = 0\n}\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    assert_eq!(
        import_keys(&program),
        vec!["a", "b"],
        "direct imports in declaration order, a duplicate import line not duplicated"
    );
    for unit in &program.imports {
        assert_eq!(
            unit.origin,
            Some(dir.path().join(format!("{}.ri", unit.import_path))),
            "{}: origin must be the resolved file",
            unit.import_path
        );
        assert_eq!(dotted(&unit.module), unit.import_path);
    }

    let expected_sources: Vec<ModuleSource> = [("c", c_src), ("a", a_src), ("b", b_src)]
        .iter()
        .map(|(key, src)| ModuleSource {
            module_key: key.to_string(),
            path: dir.path().join(format!("{key}.ri")),
            source: src.to_string(),
        })
        .chain(std::iter::once(ModuleSource {
            module_key: "main".to_string(),
            path: dir.path().join("main.ri"),
            source: main_src.to_string(),
        }))
        .collect();
    assert_eq!(source_keys(&program), vec!["c", "a", "b", "main"]);
    assert_eq!(program.sources, expected_sources);

    let entry_templates = template_names(&program.entry);
    for name in ["Robot", "Arm", "Base"] {
        assert!(
            entry_templates.contains(&name),
            "entry templates must contain {name}, got {entry_templates:?}"
        );
    }
    assert!(
        !entry_templates.contains(&"Core"),
        "transitive pub templates are not merged (1-hop, I5), got {entry_templates:?}"
    );
    assert!(
        !entry_templates.contains(&"ArmPrivate"),
        "non-pub imported templates are not merged, got {entry_templates:?}"
    );
}

/// When two direct imports export the same pub template name, the existing
/// first-wins collision Warning still reaches the entry.
#[test]
fn compile_program_keeps_cross_import_collision_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.ri",
        "module a\npub structure def Same {\n    param x : Real = 1\n}\n",
    );
    write(
        dir.path(),
        "b.ri",
        "module b\npub structure def Same {\n    param y : Real = 2\n}\n",
    );
    let main_src = "module main\nimport a\nimport b\nstructure def Top {\n    sub s = Same()\n}\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    let collisions: Vec<&str> = program
        .entry
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("imported pub template 'Same'")
                && d.message.contains("first-wins")
        })
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        collisions.len(),
        1,
        "expected one cross-import collision warning, got {:?}",
        program.entry.diagnostics
    );
}
