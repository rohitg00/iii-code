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
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Install harness and store provider credentials")]
    Setup(SetupArgs),
    #[command(about = "Start a new durable coding session")]
    Run(RunArgs),
    #[command(about = "Resume an existing durable session id")]
    Resume(ResumeArgs),
    #[command(about = "List durable run sessions persisted by turn-orchestrator")]
    Sessions(SessionsArgs),
    #[command(about = "Abort a durable session through provider-router")]
    Abort(AbortArgs),
    #[command(about = "Print read-only diagnostics for the iii-code stack")]
    Doctor,
    #[command(about = "List models from the iii models catalog")]
    Models(ModelsArgs),
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long)]
    pub skip_worker_add: bool,

    #[arg(long)]
    pub no_health_check: bool,

    #[arg(long, hide = true)]
    pub ignore_env_credentials: bool,
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
pub struct AbortArgs {
    #[arg(help = "Existing iii agent session id")]
    pub session_id: String,
}

#[derive(Debug, Args)]
pub struct ModelsArgs {
    #[arg(long)]
    pub provider: Option<String>,
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

        match cli.command {
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

        match cli.command {
            Command::Setup(args) => {
                assert!(args.no_health_check);
            }
            _ => panic!("expected setup command"),
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
        match sessions.command {
            Command::Sessions(args) => assert_eq!(args.limit, 5),
            _ => panic!("expected sessions command"),
        }

        let abort = Cli::try_parse_from(["iii-code", "abort", "s1"]).unwrap();
        match abort.command {
            Command::Abort(args) => assert_eq!(args.session_id, "s1"),
            _ => panic!("expected abort command"),
        }
    }
}
