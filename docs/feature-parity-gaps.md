# Feature Parity Gaps

This document tracks product behavior `iii-code` should add while staying a
thin terminal CLI on top of the upstream iii harness stack.

## Sources

- `iii-hq/iii` main: `3512ada`
- `iii-hq/workers` main: `90fc9fe`

## Boundary

`iii-code` is a thin Rust CLI around the installed `iii` binary and the public
worker registry. Setup must install the upstream harness with:

```bash
iii worker add harness
```

The harness worker declares the core stack in `harness/iii.worker.yaml`:
`iii-state`, `iii-queue`, `iii-stream`, `iii-bridge`, `iii-http`,
`turn-orchestrator`, `provider-router`, `session-tree`, `session-inbox`,
`models-catalog`, `hook-fanout`, `policy-denylist`, `shell`,
`provider-anthropic`, `provider-openai`, `auth-credentials`, `llm-budget`,
`skills`, `approval-gate`, and `iii-sandbox`.

`iii-code` should add terminal UX and payload construction around those workers.
It should not publish a competing harness or checked-in worker lockfile. If the
harness artifact is temporarily unavailable, the CLI may install the same core
workers from the public registry as a fallback.

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
- Session discovery: `state::list` over scope `agent` and prefix `session/`,
  filtered for run session state records.

## Added In This CLI

- Setup uses `iii worker add harness` first, logs harness installation errors,
  and falls back to the core worker stack from the public registry.
- Provider credentials are read from `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`
  and stored through `auth::set_token`; argv secret flags are not supported.
- `run` and `resume` construct the current `turn-orchestrator` payload,
  including `cwd`, `cwd_hash`, `approval_required`, sandbox `image`,
  `idle_timeout_secs`, and `max_turns`.
- `sessions` lists durable run sessions from `state::list`.
- `abort` calls `router::abort`.
- `workers`, `functions`, and `call` expose the broader worker graph without
  adding worker-specific code to the CLI.
- `state` and `stream` expose shared engine primitives.
- `approvals` lists and resolves `approval-gate` requests.
- `sandbox` wraps the main `iii-sandbox` lifecycle and exec functions.
- Errors from `iii trigger` redact JSON payloads before display.

## Parity Gaps

Features that map cleanly to existing iii workers:

- MCP and skills migration from Claude Code/OpenCode configs. This should be a
  setup helper around existing worker/config surfaces, not a new agent runtime.
- Model switching and model metadata. `models::list` is already the read path;
  the missing piece is better CLI formatting and defaults.
- Permission presets. This should compile to `approval_required` values and
  policy worker configuration.
- Continue/resume ergonomics. `resume <session-id>` and `sessions` exist; next
  work is an interactive selector.
- Session audit and benchmark smoke runs. These should use `run::start_and_wait`
  and stored `agent` state.

Features that need more design before adding:

- Multi-model orchestration and subagents. That belongs in a worker or
  orchestrator contract, not in the thin CLI.
- Tags and cost attribution. Likely should become metadata passed through the
  run payload and consumed by `llm-budget`, but there is no stable public
  contract in the current worker stack.
- Project-mode execution. This is a separate project-state machine and should
  be a new worker if adopted, with the CLI only issuing commands.
- Clipboard/image paste, web fetch/search, themes, and custom TUI affordances.
  These are useful terminal UX features, but v1 stays plain streaming output.
- ACP/editor mode. The upstream `acp` worker exists separately; `iii-code`
  should not bundle it unless the product target changes from terminal CLI to
  editor integration.

## Current Upstream Blocker

As of `iii-hq/workers@90fc9fe`, `iii worker add harness` reaches the harness
artifact and then fails the final SHA256 check in the public registry. That is
an upstream registry/artifact issue. `iii-code` falls back to installing the
core workers individually from the same registry while that artifact is fixed.
