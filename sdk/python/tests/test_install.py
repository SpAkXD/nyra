"""`install()` against a release laid out like GitHub's, served from a `file://` folder."""

import hashlib
import io
import os
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

import _setup  # noqa: F401

import nyra
from nyra import _binary

TAG = "v9.9.9"


def make_release(root: Path, payload: bytes, corrupt: bool = False) -> str:
    triple, ext = _binary.release_target()
    name = f"nyra-{TAG}-{triple}"
    buf = io.BytesIO()
    if ext == "zip":
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr(f"{name}/{_binary.EXE}", payload)
            z.writestr(f"{name}/README.md", "readme")
    else:
        with tarfile.open(fileobj=buf, mode="w:gz") as t:
            for member, data in ((f"{name}/{_binary.EXE}", payload), (f"{name}/README.md", b"readme")):
                info = tarfile.TarInfo(member)
                info.size = len(data)
                t.addfile(info, io.BytesIO(data))
    data = buf.getvalue()
    d = root / TAG
    d.mkdir(parents=True)
    (d / f"{name}.{ext}").write_bytes(data)
    digest = hashlib.sha256(data + (b"x" if corrupt else b"")).hexdigest()
    (d / "SHA256SUMS.txt").write_text(f"{'0' * 64}  nyra-{TAG}-other.tar.gz\n{digest}  {name}.{ext}\n")
    return root.resolve().as_uri()


class InstallTest(unittest.TestCase):
    def setUp(self):
        try:
            _binary.release_target()
        except nyra.InstallError:
            self.skipTest("no release build for this computer")
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.env = mock.patch.dict(os.environ, {"NYRA_CACHE_DIR": str(self.root / "cache")})
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def test_installs_the_checked_binary(self):
        base = make_release(self.root / "rel", b"#!fake nyra")
        path = nyra.install("9.9.9", base_url=base)
        self.assertEqual(Path(path), self.root / "cache" / TAG / _binary.EXE)
        self.assertEqual(Path(path).read_bytes(), b"#!fake nyra")
        if os.name != "nt":
            self.assertTrue(os.access(path, os.X_OK))
        # cached: a second call does not download (the release may even be gone)
        self.assertEqual(nyra.install(TAG, base_url=base + "/missing"), path)
        # found when nothing else is
        with mock.patch.dict(os.environ, {"NYRA_PATH": ""}), mock.patch("shutil.which", return_value=None):
            self.assertEqual(Path(nyra.find_binary()), Path(path))

    def test_checksum_mismatch_installs_nothing(self):
        base = make_release(self.root / "rel", b"tampered", corrupt=True)
        with self.assertRaises(nyra.InstallError) as ctx:
            nyra.install(TAG, base_url=base)
        self.assertIn("checksum", str(ctx.exception))
        self.assertFalse((self.root / "cache" / TAG / _binary.EXE).exists())

    def test_missing_release(self):
        with self.assertRaises(nyra.InstallError):
            nyra.install(TAG, base_url=(self.root / "nothing").resolve().as_uri())

    def test_latest_needs_github(self):
        with self.assertRaises(nyra.InstallError):
            nyra.install(base_url="file:///mirror")

    def test_not_found_message(self):
        with mock.patch.dict(os.environ, {"NYRA_PATH": ""}), mock.patch("shutil.which", return_value=None):
            with self.assertRaises(nyra.NyraNotFound) as ctx:
                nyra.find_binary()
        self.assertIn("nyra.install()", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
