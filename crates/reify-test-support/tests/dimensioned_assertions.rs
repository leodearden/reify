//! Behavioural pins for `reify_test_support::assert_dimensioned`, the canonical
//! dimensioned-`Value::Scalar` assertion hoisted by task #6323 (part A).
//!
//! The helper existed as two near-verbatim copies whose docstrings ARGUED for
//! the four properties below and pinned none of them. Since the helper's whole
//! job is to fail where an f64-folding reader would pass, "it panics on a bare
//! `Value::Real`" is precisely the claim that has to be executable — a copy
//! that quietly stopped panicking would make every call site vacuous.
//!
//! Asserts on panic-vs-not and a short substring of the payload naming the
//! OBSERVED variant; never on full message prose.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use reify_core::dimension::{DimensionVector, FORCE};
use reify_ir::Value;
use reify_test_support::{assert_dimensioned, mm, newton};

/// Serialises the panic-hook swap in [`panic_message`].
///
/// The hook is PROCESS-global while libtest runs these tests as parallel
/// threads of ONE process, so two unserialised take/set pairs interleave:
/// `A:take` (gets the default, installs its silencer) → `B:take` (gets A's
/// silencer) → `A:set(default)` → `B:set(A's silencer)` leaves a silencer
/// installed for the remainder of the binary, and a GENUINE failure in any
/// later test then reports no payload at all. The mirror interleave restores
/// the default early and prints the four deliberate panics as noise. Either
/// way it degrades exactly the diagnosability these pins exist to provide.
static PANIC_HOOK: Mutex<()> = Mutex::new(());

/// Capture a panic payload as a `String`, or `None` if the closure returned.
fn panic_message(f: impl FnOnce()) -> Option<String> {
    // Poison-tolerant: the guard protects only the swap below, which is
    // restored on every path, so a poisoned lock guards no invalid state and
    // refusing on it would turn an unrelated failure into a cascade.
    let _serialised = PANIC_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(previous);
    match outcome {
        Ok(()) => None,
        Err(payload) => Some(
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "<non-string panic payload>".to_string()),
        ),
    }
}

/// ACCEPTS a matching dimensioned `Value::Scalar`. Built through the existing
/// `values` constructors rather than hand-assembled, so this arm also pins that
/// the family and the assertion agree on the same representation.
#[test]
fn accepts_a_matching_dimensioned_scalar() {
    assert_dimensioned(&newton(5.0), 5.0, FORCE, "newton(5.0)");
    // `mm` converts to SI at construction, so the expected magnitude is metres.
    assert_dimensioned(&mm(3.0), 0.003, DimensionVector::LENGTH, "mm(3.0)");
}

/// PANICS on a bare `Value::Real` carrying the RIGHT magnitude. This is the
/// whole reason the helper exists: an f64-folding reader (`read_real`, `num()`)
/// passes identically before and after a dimensioned-ctor migration and is
/// blind to the property under test.
#[test]
fn panics_on_a_bare_real_even_with_the_right_magnitude() {
    let message = panic_message(|| {
        assert_dimensioned(&Value::Real(5.0), 5.0, FORCE, "probe");
    })
    .expect("a bare Value::Real must NOT be accepted as a dimensioned Scalar");
    assert!(
        message.contains("Real"),
        "the payload must name the OBSERVED variant so an un-migrated ctor arg \
         reads clearly rather than as a generic match failure; got: {message}"
    );
}

/// PANICS on the right `si_value` under the WRONG `DimensionVector` — a
/// wrong-dimension misparse must fail as loudly as a wrong magnitude.
#[test]
fn panics_on_a_wrong_dimension() {
    let message = panic_message(|| {
        assert_dimensioned(&newton(5.0), 5.0, DimensionVector::MASS, "probe");
    })
    .expect("a Scalar carrying the wrong dimension must not be accepted");
    assert!(
        message.contains("dimension"),
        "the payload must say which half failed; got: {message}"
    );
}

/// PANICS on the right dimension with a wrong `si_value`, ONE ULP away — the
/// magnitude comparison is EXACT, with no tolerance. Deliberate: these values
/// are literal-derived (a unit literal converted to SI at parse time), not
/// solver-derived, so there is no float jitter to tolerate and a tolerance band
/// would hide an inert-migration regression.
#[test]
fn panics_on_a_one_ulp_magnitude_difference() {
    let expected = 5.0_f64;
    let off_by_one_ulp = f64::from_bits(expected.to_bits() + 1);
    assert_ne!(expected, off_by_one_ulp, "the probe must actually differ");

    let message = panic_message(|| {
        assert_dimensioned(&newton(off_by_one_ulp), expected, FORCE, "probe");
    })
    .expect("the si_value comparison must be EXACT — one ULP must still panic");
    assert!(
        message.contains("magnitude"),
        "the payload must say which half failed; got: {message}"
    );
}

/// PANICS on a `Value` that is neither `Scalar` nor `Real` — the catch-all arm.
#[test]
fn panics_on_an_unrelated_value_variant() {
    let message = panic_message(|| {
        assert_dimensioned(&Value::Bool(true), 5.0, FORCE, "probe");
    })
    .expect("a non-numeric Value must not be accepted");
    assert!(
        message.contains("Bool"),
        "the catch-all payload must name the observed variant; got: {message}"
    );
}
