"""Finding the `nyra` executable, and `install()`: the release binary from GitHub, checked by SHA-256.

Order: an explicit path, `NYRA_PATH` (the executable or the folder that holds it), `nyra` on PATH,
then a binary that `install()` put into the user cache earlier. Nothing is downloaded unless
`install()` is called.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import platform
import shutil
import stat
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Optional

REPO = "SpAkXD/nyra"
DOWNLOAD = f"https://github.com/{REPO}/releases/download"
LATEST_API = f"https://api.github.com/repos/{REPO}/releases/latest"
EXE = "nyra.exe" if os.name == "nt" else "nyra"


class NyraNotFound(RuntimeError):
    """No `nyra` executable was found."""


class InstallError(RuntimeError):
    """`install()` failed: no build for this computer, a download failed, or the checksum did not match."""


def cache_dir() -> Path:
    """Where `install()` keeps binaries: `NYRA_CACHE_DIR`, else the platform's user cache folder."""
    env = os.environ.get("NYRA_CACHE_DIR")
    if env:
        return Path(env)
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(base) / "nyra" / "Cache"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Caches" / "nyra"
    return Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "nyra"


def _executable(p: Path) -> Optional[Path]:
    if p.is_dir():
        p = p / EXE
    return p if p.is_file() else None


def _cached() -> Optional[Path]:
    root = cache_dir()
    if not root.is_dir():
        return None
    found = sorted((d for d in root.iterdir() if (d / EXE).is_file()), key=lambda d: _version_key(d.name), reverse=True)
    return found[0] / EXE if found else None


def _version_key(tag: str):
    parts = tag.lstrip("v").split(".")
    return tuple(int(p) if p.isdigit() else -1 for p in parts)


def find_binary(path: Optional[str] = None) -> str:
    """The path of the `nyra` executable, or `NyraNotFound` with what to do."""
    if path:
        found = _executable(Path(path))
        if not found:
            raise NyraNotFound(f"no nyra executable at {path}")
        return str(found)
    env = os.environ.get("NYRA_PATH")
    if env:
        found = _executable(Path(env))
        if not found:
            raise NyraNotFound(f"NYRA_PATH={env} is not the nyra executable or a folder that holds it")
        return str(found)
    on_path = shutil.which("nyra")
    if on_path:
        return on_path
    cached = _cached()
    if cached:
        return str(cached)
    raise NyraNotFound(
        "nyra is not installed: put it on PATH (https://nyralang.dev/install), set NYRA_PATH, "
        "or call nyra.install() to download the release binary into the user cache"
    )


def release_target() -> tuple:
    """(target triple, archive extension) of the release build for this computer."""
    system, machine = platform.system(), platform.machine().lower()
    if system == "Windows" and machine in ("amd64", "x86_64"):
        return "x86_64-pc-windows-msvc", "zip"
    if system == "Linux" and machine in ("x86_64", "amd64"):
        return "x86_64-unknown-linux-gnu", "tar.gz"
    if system == "Darwin" and machine in ("arm64", "aarch64"):
        return "aarch64-apple-darwin", "tar.gz"
    raise InstallError(
        f"there is no release build of nyra for {system} {machine}; build it from source: "
        f"https://github.com/{REPO}#build-from-source"
    )


def _fetch(url: str, timeout: float) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "nyra-python-sdk"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:  # noqa: S310 (https or a file:// mirror)
            return r.read()
    except Exception as e:  # urllib raises several kinds
        raise InstallError(f"cannot download {url}: {e}") from e


def latest_version(timeout: float = 30) -> str:
    """The tag of the newest release on GitHub, e.g. `v0.9.0`."""
    data = json.loads(_fetch(LATEST_API, timeout))
    tag = data.get("tag_name")
    if not tag:
        raise InstallError("GitHub did not name the latest release")
    return tag


def install(version: Optional[str] = None, *, force: bool = False, base_url: str = DOWNLOAD, timeout: float = 120) -> str:
    """Download the release binary for this computer into the user cache and return its path.

    `version` is a tag (`v0.9.0` or `0.9.0`); without it the latest release. The archive's SHA-256
    must match the release's `SHA256SUMS.txt`, or nothing is installed. `base_url` serves the
    releases as `<base_url>/<tag>/<asset>` (a mirror, or `file://` in tests).
    """
    if version is None:
        if base_url != DOWNLOAD:
            raise InstallError("pass a version when base_url is not GitHub")
        version = latest_version(timeout)
    tag = version if version.startswith("v") else f"v{version}"
    dest = cache_dir() / tag / EXE
    if dest.is_file() and not force:
        return str(dest)

    triple, ext = release_target()
    asset = f"nyra-{tag}-{triple}.{ext}"
    sums = _fetch(f"{base_url}/{tag}/SHA256SUMS.txt", timeout).decode("utf-8", "replace")
    want = None
    for line in sums.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].lstrip("*") == asset:
            want = parts[0].lower()
    if not want:
        raise InstallError(f"SHA256SUMS.txt of {tag} does not list {asset}")
    data = _fetch(f"{base_url}/{tag}/{asset}", timeout)
    got = hashlib.sha256(data).hexdigest()
    if got != want:
        raise InstallError(f"checksum mismatch for {asset}: expected {want}, got {got}; nothing was installed")

    exe = _extract(data, ext, f"nyra-{tag}-{triple}/{EXE}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=dest.parent, prefix=".nyra-")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(exe)
        os.chmod(tmp, os.stat(tmp).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        os.replace(tmp, dest)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)
    return str(dest)


def _extract(data: bytes, ext: str, member: str) -> bytes:
    """Only the executable is read from the archive (no paths are written from it)."""
    try:
        if ext == "zip":
            with zipfile.ZipFile(io.BytesIO(data)) as z:
                return z.read(member)
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as t:
            f = t.extractfile(member)
            if f is None:
                raise KeyError(member)
            return f.read()
    except KeyError as e:
        raise InstallError(f"the archive has no {member}") from e
