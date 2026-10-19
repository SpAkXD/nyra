#!/bin/sh
# Installs Nyra (https://nyralang.dev) on macOS and Linux, for the current user only (no sudo).
#
#   curl -fsSL https://nyralang.dev/install.sh | sh
#   curl -fsSL https://nyralang.dev/install.sh | sh -s -- --version 0.7.0
#   sh install.sh --uninstall
#
# What it does:
#   1. picks the release archive for this computer (x86_64 Linux, Apple Silicon macOS; also x86_64
#      Windows from Git Bash, MSYS2 or Cygwin),
#   2. downloads it and SHA256SUMS.txt from the GitHub release and checks the archive's SHA-256,
#   3. puts the `nyra` executable in ~/.nyra/bin (or $NYRA_HOME/bin),
#   4. adds that folder to PATH in your shell's startup file (one marked line; --no-path skips it),
#   5. checks for a C compiler (only native programs need one).
# Running it again installs the requested version over the old one; nothing is added twice.
#
# Options:
#   --version X    install version X (0.7.0 or v0.7.0) instead of the latest release
#   --no-path      do not change any shell startup file
#   --uninstall    remove the executable and the PATH line
#   -h, --help     show this help
# Environment:
#   NYRA_HOME      where to install (default: ~/.nyra); the executable goes in $NYRA_HOME/bin
#   NYRA_VERSION   same as --version
#   NYRA_REPO      the GitHub repository to download from (default: SpAkXD/nyra)

set -eu

REPO="${NYRA_REPO:-SpAkXD/nyra}"
NYRA_HOME="${NYRA_HOME:-$HOME/.nyra}"
BIN="$NYRA_HOME/bin"
VERSION="${NYRA_VERSION:-}"
MODIFY_PATH=1
ACTION=install
MARK="# added by the Nyra installer"

say() { printf '%s\n' "$*"; }
err() {
    printf 'nyra install: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Installs Nyra (https://nyralang.dev) for the current user.

usage: sh install.sh [--version X] [--no-path] [--uninstall]

  --version X    install version X (0.7.0 or v0.7.0) instead of the latest release
  --no-path      do not change any shell startup file
  --uninstall    remove the executable and the PATH line
  NYRA_HOME      where to install (default: ~/.nyra); the executable goes in $NYRA_HOME/bin
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) [ $# -ge 2 ] || err "--version needs a version, e.g. --version 0.7.0"; VERSION="$2"; shift 2 ;;
        --version=*) VERSION="${1#--version=}"; shift ;;
        --no-path) MODIFY_PATH=0; shift ;;
        --uninstall) ACTION=uninstall; shift ;;
        -h | --help) usage; exit 0 ;;
        *) err "unknown option '$1' (try --help)" ;;
    esac
done

# --- the shell startup file that gets the PATH line --------------------------------------------

rc_file() {
    case "${SHELL:-}" in
        */zsh) printf '%s\n' "${ZDOTDIR:-$HOME}/.zshrc" ;;
        */bash)
            # macOS Terminal starts login shells, which read .bash_profile, not .bashrc
            if [ "$(uname -s)" = Darwin ]; then printf '%s\n' "$HOME/.bash_profile"; else printf '%s\n' "$HOME/.bashrc"; fi ;;
        */fish) printf '%s\n' "${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/nyra.fish" ;;
        *) printf '%s\n' "$HOME/.profile" ;;
    esac
}

path_line() {
    case "$1" in
        *.fish) printf 'fish_add_path "%s" %s\n' "$BIN" "$MARK" ;;
        *) printf 'export PATH="%s:$PATH" %s\n' "$BIN" "$MARK" ;;
    esac
}

# removes the marked line from a file, if it is there
strip_line() {
    [ -f "$1" ] || return 0
    grep -qF "$MARK" "$1" || return 0
    tmp="$1.nyra-tmp"
    grep -vF "$MARK" "$1" > "$tmp" || true
    cat "$tmp" > "$1"
    rm -f "$tmp"
    say "Removed the PATH line from $1"
}

# --- uninstall ---------------------------------------------------------------------------------

if [ "$ACTION" = uninstall ]; then
    found=0
    for f in "$BIN/nyra.exe" "$BIN/nyra"; do
        if [ -f "$f" ]; then rm -f "$f"; say "Removed $f"; found=1; fi
    done
    [ "$found" = 1 ] || say "Nyra is not installed in $BIN"
    rmdir "$BIN" 2>/dev/null || true
    rmdir "$NYRA_HOME" 2>/dev/null || true
    for f in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.bash_profile" "${ZDOTDIR:-$HOME}/.zshrc"; do
        strip_line "$f"
    done
    fish="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/nyra.fish"
    if [ -f "$fish" ] && grep -qF "$MARK" "$fish"; then rm -f "$fish"; say "Removed $fish"; fi
    say "Nyra is uninstalled. Open a new terminal so PATH no longer lists $BIN."
    exit 0
fi

# --- which archive -------------------------------------------------------------------------------

# (NYRA_TEST_OS and NYRA_TEST_ARCH pretend to be another platform, to test these messages)
os="${NYRA_TEST_OS:-$(uname -s)}"
arch="${NYRA_TEST_ARCH:-$(uname -m)}"
# a shell running under Rosetta on Apple Silicon reports x86_64: the arm64 build is the right one
if [ "$os" = Darwin ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
    arch=arm64
fi
FROM_SOURCE="build it from source instead: install Rust (https://rustup.rs), then run
  cargo install --git https://github.com/$REPO
(Nyra has no dependencies, so this takes about a minute)."
EXE=nyra
case "$os/$arch" in
    Linux/x86_64 | Linux/amd64) target=x86_64-unknown-linux-gnu; ext=tar.gz ;;
    Darwin/arm64 | Darwin/aarch64) target=aarch64-apple-darwin; ext=tar.gz ;;
    MINGW*/x86_64 | MSYS*/x86_64 | CYGWIN*/x86_64) target=x86_64-pc-windows-msvc; ext=zip; EXE=nyra.exe ;;
    Darwin/x86_64) err "there is no release build for Intel Macs yet; $FROM_SOURCE" ;;
    Linux/aarch64 | Linux/arm64) err "there is no release build for ARM Linux yet; $FROM_SOURCE" ;;
    Linux/*) err "there is no release build for Linux on $arch; $FROM_SOURCE" ;;
    MINGW* | MSYS* | CYGWIN*) err "there is only an x86_64 build for Windows (this is $arch); $FROM_SOURCE" ;;
    *) err "there is no release build for $os on $arch; $FROM_SOURCE" ;;
esac

# --- downloading ---------------------------------------------------------------------------------

if command -v curl >/dev/null 2>&1; then
    fetch() { curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q -O "$2" "$1"; }
else
    err "this installer needs curl or wget to download Nyra; install one of them and run it again"
fi

TMP=$(mktemp -d 2>/dev/null || mktemp -d -t nyra)
trap 'rm -rf "$TMP"' EXIT
trap 'exit 1' INT TERM

if [ -z "$VERSION" ]; then
    fetch "https://api.github.com/repos/$REPO/releases/latest" "$TMP/latest.json" ||
        err "cannot ask GitHub for the latest release (offline, or the API rate limit?); pass --version X to skip this"
    VERSION=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TMP/latest.json" | head -n 1)
    [ -n "$VERSION" ] || err "GitHub's answer has no tag_name; pass --version X"
fi
case "$VERSION" in v*) tag="$VERSION" ;; *) tag="v$VERSION" ;; esac

asset="nyra-$tag-$target.$ext"
base="https://github.com/$REPO/releases/download/$tag"
say "Installing Nyra $tag ($target) into $BIN"
fetch "$base/$asset" "$TMP/$asset" || err "cannot download $base/$asset (is $tag a release of $REPO?)"
fetch "$base/SHA256SUMS.txt" "$TMP/SHA256SUMS.txt" || err "cannot download $base/SHA256SUMS.txt"

# --- the checksum --------------------------------------------------------------------------------

expected=$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1; exit }' "$TMP/SHA256SUMS.txt")
[ -n "$expected" ] || err "SHA256SUMS.txt of $tag has no line for $asset"
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$TMP/$asset" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$TMP/$asset" | awk '{ print $1 }')
else
    err "found neither sha256sum nor shasum to check the download; install coreutils or perl and run it again"
fi
expected=$(printf '%s' "$expected" | tr 'A-F' 'a-f')
[ "$actual" = "$expected" ] || err "the SHA-256 of $asset is $actual, but SHA256SUMS.txt says $expected; nothing was installed"
say "Checked the SHA-256 of $asset"

# --- unpack and install --------------------------------------------------------------------------

mkdir -p "$TMP/x"
if [ "$ext" = zip ]; then
    if command -v unzip >/dev/null 2>&1; then unzip -q "$TMP/$asset" -d "$TMP/x"; else tar -xf "$TMP/$asset" -C "$TMP/x"; fi
else
    tar -xzf "$TMP/$asset" -C "$TMP/x"
fi
src="$TMP/x/nyra-$tag-$target/$EXE"
[ -f "$src" ] || err "the archive has no nyra-$tag-$target/$EXE"

mkdir -p "$BIN"
# copy next to the old executable, then rename over it: a running nyra keeps working
cp "$src" "$BIN/.$EXE.new"
chmod 755 "$BIN/.$EXE.new"
mv -f "$BIN/.$EXE.new" "$BIN/$EXE"
say "Installed $BIN/$EXE"

version=$("$BIN/$EXE" --version 2>&1 || true)
case "$version" in
    "nyra "*) say "$version" ;;
    *)
        say "warning: $BIN/$EXE did not start: $version"
        say "         (an old C library? then $FROM_SOURCE)" ;;
esac

# --- PATH ----------------------------------------------------------------------------------------

on_path=0
case ":${PATH:-}:" in *":$BIN:"*) on_path=1 ;; esac
if [ "$MODIFY_PATH" = 1 ]; then
    rc=$(rc_file)
    if [ -f "$rc" ] && grep -qF "$MARK" "$rc"; then
        say "PATH: $rc already adds $BIN"
    elif [ "$on_path" = 1 ]; then
        say "PATH: $BIN is already on PATH"
    else
        mkdir -p "$(dirname "$rc")"
        { [ ! -s "$rc" ] || [ -z "$(tail -c 1 "$rc")" ] || printf '\n'; path_line "$rc"; } >> "$rc"
        say "PATH: added $BIN to $rc"
    fi
    [ "$on_path" = 1 ] || say "      open a new terminal, or run:  export PATH=\"$BIN:\$PATH\""
elif [ "$on_path" = 0 ]; then
    say "PATH: not changed (--no-path). Add this to your shell's startup file:"
    say "      export PATH=\"$BIN:\$PATH\""
fi

# --- a C compiler --------------------------------------------------------------------------------

cc_found=""
for c in ${NYRA_CC:-} gcc clang cc tcc; do
    if command -v "$c" >/dev/null 2>&1; then cc_found="$c"; break; fi
done
if [ -n "$cc_found" ]; then
    say "C compiler: $cc_found (for native programs: nyra build, nyra run --native)"
else
    say "C compiler: none found. The interpreter (nyra run) and the JavaScript target (--js) work"
    say "            without one; native programs (nyra build) need one:"
    case "$os" in
        Darwin) say "              xcode-select --install" ;;
        Linux)
            say "              Debian/Ubuntu: sudo apt install gcc"
            say "              Fedora:        sudo dnf install gcc"
            say "              Arch:          sudo pacman -S gcc"
            say "              Alpine:        sudo apk add gcc musl-dev" ;;
        *) say "              in MSYS2: pacman -S mingw-w64-ucrt-x86_64-gcc" ;;
    esac
fi

say ""
say "Start a project:  nyra new script hello && cd hello && nyra run main.nyra"
say "Docs:             https://nyralang.dev  (upgrade later with: nyra upgrade)"
