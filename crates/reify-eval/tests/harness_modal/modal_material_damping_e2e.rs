//! Author-surface end-to-end gate for `MaterialDamping` on the FEA
//! `modal_analysis` path (task #6878, PRD leaf β).
//!
//! Content lands in step-7 (B4/B5/B7) and step-9 (B8). This module exists ahead of
//! them so the new `harness_modal` root and its drift-guard standing are proven
//! before any test content depends on them.
