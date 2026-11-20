//! Contract tests for [`reify_test_support::compile_source_with_prelude`]
//! (task #10208).
//!
//! The contract: the consumer is parsed and compiled exactly as
//! [`reify_test_support::compile_source`] does it (module `"test"`, panic on
//! parse errors, compile diagnostics returned rather than asserted away), but
//! against the given prelude slice, whose `pub` declarations the consumer sees.

use reify_test_support::{compile_source_named, compile_source_with_prelude, errors_only};

/// Exports one `pub` unit, so the consumer below resolves only through it.
const PRELUDE: &str = "pub unit mil : Length = 0.0000254";

/// Uses `mil`, which no built-in unit table supplies.
const CONSUMER: &str = "structure S { param w : Length = 5mil }";

fn units_prelude() -> reify_compiler::CompiledModule {
    compile_source_named(PRELUDE, "units_prelude")
}

#[test]
fn prelude_pub_unit_is_visible_to_the_consumer() {
    let module = compile_source_with_prelude(CONSUMER, &[units_prelude()]);

    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "the prelude's `pub unit mil` must resolve in the consumer; got: {errors:?}"
    );
}

#[test]
fn without_the_prelude_the_same_consumer_reports_a_compile_error_rather_than_panicking() {
    let module = compile_source_with_prelude(CONSUMER, &[]);

    assert!(
        !errors_only(&module).is_empty(),
        "with an empty prelude `mil` is unresolved, so the returned module must carry an error"
    );
}

#[test]
fn consumer_is_compiled_as_module_test() {
    let module = compile_source_with_prelude(CONSUMER, &[units_prelude()]);

    assert_eq!(module.path, reify_core::ModulePath::single("test"));
}
