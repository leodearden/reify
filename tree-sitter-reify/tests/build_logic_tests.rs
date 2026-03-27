//! Tests for build.rs pure logic functions.
//!
//! Since build.rs is compiled as a standalone build script by cargo,
//! its functions cannot be imported by test targets. This file
//! re-implements the pure logic (content hashing, staleness detection,
//! output verification) to validate correctness.

use std::path::Path;
