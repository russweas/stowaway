# Docker integration containers

`debian-container.sh` runs the deployment and rollback integration test in a
disposable Debian container by default:

```console
tests/debian-container.sh
```

Use `--keep` with a stable Compose project name when a container should survive
the test. Compose reuses the project and starts the service if necessary. The
service is configured with Docker Compose's `restart: unless-stopped` policy,
so it restarts after Docker starts on a host reboot.

```console
tests/debian-container.sh --keep --name stowaway-debian
docker compose --project-name stowaway-debian --file tests/debian/compose.yaml ps
tests/debian-container.sh --rm --name stowaway-debian
```

The test still builds the local binary and Compose image on each invocation.
The persistent mode is intended for iterative inspection and debugging; the
default disposable mode is the normal CI check.
