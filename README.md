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

Use `--wait` for smoke tests and non-interactive validation:

```bash
iii-code run "reply with hi" --wait
```

## Diagnostics

```bash
iii-code doctor
iii-code models
iii-code models --provider openai
iii-code sessions
```

`doctor` is read-only. It reports the installed iii version, managed worker
status, harness health, model catalog health, and provider auth status. Probe
failures are printed and make the command exit nonzero.

Useful checks:

```bash
iii worker list
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
