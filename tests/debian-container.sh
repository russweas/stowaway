#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temporary=$(mktemp -d)
container="stowaway-debian"
persistent=false

usage() {
    echo "usage: $0 [--keep] [--name CONTAINER]" >&2
    echo "       $0 --rm [--name CONTAINER]" >&2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --keep)
            persistent=true
            ;;
        --name)
            [ "$#" -ge 2 ] || { usage; exit 2; }
            container=$2
            shift
            ;;
        --rm)
            docker rm --force -- "$container" >/dev/null 2>&1 || true
            exit 0
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            usage
            exit 2
            ;;
    esac
    shift
done

cleanup() {
    if [ "$persistent" = false ]; then
        docker rm --force "$container" >/dev/null 2>&1 || true
    fi
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

cargo build --manifest-path "$project_root/Cargo.toml"
docker build --tag stowaway-debian-test "$project_root/tests/debian"
if [ "$persistent" = true ] && docker container inspect "$container" >/dev/null 2>&1; then
    if [ "$(docker container inspect --format '{{.State.Running}}' "$container")" != true ]; then
        docker start "$container" >/dev/null
    fi
else
    if [ "$persistent" = true ]; then
        docker run --detach --restart unless-stopped --name "$container" \
            stowaway-debian-test >/dev/null
    else
        docker run --detach --name "$container" stowaway-debian-test >/dev/null
    fi
fi

mkdir -p "$temporary/bin" "$temporary/repository/machines/debian/home/.config/server-tool"
mkdir -p "$temporary/repository/machines/debian/root/etc/network"
printf 'home configuration\n' > "$temporary/repository/machines/debian/home/.config/server-tool/config"
printf 'network configuration\n' > "$temporary/repository/machines/debian/root/etc/network/config"
cat > "$temporary/repository/machines/debian/machine.toml" <<'EOF'
version = 1

[ssh]
destination = "debian-container"

[[trees]]
source = "home"
target = "~"

[[trees]]
source = "root"
target = "/"
privileged = true
EOF

cat > "$temporary/bin/ssh" <<EOF
#!/bin/sh
set -eu
[ "\$1" = -- ]
shift
[ "\$1" = debian-container ]
shift
exec docker exec --interactive --env HOME=/home/stowaway \
    --workdir /home/stowaway '$container' bash -c "\$1"
EOF
chmod 0755 "$temporary/bin/ssh"

git -C "$temporary/repository" init --quiet
git -C "$temporary/repository" add .
git -C "$temporary/repository" \
    -c user.name='Stowaway Tests' \
    -c user.email=stowaway@example.invalid \
    commit --quiet --message fixture

PATH="$temporary/bin:$PATH" "$project_root/target/debug/stowaway" \
    --repo "$temporary/repository" apply debian --yes

docker exec "$container" test -L /home/stowaway/.config/server-tool/config
docker exec "$container" test -L /etc/network/config
docker exec "$container" grep --fixed-strings --line-regexp 'home configuration' \
    /home/stowaway/.config/server-tool/config
docker exec "$container" grep --fixed-strings --line-regexp 'network configuration' \
    /etc/network/config
docker exec "$container" grep --fixed-strings --line-regexp 'machine = "debian"' \
    /var/lib/stowaway/state.toml

state_before=$(docker exec "$container" grep '^content_digest = ' /var/lib/stowaway/state.toml)
printf 'changed home configuration\n' \
    > "$temporary/repository/machines/debian/home/.config/server-tool/config"
printf 'changed network configuration\n' \
    > "$temporary/repository/machines/debian/root/etc/network/config"
mkdir -p "$temporary/repository/machines/debian/scripts"
cat > "$temporary/repository/machines/debian/scripts/fail.sh" <<'EOF'
case "$1" in
    check) exit 10 ;;
    apply) exit 42 ;;
esac
EOF
cat >> "$temporary/repository/machines/debian/machine.toml" <<'EOF'

[[scripts]]
path = "scripts/fail.sh"
timeout_seconds = 10
EOF
git -C "$temporary/repository" add .
git -C "$temporary/repository" \
    -c user.name='Stowaway Tests' \
    -c user.email=stowaway@example.invalid \
    commit --quiet --message rollback-test

if PATH="$temporary/bin:$PATH" "$project_root/target/debug/stowaway" \
    --repo "$temporary/repository" apply debian --yes; then
    echo 'rollback test unexpectedly succeeded' >&2
    exit 1
fi

docker exec "$container" grep --fixed-strings --line-regexp 'home configuration' \
    /home/stowaway/.config/server-tool/config
docker exec "$container" grep --fixed-strings --line-regexp 'network configuration' \
    /etc/network/config
state_after=$(docker exec "$container" grep '^content_digest = ' /var/lib/stowaway/state.toml)
[ "$state_before" = "$state_after" ]

printf 'Debian deployment and rollback tests passed\n'
if [ "$persistent" = true ]; then
    printf 'Container %s is still running; remove it with %s --rm --name %s\n' \
        "$container" "$0" "$container"
fi
