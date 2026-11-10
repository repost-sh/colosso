//! The tools the server lists, from both catalogs with no per-tool code: the terminals'
//! (`docs/terminals.md` 8.5) filtered by the client's grants, and the app's
//! (`docs/app-control.md` 5) always, the app refusing by scope; and how results look.

use std::sync::Arc;

use koyo_app_control::{APP_LAUNCH, ControlError, Descriptor, catalog, summarize};
use koyo_term_control::ProtocolError;
use koyo_term_control::catalog::{CATALOG, Operation, Scope};
use rmcp::model::{CallToolResult, ContentBlock, Tool, ToolAnnotations};
use serde_json::Value;

use crate::launch;

/// The terminal tools `grants` allow, then every app tool, then `app_launch` when the
/// server can start the app.
pub fn list(grants: &[Scope], launches: bool) -> Vec<Tool> {
    let mut tools: Vec<Tool> = (CATALOG.iter())
        .filter(|operation| grants.contains(&operation.scope))
        .map(terminal)
        .collect();
    for descriptor in catalog() {
        // The app's catalog keeps clear of the terminals' names (its tests say so); a build
        // where they met would call the terminal tool, so the app's isn't listed.
        if is_terminal(descriptor.name) || descriptor.name == APP_LAUNCH {
            log::warn!(
                "the app's {} has the name of another tool, so it isn't listed",
                descriptor.name
            );
            continue;
        }
        tools.push(app(descriptor));
    }
    if launches {
        tools.push(launch::tool());
    }
    tools
}

/// Whether `name` is a terminal tool, granted or not.
pub fn is_terminal(name: &str) -> bool {
    CATALOG.iter().any(|operation| operation.name == name)
}

fn terminal(operation: &Operation) -> Tool {
    let schema = match operation.input_schema() {
        Value::Object(schema) => schema,
        _ => serde_json::Map::new(),
    };
    let hints = operation.hints;
    let annotations = ToolAnnotations::with_title(operation.title)
        .read_only(hints.read_only)
        .destructive(hints.destructive)
        .idempotent(hints.idempotent)
        .open_world(false);
    Tool::new(operation.name, operation.description, schema)
        .with_title(operation.title)
        .with_annotations(annotations)
}

fn app(descriptor: Descriptor) -> Tool {
    let Descriptor {
        info,
        input_schema,
        output_schema,
    } = descriptor;
    let annotations = ToolAnnotations::with_title(info.title)
        .read_only(info.hints.read_only)
        .destructive(info.hints.destructive)
        .idempotent(info.hints.idempotent)
        .open_world(info.hints.open_world);
    Tool::new(info.name, info.description, Arc::new(input_schema))
        .with_title(info.title)
        .with_raw_output_schema(Arc::new(output_schema))
        .with_annotations(annotations)
}

/// The summary for the reader, then the structured answer, also as text for the clients
/// that read only text.
pub fn success(summary: String, structured: Value) -> CallToolResult {
    let mut result = CallToolResult::structured(structured);
    result.content.insert(0, ContentBlock::text(summary));
    result
}

/// An app operation's output as a result. A screenshot is an image block after the summary
/// instead of its base64 as text; its structured answer leaves `data` empty rather than
/// send the image twice.
pub fn app_success(name: &str, mut output: Value) -> CallToolResult {
    let mime = (output["mimeType"].as_str())
        .filter(|mime| mime.starts_with("image/") && output["data"].is_string())
        .map(str::to_owned);
    if let Some(mime) = mime {
        // Summed up while it still holds the image, whose size the summary says.
        let summary = summarize(name, &output).unwrap_or_else(|| format!("{name} done"));
        let Value::String(data) =
            std::mem::replace(&mut output["data"], Value::String(String::new()))
        else {
            return success(summary, output);
        };
        let content = vec![ContentBlock::text(summary), ContentBlock::image(data, mime)];
        let mut result = CallToolResult::success(content);
        result.structured_content = Some(output);
        return result;
    }
    let summary = summarize(name, &output).unwrap_or_else(|| format!("{name} done"));
    success(summary, output)
}

/// The daemon's refusal as a result.
pub fn terminal_failure(error: &ProtocolError) -> CallToolResult {
    refused(
        serde_json::to_value(error).unwrap_or_default(),
        &error.message,
    )
}

/// The app's refusal as a result.
pub fn app_failure(error: &ControlError) -> CallToolResult {
    refused(
        serde_json::to_value(error).unwrap_or_default(),
        &error.message,
    )
}

/// An `isError` result whose structured content is `{ code, message, retryable, details? }`,
/// the message first as text.
fn refused(structured: Value, message: &str) -> CallToolResult {
    let mut result = CallToolResult::structured_error(structured);
    result.content.insert(0, ContentBlock::text(message));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every terminal scope, so every tool is listed.
    const ALL: [Scope; 4] = [Scope::Read, Scope::Write, Scope::Manage, Scope::Destructive];

    #[test]
    fn no_app_tool_is_lost_to_a_terminal_name() {
        let listed = list(&ALL, true);
        assert_eq!(listed.len(), CATALOG.len() + catalog().len() + 1);
        let mut names: Vec<_> = listed.iter().map(|tool| tool.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), listed.len(), "every tool name is unique");
    }

    /// A chat runs the tools that only read without asking (`KOYO_READS` in the agent host's
    /// options.ts), so that list is exactly the read-only ones.
    #[test]
    fn a_chat_runs_without_asking_exactly_the_tools_that_only_read() {
        let path = std::path::Path::new(env!("KOYO_CHECKOUT"))
            .join("koyo-js/koyo-agent-host/src/providers/claude/options.ts");
        let source = std::fs::read_to_string(path).unwrap();
        let (_, names) = source.split_once("const KOYO_READS = [").unwrap();
        let (names, _) = names.split_once("];").unwrap();
        let mut allowed: Vec<&str> = (names.split(','))
            .map(|name| name.trim().trim_matches('"'))
            .filter(|name| !name.is_empty())
            .collect();
        allowed.sort_unstable();
        let mut reads: Vec<String> = (list(&ALL, true).into_iter())
            .filter(|tool| tool.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true))
            .map(|tool| tool.name.to_string())
            .collect();
        reads.sort_unstable();
        assert_eq!(allowed, reads);
    }

    #[test]
    fn a_screenshot_is_an_image() {
        let output = serde_json::json!({
            "pane": 3, "url": "https://example.com", "mimeType": "image/png", "data": "iVBORw0KGgo="
        });
        let result = app_success("browser_screenshot", output.clone());
        let mut sent = output;
        sent["data"] = serde_json::json!("");
        assert_eq!(
            result.structured_content,
            Some(sent),
            "the image goes once, as a block"
        );
        assert!(result.content[0].as_text().is_some());
        assert!(result.content[1].as_image().is_some());
        assert_eq!(result.content.len(), 2);
    }

    #[test]
    fn a_viewport_set_with_a_screenshot_brings_its_image_as_a_block() {
        let viewport = serde_json::json!({
            "mode": "device", "device": "iphone-15", "width": 393, "height": 852, "scale": 3.0,
            "mobile": true, "touch": true, "orientation": "portrait", "zoom": "fit",
            "shownPercent": 83, "setBy": "claude-code:7f3a"
        });
        let output = serde_json::json!({
            "viewport": viewport, "settled": true, "mimeType": "image/png", "data": "iVBORw0KGgo="
        });
        let result = app_success("browser_viewport", output);
        assert_eq!(result.content.len(), 2);
        let summary = result.content[0].as_text().unwrap();
        assert!(
            summary.text.starts_with("iphone-15, 393 × 852"),
            "{}",
            summary.text
        );
        assert!(result.content[1].as_image().is_some());
        let structured = result.structured_content.unwrap();
        assert_eq!(structured["data"], "", "the image goes once, as a block");
        assert_eq!(structured["settled"], true);
        let plain = serde_json::json!({ "viewport": structured["viewport"], "settled": false });
        let result = app_success("browser_viewport", plain);
        assert!(
            result
                .content
                .iter()
                .all(|block| block.as_image().is_none())
        );
    }
}
