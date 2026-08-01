# Repository Guidelines

## Project Structure & Module Organization

Stowaway is a Rust 2024 CLI for deploying Git-managed server configuration over SSH. The entry point is `src/main.rs`; arguments live in `src/cli.rs`. Deployment workflows are in `src/commands.rs`, manifest validation in `src/config.rs`, SSH generation in `src/remote.rs`, and repository discovery in `src/repository.rs`. Unit tests are colocated under `#[cfg(test)]`. `tests/debian-container.sh` and `tests/debian/Dockerfile` provide an end-to-end deployment and rollback test. User behavior belongs in `README.md`; planned work is tracked in `PLAN.md`.

## Build, Test, and Development Commands

- `cargo build --locked`: compile the debug binary using the lockfile.
- `cargo run -- validate [MACHINE]`: run the CLI locally and validate one or all manifests.
- `cargo test --locked`: run the Rust unit test suite.
- `cargo clippy --locked --all-targets -- -D warnings`: enforce the same zero-warning lint policy used by CI.
- `cargo fmt --all -- --check`: verify standard Rust formatting; run `cargo fmt --all` to apply it.
- `tests/debian-container.sh`: exercise unprivileged and privileged deployment plus rollback in Docker. This creates and removes a disposable Debian container.
- `cargo build --locked --release`: produce the optimized release binary at `target/release/stowaway`.

## Coding Style & Naming Conventions

Use rustfmt defaults (four-space indentation) and keep Clippy clean. Follow Rust naming conventions: `snake_case` for functions, modules, and tests; `PascalCase` for types and enum variants; `SCREAMING_SNAKE_CASE` for constants. Keep command orchestration separate from repository parsing and remote shell construction. Add contextual `anyhow` errors at filesystem, Git, SSH, and command boundaries.

## Testing Guidelines

Place focused unit tests beside the code they cover and name them after observable behavior, such as `rejects_invalid_mode`. Include failure-path tests for quoting, validation, adoption, and transactional rollback. Run unit tests and Clippy before every pull request; run the Docker test when changing apply logic, privileged paths, remote commands, or deployment state.

## Commit & Pull Request Guidelines

Use concise, imperative commit subjects (for example, `Reject unsafe manifest paths`) and keep each commit scoped to one logical change. Pull requests should explain the behavior change, safety implications, and commands run. Link relevant issues and update `README.md` for CLI or manifest changes. Include terminal output when it clarifies a changed diff, apply, pull, or status workflow; screenshots are generally unnecessary for this CLI.

## Security & Configuration Tips

Never commit credentials, private keys, host secrets, or `target/` artifacts. Preserve OpenSSH host-key verification and passwordless `sudo -n` assumptions. Treat quoting, path normalization, clean-worktree checks, backups, and rollback as security-sensitive behavior.
