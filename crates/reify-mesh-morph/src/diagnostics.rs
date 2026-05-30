//! Session-level diagnostic counters + verbose-logging policy for the
//! mesh-morph engine (PRD `docs/prds/v0_3/mesh-morphing.md` task #11).
//!
//! This module is the standalone diagnostic-counter + failure-mode-logging
//! infrastructure: a set of process-global lock-free counters, per-outcome
//! recorder functions that couple a counter increment with its policy-level
//! `tracing` event, a `snapshot()` accessor for the downstream debug RPC, and
//! a `format_summary()` renderer for the `--verbose` exit line.
//!
//! Engine call-site wiring is deferred (see the `// G-allow:` markers on the
//! recorder functions); the events fire from the engine integration in
//! `reify-eval`'s `engine_build.rs` (PRD task #10).
