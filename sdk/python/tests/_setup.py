"""Shared by the tests: the package from src/ and the compiler from the repository's target/release."""

import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "src"))
REPO = HERE.parents[2]
EXE = REPO / "target" / "release" / ("nyra.exe" if os.name == "nt" else "nyra")

if not os.environ.get("NYRA_PATH") and EXE.is_file():
    os.environ["NYRA_PATH"] = str(EXE)

HAVE_BINARY = bool(os.environ.get("NYRA_PATH")) and Path(os.environ["NYRA_PATH"]).exists()
SKIP_REASON = "no nyra binary: run `CARGO_INCREMENTAL=0 cargo build --release` or set NYRA_PATH"
