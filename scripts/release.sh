#!/usr/bin/env bash
set -euo pipefail

if command -v jj >/dev/null && repo_root=$(jj root 2>/dev/null); then
    vcs=jj
else
    vcs=git
    repo_root=$(git rev-parse --show-toplevel)
fi
cd "$repo_root"
[[ $# -ge 1 ]] || { echo "Usage: $0 vVERSION [TARGET ...]" >&2; exit 2; }
tag=$1
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || { echo "invalid release tag: $tag" >&2; exit 2; }
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
[[ "$tag" == "v$version" ]] || { echo "release tag $tag does not match Cargo.toml version $version" >&2; exit 1; }
if [[ "$vcs" == jj ]]; then
    jj_status=$(jj status 2>&1) || { printf '%s\n' "$jj_status" >&2; exit 1; }
    grep -qiE 'working copy (is clean|has no changes)' <<<"$jj_status" || {
        echo "Jujutsu working copy must be clean" >&2
        exit 1
    }
    release_rev=@
    if jj log -r '@ & empty()' --no-graph -T 'commit_id' 2>/dev/null | grep -q .; then
        release_rev=@-
    fi
    jj log -r "$release_rev & bookmarks()" --no-graph -T 'bookmarks' 2>/dev/null | grep -q . || {
        echo "the Jujutsu release revision must have a bookmark" >&2
        exit 1
    }
    jj log -r "$release_rev & remote_bookmarks()" --no-graph -T 'remote_bookmarks' 2>/dev/null | grep -q . || {
        echo "the Jujutsu release bookmark must be pushed" >&2
        exit 1
    }
    release_target=$(jj log -r "$release_rev" --no-graph -T 'commit_id')
else
    [[ -z "$(git status --porcelain)" ]] || { echo "working tree must be clean" >&2; exit 1; }
    branch=$(git branch --show-current)
    [[ -n "$branch" ]] || { echo "release from a named branch" >&2; exit 1; }
    git diff --quiet "origin/$branch" HEAD 2>/dev/null || { echo "local HEAD is not equal to origin/$branch; push it first" >&2; exit 1; }
    release_target=HEAD
fi

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }
command -v gh >/dev/null || { echo "gh is required" >&2; exit 1; }
gh auth status >/dev/null

if [[ $# -gt 1 ]]; then
    targets=("${@:2}")
else
    targets=("$(rustc -vV | sed -n 's/^host: //p')")
fi
dist=$(mktemp -d)
trap 'rm -rf "$dist"' EXIT

for target in "${targets[@]}"; do
    if command -v rustup >/dev/null && ! rustup target list --installed 2>/dev/null | grep -Fxq "$target"; then
        echo "Installing Rust target $target..."
        rustup target add "$target"
    fi
    cargo build --locked --release --target "$target"
    versioned="stowaway-cli-${version}-${target}"
    mkdir -p "$dist/$versioned"
    install -Dm755 "target/$target/release/stowaway" "$dist/$versioned/stowaway"
    tar -C "$dist/$versioned" -czf "$dist/$versioned.tar.gz" stowaway
    if command -v sha256sum >/dev/null; then
        sha256sum "$dist/$versioned.tar.gz" > "$dist/$versioned.tar.gz.sha256"
    else
        shasum -a 256 "$dist/$versioned.tar.gz" > "$dist/$versioned.tar.gz.sha256"
    fi
done

cargo publish --locked

assets=("$dist"/*.tar.gz "$dist"/*.tar.gz.sha256)
if gh release view "$tag" >/dev/null 2>&1; then
    gh release upload "$tag" "${assets[@]}" --clobber
else
    gh release create "$tag" "${assets[@]}" \
        --target "$release_target" \
        --title "Stowaway $tag" \
        --generate-notes
fi

echo "Created GitHub release $tag."
