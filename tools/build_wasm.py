"""Build the compiler as WebAssembly for the playground (nyralang.dev/play) and copy it into the site.

    python tools/build_wasm.py                     # build, then copy into the site checkout it finds
    python tools/build_wasm.py --site PATH         # the folder of the `site` branch (holds site/play/)
    python tools/build_wasm.py --no-copy           # only build

No crates and no wasm-bindgen: the library target (src/lib.rs) is the command line's source with the
C-ABI exports of src/wasm.rs. It is built with the `wasm` profile of Cargo.toml (opt-level "s", LTO,
stripped), which leaves native builds alone. Needs the target once: `rustup target add wasm32-unknown-unknown`.
"""
import argparse
import gzip
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = "wasm32-unknown-unknown"
BUILT = ROOT / "target" / TARGET / "wasm" / "nyra.wasm"
# The interpreter recurses once per call of the program; most of that is on the engine's stack,
# what is left (addresses of locals) on this one, in linear memory.
STACK = 8 << 20


def find_site() -> Path | None:
    """The checkout of the `site` branch: a worktree next to this one, or a sibling folder."""
    for name in ("site-play", "site"):
        for base in (ROOT.parent, ROOT / ".claude" / "worktrees"):
            d = base / name
            if (d / "site" / "index.html").exists():
                return d
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--site", type=Path, help="checkout of the site branch (default: found next to this one)")
    ap.add_argument("--no-copy", action="store_true", help="build only")
    args = ap.parse_args()

    cmd = [
        "cargo", "rustc", "--lib", "--profile", "wasm", "--target", TARGET, "--crate-type", "cdylib",
        "--", "-C", f"link-arg=-zstack-size={STACK}",
    ]
    print("$", " ".join(cmd), flush=True)
    if subprocess.run(cmd, cwd=ROOT).returncode != 0:
        print("build_wasm: cargo failed (is the target installed? rustup target add wasm32-unknown-unknown)", file=sys.stderr)
        return 1
    data = BUILT.read_bytes()
    print(f"build_wasm: {BUILT.relative_to(ROOT)}: {len(data):,} bytes ({len(gzip.compress(data, 9)):,} gzipped)")

    if args.no_copy:
        return 0
    site = args.site or find_site()
    if site is None:
        print("build_wasm: no site checkout found; pass --site PATH (the folder of the `site` branch)", file=sys.stderr)
        return 1
    dest = site / "site" / "play" / "nyra.wasm"
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(BUILT, dest)
    print(f"build_wasm: copied to {dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
