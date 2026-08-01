# Stowaway

Stowaway is an agentless configuration deployment CLI for Linux servers. It
keeps machine configuration in Git and deploys it over the system `ssh`
client—without installing an agent on the server.

It manages files, scripts, APT packages, Docker containers, and udev rules. It
previews changes before applying them, records deployment state, and supports
rollback when a deployment step fails.

## Install

Install a published release with Cargo:

```console
cargo binstall stowaway-cli
```

Or install the prebuilt release directly:

```console
curl --fail --location https://raw.githubusercontent.com/russweas/stowaway/main/scripts/install-release.sh | bash
```

The installer supports Linux x86_64 and macOS aarch64, verifies the release
checksum, and installs to `~/.local/bin`. Set `STOWAWAY_VERSION` to choose a
version or `STOWAWAY_INSTALL_DIR` to choose another destination. You can also
install from source with `cargo install stowaway-cli`.

## Quick start

Create a Git repository with one directory per machine:

```text
machines/
  server-01/
    machine.toml
    machine.lock
    home/
      .config/example/config.toml
    root/
      etc/example/config
    scripts/
      networking.sh
```

Start with a validation pass, preview the remote changes, and apply them:

```console
stowaway --repo ./infra validate server-01
stowaway --repo ./infra diff server-01
stowaway --repo ./infra apply server-01
```

Create a new machine manifest with its SSH destination prefilled:

```console
stowaway --repo ./infra init server-01
```

Use `--yes` to skip the confirmation prompt. `apply` and `pull` require a
clean Git worktree, so commit the manifest and its files before deploying.

## Machine manifest

Here is a complete example using the supported resource types:

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
source = "root/etc/example/config"
mode = "0644"

[[scripts]]
path = "scripts/networking.sh"
privileged = false
timeout_seconds = 3600

[[apt_packages]]
name = "curl"
version = ">= 7.0, < 9.0"

[[apt_packages]]
name = "docker.io"
version = ">= 20.10"

[[containers]]
name = "stowaway-web"
image = "nginx:1.27"
restart = "unless-stopped"
ports = ["8080:80"]
environment = ["APP_ENV=production"]

[[udev_rules]]
source = "udev/80-storage.rules"
name = "80-storage.rules"
```

Trees deploy files under `~` or `/`. Files under `/` and other privileged
resources use passwordless `sudo -n` on the server.

Scripts receive either `check` or `apply` as their first argument. A script's
`check` command exits 0 when it is converged, 10 when an apply is needed, and
any other status for an error.

APT package versions are comma-separated Debian constraints. Resolve them
against the target host and create the checked-in lock file with:

```console
stowaway lock server-01
```

`apply` refuses a missing or stale lock and installs the exact locked versions.

Containers are managed as root. Stowaway recreates a container when its image,
restart policy, ports, or environment changes. Pin container images by digest
in production instead of using mutable tags.

Udev rules are installed under `/etc/udev/rules.d` and reloaded during apply.
The configured rule name must be a simple `.rules` filename.

## Commands

The global `--repo PATH` option selects the repository root and defaults to the
current directory.

| Command | Purpose |
| --- | --- |
| `init MACHINE` | Create `machines/MACHINE/machine.toml` with a starter manifest. |
| `validate [MACHINE]` | Validate one machine, or every machine when omitted. |
| `lock MACHINE` | Resolve APT ranges and write `machine.lock`. |
| `diff MACHINE` | Preview differences on a host. |
| `apply MACHINE` | Preview, confirm, and deploy a machine. `--yes` skips confirmation; `--adopt` backs up unmanaged targets before taking ownership. |
| `pull MACHINE` | Preview and import changes from declared remote files. `--yes` skips confirmation. |
| `status MACHINE` | Show the last deployment recorded on a host. |
| `devices MACHINE` | Stream udev events. `--subsystem NAME` filters events, such as `block` or `net`. |

Examples:

```console
stowaway validate
stowaway diff server-01
stowaway apply server-01 --yes
stowaway apply server-01 --adopt
stowaway pull server-01 --yes
stowaway status server-01
stowaway devices server-01 --subsystem block
```

## Safety and host requirements

Stowaway preserves OpenSSH host-key verification and delegates authentication
to the user's SSH configuration. Managed hosts require OpenSSH, Bash, GNU core
utilities, and passwordless `sudo -n` for privileged operations. Docker is
required when the manifest declares containers.

Before changing a managed target, Stowaway checks ownership and refuses
unmanaged collisions by default. `--adopt` creates a timestamped
`.stowaway-backup-*` copy before taking ownership. File deployment uses
digest-addressed staging and a rollback journal. If the SSH connection is
lost during deployment, inspect those backups and run `stowaway status` before
retrying.

`pull` only imports paths already declared in the local manifest. It never
creates a commit; review and commit the resulting worktree yourself.

For development and release instructions, see
[`CONTRIBUTING.md`](CONTRIBUTING.md).
