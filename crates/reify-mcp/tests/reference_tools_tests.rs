use reify_mcp::context::MockToolContext;
use reify_mcp::registry::ToolRegistry;
use reify_mcp::tools::register_all_tools;
// ToolError not used directly — reference tool returns Ok for all cases

fn setup_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    registry
}

// === reify_language_reference ===

const ALL_TOPICS: &[&str] = &[
    "collections",
    "connect",
    "constraints",
    "enums",
    "fields",
    "functions",
    "geometry",
    "guards",
    "occurrences",
    "parameters",
    "purposes",
    "stdlib",
    "structures",
    "syntax",
    "traits",
    "types",
    "units",
];

#[test]
fn language_reference_syntax_returns_content() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_language_reference",
            serde_json::json!({"topic": "syntax"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["topic"], "syntax");
    let content = result["content"]
        .as_str()
        .expect("content should be a string");
    assert!(
        content.len() > 100,
        "content should be substantial, got {} chars",
        content.len()
    );
}

#[test]
fn language_reference_all_topics_return_non_empty_content() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    for topic in ALL_TOPICS {
        let result = registry
            .call_tool(
                "reify_language_reference",
                serde_json::json!({"topic": topic}),
                &ctx,
            )
            .unwrap_or_else(|e| panic!("topic '{topic}' should succeed, got: {e:?}"));

        assert_eq!(
            result["topic"], *topic,
            "topic field mismatch for '{topic}'"
        );
        let content = result["content"]
            .as_str()
            .unwrap_or_else(|| panic!("content for '{topic}' should be a string"));
        assert!(
            content.len() > 100,
            "content for '{topic}' too short: {} chars",
            content.len()
        );
    }
}

#[test]
fn language_reference_unknown_topic_returns_available_topics() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_language_reference",
            serde_json::json!({"topic": "foobar"}),
            &ctx,
        )
        .expect("should succeed (not error)");

    let content = result["content"]
        .as_str()
        .expect("content should be a string");
    // Should mention available topics
    assert!(
        content.contains("syntax"),
        "help should list 'syntax' as available topic"
    );
    assert!(
        content.contains("parameters"),
        "help should list 'parameters' as available topic"
    );
}

#[test]
fn language_reference_no_topic_returns_available_topics() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool("reify_language_reference", serde_json::json!({}), &ctx)
        .expect("should succeed (not error)");

    assert_eq!(result["topic"], "help");
    let content = result["content"]
        .as_str()
        .expect("content should be a string");
    // Should list available topics
    assert!(content.contains("syntax"), "help should list 'syntax'");
    assert!(
        content.contains("constraints"),
        "help should list 'constraints'"
    );
}
