//! Mesh-morph runtime statistics — process-global accumulator.
//!
//! Exposes a `snapshot()` accessor and recorder functions
//! (`record_morph_attempt`, `record_remesh`, `record_rejection`).
//!
//! See: `docs/prds/v0_3/gui-event-channel-inventory.md` §2.3 (audit M-013).
//! RPC registration: `gui/src-tauri/src/debug_server.rs::handle_morph_stats`.

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
    }
}
