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
    udev/
      80-storage.rules
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

[[udev_rules]]
source = "udev/80-storage.rules"
name = "80-storage.rules"
```

Udev rules are installed as root under `/etc/udev/rules.d` and Stowaway
reloads the udev rule database during apply. The rule source must
be a regular file and its configured name must be a simple `.rules` filename.
Rules affect subsequent udev events; use `stowaway devices` to watch those
events and inspect the properties reported by the host.

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
cargo run -- devices server-01
cargo run -- devices server-01 --subsystem block
```

`devices` runs `udevadm monitor --udev --property` over SSH and streams device
events until interrupted. It uses the host's normal udev permissions and does
not change the server.

## Command reference

The global `--repo PATH` option selects the repository root and defaults to the
current directory. It can be used with every command.

| Command | Purpose | Options and arguments |
| --- | --- | --- |
| `validate [MACHINE]` | Validate one machine, or all machines when omitted. | `MACHINE` is optional. |
| `diff MACHINE` | Preview file and script differences on a host. | `MACHINE` is required. |
| `apply MACHINE` | Preview, confirm, and deploy a machine configuration. | `--yes` skips confirmation; `--adopt` backs up and takes ownership of unmanaged targets. |
| `pull MACHINE` | Preview and import declared remote changes into the local worktree. | `--yes` skips confirmation. |
| `status MACHINE` | Show the last deployment recorded on a host. | `MACHINE` is required. |
| `devices MACHINE` | Stream newly observed udev device events and properties from a host. | `--subsystem NAME` filters events, for example `block` or `net`. |

Examples:

```console
stowaway --repo ./infra validate
stowaway diff server-01
stowaway apply server-01 --yes
stowaway pull server-01 --yes
stowaway status server-01
stowaway devices server-01 --subsystem block
```

## How Stowaway compares

These tools overlap in configuration management, but they solve different
problems and operate at different scopes:

| Tool | Primary model | Where it runs | Best fit | How Stowaway differs |
| --- | --- | --- | --- | --- |
| **Stowaway** | Git-managed per-machine files, scripts, and udev rules; transactional deployment over SSH. | Locally, with the target server accessed through its existing OpenSSH setup. | Small fleets and administrators who want reviewable server configuration without installing an agent. | Focuses on explicit file ownership, previews, pull, adoption backups, and rollback rather than continuous convergence. |
| [Puppet](https://www.puppet.com/) | Declarative resource catalogs with recurring convergence. | Usually a Puppet agent on each node, commonly backed by a Puppet server. | Organization-wide policy, inventory, secrets integration, and large heterogeneous fleets. | Stowaway has no agent or central server and leaves scheduling and orchestration to the operator or existing automation. |
| [Nix Home Manager](https://github.com/nix-community/home-manager) | Declarative Nix expressions for a reproducible user environment and generations. | Primarily on the local machine or user account, using the Nix store. | Reproducible packages, shell environments, and user-level dotfiles with generation-based rollbacks. | Stowaway uses ordinary files in a Git repository, targets remote Linux servers, and supports privileged system files and operational scripts without requiring Nix. |
| [GNU Stow](https://www.gnu.org/software/stow/) | A symlink farm that maps package directories into a local filesystem tree. | Locally on one filesystem. | Lightweight management of dotfiles or independently packaged local software. | Stowaway adds SSH transport, manifest validation, remote inspection, deployment state, transactional apply, and machine-specific configuration. |

In short, choose Stowaway when the deployment boundary is a server reached by
SSH and you want a small, inspectable Git workflow. Choose Puppet for managed
fleet policy and continuous enforcement, Home Manager for reproducible Nix
user environments, or GNU Stow for simple local symlink management.

## Installing a release

After publishing a release, install the Linux x86_64 binary with
[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```console
cargo binstall stowaway-cli
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
cargo install --git https://github.com/russweas/stowaway.git --bin stowaway
```

To install a prebuilt release without Cargo, run:

```console
curl --fail --location https://raw.githubusercontent.com/russweas/stowaway/main/scripts/install-release.sh | bash
```

The installer supports Linux x86_64 and macOS aarch64, verifies the checksum,
and installs to `~/.local/bin`. Set `STOWAWAY_VERSION` for a specific version
or `STOWAWAY_INSTALL_DIR` for another destination. Other platforms can use
`cargo install stowaway-cli` or build from this repository.

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
