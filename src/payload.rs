use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-5";

const DEFAULT_PROVIDER: &str = "anthropic";
const DEFAULT_SYSTEM_PROMPT: &str = "You are iii-code, a power-user coding agent running on iii workers. Use the available iii functions for coding tasks, keep outputs concise, and preserve durable session context.";

#[derive(Debug, Clone)]
pub struct RunPayloadParams {
    pub session_id: String,
    pub prompt: Option<String>,
    pub provider: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub approval_required: Vec<String>,
    pub image: String,
    pub idle_timeout_secs: u32,
    pub max_turns: u32,
    pub cwd: String,
    pub cwd_hash: String,
}

pub fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn resolve_provider_model(provider: Option<&str>, model: Option<&str>) -> (String, String) {
    let provider = provider.unwrap_or(DEFAULT_PROVIDER).to_string();
    let model = model
        .map(ToString::to_string)
        .unwrap_or_else(|| default_model_for(&provider).to_string());
    (provider, model)
}

pub fn default_model_for(provider: &str) -> &'static str {
    match provider {
        "openai" => DEFAULT_OPENAI_MODEL,
        _ => DEFAULT_ANTHROPIC_MODEL,
    }
}

pub fn current_cwd_metadata() -> Result<(String, String)> {
    let cwd = std::env::current_dir().context("read current directory")?;
    cwd_metadata(&cwd)
}

pub fn cwd_metadata(path: &Path) -> Result<(String, String)> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let display = canonical.display().to_string();
    let mut hasher = Sha256::new();
    hasher.update(display.as_bytes());
    let hash = hex::encode(hasher.finalize());
    Ok((display, hash))
}

pub fn build_run_payload(params: &RunPayloadParams) -> Value {
    let mut messages = Vec::new();
    if let Some(prompt) = &params.prompt {
        messages.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": prompt }],
            "timestamp": now_millis(),
        }));
    }

    json!({
        "session_id": params.session_id,
        "provider": params.provider,
        "model": params.model,
        "system_prompt": params.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT),
        "messages": messages,
        "max_turns": params.max_turns,
        "approval_required": params.approval_required,
        "image": params.image,
        "cwd": params.cwd,
        "cwd_hash": params.cwd_hash,
        "idle_timeout_secs": params.idle_timeout_secs,
    })
}

pub fn build_auth_payload(provider: &str, key: &str) -> Value {
    json!({
        "provider": provider,
        "credential": {
            "type": "api_key",
            "key": key,
        },
    })
}

pub fn build_models_payload(provider: Option<&str>) -> Value {
    match provider {
        Some(provider) => json!({ "provider": provider }),
        None => json!({}),
    }
}

pub fn build_auth_status_payload(provider: &str) -> Value {
    json!({ "provider": provider })
}

pub fn build_stream_list_payload(session_id: &str) -> Value {
    json!({
        "stream_name": "agent::events",
        "group_id": session_id,
    })
}

pub fn build_sessions_payload() -> Value {
    json!({
        "scope": "agent",
        "prefix": "session/",
    })
}

pub fn build_abort_payload(session_id: &str) -> Value {
    json!({ "session_id": session_id })
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_anthropic_sonnet() {
        let (provider, model) = resolve_provider_model(None, None);
        assert_eq!(provider, "anthropic");
        assert_eq!(model, DEFAULT_ANTHROPIC_MODEL);
    }

    #[test]
    fn openai_defaults_to_gpt_5() {
        let (provider, model) = resolve_provider_model(Some("openai"), None);
        assert_eq!(provider, "openai");
        assert_eq!(model, DEFAULT_OPENAI_MODEL);
    }

    #[test]
    fn build_auth_payload_uses_auth_credentials_shape() {
        let payload = build_auth_payload("openai", "test-key");
        assert_eq!(payload["provider"], "openai");
        assert_eq!(payload["credential"]["type"], "api_key");
        assert_eq!(payload["credential"]["key"], "test-key");
    }

    #[test]
    fn build_run_payload_has_user_message_and_cwd_hash() {
        let payload = build_run_payload(&RunPayloadParams {
            session_id: "s1".into(),
            prompt: Some("hello".into()),
            provider: "openai".into(),
            model: "gpt-5".into(),
            system_prompt: None,
            approval_required: vec!["shell::fs::write".into()],
            image: "node".into(),
            idle_timeout_secs: 120,
            max_turns: 3,
            cwd: "/tmp/project".into(),
            cwd_hash: "abc".into(),
        });

        assert_eq!(payload["session_id"], "s1");
        assert_eq!(payload["provider"], "openai");
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][0]["content"][0]["text"], "hello");
        assert_eq!(payload["cwd_hash"], "abc");
        assert_eq!(payload["approval_required"][0], "shell::fs::write");
        assert_eq!(payload["image"], "node");
        assert_eq!(payload["idle_timeout_secs"], 120);
    }

    #[test]
    fn builds_session_helper_payloads() {
        assert_eq!(build_sessions_payload()["scope"], "agent");
        assert_eq!(build_sessions_payload()["prefix"], "session/");
        assert_eq!(build_abort_payload("s1")["session_id"], "s1");
    }
}
