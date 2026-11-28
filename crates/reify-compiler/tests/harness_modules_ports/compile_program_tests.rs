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
use reify_compiler::program::{
    CompiledProgram, ImportReach, compile_program, compile_program_with_parsed_entry,
};
use reify_compiler::program_scope::bind_program;
use reify_compiler::{
    CompiledModule, TopologyTemplate, check_module_path_decl,
};
use reify_core::{DiagnosticCode, DimensionVector, ModulePath, Severity, SourceSpan, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, Satisfaction, Value};
use reify_test_support::mocks::MockConstraintChecker;

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
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
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

/// The single-file reference an import-free program entry must match (I1):
/// the stdlib-aware parse, `program::compile_single_file`, and the
/// module-path declaration diagnostic a driver pushes afterwards.
fn single_file_reference(stem: &str, source: &str) -> CompiledModule {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(stem));
    let mut compiled = reify_compiler::program::compile_single_file(
        &parsed,
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );
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
    reify_test_support::examples_corpus::examples_dir().to_path_buf()
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
/// to the strict single-file compile `program::compile_single_file` (PRD
/// boundary #1, invariant I1).
#[test]
fn compile_program_single_file_matches_compile_single_file() {
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
        let single_file = single_file_reference(stem, source);
        let program = compile_entry(dir.path(), stem, source, &cfg);
        let entry = program.entry();

        assert_eq!(
            diag_shapes(entry),
            diag_shapes(&single_file),
            "{stem}: diagnostics must match the single-file compile"
        );
        assert_eq!(
            entry.content_hash, single_file.content_hash,
            "{stem}: content_hash must match the single-file compile"
        );
        assert_kind_names_equal(entry, &single_file, stem);
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
        template_names(program.entry()).contains(&"Cart"),
        "entry must compile from the buffer, got templates {:?}",
        template_names(program.entry())
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
/// last; the entry holds only its own templates, and the program binds the
/// direct imports' pub templates after them (1-hop).
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

    assert_eq!(template_names(program.entry()), vec!["Robot"]);
    assert_eq!(
        bound_template_names(&program),
        vec!["Robot", "Arm", "Base"],
        "the program binds the entry's own templates, then each direct import's pub \
         templates; a transitive or non-pub template is not bound"
    );
}

fn bound_template_names(program: &CompiledProgram) -> Vec<&str> {
    bind_program::<TopologyTemplate>(program, &[])
        .winners()
        .map(|t| t.name.as_str())
        .collect()
}

/// An importing entry's template list holds exactly the templates it declares
/// (resolution-unification boundary #14): no import's pub template is copied
/// into it, and a non-pub one is not visible at all.
#[test]
fn an_importing_entrys_templates_are_only_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "parts.ri",
        "module parts\n\
         pub structure def Pulley {\n    param d : Length = 10mm\n}\n\
         pub structure def Bracket {\n    param t : Length = 2mm\n}\n\
         structure def Hidden {\n    param h : Length = 1mm\n}\n",
    );
    let main_src = "module main\nimport parts\nstructure def Rig {\n    sub p = Pulley()\n}\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    assert_eq!(template_names(program.entry()), vec!["Rig"]);
    assert_eq!(
        bound_template_names(&program),
        vec!["Rig", "Pulley", "Bracket"]
    );
}

/// A template collision, entry-vs-import or import-vs-import, is reported once
/// on the entry, coded `DefinitionCollision` and naming the winner before the
/// shadowed module (resolution-unification boundary #13); no other diagnostic
/// reports it.
#[test]
fn a_template_collision_is_reported_once_with_its_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.ri",
        "module a\npub structure def Same {\n    param x : Real = 1\n}\n\
         pub structure def Both {\n    param x : Real = 1\n}\n",
    );
    write(
        dir.path(),
        "b.ri",
        "module b\npub structure def Both {\n    param y : Real = 2\n}\n",
    );
    let main_src = "module main\nimport a\nimport b\n\
                    structure def Same {\n    param z : Real = 3\n}\n\
                    structure def Top {\n    sub s = Same()\n    sub t = Both()\n}\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    let all = &program.entry().diagnostics;
    for (name, winner, shadowed) in [("Same", "'main'", "'a'"), ("Both", "'a'", "'b'")] {
        let mentioning: Vec<&reify_core::Diagnostic> = all
            .iter()
            .filter(|d| d.message.contains(&format!("'{name}'")))
            .collect();
        assert_eq!(
            mentioning.len(),
            1,
            "exactly one diagnostic reports the {name} collision, got {all:?}"
        );
        let report = mentioning[0];
        let position = |origin: &str| report.message.find(origin);
        assert!(
            report.severity == Severity::Warning
                && report.code == Some(DiagnosticCode::DefinitionCollision)
                && report.message.starts_with(&format!("structure '{name}' "))
                && report.message.contains("first-wins")
                && matches!(
                    (position(winner), position(shadowed)),
                    (Some(w), Some(s)) if w < s
                ),
            "the {name} report is a coded first-wins Warning naming {winner} before \
             {shadowed}: {report:?}"
        );
    }
    assert!(
        all.iter()
            .all(|d| !d.message.contains("imported pub template")),
        "no code-less template-merge warning remains, got {all:?}"
    );
}

/// Two imports declaring one pub unit: the FIRST import binds through the real
/// `compile_program` path (resolution-unification D-6), and the entry's own
/// compile reports the collision once, naming both modules.
#[test]
fn compile_program_unit_collision_between_imports_is_first_wins() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.ri",
        "module a\npub unit fortnight : Time = 1209600.0\n",
    );
    write(
        dir.path(),
        "b.ri",
        "module b\npub unit fortnight : Time = 2.0\n",
    );
    let main_src = "module main\nimport a\nimport b\nunit sennight : Time = 1fortnight\n\
                    structure def Top {\n    param x : Real = 1\n}\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    let factor_of = |module: &CompiledModule, name: &str| {
        module
            .units
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "unit {name} compiled in {}: {:?}",
                    dotted(module),
                    module.diagnostics
                )
            })
            .factor
    };
    let a_unit = &program.imports[0];
    assert_eq!(
        a_unit.import_path, "a",
        "precondition: a is the first import"
    );
    assert_eq!(
        factor_of(program.entry(), "sennight"),
        factor_of(&a_unit.module, "fortnight"),
        "sennight must be measured in a's fortnight (first import wins)"
    );

    let is_collision = |d: &&reify_core::Diagnostic| {
        d.severity == Severity::Warning
            && d.code == Some(DiagnosticCode::DefinitionCollision)
            && d.message.contains("'a'")
            && d.message.contains("'b'")
    };
    let on_entry: Vec<_> = program
        .entry()
        .diagnostics
        .iter()
        .filter(is_collision)
        .collect();
    assert_eq!(
        on_entry.len(),
        1,
        "exactly one DefinitionCollision warning naming a and b: {:?}",
        program.entry().diagnostics
    );
    assert_eq!(
        program.entry_own_diagnostics().filter(is_collision).count(),
        1,
        "the warning is raised by the entry's own compile, not re-homed"
    );
}

/// The program compile reports each first-wins collision whose loser is an
/// import's definition (resolution-unification D-6) on the entry, once, coded
/// `DefinitionCollision`: import-vs-import (`twice`) and entry-vs-import
/// (`Pick`). The report follows the re-homed copies of the imports' own
/// diagnostics (`b`'s `SignalKind` shadows that of the `std.ports.electrical`
/// it imports, an Error under stdlib-namespace D-2 stage 2).
#[test]
fn the_program_compile_reports_each_import_tier_collision_on_the_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "a.ri",
        "module a\n\
         pub fn twice(x: Real) -> Real { x * 2 }\n\
         pub enum Pick { Left, Right }\n",
    );
    write(
        dir.path(),
        "b.ri",
        "module b\n\
         import std.ports.electrical\n\
         pub fn twice(x: Real) -> Real { x + x }\n\
         pub enum SignalKind { Foo, Bar }\n",
    );
    let main_src = "module main\nimport a\nimport b\nenum Pick { Up, Down }\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    let all = &program.entry().diagnostics;
    let copies: Vec<&reify_core::Diagnostic> = program.rehomed_from("b").collect();
    assert!(
        !copies.is_empty(),
        "precondition: b's own compile reports its stdlib shadowing, got {all:?}"
    );
    let index_of = |target: &reify_core::Diagnostic| {
        all.iter()
            .position(|d| std::ptr::eq(d, target))
            .expect("a diagnostic of the entry")
    };
    let last_copy = copies.iter().map(|copy| index_of(copy)).max();
    for (prefix, winner, shadowed) in [
        ("function 'twice' ", "'a'", "'b'"),
        ("enum 'Pick' ", "'main'", "'a'"),
    ] {
        let reports: Vec<&reify_core::Diagnostic> = all
            .iter()
            .filter(|d| {
                let position = |origin: &str| d.message.find(origin);
                d.severity == Severity::Warning
                    && d.code == Some(DiagnosticCode::DefinitionCollision)
                    && d.message.starts_with(prefix)
                    && d.message.contains("first-wins")
                    && matches!(
                        (position(winner), position(shadowed)),
                        (Some(w), Some(s)) if w < s
                    )
            })
            .collect();
        assert_eq!(
            reports.len(),
            1,
            "one coded report of {prefix}({winner} over {shadowed}), got {all:?}"
        );
        assert!(
            program
                .entry_own_diagnostics()
                .any(|own| std::ptr::eq(own, reports[0])),
            "{prefix}is the program compile's own report, not a re-homed copy"
        );
        assert!(
            Some(index_of(reports[0])) > last_copy,
            "{prefix}follows the re-homed copies, got {all:?}"
        );
    }
}

fn errors(module: &CompiledModule) -> Vec<&str> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

fn is_std_key(key: &str) -> bool {
    key == "std" || key.starts_with("std.")
}

// ─── κ #5503: import units see only what they import (#19 inverted) ───────

/// An imported module that uses stdlib traits and constructors:
/// `ABS_Plastic`/`ElasticMaterial` come from std.materials.fea, the same
/// pattern stdlib/fdm.ri uses. `Length`/`mm` are builtins.
const PARTS_WITH_STDLIB_NAMES: &str = "module parts\n\
pub structure def Bracket {\n\
\x20   param w : Length = 10mm\n\
\x20   param material : ElasticMaterial = ABS_Plastic()\n\
}\n";

const MAIN_USING_PARTS: &str =
    "module main\nimport parts\nstructure def Rig {\n    sub b = Bracket()\n}\n";

/// Under strict visibility (stdlib-namespace NS-V1, κ #5503) an import unit
/// sees builtins plus its own imports, as the entry does. A std name it writes
/// without importing its module draws the NS-V2 fix-it, re-homed onto the
/// entry from the unit's file. This inverts RU β #5516's #19 pin, under which
/// every unit compiled against the whole stdlib.
#[test]
fn compile_program_import_unit_without_the_std_import_gets_the_fixit() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "parts.ri", PARTS_WITH_STDLIB_NAMES);
    let program = compile_entry(dir.path(), "main", MAIN_USING_PARTS, &CfgSet::default());

    let fixits: Vec<_> = program
        .rehomed_from("parts")
        .filter(|d| d.code == Some(DiagnosticCode::StdNameNotImported))
        .collect();
    assert!(
        !fixits.is_empty(),
        "parts' unimported std names surface on the entry, got {:#?}",
        program.entry().diagnostics
    );
    for fixit in fixits {
        assert_eq!(fixit.severity, Severity::Error);
        assert!(
            fixit.message.contains("std.materials.fea"),
            "the fix-it names the defining module: {}",
            fixit.message
        );
    }
}

/// With the std import declared, the unit compiles clean and `param w :
/// Length` still resolves to a Length quantity inside it.
#[test]
fn compile_program_import_unit_with_the_std_import_compiles_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parts = PARTS_WITH_STDLIB_NAMES.replacen(
        "module parts\n",
        "module parts\nimport std.materials.fea\n",
        1,
    );
    write(dir.path(), "parts.ri", &parts);
    let program = compile_entry(dir.path(), "main", MAIN_USING_PARTS, &CfgSet::default());

    let parts_unit = program
        .imports
        .iter()
        .find(|unit| unit.import_path == "parts")
        .expect("parts is in the import tier");
    assert!(
        errors(&parts_unit.module).is_empty(),
        "the import unit must compile against its declared import, got {:?}",
        errors(&parts_unit.module)
    );
    let bracket = parts_unit
        .module
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket template in the import unit");
    let w = bracket
        .value_cells
        .iter()
        .find(|c| c.id.member == "w")
        .expect("value cell `w` on Bracket");
    assert_eq!(
        w.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "`param w : Length` must resolve to a Length quantity inside the import unit"
    );
    assert!(
        errors(program.entry()).is_empty(),
        "the entry must compile cleanly, got {:?}",
        errors(program.entry())
    );
}

// ─── κ #5503: std.* is walked from the embedded stdlib (I4 inverted) ──────

/// A `std.*` import is followed through the DAG and resolved from the
/// EMBEDDED stdlib only, never from the resolver's stdlib_root (κ #5503,
/// superseding RU I4). The stdlib_root holds a deliberately unparseable
/// `units.ri`, so a filesystem read would fail with a diagnostic naming it or
/// the stdlib_root. An import unit's unknown `std.*` path is a coded Error
/// naming the dotted path, with no filesystem path to report.
#[test]
fn compile_program_walks_std_imports_from_the_embedded_stdlib_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdlib_root = dir.path().join("stdlib");
    fs::create_dir(&stdlib_root).expect("create stdlib_root");
    write(
        &stdlib_root,
        "units.ri",
        "pub structure def {{{ not reify\n",
    );
    write(
        dir.path(),
        "parts.ri",
        "module parts\nimport std.nonexistent_typo\nimport std\npub structure def Part {\n    param q : Real = 1\n}\n",
    );
    let main_src =
        "module main\nimport std.units\nimport parts\nstructure def Rig {\n    sub p = Part()\n}\n";

    let program = compile_entry(dir.path(), "main", main_src, &CfgSet::default());

    let stdlib_root_text = stdlib_root.display().to_string();
    for d in &program.entry().diagnostics {
        assert!(
            !d.message.contains("units.ri") && !d.message.contains(&stdlib_root_text),
            "std.* resolves from the embedded stdlib only, but a diagnostic names the \
             stdlib_root: {d:?}"
        );
    }
    let units = program
        .imports
        .iter()
        .find(|unit| unit.import_path == "std.units")
        .unwrap_or_else(|| {
            panic!(
                "std.units is in the import tier, got {:?}",
                import_keys(&program)
            )
        });
    assert_eq!(units.origin, None, "an embedded std unit has no file");
    assert_eq!(units.reach, ImportReach::Direct);
    assert!(
        !source_keys(&program).into_iter().any(is_std_key),
        "sources hold user modules only, got {:?}",
        source_keys(&program)
    );
    let dir_text = dir.path().display().to_string();
    let typo = errors(program.entry())
        .into_iter()
        .find(|message| message.contains("std.nonexistent_typo"))
        .unwrap_or_else(|| {
            panic!(
                "the unknown std path is an Error on the entry, got {:#?}",
                program.entry().diagnostics
            )
        });
    assert!(
        !typo.contains(&dir_text) && !typo.contains(".ri"),
        "an embedded-only resolution has no filesystem path to report: {typo}"
    );
}

// ─── task 5516 step-5: a cfg-gated import is inert (I3) ────────────────────

/// A cfg-unsatisfied import is never resolved, compiled, merged or listed —
/// at the entry and inside a followed import unit alike (PRD I3).
#[test]
fn compile_program_cfg_gated_imports_are_inert() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "broken.ri",
        "module broken\npub structure def BrokenThing { param = }\n",
    );
    write(
        dir.path(),
        "mid.ri",
        "module mid\n#cfg(target = \"wasm\")\nimport ghost2\npub structure def Mid {\n    param m : Real = 1\n}\n",
    );
    let main_src = "module main\n\
                    #cfg(target = \"wasm\")\nimport ghost\n\
                    #cfg(target = \"wasm\")\nimport broken\n\
                    import mid\n\
                    structure def Rig {\n    sub m = Mid()\n}\n";
    let linux = CfgSet {
        target: Some("linux".to_string()),
        ..Default::default()
    };

    let program = compile_entry(dir.path(), "main", main_src, &linux);

    for message in errors(program.entry()) {
        for needle in ["ghost", "broken", "ghost2"] {
            assert!(
                !message.contains(needle),
                "a gated-out import must be inert, but an Error names {needle}: {message}"
            );
        }
    }
    for key in import_keys(&program)
        .into_iter()
        .chain(source_keys(&program))
    {
        assert!(
            !["ghost", "broken", "ghost2"].contains(&key),
            "a gated-out import must not be listed, found {key}"
        );
    }
    assert!(
        !template_names(program.entry()).contains(&"BrokenThing"),
        "no template from a gated-out import may reach the entry"
    );
    assert_eq!(import_keys(&program), vec!["mid"]);
}

// ─── task 5516 step-7: broken imports surface on the entry, attributed (I2) ──

const HEALTHY_SRC: &str =
    "module healthy\npub structure def Healthy {\n    param h : Real = 1\n}\n";

/// Write `healthy.ri` plus `extra` siblings, compile `main_src` as `main`, and
/// assert the entry still compiled: it declares its own `Rig`, and the healthy
/// import is one of the program's import units.
fn compile_with_healthy_sibling(
    dir: &Path,
    extra: &[(&str, &str)],
    main_src: &str,
) -> CompiledProgram {
    write(dir, "healthy.ri", HEALTHY_SRC);
    for (name, src) in extra {
        write(dir, name, src);
    }
    let program = compile_entry(dir, "main", main_src, &CfgSet::default());
    let names = template_names(program.entry());
    assert!(
        names.contains(&"Rig") && import_keys(&program).contains(&"healthy"),
        "the entry must still compile despite a broken import, got templates {names:?} \
         and imports {:?}",
        import_keys(&program)
    );
    program
}

/// (a) A missing import file surfaces as an Error naming the module and the
/// path that was tried.
#[test]
fn compile_program_missing_import_surfaces_error_naming_tried_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = compile_with_healthy_sibling(
        dir.path(),
        &[],
        "module main\nimport nowhere\nimport healthy\nstructure def Rig {\n    sub h = Healthy()\n}\n",
    );
    let tried = dir.path().join("nowhere.ri").display().to_string();
    assert!(
        errors(program.entry())
            .iter()
            .any(|m| m.contains("nowhere") && m.contains(&tried)),
        "expected an Error naming `nowhere` and {tried}, got {:?}",
        errors(program.entry())
    );
}

/// (b) An import that fails to parse surfaces one coded Error per parse error,
/// naming the imported file and the parser's own line:col.
#[test]
fn compile_program_import_parse_error_is_coded_and_file_attributed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let broken_src = "module broken\npub structure def B { param = }\n";
    let program = compile_with_healthy_sibling(
        dir.path(),
        &[("broken.ri", broken_src)],
        "module main\nimport broken\nimport healthy\nstructure def Rig {\n    sub h = Healthy()\n}\n",
    );

    let rendered = reify_compiler::parse_with_stdlib(broken_src, ModulePath::single("broken"))
        .render_errors(broken_src);
    assert!(
        !rendered.is_empty(),
        "precondition: broken.ri must fail to parse"
    );
    let broken_path = dir.path().join("broken.ri").display().to_string();
    for line_col_message in &rendered {
        let located = format!("{broken_path}:{line_col_message}");
        assert!(
            program.entry().diagnostics.iter().any(|d| {
                d.severity == Severity::Error
                    && d.code == Some(DiagnosticCode::ImportedModuleParseError)
                    && d.message.contains(&located)
            }),
            "expected a coded ImportedModuleParseError containing {located:?}, got {:?}",
            program.entry().diagnostics
        );
    }
    assert!(!import_keys(&program).contains(&"broken"));
    assert!(!source_keys(&program).contains(&"broken"));
}

/// (c) An import that parses and compiles but carries Error/Warning
/// diagnostics has each of them re-homed onto the entry: same severity, code
/// and candidates, the message prefixed with the imported module and its
/// `file:line:col`, and every label moved to the prelude sentinel because its
/// span indexes the imported file, not the entry.
#[test]
fn compile_program_rehomes_import_unit_diagnostics_onto_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parts_src =
        "module parts\npub structure def Part {\n    param q : Frobnicate = zorp()\n}\n";
    let program = compile_with_healthy_sibling(
        dir.path(),
        &[("parts.ri", parts_src)],
        "module main\nimport parts\nimport healthy\nstructure def Rig {\n    sub p = Part()\n    sub h = Healthy()\n}\n",
    );

    let parts_unit = program
        .imports
        .iter()
        .find(|u| u.import_path == "parts")
        .expect("parts is a compiled import unit");
    let originals: Vec<&reify_core::Diagnostic> = parts_unit
        .module
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
        .collect();
    assert!(
        originals.iter().any(|d| d.severity == Severity::Error),
        "precondition: the parts unit must carry at least one Error, got {:?}",
        parts_unit.module.diagnostics
    );

    let parts_path = dir.path().join("parts.ri").display().to_string();
    let rehomed: Vec<&reify_core::Diagnostic> = program.rehomed_from("parts").collect();
    for original in &originals {
        let location = original
            .labels
            .iter()
            .find(|l| l.span != SourceSpan::prelude())
            .map(|l| {
                let (line, col) =
                    reify_core::byte_offset_to_line_col(parts_src, l.span.start as usize);
                format!("{parts_path}:{line}:{col}")
            })
            .unwrap_or_else(|| parts_path.clone());
        let copy = rehomed.iter().find(|d| {
            d.severity == original.severity
                && d.code == original.code
                && d.candidates == original.candidates
                && d.message.contains(&format!("({location})"))
                && d.message.ends_with(&original.message)
        });
        let copy = copy.unwrap_or_else(|| {
            panic!(
                "no re-homed copy of {original:?} attributed to {location}; re-homed: {rehomed:?}"
            )
        });
        assert!(
            copy.labels.iter().all(|l| l.span == SourceSpan::prelude()),
            "a re-homed label must not index the entry file: {copy:?}"
        );
    }
    assert!(
        rehomed.iter().any(|d| d.severity == Severity::Error),
        "at least one re-homed copy must be an Error"
    );
    assert!(
        rehomed.iter().all(|d| d.severity != Severity::Info),
        "Info diagnostics are not re-homed: {rehomed:?}"
    );
    let own_errors: Vec<&reify_core::Diagnostic> = program
        .entry_own_diagnostics()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        own_errors.is_empty(),
        "the entry raised no Error of its own; its own diagnostics must hold no copy: \
         {own_errors:?}"
    );
}

/// (d) A transitive import's Error is surfaced on the entry exactly once,
/// attributed to its own file.
#[test]
fn compile_program_rehomes_transitive_import_errors_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = compile_with_healthy_sibling(
        dir.path(),
        &[
            (
                "mid.ri",
                "module mid\nimport leaf\npub structure def Mid {\n    param m : Real = 1\n}\n",
            ),
            (
                "leaf.ri",
                "module leaf\npub structure def Leaf {\n    param z : Nope\n}\n",
            ),
        ],
        "module main\nimport mid\nimport healthy\nstructure def Rig {\n    sub m = Mid()\n    sub h = Healthy()\n}\n",
    );

    let leaf_path = dir.path().join("leaf.ri").display().to_string();
    let leaf_errors: Vec<&reify_core::Diagnostic> = program
        .rehomed_from("leaf")
        .filter(|d| d.severity == Severity::Error && d.message.contains("Nope"))
        .collect();
    assert_eq!(
        leaf_errors.len(),
        1,
        "the leaf Error must reach the entry exactly once, got {:?}",
        program.entry().diagnostics
    );
    assert!(
        leaf_errors[0].message.contains(&leaf_path),
        "the leaf Error must be attributed to {leaf_path}: {:?}",
        leaf_errors[0]
    );
}

/// (e) A healthy import with a `module` declaration contributes nothing.
#[test]
fn compile_program_healthy_import_contributes_no_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = compile_with_healthy_sibling(
        dir.path(),
        &[],
        "module main\nimport healthy\nstructure def Rig {\n    sub h = Healthy()\n}\n",
    );
    let healthy_unit = &program.imports[0].module;
    assert!(
        !healthy_unit
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Error | Severity::Warning)),
        "precondition: the healthy unit must be clean, got {:?}",
        healthy_unit.diagnostics
    );
    assert_eq!(
        program.rehomed_from("healthy").count(),
        0,
        "a clean import must contribute nothing, got {:?}",
        program.entry().diagnostics
    );
}

const DIAMOND_MAIN_SRC: &str = "module main\nimport a\nimport b\nimport healthy\n\
     structure def Rig {\n    sub h = Healthy()\n}\n";

/// The two sides of a diamond: `a` and `b` both import `leaf`.
fn diamond_sides() -> [(&'static str, &'static str); 2] {
    [
        (
            "a.ri",
            "module a\nimport leaf\npub structure def A {\n    param x : Real = 1\n}\n",
        ),
        (
            "b.ri",
            "module b\nimport leaf\npub structure def B {\n    param y : Real = 1\n}\n",
        ),
    ]
}

/// (f) A leaf that fails to parse, imported by two direct imports, has each of
/// its parse errors reported on the entry once, not once per importer.
#[test]
fn compile_program_reports_a_broken_diamond_leaf_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let leaf_src = "module leaf\npub structure def Leaf { param = }\n";
    let [a, b] = diamond_sides();
    let program =
        compile_with_healthy_sibling(dir.path(), &[a, b, ("leaf.ri", leaf_src)], DIAMOND_MAIN_SRC);

    let rendered = reify_compiler::parse_with_stdlib(leaf_src, ModulePath::single("leaf"))
        .render_errors(leaf_src);
    assert!(
        !rendered.is_empty(),
        "precondition: leaf.ri must fail to parse"
    );
    let leaf_path = dir.path().join("leaf.ri").display().to_string();
    for line_col_message in &rendered {
        let located = format!("{leaf_path}:{line_col_message}");
        let reports = program
            .entry()
            .diagnostics
            .iter()
            .filter(|d| {
                d.code == Some(DiagnosticCode::ImportedModuleParseError)
                    && d.message.contains(&located)
            })
            .count();
        assert_eq!(
            reports,
            1,
            "the parse error at {located:?} must reach the entry exactly once, got {:?}",
            program.entry().diagnostics
        );
    }
}

/// (g) A leaf that compiles with an Error, imported by two direct imports, has
/// that Error re-homed onto the entry once.
#[test]
fn compile_program_rehomes_an_erroring_diamond_leaf_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let leaf_src = "module leaf\npub structure def Leaf {\n    param z : Nope\n}\n";
    let [a, b] = diamond_sides();
    let program =
        compile_with_healthy_sibling(dir.path(), &[a, b, ("leaf.ri", leaf_src)], DIAMOND_MAIN_SRC);

    let leaf_errors = program
        .rehomed_from("leaf")
        .filter(|d| d.severity == Severity::Error && d.message.contains("Nope"))
        .count();
    assert_eq!(
        leaf_errors,
        1,
        "the leaf Error must reach the entry exactly once, got {:?}",
        program.entry().diagnostics
    );
}

/// (h) The re-homed copies sit between entry-own diagnostics on both sides —
/// the entry's own and missing-import Errors before them, the program's coded
/// collision Warning after — and `entry_own_diagnostics` plus `rehomed_from`
/// still partition the entry's diagnostics exactly, in order.
#[test]
fn compile_program_partitions_entry_diagnostics_around_the_rehomed_block() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = compile_with_healthy_sibling(
        dir.path(),
        &[
            (
                "mid.ri",
                "module mid\nimport leaf\npub structure def Mid {\n    param m : Real = 1\n}\n",
            ),
            (
                "leaf.ri",
                "module leaf\npub structure def Leaf {\n    param z : Nope\n}\n",
            ),
            (
                "parts.ri",
                "module parts\npub structure def Part {\n    param q : Frobnicate = zorp()\n}\n",
            ),
            (
                "a.ri",
                "module a\npub structure def Same {\n    param x : Real = 1\n}\n",
            ),
            (
                "b.ri",
                "module b\npub structure def Same {\n    param y : Real = 2\n}\n",
            ),
        ],
        "module main\nimport nowhere\nimport mid\nimport parts\nimport a\nimport b\n\
         import healthy\nstructure def Rig {\n    param bad : Bogus\n    sub h = Healthy()\n}\n",
    );

    let all = &program.entry().diagnostics;
    let keys = source_keys(&program);
    let rehomed: Vec<(&str, &reify_core::Diagnostic)> = keys
        .iter()
        .flat_map(|key| program.rehomed_from(key).map(move |copy| (*key, copy)))
        .collect();
    let own: Vec<&reify_core::Diagnostic> = program.entry_own_diagnostics().collect();
    let is_rehomed = |d: &reify_core::Diagnostic| rehomed.iter().any(|(_, c)| std::ptr::eq(*c, d));
    let is_own = |d: &reify_core::Diagnostic| own.iter().any(|o| std::ptr::eq(*o, d));
    let index_of = |target: &reify_core::Diagnostic| {
        all.iter()
            .position(|d| std::ptr::eq(d, target))
            .unwrap_or_else(|| panic!("{target:?} is not one of the entry's diagnostics"))
    };
    let find = |severity: Severity, needle: &str| {
        all.iter()
            .find(|d| d.severity == severity && d.message.contains(needle))
            .unwrap_or_else(|| panic!("no {severity:?} mentioning {needle:?} in {all:?}"))
    };

    for key in ["leaf", "parts"] {
        assert!(
            program.rehomed_from(key).next().is_some(),
            "precondition: {key} must re-home at least one diagnostic, got {all:?}"
        );
    }
    let copy_indices: Vec<usize> = rehomed.iter().map(|(_, copy)| index_of(copy)).collect();
    let first_copy = *copy_indices.iter().min().expect("re-homed copies exist");
    let last_copy = *copy_indices.iter().max().expect("re-homed copies exist");
    assert!(
        all[..first_copy].iter().any(|d| !is_rehomed(d)),
        "precondition: an entry-own diagnostic must precede the re-homed run, got {all:?}"
    );
    let collision = all
        .iter()
        .find(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::DefinitionCollision)
                && d.message.starts_with("structure 'Same' ")
        })
        .unwrap_or_else(|| panic!("no cross-import collision Warning in {all:?}"));
    assert!(
        index_of(collision) > last_copy,
        "precondition: the collision Warning must follow the re-homed run, got {all:?}"
    );

    for d in all {
        let yielded = usize::from(is_own(d))
            + rehomed
                .iter()
                .filter(|(_, copy)| std::ptr::eq(*copy, d))
                .count();
        assert_eq!(
            yielded, 1,
            "{d:?} must be yielded by exactly one of entry_own_diagnostics and rehomed_from"
        );
    }
    let not_rehomed: Vec<&reify_core::Diagnostic> = all.iter().filter(|d| !is_rehomed(d)).collect();
    assert!(
        own.len() == not_rehomed.len()
            && own
                .iter()
                .zip(&not_rehomed)
                .all(|(o, n)| std::ptr::eq(*o, *n)),
        "entry_own_diagnostics must be the non-re-homed diagnostics in order: \
         own {own:?}, expected {not_rehomed:?}"
    );
    for (key, copy) in &rehomed {
        let prefix = format!("in imported module '{key}' (");
        assert!(
            copy.message.starts_with(&prefix),
            "a copy re-homed from {key} must start {prefix:?}: {copy:?}"
        );
    }
    for (label, d) in [
        ("collision Warning", collision),
        ("missing-import Error", find(Severity::Error, "nowhere")),
        ("entry's own Error", find(Severity::Error, "Bogus")),
    ] {
        assert!(is_own(d), "the {label} must be entry-own: {d:?}");
    }
    for key in ["main", "nowhere"] {
        assert_eq!(
            program.rehomed_from(key).count(),
            0,
            "{key} re-homes nothing"
        );
    }
}

// ─── task 5516 step-13: the caller-owned-parse seam ────────────────────────

fn compile_parsed_entry(
    dir: &Path,
    stem: &str,
    source: &str,
    parsed: &reify_ast::ParsedModule,
) -> CompiledProgram {
    compile_program_with_parsed_entry(
        &dir.join(format!("{stem}.ri")),
        source,
        parsed,
        &resolver_for(dir),
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    )
}

/// Handed the same stdlib-aware parse `compile_program` makes itself, the seam
/// compiles the same program.
#[test]
fn compile_program_with_parsed_entry_matches_compile_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "healthy.ri",
        "module healthy\npub structure def Healthy {\n    param k : Real = 1\n}\n",
    );
    let cases = [
        (
            "solo",
            "module solo\nstructure def Plate {\n    param w : Length = 10mm\n}\n",
        ),
        (
            "main",
            "module main\nimport healthy\nstructure def Rig {\n    sub h = Healthy()\n}\n",
        ),
    ];
    for (stem, source) in cases {
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(stem));
        let seam = compile_parsed_entry(dir.path(), stem, source, &parsed);
        let whole = compile_entry(dir.path(), stem, source, &CfgSet::default());

        assert_eq!(
            diag_shapes(seam.entry()),
            diag_shapes(whole.entry()),
            "{stem}: entry diagnostics"
        );
        assert_eq!(
            template_names(seam.entry()),
            template_names(whole.entry()),
            "{stem}: entry templates"
        );
        assert_eq!(import_keys(&seam), import_keys(&whole), "{stem}: imports");
        assert_eq!(source_keys(&seam), source_keys(&whole), "{stem}: sources");
    }
}

/// The seam compiles the caller's parse, not a re-parse: a declaration only
/// the caller's parse carries reaches the entry's declared templates.
#[test]
fn compile_program_with_parsed_entry_compiles_the_callers_parse() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = "structure def Main {\n    param k : Real = 1\n}\n";
    let callers_text =
        format!("{source}structure def OnlyInCallersParse {{\n    param z : Real = 2\n}}\n");
    let parsed = reify_compiler::parse_with_stdlib(&callers_text, ModulePath::single("main"));

    let program = compile_parsed_entry(dir.path(), "main", source, &parsed);

    assert!(
        errors(program.entry()).is_empty(),
        "the caller's parse must compile cleanly, got {:?}",
        program.entry().diagnostics
    );
    assert_eq!(
        template_names(program.entry()),
        ["Main", "OnlyInCallersParse"],
        "the seam must compile the caller's parse, not a re-parse of the text"
    );
}

// ─── task 5522: imported-enum access resolves in the compile phase (D-9) ───

/// `parse_with_stdlib` seeds only stdlib enum names, so an imported enum's
/// `Pick.Left` reaches the compiler as a member access on `Pick`.
const PICK_MODULE: &str = "pub enum Pick { Left }\n";

fn pick_entry(variant: &str) -> String {
    format!("import a\n\nstructure def Main {{\n    let p = Pick.{variant}\n}}\n")
}

fn let_default<'a>(module: &'a CompiledModule, template: &str, member: &str) -> &'a CompiledExpr {
    module
        .templates
        .iter()
        .find(|t| t.name == template)
        .unwrap_or_else(|| panic!("template {template} compiled in {}", dotted(module)))
        .value_cells
        .iter()
        .find(|cell| cell.id.member == member)
        .and_then(|cell| cell.default_expr.as_ref())
        .unwrap_or_else(|| panic!("{template}.{member} has a compiled default"))
}

fn assert_is_pick_left(expr: &CompiledExpr) {
    assert!(
        matches!(
            &expr.kind,
            CompiledExprKind::Literal(Value::Enum { type_name, variant, payload })
                if type_name == "Pick" && variant == "Left" && payload.is_empty()
        ),
        "expected the enum value Pick.Left, got {expr:?}"
    );
    assert_eq!(expr.result_type, Type::Enum("Pick".to_string()));
}

#[test]
fn imported_enum_variant_access_compiles_to_the_enum_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "a.ri", PICK_MODULE);

    let program = compile_entry(dir.path(), "main", &pick_entry("Left"), &CfgSet::default());

    assert!(
        errors(program.entry()).is_empty(),
        "an imported enum's variant must resolve, got {:?}",
        program.entry().diagnostics
    );
    assert_is_pick_left(let_default(program.entry(), "Main", "p"));
}

#[test]
fn imported_enum_unknown_variant_is_reported_as_such() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "a.ri", PICK_MODULE);

    let program = compile_entry(dir.path(), "main", &pick_entry("Nope"), &CfgSet::default());

    let errors = errors(program.entry());
    assert!(
        errors
            .iter()
            .any(|e| e.contains("unknown variant 'Nope' on enum 'Pick'")),
        "expected the unknown-variant Error, got {errors:?}"
    );
    assert!(
        errors.iter().all(|e| !e.contains("unresolved name: Pick")),
        "an imported enum name must resolve, got {errors:?}"
    );
}

/// D-9 tests `E` against the compile env's enums, which still bind a direct
/// import's NON-pub enums (the compile side's ambient import visibility), so
/// a hidden enum's `E.V` compiles today. Narrowing that to the D-5 `pub`
/// surface DefEnv already applies is owned by λ #5525, which inverts this pin.
#[test]
fn imported_non_pub_enum_variant_access_still_compiles() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "a.ri", "enum Pick { Left }\n");

    let program = compile_entry(dir.path(), "main", &pick_entry("Left"), &CfgSet::default());

    let pick = program.imports[0]
        .module
        .enum_defs
        .iter()
        .find(|e| e.name == "Pick")
        .expect("a compiles its enum Pick");
    assert!(!pick.is_pub, "precondition: a's Pick is not pub");
    assert!(
        errors(program.entry()).is_empty(),
        "a direct import's non-pub enum value still resolves, got {:?}",
        program.entry().diagnostics
    );
    assert_is_pick_left(let_default(program.entry(), "Main", "p"));
}

/// D-9 applies inside every DAG unit, not just the entry: an import unit is
/// parsed with stdlib enum names only, too.
#[test]
fn imported_enum_access_resolves_inside_a_transitive_import_unit() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "a.ri", PICK_MODULE);
    write(
        dir.path(),
        "b.ri",
        "module b\nimport a\npub structure def UsesPick {\n    let q = Pick.Left\n}\n",
    );
    let entry = "import b\n\nstructure def Main {\n    sub u = UsesPick()\n}\n";

    let program = compile_entry(dir.path(), "main", entry, &CfgSet::default());

    let rehomed_errors: Vec<&reify_core::Diagnostic> = program
        .rehomed_from("b")
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        rehomed_errors.is_empty(),
        "b must compile its imported-enum access cleanly, got {rehomed_errors:?}"
    );
    let b_unit = program
        .imports
        .iter()
        .find(|unit| unit.import_path == "b")
        .expect("b is a compiled import unit");
    assert_is_pick_left(let_default(&b_unit.module, "UsesPick", "q"));
}

// ─── task 5520: the routing predicate a driver asks before a program compile ─

/// `entry_follows_imports` answers through the one import gate: every
/// cfg-satisfied import counts, `std.*` included (κ #5503), whether or not it
/// resolves.
#[test]
fn entry_follows_imports_for_every_cfg_satisfied_import_std_included() {
    let cfg = CfgSet {
        target: Some("linux".to_string()),
        ..Default::default()
    };
    let body = "structure def Rod {\n    param r : Length = 2mm\n}\n";
    let cases = [
        ("import-free", format!("module main\n{body}"), false),
        (
            "std-only",
            format!("module main\nimport std.units\n{body}"),
            true,
        ),
        (
            "cfg-gated-out user import",
            format!("module main\n#cfg(target = \"wasm\")\nimport ghost\n{body}"),
            false,
        ),
        ("user import", format!("module main\nimport parts\n{body}"), true),
        (
            "std plus user import",
            format!("module main\nimport std.units\nimport parts\n{body}"),
            true,
        ),
    ];
    for (label, source, expected) in cases {
        let parsed = reify_compiler::parse_with_stdlib(&source, ModulePath::single("main"));
        assert_eq!(
            reify_compiler::program::entry_follows_imports(&parsed, &cfg),
            expected,
            "{label}"
        );
    }
}

/// `followed_imports` names exactly the imports `compile_program` compiles:
/// cfg-satisfied (I3), `std.*` included (κ #5503), in declaration order, one
/// per path.
#[test]
fn followed_imports_lists_exactly_what_compile_program_follows() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "parts.ri",
        "module parts\npub structure def Part {\n    param p : Real = 1\n}\n",
    );
    write(
        dir.path(),
        "util.ri",
        "module util\npub structure def Util {\n    param u : Real = 2\n}\n",
    );
    let main_src = "module main\n\
                    import std.units\n\
                    import parts\n\
                    #cfg(target = \"wasm\")\nimport ghost\n\
                    import parts\n\
                    import util\n\
                    structure def Rig {\n    sub p = Part()\n    sub u = Util()\n}\n";
    let linux = CfgSet {
        target: Some("linux".to_string()),
        ..Default::default()
    };
    let parsed = reify_compiler::parse_with_stdlib(main_src, ModulePath::single("main"));

    let followed = reify_compiler::program::followed_imports(&parsed, &linux);
    let followed_paths: Vec<&str> = followed.iter().map(|import| import.path.as_str()).collect();

    assert_eq!(
        followed_paths,
        vec!["std.units", "parts", "util"],
        "declaration order, the cfg-gated import excluded, the duplicate listed once"
    );
    assert_eq!(
        followed_paths,
        import_keys(&compile_entry(dir.path(), "main", main_src, &linux)),
        "followed_imports must report the follow set compile_program compiles"
    );
    let follows = reify_compiler::program::entry_follows_imports(&parsed, &linux);
    assert!(follows);
    assert_eq!(follows, !followed.is_empty());
}

/// `entry_dir_resolver` resolves an entry's user imports from the entry file's
/// own directory.
#[test]
fn entry_dir_resolver_resolves_imports_beside_the_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "healthy.ri", HEALTHY_SRC);
    let main_src = "module main\nimport healthy\nstructure def Rig {\n    sub h = Healthy()\n}\n";
    let main = write(dir.path(), "main.ri", main_src);

    let program = compile_program(
        &main,
        main_src,
        &reify_compiler::program::entry_dir_resolver(&main),
        &CfgSet::default(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );

    assert_eq!(import_keys(&program), vec!["healthy"]);
    assert!(
        errors(program.entry()).is_empty(),
        "the sibling import must resolve, got {:?}",
        errors(program.entry())
    );
}
