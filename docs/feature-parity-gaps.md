# Feature Parity Gaps

This document tracks product behavior `iii-code` should add to reach practical
Pi/Kimchi coding-agent parity while staying on top of the upstream iii harness
stack.

## Sources

- `iii-hq/iii` main: `3512ada`
- `iii-hq/workers` main: `ee90c51`

## Boundary

`iii-code` is a Rust terminal coding agent around the installed `iii` binary
and the public worker registry. The target is not a minimal wrapper; it should
feel like a complete coding agent when the iii workers provide the underlying
capability. Setup must install the upstream harness with:

```bash
iii worker add harness
```

The harness worker declares the core stack in `harness/iii.worker.yaml`:
`iii-state`, `iii-queue`, `iii-stream`, `iii-bridge`, `iii-http`,
`turn-orchestrator`, `provider-router`, `session-tree`, `session-inbox`,
`models-catalog`, `hook-fanout`, `policy-denylist`, `shell`,
`provider-anthropic`, `provider-openai`, `auth-credentials`, `llm-budget`,
`skills`, `approval-gate`, and `iii-sandbox`.

`iii-code` should add terminal UX, parity-oriented setup defaults, diagnostics,
and payload construction around those workers. It should not publish a
competing harness or checked-in worker lockfile. If the harness artifact is
temporarily unavailable, the CLI may install the same core workers from the
public registry as a fallback.

For a fuller coding profile, `iii-code setup --coding-full` additionally
installs public registry workers `mcp`, `iii-lsp`, and `iii-database@1.0.4`.
The CLI installs that profile and verifies both configured workers and live
runtime functions for MCP and database access; the model still discovers usable
functions from the running engine.

## Covered By Existing Workers

- Run and resume: `run::start` and `run::start_and_wait` from
  `turn-orchestrator`.
- Streaming: `agent::events` through `stream::list` or the harness bridge.
- Provider/model selection: `provider-router`, `provider-openai`,
  `provider-anthropic`, and `models-catalog`.
- Credentials: `auth::set_token` and `auth::status` from `auth-credentials`.
- Approvals: `approval_required` in the run payload plus `approval-gate`.
- Shell execution and sandboxing: `shell` plus `iii-sandbox`.
- Skills: the upstream `skills` worker.
- Abort: `router::abort` from `provider-router`.
- Session discovery, transcript loading, fork, and repair: `session-tree::*`,
  with legacy `state::*` fallback for older persisted sessions.

## Added In This CLI

- Setup uses `iii worker add harness` first, logs harness installation errors,
  and falls back to the core worker stack from the public registry.
- `setup --coding-full` installs `mcp`, `iii-lsp`, and
  `iii-database@1.0.4`; `doctor --coding-full` verifies those workers are
  configured and that the MCP/database functions are actually registered.
- `config.example.yaml` uses coding-agent defaults for the shell worker:
  normal validation commands, `npm run ...`, and short `node -e`/`python -c`
  checks are allowed while destructive patterns remain blocked.
- Provider credentials are read from `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`
  and stored through `auth::set_token`; argv secret flags are not supported.
- `run` and `resume` construct the current `turn-orchestrator` payload,
  including `cwd`, `cwd_hash`, `approval_required`, sandbox `image`,
  `idle_timeout_secs`, and `max_turns`.
- The default command and `chat` open an interactive terminal shell with slash
  commands for sessions, messages, function discovery, approvals, repair, fork,
  and diagnostics.
- `resume <session-id> <prompt>` loads the persisted `session-tree` transcript,
  appends the new user message, and sends the full history back through
  `run::start`.
- `sessions`, `messages`, `fork`, and `repair` use `session-tree::*` first and
  fall back to legacy state where needed.
- `abort` calls `router::abort`.
- `workers`, `functions`, and `call` expose the broader worker graph without
  adding worker-specific code to the CLI.
- New sessions add first-turn client context that tells the model to inspect
  installed workers, live functions, and skill docs instead of assuming a fixed
  capability set.
- `state` and `stream` expose shared engine primitives.
- `approvals` lists and resolves `approval-gate` requests.
- `sandbox` wraps the main `iii-sandbox` lifecycle and exec functions.
- Errors from `iii trigger` redact JSON payloads before display.

## Parity Gaps

Highest-priority Kimchi/Pi parity gaps that map cleanly to existing iii
workers or CLI behavior:

- MCP server configuration import/export. The `mcp` worker is now part of the
  optional coding profile; the missing piece is a setup helper around existing
  worker config surfaces.
- Model switching and model metadata. `models::list` is already the read path;
  the missing piece is better CLI formatting and defaults.
- Permission presets. This should compile to `approval_required` values, shell
  policy worker configuration, and named modes that match common coding-agent
  expectations.
- Continue/resume ergonomics. The shell and session-tree commands exist; next
  work is a richer selector over entries and branches, plus automatic recovery
  when a turn times out before emitting a terminal event.
- Session audit and benchmark smoke runs. These should use `run::start_and_wait`
  and stored `agent` state.
- Prompt recovery parity. Kimchi injects continuation nudges and strips stale
  prompt scaffolding; equivalent behavior should live in the orchestrator or a
  stable worker contract, with the CLI only surfacing controls.

Features that need a worker or orchestrator contract before adding:

- Multi-model orchestration and subagents. Kimchi/Pi provide this through
  extensions; iii-code should expose it once the iii worker contract exists.
- Tags and cost attribution. Likely should become metadata passed through the
  run payload and consumed by `llm-budget`, but there is no stable public
  contract in the current worker stack.
- Project-mode execution. This is a separate project-state machine and should
  be a new worker if adopted, with the CLI only issuing commands.
- Clipboard/image paste, web fetch/search, themes, and custom TUI affordances.
  These are useful parity features, not out-of-scope forever; they need clear
  worker/function contracts or terminal UX designs.
- ACP/editor mode. The upstream `acp` worker exists separately; `iii-code`
  should not bundle it unless the product target changes from terminal CLI to
  editor integration.

## Current Upstream Blocker

As of `iii-hq/workers@ee90c51`, `iii worker add harness` reaches the harness
artifact and then fails the final SHA256 check in the public registry. That is
an upstream registry/artifact issue. `iii-code` falls back to installing the
core workers individually from the same registry while that artifact is fixed.
