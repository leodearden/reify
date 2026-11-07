//! Behavioural pin: `#[track_caller]` does not propagate into a closure
//! body. A `panic!` inside `opt.unwrap_or_else(|| panic!(..))` reports the
//! closure's own line, inside the helper — not the calling test's line —
//! because the closure is not itself `#[track_caller]`, so the `panic!`
//! macro's location is simply wherever it is lexically written. Verified on
//! rustc 1.98.1 `--edition 2024`: a two-level `#[track_caller]` chain
//! (`outer -> inner -> panic`) reports the helper's own line for the
//! closure form, and the caller's line for a `let-else` form with the
//! `panic!` directly in the `#[track_caller]` fn body.
//!
//! This file guards `reify-test-support`'s panicking helpers against
//! regressing back to the closure form, or dropping `#[track_caller]` from
//! any link of a delegation chain: `require_default_expr` (reached through
//! the public `get_let_expr_in_template`/`get_let_expr_in`/`get_let_expr`
//! wrappers), `cell_value`, and the value-cell lookup family's own panics
//! ("no template named", "no value cell named", "ambiguous cell name",
//! "expected at least one template"), each driven through the longest public
//! chain that reaches it. Each guard asserts the reported location plus a
//! SHORT payload substring — never full message prose, per the wording rule
//! `dimensioned_assertions.rs` already follows.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

/// Serialises the panic-hook swap in [`panic_site`]. The hook is
/// PROCESS-global while libtest runs this binary's tests as parallel
/// threads of ONE process; see `dimensioned_assertions.rs`'s `PANIC_HOOK`
/// for the exact interleave this guards against (an unserialised swap can
/// leave a silencer installed for the rest of the binary).
static PANIC_HOOK: Mutex<()> = Mutex::new(());

/// A captured panic: its payload message and the `Location` `#[track_caller]`
/// attributed it to.
struct PanicSite {
    message: String,
    file: String,
    line: u32,
}

/// Run `f`, which must panic, and capture the panic's payload and
/// `#[track_caller]` location. Restores the previous panic hook before
/// returning — on every path — so a genuine failure later in this binary is
/// never silenced.
fn panic_site(f: impl FnOnce()) -> PanicSite {
    // Poison-tolerant: the guard protects only the hook swap below, which is
    // restored on every path, so a poisoned lock guards no invalid state and
    // refusing on it would turn an unrelated failure into a cascade.
    let _serialised = PANIC_HOOK.lock().unwrap_or_else(|e| e.into_inner());

    // libtest runs this binary's tests as parallel threads of ONE process, so
    // the hook we install below — process-global — would otherwise also
    // capture (and silence) a panic raised by a sibling test on another
    // thread. Only record on this thread; forward everything else to the
    // hook that was previously installed, wrapped in an `Arc` so both the
    // temporary hook (which forwards) and the final restore (below) can
    // reach it.
    let calling_thread = std::thread::current().id();
    let location: Arc<Mutex<Option<(String, u32)>>> = Arc::new(Mutex::new(None));
    let location_hook = Arc::clone(&location);

    let previous = Arc::new(std::panic::take_hook());
    let previous_for_hook = Arc::clone(&previous);
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == calling_thread {
            if let Some(loc) = info.location() {
                *location_hook.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((loc.file().to_string(), loc.line()));
            }
        } else {
            previous_for_hook(info);
        }
    }));

    let outcome = catch_unwind(AssertUnwindSafe(f));

    std::panic::set_hook(Box::new(move |info| previous(info)));

    let payload = match outcome {
        Ok(()) => panic!("expected the captured call to panic, but it returned normally"),
        Err(payload) => payload,
    };
    let (file, line) = location
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .expect("panic hook did not record a location");

    PanicSite {
        message: reify_core::panic_payload_to_string(&*payload),
        file,
        line,
    }
}

/// Asserts `site` was attributed to the CALLER (this file), not to `helpers.rs`, and that
/// its payload contains `needle`. File identity alone discriminates the regression, so no
/// exact line is pinned (`rustfmt --edition 2024` is free to shift it); `site.line` is
/// surfaced for diagnosis only.
#[track_caller]
fn assert_attributed_here(site: &PanicSite, needle: &str) {
    assert_eq!(
        site.file.as_str(),
        file!(),
        "panic must attribute to the CALLER (this file), not to helpers.rs; reported {}:{}",
        site.file,
        site.line,
    );
    assert!(
        site.message.contains(needle),
        "panic message no longer contains {needle:?}; got: {}",
        site.message,
    );
}

#[test]
fn require_default_expr_panic_reports_caller_not_helpers_rs() {
    let template = reify_test_support::TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", reify_core::Type::dimensionless_scalar())
        .build();

    let site = panic_site(|| {
        reify_test_support::get_let_expr_in_template(&template, "x");
    });

    assert_attributed_here(&site, "has no default expr");
}

#[cfg(feature = "eval-helpers")]
#[test]
fn cell_value_panic_reports_caller_not_helpers_rs() {
    let result = reify_test_support::eval_source("structure S { let x = 1.0 }");

    let site = panic_site(|| {
        reify_test_support::cell_value(&result, "Nope", "missing");
    });

    assert_attributed_here(&site, "not found in eval result");
}

#[test]
fn get_value_cell_empty_module_panic_reports_caller_not_helpers_rs() {
    let module =
        reify_test_support::CompiledModuleBuilder::new(reify_core::ModulePath::single("empty"))
            .build();

    let site = panic_site(|| {
        reify_test_support::get_value_cell(&module, "x");
    });

    assert_attributed_here(&site, "expected at least one template");
}

#[test]
fn get_value_cell_missing_cell_panic_reports_caller_not_helpers_rs() {
    let module = reify_test_support::compile_source("structure S { let x = 1.0 }");

    let site = panic_site(|| {
        reify_test_support::get_value_cell(&module, "missing");
    });

    assert_attributed_here(&site, "no value cell named");
}

#[test]
fn get_value_cell_in_missing_template_panic_reports_caller_not_helpers_rs() {
    let module = reify_test_support::compile_source("structure S { let x = 1.0 }");

    let site = panic_site(|| {
        reify_test_support::get_value_cell_in(&module, "DoesNotExist", "x");
    });

    assert_attributed_here(&site, "no template named");
}

#[test]
fn get_let_expr_in_missing_cell_panic_reports_caller_not_helpers_rs() {
    let module = reify_test_support::compile_source("structure S { let x = 1.0 }");

    let site = panic_site(|| {
        reify_test_support::get_let_expr_in(&module, "S", "missing");
    });

    assert_attributed_here(&site, "no value cell named");
}

#[test]
fn get_let_expr_ambiguous_cell_panic_reports_caller_not_helpers_rs() {
    let template = reify_test_support::TopologyTemplateBuilder::new("Bracket")
        .auto_param("First", "x", reify_core::Type::dimensionless_scalar())
        .auto_param("Second", "x", reify_core::Type::dimensionless_scalar())
        .build();
    let module =
        reify_test_support::CompiledModuleBuilder::new(reify_core::ModulePath::single("test"))
            .template(template)
            .build();

    let site = panic_site(|| {
        reify_test_support::get_let_expr(&module, "x");
    });

    assert_attributed_here(&site, "ambiguous cell name");
}

#[test]
fn get_let_expr_empty_module_panic_reports_caller_not_helpers_rs() {
    let module =
        reify_test_support::CompiledModuleBuilder::new(reify_core::ModulePath::single("empty"))
            .build();

    let site = panic_site(|| {
        reify_test_support::get_let_expr(&module, "x");
    });

    assert_attributed_here(&site, "expected at least one template");
}
