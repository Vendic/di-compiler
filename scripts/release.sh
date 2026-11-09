#!/usr/bin/env bash
# Builds fast-di-compile for macOS and Linux (arm64 and amd64) on this Mac and publishes a
# GitHub release on Vendic/di-compiler with the four binaries and checksums.txt.
# Runs locally on purpose: the release does not use GitHub Actions.
#
# Usage: scripts/release.sh <version>            for example: scripts/release.sh 0.1.0
#   NOTES_FILE=notes.md   use this file as release notes instead of GitHub's generated notes
#   DRY_RUN=1             build and check everything, but do not push the tag or publish
#
# Needs: a Mac on Apple silicon, rustup, Docker (OrbStack or Docker Desktop) for the Linux
# builds, and gh logged in with write access to Vendic/di-compiler.
#
# Before you run it: set [workspace.package] version in Cargo.toml to <version> and merge
# that to main. The script releases the commit that origin/main points to, nothing else.
set -euo pipefail

REPO=Vendic/di-compiler
BIN=fast-di-compile
VERSION=${1:-}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIST="$ROOT/dist/$VERSION"
TAG_CREATED=0

fail() { echo "release: $*" >&2; exit 1; }
step() { echo; echo "==> $*"; }

cleanup() {
    # A tag made by this run is removed again when the run does not publish.
    if [ "$TAG_CREATED" = 1 ]; then git -C "$ROOT" tag -d "$VERSION" > /dev/null; fi
}
trap cleanup EXIT

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "usage: scripts/release.sh <version>, for example 0.1.0"
[ "$(uname -s) $(uname -m)" = "Darwin arm64" ] || fail "run this on a Mac with Apple silicon"
command -v docker > /dev/null && docker info > /dev/null 2>&1 || fail "Docker is not running"
gh auth status > /dev/null 2>&1 || fail "gh is not logged in"
[ -z "${NOTES_FILE:-}" ] || [ -f "$NOTES_FILE" ] || fail "NOTES_FILE $NOTES_FILE does not exist"
cd "$ROOT"

step "Checking the checkout"
git fetch --quiet origin main
[ -z "$(git status --porcelain)" ] || fail "the working tree is not clean"
if [ "${DRY_RUN:-0}" = 1 ]; then
    echo "Dry run: the branch $(git branch --show-current) is not checked against origin/main"
else
    [ "$(git branch --show-current)" = main ] || fail "check out main first"
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || fail "main is not equal to origin/main"
fi
cargo_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ "$cargo_version" = "$VERSION" ] || fail "Cargo.toml says $cargo_version, set it to $VERSION and merge that first"
git ls-remote --exit-code --tags origin "refs/tags/$VERSION" > /dev/null 2>&1 && fail "tag $VERSION already exists on origin"
gh release view "$VERSION" -R "$REPO" > /dev/null 2>&1 && fail "release $VERSION already exists"
git rev-parse -q --verify "refs/tags/$VERSION" > /dev/null && fail "tag $VERSION already exists locally, remove it with: git tag -d $VERSION"
COMMIT=$(git rev-parse HEAD)
echo "Releasing $VERSION from $(git log --oneline -1)"

step "Checking formatting, Clippy and tests"
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace

# The binary reports `git describe --tags`, so the tag must be on HEAD before the build.
git tag -a "$VERSION" -m "$VERSION" "$COMMIT"
TAG_CREATED=1

rm -rf "$DIST"
mkdir -p "$DIST"

step "Building macOS arm64 and amd64"
rustup target add aarch64-apple-darwin x86_64-apple-darwin > /dev/null
for pair in aarch64-apple-darwin:darwin-arm64 x86_64-apple-darwin:darwin-amd64; do
    target=${pair%%:*} asset="$BIN-${pair#*:}"
    cargo build --locked --release -p "$BIN" --target "$target"
    cp "target/$target/release/$BIN" "$DIST/$asset"
done

step "Building Linux arm64 and amd64 in Docker"
# Same Rust version as this Mac. Debian bookworm keeps the glibc requirement low enough for
# Ubuntu 22.04 and newer. Each architecture gets its own target dir and registry cache.
rust_version=$(rustc --version | awk '{print $2}')
for arch in arm64 amd64; do
    docker run --rm --platform "linux/$arch" \
        -v "$ROOT":/src -w /src \
        -v "di-compiler-cargo-registry-$arch":/usr/local/cargo/registry \
        -e CARGO_TARGET_DIR="/src/target/docker-linux-$arch" \
        "rust:$rust_version-bookworm" \
        sh -c "git config --global --add safe.directory /src && cargo build --locked --release -p $BIN"
    cp "target/docker-linux-$arch/release/$BIN" "$DIST/$BIN-linux-$arch"
done

step "Checking the binaries"
check_version() {
    local asset=$1 reported=$2
    [ "$reported" = "$BIN $VERSION" ] || fail "$asset reports '$reported', expected '$BIN $VERSION'"
    echo "$asset: $reported"
}
check_version "$BIN-darwin-arm64" "$("$DIST/$BIN-darwin-arm64" --version)"
if arch -x86_64 /usr/bin/true 2> /dev/null; then
    check_version "$BIN-darwin-amd64" "$(arch -x86_64 "$DIST/$BIN-darwin-amd64" --version)"
else
    echo "$BIN-darwin-amd64: not run, Rosetta is not installed (softwareupdate --install-rosetta)"
fi
for arch in arm64 amd64; do
    check_version "$BIN-linux-$arch" "$(docker run --rm --platform "linux/$arch" -v "$DIST":/dist:ro \
        debian:bookworm-slim "/dist/$BIN-linux-$arch" --version)"
done
file "$DIST"/$BIN-*

(cd "$DIST" && shasum -a 256 $BIN-* > checksums.txt && cat checksums.txt)

if [ "${DRY_RUN:-0}" = 1 ]; then
    echo; echo "Dry run: built and checked $VERSION in $DIST, nothing was pushed or published."
    exit 0
fi

step "Publishing $VERSION on $REPO"
git push origin "refs/tags/$VERSION"
TAG_CREATED=0
notes=(--generate-notes)
[ -n "${NOTES_FILE:-}" ] && notes=(--notes-file "$NOTES_FILE")
gh release create "$VERSION" -R "$REPO" --verify-tag --title "$VERSION" "${notes[@]}" \
    "$DIST"/$BIN-* "$DIST/checksums.txt"

# Download what was published and check it against the local checksums.
verify=$(mktemp -d)
gh release download "$VERSION" -R "$REPO" -D "$verify"
(cd "$verify" && shasum -a 256 -c checksums.txt)
cmp "$verify/checksums.txt" "$DIST/checksums.txt"
rm -rf "$verify"
echo; echo "Published: https://github.com/$REPO/releases/tag/$VERSION"
