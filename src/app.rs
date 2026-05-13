use std::env;
use std::fs;
use std::io::{BufRead, Write};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::{
    AbortArgs, ApprovalDenyArgs, ApprovalResolveArgs, ApprovalsArgs, ApprovalsCommand,
    ApprovalsListArgs, CallArgs, ChatArgs, Cli, Command, ForkArgs, FunctionsArgs, MessagesArgs,
    ModelsArgs, RepairArgs, ResumeArgs, RunArgs, SandboxArgs, SandboxCommand, SandboxCreateArgs,
    SandboxExecArgs, SandboxStopArgs, SessionsArgs, SetupArgs, StateArgs, StateCommand,
    StateDeleteArgs, StateGetArgs, StateListArgs, StateSetArgs, StreamArgs, StreamCommand,
    WorkersArgs,
};
use crate::events::{is_agent_end, normalize_stream_item, render_event, render_final_messages};
use crate::iii::{CommandRunner, IiiClient};
use crate::payload::{
    RunPayloadParams, SandboxCreatePayloadParams, build_abort_payload, build_approval_list_payload,
    build_approval_resolve_payload, build_auth_payload, build_auth_status_payload,
    build_connected_workers_payload, build_functions_payload, build_legacy_sessions_payload,
    build_models_payload, build_run_payload, build_sandbox_create_payload,
    build_sandbox_exec_payload, build_sandbox_stop_payload, build_session_fork_payload,
    build_session_messages_payload, build_session_reconcile_payload, build_sessions_payload,
    build_state_get_payload, build_state_list_payload, build_state_set_payload,
    build_stream_list_payload, build_stream_list_payload_for, build_user_message,
    current_cwd_metadata, new_session_id, resolve_provider_model,
};

const DOCTOR_PROBE_TIMEOUT_MS: u64 = 1_000;
const CORE_RUNTIME_FUNCTIONS: &[&str] = &[
    "run::start",
    "run::start_and_wait",
    "models::list",
    "auth::status",
    "session-tree::list",
    "session-tree::messages",
    "stream::list",
    "router::abort",
    "shell::exec",
    "approval::list_pending",
    "sandbox::create",
];
const AUTH_PROVIDERS: &[&str] = &["openai", "anthropic"];

#[cfg(test)]
pub fn run<R: CommandRunner, W: Write>(cli: Cli, runner: R, out: &mut W) -> Result<()> {
    run_with_input(cli, runner, std::io::empty(), out)
}

pub fn run_with_input<R: CommandRunner, I: BufRead, W: Write>(
    cli: Cli,
    runner: R,
    input: I,
    out: &mut W,
) -> Result<()> {
    let client = IiiClient::new(runner, cli.address, cli.port);
    match cli.command {
        None => chat(&client, ChatArgs::default(), input, out),
        Some(Command::Chat(args)) => chat(&client, args, input, out),
        Some(Command::Setup(args)) => setup(&client, args, out),
        Some(Command::Run(args)) => run_session(&client, args, out),
        Some(Command::Resume(args)) => resume_session(&client, args, out),
        Some(Command::Sessions(args)) => sessions(&client, args, out),
        Some(Command::Messages(args)) => messages(&client, args, out),
        Some(Command::Fork(args)) => fork_session(&client, args, out),
        Some(Command::Repair(args)) => repair_session(&client, args, out),
        Some(Command::Abort(args)) => abort_session(&client, args, out),
        Some(Command::Doctor) => doctor(&client, out),
        Some(Command::Models(args)) => models(&client, args, out),
        Some(Command::Workers(args)) => workers(&client, args, out),
        Some(Command::Functions(args)) => functions(&client, args, out),
        Some(Command::Call(args)) => call_function(&client, args, out),
        Some(Command::State(args)) => state(&client, args, out),
        Some(Command::Stream(args)) => stream(&client, args, out),
        Some(Command::Approvals(args)) => approvals(&client, args, out),
        Some(Command::Sandbox(args)) => sandbox(&client, args, out),
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
    let config = RunConfig::from_run_args(&args)?;
    start_session(
        client,
        &session_id,
        vec![build_user_message(&args.prompt)],
        &config,
        out,
    )
}

fn resume_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: ResumeArgs,
    out: &mut W,
) -> Result<()> {
    let config = RunConfig::from_resume_args(&args)?;
    let mut messages = load_session_messages(client, &args.session_id)
        .with_context(|| format!("load session {} transcript", args.session_id))?;
    if let Some(prompt) = &args.prompt {
        messages.push(build_user_message(prompt));
    }
    if messages.is_empty() {
        return Err(anyhow!(
            "no persisted transcript found for {}; pass a prompt or repair the session tree",
            args.session_id
        ));
    }
    start_session(client, &args.session_id, messages, &config, out)
}

#[derive(Debug, Clone)]
struct RunConfig {
    provider: String,
    model: String,
    system_prompt: Option<String>,
    approval_required: Vec<String>,
    image: String,
    idle_timeout_secs: u32,
    max_turns: u32,
    wait: bool,
    poll_interval_ms: u64,
    stream_timeout_ms: u64,
}

impl RunConfig {
    fn from_run_args(args: &RunArgs) -> Result<Self> {
        Self::new(
            args.provider.as_deref(),
            args.model.as_deref(),
            args.system_prompt.clone(),
            args.approval_required.clone(),
            args.image.clone(),
            args.idle_timeout_secs,
            args.max_turns,
            args.wait,
            args.poll_interval_ms,
            args.stream_timeout_ms,
        )
    }

    fn from_resume_args(args: &ResumeArgs) -> Result<Self> {
        Self::new(
            args.provider.as_deref(),
            args.model.as_deref(),
            args.system_prompt.clone(),
            args.approval_required.clone(),
            args.image.clone(),
            args.idle_timeout_secs,
            args.max_turns,
            args.wait,
            args.poll_interval_ms,
            args.stream_timeout_ms,
        )
    }

    fn from_chat_args(args: &ChatArgs) -> Result<Self> {
        Self::new(
            args.provider.as_deref(),
            args.model.as_deref(),
            args.system_prompt.clone(),
            args.approval_required.clone(),
            args.image.clone(),
            args.idle_timeout_secs,
            args.max_turns,
            args.wait,
            args.poll_interval_ms,
            args.stream_timeout_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        provider: Option<&str>,
        model: Option<&str>,
        system_prompt: Option<String>,
        approval_required: Vec<String>,
        image: String,
        idle_timeout_secs: u32,
        max_turns: u32,
        wait: bool,
        poll_interval_ms: u64,
        stream_timeout_ms: u64,
    ) -> Result<Self> {
        let (provider, model) =
            resolve_provider_model(provider, model).context("resolve provider/model")?;
        Ok(Self {
            provider,
            model,
            system_prompt,
            approval_required,
            image,
            idle_timeout_secs,
            max_turns,
            wait,
            poll_interval_ms,
            stream_timeout_ms,
        })
    }
}

fn start_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    session_id: &str,
    messages: Vec<Value>,
    config: &RunConfig,
    out: &mut W,
) -> Result<()> {
    let (cwd, cwd_hash) = current_cwd_metadata()?;
    let payload = build_run_payload(&RunPayloadParams {
        session_id: session_id.to_string(),
        messages,
        provider: config.provider.clone(),
        model: config.model.clone(),
        system_prompt: config.system_prompt.clone(),
        approval_required: config.approval_required.clone(),
        image: config.image.clone(),
        idle_timeout_secs: config.idle_timeout_secs,
        max_turns: config.max_turns,
        cwd,
        cwd_hash,
    });

    execute_run(
        client,
        session_id,
        payload,
        config.wait,
        config.poll_interval_ms,
        config.stream_timeout_ms,
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

fn chat<R: CommandRunner, I: BufRead, W: Write>(
    client: &IiiClient<R>,
    args: ChatArgs,
    mut input: I,
    out: &mut W,
) -> Result<()> {
    let config = RunConfig::from_chat_args(&args)?;
    let mut session_id = args.session_id.unwrap_or_else(new_session_id);

    writeln!(out, "iii-code")?;
    writeln!(out, "session: {session_id}")?;
    writeln!(out, "model: {}/{}", config.provider, config.model)?;
    writeln!(out, "sandbox: {}", config.image)?;
    writeln!(out, "type /help for commands, /quit to exit")?;

    if let Some(prompt) = args.prompt {
        send_chat_prompt(client, &session_id, &prompt, &config, out)?;
    }

    let mut line = String::new();
    loop {
        write!(out, "iii-code> ")?;
        out.flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if let Some(next_session_id) = handle_chat_command(client, text, &session_id, out)? {
            if next_session_id == "__quit__" {
                break;
            }
            session_id = next_session_id;
            continue;
        }
        send_chat_prompt(client, &session_id, text, &config, out)?;
    }

    writeln!(out, "session: {session_id}")?;
    Ok(())
}

fn send_chat_prompt<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    session_id: &str,
    prompt: &str,
    config: &RunConfig,
    out: &mut W,
) -> Result<()> {
    let mut messages = load_session_messages(client, session_id).unwrap_or_default();
    messages.push(build_user_message(prompt));
    start_session(client, session_id, messages, config, out)
}

fn handle_chat_command<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    text: &str,
    session_id: &str,
    out: &mut W,
) -> Result<Option<String>> {
    if !text.starts_with('/') {
        return Ok(None);
    }

    let without_slash = text.trim_start_matches('/');
    let mut parts = without_slash.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();

    match command {
        "q" | "quit" | "exit" => Ok(Some("__quit__".to_string())),
        "help" => {
            print_chat_help(out)?;
            Ok(Some(session_id.to_string()))
        }
        "new" => {
            let next = new_session_id();
            writeln!(out, "session: {next}")?;
            Ok(Some(next))
        }
        "resume" => {
            if rest.is_empty() {
                writeln!(out, "usage: /resume <session-id>")?;
                return Ok(Some(session_id.to_string()));
            }
            writeln!(out, "session: {rest}")?;
            Ok(Some(rest.to_string()))
        }
        "sessions" => {
            sessions(client, SessionsArgs { limit: 20 }, out)?;
            Ok(Some(session_id.to_string()))
        }
        "messages" => {
            let target = if rest.is_empty() { session_id } else { rest };
            print_transcript(client, target, out)?;
            Ok(Some(session_id.to_string()))
        }
        "models" => {
            models(client, ModelsArgs { provider: None }, out)?;
            Ok(Some(session_id.to_string()))
        }
        "workers" => {
            workers(
                client,
                WorkersArgs {
                    connected: true,
                    worker_id: None,
                },
                out,
            )?;
            Ok(Some(session_id.to_string()))
        }
        "functions" => {
            functions(
                client,
                FunctionsArgs {
                    include_internal: false,
                    filter: if rest.is_empty() {
                        None
                    } else {
                        Some(rest.to_string())
                    },
                },
                out,
            )?;
            Ok(Some(session_id.to_string()))
        }
        "approvals" => {
            approvals(
                client,
                ApprovalsArgs {
                    command: ApprovalsCommand::List(ApprovalsListArgs {
                        session_id: Some(session_id.to_string()),
                    }),
                },
                out,
            )?;
            Ok(Some(session_id.to_string()))
        }
        "allow" => {
            if rest.is_empty() {
                writeln!(out, "usage: /allow <function-call-id>")?;
            } else {
                approvals(
                    client,
                    ApprovalsArgs {
                        command: ApprovalsCommand::Allow(ApprovalResolveArgs {
                            session_id: session_id.to_string(),
                            function_call_id: rest.to_string(),
                        }),
                    },
                    out,
                )?;
            }
            Ok(Some(session_id.to_string()))
        }
        "deny" => {
            let mut deny_parts = rest.splitn(2, char::is_whitespace);
            let call_id = deny_parts.next().unwrap_or("");
            if call_id.is_empty() {
                writeln!(out, "usage: /deny <function-call-id> [reason]")?;
            } else {
                approvals(
                    client,
                    ApprovalsArgs {
                        command: ApprovalsCommand::Deny(ApprovalDenyArgs {
                            session_id: session_id.to_string(),
                            function_call_id: call_id.to_string(),
                            reason: deny_parts
                                .next()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string),
                        }),
                    },
                    out,
                )?;
            }
            Ok(Some(session_id.to_string()))
        }
        "repair" => {
            repair_session(
                client,
                RepairArgs {
                    session_id: session_id.to_string(),
                },
                out,
            )?;
            Ok(Some(session_id.to_string()))
        }
        "fork" => {
            if rest.is_empty() {
                writeln!(out, "usage: /fork <entry-id>")?;
                return Ok(Some(session_id.to_string()));
            }
            let value = client
                .trigger(
                    "session-tree::fork",
                    build_session_fork_payload(session_id, rest),
                    5_000,
                )
                .context("fork session")?;
            let next = value
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            print_json(&value, out)?;
            Ok(Some(next.unwrap_or_else(|| session_id.to_string())))
        }
        "doctor" => {
            doctor(client, out)?;
            Ok(Some(session_id.to_string()))
        }
        _ => {
            writeln!(out, "unknown command: /{command}")?;
            Ok(Some(session_id.to_string()))
        }
    }
}

fn print_chat_help<W: Write>(out: &mut W) -> Result<()> {
    writeln!(out, "/new")?;
    writeln!(out, "/resume <session-id>")?;
    writeln!(out, "/sessions")?;
    writeln!(out, "/messages [session-id]")?;
    writeln!(out, "/functions [filter]")?;
    writeln!(out, "/workers")?;
    writeln!(out, "/models")?;
    writeln!(out, "/approvals")?;
    writeln!(out, "/allow <function-call-id>")?;
    writeln!(out, "/deny <function-call-id> [reason]")?;
    writeln!(out, "/repair")?;
    writeln!(out, "/fork <entry-id>")?;
    writeln!(out, "/doctor")?;
    writeln!(out, "/quit")?;
    Ok(())
}

fn sessions<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: SessionsArgs,
    out: &mut W,
) -> Result<()> {
    let tree_value = client.trigger(
        "session-tree::list",
        build_sessions_payload(args.limit),
        5_000,
    );
    let value = match tree_value {
        Ok(value) if session_tree_is_empty(&value) => client
            .trigger("state::list", build_legacy_sessions_payload(), 5_000)
            .unwrap_or(value),
        Ok(value) => value,
        Err(_) => client
            .trigger("state::list", build_legacy_sessions_payload(), 5_000)
            .context("list legacy persisted run sessions")?,
    };
    print_sessions(&value, args.limit, out)
}

fn session_tree_is_empty(value: &Value) -> bool {
    let total_empty = value.get("total").and_then(Value::as_u64) == Some(0);
    let sessions_empty = value
        .get("sessions")
        .and_then(Value::as_array)
        .is_some_and(|sessions| sessions.is_empty());
    let missing_both = value.get("total").is_none() && value.get("sessions").is_none();
    total_empty || sessions_empty || missing_both
}

fn messages<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: MessagesArgs,
    out: &mut W,
) -> Result<()> {
    if args.raw {
        let value = client
            .trigger(
                "session-tree::messages",
                build_session_messages_payload(&args.session_id),
                5_000,
            )
            .context("load session messages")?;
        return print_json(&value, out);
    }
    print_transcript(client, &args.session_id, out)
}

fn fork_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: ForkArgs,
    out: &mut W,
) -> Result<()> {
    let value = client
        .trigger(
            "session-tree::fork",
            build_session_fork_payload(&args.session_id, &args.entry_id),
            5_000,
        )
        .context("fork session")?;
    print_json(&value, out)
}

fn repair_session<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: RepairArgs,
    out: &mut W,
) -> Result<()> {
    let state_snapshot = client
        .trigger(
            "state::get",
            build_state_get_payload("agent", &format!("session/{}/messages", args.session_id)),
            5_000,
        )
        .context("load legacy session messages")?;
    let value = client
        .trigger(
            "session-tree::reconcile",
            build_session_reconcile_payload(&args.session_id, state_snapshot),
            5_000,
        )
        .context("repair session tree")?;
    print_json(&value, out)
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
    if let Some(failure) = report_harness_or_core(client, out, "harness")? {
        failures.push(failure);
    }
    if let Some(failure) = report_probe(client, out, "models", "models::list", json!({}))? {
        failures.push(failure);
    }
    if let Some(failure) = report_auth_statuses(client, out)? {
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
    if let Some(failure) = report_harness_or_core(client, out, "harness::status")? {
        return Err(anyhow!("{} failed: {}", failure.label, failure.error));
    }
    trigger_with_retry(client, "models::list", json!({}), 5_000, "models::list")?;
    writeln!(out, "ok models::list")?;
    require_any_provider_auth(client, out)?;
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

fn report_auth_statuses<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    out: &mut W,
) -> Result<Option<ProbeFailure>> {
    let mut failures = Vec::new();
    let mut configured = false;

    for provider in AUTH_PROVIDERS {
        let label = format!("{provider} auth");
        match report_probe(
            client,
            out,
            &label,
            "auth::status",
            build_auth_status_payload(provider),
        )? {
            Some(failure) => failures.push(failure),
            None => configured = true,
        }
    }

    if configured {
        Ok(None)
    } else {
        Ok(Some(provider_auth_failure(failures)))
    }
}

fn require_any_provider_auth<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    out: &mut W,
) -> Result<()> {
    let mut failures = Vec::new();
    let mut configured = false;

    for provider in AUTH_PROVIDERS {
        let label = format!("auth::status {provider}");
        let value = trigger_with_retry(
            client,
            "auth::status",
            build_auth_status_payload(provider),
            5_000,
            &format!("auth::status for {provider}"),
        )?;
        if let Some(failure) = probe_failure_from_value(&label, "auth::status", &value) {
            writeln!(out, "{label}: error: {}", failure.error)?;
            failures.push(failure);
        } else {
            configured = true;
            writeln!(out, "ok {label}")?;
        }
    }

    if configured {
        Ok(())
    } else {
        let failure = provider_auth_failure(failures);
        Err(anyhow!("{} failed: {}", failure.label, failure.error))
    }
}

fn provider_auth_failure(failures: Vec<ProbeFailure>) -> ProbeFailure {
    let error = if failures.is_empty() {
        "no configured provider credentials".to_string()
    } else {
        format!(
            "no configured provider credentials ({})",
            format_probe_failures(&failures)
        )
    };
    ProbeFailure {
        label: "provider auth".to_string(),
        error,
    }
}

fn report_harness_or_core<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    out: &mut W,
    harness_label: &str,
) -> Result<Option<ProbeFailure>> {
    let harness_failure =
        match client.trigger("harness::status", json!({}), DOCTOR_PROBE_TIMEOUT_MS) {
            Ok(value) => {
                if let Some(failure) =
                    probe_failure_from_value(harness_label, "harness::status", &value)
                {
                    writeln!(out, "{harness_label}: unavailable: {}", failure.error)?;
                    failure
                } else {
                    writeln!(out, "{harness_label}: ok")?;
                    return Ok(None);
                }
            }
            Err(err) => {
                let error = err.to_string();
                writeln!(out, "{harness_label}: unavailable: {error}")?;
                ProbeFailure {
                    label: harness_label.to_string(),
                    error,
                }
            }
        };

    match client.trigger(
        "engine::functions::list",
        build_functions_payload(false),
        DOCTOR_PROBE_TIMEOUT_MS,
    ) {
        Ok(value) => {
            let missing = missing_core_runtime_functions(&value);
            if missing.is_empty() {
                writeln!(out, "core stack: ok")?;
                Ok(None)
            } else {
                let missing = missing.join(", ");
                writeln!(out, "core stack: error: missing {missing}")?;
                Ok(Some(ProbeFailure {
                    label: "core stack".to_string(),
                    error: format!(
                        "harness unavailable ({}); missing core functions: {missing}",
                        harness_failure.error
                    ),
                }))
            }
        }
        Err(err) => {
            let error = err.to_string();
            writeln!(out, "core stack: error: {error}")?;
            Ok(Some(ProbeFailure {
                label: "core stack".to_string(),
                error: format!(
                    "harness unavailable ({}); core probe failed: {error}",
                    harness_failure.error
                ),
            }))
        }
    }
}

fn missing_core_runtime_functions(value: &Value) -> Vec<&'static str> {
    let ids = function_ids_from_value(value);
    CORE_RUNTIME_FUNCTIONS
        .iter()
        .copied()
        .filter(|required| !ids.iter().any(|id| id == required))
        .collect()
}

fn function_ids_from_value(value: &Value) -> Vec<String> {
    let source = value
        .get("functions")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    source
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            item.get("function_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
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
    let source = value
        .get("sessions")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let mut rows = source
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let session_id = item.get("session_id").and_then(Value::as_str)?;
            let state = item.get("state").and_then(Value::as_str).unwrap_or("tree");
            let turn_count = item
                .get("turn_count")
                .or_else(|| item.get("entry_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let updated_at_ms = item
                .get("updated_at_ms")
                .or_else(|| item.get("updated_at"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let summary = item
                .get("last_message_summary")
                .and_then(Value::as_str)
                .unwrap_or("");
            Some((
                session_id.to_string(),
                state.to_string(),
                turn_count,
                updated_at_ms,
                summary.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| std::cmp::Reverse(row.3));

    for (session_id, state, turn_count, updated_at_ms, summary) in rows.into_iter().take(limit) {
        writeln!(
            out,
            "{session_id}\t{state}\tentries={turn_count}\tupdated={updated_at_ms}\t{summary}"
        )?;
    }
    Ok(())
}

fn load_session_messages<R: CommandRunner>(
    client: &IiiClient<R>,
    session_id: &str,
) -> Result<Vec<Value>> {
    if let Ok(value) = client.trigger(
        "session-tree::messages",
        build_session_messages_payload(session_id),
        5_000,
    ) {
        let messages = extract_session_messages(&value);
        if !messages.is_empty() {
            return Ok(messages);
        }
    }

    let value = client
        .trigger(
            "state::get",
            build_state_get_payload("agent", &format!("session/{session_id}/messages")),
            5_000,
        )
        .context("load legacy session messages")?;
    Ok(extract_session_messages(&value))
}

fn extract_session_messages(value: &Value) -> Vec<Value> {
    let source = value
        .get("messages")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    source
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.get("message").cloned().unwrap_or(item))
        .collect()
}

fn print_transcript<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    session_id: &str,
    out: &mut W,
) -> Result<()> {
    let messages = load_session_messages(client, session_id)?;
    if messages.is_empty() {
        writeln!(out, "no messages for {session_id}")?;
        return Ok(());
    }
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("message");
        if let Some(text) = message_plain_text(&message) {
            writeln!(out, "{role}:\n{text}")?;
        } else {
            writeln!(out, "{role}: {}", serde_json::to_string(&message)?)?;
        }
    }
    Ok(())
}

fn message_plain_text(message: &Value) -> Option<String> {
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

fn workers<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: WorkersArgs,
    out: &mut W,
) -> Result<()> {
    if args.connected {
        let value = client
            .trigger(
                "engine::workers::list",
                build_connected_workers_payload(args.worker_id.as_deref()),
                5_000,
            )
            .context("list connected workers")?;
        print_json(&value, out)
    } else {
        if args.worker_id.is_some() {
            return Err(anyhow!("--worker-id requires --connected"));
        }
        let text = client.worker_list().context("list configured workers")?;
        write!(out, "{text}")?;
        Ok(())
    }
}

fn functions<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: FunctionsArgs,
    out: &mut W,
) -> Result<()> {
    let value = client
        .trigger(
            "engine::functions::list",
            build_functions_payload(args.include_internal),
            5_000,
        )
        .context("list functions")?;
    let value = filter_json_array(value, args.filter.as_deref());
    print_json(&value, out)
}

fn call_function<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: CallArgs,
    out: &mut W,
) -> Result<()> {
    let payload = json_arg(args.payload.as_deref(), args.payload_file.as_ref())?;
    let value = client
        .trigger(&args.function_id, payload, args.timeout_ms)
        .with_context(|| format!("call {}", args.function_id))?;
    print_json(&value, out)
}

fn state<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: StateArgs,
    out: &mut W,
) -> Result<()> {
    let (function_id, payload, timeout_ms) = match args.command {
        StateCommand::Get(args) => state_get(args),
        StateCommand::List(args) => state_list(args),
        StateCommand::Set(args) => state_set(args)?,
        StateCommand::Delete(args) => state_delete(args),
    };
    let value = client
        .trigger(function_id, payload, timeout_ms)
        .with_context(|| format!("call {function_id}"))?;
    print_json(&value, out)
}

fn state_get(args: StateGetArgs) -> (&'static str, Value, u64) {
    (
        "state::get",
        build_state_get_payload(&args.scope, &args.key),
        5_000,
    )
}

fn state_list(args: StateListArgs) -> (&'static str, Value, u64) {
    (
        "state::list",
        build_state_list_payload(&args.scope, args.prefix.as_deref()),
        5_000,
    )
}

fn state_set(args: StateSetArgs) -> Result<(&'static str, Value, u64)> {
    let value = parse_json_value(&args.value).context("parse state value JSON")?;
    Ok((
        "state::set",
        build_state_set_payload(&args.scope, &args.key, value),
        5_000,
    ))
}

fn state_delete(args: StateDeleteArgs) -> (&'static str, Value, u64) {
    (
        "state::delete",
        build_state_get_payload(&args.scope, &args.key),
        5_000,
    )
}

fn stream<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: StreamArgs,
    out: &mut W,
) -> Result<()> {
    match args.command {
        StreamCommand::List(args) => {
            let value = client
                .trigger(
                    "stream::list",
                    build_stream_list_payload_for(&args.stream_name, args.group_id.as_deref()),
                    5_000,
                )
                .context("list stream frames")?;
            print_json(&value, out)
        }
    }
}

fn approvals<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: ApprovalsArgs,
    out: &mut W,
) -> Result<()> {
    let (function_id, payload) = match args.command {
        ApprovalsCommand::List(args) => approval_list(args),
        ApprovalsCommand::Allow(args) => approval_allow(args),
        ApprovalsCommand::Deny(args) => approval_deny(args),
    };
    let value = client
        .trigger(function_id, payload, 5_000)
        .with_context(|| format!("call {function_id}"))?;
    print_json(&value, out)
}

fn approval_list(args: ApprovalsListArgs) -> (&'static str, Value) {
    (
        "approval::list_pending",
        build_approval_list_payload(args.session_id.as_deref()),
    )
}

fn approval_allow(args: ApprovalResolveArgs) -> (&'static str, Value) {
    (
        "approval::resolve",
        build_approval_resolve_payload(&args.session_id, &args.function_call_id, "allow", None),
    )
}

fn approval_deny(args: ApprovalDenyArgs) -> (&'static str, Value) {
    (
        "approval::resolve",
        build_approval_resolve_payload(
            &args.session_id,
            &args.function_call_id,
            "deny",
            args.reason.as_deref(),
        ),
    )
}

fn sandbox<R: CommandRunner, W: Write>(
    client: &IiiClient<R>,
    args: SandboxArgs,
    out: &mut W,
) -> Result<()> {
    let (function_id, payload, timeout_ms) = match args.command {
        SandboxCommand::List => ("sandbox::list", json!({}), 5_000),
        SandboxCommand::Create(args) => sandbox_create(args),
        SandboxCommand::Exec(args) => sandbox_exec(args),
        SandboxCommand::Stop(args) => sandbox_stop(args),
    };
    let value = client
        .trigger(function_id, payload, timeout_ms)
        .with_context(|| format!("call {function_id}"))?;
    print_json(&value, out)
}

fn sandbox_create(args: SandboxCreateArgs) -> (&'static str, Value, u64) {
    (
        "sandbox::create",
        build_sandbox_create_payload(SandboxCreatePayloadParams {
            image: args.image,
            name: args.name,
            network: args.network,
            idle_timeout_secs: args.idle_timeout_secs,
            cpus: args.cpus,
            memory_mb: args.memory_mb,
        }),
        300_000,
    )
}

fn sandbox_exec(args: SandboxExecArgs) -> (&'static str, Value, u64) {
    (
        "sandbox::exec",
        build_sandbox_exec_payload(
            &args.sandbox_id,
            &args.cmd,
            args.args,
            args.timeout_ms,
            args.workdir.as_deref(),
        ),
        args.timeout_ms.saturating_add(5_000),
    )
}

fn sandbox_stop(args: SandboxStopArgs) -> (&'static str, Value, u64) {
    (
        "sandbox::stop",
        build_sandbox_stop_payload(&args.sandbox_id, args.wait),
        30_000,
    )
}

fn json_arg(payload: Option<&str>, payload_file: Option<&std::path::PathBuf>) -> Result<Value> {
    match (payload, payload_file) {
        (Some(payload), None) => parse_json_value(payload).context("parse --payload JSON"),
        (None, Some(path)) => {
            let text = fs::read_to_string(path)
                .with_context(|| format!("read payload file {}", path.display()))?;
            parse_json_value(&text).context("parse --payload-file JSON")
        }
        (None, None) => Ok(json!({})),
        (Some(_), Some(_)) => Err(anyhow!("use --payload or --payload-file, not both")),
    }
}

fn parse_json_value(input: &str) -> Result<Value> {
    serde_json::from_str(input).context("invalid JSON")
}

fn print_json<W: Write>(value: &Value, out: &mut W) -> Result<()> {
    writeln!(out, "{}", serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn filter_json_array(value: Value, filter: Option<&str>) -> Value {
    let Some(filter) = filter else {
        return value;
    };
    let filter = filter.to_ascii_lowercase();
    match value {
        Value::Array(items) => Value::Array(filter_items(items, &filter)),
        Value::Object(mut object) => {
            if let Some(Value::Array(items)) = object.remove("functions") {
                object.insert(
                    "functions".to_string(),
                    Value::Array(filter_items(items, &filter)),
                );
            }
            Value::Object(object)
        }
        other => other,
    }
}

fn filter_items(items: Vec<Value>, filter: &str) -> Vec<Value> {
    items
        .into_iter()
        .filter(|item| item.to_string().to_ascii_lowercase().contains(filter))
        .collect()
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
        assert!(!err.contains("provider auth"));
    }

    #[test]
    fn doctor_accepts_core_stack_when_harness_probe_fails() {
        let runner = MockRunner::new(vec![
            MockRunner::ok("0.11.6\n"),
            MockRunner::ok("core workers running\n"),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "missing harness".into(),
            },
            MockRunner::ok(core_function_list_json()),
            MockRunner::ok(r#"{"models":[]}"#),
            MockRunner::ok(r#"{"configured":true}"#),
            MockRunner::ok(r#"{"configured":true}"#),
        ]);
        let cli = Cli::try_parse_from(["iii-code", "doctor"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("harness: unavailable"));
        assert!(text.contains("core stack: ok"));
        assert!(text.contains("anthropic auth: ok"));
    }

    #[test]
    fn doctor_fails_when_no_provider_auth_is_configured() {
        let runner = MockRunner::new(vec![
            MockRunner::ok("0.11.6\n"),
            MockRunner::ok("harness ready\n"),
            MockRunner::ok(r#"{"ok":true}"#),
            MockRunner::ok(r#"{"models":[]}"#),
            MockRunner::ok(r#"{"configured":false}"#),
            MockRunner::ok(r#"{"configured":false}"#),
        ]);
        let cli = Cli::try_parse_from(["iii-code", "doctor"]).unwrap();
        let mut out = Vec::new();

        let err = run(cli, &runner, &mut out).unwrap_err().to_string();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("openai auth: error: not configured"));
        assert!(text.contains("anthropic auth: error: not configured"));
        assert!(err.contains("provider auth"));
        assert!(err.contains("no configured provider credentials"));
    }

    #[test]
    fn health_probe_accepts_core_stack_when_harness_probe_fails() {
        let runner = MockRunner::new(vec![
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "missing harness".into(),
            },
            MockRunner::ok(core_function_list_json()),
            MockRunner::ok(r#"{"models":[]}"#),
            MockRunner::ok(r#"{"configured":true}"#),
            MockRunner::ok(r#"{"configured":true}"#),
        ]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);
        let mut out = Vec::new();

        health_probe(&client, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("harness::status: unavailable"));
        assert!(text.contains("core stack: ok"));
        assert!(text.contains("ok models::list"));
    }

    #[test]
    fn health_probe_fails_when_harness_and_core_stack_fail() {
        let runner = MockRunner::new(vec![
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "missing harness".into(),
            },
            MockRunner::ok(r#"{"functions":[{"function_id":"models::list"}]}"#),
        ]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);
        let mut out = Vec::new();

        let err = health_probe(&client, &mut out).unwrap_err().to_string();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("core stack: error"));
        assert!(err.contains("core stack failed"));
        assert!(err.contains("missing core functions"));
    }

    #[test]
    fn health_probe_accepts_one_configured_provider() {
        let runner = MockRunner::new(vec![
            MockRunner::ok(r#"{"ok":true}"#),
            MockRunner::ok(r#"{"models":[]}"#),
            MockRunner::ok(r#"{"configured":false}"#),
            MockRunner::ok(r#"{"configured":true}"#),
        ]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);
        let mut out = Vec::new();

        health_probe(&client, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("auth::status openai: error: not configured"));
        assert!(text.contains("ok auth::status anthropic"));
    }

    #[test]
    fn health_probe_fails_when_no_provider_auth_is_configured() {
        let runner = MockRunner::new(vec![
            MockRunner::ok(r#"{"ok":true}"#),
            MockRunner::ok(r#"{"models":[]}"#),
            MockRunner::ok(r#"{"configured":false}"#),
            MockRunner::ok(r#"{"configured":false}"#),
        ]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);
        let mut out = Vec::new();

        let err = health_probe(&client, &mut out).unwrap_err().to_string();

        assert!(err.contains("provider auth failed"));
        assert!(err.contains("no configured provider credentials"));
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

    #[test]
    fn resume_with_prompt_preserves_session_tree_messages() {
        let runner = MockRunner::new(vec![
            MockRunner::ok(
                r#"{"messages":[{"entry_id":"e1","message":{"role":"user","content":[{"type":"text","text":"old"}],"timestamp":1}}]}"#,
            ),
            MockRunner::ok(r#"{"session_id":"s1"}"#),
            MockRunner::ok(r#"[{"data":{"type":"agent_end"}}]"#),
        ]);
        let cli = Cli::try_parse_from(["iii-code", "resume", "s1", "new"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"session-tree::messages".to_string()));
        assert!(calls[1].contains(&"run::start".to_string()));
        let payload = calls[1].join(" ");
        assert!(payload.contains("old"));
        assert!(payload.contains("new"));
    }

    #[test]
    fn default_command_opens_chat_shell() {
        let runner = MockRunner::new(vec![]);
        let cli = Cli::try_parse_from(["iii-code"]).unwrap();
        let mut out = Vec::new();

        run_with_input(cli, &runner, "/quit\n".as_bytes(), &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("iii-code"));
        assert!(text.contains("type /help"));
    }

    #[test]
    fn sessions_falls_back_when_session_tree_is_empty() {
        let runner = MockRunner::new(vec![
            MockRunner::ok(r#"{"sessions":[]}"#),
            MockRunner::ok(
                r#"[{"session_id":"legacy","state":"stopped","turn_count":1,"updated_at_ms":2}]"#,
            ),
        ]);
        let cli = Cli::try_parse_from(["iii-code", "sessions"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("legacy"));
    }

    #[test]
    fn session_tree_empty_handles_missing_fields() {
        assert!(session_tree_is_empty(&json!({"total":0})));
        assert!(session_tree_is_empty(&json!({"sessions":[]})));
        assert!(session_tree_is_empty(&json!({})));
        assert!(!session_tree_is_empty(&json!({"total":1})));
        assert!(!session_tree_is_empty(
            &json!({"sessions":[{"session_id":"s1"}]})
        ));
    }

    #[test]
    fn call_invokes_arbitrary_function_with_payload() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"{"models":[]}"#)]);
        let cli = Cli::try_parse_from([
            "iii-code",
            "call",
            "models::list",
            "--payload",
            r#"{"provider":"openai"}"#,
        ])
        .unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"models::list".to_string()));
        assert!(calls[0].contains(&json!({"provider":"openai"}).to_string()));
        assert!(String::from_utf8(out).unwrap().contains("\"models\""));
    }

    #[test]
    fn functions_lists_registered_functions() {
        let runner = MockRunner::new(vec![MockRunner::ok(
            r#"{"functions":[{"id":"run::start"},{"id":"models::list"}]}"#,
        )]);
        let cli = Cli::try_parse_from(["iii-code", "functions", "--filter", "models"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"engine::functions::list".to_string()));
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("models::list"));
        assert!(!text.contains("run::start"));
    }

    #[test]
    fn workers_can_list_connected_workers() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"[{"worker_id":"w1"}]"#)]);
        let cli = Cli::try_parse_from(["iii-code", "workers", "--connected"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"engine::workers::list".to_string()));
        assert!(String::from_utf8(out).unwrap().contains("w1"));
    }

    #[test]
    fn state_set_parses_json_value() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"{"ok":true}"#)]);
        let cli = Cli::try_parse_from(["iii-code", "state", "set", "scope", "key", r#"{"a":1}"#])
            .unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"state::set".to_string()));
        assert!(
            calls[0].contains(&json!({"scope":"scope","key":"key","value":{"a":1}}).to_string())
        );
    }

    #[test]
    fn approvals_resolve_calls_approval_worker() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"{"ok":true}"#)]);
        let cli = Cli::try_parse_from(["iii-code", "approvals", "allow", "s1", "fc1"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"approval::resolve".to_string()));
        assert!(calls[0].contains(
            &json!({"session_id":"s1","function_call_id":"fc1","decision":"allow"}).to_string()
        ));
    }

    #[test]
    fn sandbox_exec_calls_sandbox_worker() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"{"success":true}"#)]);
        let cli =
            Cli::try_parse_from(["iii-code", "sandbox", "exec", "sb1", "npm", "test"]).unwrap();
        let mut out = Vec::new();

        run(cli, &runner, &mut out).unwrap();

        let calls = runner.calls.borrow();
        assert!(calls[0].contains(&"sandbox::exec".to_string()));
        assert!(
            calls[0].contains(
                &json!({"sandbox_id":"sb1","cmd":"npm","args":["test"],"timeout_ms":30000u64})
                    .to_string()
            )
        );
    }

    fn core_function_list_json() -> String {
        let functions = CORE_RUNTIME_FUNCTIONS
            .iter()
            .map(|id| json!({ "function_id": id }))
            .collect::<Vec<_>>();
        json!({ "functions": functions }).to_string()
    }
}
