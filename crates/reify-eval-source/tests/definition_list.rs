//! The definition list of the second output (docs/prds/instances-on-the-core.md
//! section 5.2), seen through its one entry, `compile_entry`.

use std::path::Path;

use reify_core::{DefinitionName, ModuleName, ModulePath, PathName};
use reify_eval_source::{DefinitionList, SourceOutput, SourceRefusal, compile_entry};

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
    let event = output
        .definitions
        .get(&entry_definition("Event"))
        .expect("the occurrence def is listed");
    let plate = output
        .definitions
        .get(&entry_definition("Plate"))
        .expect("the structure def is listed");
    assert_eq!(
        event.members.len(),
        plate.members.len(),
        "an occurrence def is lowered exactly as a structure def"
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

#[test]
fn tables_are_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<reify_eval_source::DefinitionList>();
}
