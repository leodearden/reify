//! Guard compilation tests.
//!
//! Tests that per-declaration where clauses, block guards, nested guards,
//! else blocks, and reference safety are compiled correctly into
//! CompiledGuardedGroup entries on TopologyTemplate.

use reify_compiler::*;
use reify_test_support::*;
use reify_types::*;
