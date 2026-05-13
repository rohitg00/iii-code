# Contributing

Thanks for taking the time to improve `iii-code`.

`iii-code` is intentionally a thin Rust CLI over the installed `iii` binary and
the public worker stack. Contributions should preserve that boundary: prefer
terminal UX, payload construction, diagnostics, and documentation changes over
embedding another agent runtime in this repository.

## Development Setup

Install the Rust stable toolchain, then check the project locally:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The default test suite does not require a running `iii` engine. The ignored
smoke test exercises the real engine and worker stack:

```bash
cp config.example.yaml config.yaml
iii worker add harness
iii
cargo test -- --ignored
```

Run `iii` from the repository root so the example shell filesystem settings are
jailed to the same directory as the checkout.

## Pull Requests

Keep pull requests focused and include:

- the user-facing behavior or documentation change
- the commands you ran, including skipped checks if any
- any required `iii` engine, worker, or provider credential assumptions

Good first changes include:

- README and troubleshooting improvements
- CLI formatting and diagnostics
- tests for argument parsing, payload construction, event rendering, and error
  redaction
- small terminal UX improvements listed in
  [docs/feature-parity-gaps.md](docs/feature-parity-gaps.md)

Before opening a larger feature, start with an issue or discussion so the
worker boundary and public `iii` contracts are clear.
