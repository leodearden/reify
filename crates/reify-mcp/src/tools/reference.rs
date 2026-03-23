// Reference tools (1 tool)

use super::language_chunks;
use crate::registry::ToolRegistry;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(
        "reify_language_reference",
        "Look up Reify language reference documentation for a topic.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "The language feature or topic to look up (e.g., 'syntax', 'constraints', 'parameters')."
                }
            }
        }),
        |params, _ctx| {
            let topic = params["topic"].as_str();

            match topic {
                Some(t) => {
                    if let Some(content) = language_chunks::get_chunk(t) {
                        Ok(serde_json::json!({
                            "topic": t,
                            "content": content,
                        }))
                    } else {
                        let help = format_topics_help();
                        Ok(serde_json::json!({
                            "topic": t,
                            "content": help,
                        }))
                    }
                }
                None => {
                    let help = format_topics_help();
                    Ok(serde_json::json!({
                        "topic": "help",
                        "content": help,
                    }))
                }
            }
        },
    );
}

fn format_topics_help() -> String {
    let topics = language_chunks::available_topics();
    format!(
        "Available topics: {}. Pass one as the 'topic' parameter to get documentation.",
        topics.join(", ")
    )
}
