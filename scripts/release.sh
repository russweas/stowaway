#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

usage() {
    echo "Usage: $0 [v]VERSION" >&2
    echo "Example: $0 v0.1.0" >&2
    exit 2
}

[[ $# -eq 1 ]] || usage
tag=$1
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || usage

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
[[ "$tag" == "v$version" ]] || {
    echo "release tag $tag does not match Cargo.toml version $version" >&2
    exit 1
}

[[ -z "$(git status --porcelain)" ]] || {
    echo "the working tree must be clean" >&2
    exit 1
}

gh auth status >/dev/null
branch=$(git branch --show-current)
[[ -n "$branch" ]] || {
    echo "release from a named branch, not detached HEAD" >&2
    exit 1
}

git diff --quiet "origin/$branch" HEAD 2>/dev/null || {
    echo "local HEAD is not equal to origin/$branch; push it before releasing" >&2
    exit 1
}

gh workflow run release.yaml --ref "$branch" --field "tag=$tag"
echo "Release workflow dispatched for $tag. Monitor it with:"
echo "  gh run list --workflow release.yaml --limit 1"
