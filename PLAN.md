# Stowaway v0.1.0 Plan

This file tracks the implementation of the first usable release. A checked item
must be covered by automated tests and pass `cargo clippy --all-targets -- -D
warnings`.

## Repository and CLI

- [x] Scaffold a small synchronous Rust CLI.
- [x] Define strict, versioned per-machine TOML manifests.
- [x] Validate tree boundaries, source types, targets, modes, and scripts.
- [x] Produce deterministic deployment content hashes.
- [x] Document the repository layout and script contract.
- [x] Require a clean Git worktree before `apply` and `pull`.

## Remote inspection

- [x] Execute commands through the user's system OpenSSH client.
- [x] Inspect regular files, links, modes, missing paths, and collisions.
- [x] Use `sudo -n` for privileged file inspection.
- [x] Render unified text diffs and concise binary/type/mode changes.
- [x] Run script `check` actions and include their output in previews.
- [x] Read and display protected remote deployment state.

## Apply

- [x] Always preview and confirm before mutation unless `--yes` is supplied.
- [x] Transfer content into digest-addressed user and root staging trees.
- [x] Journal target changes and roll them back on any failure.
- [x] Refuse unmanaged collisions by default.
- [x] Implement `--adopt` with timestamped remote backups.
- [x] Create per-file links and safely remove obsolete managed links.
- [x] Run ordered script `apply` actions with timeouts.
- [x] Atomically record Git commit, content digest, timestamp, and managed paths.
- [x] Make repeated deployment idempotent.

## Pull

- [x] Preview changes for locally declared managed files only.
- [x] Confirm before changing the local worktree.
- [x] Import contents, deletions, and executable-bit changes.
- [x] Never create commits automatically.

## Release quality

- [x] Add fake-SSH tests for protocol errors, timeouts, and connection failures.
- [x] Add Debian container tests for home and privileged `/etc` deployments.
- [x] Add rollback tests proving file recovery.
- [x] Document remote prerequisites, sudo policy, adoption, and recovery.
- [x] Build and smoke-test an optimized release binary.
- [ ] Tag `v0.1.0` only when every item above is complete.
