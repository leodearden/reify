//! The `std.prelude` facade (task #5502; PRD `docs/prds/v0_6/stdlib-namespace.md`
//! ι, D-4, boundary #11): one `import std.prelude` brings the curated core,
//! because the facade `pub import`s each core module and declares nothing of
//! its own.

use reify_compiler::CompiledModule;
use reify_compiler::stdlib_loader;
use reify_core::Severity;

/// The curated core, in declaration order: the facade's public contract.
const CURATED_CORE: [&str; 6] = [
    "std.units",
    "std.si_units",
    "std.option_recovery",
    "std.result",
    "std.determinacy.purposes",
    "std.io",
];

fn std_prelude() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|module| module.path.to_dotted() == "std.prelude")
        .expect("std.prelude is a registered stdlib module")
}

#[test]
fn std_prelude_is_a_facade_over_the_curated_core() {
    let facade = std_prelude();

    assert!(
        facade.imports.iter().all(|import| import.is_pub),
        "every std.prelude import must be a `pub import`; got: {:?}",
        facade.imports
    );
    let imported: Vec<&str> = facade.imports.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(imported, CURATED_CORE, "std.prelude's curated core");

    assert!(
        facade.templates.is_empty(),
        "std.prelude declares no templates"
    );
    assert!(
        facade.functions.is_empty(),
        "std.prelude declares no functions"
    );
    assert!(facade.enum_defs.is_empty(), "std.prelude declares no enums");
    assert!(
        facade.trait_defs.is_empty(),
        "std.prelude declares no traits"
    );
    assert!(facade.units.is_empty(), "std.prelude declares no units");
    assert!(
        facade.type_aliases.is_empty(),
        "std.prelude declares no type aliases"
    );
    assert!(
        facade.constraint_defs.is_empty(),
        "std.prelude declares no constraint defs"
    );
    assert!(facade.fields.is_empty(), "std.prelude declares no fields");
    assert!(
        facade
            .compiled_purposes
            .iter()
            .all(|p| p.declaration_span.is_prelude()),
        "std.prelude may carry only merged purpose copies; got own: {:?}",
        facade
            .compiled_purposes
            .iter()
            .filter(|p| !p.declaration_span.is_prelude())
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    let errors: Vec<_> = facade
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "std.prelude has errors: {errors:?}");
}
