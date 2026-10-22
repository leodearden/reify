#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use reify_core::{Diagnostic, Severity};
    use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

    use super::{LoadCaseParts, LoadCaseSweep, run_load_cases};
    use crate::compute_targets::fea_diagnostics::{ElementId, FeaDiagnosticDetail};
    use crate::{
        CancellationHandle, ComputeOutcome, RealizationReadHandle, StructuredComputeDetail,
    };

    /// Completes `Int(n)` loads with one warning and one `ProblemElements [n]`
    /// detail; fails anything else with one error and `ProblemElements [99]`.
    fn stub_solve_case(
        value_inputs: &[Value],
        _realization_inputs: &[RealizationReadHandle],
        _options: &Value,
        _prior_warm_state: Option<&OpaqueState>,
        _cancellation: &CancellationHandle,
    ) -> ComputeOutcome {
        match &value_inputs[0] {
            Value::Int(n) => ComputeOutcome::Completed {
                result: Value::Int(*n),
                new_warm_state: None,
                cost_per_byte: None,
                diagnostics: vec![Diagnostic::warning(format!("case {n}"))],
                structured_detail: vec![problem(*n as usize)],
            },
            _ => ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error("stub failure")],
                structured_detail: vec![problem(99)],
            },
        }
    }

    const SWEEP: LoadCaseSweep = LoadCaseSweep {
        label: "test_fn (solver::test)",
        empty_cases_message: "test_fn needs at least one LoadCase.",
        solve_case: stub_solve_case,
    };

    fn problem(id: usize) -> StructuredComputeDetail {
        StructuredComputeDetail::Fea(FeaDiagnosticDetail::ProblemElements {
            ids: vec![ElementId(id)],
        })
    }

    fn load_case_with(name: &str, loads: Option<Value>, options: Option<Value>) -> Value {
        let mut fields: PersistentMap<String, Value> = [
            ("name".to_string(), Value::String(name.to_string())),
            ("supports".to_string(), Value::List(vec![])),
        ]
        .into_iter()
        .collect();
        if let Some(loads) = loads {
            fields.insert("loads".to_string(), loads);
        }
        if let Some(options) = options {
            fields.insert("options".to_string(), options);
        }
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "LoadCase".to_string(),
            version: 1,
            fields,
        }))
    }

    fn load_case(name: &str, loads: Option<Value>) -> Value {
        load_case_with(name, loads, None)
    }

    fn run(cases: &Value) -> ComputeOutcome {
        run_load_cases(
            &SWEEP,
            cases,
            |parts: LoadCaseParts| vec![parts.loads, parts.supports],
            &[],
            &CancellationHandle::new(),
        )
    }

    fn summary(diagnostics: &[Diagnostic]) -> Vec<(Severity, String)> {
        diagnostics
            .iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect()
    }

    fn warning(message: &str) -> (Severity, String) {
        (Severity::Warning, message.to_string())
    }

    fn error(message: &str) -> (Severity, String) {
        (Severity::Error, message.to_string())
    }

    fn cases_map(entries: &[(&str, Value)]) -> Value {
        let inner: BTreeMap<Value, Value> = entries
            .iter()
            .map(|(name, value)| (Value::String(name.to_string()), value.clone()))
            .collect();
        Value::Map(
            [(Value::String("cases".to_string()), Value::Map(inner))]
                .into_iter()
                .collect(),
        )
    }

    fn expect_completed(
        outcome: ComputeOutcome,
    ) -> (Value, Vec<Diagnostic>, Vec<StructuredComputeDetail>) {
        match outcome {
            ComputeOutcome::Completed {
                result,
                diagnostics,
                structured_detail,
                ..
            } => (result, diagnostics, structured_detail),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    fn expect_failed(outcome: ComputeOutcome) -> (Vec<Diagnostic>, Vec<StructuredComputeDetail>) {
        match outcome {
            ComputeOutcome::Failed {
                diagnostics,
                structured_detail,
            } => (diagnostics, structured_detail),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn completed_carries_every_case_detail_in_case_order() {
        let cases = Value::List(vec![
            load_case("a", Some(Value::Int(1))),
            load_case("b", Some(Value::Int(2))),
        ]);

        let (result, diagnostics, structured_detail) = expect_completed(run(&cases));

        assert_eq!(
            summary(&diagnostics),
            vec![warning("case 1"), warning("case 2")]
        );
        assert_eq!(structured_detail, vec![problem(1), problem(2)]);
        assert_eq!(
            result,
            cases_map(&[("a", Value::Int(1)), ("b", Value::Int(2))])
        );
    }

    #[test]
    fn failing_case_carries_the_earlier_case_detail_and_its_own() {
        let cases = Value::List(vec![
            load_case("a", Some(Value::Int(1))),
            load_case("b", Some(Value::Real(0.0))),
        ]);

        let (diagnostics, structured_detail) = expect_failed(run(&cases));

        assert_eq!(
            summary(&diagnostics),
            vec![warning("case 1"), error("stub failure")]
        );
        assert_eq!(structured_detail, vec![problem(1), problem(99)]);
    }

    #[test]
    fn malformed_case_after_a_completed_case_keeps_what_was_accumulated() {
        let cases = Value::List(vec![
            load_case("a", Some(Value::Int(1))),
            load_case("b", None),
        ]);

        let (diagnostics, structured_detail) = expect_failed(run(&cases));

        assert_eq!(
            summary(&diagnostics),
            vec![
                warning("case 1"),
                error("test_fn (solver::test): LoadCase \"b\" is missing the \"loads\" field"),
            ]
        );
        assert_eq!(structured_detail, vec![problem(1)]);
    }

    #[test]
    fn duplicate_name_keeps_the_later_result_and_both_details() {
        let cases = Value::List(vec![
            load_case("a", Some(Value::Int(1))),
            load_case("a", Some(Value::Int(2))),
        ]);

        let (result, diagnostics, structured_detail) = expect_completed(run(&cases));

        assert_eq!(
            summary(&diagnostics),
            vec![
                warning("case 1"),
                warning("case 2"),
                warning(
                    "test_fn (solver::test): duplicate LoadCase name \"a\" — the earlier \
                     result for this case is overwritten; this is almost certainly a user error"
                ),
            ]
        );
        assert_eq!(structured_detail, vec![problem(1), problem(2)]);
        assert_eq!(result, cases_map(&[("a", Value::Int(2))]));
    }

    #[test]
    fn cancelled_before_a_case_returns_cancelled() {
        let cancellation = CancellationHandle::new();
        cancellation.cancel();
        let cases = Value::List(vec![load_case("a", Some(Value::Int(1)))]);

        let outcome = run_load_cases(
            &SWEEP,
            &cases,
            |parts: LoadCaseParts| vec![parts.loads, parts.supports],
            &[],
            &cancellation,
        );

        assert!(
            matches!(outcome, ComputeOutcome::Cancelled),
            "got {outcome:?}"
        );
    }

    #[test]
    fn non_list_and_empty_cases_fail_without_detail() {
        let (diagnostics, structured_detail) = expect_failed(run(&Value::Int(0)));
        assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert!(
            diagnostics[0]
                .message
                .starts_with("test_fn (solver::test): cases argument must be Value::List"),
            "got {:?}",
            diagnostics[0].message
        );
        assert!(structured_detail.is_empty());

        let (diagnostics, structured_detail) = expect_failed(run(&Value::List(vec![])));
        assert_eq!(
            summary(&diagnostics),
            vec![error(SWEEP.empty_cases_message)]
        );
        assert!(structured_detail.is_empty());
    }

    #[test]
    fn options_override_is_the_inner_value_of_a_some_options_field_only() {
        let cases = Value::List(vec![
            load_case_with(
                "some",
                Some(Value::Int(1)),
                Some(Value::Option(Some(Box::new(Value::Int(7))))),
            ),
            load_case_with("none", Some(Value::Int(2)), Some(Value::Option(None))),
            load_case_with("absent", Some(Value::Int(3)), None),
        ]);
        let seen: RefCell<Vec<Option<Value>>> = RefCell::new(Vec::new());

        let outcome = run_load_cases(
            &SWEEP,
            &cases,
            |parts: LoadCaseParts| {
                seen.borrow_mut().push(parts.options_override.clone());
                vec![parts.loads, parts.supports]
            },
            &[],
            &CancellationHandle::new(),
        );

        expect_completed(outcome);
        assert_eq!(seen.into_inner(), vec![Some(Value::Int(7)), None, None]);
    }
}
