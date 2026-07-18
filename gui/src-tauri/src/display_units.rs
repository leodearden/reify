//! Thin re-export of the per-cell display-unit ladder registry.
//!
//! The registry (task #5199) relocated to `reify_core::display_units`
//! (task #5232) so it is reachable from reify-ir/reify-expr/reify-lsp, not
//! just the GUI. Re-exporting it here keeps
//! `reify_gui::display_units::{DimensionLadder, UnitOption, unit_ladders}`
//! resolving unchanged for `main.rs`'s `get_unit_ladders` Tauri command and
//! the Parameters panel's unit picker.

pub use reify_core::display_units::{AutoScale, DimensionLadder, UnitOption, unit_ladders};
