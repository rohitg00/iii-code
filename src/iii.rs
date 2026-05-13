use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::process::Command;

pub const CORE_WORKER_STACK: &[&str] = &[
    "iii-state",
    "iii-queue",
    "iii-stream",
    "iii-bridge",
    "iii-http",
    "turn-orchestrator",
    "provider-router",
    "session-tree",
    "session-inbox",
    "models-catalog",
    "hook-fanout",
    "policy-denylist",
    "shell",
    "provider-anthropic",
    "provider-openai",
    "auth-credentials",
    "llm-budget",
    "skills",
    "approval-gate",
    "iii-sandbox",
];

pub const CODING_FULL_WORKER_STACK: &[&str] = &["mcp", "iii-lsp", "iii-database"];
pub const CODING_FULL_WORKER_ADD_SPECS: &[&str] = &["mcp", "iii-lsp", "iii-database@1.0.4"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandRunner {
    fn run(&self, args: &[String]) -> Result<CommandOutput>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl ProcessRunner {
    pub fn new() -> Self {
        Self
    }
}

impl CommandRunner for ProcessRunner {
    fn run(&self, args: &[String]) -> Result<CommandOutput> {
        let output = Command::new("iii")
            .args(args)
            .output()
            .with_context(|| format!("run iii {}", display_args(args)))?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

pub struct IiiClient<R> {
    runner: R,
    address: String,
    port: u16,
}

impl<R: CommandRunner> IiiClient<R> {
    pub fn new(runner: R, address: impl Into<String>, port: u16) -> Self {
        Self {
            runner,
            address: address.into(),
            port,
        }
    }

    pub fn version(&self) -> Result<String> {
        let out = self.checked_run(vec!["--version".into()])?;
        Ok(out.stdout.trim().to_string())
    }

    pub fn worker_add_harness(&self) -> Result<String> {
        let out = self.checked_run(vec!["worker".into(), "add".into(), "harness".into()])?;
        Ok(join_output(&out))
    }

    pub fn worker_add_core(&self) -> Result<String> {
        let mut args = vec!["worker".into(), "add".into(), "--no-wait".into()];
        args.extend(CORE_WORKER_STACK.iter().map(|worker| worker.to_string()));
        let out = self.checked_run(args)?;
        Ok(join_output(&out))
    }

    pub fn worker_add_coding_full(&self) -> Result<String> {
        let mut args = vec!["worker".into(), "add".into(), "--no-wait".into()];
        args.extend(
            CODING_FULL_WORKER_ADD_SPECS
                .iter()
                .map(|worker| worker.to_string()),
        );
        let out = self.checked_run(args)?;
        Ok(join_output(&out))
    }

    pub fn worker_list(&self) -> Result<String> {
        let out = self.checked_run(vec!["worker".into(), "list".into()])?;
        Ok(join_output(&out))
    }

    pub fn trigger(&self, function_id: &str, payload: Value, timeout_ms: u64) -> Result<Value> {
        let args = vec![
            "trigger".into(),
            "--function-id".into(),
            function_id.into(),
            "--payload".into(),
            payload.to_string(),
            "--address".into(),
            self.address.clone(),
            "--port".into(),
            self.port.to_string(),
            "--timeout-ms".into(),
            timeout_ms.to_string(),
        ];
        let out = self.checked_run(args)?;
        parse_json_stdout(function_id, &out.stdout)
    }

    fn checked_run(&self, args: Vec<String>) -> Result<CommandOutput> {
        let out = self.runner.run(&args)?;
        if out.status == 0 {
            Ok(out)
        } else {
            Err(anyhow!(
                "iii {} failed with status {}\n{}",
                display_args(&args),
                out.status,
                join_output(&out)
            ))
        }
    }
}

fn display_args(args: &[String]) -> String {
    sanitize_args(args).join(" ")
}

fn sanitize_args(args: &[String]) -> Vec<String> {
    let redact_payload = args.first().map(|arg| arg == "trigger").unwrap_or(false)
        || args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "auth::set_token" | "run::start" | "run::start_and_wait"
            )
        });
    let mut sanitized = Vec::with_capacity(args.len());
    let mut redact_next = false;

    for arg in args {
        if redact_next {
            sanitized.push("[REDACTED]".to_string());
            redact_next = false;
            continue;
        }

        if redact_payload && arg == "--payload" {
            sanitized.push(arg.clone());
            redact_next = true;
        } else if redact_payload && arg.starts_with("--payload=") {
            sanitized.push("--payload=[REDACTED]".to_string());
        } else {
            sanitized.push(arg.clone());
        }
    }

    sanitized
}

fn parse_json_stdout(function_id: &str, stdout: &str) -> Result<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(trimmed)
        .with_context(|| format!("parse JSON output from {function_id}: {trimmed}"))
}

fn join_output(out: &CommandOutput) -> String {
    match (out.stdout.trim(), out.stderr.trim()) {
        ("", "") => String::new(),
        (stdout, "") => stdout.to_string(),
        ("", stderr) => stderr.to_string(),
        (stdout, stderr) => format!("{stdout}\n{stderr}"),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    pub struct MockRunner {
        pub calls: RefCell<Vec<Vec<String>>>,
        outputs: RefCell<VecDeque<CommandOutput>>,
    }

    impl MockRunner {
        pub fn new(outputs: Vec<CommandOutput>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                outputs: RefCell::new(outputs.into()),
            }
        }

        pub fn ok(stdout: impl Into<String>) -> CommandOutput {
            CommandOutput {
                status: 0,
                stdout: stdout.into(),
                stderr: String::new(),
            }
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, args: &[String]) -> Result<CommandOutput> {
            self.calls.borrow_mut().push(args.to_vec());
            self.outputs
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| anyhow!("mock output queue exhausted"))
        }
    }

    #[test]
    fn trigger_passes_function_payload_and_engine_address() {
        let runner = MockRunner::new(vec![MockRunner::ok(r#"{"ok":true}"#)]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);

        let value = client
            .trigger("harness::status", serde_json::json!({}), 5_000)
            .unwrap();

        assert_eq!(value["ok"], true);
        let calls = runner.calls.borrow();
        assert_eq!(calls[0][0], "trigger");
        assert!(calls[0].contains(&"harness::status".to_string()));
        assert!(calls[0].contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn checked_run_redacts_auth_payload_in_errors() {
        let runner = MockRunner::new(vec![CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: "auth failed".into(),
        }]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);

        let err = client
            .trigger(
                "auth::set_token",
                serde_json::json!({"credential":{"key":"test-secret-value"}}),
                5_000,
            )
            .unwrap_err()
            .to_string();

        assert!(err.contains("--payload [REDACTED]"));
        assert!(!err.contains("test-secret-value"));
    }

    #[test]
    fn checked_run_redacts_run_payload_in_errors() {
        let runner = MockRunner::new(vec![CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: "run failed".into(),
        }]);
        let client = IiiClient::new(&runner, "127.0.0.1", 49134);

        let err = client
            .trigger(
                "run::start",
                serde_json::json!({"messages":[{"content":"private prompt"}]}),
                5_000,
            )
            .unwrap_err()
            .to_string();

        assert!(err.contains("--payload [REDACTED]"));
        assert!(!err.contains("private prompt"));
    }

    #[test]
    fn sanitize_args_redacts_payload_equals_form() {
        let args = vec![
            "trigger".to_string(),
            "--function-id".to_string(),
            "auth::set_token".to_string(),
            "--payload={\"key\":\"test-secret-value\"}".to_string(),
        ];

        let rendered = display_args(&args);

        assert!(rendered.contains("--payload=[REDACTED]"));
        assert!(!rendered.contains("test-secret-value"));
    }

    #[test]
    fn sanitize_args_redacts_any_trigger_payload() {
        let args = vec![
            "trigger".to_string(),
            "--function-id".to_string(),
            "custom::worker".to_string(),
            "--payload".to_string(),
            "{\"secret\":\"value\"}".to_string(),
        ];

        let rendered = display_args(&args);

        assert!(rendered.contains("--payload [REDACTED]"));
        assert!(!rendered.contains("secret"));
    }

    impl<T: CommandRunner + ?Sized> CommandRunner for &T {
        fn run(&self, args: &[String]) -> Result<CommandOutput> {
            (*self).run(args)
        }
    }
}
