use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-5";

const DEFAULT_PROVIDER: &str = "anthropic";

#[derive(Debug, Clone)]
pub struct RunPayloadParams {
    pub session_id: String,
    pub messages: Vec<Value>,
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

#[derive(Debug, Clone)]
pub struct SessionCompactPayloadParams {
    pub session_id: String,
    pub summary: String,
    pub tokens_before: u64,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub parent_id: Option<String>,
}

pub fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn resolve_provider_model(
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<(String, String)> {
    let provider = provider.unwrap_or(DEFAULT_PROVIDER);
    validate_provider(provider)?;

    let model = match model {
        Some(model) => {
            validate_model_for_provider(provider, model)?;
            model.to_string()
        }
        None => default_model_for(provider)?.to_string(),
    };

    Ok((provider.to_string(), model))
}

pub fn default_model_for(provider: &str) -> Result<&'static str> {
    match provider {
        "openai" => Ok(DEFAULT_OPENAI_MODEL),
        "anthropic" => Ok(DEFAULT_ANTHROPIC_MODEL),
        _ => Err(anyhow!("unknown provider '{provider}'")),
    }
}

fn validate_provider(provider: &str) -> Result<()> {
    match provider {
        "openai" | "anthropic" => Ok(()),
        _ => Err(anyhow!("unknown provider '{provider}'")),
    }
}

fn validate_model_for_provider(provider: &str, model: &str) -> Result<()> {
    let normalized = model.to_ascii_lowercase();
    if provider == "anthropic" && normalized.starts_with("gpt") {
        return Err(anyhow!(
            "model '{model}' is not compatible with provider 'anthropic'"
        ));
    }
    if provider == "openai" && normalized.starts_with("claude-") {
        return Err(anyhow!(
            "model '{model}' is not compatible with provider 'openai'"
        ));
    }
    Ok(())
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
    let mut payload = json!({
        "session_id": params.session_id,
        "provider": params.provider,
        "model": params.model,
        "messages": params.messages,
        "max_turns": params.max_turns,
        "approval_required": params.approval_required,
        "image": params.image,
        "cwd": params.cwd,
        "cwd_hash": params.cwd_hash,
        "idle_timeout_secs": params.idle_timeout_secs,
    });
    if let Some(system_prompt) = params.system_prompt.as_deref().filter(|s| !s.is_empty()) {
        payload["system_prompt"] = json!(system_prompt);
    }
    payload
}

pub fn build_user_message(prompt: &str) -> Value {
    json!({
        "role": "user",
        "content": [{ "type": "text", "text": prompt }],
        "timestamp": now_millis(),
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

pub fn build_sessions_payload(limit: usize) -> Value {
    json!({
        "limit": limit,
        "order": "desc",
    })
}

pub fn build_legacy_sessions_payload() -> Value {
    json!({
        "scope": "agent",
        "prefix": "session/",
    })
}

pub fn build_abort_payload(session_id: &str) -> Value {
    json!({ "session_id": session_id })
}

pub fn build_session_messages_payload(session_id: &str) -> Value {
    json!({ "session_id": session_id })
}

pub fn build_session_fork_payload(session_id: &str, entry_id: &str) -> Value {
    json!({
        "source_session_id": session_id,
        "from_entry_id": entry_id,
    })
}

pub fn build_session_clone_payload(session_id: &str) -> Value {
    json!({
        "source_session_id": session_id,
    })
}

pub fn build_session_tree_payload(session_id: &str) -> Value {
    json!({
        "session_id": session_id,
    })
}

pub fn build_session_export_payload(session_id: &str, branch_leaf: Option<&str>) -> Value {
    let mut payload = json!({
        "session_id": session_id,
    });
    if let Some(branch_leaf) = branch_leaf {
        payload["branch_leaf"] = json!(branch_leaf);
    }
    payload
}

pub fn build_session_compact_payload(params: SessionCompactPayloadParams) -> Value {
    let mut payload = json!({
        "session_id": params.session_id,
        "summary": params.summary,
        "tokens_before": params.tokens_before,
        "details": {
            "read_files": params.read_files,
            "modified_files": params.modified_files,
        },
    });
    if let Some(parent_id) = params.parent_id {
        payload["parent_id"] = json!(parent_id);
    }
    payload
}

pub fn build_session_reconcile_payload(session_id: &str, state_snapshot: Value) -> Value {
    json!({
        "session_id": session_id,
        "state_snapshot": state_snapshot,
    })
}

pub fn build_functions_payload(include_internal: bool) -> Value {
    json!({ "include_internal": include_internal })
}

pub fn build_connected_workers_payload(worker_id: Option<&str>) -> Value {
    match worker_id {
        Some(worker_id) => json!({ "worker_id": worker_id }),
        None => json!({}),
    }
}

pub fn build_state_get_payload(scope: &str, key: &str) -> Value {
    json!({ "scope": scope, "key": key })
}

pub fn build_state_list_payload(scope: &str, prefix: Option<&str>) -> Value {
    let mut payload = json!({ "scope": scope });
    if let Some(prefix) = prefix {
        payload["prefix"] = json!(prefix);
    }
    payload
}

pub fn build_state_set_payload(scope: &str, key: &str, value: Value) -> Value {
    json!({ "scope": scope, "key": key, "value": value })
}

pub fn build_approval_list_payload(session_id: Option<&str>) -> Value {
    match session_id {
        Some(session_id) => json!({ "session_id": session_id }),
        None => json!({}),
    }
}

pub fn build_approval_resolve_payload(
    session_id: &str,
    function_call_id: &str,
    decision: &str,
    reason: Option<&str>,
) -> Value {
    let mut payload = json!({
        "session_id": session_id,
        "function_call_id": function_call_id,
        "decision": decision,
    });
    if let Some(reason) = reason {
        payload["reason"] = json!(reason);
    }
    payload
}

pub fn build_stream_list_payload_for(stream_name: &str, group_id: Option<&str>) -> Value {
    let mut payload = json!({ "stream_name": stream_name });
    if let Some(group_id) = group_id {
        payload["group_id"] = json!(group_id);
    }
    payload
}

#[derive(Debug, Clone)]
pub struct SandboxCreatePayloadParams {
    pub image: String,
    pub name: Option<String>,
    pub network: bool,
    pub idle_timeout_secs: Option<u32>,
    pub cpus: Option<u32>,
    pub memory_mb: Option<u32>,
}

pub fn build_sandbox_create_payload(params: SandboxCreatePayloadParams) -> Value {
    let mut payload = json!({
        "image": params.image,
        "network": params.network,
    });
    if let Some(name) = params.name {
        payload["name"] = json!(name);
    }
    if let Some(idle_timeout_secs) = params.idle_timeout_secs {
        payload["idle_timeout_secs"] = json!(idle_timeout_secs);
    }
    if let Some(cpus) = params.cpus {
        payload["cpus"] = json!(cpus);
    }
    if let Some(memory_mb) = params.memory_mb {
        payload["memory_mb"] = json!(memory_mb);
    }
    payload
}

pub fn build_sandbox_exec_payload(
    sandbox_id: &str,
    cmd: &str,
    args: Vec<String>,
    timeout_ms: u64,
    workdir: Option<&str>,
) -> Value {
    let mut payload = json!({
        "sandbox_id": sandbox_id,
        "cmd": cmd,
        "args": args,
        "timeout_ms": timeout_ms,
    });
    if let Some(workdir) = workdir {
        payload["workdir"] = json!(workdir);
    }
    payload
}

pub fn build_sandbox_stop_payload(sandbox_id: &str, wait: bool) -> Value {
    json!({ "sandbox_id": sandbox_id, "wait": wait })
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
        let (provider, model) = resolve_provider_model(None, None).unwrap();
        assert_eq!(provider, "anthropic");
        assert_eq!(model, DEFAULT_ANTHROPIC_MODEL);
    }

    #[test]
    fn openai_defaults_to_gpt_5() {
        let (provider, model) = resolve_provider_model(Some("openai"), None).unwrap();
        assert_eq!(provider, "openai");
        assert_eq!(model, DEFAULT_OPENAI_MODEL);
    }

    #[test]
    fn rejects_unknown_provider() {
        let err = resolve_provider_model(Some("bedrock"), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown provider"));
    }

    #[test]
    fn rejects_provider_model_mismatch() {
        let anthropic_err = resolve_provider_model(Some("anthropic"), Some("gpt-5"))
            .unwrap_err()
            .to_string();
        assert!(anthropic_err.contains("not compatible"));

        let openai_err = resolve_provider_model(Some("openai"), Some("claude-sonnet-4-6"))
            .unwrap_err()
            .to_string();
        assert!(openai_err.contains("not compatible"));
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
            messages: vec![build_user_message("hello")],
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
        assert!(payload.get("system_prompt").is_none());
        assert_eq!(payload["approval_required"][0], "shell::fs::write");
        assert_eq!(payload["image"], "node");
        assert_eq!(payload["idle_timeout_secs"], 120);
    }

    #[test]
    fn build_run_payload_preserves_system_prompt_override() {
        let payload = build_run_payload(&RunPayloadParams {
            session_id: "s1".into(),
            messages: vec![],
            provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            system_prompt: Some("custom".into()),
            approval_required: vec![],
            image: "python".into(),
            idle_timeout_secs: 120,
            max_turns: 3,
            cwd: "/tmp/project".into(),
            cwd_hash: "abc".into(),
        });

        assert_eq!(payload["system_prompt"], "custom");
    }

    #[test]
    fn builds_session_helper_payloads() {
        assert_eq!(build_sessions_payload(20)["order"], "desc");
        assert_eq!(build_legacy_sessions_payload()["scope"], "agent");
        assert_eq!(build_legacy_sessions_payload()["prefix"], "session/");
        assert_eq!(build_abort_payload("s1")["session_id"], "s1");
        assert_eq!(build_session_messages_payload("s1")["session_id"], "s1");
        assert_eq!(
            build_session_fork_payload("s1", "e1")["from_entry_id"],
            "e1"
        );
        assert_eq!(build_session_clone_payload("s1")["source_session_id"], "s1");
        assert_eq!(build_session_tree_payload("s1")["session_id"], "s1");
        assert_eq!(
            build_session_export_payload("s1", Some("leaf"))["branch_leaf"],
            "leaf"
        );
        let compact = build_session_compact_payload(SessionCompactPayloadParams {
            session_id: "s1".into(),
            summary: "checkpoint".into(),
            tokens_before: 10,
            read_files: vec!["a".into()],
            modified_files: vec!["b".into()],
            parent_id: Some("p1".into()),
        });
        assert_eq!(compact["summary"], "checkpoint");
        assert_eq!(compact["details"]["read_files"][0], "a");
        assert_eq!(compact["parent_id"], "p1");
        assert_eq!(
            build_session_reconcile_payload("s1", json!([]))["state_snapshot"],
            json!([])
        );
    }

    #[test]
    fn builds_worker_state_approval_and_sandbox_payloads() {
        assert_eq!(build_functions_payload(true)["include_internal"], true);
        assert_eq!(
            build_connected_workers_payload(Some("w1"))["worker_id"],
            "w1"
        );
        assert_eq!(build_state_get_payload("s", "k")["key"], "k");
        assert_eq!(build_state_list_payload("s", Some("p"))["prefix"], "p");
        assert_eq!(build_state_set_payload("s", "k", json!(1))["value"], 1);
        assert_eq!(
            build_approval_resolve_payload("s", "fc", "deny", Some("no"))["reason"],
            "no"
        );
        assert_eq!(
            build_stream_list_payload_for("agent::events", Some("s1"))["group_id"],
            "s1"
        );
        assert_eq!(
            build_sandbox_create_payload(SandboxCreatePayloadParams {
                image: "node".into(),
                name: Some("job".into()),
                network: true,
                idle_timeout_secs: Some(10),
                cpus: Some(2),
                memory_mb: Some(1024),
            })["image"],
            "node"
        );
        assert_eq!(
            build_sandbox_exec_payload("sb", "npm", vec!["test".into()], 30_000, Some("/repo"))["cmd"],
            "npm"
        );
        assert_eq!(build_sandbox_stop_payload("sb", true)["wait"], true);
    }
}
