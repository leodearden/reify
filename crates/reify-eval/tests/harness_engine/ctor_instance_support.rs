//! Cell and ctor-field readers shared by the ctor-built-instance e2e modules,
//! `ctor_instance_member_order` (task #10098) and
//! `ctor_instance_geometry_selector` (task #10352).

use reify_core::{Severity, ValueCellId};
use reify_eval::EvalResult;
use reify_ir::Value;

/// The value of a plain cell: a template member or a `sub` member.
#[track_caller]
pub(crate) fn cell(result: &EvalResult, entity: &str, member: &str) -> Value {
    result
        .values
        .get(&ValueCellId::new(entity, member))
        .cloned()
        .unwrap_or_else(|| panic!("no cell {entity}.{member} after eval"))
}

/// A field of the structure instance `instance`, described as `what`.
#[track_caller]
pub(crate) fn instance_field(instance: &Value, what: &str, member: &str) -> Value {
    match instance {
        Value::StructureInstance(data) => data
            .fields
            .get(member)
            .cloned()
            .unwrap_or_else(|| panic!("{what} has no `{member}` field: {:?}", data.fields)),
        other => panic!("{what} must hold a StructureInstance, got {other:?}"),
    }
}

/// A field of the structure instance held by cell `host.holder`.
#[track_caller]
pub(crate) fn ctor_field(result: &EvalResult, host: &str, holder: &str, member: &str) -> Value {
    instance_field(
        &cell(result, host, holder),
        &format!("{host}.{holder}"),
        member,
    )
}

#[track_caller]
pub(crate) fn assert_no_error_diagnostics(result: &EvalResult, what: &str) {
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for {what}; got: {errors:?}"
    );
}
