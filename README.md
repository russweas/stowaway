# Stowaway

Stowaway is an agentless configuration deployment CLI for Linux servers. It
keeps server-specific configuration in Git and communicates through
the system `ssh` command.

Remote inspection, diffing, transactional deployment, deployment-state
reporting, and importing declared files are available.

## Repository layout

```text
machines/
  server-01/
    machine.toml
    home/
      .config/example/config.toml
    root/
      etc/network/interfaces.d/10-server
    scripts/
      networking.sh
```

Example `machine.toml`:

```toml
version = 1

[ssh]
destination = "server-01" # An OpenSSH host or alias.

[[trees]]
source = "home"
target = "~"

[[trees]]
source = "root"
target = "/"
privileged = true

[[metadata]]
source = "root/etc/network/interfaces.d/10-server"
mode = "0644"

[[scripts]]
path = "scripts/networking.sh"
privileged = false
timeout_seconds = 3600
```

Scripts receive either `check` or `apply` as their first argument. `check`
returns 0 when the host is already configured, 10 when an apply is needed, and
any other status for an error. `diff` runs checks in manifest order, displays
their standard output and error output, and enforces each configured timeout.

```console
cargo run -- validate server-01
cargo run -- validate
cargo run -- diff server-01
cargo run -- apply server-01
cargo run -- apply server-01 --yes
cargo run -- apply server-01 --adopt
cargo run -- pull server-01
cargo run -- status server-01
```

`apply` and `pull` require a clean Git worktree and preview their changes before
asking for confirmation. `apply --yes` and `pull --yes` skip that prompt.
Existing unmanaged remote targets are refused; `apply --adopt` creates a
timestamped sibling backup before taking ownership. Apply stages immutable
content under `~/.local/share/stowaway/store` and `/var/lib/stowaway/store`,
rolls file changes back if a later file or script fails, and records state in
`/var/lib/stowaway/state.toml`.

`pull` only imports paths already declared by the local manifest. It imports
file contents, remote deletions, and executable-bit changes, but never creates
a Git commit. Review the resulting worktree and commit it normally.

Managed hosts currently require OpenSSH, Bash, GNU core utilities, and
passwordless `sudo -n` for privileged files, store directories, and deployment
state. SSH authentication and host-key checking are delegated to the user's
OpenSSH configuration. If an interrupted deployment cannot roll back because
the connection or host was lost, inspect the `.stowaway-backup-*` adoption
copies and the last state reported by `stowaway status` before retrying.

Run `tests/debian-container.sh` to exercise home-directory and privileged
`/etc` deployments plus rollback in a disposable Debian container. The test
requires Docker.
