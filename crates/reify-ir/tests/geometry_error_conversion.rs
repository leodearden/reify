//! `From<GeometryError>` for the query, export and tessellation error types.
//!
//! A kernel resolves every operand through one `GeometryError`-returning
//! lookup and propagates it with `?`. These conversions decide what the
//! caller sees: an unknown handle (`InvalidReference`) is the only
//! invalid-handle case, and every other `GeometryError` becomes the target's
//! failure variant carrying its cause.

use reify_ir::{ExportError, GeometryError, GeometryHandleId, QueryError, TessError};

fn unknown_handle() -> GeometryError {
    GeometryError::InvalidReference(GeometryHandleId(7))
}

fn corrupted_handle_cause() -> String {
    "shape handle GeometryHandleId(3) wraps null/empty topology (IsNull)".to_string()
}

fn init_failure() -> GeometryError {
    GeometryError::InitFailed("boom".into())
}

#[test]
fn query_error_from_unknown_handle_is_invalid_handle() {
    match QueryError::from(unknown_handle()) {
        QueryError::InvalidHandle(id) => assert_eq!(id, GeometryHandleId(7)),
        other => panic!("expected QueryError::InvalidHandle(7), got {other:?}"),
    }
}

#[test]
fn query_error_from_operation_failed_keeps_the_cause_verbatim() {
    let cause = corrupted_handle_cause();
    match QueryError::from(GeometryError::OperationFailed(cause.clone())) {
        QueryError::QueryFailed(m) => assert_eq!(m, cause),
        other => panic!("expected QueryError::QueryFailed(cause), got {other:?}"),
    }
}

#[test]
fn query_error_from_init_failed_is_a_failure_carrying_its_display() {
    let expected = init_failure().to_string();
    match QueryError::from(init_failure()) {
        QueryError::QueryFailed(m) => assert_eq!(m, expected),
        other => panic!("expected QueryError::QueryFailed, got {other:?}"),
    }
}

#[test]
fn export_error_from_unknown_handle_is_invalid_handle() {
    match ExportError::from(unknown_handle()) {
        ExportError::InvalidHandle(id) => assert_eq!(id, GeometryHandleId(7)),
        other => panic!("expected ExportError::InvalidHandle(7), got {other:?}"),
    }
}

#[test]
fn export_error_from_operation_failed_keeps_the_cause_verbatim() {
    let cause = corrupted_handle_cause();
    match ExportError::from(GeometryError::OperationFailed(cause.clone())) {
        ExportError::FormatError(m) => assert_eq!(m, cause),
        other => panic!("expected ExportError::FormatError(cause), got {other:?}"),
    }
}

#[test]
fn export_error_from_init_failed_is_a_failure_carrying_its_display() {
    let expected = init_failure().to_string();
    match ExportError::from(init_failure()) {
        ExportError::FormatError(m) => assert_eq!(m, expected),
        other => panic!("expected ExportError::FormatError, got {other:?}"),
    }
}

#[test]
fn tess_error_from_unknown_handle_is_invalid_handle() {
    match TessError::from(unknown_handle()) {
        TessError::InvalidHandle(id) => assert_eq!(id, GeometryHandleId(7)),
        other => panic!("expected TessError::InvalidHandle(7), got {other:?}"),
    }
}

#[test]
fn tess_error_from_operation_failed_keeps_the_cause_verbatim() {
    let cause = corrupted_handle_cause();
    match TessError::from(GeometryError::OperationFailed(cause.clone())) {
        TessError::TessellationFailed(m) => assert_eq!(m, cause),
        other => panic!("expected TessError::TessellationFailed(cause), got {other:?}"),
    }
}

#[test]
fn tess_error_from_init_failed_is_a_failure_carrying_its_display() {
    let expected = init_failure().to_string();
    match TessError::from(init_failure()) {
        TessError::TessellationFailed(m) => assert_eq!(m, expected),
        other => panic!("expected TessError::TessellationFailed, got {other:?}"),
    }
}
