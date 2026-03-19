//! Guard evaluation tests.
//!
//! Tests that guarded groups are correctly processed during evaluation:
//! guard-true includes members, guard-false includes else members,
//! undef guards leave members indeterminate, and guard changes
//! trigger schema re-elaboration.

use reify_compiler::*;
use reify_eval::*;
use reify_test_support::*;
use reify_types::*;
