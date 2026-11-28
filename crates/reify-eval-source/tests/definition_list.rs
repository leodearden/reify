//! The definition list of the second output (docs/prds/instances-on-the-core.md
//! section 5.2), seen through its one entry, `compile_entry`.

use std::path::Path;

use reify_ast::Declaration;
use reify_compiler::parse_with_stdlib;
use reify_compiler::stdlib_loader::stdlib_sources;
use reify_core::{
    DefinitionName, DimensionVector, ModuleName, ModulePath, PathName, Severity, Type,
};
use reify_eval_source::{
    DeclaredType, DefinitionList, MemberKind, SourceOutput, SourceRefusal, compile_entry,
};

fn name(text: &str) -> PathName {
    PathName::new(text).unwrap_or_else(|e| panic!("`{text}` is not a name: {e:?}"))
}

fn module(dotted: &str) -> ModuleName {
    ModuleName::new(dotted.split('.').map(name).collect())
        .unwrap_or_else(|e| panic!("`{dotted}` is not a module: {e:?}"))
}

fn entry_definition(text: &str) -> DefinitionName {
    DefinitionName::new(None, name(text))
}

fn module_definition(dotted: &str, text: &str) -> DefinitionName {
    DefinitionName::new(Some(module(dotted)), name(text))
}

fn accepted(path: &Path, source: &str) -> SourceOutput {
    compile_entry(path, source)
        .unwrap_or_else(|refusal| panic!("the source was refused: {refusal:?}\n{source}"))
}

fn names(list: &DefinitionList) -> Vec<DefinitionName> {
    list.iter()
        .map(|definition| definition.name.clone())
        .collect()
}

const EVERY_TOP_LEVEL_FORM: &str = "
structure def Plate { param t : Length = 3mm }
trait Marked { param mark : Int = 0 }
occurrence def Event { param at : Real = 0 }
enum Colour { Red, Green }
fn twice(x : Real) -> Real { x * 2 }
constraint def Positive {
    param v : Real
    v > 0
}
purpose ready(subject : Structure) {
    constraint determined(subject)
}
structure def Bracket { param w : Length = 5mm }
";

#[test]
fn the_list_holds_the_entry_files_definitions_by_qualified_name() {
    let output = accepted(Path::new("forms.ri"), EVERY_TOP_LEVEL_FORM);
    let entry: Vec<DefinitionName> = names(&output.definitions)
        .into_iter()
        .filter(|definition| definition.module().is_none())
        .collect();
    assert_eq!(
        entry,
        vec![
            entry_definition("Plate"),
            entry_definition("Event"),
            entry_definition("Bracket"),
        ],
        "the entry module's definitions are its structure and occurrence defs, in source order"
    );
    for not_a_definition in ["Marked", "Colour", "twice", "Positive", "ready"] {
        assert!(
            output
                .definitions
                .get(&entry_definition(not_a_definition))
                .is_none(),
            "`{not_a_definition}` is not a definition of the list"
        );
    }
}

#[test]
fn an_occurrence_def_is_lowered_exactly_as_a_structure_def() {
    let body = "{\n    param t : Length = 3mm\n    let area = t * t\n    constraint t > 0mm\n}\n";
    let source = format!("structure def Plate {body}occurrence def Event {body}");
    let output = accepted(Path::new("same_body.ri"), &source);
    let table = |definition: &str| {
        &output
            .definitions
            .get(&entry_definition(definition))
            .unwrap_or_else(|| panic!("`{definition}` is listed"))
            .members
    };
    let shape: Vec<(Option<&str>, MemberKind, &DeclaredType)> = table("Plate")
        .iter()
        .map(|member| {
            (
                member.name.as_ref().map(PathName::as_str),
                member.kind,
                &member.declared_type,
            )
        })
        .collect();
    let length = DeclaredType::Value(Type::length());
    let area = DeclaredType::Value(Type::Scalar {
        dimension: DimensionVector::AREA,
    });
    assert_eq!(
        shape,
        vec![
            (Some("t"), MemberKind::Cell, &length),
            (Some("area"), MemberKind::Cell, &area),
            (None, MemberKind::Constraint, &DeclaredType::None),
        ],
        "the structure def's table lowers its body"
    );
    assert_eq!(
        table("Event"),
        table("Plate"),
        "an occurrence def with the same body has the same table: names, kinds, declared \
         types, origins and branches"
    );
}

#[test]
fn the_list_reaches_imports_and_the_standard_library() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let main_source = "module main\nimport lib\nstructure def Main { sub m : Motor }\n";
    std::fs::write(dir.path().join("main.ri"), main_source).expect("main.ri written");
    std::fs::write(
        dir.path().join("lib.ri"),
        "module lib\npub structure def Motor { param p : Real = 1 }\n\
         structure def Hidden { param q : Real = 2 }\n",
    )
    .expect("lib.ri written");

    let output = accepted(&dir.path().join("main.ri"), main_source);
    let listed = names(&output.definitions);
    let position = |definition: &DefinitionName| {
        listed
            .iter()
            .position(|listed| listed == definition)
            .unwrap_or_else(|| panic!("{definition:?} is not in the list: {listed:?}"))
    };

    let main = position(&entry_definition("Main"));
    let motor = position(&module_definition("lib", "Motor"));
    let hidden = position(&module_definition("lib", "Hidden"));
    let tolerance = position(&module_definition(
        "std.tolerancing",
        "DimensionalTolerance",
    ));

    assert!(
        main < motor && motor < hidden,
        "the entry's definitions come first, then lib's in source order: {listed:?}"
    );
    assert!(
        hidden < tolerance,
        "the user modules' definitions come before the standard library's: {listed:?}"
    );
    let last_entry = listed
        .iter()
        .rposition(|definition| definition.module().is_none())
        .expect("the entry has a definition");
    let first_stdlib = listed
        .iter()
        .position(|definition| {
            definition
                .module()
                .is_some_and(|module| module.segments()[0].as_str() == "std")
        })
        .expect("the standard library has a definition");
    let last_user = listed
        .iter()
        .rposition(|definition| {
            definition
                .module()
                .is_some_and(|module| module.segments()[0].as_str() != "std")
        })
        .expect("lib has a definition");
    assert!(
        last_entry < motor && last_user < first_stdlib,
        "entry, then user modules, then the standard library: {listed:?}"
    );
}

#[test]
fn a_source_that_does_not_parse_is_refused() {
    let refusal = compile_entry(
        Path::new("broken.ri"),
        "structure def Broken { param t : Length = }\n",
    )
    .expect_err("a source with a syntax error is refused, never a partial list");
    match refusal {
        SourceRefusal::NotParsed { module, errors } => {
            assert_eq!(module, ModulePath::single("broken"));
            assert!(!errors.is_empty(), "the refusal carries the parse errors");
        }
        other => panic!("expected NotParsed, got {other:?}"),
    }
}

#[test]
fn an_import_the_program_could_not_compile_is_refused() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let source = "import missing_mod\nstructure def A { param t : Length = 1mm }\n";
    let refusal = compile_entry(&dir.path().join("needs.ri"), source)
        .expect_err("an import the program did not compile is refused");
    match refusal {
        SourceRefusal::ImportNotCompiled { module, import } => {
            assert_eq!(module, ModulePath::single("needs"));
            assert_eq!(import, ModulePath::single("missing_mod"));
        }
        other => panic!("expected ImportNotCompiled, got {other:?}"),
    }
}

/// A program compile follows a `std.*` import and compiles it from the
/// embedded stdlib, so the import is compiled even though no file backs it.
#[test]
fn a_std_import_the_program_compiled_is_not_refused() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let source = "import std.si_units\nstructure def A { param p : Pressure = 200GPa }\n";
    let output = accepted(&dir.path().join("stiff.ri"), source);
    assert!(
        names(&output.definitions).contains(&entry_definition("A")),
        "the entry's definition is listed"
    );
}

#[test]
fn a_std_import_naming_no_std_module_is_refused() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let source = "import std.no_such_module\nstructure def A { param t : Length = 1mm }\n";
    let refusal = compile_entry(&dir.path().join("needs.ri"), source)
        .expect_err("a std import the program did not compile is refused");
    match refusal {
        SourceRefusal::ImportNotCompiled { module, import } => {
            assert_eq!(module, ModulePath::single("needs"));
            assert_eq!(
                import,
                ModulePath::from_dotted("std.no_such_module").expect("a dotted path")
            );
        }
        other => panic!("expected ImportNotCompiled, got {other:?}"),
    }
}

#[test]
fn tables_are_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<reify_eval_source::DefinitionList>();
}

/// Every structure and occurrence definition the standard library's sources
/// write, in registration order, with whether it declares members of its own.
fn stdlib_definitions_written() -> Vec<(DefinitionName, bool)> {
    let mut written = Vec::new();
    for (registered, source) in stdlib_sources() {
        let path = ModulePath::from_dotted(registered)
            .unwrap_or_else(|e| panic!("`{registered}` is a module path: {e:?}"));
        let parsed = parse_with_stdlib(&source, path);
        assert!(
            parsed.errors.is_empty(),
            "the stdlib module `{registered}` parses: {:?}",
            parsed.errors
        );
        for declaration in &parsed.declarations {
            let (definition, members) = match declaration {
                Declaration::Structure(def) => (&def.name, &def.members),
                Declaration::Occurrence(def) => (&def.name, &def.members),
                _ => continue,
            };
            written.push((
                module_definition(registered, definition),
                !members.is_empty(),
            ));
        }
    }
    written
}

#[test]
fn the_standard_library_lowers_with_no_error() {
    let output = accepted(
        Path::new("probe.ri"),
        "structure def Probe { param x : Length = 1mm }\n",
    );
    let errors: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the standard library lowers with no error: {errors:#?}"
    );

    let written = stdlib_definitions_written();
    assert!(
        !written.is_empty(),
        "the standard library writes definitions"
    );
    let listed: Vec<DefinitionName> = names(&output.definitions)
        .into_iter()
        .filter(|definition| {
            definition
                .module()
                .is_some_and(|module| module.segments()[0].as_str() == "std")
        })
        .collect();
    let expected: Vec<DefinitionName> = written.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(
        listed, expected,
        "the list holds every stdlib structure and occurrence def, in registration order"
    );
    for (definition, declares_members) in &written {
        let table = &output
            .definitions
            .get(definition)
            .unwrap_or_else(|| panic!("{definition:?} is listed"))
            .members;
        assert!(
            !declares_members || !table.is_empty(),
            "{definition:?} declares members, so its table is not empty"
        );
    }
}

#[test]
fn the_entry_is_deterministic() {
    let source = "
trait Weighted { param mass : Mass = 1kg }
structure def Plate : Weighted {
    param t : Length = 3mm
    let area = t * t
    sub tolerance : DimensionalTolerance
}
structure def Holder { sub p : Plate   let doubled = p.t * 2 }
";
    let first = accepted(Path::new("same.ri"), source);
    let second = accepted(Path::new("same.ri"), source);
    assert_eq!(
        first.definitions, second.definitions,
        "two lowerings of one source give equal lists"
    );
}

#[test]
fn an_import_that_binds_against_the_standard_library_lists_it() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let main_source = "module main\n#no_prelude\nimport lib\n\
                       structure def Main { param x : Real = 1.0 }\n";
    std::fs::write(dir.path().join("main.ri"), main_source).expect("main.ri written");
    std::fs::write(
        dir.path().join("lib.ri"),
        "module lib\npub structure def L { sub d : DimensionalTolerance }\n",
    )
    .expect("lib.ri written");

    let output = accepted(&dir.path().join("main.ri"), main_source);
    let lib = output
        .definitions
        .get(&module_definition("lib", "L"))
        .expect("the import's definition is listed");
    let d = lib
        .members
        .iter()
        .find(|member| member.name == Some(name("d")))
        .expect("`d` is in L's table");
    let tolerance = module_definition("std.tolerancing", "DimensionalTolerance");
    assert_eq!(
        d.declared_type,
        DeclaredType::Definition(tolerance.clone()),
        "lib does not opt out of the prelude, so its sub binds into the standard library"
    );
    assert!(
        output.definitions.get(&tolerance).is_some(),
        "a definition a listed sub names is listed too, though the entry opts out of the \
         prelude: {:?}",
        names(&output.definitions)
    );
}

#[test]
fn a_program_whose_every_module_opts_out_lists_no_stdlib_definition() {
    let output = accepted(
        Path::new("alone.ri"),
        "#no_prelude\nstructure def Alone { param x : Real = 1.0 }\n",
    );
    assert_eq!(
        names(&output.definitions),
        vec![entry_definition("Alone")],
        "no module binds against the standard library, so none of it is listed"
    );
}
