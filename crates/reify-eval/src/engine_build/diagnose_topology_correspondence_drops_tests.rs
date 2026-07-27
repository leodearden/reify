    use reify_core::{Diagnostic, DiagnosticCode, Severity};
    use reify_ir::{
        AttributeHistory, BooleanOpHistoryRecords, LocalFeatureOpHistoryRecords,
        LoftOpHistoryRecords, SweepOpHistoryRecords,
    };

    use super::diagnose_topology_correspondence_drops;

    /// Helper: call the helper and return the collected diagnostics.
    fn run(history: &AttributeHistory) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        diagnose_topology_correspondence_drops(history, "test-context", &mut diags);
        diags
    }

    /// Boolean silent_drop_count > 0 → exactly one Warning with
    /// TopologyCorrespondenceDropped and the count in the message.
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn boolean_silent_drop_emits_one_warning() {
        let history = AttributeHistory::Boolean(BooleanOpHistoryRecords {
            silent_drop_count: 3,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one diagnostic; got: {diags:?}"
        );
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        assert!(
            d.message.contains("silent_drop_count=3"),
            "message should contain 'silent_drop_count=3'; got: {:?}",
            d.message
        );
        assert!(
            d.message.to_lowercase().contains("bool")
                || d.message.to_lowercase().contains("boolean"),
            "message should name the op kind; got: {:?}",
            d.message
        );
    }

    /// Boolean silent_drop_count == 0 → no diagnostics.
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn boolean_silent_drop_zero_emits_nothing() {
        let history = AttributeHistory::Boolean(BooleanOpHistoryRecords {
            silent_drop_count: 0,
            ..Default::default()
        });
        let diags = run(&history);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for zero count; got: {diags:?}"
        );
    }

    /// Extrude with all three non-zero SweepOpHistoryRecords counters →
    /// exactly three Warnings, each with the code and the respective count.
    /// Also verifies the op_kind label is "extrude" and that each message
    /// pins the counter name alongside the count (not just a bare digit).
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn extrude_three_nonzero_counters_emits_three_warnings() {
        let history = AttributeHistory::Extrude(SweepOpHistoryRecords {
            silent_drop_count: 1,
            unsynthesized_profile_edge_count: 2,
            duplicate_parent_subshape_index_count: 4,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(diags.len(), 3, "expected 3 diagnostics; got: {diags:?}");
        for d in &diags {
            assert_eq!(d.severity, Severity::Warning);
            assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        // Op-kind label must be present.
        assert!(
            messages.iter().any(|m| m.contains("extrude")),
            "op_kind 'extrude' not found in any message; messages: {messages:?}"
        );
        // Each counter must be reported as `counter_name=count` — not just a
        // bare digit — so the association between name and value is pinned.
        assert!(
            messages.iter().any(|m| m.contains("silent_drop_count=1")),
            "silent_drop_count=1 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unsynthesized_profile_edge_count=2")),
            "unsynthesized_profile_edge_count=2 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("duplicate_parent_subshape_index_count=4")),
            "duplicate_parent_subshape_index_count=4 not found in any message; messages: {messages:?}"
        );
    }

    /// Revolve with all three non-zero SweepOpHistoryRecords counters →
    /// exactly three Warnings with op_kind "revolve" and counter_name=count
    /// tokens in the messages.
    #[test]
    fn revolve_three_nonzero_counters_emits_three_warnings() {
        let history = AttributeHistory::Revolve(SweepOpHistoryRecords {
            silent_drop_count: 1,
            unsynthesized_profile_edge_count: 2,
            duplicate_parent_subshape_index_count: 4,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(diags.len(), 3, "expected 3 diagnostics; got: {diags:?}");
        for d in &diags {
            assert_eq!(d.severity, Severity::Warning);
            assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("revolve")),
            "op_kind 'revolve' not found in any message; messages: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("silent_drop_count=1")),
            "silent_drop_count=1 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unsynthesized_profile_edge_count=2")),
            "unsynthesized_profile_edge_count=2 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("duplicate_parent_subshape_index_count=4")),
            "duplicate_parent_subshape_index_count=4 not found in any message; messages: {messages:?}"
        );
    }

    /// Sweep with all three non-zero SweepOpHistoryRecords counters →
    /// exactly three Warnings with op_kind "sweep" and counter_name=count
    /// tokens in the messages.
    #[test]
    fn sweep_three_nonzero_counters_emits_three_warnings() {
        let history = AttributeHistory::Sweep(SweepOpHistoryRecords {
            silent_drop_count: 1,
            unsynthesized_profile_edge_count: 2,
            duplicate_parent_subshape_index_count: 4,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(diags.len(), 3, "expected 3 diagnostics; got: {diags:?}");
        for d in &diags {
            assert_eq!(d.severity, Severity::Warning);
            assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("sweep")),
            "op_kind 'sweep' not found in any message; messages: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("silent_drop_count=1")),
            "silent_drop_count=1 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unsynthesized_profile_edge_count=2")),
            "unsynthesized_profile_edge_count=2 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("duplicate_parent_subshape_index_count=4")),
            "duplicate_parent_subshape_index_count=4 not found in any message; messages: {messages:?}"
        );
    }

    /// LocalFeature silent_drop_count > 0 → exactly one Warning with the code
    /// and count 5.
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn local_feature_silent_drop_emits_one_warning() {
        let history = AttributeHistory::LocalFeature(LocalFeatureOpHistoryRecords {
            silent_drop_count: 5,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one diagnostic; got: {diags:?}"
        );
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        assert!(
            d.message.contains("silent_drop_count=5"),
            "message should contain 'silent_drop_count=5'; got: {:?}",
            d.message
        );
    }

    /// Loft → no diagnostics (LoftOpHistoryRecords has no counters by design).
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn loft_emits_nothing() {
        let history = AttributeHistory::Loft(LoftOpHistoryRecords::default());
        let diags = run(&history);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for Loft; got: {diags:?}"
        );
    }

    /// AttributeHistory::None → no diagnostics (zero-cost no-op).
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn none_emits_nothing() {
        let history = AttributeHistory::None;
        let diags = run(&history);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for None; got: {diags:?}"
        );
    }
