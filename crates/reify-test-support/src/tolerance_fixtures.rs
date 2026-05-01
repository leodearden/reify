//! Shared fixture builders for tolerance integration tests.
//!
//! Houses the template/purpose builders used by `tolerance_combine`,
//! `tolerance_import_promise`, and future tolerance integration tests. The
//! recognition shapes these fixtures produce must stay byte-identical across
//! all test files — centralising them here ensures a single source of truth
//! and lets co-located unit tests pin each shape explicitly.
