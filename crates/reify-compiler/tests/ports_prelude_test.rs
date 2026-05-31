//! Spec §7.6 implicit-prelude CI signal.
//!
//! `Port`, `Directionality`, and refining port traits (e.g. `RotaryPort`)
//! must resolve WITHOUT an explicit `import std.ports*` because they ship in
//! the implicit prelude (every registered stdlib module is in scope unless a
//! module carries `#no_prelude`).
//!
//! This test compiles `examples/stdlib/ports_prelude.ri` — a deliberately
//! import-free file — and asserts zero Severity::Error diagnostics plus the
//! presence of the `PreludeCoupling` structure template.

use reify_compiler::EntityKind;
use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

/// `examples/stdlib/ports_prelude.ri` must compile without errors AND must
/// not contain any explicit `import std.ports` statement (the whole point is
/// that resolution comes from the implicit prelude, not an import).
///
/// Additionally the compiled output must contain a template named
/// `PreludeCoupling` of `EntityKind::Structure` — the spec §7.6 witness.
#[test]
fn example_ports_prelude_ri_compiles_without_import() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example_path = manifest_dir
        .join("../../examples/stdlib/ports_prelude.ri")
        .canonicalize()
        .expect("examples/stdlib/ports_prelude.ri should exist on disk");

    let source = std::fs::read_to_string(&example_path)
        .expect("failed to read examples/stdlib/ports_prelude.ri");

    // Guard: the example must demonstrate PRELUDE resolution, not import resolution.
    assert!(
        !source.contains("import std.ports"),
        "examples/stdlib/ports_prelude.ri must not contain `import std.ports` — \
         Port/Directionality/RotaryPort must resolve from the implicit prelude"
    );

    let compiled = compile_source_with_stdlib(&source);

    let example_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        example_errors.is_empty(),
        "examples/stdlib/ports_prelude.ri should compile without errors; got: {:?}",
        example_errors
    );

    assert!(
        compiled.templates.iter().any(|t| {
            t.name == "PreludeCoupling" && t.entity_kind == EntityKind::Structure
        }),
        "examples/stdlib/ports_prelude.ri should declare \
         'structure def PreludeCoupling<D: RotaryPort, N: Port>'; \
         found templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}
