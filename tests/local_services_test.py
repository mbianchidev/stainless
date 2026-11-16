import hashlib
import importlib.util
import io
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("local_services", ROOT / "tools/local_services.py")
services = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(services)
sys.modules["local_services"] = services
OCR_SPEC = importlib.util.spec_from_file_location("local_ocr_check", ROOT / "tools/local_ocr_check.py")
ocr = importlib.util.module_from_spec(OCR_SPEC)
OCR_SPEC.loader.exec_module(ocr)


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


class RuntimeBoundaryTest(unittest.TestCase):
    def test_fast_process_reports_real_peak_and_cannot_bypass_memory_limit(self):
        command = [sys.executable, "-c", "pass"]
        limits = {"maximum_seconds_per_case": 5, "maximum_rss_bytes": 512 * 1024 * 1024}
        _, rss, stdout, stderr = services.run_bounded(command, os.environ.copy(), limits)
        self.assertGreater(rss, 0)
        self.assertEqual((stdout, stderr), ("", ""))
        with self.assertRaisesRegex(MemoryError, "RSS limit"):
            services.run_bounded(
                command, os.environ.copy(), dict(limits, maximum_rss_bytes=1)
            )

    def test_owned_runtime_deadline_is_not_an_empty_success(self):
        with self.assertRaisesRegex(TimeoutError, "deadline"):
            services.run_bounded(
                [sys.executable, "-c", "import time; time.sleep(10)"],
                os.environ.copy(),
                {"maximum_seconds_per_case": 0.05, "maximum_rss_bytes": 512 * 1024 * 1024},
            )

    def test_ocr_preserves_real_unicode_words_and_geometry(self):
        header = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n"
        row = "5\t1\t1\t1\t1\t1\t10\t20\t40\t12\t99.5\tboîtes\n"
        parsed = ocr.parse_tsv(header + row, 100, 100)
        self.assertEqual(parsed, [{
            "text": "boîtes", "confidence": 99.5,
            "box": {"left": 10, "top": 20, "width": 40, "height": 12},
        }])
        self.assertEqual(ocr.words("Boîtes, bleues."), ["boîtes", "bleues"])

    def test_bad_ocr_shape_confidence_and_boxes_fail_explicitly(self):
        header = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n"
        rows = [
            "",
            header + "5\t1\t1\t1\t1\t1\t97\t20\t40\t12\t99\trobot\n",
            header + "5\t1\t1\t1\t1\t1\t10\t20\t40\t12\tnan\trobot\n",
            header + "5\t1\t1\t1\t1\t1\t10\t20\t40\t12\t99\n",
        ]
        for value in rows:
            with self.subTest(value=value), self.assertRaises(ValueError):
                ocr.parse_tsv(value, 100, 100)


if __name__ == "__main__":
    unittest.main()
