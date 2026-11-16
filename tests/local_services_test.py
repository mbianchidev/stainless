import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[1]


def load_tool(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "tools" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


services = load_tool("local_services")
ocr = load_tool("local_ocr_check")
translation_prepare = load_tool("local_translation_prepare")
translation = load_tool("local_translation_check")


class InputPreparationTest(unittest.TestCase):
    def model_archive(self, root, name="model.npz", mode=0o100644):
        archive = root / "model.zip"
        data = b"synthetic-model-data"
        item = zipfile.ZipInfo(name)
        item.external_attr = mode << 16
        with zipfile.ZipFile(archive, "w") as target:
            target.writestr(item, data)
            target.writestr("publisher.log", b"ignored synthetic log")
        entry = {
            "members": {name: {
                "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
            }},
            "ignored_source_members": ["publisher.log"],
        }
        return archive, entry

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

    def test_composed_runtime_preserves_nested_submodule_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sources = {}
            entries = {}
            for name in ("ctranslate2", "spdlog", "cxxopts", "ruy", "cpuinfo"):
                path = root / name
                path.mkdir()
                (path / "source.cc").write_text("synthetic original " + name)
                sources[name] = str(path)
                entries[name] = {"revision": "synthetic-" + name}
            cache = root / "cache"
            (cache / "sources").mkdir(parents=True)
            destination, _ = services.translation_sources(
                {"sources": entries}, sources, cache
            )
            edited = destination / "third_party/ruy/third_party/cpuinfo/source.cc"
            edited.write_text("synthetic local edit")
            with self.assertRaisesRegex(ValueError, "Changed composed source"):
                services.translation_sources({"sources": entries}, sources, cache)
            self.assertEqual(edited.read_text(), "synthetic local edit")

    def test_original_model_zip_extracts_only_data_and_preserves_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, entry = self.model_archive(root)
            destination = root / "source"
            services.extract_model_zip(archive, destination, entry)
            services.extract_model_zip(archive, destination, entry)
            self.assertEqual({p.name for p in destination.iterdir()}, {"model.npz"})
            model = destination / "model.npz"
            model.write_bytes(b"synthetic local edit")
            with self.assertRaisesRegex(ValueError, "mismatch"):
                services.extract_model_zip(archive, destination, entry)
            self.assertEqual(model.read_bytes(), b"synthetic local edit")

    def test_original_model_zip_rejects_traversal_links_and_executable_data(self):
        for name, mode in (
            ("../escape", 0o100644), ("model.npz", 0o120777),
            ("model.npz", 0o100755),
        ):
            with self.subTest(name=name, mode=mode), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                archive, entry = self.model_archive(root, name, mode)
                with self.assertRaises(ValueError):
                    services.extract_model_zip(archive, root / "source", entry)
                self.assertFalse((root / "source").exists())
                self.assertFalse((root / "escape").exists())

    def test_original_model_zip_rejects_unreviewed_members_and_damaged_data(self):
        for extra in (False, True):
            with self.subTest(extra=extra), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                archive, entry = self.model_archive(root)
                if extra:
                    with zipfile.ZipFile(archive, "a") as target:
                        target.writestr("unexpected.bin", b"synthetic unknown")
                else:
                    entry["members"]["model.npz"]["sha256"] = "0" * 64
                with self.assertRaises(ValueError):
                    services.extract_model_zip(archive, root / "source", entry)
                self.assertFalse((root / "source").exists())


class RuntimeBoundaryTest(unittest.TestCase):
    def test_translation_requires_every_frozen_concept_and_whole_unicode_words(self):
        concepts = json.loads(translation.CORPUS.read_text())["destination_concepts"]
        self.assertTrue(translation.assess(
            "The RED robot holds three blue boxes.", concepts["en"]
        )["passed"])
        self.assertFalse(translation.assess(
            "A hundred robots hold three blue boxes.", concepts["en"]
        )["passed"])
        self.assertTrue(translation.assess(
            "Le robot rouge porte trois boi\u0302tes bleues.", concepts["fr"]
        )["passed"])

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


class ConvertedModelTest(unittest.TestCase):
    def model(self, directory):
        files = {
            "model.bin": b"synthetic int8 data", "config.json": b"{}",
            "shared_vocabulary.json": b'["synthetic"]',
            "source.spm": b"synthetic source tokenizer",
            "target.spm": b"synthetic target tokenizer",
            "ORIGINAL_MODEL_LICENSE.txt": b"synthetic license notice",
            "ORIGINAL_MODEL_README.md": b"synthetic model card",
            "NOTICES.txt": b"synthetic conversion notice",
        }
        records = {}
        for name, data in files.items():
            path = directory / name
            path.write_bytes(data)
            records[name] = {"bytes": len(data), "sha256": services.digest(path)}
        provenance = {"recipe_sha256": "synthetic-recipe", "files": records}
        (directory / "provenance.json").write_text(json.dumps(provenance))
        return provenance

    def test_edited_converted_weights_are_rejected_without_replacement(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            self.model(directory)
            translation_prepare.verify_output(directory, "synthetic-recipe")
            model = directory / "model.bin"
            model.write_bytes(b"synthetic local edit")
            with self.assertRaisesRegex(ValueError, "mismatch"):
                translation_prepare.verify_output(directory, "synthetic-recipe")
            self.assertEqual(model.read_bytes(), b"synthetic local edit")

    def test_converted_model_rejects_unreviewed_paths_and_linked_provenance(self):
        for kind in ("traversal", "extra-file", "linked-record"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                directory = root / "model"
                directory.mkdir()
                provenance = self.model(directory)
                record = directory / "provenance.json"
                if kind == "traversal":
                    provenance["files"]["../escape"] = {"bytes": 0, "sha256": "0" * 64}
                    record.write_text(json.dumps(provenance))
                elif kind == "extra-file":
                    (directory / "unreviewed.bin").write_bytes(b"synthetic unknown")
                else:
                    original = root / "external-record.json"
                    record.rename(original)
                    record.symlink_to(original)
                with self.assertRaises(ValueError):
                    translation_prepare.verify_output(directory, "synthetic-recipe")


if __name__ == "__main__":
    unittest.main()
