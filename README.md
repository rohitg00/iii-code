# iii-code

`iii-code` is a small Rust CLI for power users who want a terminal coding-agent
loop on top of the installed `iii` engine. It does not embed a second agent
runtime, and does not keep its own secrets store.

The boundary is the public `iii` CLI and worker functions:

- worker install through `iii worker add harness`, with a core worker fallback
  if the harness artifact cannot be installed
- run control through `run::start` and `run::start_and_wait`
- event streaming through `stream::list` over `agent::events`
- credentials through `auth::set_token` and `auth::status`
- model discovery through `models::list`
- session listing through `state::list` over the `agent` scope and `session/`
  prefix
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
- `iii` engine started from this repo when running sessions
- provider credentials in environment variables

```bash
export ANTHROPIC_API_KEY=...
export OPENAI_API_KEY=...
iii
```

In another terminal:

```bash
iii-code setup
```

`setup` verifies `iii --version`, installs/updates the worker stack with
`iii worker add harness`, falls back to installing the core worker dependencies
from the same registry if harness installation fails, stores `OPENAI_API_KEY`
and `ANTHROPIC_API_KEY` through `auth::set_token`, then probes
`harness::status`, `models::list`, and `auth::status`.

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

Secrets are intentionally not accepted through CLI flags because argv can leak
through process listings and error logs.

Current upstream note: a fresh `iii worker add harness` resolves the dependency
graph but fails on the final `harness` binary SHA256 check in the public worker
registry. `iii-code setup` logs that error and installs the core dependency
stack from the same public registry so the CLI can still be exercised while the
harness artifact is fixed upstream.

## Run

```bash
iii-code run "inspect this repo and suggest the first cleanup"
```

By default, `iii-code` starts a durable `run::start` session and polls
`stream::list` for new `agent::events` frames. It prints the session id so you
can resume:

```bash
iii-code resume <session-id>
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
iii-code abort <session-id>
```

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
status, harness health, model catalog health, and provider auth status. Probe
failures are printed and make the command exit nonzero.

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
- `https://github.com/iii-hq/workers` at `90fc9fe`

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
