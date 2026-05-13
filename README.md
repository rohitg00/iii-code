# iii-code

`iii-code` is a small Rust CLI for power users who want a terminal coding-agent
loop on top of the installed `iii` engine. It does not embed a second agent
runtime, and does not keep its own secrets store.

The default command opens a terminal harness shell:

```bash
iii-code
iii-code chat "inspect this repo and find the next fix"
```

Inside the shell, regular text sends a new user turn. Slash commands expose the
same worker-backed controls as the browser harness:

```text
/sessions
/messages [session-id]
/status [session-id]
/tree [session-id]
/clone [session-id]
/export [output.html]
/compact <summary>
/functions [filter]
/workers
/approvals
/allow <function-call-id>
/deny <function-call-id> [reason]
/repair
/fork <entry-id>
/doctor
```

The boundary is the public `iii` CLI and worker functions:

- worker install through `iii worker add harness`, with a core worker fallback
  if the harness artifact cannot be installed
- run control through `run::start` and `run::start_and_wait`
- event streaming through `stream::list` over `agent::events`
- agent tool dispatch through the canonical `agent_call` tool owned by
  `turn-orchestrator`
- credentials through `auth::set_token` and `auth::status`
- model discovery through `models::list`
- session discovery, transcript loading, tree, clone, export, compact, fork,
  and repair through
  `session-tree::*`, with legacy state fallback where needed
- abort through `router::abort`
- worker and function discovery through `engine::*::list`
- direct calls into any worker through `iii-code call`
- approval resolution through `approval::resolve`
- sandbox lifecycle through `sandbox::*`

`iii-code` does not recreate the harness stack. The harness worker from
`iii-hq/workers` is the source of truth for `turn-orchestrator`,
`provider-router`, `session-tree`, `session-inbox`, `models-catalog`, `shell`,
`skills`, `approval-gate`, `auth-credentials`, `llm-budget`, provider workers,
`iii-sandbox`, and the bridge pieces.

## Install

```bash
cargo install --path .
```

## Prerequisites

- latest `iii` CLI on `PATH`
- worker stack installed in the local iii config
- `iii` engine started from this repo when running sessions
- provider credentials in environment variables

```bash
export ANTHROPIC_API_KEY=...
export OPENAI_API_KEY=...
cp config.example.yaml config.yaml
iii worker add harness
iii
```

In another terminal:

```bash
iii-code setup
iii-code setup --coding-full
```

`setup` verifies `iii --version`, installs/updates the worker stack with
`iii worker add harness`, falls back to installing the core worker dependencies
from the same registry if harness installation fails, stores `OPENAI_API_KEY`
and `ANTHROPIC_API_KEY` through `auth::set_token`, then probes
`harness::status` or the required core worker functions, `models::list`, and
`auth::status`. At least one supported provider credential must be configured;
the other provider is reported as missing without blocking single-provider use.

Use `iii-code setup --coding-full` when you want the richer coding profile from
the public registry. It installs the base harness stack plus `mcp`, `iii-lsp`,
and `iii-database@1.0.4`, then verifies those configured workers during health
checks. The database worker is pinned because the current public registry has
no `latest` tag for `iii-database`. For read-only verification later, run:

```bash
iii-code doctor --coding-full
```

`iii worker list` showing `stopped` is normal when the engine is not running.
Keep `iii` running in one terminal; the same list should then show the engine,
provider, shell, approval, skills, and sandbox workers as `running`.

The installed stack must include the execution path, not just the run
orchestrator. In practice that means `setup` should leave these workers in the
local iii config:

- `turn-orchestrator`
- `provider-router`
- `provider-openai` / `provider-anthropic`
- `auth-credentials`
- `models-catalog`
- `shell`
- `approval-gate`
- `skills`
- `iii-sandbox`

`iii-sandbox` is required for `shell::*` tool calls. Its default config allows
the `python` and `node` sandbox images, auto-installs root filesystems on first
use, and reaps idle sandboxes after 300 seconds. Host support is required:
Apple Silicon macOS or Linux with readable `/dev/kvm`; Windows and Intel Macs
cannot boot the sandbox microVMs.

`config.example.yaml` includes the complete core harness worker stack, sandbox
worker, and shell settings. Copy it to `config.yaml` for local use. It pins
`shell.fs.host_root` and `shell.working_dir` to this repo with `.`. Start `iii`
from the repo root so `shell::fs::*` can read and write project files. If
`shell::fs::ls` returns `S215 path escapes host_root`, stop the running `iii`
process, confirm the engine was started from the repo root, and start it again
so it picks up the current config. The shell allowlist includes common
repo-inspection and validation commands such as `rg`, `git`, `cargo`, `npm`,
`pnpm`, `bun`, `node`, `python`, and `make`; approval policy still lives in the
worker stack.

Secrets are intentionally not accepted through CLI flags because argv can leak
through process listings and error logs.

Current upstream note: a fresh `iii worker add harness` resolves the dependency
graph but fails on the final `harness` binary SHA256 check in the public worker
registry. `iii-code setup` logs that error and installs the core dependency
stack from the same public registry so the CLI can still be exercised while the
harness artifact is fixed upstream.

## Run

Open the interactive shell:

```bash
iii-code
iii-code --help
```

Or start with a prompt:

```bash
iii-code chat "inspect this repo and suggest the first cleanup"
iii-code chat --new "use the harness tools to list this repo and summarize it"
```

For scripts, call one turn directly:

```bash
iii-code run "inspect this repo and suggest the first cleanup"
```

By default, `iii-code` starts a durable `run::start` session and polls
`stream::list` for new `agent::events` frames. It prints the session id so you
can continue from the same transcript:

```bash
iii-code resume <session-id>
iii-code resume <session-id> "continue from there and make the change"
```

Override provider/model when needed:

```bash
iii-code run "reply with hi" --provider openai --model gpt-5
iii-code run "reply with hi" --provider anthropic --model claude-sonnet-4-6
```

Use existing worker controls when you need more of the harness behavior:

```bash
iii-code run "edit src/main.rs" --approval-required shell::fs::write
iii-code run "run the node test suite" --image node
iii-code sessions
iii-code status <session-id>
iii-code messages <session-id>
iii-code tree <session-id>
iii-code clone <session-id>
iii-code export <session-id> --output session.html
iii-code compact <session-id> "compacted after initial investigation"
iii-code fork <session-id> <entry-id>
iii-code repair <session-id>
iii-code abort <session-id>
```

When no `--system-prompt` override is passed, `iii-code` deliberately lets
`turn-orchestrator` build the canonical harness prompt. That prompt gives the
model the single `agent_call` tool, current working directory, skills index,
live function discovery rules, and recovery policy. Passing `--system-prompt`
replaces that harness prompt, so use it only when you intentionally want to
take over the agent contract.

On the first user turn of a new session, `iii-code` adds a short client-context
preamble telling the model to inspect installed iii workers and live functions
before assuming a capability is missing. This does not replace the canonical
harness prompt; it is just first-turn context inside the transcript.

`--image` selects the sandbox image used by `shell::*` tools. The default is
`python`; use `--image node` for JavaScript/TypeScript repo work. If you add
custom sandbox images in `config.yaml`, they must also be allowed by the
`iii-sandbox` config before runs can boot them.

Use `--wait` for smoke tests and non-interactive validation:

```bash
iii-code run "reply with hi" --wait
```

## Worker Surface

`iii-code` is also a thin operator shell for connected workers. Use these when
the task needs a worker that is not hard-coded into the coding-session flow:

```bash
iii-code workers
iii-code workers --connected
iii-code functions --filter sandbox
iii-code call models::list --payload '{"provider":"openai"}'
iii-code call custom::function --payload-file payload.json
```

State and stream helpers expose the shared engine primitives:

```bash
iii-code state get agent session/<session-id>/turn_state
iii-code state list agent --prefix session/
iii-code state set scratch answer '{"ok":true}'
iii-code state delete scratch answer
iii-code stream list agent::events --group-id <session-id>
```

Approvals are first-class, so terminal runs can block on protected tools and be
released from another terminal:

```bash
iii-code approvals list <session-id>
iii-code approvals allow <session-id> <function-call-id>
iii-code approvals deny <session-id> <function-call-id> --reason "not safe"
```

Sandbox commands are direct wrappers over `iii-sandbox`:

```bash
iii-code sandbox create --image node --name test-runner
iii-code sandbox exec <sandbox-id> npm test
iii-code sandbox list
iii-code sandbox stop <sandbox-id> --wait
```

## Diagnostics

```bash
iii-code doctor
iii-code models
iii-code models --provider openai
iii-code sessions
iii-code workers --connected
iii-code functions --filter run::
```

`doctor` is read-only. It reports the installed iii version, managed worker
status, harness or core runtime health, workspace filesystem access, model
catalog health, and provider auth status. Probe failures are printed and make
the command exit nonzero.

Useful checks:

```bash
iii worker list
iii worker list | rg 'iii-sandbox|shell|turn-orchestrator|provider-router'
iii-code doctor
iii-code models
```

## Development

Fresh upstream references were cloned from:

- `https://github.com/iii-hq/iii` at `3512ada`
- `https://github.com/iii-hq/workers` at `ee90c51`

The CLI intentionally depends on the installed `iii` binary rather than local
checkout paths.

Feature parity notes live in [docs/feature-parity-gaps.md](docs/feature-parity-gaps.md).

```bash
cargo test
cargo clippy -- -D warnings
cargo test -- --ignored
```

Ignored tests require a running iii engine, installed workers, and provider
credentials.
