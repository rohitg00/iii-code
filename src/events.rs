use serde_json::Value;

pub fn normalize_stream_item(item: &Value) -> Value {
    item.get("data").cloned().unwrap_or_else(|| item.clone())
}

pub fn is_agent_end(event: &Value) -> bool {
    event.get("type").and_then(Value::as_str) == Some("agent_end")
}

pub fn render_event(event: &Value) -> Option<String> {
    match event.get("type").and_then(Value::as_str)? {
        "agent_start" => Some("session started".into()),
        "turn_start" => Some("turn started".into()),
        "message_end" => render_message_end(event),
        "function_execution_start" | "tool_execution_start" => {
            let function_id = event
                .get("function_id")
                .or_else(|| event.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            Some(format!("tool start: {function_id}"))
        }
        "function_execution_end" | "tool_execution_end" => {
            let function_id = event
                .get("function_id")
                .or_else(|| event.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let status = if event.get("is_error").and_then(Value::as_bool) == Some(true) {
                "error"
            } else {
                "ok"
            };
            Some(format!("tool end: {function_id} ({status})"))
        }
        "approval_requested" => {
            let function_id = event
                .get("function_id")
                .or_else(|| event.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            Some(format!("approval requested: {function_id}"))
        }
        "agent_end" => Some("session ended".into()),
        _ => None,
    }
}

pub fn render_final_messages(value: &Value) -> Vec<String> {
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };
    messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .filter_map(message_text)
        .collect()
}

fn render_message_end(event: &Value) -> Option<String> {
    let message = event.get("message")?;
    match message.get("role").and_then(Value::as_str) {
        Some("assistant") => message_text(message).map(|text| format!("assistant:\n{text}")),
        Some("function_result") | Some("tool_result") => {
            let function_id = message
                .get("function_id")
                .or_else(|| message.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            Some(format!("tool result: {function_id}"))
        }
        _ => None,
    }
}

fn message_text(message: &Value) -> Option<String> {
    let content = message.get("content")?.as_array()?;
    let mut parts = Vec::new();
    for block in content {
        if let Some(text) = block.get("text").and_then(Value::as_str) {
            parts.push(text.to_string());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_assistant_message_end() {
        let event = json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{ "type": "text", "text": "hi" }]
            }
        });

        assert_eq!(render_event(&event).unwrap(), "assistant:\nhi");
    }

    #[test]
    fn detects_agent_end() {
        assert!(is_agent_end(&json!({ "type": "agent_end" })));
        assert!(!is_agent_end(&json!({ "type": "turn_end" })));
    }
}
