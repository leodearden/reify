//! Mesh-morph runtime statistics — process-global accumulator.
//!
//! Exposes a `snapshot()` accessor and recorder functions
//! (`record_morph_attempt`, `record_remesh`, `record_rejection`).
//!
//! See: `docs/prds/v0_3/gui-event-channel-inventory.md` §2.3 (audit M-013).
//! RPC registration: `gui/src-tauri/src/debug_server.rs::handle_morph_stats`.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Mesh-morph runtime statistics DTO.
///
/// Response shape for the `morph_stats` debug-MCP RPC.
/// Per PRD §3.2 field names match exactly — no `#[serde(rename_all)]`.
/// `last_rejection_reason` is serialised `skip_serializing_if = "Option::is_none"`,
/// so it is absent from the JSON payload when no rejection has been recorded.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct MorphStats {
    pub morph_count: u32,
    pub remesh_count: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_rejection_reason: Option<String>,
}

#[derive(Default)]
struct StatsState {
    morph_count: u32,
    remesh_count: u32,
    last_rejection_reason: Option<String>,
}

static STATS: OnceLock<Mutex<StatsState>> = OnceLock::new();

fn state() -> &'static Mutex<StatsState> {
    STATS.get_or_init(|| Mutex::new(StatsState::default()))
}

/// Return a point-in-time snapshot of the process-global morph stats.
pub fn snapshot() -> MorphStats {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    MorphStats {
        morph_count: guard.morph_count,
        remesh_count: guard.remesh_count,
        last_rejection_reason: guard.last_rejection_reason.clone(),
    }
}

/// Increment the morph-attempt counter.
///
/// To be wired into the engine call site by mesh-morphing PRD tasks #2947-#2949;
/// until then, only test code calls this. `saturating_add` ensures no panic on
/// `u32::MAX` overflow (production stability concern).
// G-allow: mesh-morph engine call-site wiring deferred to tasks #2947-#2949
pub fn record_morph_attempt() {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.morph_count = guard.morph_count.saturating_add(1);
}

/// Increment the remesh-fallback counter.
///
/// `saturating_add` ensures no panic on `u32::MAX` overflow.
// G-allow: mesh-morph engine call-site wiring deferred to tasks #2947-#2949
pub fn record_remesh() {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.remesh_count = guard.remesh_count.saturating_add(1);
}

/// Record the most-recent rejection reason. Overwrites prior value (latest-wins).
// G-allow: mesh-morph engine call-site wiring deferred to tasks #2947-#2949
pub fn record_rejection(reason: impl Into<String>) {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.last_rejection_reason = Some(reason.into());
}

/// Reset state to defaults.
///
/// Available in same-crate `#[cfg(test)]` context, and also when the crate is
/// compiled with `features = ["testing"]` — enabling cross-crate test isolation
/// (e.g. from `reify-gui`'s `[dev-dependencies]`) once engine wiring lands and
/// recorders are called from production code paths (PRD #2947-#2949).
#[cfg(any(test, feature = "testing"))]
pub fn reset_for_test() {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    *guard = StatsState::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize parallel test access to the process-global stats.
    /// Each test acquires this before resetting state so tests don't
    /// interfere with each other regardless of execution order.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Run `f` under `TEST_LOCK` with a fresh stats state.
    fn with_locked_stats<F: FnOnce()>(f: F) {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_for_test();
        f();
    }

    #[test]
    fn snapshot_returns_zeros_and_none_by_default() {
        with_locked_stats(|| {
            let s = snapshot();
            assert_eq!(s.morph_count, 0, "morph_count should be 0 by default");
            assert_eq!(s.remesh_count, 0, "remesh_count should be 0 by default");
            assert!(
                s.last_rejection_reason.is_none(),
                "last_rejection_reason should be None by default"
            );
        });
    }

    #[test]
    fn morph_stats_serde_roundtrip() {
        let original = MorphStats {
            morph_count: 7,
            remesh_count: 3,
            last_rejection_reason: Some("Ineligible(StructuralChange)".into()),
        };

        let json_val = serde_json::to_value(&original).expect("serialize must succeed");

        // PRD §3.2: field names match exactly — no #[serde(rename_all)]
        assert_eq!(
            json_val["morph_count"].as_u64(),
            Some(7),
            "morph_count key must be present"
        );
        assert_eq!(
            json_val["remesh_count"].as_u64(),
            Some(3),
            "remesh_count key must be present"
        );
        assert_eq!(
            json_val["last_rejection_reason"].as_str(),
            Some("Ineligible(StructuralChange)"),
            "last_rejection_reason key must be present"
        );

        let roundtripped: MorphStats =
            serde_json::from_value(json_val).expect("deserialize must succeed");
        assert_eq!(original, roundtripped, "roundtrip must be identity");

        // Verify `#[serde(skip_serializing_if = "Option::is_none")]` contract:
        // when last_rejection_reason is None it must be absent from the JSON (no null key).
        let default_val = serde_json::to_value(&MorphStats::default())
            .expect("serialize default must succeed");
        assert!(
            default_val.get("last_rejection_reason").is_none(),
            "last_rejection_reason must be absent from JSON when None (skip_serializing_if); \
             got: {:?}",
            default_val.get("last_rejection_reason")
        );

        // Deserializing a payload that omits last_rejection_reason must yield None.
        let no_reason_json = serde_json::json!({
            "morph_count": 0u32,
            "remesh_count": 0u32
        });
        let deserialized: MorphStats =
            serde_json::from_value(no_reason_json).expect("deserialize without key must succeed");
        assert!(
            deserialized.last_rejection_reason.is_none(),
            "last_rejection_reason must be None when key is absent from payload"
        );
    }

    #[test]
    fn record_morph_attempt_increments_morph_count() {
        with_locked_stats(|| {
            record_morph_attempt();
            record_morph_attempt();
            record_morph_attempt();
            assert_eq!(snapshot().morph_count, 3, "morph_count should be 3 after 3 calls");
        });
    }

    #[test]
    fn record_remesh_increments_remesh_count() {
        with_locked_stats(|| {
            record_remesh();
            record_remesh();
            let s = snapshot();
            assert_eq!(s.remesh_count, 2, "remesh_count should be 2 after 2 calls");
            assert_eq!(s.morph_count, 0, "morph_count must be unaffected by record_remesh");
        });
    }

    #[test]
    fn record_rejection_sets_last_reason_and_overwrites() {
        with_locked_stats(|| {
            record_rejection("first");
            record_rejection("second");
            assert_eq!(
                snapshot().last_rejection_reason,
                Some("second".into()),
                "last_rejection_reason should be 'second' (overwrite semantics — latest wins)"
            );
        });
    }
}
