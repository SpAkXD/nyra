#!/bin/sh
# Fills the package manifest templates of this folder for one release, from its SHA256SUMS.txt.
#
#   sh install/packaging/fill.sh v0.9.0 dist/SHA256SUMS.txt dist
#
# writes into the output folder (flat, so they can be release assets):
#   nyra.rb                                   Homebrew formula (for a tap: Formula/nyra.rb)
#   winget-SpAkXD.Nyra.yaml                   winget manifests (for winget-pkgs:
#   winget-SpAkXD.Nyra.installer.yaml           manifests/s/SpAkXD/Nyra/<version>/, without the
#   winget-SpAkXD.Nyra.locale.en-US.yaml        "winget-" prefix)
# Nothing is submitted anywhere. Fails when a checksum is missing or a placeholder is left.

set -eu

[ $# -eq 3 ] || { echo "usage: sh fill.sh <tag> <SHA256SUMS.txt> <out-dir>" >&2; exit 2; }
tag="$1"
sums="$2"
out="$3"
here=$(cd "$(dirname "$0")" && pwd)
version="${tag#v}"

sum_of() {
    s=$(awk -v f="$1" '$2 == f || $2 == "*" f { print $1; exit }' "$sums")
    [ -n "$s" ] || { echo "fill.sh: $sums has no line for $1" >&2; exit 1; }
    printf '%s' "$s"
}

win=$(sum_of "nyra-$tag-x86_64-pc-windows-msvc.zip")
mac=$(sum_of "nyra-$tag-aarch64-apple-darwin.tar.gz")
linux=$(sum_of "nyra-$tag-x86_64-unknown-linux-gnu.tar.gz")
date=$(date -u +%Y-%m-%d)

fill() {
    sed -e "s/{{VERSION}}/$version/g" \
        -e "s/{{SHA256_WINDOWS_X64}}/$win/g" \
        -e "s/{{SHA256_MACOS_ARM64}}/$mac/g" \
        -e "s/{{SHA256_LINUX_X64}}/$linux/g" \
        -e "s/{{RELEASE_DATE}}/$date/g" \
        "$1" > "$2"
    if grep -q '{{' "$2"; then
        echo "fill.sh: $2 still has a placeholder: $(grep '{{' "$2" | head -n 1)" >&2
        exit 1
    fi
    echo "wrote $2"
}

mkdir -p "$out"
fill "$here/homebrew/nyra.rb" "$out/nyra.rb"
for f in SpAkXD.Nyra.yaml SpAkXD.Nyra.installer.yaml SpAkXD.Nyra.locale.en-US.yaml; do
    fill "$here/winget/$f" "$out/winget-$f"
done
