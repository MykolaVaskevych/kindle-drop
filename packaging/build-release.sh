#!/usr/bin/env bash
# Build the release binary and assemble the distributable assets into dist/.
#
# Needs a Rust toolchain plus perl/make/pkg-config/zlib headers (for the vendored
# OpenSSL build). Run it inside an old-glibc container to get a binary that works
# on more than just the machine that built it — that's what .github/workflows/
# release.yml does, and what `--container` below does locally:
#
#     packaging/build-release.sh              # build with the local toolchain
#     packaging/build-release.sh --container  # build in Debian bullseye via docker
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"

# Re-exec inside Debian bullseye (glibc 2.31), which is what CI uses. The image
# already carries every build dependency, so there's no apt-get to go stale.
if [[ "${1:-}" == "--container" ]]; then
    # Keep the container's artifacts out of ./target so the local dev build
    # (different rustc, different glibc) isn't invalidated every time.
    exec docker run --rm -v "$repo":/src -w /src -u "$(id -u):$(id -g)" \
        -e CARGO_HOME=/src/target/container/.cargo \
        -e CARGO_TARGET_DIR=/src/target/container \
        rust:bullseye packaging/build-release.sh
fi

cd "$repo"
target="${CARGO_TARGET_DIR:-target}"
version="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
name="kindle-drop-${version}-x86_64-linux"

echo "==> Building kindle-drop ${version} (vendored OpenSSL)…"
cargo build --release --locked --features vendored-openssl
strip "$target/release/kindle-drop"

echo "==> Staging ${name}…"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -Dm755 "$target/release/kindle-drop"  "$stage/$name/kindle-drop"
install -Dm755 packaging/install-release.sh   "$stage/$name/install.sh"
install -Dm755 uninstall.sh                   "$stage/$name/uninstall.sh"
install -Dm644 packaging/kindle-drop.desktop  "$stage/$name/kindle-drop.desktop"
install -Dm644 packaging/kindle-drop.svg      "$stage/$name/kindle-drop.svg"
install -Dm644 README.md                      "$stage/$name/README.md"
install -Dm644 LICENSE                        "$stage/$name/LICENSE"

echo "==> Writing dist/…"
rm -rf dist
mkdir -p dist
tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
# Bare binary too, for people who just want to run it.
install -Dm755 "$target/release/kindle-drop" dist/kindle-drop
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256" \
         && sha256sum kindle-drop     > kindle-drop.sha256)

echo
echo "Minimum glibc required by this binary:"
objdump -T "$target/release/kindle-drop" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
echo
ls -lh dist
