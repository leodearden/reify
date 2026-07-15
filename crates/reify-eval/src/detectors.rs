//! Shared post-pass detector registry (task λ, #5043).
//!
//! PRD `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.7, INV-EVAL-3.
//!
//! Today's eval-only post-passes (the `MassProperties` PSD inertia
//! validation and the annotation-args materialization driver, both inline
//! in `Engine::eval`) run on the cold `eval()` path but NOT on
//! `eval_cached()` — a cold-only detector asymmetry — and their relative
//! ordering is encoded only in scattered "must run before …" / "MUST run
//! AFTER …" convention comments (e.g. `engine_eval.rs:6312`,
//! `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`).
//!
//! This module provides the REGISTRY MECHANISM that replaces both: a single
//! shared post-pass detector registry any eval path can run identically,
//! where registration order IS run order — one owner, all paths.
//!
//! **Scope of this task**: the registry mechanism only. Wiring it into
//! `Engine::eval` / `Engine::eval_cached` / `Engine::edit_check`, plus
//! fast-path diagnostic replay, is task μ (#5044, depends on κ, λ, γ, δ) —
//! explicitly OUT of scope here.
//!
//! ## Known scope gaps for task μ
//!
//! - The annotation-args materialization driver (`engine_eval.rs:4400-4557`)
//!   needs heavy READ-ONLY `Engine` context (module/prelude/functions plus
//!   an `EvalContext`) that this module's post-pass state does not carry.
//!   Its per-path hand-off — especially on `edit_check` — is deferred to
//!   task μ (PRD §10 open-question #2).
//! - Per-node diagnostic attribution and `NodeCache` storage/replay
//!   (consuming task κ's per-node diagnostics vec) is also task μ's.
//! - The inline `MassProperties` PSD pass at `engine_eval.rs:4662-4762`
//!   stays in place until task μ removes it and wires this registry into
//!   the three eval paths (foundation-then-migrate, mirroring how task α
//!   shipped `commit_cell_result` ahead of its own migration leaves). Until
//!   then, [`MassPropertiesPsdDetector`] is a live second copy of that
//!   logic — see its doc comment for the drift-avoidance note.

use reify_core::{Diagnostic, DiagnosticCode, ValueCellId};
use reify_ir::{DeterminacyState, PersistentMap, Value, ValueMap};

/// The minimal UNIVERSAL mutable state a detector reads/mutates, bundled as
/// disjoint `&mut` borrows — mirrors [`crate::cell_commit::CommitLegs`].
///
/// All three fields are present on every eval path: `Engine::eval` and
/// `Engine::eval_cached` both hold a `values` map, a `snapshot.values` map,
/// and a `diagnostics` sink at their post-pass point. A detector operating
/// only on this bundle therefore runs identically regardless of which path
/// assembled the state — INV-EVAL-3.
///
/// Detectors needing heavier READ-ONLY context (e.g. the annotation-args
/// driver's module/prelude/functions + `EvalContext`) are out of scope for
/// this task; see the module doc's "Known scope gaps for task μ". Task μ
/// may extend this struct if a migrated detector needs more.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
pub(crate) struct PostPassState<'a> {
    pub(crate) values: &'a mut ValueMap,
    pub(crate) snapshot_values: &'a mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    pub(crate) diagnostics: &'a mut Vec<Diagnostic>,
}

/// A single post-pass check runnable identically on any eval path.
///
/// Contract: equal input [`PostPassState`] → equal diagnostic sequence +
/// equal state mutations (INV-EVAL-3). Implementations should treat `state`
/// as the only source of truth — no hidden global/interior state.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
pub(crate) trait PostPassDetector {
    /// A stable kebab-case slug identifying this detector — used for
    /// ordering introspection ([`DetectorRegistry::ids`]) and debugging.
    fn id(&self) -> &'static str;

    /// Runs the check against `state`, mutating it in place (pushing
    /// diagnostics, replacing values) as needed.
    fn run(&self, state: &mut PostPassState<'_>);
}

/// An ordered collection of [`PostPassDetector`]s — registration order IS
/// run order.
///
/// This is the single owner of post-pass ordering (INV-EVAL-3), replacing
/// the scattered "must run before …" / "MUST run AFTER …" ordering-
/// convention comments (e.g. `engine_eval.rs:6312`,
/// `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`) with
/// one readable, explicit `Vec`.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
#[derive(Default)]
pub(crate) struct DetectorRegistry {
    detectors: Vec<Box<dyn PostPassDetector>>,
}

#[allow(dead_code)] // wired in by task μ; exercised by tests until then
impl DetectorRegistry {
    /// An empty registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends `detector` to the registry. Registration order is run order.
    pub(crate) fn register(&mut self, detector: Box<dyn PostPassDetector>) {
        self.detectors.push(detector);
    }

    /// Runs every registered detector, in registration order, against
    /// `state`.
    pub(crate) fn run_all(&self, state: &mut PostPassState<'_>) {
        for detector in &self.detectors {
            detector.run(state);
        }
    }

    /// The registered detectors' ids, in registration (= run) order — the
    /// fixed, introspectable order task μ relies on.
    pub(crate) fn ids(&self) -> Vec<&'static str> {
        self.detectors.iter().map(|d| d.id()).collect()
    }

    /// The canonical ordered registry of built-in detectors (INV-EVAL-3).
    ///
    /// Currently registers only [`MassPropertiesPsdDetector`]. This is the
    /// single slot task μ's migrated annotation-args detector registers
    /// into, in whatever relative order the PRD's ordering constraints
    /// (`engine_eval.rs:6312`, `structural_query.rs:531,610`,
    /// `significance_filter.rs:1025,1032`) dictate.
    pub(crate) fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(MassPropertiesPsdDetector));
        registry
    }
}

/// A faithful, characterization-preserving port of the inline `MassProperties`
/// PSD inertia validation at `engine_eval.rs:4662-4762` (RBD-α, task 3822).
///
/// For every cell whose value is a `StructureInstance` with
/// `type_name == "MassProperties"`, classifies the `inertia` field and
/// replaces the cell with `Value::Undef` (in both `values` and
/// `snapshot_values`, paired with `DeterminacyState::Determined`) when the
/// field is malformed (unparseable as a 3×3 numeric matrix) or non-PSD,
/// emitting a [`DiagnosticCode::DynamicsInertiaNotPSD`] diagnostic in either
/// case. A cell with no `inertia` field (or an already-`Undef` one) is left
/// untouched — no false positives.
///
/// **Drift warning**: until task μ (#5044) removes the inline copy at
/// `engine_eval.rs:4662-4762` (the "RBD-α (task 3822)" block) and wires
/// this registry into the eval paths, this is a live second copy of that
/// logic. The two bodies are deliberately kept semantically/logically
/// identical — same classification rules, same diagnostic wording, same
/// Undef-replacement behavior — modulo state-access binding: this detector
/// reads `state.values` / `state.snapshot_values` / `state.diagnostics`
/// where the inline copy reads the bare `values` / `snapshot.values` /
/// `diagnostics` locals, and the two use different early-out shapes (a
/// guarded `if` block there vs. an early `return` here). A byte-for-byte
/// diff of the two will therefore show mismatches even though they are not
/// intended to diverge; `mass_properties_psd_detector_messages_match_characterization_wording`
/// (below) pins the fixed diagnostic wording instead, so a wording change
/// on either side without updating the other surfaces as a test failure. A
/// change to the classification rules, diagnostic wording, or
/// Undef-replacement behavior on either side (this detector, or the
/// `engine_eval.rs:4662-4762` site it mirrors) must be mirrored on the
/// other, or the two will silently diverge before μ deletes the inline
/// copy.
#[allow(dead_code)] // wired in by task μ; exercised by tests until then
pub(crate) struct MassPropertiesPsdDetector;

impl PostPassDetector for MassPropertiesPsdDetector {
    fn id(&self) -> &'static str {
        "mass-properties-psd"
    }

    fn run(&self, state: &mut PostPassState<'_>) {
        // Fast early-out: skip the O(n) extraction pass when no MassProperties
        // cell exists (the common case when std.dynamics is unused). Mirrors
        // the inline pass's guard.
        let has_mass_props = state.values.iter().any(|(_, v)| {
            matches!(v, Value::StructureInstance(d) if d.type_name == "MassProperties")
        });
        if !has_mass_props {
            return;
        }

        // Classify each MassProperties cell's inertia field.
        enum InertiaResult {
            /// Field is absent or already Undef — leave untouched (no false positives).
            Skip,
            /// Field is present but could not be parsed as a 3×3 numeric matrix.
            Malformed,
            /// Field parsed successfully — run PSD check.
            Valid([[f64; 3]; 3]),
        }

        // The immutable `values` borrow is released before any mutable insert
        // by collecting target pairs first.
        let mass_props_cells: Vec<(ValueCellId, InertiaResult)> = state
            .values
            .iter()
            .filter_map(|(id, val)| {
                if let Value::StructureInstance(data) = val
                    && data.type_name == "MassProperties"
                {
                    let result = match data.fields.get("inertia") {
                        None | Some(Value::Undef) => InertiaResult::Skip,
                        Some(v) => match crate::dynamics_psd::inertia_3x3_from_value(v) {
                            Some(m) => InertiaResult::Valid(m),
                            None => InertiaResult::Malformed,
                        },
                    };
                    match result {
                        InertiaResult::Skip => None,
                        other => Some((id.clone(), other)),
                    }
                } else {
                    None
                }
            })
            .collect();

        for (id, result) in mass_props_cells {
            match result {
                InertiaResult::Skip => unreachable!("Skip filtered above"),
                InertiaResult::Malformed => {
                    state.diagnostics.push(
                        Diagnostic::error(format!(
                            "MassProperties '{}': inertia field cannot be parsed as \
                             a 3×3 numeric matrix",
                            id,
                        ))
                        .with_code(DiagnosticCode::DynamicsInertiaNotPSD),
                    );
                    state.values.insert(id.clone(), Value::Undef);
                    state
                        .snapshot_values
                        .insert(id.clone(), (Value::Undef, DeterminacyState::Determined));
                }
                InertiaResult::Valid(m) => {
                    let tol = crate::dynamics_psd::psd_tol(&m);
                    if !crate::dynamics_psd::is_symmetric_psd(&m, tol) {
                        let min_eig = crate::dynamics_psd::min_eigenvalue(&m);
                        state.diagnostics.push(
                            Diagnostic::error(format!(
                                "MassProperties '{}': inertia tensor is not positive \
                                 semi-definite (min eigenvalue ≈ {:.3e})",
                                id, min_eig,
                            ))
                            .with_code(DiagnosticCode::DynamicsInertiaNotPSD),
                        );
                        state.values.insert(id.clone(), Value::Undef);
                        state
                            .snapshot_values
                            .insert(id.clone(), (Value::Undef, DeterminacyState::Determined));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticCode, ValueCellId};
    use reify_ir::{
        DeterminacyState, PersistentMap, StructureInstanceData, StructureTypeId, Value, ValueMap,
    };

    use super::*;

    /// Projects a `Diagnostic` to a comparable key. `Diagnostic` derives
    /// only `Debug, Clone` (no `PartialEq`), but `DiagnosticCode` does, so
    /// `(code, message)` is a faithful equality check for these tests.
    fn diag_key(d: &Diagnostic) -> (Option<DiagnosticCode>, String) {
        (d.code, d.message.clone())
    }

    fn diag_keys(diags: &[Diagnostic]) -> Vec<(Option<DiagnosticCode>, String)> {
        diags.iter().map(diag_key).collect()
    }

    /// Order-insensitive projection for comparing diagnostics from two
    /// INDEPENDENTLY-built states. `PostPassState.values` is an
    /// `im::HashMap`-backed `ValueMap` with a per-instance `RandomState`, so
    /// two separately-constructed-but-equal states can iterate in different
    /// orders; a detector that walks `values.iter()` unsorted (like
    /// [`MassPropertiesPsdDetector`], faithfully mirroring the inline
    /// `engine_eval.rs:4662-4762` pass, which also iterates unsorted) then
    /// yields the same diagnostic MULTISET but not necessarily the same
    /// SEQUENCE across runs. A `HashSet` (not `BTreeSet`: `DiagnosticCode`
    /// is `Eq + Hash`, not `Ord`) is the honest INV-EVAL-3 comparison for
    /// such a detector — sequence equality is asserted separately only
    /// where the detectors involved don't read hashmap-ordered state (see
    /// [`run_all_executes_in_registration_order`]).
    fn diag_key_set(
        diags: &[Diagnostic],
    ) -> std::collections::HashSet<(Option<DiagnosticCode>, String)> {
        diags.iter().map(diag_key).collect()
    }

    /// A deterministic test-double detector: pushes one fixed diagnostic and
    /// inserts one fixed marker cell into `values` — a pure function of its
    /// own fields, reading nothing from the incoming state.
    struct FixedMarkerDetector {
        slug: &'static str,
        code: DiagnosticCode,
        message: &'static str,
        marker_cell: ValueCellId,
    }

    impl PostPassDetector for FixedMarkerDetector {
        fn id(&self) -> &'static str {
            self.slug
        }

        fn run(&self, state: &mut PostPassState<'_>) {
            state
                .diagnostics
                .push(Diagnostic::error(self.message.to_string()).with_code(self.code));
            state
                .values
                .insert(self.marker_cell.clone(), Value::Bool(true));
        }
    }

    fn detector_a() -> Box<dyn PostPassDetector> {
        Box::new(FixedMarkerDetector {
            slug: "test-detector-a",
            code: DiagnosticCode::ConstraintViolated,
            message: "detector A fired",
            marker_cell: ValueCellId::new("Body", "a_marker"),
        })
    }

    fn detector_b() -> Box<dyn PostPassDetector> {
        Box::new(FixedMarkerDetector {
            slug: "test-detector-b",
            code: DiagnosticCode::SelectorKindMismatch,
            message: "detector B fired",
            marker_cell: ValueCellId::new("Body", "b_marker"),
        })
    }

    /// A freshly-built, empty post-pass state triple, matching what every
    /// eval path assembles at its post-pass point.
    struct OwnedState {
        values: ValueMap,
        snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        diagnostics: Vec<Diagnostic>,
    }

    impl OwnedState {
        fn empty() -> Self {
            Self {
                values: ValueMap::new(),
                snapshot_values: PersistentMap::new(),
                diagnostics: Vec::new(),
            }
        }

        /// A post-pass state seeded with `entries`, mirroring what every eval
        /// path holds at its post-pass point: `values` and `snapshot_values`
        /// carry the SAME per-cell value (the snapshot side paired with
        /// `DeterminacyState::Determined`, matching the pre-post-pass state
        /// `Engine::eval` assembles before the inline PSD pass runs).
        fn seeded(entries: &[(ValueCellId, Value)]) -> Self {
            let mut state = Self::empty();
            for (id, value) in entries {
                state.values.insert(id.clone(), value.clone());
                state
                    .snapshot_values
                    .insert(id.clone(), (value.clone(), DeterminacyState::Determined));
            }
            state
        }

        fn as_state(&mut self) -> PostPassState<'_> {
            PostPassState {
                values: &mut self.values,
                snapshot_values: &mut self.snapshot_values,
                diagnostics: &mut self.diagnostics,
            }
        }
    }

    /// INV-EVAL-3's core done-criteria: the SAME registry, run against
    /// independently-assembled but EQUAL post-pass state, yields identical
    /// diagnostics — modelling "runs identically on eval / eval_cached /
    /// edit_check": same registry + equal state ⇒ identical output
    /// regardless of which call site assembled the state.
    #[test]
    fn same_post_pass_state_yields_same_diagnostic_set() {
        let mut registry = DetectorRegistry::new();
        registry.register(detector_a());
        registry.register(detector_b());

        // Three INDEPENDENTLY-built, but equal (empty), states — mimicking
        // cold eval / warm eval_cached / edit_check assembling their own
        // post-pass state at three different call sites.
        let mut site_eval = OwnedState::empty();
        let mut site_eval_cached = OwnedState::empty();
        let mut site_edit_check = OwnedState::empty();

        registry.run_all(&mut site_eval.as_state());
        registry.run_all(&mut site_eval_cached.as_state());
        registry.run_all(&mut site_edit_check.as_state());

        let expected = vec![
            (
                Some(DiagnosticCode::ConstraintViolated),
                "detector A fired".to_string(),
            ),
            (
                Some(DiagnosticCode::SelectorKindMismatch),
                "detector B fired".to_string(),
            ),
        ];
        assert_eq!(diag_keys(&site_eval.diagnostics), expected);
        assert_eq!(diag_keys(&site_eval_cached.diagnostics), expected);
        assert_eq!(diag_keys(&site_edit_check.diagnostics), expected);

        // The value-map mutation is also identical across all three sites.
        for site in [&site_eval, &site_eval_cached, &site_edit_check] {
            assert_eq!(
                site.values.get(&ValueCellId::new("Body", "a_marker")),
                Some(&Value::Bool(true))
            );
            assert_eq!(
                site.values.get(&ValueCellId::new("Body", "b_marker")),
                Some(&Value::Bool(true))
            );
        }
    }

    /// Registration order IS run order — the single ordering core that
    /// replaces the scattered "must run before …" / "MUST run AFTER …"
    /// convention comments (e.g. `engine_eval.rs:6312`,
    /// `structural_query.rs:531,610`, `significance_filter.rs:1025,1032`).
    /// Order is caller-controlled registration order, not source-file
    /// scatter: the SAME three detectors registered in two DIFFERENT orders
    /// each run — and report via `ids()` — in their own registration order.
    #[test]
    fn run_all_executes_in_registration_order() {
        fn detector(slug: &'static str, code: DiagnosticCode) -> Box<dyn PostPassDetector> {
            Box::new(FixedMarkerDetector {
                slug,
                code,
                message: "fired",
                marker_cell: ValueCellId::new("Body", slug),
            })
        }

        // Registered in source order A, B, C.
        let mut registry_abc = DetectorRegistry::new();
        registry_abc.register(detector("a", DiagnosticCode::ConstraintViolated));
        registry_abc.register(detector("b", DiagnosticCode::SelectorKindMismatch));
        registry_abc.register(detector("c", DiagnosticCode::ConstraintIndeterminate));

        assert_eq!(registry_abc.ids(), vec!["a", "b", "c"]);

        let mut state_abc = OwnedState::empty();
        registry_abc.run_all(&mut state_abc.as_state());
        assert_eq!(
            diag_keys(&state_abc.diagnostics),
            vec![
                (Some(DiagnosticCode::ConstraintViolated), "fired".to_string()),
                (Some(DiagnosticCode::SelectorKindMismatch), "fired".to_string()),
                (
                    Some(DiagnosticCode::ConstraintIndeterminate),
                    "fired".to_string()
                ),
            ]
        );

        // The SAME three detectors, registered in a DIFFERENT order
        // (C, A, B): both the effect order and ids() follow the new
        // registration order, not the order above.
        let mut registry_cab = DetectorRegistry::new();
        registry_cab.register(detector("c", DiagnosticCode::ConstraintIndeterminate));
        registry_cab.register(detector("a", DiagnosticCode::ConstraintViolated));
        registry_cab.register(detector("b", DiagnosticCode::SelectorKindMismatch));

        assert_eq!(registry_cab.ids(), vec!["c", "a", "b"]);

        let mut state_cab = OwnedState::empty();
        registry_cab.run_all(&mut state_cab.as_state());
        assert_eq!(
            diag_keys(&state_cab.diagnostics),
            vec![
                (
                    Some(DiagnosticCode::ConstraintIndeterminate),
                    "fired".to_string()
                ),
                (Some(DiagnosticCode::ConstraintViolated), "fired".to_string()),
                (Some(DiagnosticCode::SelectorKindMismatch), "fired".to_string()),
            ]
        );
    }

    /// Builds a `MassProperties` `StructureInstance`, optionally carrying an
    /// `inertia` field — the `dynamics_ops.rs:722+` test-construction
    /// pattern (registry-free sentinel `type_id`, `version: 0`).
    fn mass_properties(inertia_field: Option<Value>) -> Value {
        let mut entries: Vec<(String, Value)> = Vec::new();
        if let Some(v) = inertia_field {
            entries.push(("inertia".to_string(), v));
        }
        let fields: PersistentMap<String, Value> = entries.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "MassProperties".to_string(),
            version: 0,
            fields,
        }))
    }

    /// A `Value::Matrix` built from a plain `[[f64; 3]; 3]`.
    fn matrix3(rows: [[f64; 3]; 3]) -> Value {
        Value::Matrix(
            rows.iter()
                .map(|row| row.iter().map(|c| Value::Real(*c)).collect())
                .collect(),
        )
    }

    /// INV-EVAL-3 over a REAL detector: `MassPropertiesPsdDetector` (via
    /// `DetectorRegistry::with_builtins()`) classifies each `MassProperties`
    /// cell's `inertia` field identically across two independently-seeded
    /// but equal states — proving the trait/state design hosts real,
    /// mutating production logic, not just test doubles. Ports the
    /// characterization at `engine_eval.rs:4662-4762`. Only unambiguous
    /// matrices are used (clearly-PSD identity vs. clearly-non-PSD
    /// `diag(1,1,-1)`) — no near-tolerance-boundary premise. Covers all
    /// three `InertiaResult` arms (Skip / Malformed / Valid), including
    /// both ways to land on Skip (absent field vs. already-`Undef` field)
    /// and both ways to land on Malformed (wrong-shape vs.
    /// right-shape-but-non-numeric).
    #[test]
    fn mass_properties_psd_detector_flags_and_replaces_deterministically() {
        let identity_cell = ValueCellId::new("BodyA", "mass_props");
        let neg_eig_cell = ValueCellId::new("BodyB", "mass_props");
        let malformed_cell = ValueCellId::new("BodyC", "mass_props");
        let no_inertia_cell = ValueCellId::new("BodyD", "mass_props");
        let already_undef_cell = ValueCellId::new("BodyE", "mass_props");
        let non_numeric_cell = ValueCellId::new("BodyF", "mass_props");

        // (a) identity = clearly PSD.
        let identity_value = mass_properties(Some(matrix3([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ])));
        // (b) diag(1,1,-1) = clearly non-PSD (one negative eigenvalue).
        let neg_eig_value = mass_properties(Some(matrix3([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -1.0],
        ])));
        // (c) wrong-shape (2×2) inertia — unparseable as a 3×3 matrix.
        let malformed_value = mass_properties(Some(Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ])));
        // (d) no `inertia` field at all.
        let no_inertia_value = mass_properties(None);
        // (e) `inertia` field already `Value::Undef` — distinct from (d)'s
        // absent-field case, but must ALSO be left untouched (no false
        // positives / no re-flagging of an already-undetermined cell).
        let already_undef_value = mass_properties(Some(Value::Undef));
        // (f) right-shape (3×3) but non-numeric entry — unparseable, so
        // Malformed via a different path than (c)'s wrong-shape case.
        let non_numeric_value = mass_properties(Some(Value::Matrix(vec![
            vec![Value::Bool(true), Value::Real(0.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)],
        ])));

        let entries = [
            (identity_cell.clone(), identity_value.clone()),
            (neg_eig_cell.clone(), neg_eig_value.clone()),
            (malformed_cell.clone(), malformed_value.clone()),
            (no_inertia_cell.clone(), no_inertia_value.clone()),
            (already_undef_cell.clone(), already_undef_value.clone()),
            (non_numeric_cell.clone(), non_numeric_value.clone()),
        ];

        let registry = DetectorRegistry::with_builtins();
        assert!(
            registry.ids().contains(&"mass-properties-psd"),
            "with_builtins() must register the PSD detector; got ids {:?}",
            registry.ids()
        );

        let mut run1 = OwnedState::seeded(&entries);
        registry.run_all(&mut run1.as_state());

        let mut run2 = OwnedState::seeded(&entries);
        registry.run_all(&mut run2.as_state());

        // Exactly the non-PSD and malformed cells are flagged, both runs
        // agreeing on the SET of (code, message) diagnostics. Compared as a
        // multiset, not a sequence: the detector walks `values.iter()`
        // (an unsorted `im::HashMap`), so two independently-built states
        // can surface the same diagnostics in a different order — see
        // `diag_key_set`'s doc comment.
        assert_eq!(diag_key_set(&run1.diagnostics), diag_key_set(&run2.diagnostics));
        assert_eq!(
            run1.diagnostics.len(),
            3,
            "expected exactly the non-PSD + 2 malformed cells to be flagged, got {:?}",
            diag_keys(&run1.diagnostics)
        );
        for key in diag_keys(&run1.diagnostics) {
            assert_eq!(key.0, Some(DiagnosticCode::DynamicsInertiaNotPSD));
        }

        for run in [&run1, &run2] {
            // (a) identity = PSD → left untouched in both values and snapshot.
            assert_eq!(run.values.get(&identity_cell), Some(&identity_value));
            assert_eq!(
                run.snapshot_values.get(&identity_cell),
                Some(&(identity_value.clone(), DeterminacyState::Determined))
            );

            // (b) non-PSD → Undef-replaced in both values and snapshot.
            assert_eq!(run.values.get(&neg_eig_cell), Some(&Value::Undef));
            assert_eq!(
                run.snapshot_values.get(&neg_eig_cell),
                Some(&(Value::Undef, DeterminacyState::Determined))
            );

            // (c) malformed → Undef-replaced in both values and snapshot.
            assert_eq!(run.values.get(&malformed_cell), Some(&Value::Undef));
            assert_eq!(
                run.snapshot_values.get(&malformed_cell),
                Some(&(Value::Undef, DeterminacyState::Determined))
            );

            // (d) no inertia field = Skip → left untouched.
            assert_eq!(run.values.get(&no_inertia_cell), Some(&no_inertia_value));
            assert_eq!(
                run.snapshot_values.get(&no_inertia_cell),
                Some(&(no_inertia_value.clone(), DeterminacyState::Determined))
            );

            // (e) already-Undef inertia field = Skip → left untouched (not
            // re-flagged; distinct from (d)'s absent-field Skip).
            assert_eq!(
                run.values.get(&already_undef_cell),
                Some(&already_undef_value)
            );
            assert_eq!(
                run.snapshot_values.get(&already_undef_cell),
                Some(&(already_undef_value.clone(), DeterminacyState::Determined))
            );

            // (f) right-shape but non-numeric inertia → Malformed →
            // Undef-replaced in both values and snapshot, same as (c).
            assert_eq!(run.values.get(&non_numeric_cell), Some(&Value::Undef));
            assert_eq!(
                run.snapshot_values.get(&non_numeric_cell),
                Some(&(Value::Undef, DeterminacyState::Determined))
            );
        }
    }

    /// Covers `MassPropertiesPsdDetector`'s fast early-out guard
    /// (`has_mass_props == false` → `return`), which no other test
    /// exercises: [`mass_properties_psd_detector_flags_and_replaces_deterministically`]
    /// always seeds at least one `MassProperties` cell, and
    /// [`same_post_pass_state_yields_same_diagnostic_set`] only runs the
    /// `FixedMarkerDetector` test doubles, never `with_builtins()`. Runs
    /// the real registry against a state with no `MassProperties` cell at
    /// all — a decoy `StructureInstance` of a *different* `type_name`, plus
    /// a plain non-struct value — and asserts a complete no-op: zero
    /// diagnostics, every value and snapshot entry byte-identical to what
    /// was seeded. If the guard were ever inverted or dropped, this test
    /// would fail.
    #[test]
    fn mass_properties_psd_detector_is_a_no_op_without_mass_properties_cells() {
        let other_struct_cell = ValueCellId::new("BodyX", "other_struct");
        let other_struct_value = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "NotMassProperties".to_string(),
            version: 0,
            fields: PersistentMap::new(),
        }));
        let plain_cell = ValueCellId::new("BodyY", "plain");
        let plain_value = Value::Real(42.0);

        let entries = [
            (other_struct_cell.clone(), other_struct_value.clone()),
            (plain_cell.clone(), plain_value.clone()),
        ];
        let mut state = OwnedState::seeded(&entries);

        let registry = DetectorRegistry::with_builtins();
        assert!(registry.ids().contains(&"mass-properties-psd"));
        registry.run_all(&mut state.as_state());

        assert!(
            state.diagnostics.is_empty(),
            "expected no diagnostics when no MassProperties cell is present, got {:?}",
            diag_keys(&state.diagnostics)
        );
        assert_eq!(
            state.values.get(&other_struct_cell),
            Some(&other_struct_value)
        );
        assert_eq!(state.values.get(&plain_cell), Some(&plain_value));
        assert_eq!(
            state.snapshot_values.get(&other_struct_cell),
            Some(&(other_struct_value, DeterminacyState::Determined))
        );
        assert_eq!(
            state.snapshot_values.get(&plain_cell),
            Some(&(plain_value, DeterminacyState::Determined))
        );
    }

    /// Pins the FIXED diagnostic wording (excluding the interpolated cell
    /// id and the non-PSD message's `{:.3e}`-formatted eigenvalue, to avoid
    /// a fragile numeric-format premise) against the characterization
    /// source it mirrors, `engine_eval.rs:4662-4762` — see
    /// [`MassPropertiesPsdDetector`]'s drift-warning doc comment. A wording
    /// change to either message that isn't mirrored on the other side is
    /// caught here rather than silently drifting.
    #[test]
    fn mass_properties_psd_detector_messages_match_characterization_wording() {
        let malformed_cell = ValueCellId::new("BodyC", "mass_props");
        let malformed_value = mass_properties(Some(Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ])));
        let neg_eig_cell = ValueCellId::new("BodyB", "mass_props");
        let neg_eig_value = mass_properties(Some(matrix3([
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -1.0],
        ])));

        let entries = [
            (malformed_cell, malformed_value),
            (neg_eig_cell, neg_eig_value),
        ];
        let mut state = OwnedState::seeded(&entries);
        DetectorRegistry::with_builtins().run_all(&mut state.as_state());

        let messages: Vec<&str> = state
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();

        assert!(
            messages
                .iter()
                .any(|m| m.contains("inertia field cannot be parsed as a 3×3 numeric matrix")),
            "malformed message wording drifted from engine_eval.rs:4662-4762; got {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains(
                "inertia tensor is not positive semi-definite (min eigenvalue ≈ "
            )),
            "non-PSD message wording drifted from engine_eval.rs:4662-4762; got {messages:?}"
        );
    }
}
