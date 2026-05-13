use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "iii-code")]
#[command(about = "Thin terminal coding agent on top of iii workers")]
#[command(version)]
pub struct Cli {
    #[arg(long, default_value = "localhost", global = true)]
    pub address: String,

    #[arg(long, default_value_t = 49134, global = true)]
    pub port: u16,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(
        about = "Open the interactive terminal coding-agent shell",
        alias = "tui"
    )]
    Chat(ChatArgs),
    #[command(about = "Install harness and store provider credentials")]
    Setup(SetupArgs),
    #[command(about = "Start a new durable coding session")]
    Run(RunArgs),
    #[command(about = "Resume an existing durable session id, optionally with a new prompt")]
    Resume(ResumeArgs),
    #[command(about = "List durable sessions from session-tree")]
    Sessions(SessionsArgs),
    #[command(about = "Print the active transcript for a session")]
    Messages(MessagesArgs),
    #[command(about = "Print the full session DAG")]
    Tree(TreeArgs),
    #[command(about = "Fork a session from a session-tree entry id")]
    Fork(ForkArgs),
    #[command(about = "Clone a complete session tree")]
    Clone(CloneArgs),
    #[command(about = "Export a session branch as self-contained HTML")]
    Export(ExportArgs),
    #[command(about = "Append a compaction checkpoint to a session")]
    Compact(CompactArgs),
    #[command(about = "Print durable turn state for a session")]
    Status(StatusArgs),
    #[command(about = "Repair session-tree rows from legacy persisted messages")]
    Repair(RepairArgs),
    #[command(about = "Abort a durable session through provider-router")]
    Abort(AbortArgs),
    #[command(about = "Print read-only diagnostics for the iii-code stack")]
    Doctor(DoctorArgs),
    #[command(about = "List models from the iii models catalog")]
    Models(ModelsArgs),
    #[command(about = "List configured or connected workers")]
    Workers(WorkersArgs),
    #[command(about = "List registered iii functions")]
    Functions(FunctionsArgs),
    #[command(about = "Call any iii function with a JSON payload")]
    Call(CallArgs),
    #[command(about = "Read and write iii state")]
    State(StateArgs),
    #[command(about = "Inspect iii streams")]
    Stream(StreamArgs),
    #[command(about = "List and resolve approval-gate requests")]
    Approvals(ApprovalsArgs),
    #[command(about = "Manage iii sandboxes")]
    Sandbox(SandboxArgs),
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long)]
    pub skip_worker_add: bool,

    #[arg(
        long,
        help = "Install coding-adjacent workers: mcp, iii-lsp, iii-database"
    )]
    pub coding_full: bool,

    #[arg(long)]
    pub no_health_check: bool,

    #[arg(long, hide = true)]
    pub ignore_env_credentials: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(
        long,
        help = "Verify coding-adjacent workers: mcp, iii-lsp, iii-database"
    )]
    pub coding_full: bool,
}

#[derive(Debug, Args, Clone)]
pub struct ChatArgs {
    #[arg(help = "Optional first prompt to send after opening the shell")]
    pub prompt: Option<String>,

    #[arg(long)]
    pub session_id: Option<String>,

    #[arg(long, help = "Start a fresh session instead of resuming this cwd")]
    pub new: bool,

    #[arg(long, env = "III_CODE_PROVIDER")]
    pub provider: Option<String>,

    #[arg(long, env = "III_CODE_MODEL")]
    pub model: Option<String>,

    #[arg(long)]
    pub system_prompt: Option<String>,

    #[arg(long = "approval-required")]
    pub approval_required: Vec<String>,

    #[arg(
        long,
        default_value = "python",
        help = "Sandbox image for tool execution"
    )]
    pub image: String,

    #[arg(long, default_value_t = 300)]
    pub idle_timeout_secs: u32,

    #[arg(long, default_value_t = 20)]
    pub max_turns: u32,

    #[arg(long, default_value_t = 750)]
    pub poll_interval_ms: u64,

    #[arg(long, default_value_t = 600_000)]
    pub stream_timeout_ms: u64,

    #[arg(long)]
    pub wait: bool,
}

impl Default for ChatArgs {
    fn default() -> Self {
        Self {
            prompt: None,
            session_id: None,
            new: false,
            provider: None,
            model: None,
            system_prompt: None,
            approval_required: Vec::new(),
            image: "python".to_string(),
            idle_timeout_secs: 300,
            max_turns: 20,
            poll_interval_ms: 750,
            stream_timeout_ms: 600_000,
            wait: false,
        }
    }
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(help = "Prompt to send as the first user message")]
    pub prompt: String,

    #[arg(long, env = "III_CODE_PROVIDER")]
    pub provider: Option<String>,

    #[arg(long, env = "III_CODE_MODEL")]
    pub model: Option<String>,

    #[arg(long)]
    pub system_prompt: Option<String>,

    #[arg(long = "approval-required")]
    pub approval_required: Vec<String>,

    #[arg(
        long,
        default_value = "python",
        help = "Sandbox image for tool execution"
    )]
    pub image: String,

    #[arg(long, default_value_t = 300)]
    pub idle_timeout_secs: u32,

    #[arg(long, default_value_t = 20)]
    pub max_turns: u32,

    #[arg(long, default_value_t = 750)]
    pub poll_interval_ms: u64,

    #[arg(long, default_value_t = 600_000)]
    pub stream_timeout_ms: u64,

    #[arg(long)]
    pub wait: bool,
}

#[derive(Debug, Args)]
pub struct ResumeArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,

    #[arg(help = "Optional follow-up prompt to append to the persisted transcript")]
    pub prompt: Option<String>,

    #[arg(long, env = "III_CODE_PROVIDER")]
    pub provider: Option<String>,

    #[arg(long, env = "III_CODE_MODEL")]
    pub model: Option<String>,

    #[arg(long)]
    pub system_prompt: Option<String>,

    #[arg(long = "approval-required")]
    pub approval_required: Vec<String>,

    #[arg(
        long,
        default_value = "python",
        help = "Sandbox image for tool execution"
    )]
    pub image: String,

    #[arg(long, default_value_t = 300)]
    pub idle_timeout_secs: u32,

    #[arg(long, default_value_t = 20)]
    pub max_turns: u32,

    #[arg(long, default_value_t = 750)]
    pub poll_interval_ms: u64,

    #[arg(long, default_value_t = 600_000)]
    pub stream_timeout_ms: u64,

    #[arg(long)]
    pub wait: bool,
}

#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct MessagesArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,

    #[arg(long)]
    pub raw: bool,
}

#[derive(Debug, Args)]
pub struct TreeArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct ForkArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,

    #[arg(help = "session-tree entry id to fork from")]
    pub entry_id: String,
}

#[derive(Debug, Args)]
pub struct CloneArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,

    #[arg(long)]
    pub branch_leaf: Option<String>,

    #[arg(long, short)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CompactArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,

    #[arg(help = "Summary text to record in the session tree")]
    pub summary: String,

    #[arg(long, default_value_t = 0)]
    pub tokens_before: u64,

    #[arg(long = "read-file")]
    pub read_files: Vec<String>,

    #[arg(long = "modified-file")]
    pub modified_files: Vec<String>,

    #[arg(long)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct RepairArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct AbortArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct ModelsArgs {
    #[arg(long)]
    pub provider: Option<String>,
}

#[derive(Debug, Args)]
pub struct WorkersArgs {
    #[arg(long)]
    pub connected: bool,

    #[arg(long)]
    pub worker_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct FunctionsArgs {
    #[arg(long)]
    pub include_internal: bool,

    #[arg(long)]
    pub filter: Option<String>,
}

#[derive(Debug, Args)]
pub struct CallArgs {
    #[arg(help = "iii function id, for example models::list")]
    pub function_id: String,

    #[arg(long, conflicts_with = "payload_file")]
    pub payload: Option<String>,

    #[arg(long = "payload-file", conflicts_with = "payload")]
    pub payload_file: Option<PathBuf>,

    #[arg(long, default_value_t = 30_000)]
    pub timeout_ms: u64,
}

#[derive(Debug, Args)]
pub struct StateArgs {
    #[command(subcommand)]
    pub command: StateCommand,
}

#[derive(Debug, Subcommand)]
pub enum StateCommand {
    #[command(about = "Get one state value")]
    Get(StateGetArgs),
    #[command(about = "List state values in a scope")]
    List(StateListArgs),
    #[command(about = "Set one state value")]
    Set(StateSetArgs),
    #[command(about = "Delete one state value")]
    Delete(StateDeleteArgs),
}

#[derive(Debug, Args)]
pub struct StateGetArgs {
    pub scope: String,
    pub key: String,
}

#[derive(Debug, Args)]
pub struct StateListArgs {
    pub scope: String,

    #[arg(long)]
    pub prefix: Option<String>,
}

#[derive(Debug, Args)]
pub struct StateSetArgs {
    pub scope: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Args)]
pub struct StateDeleteArgs {
    pub scope: String,
    pub key: String,
}

#[derive(Debug, Args)]
pub struct StreamArgs {
    #[command(subcommand)]
    pub command: StreamCommand,
}

#[derive(Debug, Subcommand)]
pub enum StreamCommand {
    #[command(about = "List stream frames")]
    List(StreamListArgs),
}

#[derive(Debug, Args)]
pub struct StreamListArgs {
    pub stream_name: String,

    #[arg(long)]
    pub group_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ApprovalsArgs {
    #[command(subcommand)]
    pub command: ApprovalsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ApprovalsCommand {
    #[command(about = "List pending approvals")]
    List(ApprovalsListArgs),
    #[command(about = "Allow one pending approval")]
    Allow(ApprovalResolveArgs),
    #[command(about = "Deny one pending approval")]
    Deny(ApprovalDenyArgs),
}

#[derive(Debug, Args)]
pub struct ApprovalsListArgs {
    pub session_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ApprovalResolveArgs {
    pub session_id: String,
    pub function_call_id: String,
}

#[derive(Debug, Args)]
pub struct ApprovalDenyArgs {
    pub session_id: String,
    pub function_call_id: String,

    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct SandboxArgs {
    #[command(subcommand)]
    pub command: SandboxCommand,
}

#[derive(Debug, Subcommand)]
pub enum SandboxCommand {
    #[command(about = "List active sandboxes")]
    List,
    #[command(about = "Create a sandbox")]
    Create(SandboxCreateArgs),
    #[command(about = "Run a command inside a sandbox")]
    Exec(SandboxExecArgs),
    #[command(about = "Stop a sandbox")]
    Stop(SandboxStopArgs),
}

#[derive(Debug, Args)]
pub struct SandboxCreateArgs {
    #[arg(long, default_value = "python")]
    pub image: String,

    #[arg(long)]
    pub name: Option<String>,

    #[arg(long)]
    pub network: bool,

    #[arg(long)]
    pub idle_timeout_secs: Option<u32>,

    #[arg(long)]
    pub cpus: Option<u32>,

    #[arg(long)]
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Args)]
pub struct SandboxExecArgs {
    pub sandbox_id: String,
    pub cmd: String,

    #[arg(num_args = 0.., trailing_var_arg = true)]
    pub args: Vec<String>,

    #[arg(long, default_value_t = 30_000)]
    pub timeout_ms: u64,

    #[arg(long)]
    pub workdir: Option<String>,
}

#[derive(Debug, Args)]
pub struct SandboxStopArgs {
    pub sandbox_id: String,

    #[arg(long)]
    pub wait: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use clap::error::ErrorKind;

    #[test]
    fn parses_run_command() {
        let cli = Cli::try_parse_from([
            "iii-code",
            "run",
            "build tetris",
            "--provider",
            "openai",
            "--model",
            "gpt-5",
        ])
        .unwrap();

        match cli.command.unwrap() {
            Command::Run(args) => {
                assert_eq!(args.prompt, "build tetris");
                assert_eq!(args.provider.as_deref(), Some("openai"));
                assert_eq!(args.model.as_deref(), Some("gpt-5"));
                assert!(args.approval_required.is_empty());
                assert_eq!(args.image, "python");
                assert_eq!(args.idle_timeout_secs, 300);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn parses_setup_without_secret_flags() {
        let cli = Cli::try_parse_from(["iii-code", "setup", "--no-health-check"]).unwrap();

        match cli.command.unwrap() {
            Command::Setup(args) => {
                assert!(args.no_health_check);
                assert!(!args.coding_full);
            }
            _ => panic!("expected setup command"),
        }
    }

    #[test]
    fn parses_coding_full_profile_flags() {
        let setup = Cli::try_parse_from(["iii-code", "setup", "--coding-full"]).unwrap();
        match setup.command.unwrap() {
            Command::Setup(args) => assert!(args.coding_full),
            _ => panic!("expected setup command"),
        }

        let doctor = Cli::try_parse_from(["iii-code", "doctor", "--coding-full"]).unwrap();
        match doctor.command.unwrap() {
            Command::Doctor(args) => assert!(args.coding_full),
            _ => panic!("expected doctor command"),
        }
    }

    #[test]
    fn rejects_argv_secret_flags() {
        let err =
            Cli::try_parse_from(["iii-code", "setup", "--openai-api-key", "test-key"]).unwrap_err();

        assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn parses_sessions_and_abort_commands() {
        let sessions = Cli::try_parse_from(["iii-code", "sessions", "--limit", "5"]).unwrap();
        match sessions.command.unwrap() {
            Command::Sessions(args) => assert_eq!(args.limit, 5),
            _ => panic!("expected sessions command"),
        }

        let abort = Cli::try_parse_from(["iii-code", "abort", "s1"]).unwrap();
        match abort.command.unwrap() {
            Command::Abort(args) => assert_eq!(args.session_id, "s1"),
            _ => panic!("expected abort command"),
        }
    }

    #[test]
    fn parses_default_and_chat_commands() {
        let default_cli = Cli::try_parse_from(["iii-code"]).unwrap();
        assert!(default_cli.command.is_none());

        let chat = Cli::try_parse_from(["iii-code", "chat", "--session-id", "s1", "hi"]).unwrap();
        match chat.command.unwrap() {
            Command::Chat(args) => {
                assert_eq!(args.session_id.as_deref(), Some("s1"));
                assert_eq!(args.prompt.as_deref(), Some("hi"));
            }
            _ => panic!("expected chat command"),
        }
    }

    #[test]
    fn parses_resume_followup_and_session_tree_commands() {
        let resume = Cli::try_parse_from(["iii-code", "resume", "s1", "continue"]).unwrap();
        match resume.command.unwrap() {
            Command::Resume(args) => assert_eq!(args.prompt.as_deref(), Some("continue")),
            _ => panic!("expected resume command"),
        }

        let messages = Cli::try_parse_from(["iii-code", "messages", "s1", "--raw"]).unwrap();
        match messages.command.unwrap() {
            Command::Messages(args) => assert!(args.raw),
            _ => panic!("expected messages command"),
        }

        let fork = Cli::try_parse_from(["iii-code", "fork", "s1", "e1"]).unwrap();
        match fork.command.unwrap() {
            Command::Fork(args) => assert_eq!(args.entry_id, "e1"),
            _ => panic!("expected fork command"),
        }

        let tree = Cli::try_parse_from(["iii-code", "tree", "s1"]).unwrap();
        match tree.command.unwrap() {
            Command::Tree(args) => assert_eq!(args.session_id, "s1"),
            _ => panic!("expected tree command"),
        }

        let clone = Cli::try_parse_from(["iii-code", "clone", "s1"]).unwrap();
        match clone.command.unwrap() {
            Command::Clone(args) => assert_eq!(args.session_id, "s1"),
            _ => panic!("expected clone command"),
        }

        let export =
            Cli::try_parse_from(["iii-code", "export", "s1", "--output", "session.html"]).unwrap();
        match export.command.unwrap() {
            Command::Export(args) => {
                assert_eq!(args.output.unwrap(), PathBuf::from("session.html"))
            }
            _ => panic!("expected export command"),
        }

        let compact = Cli::try_parse_from([
            "iii-code",
            "compact",
            "s1",
            "checkpoint",
            "--read-file",
            "src/main.rs",
        ])
        .unwrap();
        match compact.command.unwrap() {
            Command::Compact(args) => assert_eq!(args.read_files, vec!["src/main.rs"]),
            _ => panic!("expected compact command"),
        }

        let status = Cli::try_parse_from(["iii-code", "status", "s1"]).unwrap();
        match status.command.unwrap() {
            Command::Status(args) => assert_eq!(args.session_id, "s1"),
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn parses_worker_function_and_call_commands() {
        let workers = Cli::try_parse_from(["iii-code", "workers", "--connected"]).unwrap();
        match workers.command.unwrap() {
            Command::Workers(args) => assert!(args.connected),
            _ => panic!("expected workers command"),
        }

        let functions =
            Cli::try_parse_from(["iii-code", "functions", "--include-internal"]).unwrap();
        match functions.command.unwrap() {
            Command::Functions(args) => assert!(args.include_internal),
            _ => panic!("expected functions command"),
        }

        let call = Cli::try_parse_from([
            "iii-code",
            "call",
            "models::list",
            "--payload",
            r#"{"provider":"openai"}"#,
        ])
        .unwrap();
        match call.command.unwrap() {
            Command::Call(args) => assert_eq!(args.function_id, "models::list"),
            _ => panic!("expected call command"),
        }
    }

    #[test]
    fn parses_state_approval_and_sandbox_commands() {
        let state = Cli::try_parse_from(["iii-code", "state", "get", "agent", "k"]).unwrap();
        match state.command.unwrap() {
            Command::State(args) => match args.command {
                StateCommand::Get(args) => assert_eq!(args.scope, "agent"),
                _ => panic!("expected state get command"),
            },
            _ => panic!("expected state command"),
        }

        let approval = Cli::try_parse_from([
            "iii-code",
            "approvals",
            "deny",
            "s1",
            "fc1",
            "--reason",
            "no",
        ])
        .unwrap();
        match approval.command.unwrap() {
            Command::Approvals(args) => match args.command {
                ApprovalsCommand::Deny(args) => assert_eq!(args.reason.as_deref(), Some("no")),
                _ => panic!("expected approval deny command"),
            },
            _ => panic!("expected approvals command"),
        }

        let sandbox =
            Cli::try_parse_from(["iii-code", "sandbox", "exec", "sb1", "npm", "test"]).unwrap();
        match sandbox.command.unwrap() {
            Command::Sandbox(args) => match args.command {
                SandboxCommand::Exec(args) => assert_eq!(args.args, vec!["test"]),
                _ => panic!("expected sandbox exec command"),
            },
            _ => panic!("expected sandbox command"),
        }
    }
}
