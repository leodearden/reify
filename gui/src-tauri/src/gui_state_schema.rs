//! Schema-derived per-field GUI state sync (PRD `docs/prds/v0_6/gui-state-sync.md`
//! §8 L5, INV-GUI-1 real fix).
//!
//! Houses the `gui_state!` macro (added in a later step) that generates
//! `GuiState`, `StateDelta`, `StateDelta::full`, a diff function, and an
//! events function from a single schema definition — a field written
//! without a leading sync classification token matches no muncher arm and
//! is therefore a compile error, making sync-drift unrepresentable.
//!
//! This module is scaffolded ahead of the macro body so downstream steps
//! have a stable `$crate::gui_state_schema::...` path to target.
