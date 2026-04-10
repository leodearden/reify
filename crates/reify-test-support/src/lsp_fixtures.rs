/// Minimal valid `InitializeParams` payload as a raw JSON string.
///
/// Equivalent to [`minimal_init_params()`] — the two forms are pinned by a unit
/// test in this module so they can never drift silently.
pub const MINIMAL_INIT_PARAMS_JSON: &str = r#"{"capabilities":{}}"#;

/// Minimal valid `InitializeParams` payload as a [`serde_json::Value`].
///
/// Equivalent to [`MINIMAL_INIT_PARAMS_JSON`] — the two forms are pinned by a
/// unit test in this module so they can never drift silently.
pub fn minimal_init_params() -> serde_json::Value {
    serde_json::json!({"capabilities": {}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_init_params_has_empty_capabilities_object() {
        let v = minimal_init_params();
        let caps = &v["capabilities"];
        assert!(caps.is_object(), "capabilities should be an object");
        assert!(
            caps.as_object().unwrap().is_empty(),
            "capabilities should be empty"
        );
    }

    #[test]
    fn const_and_fn_produce_equivalent_values() {
        let from_const: serde_json::Value =
            serde_json::from_str(MINIMAL_INIT_PARAMS_JSON).expect("const should parse");
        assert_eq!(from_const, minimal_init_params());
    }
}
