import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import struct
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile


ROOT = Path(__file__).resolve().parents[1]


def load_tool(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "tools" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


advisories = load_tool("local_services_advisories")
services = load_tool("local_services")
ocr = load_tool("local_ocr_check")
translation_prepare = load_tool("local_translation_prepare")
translation = load_tool("local_translation_check")
vision = load_tool("local_vision_check")
push_sender = load_tool("push_browser_sender")
speech = load_tool("local_speech_check")
runtime_fixture = load_tool("local_runtime_fixture")
model_packs = load_tool("local_model_packs")


class InputPreparationTest(unittest.TestCase):
    def test_model_packs_are_byte_deterministic_data_only_archives(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            model = root / "synthetic.bin"
            model.write_bytes(b"Fictional immutable model bytes")
            first = model_packs.write_pack(root / "first.zip", {
                "model.bin": model, "NOTICES.txt": b"Fictional permissive notice",
            })
            os.utime(model, (1, 1))
            second = model_packs.write_pack(root / "second.zip", {
                "NOTICES.txt": b"Fictional permissive notice", "model.bin": model,
            })
            self.assertEqual(first["archive_sha256"], second["archive_sha256"])
            self.assertEqual(first["members"], second["members"])
            with zipfile.ZipFile(root / "first.zip") as archive:
                self.assertEqual(archive.namelist(), ["NOTICES.txt", "model.bin"])
                self.assertEqual(archive.read("model.bin"), model.read_bytes())
                for member in archive.infolist():
                    self.assertEqual(member.external_attr >> 16, 0o100400)
                    self.assertEqual(member.date_time, (1980, 1, 1, 0, 0, 0))

    def test_model_pack_rejects_paths_code_and_linked_or_executable_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ("../escape.bin", "nested/model.bin", "runtime.dylib", "script.py"):
                with self.subTest(name=name), self.assertRaises(ValueError):
                    model_packs.write_pack(root / "bad.zip", {name: b"Fictional data"})
            source = root / "source.bin"
            source.write_bytes(b"Fictional model")
            linked = root / "linked.bin"
            linked.symlink_to(source)
            with self.assertRaises(ValueError):
                model_packs.write_pack(root / "linked.zip", {"model.bin": linked})
            source.chmod(0o700)
            with self.assertRaises(ValueError):
                model_packs.write_pack(root / "executable.zip", {"model.bin": source})

    def test_build_host_advisory_authorization_is_not_forwarded_to_other_hosts(self):
        with mock.patch.object(advisories, "github_cli", return_value="/fictional/gh"):
            with mock.patch.object(
                advisories.subprocess, "check_output", return_value=b"{}"
            ) as request:
                advisories.query("https://api.github.com/repos/example/runtime/security-advisories")
                self.assertEqual(request.call_args.args[0][0], "/fictional/gh")
                for url, body in (
                    ("https://api.osv.dev/v1/query", {"commit": "1" * 40}),
                    ("https://api.github.com.foreign.example.test/feed", None),
                ):
                    advisories.query(url, body)
                    self.assertEqual(request.call_args.args[0][0], "/usr/bin/curl")

    def test_vendored_header_without_standalone_git_still_requires_live_advisories(self):
        manifest = {
            "sources": {"parent": {
                "version": "1.0.0", "revision": "1" * 40,
                "url": "https://github.com/example/parent/",
            }},
            "embedded_sources": {"header": {
                "version": "2.30", "parent": "parent", "sha256": "2" * 64,
                "repository": "https://github.com/example/header/",
                "advisory_coverage": "Fictional parent-locked header; standalone Git unknown",
            }},
        }
        advisory = {
            "ghsa_id": "GHSA-fictional-header", "description": "Fictional header issue",
            "vulnerabilities": [{
                "vulnerable_version_range": "<= 2.30",
                "first_patched_version": "2.31",
            }],
        }
        for outcome in ("clear", "affected", "unavailable"):
            def response(url, body=None):
                if body is not None:
                    return {"vulns": []}
                if outcome == "unavailable":
                    raise OSError("Fictional upstream evidence unavailable")
                return [advisory] if outcome == "affected" else []

            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as temporary:
                with mock.patch.object(advisories, "query", side_effect=response) as queried:
                    if outcome == "clear":
                        evidence = advisories.check_source_advisories(
                            manifest, Path(temporary), {"header"}
                        )
                        record = evidence["inputs"]["header"]
                        self.assertNotIn("response", record)
                        self.assertIn("unknown", record["osv_standalone_commit"])
                        self.assertEqual(record["upstream"]["dispositions"], [])
                    else:
                        with self.assertRaises((ValueError, OSError)):
                            advisories.check_source_advisories(
                                manifest, Path(temporary), {"header"}
                            )
                    self.assertTrue(any(
                        "/repos/example/header/security-advisories" in call.args[0]
                        for call in queried.call_args_list
                    ))

    def test_native_non_speech_fixtures_are_deterministic_and_bounded(self):
        specification = json.loads(speech.CORPUS.read_text())["negative_cases"]
        first = runtime_fixture.negative_speech_pcm(specification)
        self.assertEqual(first, runtime_fixture.negative_speech_pcm(specification))
        self.assertEqual(set(first), {
            "silence", "noise", "tones", "silence_after_speech",
        })
        self.assertFalse(any(first["silence"]))
        self.assertFalse(any(first["silence_after_speech"]))
        self.assertTrue(any(first["noise"]))
        self.assertTrue(any(first["tones"]))
        for value in first.values():
            self.assertLessEqual(len(value), 960000)
        with self.assertRaisesRegex(ValueError, "PCM window"):
            runtime_fixture.negative_speech_pcm(dict(specification, seconds=31))

    def test_git_advisory_requires_fixed_ancestor_not_just_a_new_version_label(self):
        fixed = "1" * 40
        proof = {
            "status": "ahead", "base_commit": {"sha": fixed},
            "merge_base_commit": {"sha": fixed},
        }
        self.assertEqual(
            advisories.ancestor_evidence(proof, "2" * 40, "https://api.example.test")[
                "fixed_commit"
            ], fixed,
        )
        for value in (
            dict(proof, status="behind"),
            dict(proof, merge_base_commit={"sha": "3" * 40}),
            {"status": "ahead"}, [],
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                advisories.ancestor_evidence(value, "2" * 40, "https://api.example.test")

    def test_uncompiled_advisory_requires_reviewed_scope_and_actual_options(self):
        text = "Fictional remote parser vulnerability, not local image inference."
        identifier = "GHSA-fictional-rpc"
        entry = {
            "version": "1.0.0",
            "configuration_advisory_exclusions": {identifier: {
                "description_sha256": hashlib.sha256(text.encode()).hexdigest(),
                "required_options": {"GGML_RPC": "OFF"},
                "reason": "Fictional test-only exclusion",
                "source": "https://example.test/fictional-advisory",
            }},
        }
        advisory = {
            "ghsa_id": identifier, "description": text,
            "vulnerabilities": [{"vulnerable_version_range": "<= b42"}],
        }
        item = advisories.disposition(entry, advisory, "fictional/runtime", {})
        evidence = {"inputs": {"runtime": {"upstream": {"dispositions": [item]}}}}
        with self.assertRaises(ValueError):
            advisories.ensure_configurations_verified(evidence)
        with self.assertRaises(ValueError):
            advisories.verify_disabled_advisories(evidence, {"GGML_RPC": "ON"})
        advisories.verify_disabled_advisories(evidence, {"GGML_RPC": "OFF"})
        advisories.ensure_configurations_verified(evidence)
        self.assertEqual(item["verified_disabled_options"], {"GGML_RPC": "OFF"})
        with self.assertRaises(ValueError):
            advisories.disposition(
                entry, dict(advisory, description="Scope changed"), "fictional/runtime", {}
            )

    def test_trial_requires_exact_reviewed_model_reference_bytes(self):
        notice = b"Synthetic permissive model notice"
        manifest = {
            "models": {"fictional-model": {"references": ["fictional-notice"]}},
            "model_reference_inputs": {"fictional-notice": {
                "bytes": len(notice), "sha256": hashlib.sha256(notice).hexdigest(),
            }},
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError):
                services.verify_model_references(manifest, root, ["fictional-model"])
            directory = root / "model-notices"
            directory.mkdir()
            path = directory / "fictional-notice"
            path.write_bytes(notice)
            self.assertIn("fictional-notice", services.verify_model_references(
                manifest, root, ["fictional-model"]
            ))
            path.write_bytes(b"Changed synthetic model notice")
            with self.assertRaises(ValueError):
                services.verify_model_references(manifest, root, ["fictional-model"])

    def test_upstream_advisory_ranges_fail_closed_at_patch_boundaries(self):
        self.assertTrue(services.version_in_range("5.5.3", ">= 4.0.0, <= 5.5.3"))
        self.assertFalse(services.version_in_range("5.5.4", ">= 4.0.0, <= 5.5.3"))
        self.assertFalse(services.version_in_range("5.5.4", "< 5.5.4"))
        self.assertTrue(services.version_in_range("3.11.2", "< 3.0 || = 3.11.2"))
        for version, value in (("5.5.4", None), ("5.5.4", "*"), ("5.5-dev", "< 6.0")):
            with self.subTest(version=version, value=value), self.assertRaises(ValueError):
                services.version_in_range(version, value)

    def test_native_push_sender_never_accepts_foreign_or_credentialed_targets(self):
        self.assertEqual(
            push_sender.origin("https://localhost:9443/wpush/fictional", "https"),
            ("https", "localhost", 9443),
        )
        for value in (
            "https://foreign.example.test:9443/wpush",
            "http://localhost:9443/wpush",
            "https://fictional:password@localhost:9443/wpush",
            "https://localhost:9443/wpush#fragment",
            "https://localhost/wpush",
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                push_sender.origin(value, "https")

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
    def test_neural_voice_pcm_retains_reference_normalization_and_rejects_bad_audio(self):
        quiet = speech.normalized_voice_pcm([-0.2, 0.1, 0.0, 0.2])
        loud = speech.normalized_voice_pcm([-2.0, 1.0, 0.0, 2.0])
        self.assertEqual(quiet, loud)
        self.assertEqual(struct.unpack("<4h", quiet), (-32767, 16383, 0, 32767))
        for audio in ([], [0.0], [1e-10], [float("nan")], [float("inf")]):
            with self.subTest(audio=audio), self.assertRaises(ValueError):
                speech.normalized_voice_pcm(audio)

    @unittest.skipUnless(sys.platform == "darwin", "Native runtime ABI targets macOS")
    def test_native_model_handles_and_c_only_header(self):
        source = ROOT / "tools/local_services/runtime"
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            subprocess.run([
                "/usr/bin/xcrun", "clang", "-x", "c", "-std=c11",
                "-Wall", "-Wextra", "-Werror", "-fsyntax-only",
                str(source / "stainless_local_runtime.h"),
            ], check=True, capture_output=True, text=True)
            binary = directory / "native-input-test"
            subprocess.run([
                "/usr/bin/xcrun", "clang++", "-std=c++17",
                "-Wall", "-Wextra", "-Werror",
                str(ROOT / "tests/local_runtime_input_test.cc"), "-o", str(binary),
            ], check=True, capture_output=True, text=True)
            subprocess.run([str(binary), str(directory)], check=True,
                           capture_output=True, text=True)

    def test_vision_evaluates_only_generated_text_and_rejects_contradictions(self):
        response = vision.parse_response(
            "metadata red circle\n" + vision.MARKER + "\nA blue square.\n"
        )
        cases = json.loads(vision.CORPUS.read_text())["cases"]
        self.assertFalse(vision.assess(response, cases[0])["passed"])
        self.assertTrue(vision.assess(response, cases[1])["passed"])
        self.assertFalse(vision.assess("A red circle and a blue dog.", cases[0])["passed"])
        for text in ("unmarked text", vision.MARKER, vision.MARKER * 2):
            with self.subTest(text=text), self.assertRaises(ValueError):
                vision.parse_response(text)

    def test_vision_fixture_contains_declared_foreground_and_background(self):
        corpus = json.loads(vision.CORPUS.read_text())
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scene.ppm"
            for case in corpus["cases"]:
                vision.raster(case, corpus["raster"], path)
                header, width, maximum, pixels = path.read_bytes().split(b"\n", 3)
                self.assertEqual((header, width, maximum), (b"P6", b"512 512", b"255"))
                self.assertEqual(pixels[:3], b"\xff\xff\xff")
                offset = (256 * 512 + 256) * 3
                self.assertEqual(pixels[offset:offset + 3], bytes(case["color"]))
            with self.assertRaisesRegex(ValueError, "dimensions"):
                vision.raster(
                    corpus["cases"][0],
                    dict(corpus["raster"], width=4097), path,
                )

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
