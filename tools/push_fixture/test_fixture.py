#!/usr/bin/env python3
"""Focused lifecycle/scope regressions; real transport checks are separate."""

import base64
import json
import os
from pathlib import Path
import secrets
import shutil
import unittest
from unittest.mock import patch
import uuid

import fixture


class FixtureSafetyTests(unittest.TestCase):
    def setUp(self):
        self.previous = fixture.WORK
        self.root = self.previous / ("unit-" + uuid.uuid4().hex)
        (self.root / "run" / "secrets").mkdir(parents=True, mode=0o700)
        fixture.WORK = self.root
        self.value = {
            "workspace": str(fixture.HERE.parent.parent),
            "run_id": uuid.uuid4().hex,
            "project": "stainless-push-0123456789ab",
            "connection_port": 47001, "publishing_port": 47002,
            "connection_url": "wss://localhost:47001/",
            "publishing_origin": "https://localhost:47002",
            "certificate": str(self.root / "fictional-cert.pem"),
            "private_key": str(self.root / "fictional-key.pem"),
            "test_certificate": True, "uid": os.getuid(), "gid": os.getgid(),
            "image": "sha256:" + "a" * 64,
            "database_image": "sha256:" + "b" * 64,
            "proxy_image": "sha256:" + "c" * 64,
            "containers": [],
        }
        self.password = secrets.token_hex(32)
        (self.root / "run" / "secrets" / "database-password").write_text(self.password)
        self.original_key = base64.urlsafe_b64encode(secrets.token_bytes(32)).decode()
        fixture.write_json(
            self.root / "run" / "secrets" / "key-ring.json", {"keys": [self.original_key]},
            private=True,
        )
        fixture.write_json(
            self.root / "run" / "secrets" / "bridge-auth.json",
            {"keys": [base64.urlsafe_b64encode(secrets.token_bytes(32)).decode()]},
            private=True,
        )

    def tearDown(self):
        fixture.WORK = self.previous
        self.assertEqual(self.root.parent, self.previous)
        self.assertFalse(self.root.is_symlink())
        shutil.rmtree(self.root)

    def test_refuses_foreign_container_without_stopping_it(self):
        foreign = {
            "Id": "fictional-foreign-container",
            "Config": {"Labels": {"com.docker.compose.service": "database"}},
        }
        with patch.object(fixture, "docker", side_effect=["fictional-foreign-container", json.dumps([foreign])]) as docker:
            with self.assertRaisesRegex(fixture.FixtureError, "Ownership mismatch"):
                fixture.stop(self.value)
            self.assertFalse(any(call.args[0] == "stop" for call in docker.call_args_list))

    def test_refuses_reused_database_volume(self):
        volume = self.value["project"] + "_database"
        with patch.object(fixture, "docker", side_effect=[volume, json.dumps([{"Labels": {}}])]):
            with self.assertRaisesRegex(fixture.FixtureError, "Ownership mismatch"):
                fixture.owned_storage(self.value)

    def test_refuses_changed_configuration_without_overwriting_it(self):
        fixture.render(self.value)
        path = self.root / "run" / "compose.json"
        path.write_text('{"fictional_unknown_edit": true}\n')
        with self.assertRaisesRegex(fixture.FixtureError, "configuration changed"):
            fixture.verify_config(self.value)
        self.assertIn("fictional_unknown_edit", path.read_text())

    def test_generated_boundary_keeps_secrets_out_of_container_arguments(self):
        fixture.render(self.value)
        rendered = (self.root / "run" / "compose.json").read_text()
        config = json.loads(rendered)
        self.assertNotIn(self.password, rendered)
        self.assertNotIn(self.original_key, rendered)
        self.assertNotIn("networks", config)
        self.assertEqual(config["services"]["proxy"]["network_mode"], "none")
        for name, service in config["services"].items():
            fixture.verify_labels(service["labels"], self.value)
            if name != "proxy":
                self.assertEqual(service["network_mode"], "service:proxy")
            self.assertNotIn("ports", service)
        self.assertEqual((self.root / "run" / "secrets" / "key-ring.json").stat().st_mode & 0o777, 0o600)
        fixture.verify_config(self.value)

    def test_rotation_retains_old_decryption_keys(self):
        fixture.render(self.value)
        running = [{"State": {"Running": True}} for _ in range(4)]
        with (
            patch.object(fixture, "owned_containers", return_value=running),
            patch.object(fixture, "compose"),
            patch.object(fixture, "record_resources"),
            patch.object(fixture, "ready"),
        ):
            fixture.rotate(self.value)
        ring = fixture.read_json(self.root / "run" / "secrets" / "key-ring.json")["keys"]
        self.assertEqual(len(ring), 2)
        self.assertTrue(ring[1] == self.original_key)
        self.assertTrue(ring[0] != self.original_key)
        fixture.verify_config(self.value)

    def test_missing_stack_cannot_silently_rotate_or_start_other_services(self):
        fixture.render(self.value)
        with patch.object(fixture, "owned_containers", return_value=[]), patch.object(fixture, "compose") as compose:
            with self.assertRaisesRegex(fixture.FixtureError, "requires"):
                fixture.rotate(self.value)
            compose.assert_not_called()
        self.assertEqual(
            fixture.read_json(self.root / "run" / "secrets" / "key-ring.json")["keys"],
            [self.original_key],
        )

    def test_corrupt_cached_download_is_preserved_and_rejected(self):
        path = self.root / "fictional-source.tar.gz"
        path.write_bytes(b"fictional corrupt input")
        with patch.object(fixture.urllib.request, "urlopen") as network:
            with self.assertRaisesRegex(fixture.FixtureError, "does not match"):
                fixture.fetch(path, {"sha256": "0" * 64, "bytes": 7})
            network.assert_not_called()
        self.assertEqual(path.read_bytes(), b"fictional corrupt input")

    def test_failed_atomic_key_update_preserves_previous_ring(self):
        path = self.root / "run" / "secrets" / "key-ring.json"
        original = path.read_bytes()
        with patch.object(Path, "replace", side_effect=OSError("fictional write failure")):
            with self.assertRaises(OSError):
                fixture.write_json(path, {"keys": ["fictional-new-key"]}, private=True)
        self.assertTrue(path.read_bytes() == original)
        self.assertFalse(list(path.parent.glob(".key-ring.json.next-*")))

    def test_namespace_recreation_removes_only_stopped_owned_containers_not_volumes(self):
        containers = [
            {"Id": "fictional-" + service, "State": {"Running": False},
             "Config": {"Labels": {"com.docker.compose.service": service}}}
            for service in ("database", "proxy", "endpoint", "connect")
        ]
        with (
            patch.object(fixture, "owned_containers", return_value=containers),
            patch.object(fixture, "docker") as docker,
            patch.object(fixture, "compose") as compose,
            patch.object(fixture, "record_resources"),
            patch.object(fixture, "verify_network_none"),
        ):
            fixture.recreate_containers(self.value)
        self.assertEqual(
            [call.args for call in docker.call_args_list],
            [("rm", "fictional-" + service) for service in ("connect", "endpoint", "database", "proxy")],
        )
        self.assertEqual(compose.call_args_list[0].args[-1], "proxy")
        self.assertNotIn("--force-recreate", compose.call_args_list[1].args)

    def test_recreation_refuses_an_in_use_namespace(self):
        with (
            patch.object(fixture, "owned_containers", return_value=[{"State": {"Running": True}}]),
            patch.object(fixture, "docker") as docker,
        ):
            with self.assertRaisesRegex(fixture.FixtureError, "in-use"):
                fixture.recreate_containers(self.value)
            docker.assert_not_called()

    def test_resource_snapshot_cannot_undo_a_completed_rotation(self):
        fixture.render(self.value)
        current = fixture.read_json(self.root / "run.json")
        current["config_sha256"]["connect.toml"] = "fictional-completed-rotation"
        fixture.write_json(self.root / "run.json", current, private=True)
        with patch.object(fixture, "owned_containers", return_value=[]):
            fixture.record_resources(self.value)
        self.assertEqual(
            fixture.read_json(self.root / "run.json")["config_sha256"]["connect.toml"],
            "fictional-completed-rotation",
        )


if __name__ == "__main__":
    unittest.main()
