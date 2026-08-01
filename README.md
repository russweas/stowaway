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

## Installing a release

After publishing a release, install the Linux x86_64 binary with
[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```console
cargo binstall stowaway
```

This uses the release archive when the package is published on crates.io and
falls back to compiling when no compatible binary is available. The release
tooling publishes Linux x86_64 and macOS aarch64 binaries. By default it builds
the current host target. To add the macOS binary from an Apple Silicon Mac
after creating the Linux release, run:

```console
./scripts/release.sh v0.1.1 aarch64-apple-darwin
```

When the GitHub release already exists, the script uploads the new target
instead of trying to create the release again. To compile and install directly
from GitHub, use:

```console
cargo install --git https://github.com/russweas/stowaway.git stowaway
```

To install a prebuilt release without Cargo, run:

```console
curl --fail --location https://raw.githubusercontent.com/russweas/stowaway/main/scripts/install-release.sh | bash
```

The installer supports Linux x86_64 and macOS aarch64, verifies the checksum,
and installs to `~/.local/bin`. Set `STOWAWAY_VERSION` for a specific version
or `STOWAWAY_INSTALL_DIR` for another destination. Other platforms can use
`cargo install stowaway` or build from this repository.

To publish a release locally, update the `version` in `Cargo.toml`, commit and
push it, then run:

```console
./scripts/release.sh v0.1.0
```

The script requires an authenticated `gh` CLI and a clean worktree. With Git,
the local branch must match its `origin` branch. With Jujutsu, the release
revision must have a bookmark whose remote bookmark points to the same commit;
the normal empty Jujutsu working-copy commit is handled automatically.
It builds the selected target locally and creates or updates the GitHub release
with generated notes. The tag must match the Cargo version. Building macOS
from Linux requires an Apple SDK and cross-linker such as an osxcross setup;
the recommended path is to build that target on an Apple Silicon Mac.

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
