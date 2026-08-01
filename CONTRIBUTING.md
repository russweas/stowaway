# Contributing

## Development setup

Stowaway is a Rust 2024 CLI. The main modules are:

- `src/main.rs` — application entry point
- `src/cli.rs` — command-line arguments
- `src/commands.rs` — deployment workflows
- `src/config.rs` — manifest validation
- `src/remote.rs` — SSH and remote shell construction
- `src/repository.rs` — repository discovery and deployment loading

User-facing behavior belongs in `README.md`; planned work is tracked in
`PLAN.md`.

## Build and test

Use the lockfile for all Cargo commands:

```console
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --locked --release
```

The Debian integration test exercises unprivileged and privileged deployment,
rollback, and remote command behavior in a disposable Docker container:

```console
tests/debian-container.sh
```

Run it when changing apply logic, privileged paths, remote commands, or
deployment state. The test requires Docker.

See [`tests/docker.md`](tests/docker.md) for options to keep or remove the
integration container while debugging.

Focused unit tests should live beside the code they cover. Include failure-path
coverage for quoting, validation, adoption, convergence, and rollback.

## Style and security

Use rustfmt defaults and keep Clippy clean. Use Rust naming conventions:
`snake_case` for functions and modules, `PascalCase` for types, and
`SCREAMING_SNAKE_CASE` for constants. Keep command orchestration separate from
repository parsing and remote shell construction. Add contextual `anyhow`
errors at filesystem, Git, SSH, and command boundaries.

Never commit credentials, private keys, host secrets, or `target/` artifacts.
Treat quoting, path normalization, clean-worktree checks, backups, and rollback
as security-sensitive behavior.

## Releases

Release tooling publishes Linux x86_64 and macOS aarch64 binaries. A release
requires an authenticated `gh` CLI, a clean worktree, and a tag matching the
version in `Cargo.toml`:

```console
./scripts/release.sh v0.1.0
```

To add another target to an existing release:

```console
./scripts/release.sh v0.1.1 aarch64-apple-darwin
```

Cross-compiling macOS from Linux requires an Apple SDK and linker such as
`osxcross`; building on an Apple Silicon Mac or CI runner is usually simpler.

## Commits and pull requests

Use concise, imperative commit subjects, for example `Reject unsafe manifest
paths`. Keep each commit scoped to one logical change. Pull requests should
explain behavior changes and safety implications, list commands run, link
relevant issues, and update `README.md` for CLI or manifest changes.
