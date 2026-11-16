import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("local_services", ROOT / "tools/local_services.py")
services = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(services)


class InputPreparationTest(unittest.TestCase):
    def test_existing_corrupt_download_is_rejected_without_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "model.bin"
            path.write_bytes(b"synthetic-corrupted-model")
            original = path.read_bytes()
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                services.fetch(path, {
                    "bytes": len(original), "sha256": hashlib.sha256(b"expected").hexdigest(),
                    "url": "https://invalid.example.test/not-contacted",
                })
            self.assertEqual(path.read_bytes(), original)

    def test_verified_tree_preserves_and_rejects_local_source_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "source.tar.gz"
            with tarfile.open(archive, "w:gz") as target:
                member = tarfile.TarInfo("pinned-source/source.c")
                member.size = 9
                target.addfile(member, io.BytesIO(b"original\n"))
            destination = root / "source"
            services.extract(archive, destination, {"root": "pinned-source"})
            source = destination / "source.c"
            source.write_bytes(b"local edit\n")
            with self.assertRaisesRegex(ValueError, "Changed source input"):
                services.extract(archive, destination, {"root": "pinned-source"})
            self.assertEqual(source.read_bytes(), b"local edit\n")

    def test_traversing_archive_never_creates_escaped_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "source.tar.gz"
            with tarfile.open(archive, "w:gz") as target:
                member = tarfile.TarInfo("pinned-source/../../escape")
                member.size = 4
                target.addfile(member, io.BytesIO(b"data"))
            with self.assertRaisesRegex(ValueError, "Unsafe input archive path"):
                services.extract(archive, root / "source", {"root": "pinned-source"})
            self.assertFalse((root / "escape").exists())
            self.assertFalse((root / "source").exists())


if __name__ == "__main__":
    unittest.main()
