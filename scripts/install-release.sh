#!/usr/bin/env bash
set -euo pipefail

version=${STOWAWAY_VERSION:-latest}
install_dir=${STOWAWAY_INSTALL_DIR:-$HOME/.local/bin}
repo="russweas/stowaway"

case "$(uname -s):$(uname -m)" in
    Linux:x86_64) target=x86_64-unknown-linux-gnu ;;
    Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
    *) echo "unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

command -v curl >/dev/null || { echo "curl is required" >&2; exit 1; }
command -v tar >/dev/null || { echo "tar is required" >&2; exit 1; }

temporary_dir=$(mktemp -d)
trap 'rm -rf "$temporary_dir"' EXIT
asset="stowaway-${version}-${target}"
base_url="https://github.com/${repo}/releases"
if [[ "$version" == latest ]]; then
    download_url="$base_url/latest/download"
else
    download_url="$base_url/download/v${version#v}"
fi

curl --fail --location --silent --show-error "$download_url/$asset.tar.gz" --output "$temporary_dir/$asset.tar.gz"
curl --fail --location --silent --show-error "$download_url/$asset.tar.gz.sha256" --output "$temporary_dir/$asset.tar.gz.sha256"
(cd "$temporary_dir" && if command -v sha256sum >/dev/null; then
    sha256sum --check "$asset.tar.gz.sha256"
else
    shasum --algorithm 256 --check "$asset.tar.gz.sha256"
fi)
tar --extract --gzip --file "$temporary_dir/$asset.tar.gz" --directory "$temporary_dir"
install -Dm755 "$temporary_dir/$asset/stowaway" "$install_dir/stowaway"
echo "Installed stowaway to $install_dir/stowaway"
