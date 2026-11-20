#!/usr/bin/env bash
set -euo pipefail

# Install the Ghost CLI.
#
#   bash scripts/install.sh          # print the plan, change nothing
#   bash scripts/install.sh --yes    # perform the install
#
# Order:
#   1. use an existing `ghost`
#   2. build from $GHOST_SOURCE with cargo
#   3. install the published crate with cargo if available
#   4. download a release tarball and verify its SHA-256
#
# Environment:
#   GHOST_SOURCE    path to a Ghost checkout (preferred for development)
#   GHOST_VERSION   release tag to download (default: v2.0.0)
#   GHOST_BIN_DIR   install directory for a downloaded binary (default: ~/.local/bin)

REPO="Shreyas-a-j/ghost"
VERSION="${GHOST_VERSION:-v2.0.0}"
BIN_DIR="${GHOST_BIN_DIR:-$HOME/.local/bin}"
CONFIRM="${1:-}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if command -v ghost >/dev/null 2>&1; then
  say "ghost is already installed: $(ghost --version 2>/dev/null || echo 'unknown version')"
  exit 0
fi

if [ -n "${GHOST_SOURCE:-}" ]; then
  [ -d "$GHOST_SOURCE" ] || die "GHOST_SOURCE is not a directory: $GHOST_SOURCE"
  [ -f "$GHOST_SOURCE/Cargo.toml" ] || die "GHOST_SOURCE has no Cargo.toml: $GHOST_SOURCE"
  if [ "$CONFIRM" != "--yes" ]; then
    say "Would run: cargo install --path '$GHOST_SOURCE' --locked"
    say "Re-run with --yes to install."
    exit 0
  fi
  command -v cargo >/dev/null 2>&1 || die "cargo is required to build from source"
  cargo install --path "$GHOST_SOURCE" --locked
  exit 0
fi

if command -v cargo >/dev/null 2>&1; then
  if [ "$CONFIRM" != "--yes" ]; then
    say "Would try: cargo install ghost-workflow --locked"
    say "Fallback: download ghost-<target>.tar.gz from https://github.com/$REPO/releases"
    say "Re-run with --yes to install."
    exit 0
  fi
  say "trying cargo install ghost-workflow --locked"
  if cargo install ghost-workflow --locked; then
    exit 0
  fi
  say "crates.io install failed; falling back to a release archive"
else
  say "cargo not found; using a release archive"
fi

[ "$CONFIRM" = "--yes" ] || {
  say "Would download a release archive for this platform."
  say "Re-run with --yes to install."
  exit 0
}

case "$(uname -s)" in
  Linux)  os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) die "unsupported operating system: $(uname -s); Ghost currently supports Linux and macOS" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) die "unsupported architecture: $(uname -m)" ;;
esac

TARGET="${arch}-${os}"
ASSET="ghost-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

command -v curl >/dev/null 2>&1 || die "curl is required to download $ASSET"
command -v tar >/dev/null 2>&1 || die "tar is required to unpack $ASSET"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

say "downloading $URL"
curl -fsSL "$URL" -o "$TMP/$ASSET"
curl -fsSL "$URL.sha256" -o "$TMP/$ASSET.sha256"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$TMP" && sha256sum -c "$ASSET.sha256")
else
  (cd "$TMP" && shasum -a 256 -c "$ASSET.sha256")
fi

tar -C "$TMP" -xzf "$TMP/$ASSET"
mkdir -p "$BIN_DIR"
install -m 0755 "$TMP/ghost" "$BIN_DIR/ghost"
say "installed $BIN_DIR/ghost"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "add $BIN_DIR to PATH to run 'ghost' from any directory" ;;
esac
