//! The strict stdlib build (stdlib-namespace ζ, task #5499; PRD
//! `docs/prds/v0_6/stdlib-namespace.md` NS-V3 boundary #7, NS-M3 boundary #8):
//! a stdlib module sees builtins plus the pub surfaces of the modules it
//! DECLARES as imports, nothing else, and the build's output does not depend on
//! the order modules are registered in.
//!
//! Every test drives the production entry `stdlib_loader::build_stdlib_modules`.
//! Negatives assert on the panic message's content (the module and the
//! offending name), so an unrelated build failure cannot satisfy them.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_compiler::stdlib_loader::{build_stdlib_modules, load_stdlib, stdlib_sources};

use crate::common::stdlib_build_panic_message;

const PURPOSES_MODULE: &str = "std.determinacy.purposes";

#[track_caller]
fn assert_build_panics_naming(sources: &[(&str, &str)], needles: &[&str]) {
    let msg = stdlib_build_panic_message(sources);
    for needle in needles {
        assert!(
            msg.contains(needle),
            "stdlib build panic should name {needle:?}; got:\n{msg}"
        );
    }
}

// ─── boundary #7: a module sees only what it declares ──────────────────────

#[test]
fn a_stdlib_module_referencing_an_undeclared_sibling_fails_the_stdlib_build() {
    let provider = ("std.zz_provider", "module std.zz_provider\npub type Widget = Real\n");
    assert_build_panics_naming(
        &[
            provider,
            (
                "std.zz_consumer",
                "module std.zz_consumer\nstructure def C {\n    param w : Widget\n}\n",
            ),
        ],
        &["std.zz_consumer", "Widget"],
    );

    let built = build_stdlib_modules(&[
        provider,
        (
            "std.zz_consumer",
            "module std.zz_consumer\nimport std.zz_provider\nstructure def C {\n    param w : Widget\n}\n",
        ),
    ]);
    assert_eq!(built.len(), 2, "the declared import must make the build succeed");
}

#[test]
fn a_stdlib_import_does_not_expose_the_imports_of_its_target() {
    assert_build_panics_naming(
        &[
            ("std.zz_a", "module std.zz_a\npub type Widget = Real\n"),
            (
                "std.zz_b",
                "module std.zz_b\nimport std.zz_a\nstructure def B {\n    param w : Widget\n}\n",
            ),
            (
                "std.zz_c",
                "module std.zz_c\nimport std.zz_b\nstructure def C {\n    param w : Widget\n}\n",
            ),
        ],
        &["std.zz_c", "Widget"],
    );
}

#[test]
fn deleting_one_declared_import_from_the_real_stdlib_fails_its_build() {
    const VICTIM: &str = "std.fdm.as_printed";
    let owned = stdlib_sources();
    let edited: Vec<(&str, String)> = owned
        .iter()
        .map(|(name, source)| {
            if *name != VICTIM {
                return (*name, source.clone());
            }
            let kept: Vec<&str> = source
                .split_inclusive('\n')
                .filter(|line| line.trim_end() != "import std.fdm")
                .collect();
            assert_eq!(
                kept.len() + 1,
                source.split_inclusive('\n').count(),
                "{VICTIM} must declare `import std.fdm` exactly once"
            );
            (*name, kept.concat())
        })
        .collect();
    let sources: Vec<(&str, &str)> = edited
        .iter()
        .map(|(name, source)| (*name, source.as_str()))
        .collect();
    assert_build_panics_naming(&sources, &[VICTIM, "FDMProcess"]);
}

#[test]
fn a_stdlib_module_declaring_the_wrong_module_path_fails_the_stdlib_build() {
    assert_build_panics_naming(
        &[("std.zz_right", "module std.zz_wrong\npub type T = Real\n")],
        &["E_MODULE_PATH_MISMATCH", "std.zz_right"],
    );
}

#[test]
fn a_stdlib_import_of_a_module_outside_the_set_fails_the_stdlib_build() {
    assert_build_panics_naming(
        &[(
            "std.zz_lonely",
            "module std.zz_lonely\nimport std.zz_absent\npub type T = Real\n",
        )],
        &["std.zz_absent", "std.zz_lonely"],
    );
}

// ─── boundary #8 (NS-M3): registration order is unobservable ───────────────

/// One module of a build, as far as NS-M3 is concerned. A build's fingerprint
/// is the Vec of these in OUTPUT order, so module order is part of it.
#[derive(Debug, PartialEq)]
struct ModuleFingerprint {
    path: String,
    content_hash: String,
    diagnostics: String,
}

fn fingerprint(modules: &[CompiledModule]) -> Vec<ModuleFingerprint> {
    modules
        .iter()
        .map(|module| ModuleFingerprint {
            path: module.path.to_dotted(),
            content_hash: format!("{:?}", module.content_hash),
            diagnostics: format!("{:?}", module.diagnostics),
        })
        .collect()
}

fn build_permuted(permute: impl FnOnce(&mut Vec<(&'static str, String)>)) -> Vec<CompiledModule> {
    let mut owned = stdlib_sources();
    permute(&mut owned);
    let sources: Vec<(&str, &str)> = owned
        .iter()
        .map(|(name, source)| (*name, source.as_str()))
        .collect();
    build_stdlib_modules(&sources)
}

fn reversed_build() -> &'static [CompiledModule] {
    static BUILT: OnceLock<Vec<CompiledModule>> = OnceLock::new();
    BUILT.get_or_init(|| build_permuted(|sources| sources.reverse()))
}

fn purposes_first_build() -> &'static [CompiledModule] {
    static BUILT: OnceLock<Vec<CompiledModule>> = OnceLock::new();
    BUILT.get_or_init(|| {
        build_permuted(|sources| {
            let at = sources
                .iter()
                .position(|(name, _)| *name == PURPOSES_MODULE)
                .unwrap_or_else(|| panic!("{PURPOSES_MODULE} is registered"));
            let purposes = sources.remove(at);
            sources.insert(0, purposes);
        })
    })
}

/// `None` when equal, else where the two fingerprints first part ways.
fn first_difference(a: &[ModuleFingerprint], b: &[ModuleFingerprint]) -> Option<String> {
    if let Some((index, (x, y))) = a.iter().zip(b).enumerate().find(|(_, (x, y))| x != y) {
        let what = if x.path != y.path {
            format!("module {} vs {}", x.path, y.path)
        } else if x.content_hash != y.content_hash {
            format!("content_hash of {}", x.path)
        } else {
            format!("diagnostics of {}: {} vs {}", x.path, x.diagnostics, y.diagnostics)
        };
        return Some(format!("index {index}: {what}"));
    }
    (a.len() != b.len()).then(|| format!("module count {} vs {}", a.len(), b.len()))
}

#[test]
fn stdlib_build_output_is_identical_under_any_registration_order() {
    let registered = fingerprint(load_stdlib());
    for (label, build) in [
        ("reversed", reversed_build()),
        ("purposes-first", purposes_first_build()),
    ] {
        if let Some(difference) = first_difference(&registered, &fingerprint(build)) {
            panic!("the {label} registration order changes the stdlib build at {difference}");
        }
    }
}

#[test]
fn std_determinacy_purposes_reaches_no_sibling_even_when_registered_first() {
    let built = purposes_first_build();
    let own_purposes: BTreeSet<&str> = built
        .iter()
        .find(|module| module.path.to_dotted() == PURPOSES_MODULE)
        .unwrap_or_else(|| panic!("{PURPOSES_MODULE} is built"))
        .compiled_purposes
        .iter()
        .filter(|purpose| purpose.is_pub && !purpose.declaration_span.is_prelude())
        .map(|purpose| purpose.name.as_str())
        .collect();
    assert!(
        !own_purposes.is_empty(),
        "{PURPOSES_MODULE} declares the standard purposes"
    );

    let leaks: Vec<String> = built
        .iter()
        .filter(|module| module.path.to_dotted() != PURPOSES_MODULE)
        .flat_map(|module| {
            module
                .compiled_purposes
                .iter()
                .filter(|purpose| {
                    own_purposes.contains(purpose.name.as_str())
                        || purpose.declaration_span.is_prelude()
                })
                .map(move |purpose| format!("{} carries {}", module.path.to_dotted(), purpose.name))
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "stdlib modules inherited purposes they do not import:\n{}",
        leaks.join("\n")
    );
}
