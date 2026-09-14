//! `chain` default-port inference tests (spec §6.2).
//!
//! A bare chain element names an occurrence/structure sub rather than a port, and
//! each hop is desugared to that sub's unique port in the direction the hop needs.
//! Lives apart from `connect_compile_tests.rs`, which already carries the explicit
//! `connect` surface at 2368 lines.

use reify_core::*;
use reify_test_support::{
    assert_has_diagnostic, assert_no_diagnostic, compile_first_template, compile_source,
};
