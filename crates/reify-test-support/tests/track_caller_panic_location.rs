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
//! This file guards two `reify-test-support` helpers against regressing
//! back to the closure form: `require_default_expr` (reached through the
//! public `get_let_expr_in_template`/`get_let_expr_in`/`get_let_expr`
//! wrappers) and `cell_value`. Each guard asserts the reported location plus
//! a SHORT payload substring — never full message prose, per the wording
//! rule `dimensioned_assertions.rs` already follows.

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

#[test]
fn require_default_expr_panic_reports_caller_not_helpers_rs() {
    let template = reify_test_support::TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", reify_core::Type::dimensionless_scalar())
        .build();

    let site = panic_site(|| {
        reify_test_support::get_let_expr_in_template(&template, "x");
    });

    // File identity alone discriminates the regression: helpers.rs is a
    // different file by construction, so "attributes to the caller, not
    // helpers.rs" is fully captured without pinning an exact line (which
    // `rustfmt --edition 2024` would otherwise be free to shift, per this
    // task's review). `site.line` is still surfaced below for diagnosis.
    assert_eq!(
        site.file.as_str(),
        file!(),
        "require_default_expr's panic must attribute to the CALLER (this \
         file), not to helpers.rs; reported {}:{}",
        site.file,
        site.line,
    );
    assert!(
        site.message.contains("has no default expr"),
        "panic message changed; got: {}",
        site.message,
    );
}

#[cfg(feature = "eval-helpers")]
#[test]
fn cell_value_panic_reports_caller_not_helpers_rs() {
    let result = reify_test_support::eval_source("structure S { let x = 1.0 }");

    let site = panic_site(|| {
        reify_test_support::cell_value(&result, "Nope", "missing");
    });

    // See `require_default_expr_panic_reports_caller_not_helpers_rs` above:
    // file identity is the contract; the exact line is diagnostic-only.
    assert_eq!(
        site.file.as_str(),
        file!(),
        "cell_value's panic must attribute to the CALLER (this file), not to \
         helpers.rs; reported {}:{}",
        site.file,
        site.line,
    );
    assert!(
        site.message.contains("not found in eval result"),
        "panic message changed; got: {}",
        site.message,
    );
}
