#!/bin/sh
# Install the `agentos` CLI from a published GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/anpl1623/AgentOS/main/scripts/install.sh | sh
#
# This installs the CLI only. The desktop application is an unsigned GUI bundle
# and is deliberately not something a shell pipeline drops onto your machine;
# take it from the releases page, and read what the release notes say about the
# missing signature before you open it.
#
# Nothing here needs root. The binary goes to ~/.local/bin unless you say
# otherwise, because an installer that asks for sudo to write a single file has
# asked for far more than it needs.
#
# Environment:
#   AGENTOS_VERSION      Version to install, e.g. 0.2.0. Default: the latest release.
#   AGENTOS_INSTALL_DIR  Where the binary goes. Default: ~/.local/bin.

set -eu

REPO="anpl1623/AgentOS"
INSTALL_DIR="${AGENTOS_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "this script needs \`$1\` and cannot find it"
}

# The downloader and the checksum tool are the two things there is no working
# around, so they are resolved before anything is fetched rather than halfway
# through an install.
need uname
need tar

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    fetch_stdout() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
    fetch_stdout() { wget -qO- "$1"; }
else
    die "this script needs either \`curl\` or \`wget\` and cannot find either"
fi

if command -v sha256sum >/dev/null 2>&1; then
    checksum() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    checksum() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    die "this script needs \`sha256sum\` or \`shasum\` to verify the download"
fi

# --- Which build ------------------------------------------------------------

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
    Darwin)
        case "$arch" in
            arm64|aarch64) target="aarch64-apple-darwin" ;;
            x86_64)        target="x86_64-apple-darwin" ;;
            *) die "no macOS build for $arch. Build from source: https://github.com/$REPO#development" ;;
        esac
        ;;
    Linux)
        case "$arch" in
            x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
            *) die "no Linux build for $arch. Build from source: https://github.com/$REPO#development" ;;
        esac
        ;;
    MINGW*|MSYS*|CYGWIN*)
        die "Windows ships as a .zip on the releases page rather than through this script: https://github.com/$REPO/releases/latest"
        ;;
    *)
        die "unrecognised system '$os'. Build from source: https://github.com/$REPO#development"
        ;;
esac

# --- Which version ----------------------------------------------------------

version="${AGENTOS_VERSION:-}"
if [ -z "$version" ]; then
    say "Looking up the latest release..."
    # The tag is the only field needed, so it is read out of the API response
    # rather than by following the /latest redirect. An unreachable API and a
    # repository with no published release both land here as an empty string,
    # and the message covers both because the fix is the same either way.
    version="$(
        fetch_stdout "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
            | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' \
            | head -n 1
    )" || true
    [ -n "$version" ] || die "no published release found for $REPO, or the GitHub API is unreachable.
Set AGENTOS_VERSION to install a specific version, or build from source:
  https://github.com/$REPO#development"
fi

name="agentos-$version-$target"
base="https://github.com/$REPO/releases/download/v$version"

# --- Download and verify ----------------------------------------------------

tmp="$(mktemp -d)"
# Cleanup on every exit path, the failures included, so a broken download does
# not leave an archive of unverified bytes lying around in /tmp.
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading agentos $version for $target..."
fetch "$base/$name.tar.gz" "$tmp/$name.tar.gz" \
    || die "could not download $base/$name.tar.gz"
fetch "$base/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" \
    || die "could not download the checksum for $name.tar.gz"

# The checksum is checked before the archive is opened. Handing `tar` an
# attacker-controlled file is already a decision, and this is the point at which
# it stops being one.
expected="$(cut -d' ' -f1 < "$tmp/$name.tar.gz.sha256")"
actual="$(checksum "$tmp/$name.tar.gz")"
if [ "$expected" != "$actual" ]; then
    die "checksum mismatch for $name.tar.gz
  expected $expected
  got      $actual
Nothing has been installed. Do not run the downloaded file."
fi
say "Checksum verified."

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
[ -f "$tmp/$name/agentos" ] || die "the archive did not contain an \`agentos\` binary"

# --- Install ----------------------------------------------------------------

mkdir -p "$INSTALL_DIR" || die "could not create $INSTALL_DIR"
if ! install -m 755 "$tmp/$name/agentos" "$INSTALL_DIR/agentos" 2>/dev/null; then
    cp "$tmp/$name/agentos" "$INSTALL_DIR/agentos" \
        || die "could not write to $INSTALL_DIR. Set AGENTOS_INSTALL_DIR to somewhere you can write."
    chmod 755 "$INSTALL_DIR/agentos"
fi

say ""
say "Installed agentos $version to $INSTALL_DIR/agentos"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        say ""
        say "$INSTALL_DIR is not on your PATH. Add it:"
        say ""
        say "  export PATH=\"\$PATH:$INSTALL_DIR\""
        ;;
esac

say ""
say "Next:"
say ""
say "  agentos doctor           # check the install and report what is missing"
say "  agentos demo --scripted  # the end-to-end demonstration, no API key needed"
say ""
