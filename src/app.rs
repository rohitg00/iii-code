use std::env;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::{
    AbortArgs, Cli, Command, ModelsArgs, ResumeArgs, RunArgs, SessionsArgs, SetupArgs,
};
use crate::events::{is_agent_end, normalize_stream_item, render_event, render_final_messages};
use crate::iii::{CommandRunner, IiiClient};
use crate::payload::{
    RunPayloadParams, build_abort_payload, build_auth_payload, build_auth_status_payload,
    build_models_payload, build_run_payload, build_sessions_payload, build_stream_list_payload,
    current_cwd_metadata, new_session_id, resolve_provider_model,
};

const DOCTOR_PROBE_TIMEOUT_MS: u64 = 1_000;

pub fn run<R: CommandRunner, W: Write>(cli: Cli, runner: R, out: &mut W) -> Result<()> {
    let client = IiiClient::new(runner, cli.address, cli.port);
    match cli.command {
        Command::Setup(args) => setup(&client, args, out),
        Command::Run(args) => run_session(&client, args, out),
        Command::Resume(args) => resume_session(&client, args, out),
        Command::Sessions(args) => sessions(&client, args, out),
        Command::Abort(args) => abort_session(&client, args, out),
        Command::Doctor => doctor(&client, out),
        Command::Models(args) => models(&client, args, out),
    }
}

fn setup<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: SetupArgs,
    out: &mut W,
) -> Result<()> {
    let version = client.version().context("verify iii CLI")?;
    writeln!(out, "iii {version}")?;

    if !args.skip_worker_add {
        writeln!(out, "installing harness worker stack")?;
        let install = match client.worker_add_harness() {
            Ok(install) => install,
            Err(err) => {
                writeln!(out, "harness install failed; installing core worker stack")?;
                writeln!(out, "{err}")?;
                client
                    .worker_add_core()
                    .context("install core worker stack fallback")?
            }
        };
        if !install.trim().is_empty() {
            writeln!(out, "{install}")?;
        }
    }

    let openai = credential("OPENAI_API_KEY", args.ignore_env_credentials);
    let anthropic = credential("ANTHROPIC_API_KEY", args.ignore_env_credentials);

    if let Some(key) = openai {
        client
            .trigger(
                "auth::set_token",
                build_auth_payload("openai", &key),
                30_000,
            )
            .context("store OpenAI credential")?;
        writeln!(out, "stored OpenAI credential")?;
    }
    if let Some(key) = anthropic {
        client
            .trigger(
                "auth::set_token",
                build_auth_payload("anthropic", &key),
                30_000,
            )
            .context("store Anthropic credential")?;
        writeln!(out, "stored Anthropic credential")?;
    }

    if !args.no_health_check {
        health_probe(client, out)?;
    }

    Ok(())
}

fn run_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: RunArgs,
    out: &mut W,
) -> Result<()> {
    let session_id = new_session_id();
    let (provider, model) = resolve_provider_model(args.provider.as_deref(), args.model.as_deref())
        .context("resolve provider/model")?;
    let (cwd, cwd_hash) = current_cwd_metadata()?;
    let payload = build_run_payload(&RunPayloadParams {
        session_id: session_id.clone(),
        prompt: Some(args.prompt),
        provider,
        model,
        system_prompt: args.system_prompt,
        approval_required: args.approval_required,
        image: args.image,
        idle_timeout_secs: args.idle_timeout_secs,
        max_turns: args.max_turns,
        cwd,
        cwd_hash,
    });

    execute_run(
        client,
        &session_id,
        payload,
        args.wait,
        args.poll_interval_ms,
        args.stream_timeout_ms,
        out,
    )
}

fn resume_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: ResumeArgs,
    out: &mut W,
) -> Result<()> {
    let (provider, model) = resolve_provider_model(args.provider.as_deref(), args.model.as_deref())
        .context("resolve provider/model")?;
    let (cwd, cwd_hash) = current_cwd_metadata()?;
    let payload = build_run_payload(&RunPayloadParams {
        session_id: args.session_id.clone(),
        prompt: None,
        provider,
        model,
        system_prompt: args.system_prompt,
        approval_required: args.approval_required,
        image: args.image,
        idle_timeout_secs: args.idle_timeout_secs,
        max_turns: args.max_turns,
        cwd,
        cwd_hash,
    });

    execute_run(
        client,
        &args.session_id,
        payload,
        args.wait,
        args.poll_interval_ms,
        args.stream_timeout_ms,
        out,
    )
}

fn execute_run<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    session_id: &str,
    payload: Value,
    wait: bool,
    poll_interval_ms: u64,
    stream_timeout_ms: u64,
    out: &mut W,
) -> Result<()> {
    if wait {
        let mut payload = payload;
        payload["timeout_ms"] = json!(stream_timeout_ms);
        let result = client
            .trigger("run::start_and_wait", payload, stream_timeout_ms)
            .context("run session and wait")?;
        writeln!(out, "session: {session_id}")?;
        for text in render_final_messages(&result) {
            writeln!(out, "assistant:\n{text}")?;
        }
        return Ok(());
    }

    client
        .trigger("run::start", payload, 30_000)
        .context("start session")?;
    writeln!(out, "session: {session_id}")?;
    stream_events(client, session_id, poll_interval_ms, stream_timeout_ms, out)
}

fn stream_events<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    session_id: &str,
    poll_interval_ms: u64,
    stream_timeout_ms: u64,
    out: &mut W,
) -> Result<()> {
    let started = Instant::now();
    let interval = Duration::from_millis(poll_interval_ms);
    let mut seen = 0usize;

    loop {
        let value = client
            .trigger(
                "stream::list",
                build_stream_list_payload(session_id),
                poll_interval_ms.max(5_000),
            )
            .context("poll agent events")?;
        let items = value.as_array().cloned().unwrap_or_default();
        let mut terminal = false;

        for item in items.iter().skip(seen) {
            let event = normalize_stream_item(item);
            if let Some(line) = render_event(&event) {
                writeln!(out, "{line}")?;
            }
            if is_agent_end(&event) {
                terminal = true;
            }
        }
        seen = items.len();

        if terminal {
            break;
        }
        if started.elapsed() >= Duration::from_millis(stream_timeout_ms) {
            writeln!(
                out,
                "stream timeout reached; resume with: iii-code resume {session_id}"
            )?;
            break;
        }
        sleep(interval);
    }

    Ok(())
}

fn sessions<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: SessionsArgs,
    out: &mut W,
) -> Result<()> {
    let value = client
        .trigger("state::list", build_sessions_payload(), 5_000)
        .context("list persisted run sessions")?;
    print_sessions(&value, args.limit, out)
}

fn abort_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: AbortArgs,
    out: &mut W,
) -> Result<()> {
    client
        .trigger(
            "router::abort",
            build_abort_payload(&args.session_id),
            5_000,
        )
        .context("abort session")?;
    writeln!(out, "aborted: {}", args.session_id)?;
    Ok(())
}

fn doctor<R: CommandRunner, W: Write>(client: &IiiClient<R>, out: &mut W) -> Result<()> {
    writeln!(
        out,
        "iii: {}",
        client.version().unwrap_or_else(|e| format!("error: {e}"))
    )?;
    writeln!(
        out,
        "workers:\n{}",
        client
            .worker_list()
            .unwrap_or_else(|e| format!("error: {e}"))
            .trim()
    )?;
    let mut failures = Vec::new();
    if let Some(failure) = report_probe(client, out, "harness", "harness::status", json!({}))? {
        failures.push(failure);
    }
    if let Some(failure) = report_probe(client, out, "models", "models::list", json!({}))? {
        failures.push(failure);
    }
    if let Some(failure) = report_probe(
        client,
        out,
        "openai auth",
        "auth::status",
        build_auth_status_payload("openai"),
    )? {
        failures.push(failure);
    }
    if let Some(failure) = report_probe(
        client,
        out,
        "anthropic auth",
        "auth::status",
        build_auth_status_payload("anthropic"),
    )? {
        failures.push(failure);
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "doctor probes failed: {}",
            format_probe_failures(&failures)
        ))
    }
}

fn health_probe<R: CommandRunner, W: Write>(client: &IiiClient<R>, out: &mut W) -> Result<()> {
    writeln!(out, "health checks")?;
    require_probe(client, out, "harness::status", "harness::status", json!({}))?;
    trigger_with_retry(client, "models::list", json!({}), 5_000, "models::list")?;
    writeln!(out, "ok models::list")?;
    for provider in ["openai", "anthropic"] {
        let value = trigger_with_retry(
            client,
            "auth::status",
            build_auth_status_payload(provider),
            5_000,
            &format!("auth::status for {provider}"),
        )?;
        if let Some(failure) =
            probe_failure_from_value(&format!("auth::status {provider}"), "auth::status", &value)
        {
            return Err(anyhow!("{} failed: {}", failure.label, failure.error));
        }
        writeln!(out, "ok auth::status {provider}")?;
    }
    Ok(())
}

fn trigger_with_retry<R: CommandRunner>(
    client: &IiiClient<R>,
    function_id: &str,
    payload: Value,
    timeout_ms: u64,
    label: &str,
) -> Result<Value> {
    let started = Instant::now();
    let mut last_error = None;

    while started.elapsed() < Duration::from_secs(30) {
        match client.trigger(function_id, payload.clone(), timeout_ms) {
            Ok(value) => return Ok(value),
            Err(err) => {
                last_error = Some(err);
                sleep(Duration::from_millis(500));
            }
        }
    }

    match last_error {
        Some(err) => Err(err).with_context(|| label.to_string()),
        None => client
            .trigger(function_id, payload, timeout_ms)
            .with_context(|| label.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeFailure {
    label: String,
    error: String,
}

impl ProbeFailure {
    fn summary(&self) -> String {
        format!("{}: {}", self.label, self.error)
    }
}

fn require_probe<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    out: &mut W,
    label: &str,
    function_id: &str,
    payload: Value,
) -> Result<()> {
    if let Some(failure) = report_probe(client, out, label, function_id, payload)? {
        Err(anyhow!("{} failed: {}", failure.label, failure.error))
    } else {
        Ok(())
    }
}

fn report_probe<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    out: &mut W,
    label: &str,
    function_id: &str,
    payload: Value,
) -> Result<Option<ProbeFailure>> {
    match client.trigger(function_id, payload, DOCTOR_PROBE_TIMEOUT_MS) {
        Ok(value) => {
            if let Some(failure) = probe_failure_from_value(label, function_id, &value) {
                writeln!(out, "{label}: error: {}", failure.error)?;
                return Ok(Some(failure));
            }
            writeln!(out, "{label}: ok")?;
            Ok(None)
        }
        Err(err) => {
            let error = err.to_string();
            writeln!(out, "{label}: error: {error}")?;
            Ok(Some(ProbeFailure {
                label: label.to_string(),
                error,
            }))
        }
    }
}

fn probe_failure_from_value(label: &str, function_id: &str, value: &Value) -> Option<ProbeFailure> {
    if function_id == "auth::status"
        && value.get("configured").and_then(Value::as_bool) == Some(false)
    {
        Some(ProbeFailure {
            label: label.to_string(),
            error: "not configured".to_string(),
        })
    } else {
        None
    }
}

fn format_probe_failures(failures: &[ProbeFailure]) -> String {
    failures
        .iter()
        .map(ProbeFailure::summary)
        .collect::<Vec<_>>()
        .join("; ")
}

fn print_sessions<W: Write>(value: &Value, limit: usize, out: &mut W) -> Result<()> {
    let mut rows = value
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let session_id = item.get("session_id").and_then(Value::as_str)?;
            let state = item
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let turn_count = item.get("turn_count").and_then(Value::as_u64).unwrap_or(0);
            let updated_at_ms = item
                .get("updated_at_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Some((
                session_id.to_string(),
                state.to_string(),
                turn_count,
                updated_at_ms,
            ))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| std::cmp::Reverse(row.3));

    for (session_id, state, turn_count, updated_at_ms) in rows.into_iter().take(limit) {
        writeln!(
            out,
            "{session_id}\t{state}\tturns={turn_count}\tupdated={updated_at_ms}"
        )?;
    }
    Ok(())
}

fn models<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: ModelsArgs,
    out: &mut W,
) -> Result<()> {
    let value = client
        .trigger(
            "models::list",
            build_models_payload(args.provider.as_deref()),
            5_000,
        )
        .context("list models")?;
    print_models(&value, out)
}

fn print_models<W: Write>(value: &Value, out: &mut W) -> Result<()> {
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = models
        .iter()
        .map(|model| {
            let provider = model
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let id = model.get("id").and_then(Value::as_str).unwrap_or("unknown");
            (provider.to_string(), id.to_string())
        })
        .collect::<Vec<_>>();
    rows.sort();

    let mut last_provider = String::new();
    for (provider, id) in rows {
        if provider != last_provider {
            last_provider = provider.clone();
            writeln!(out, "{provider}")?;
        }
        writeln!(out, "  {id}")?;
    }
    Ok(())
}

fn credential(env_name: &str, ignore_env: bool) -> Option<String> {
    if ignore_env {
        None
    } else {
        env::var(env_name).ok().filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::iii::CommandOutput;
    use crate::iii::tests::MockRunner;
    use clap::Parser;

    #[test]
    fn setup_uses_worker_add_without_argv_credentials() {
        let runner = MockRunner::new(vec![
            MockRunner::ok("0.11.6\n"),
            MockRunner::ok("installed harness\n"),
        ]);
        let cli = Cli::try_parse_from([
            "iii-code",
            "setup",
            "--no-health-check",
            "--ignore-env-credentials",
        ])
        .unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert_eq!(calls[0], vec!["--version".to_string()]);
        assert_eq!(
            calls[1],
            vec![
                "worker".to_string(),
                "add".to_string(),
                "harness".to_string()
            ]
        );
        assert!(
            !calls
                .iter()
                .any(|call| call.contains(&"auth::set_token".to_string()))
        );
    }

    #[test]
    fn setup_falls_back_to_core_when_harness_add_fails() {
        let runner = MockRunner::new(vec![
            MockRunner::ok("0.11.6\n"),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "checksum mismatch".into(),
            },
            MockRunner::ok("core installed\n"),
        ]);
        let cli = Cli::try_parse_from([
            "iii-code",
            "setup",
            "--no-health-check",
            "--ignore-env-credentials",
        ])
        .unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert_eq!(
            calls[1],
            vec![
                "worker".to_string(),
                "add".to_string(),
                "harness".to_string()
            ]
        );
        assert_eq!(calls.len(), 3);
        assert!(calls[2].contains(&"--no-wait".to_string()));
        assert!(calls[2].contains(&"turn-orchestrator".to_string()));
        assert!(calls[2].contains(&"provider-openai".to_string()));

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("harness install failed"));
        assert!(text.contains("checksum mismatch"));
        assert!(text.contains("core installed"));
    }

    #[test]
    fn setup_returns_error_when_core_fallback_fails() {
        let runner = MockRunner::new(vec![
            MockRunner::ok("0.11.6\n"),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "harness mismatch".into(),
            },
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "core failed".into(),
            },
        ]);
        let cli = Cli::try_parse_from([
            "iii-code",
            "setup",
            "--no-health-check",
            "--ignore-env-credentials",
        ])
        .unwrap();
        let mut out = Vec::new();

        let err = run(cli, &runner, &mut out).unwrap_err();
        let details = format!("{err:#}");

        assert!(details.contains("install core worker stack fallback"));
        assert!(details.contains("core failed"));
    }

    #[test]
    fn doctor_reports_all_probes_and_fails_on_probe_error() {
        let runner = MockRunner::new(vec![
            MockRunner::ok("0.11.6\n"),
            MockRunner::ok("harness ready\n"),
            MockRunner::ok(r#"{"ok":true}"#),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "models down".into(),
            },
            MockRunner::ok(r#"{"configured":true}"#),
            MockRunner::ok(r#"{"configured":false}"#),
        ]);
        let cli = Cli::try_parse_from(["iii-code", "doctor"]).unwrap();
        let mut out = Vec::new();

        let err = run(cli, &runner, &mut out).unwrap_err().to_string();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("harness: ok"));
        assert!(text.contains("models: error"));
        assert!(text.contains("openai auth: ok"));
        assert!(text.contains("anthropic auth: error: not configured"));
        assert!(err.contains("doctor probes failed"));
        assert!(err.contains("models"));
        assert!(err.contains("anthropic auth"));
        assert!(err.contains("not configured"));
    }

    #[test]
    fn health_probe_fails_when_harness_probe_fails() {
        let runner = MockRunner::new(vec![CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: "missing harness".into(),
        }]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);
        let mut out = Vec::new();

        let err = health_probe(&client, &mut out).unwrap_err().to_string();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("harness::status: error"));
        assert!(err.contains("harness::status failed"));
    }

    #[test]
    fn health_probe_fails_when_auth_status_is_unconfigured() {
        let runner = MockRunner::new(vec![
            MockRunner::ok(r#"{"ok":true}"#),
            MockRunner::ok(r#"{"models":[]}"#),
            MockRunner::ok(r#"{"configured":false}"#),
        ]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);
        let mut out = Vec::new();

        let err = health_probe(&client, &mut out).unwrap_err().to_string();

        assert!(err.contains("auth::status openai failed"));
        assert!(err.contains("not configured"));
    }

    #[test]
    fn prints_sessions_from_agent_state() {
        let value = json!([
            {"session_id":"old","state":"stopped","turn_count":1,"updated_at_ms":1},
            ["not a session"],
            {"session_id":"new","state":"running","turn_count":3,"updated_at_ms":9}
        ]);
        let mut out = Vec::new();

        print_sessions(&value, 10, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.lines().next().unwrap().starts_with("new\trunning"));
        assert!(text.contains("old\tstopped"));
    }

    #[test]
    fn abort_calls_provider_router() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"{"ok":true}"#)]);
        let cli = Cli::try_parse_from(["iii-code", "abort", "s1"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"router::abort".to_string()));
        assert!(calls[0].contains(&json!({"session_id":"s1"}).to_string()));
    }
}
