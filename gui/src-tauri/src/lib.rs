// `Value` (reify-types) contains a `SampledField` variant whose `AtomicBool`
// gives it interior mutability, so any `BTreeMap<Value, Value>` use trips
// clippy's `mutable_key_type` lint. Engine code holds several such maps; the
// hash/order is computed from the non-mutable discriminants, never from the
// `AtomicBool`, so this is safe in practice. Mirrors the same crate-wide
// allow used by every other workspace crate that holds `BTreeMap<Value, _>`
// (reify-types, reify-eval, reify-expr, reify-stdlib, reify-lsp,
// reify-compiler, reify-constraints, reify-test-support).
#![allow(clippy::mutable_key_type)]

pub mod claude_bridge;
pub mod commands;
pub mod path_key;
#[cfg(feature = "gui")]
pub mod debug;
#[cfg(feature = "gui")]
pub mod debug_server;
#[cfg(feature = "gui")]
pub mod event_bus;
pub mod diff;
pub mod engine;
pub mod engine_lock;
pub mod kernel_status;
pub mod lsp_bridge;
pub mod mcp_context;
pub mod types;
pub mod watcher;

#[cfg(test)]
mod tests;
