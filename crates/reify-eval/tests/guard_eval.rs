//! Guard evaluation tests.
//!
//! Tests for evaluating guarded groups: conditional member activation,
//! else branches, undef guards, and schema re-elaboration.

use reify_eval::*;
use reify_test_support::*;
use reify_types::*;
