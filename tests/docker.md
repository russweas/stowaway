# Docker integration containers

`debian-container.sh` runs the deployment and rollback integration test in a
disposable Debian container by default:

```console
tests/debian-container.sh
```

Use `--keep` with a stable name when a container should survive the test. An
existing container with that name is reused and started if necessary; the
container is configured with Docker's `unless-stopped` restart policy.

```console
tests/debian-container.sh --keep --name stowaway-debian
docker ps --filter name=stowaway-debian
tests/debian-container.sh --rm --name stowaway-debian
```

The test still builds the local binary and Docker image on each invocation.
The persistent mode is intended for iterative inspection and debugging; the
default disposable mode is the normal CI check.
