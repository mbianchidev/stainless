"""Source-informed checks using Chromium's real Finder metadata implementation."""

import importlib.util
from pathlib import Path
import re
import shutil
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SOURCE = Path(sys.argv.pop(1))
BACKGROUND = Path(sys.argv.pop(1))
SCRIPT = Path(__file__).resolve().parents[2] / "tools/xtask/macos/dmg_layout.py"
SPEC = importlib.util.spec_from_file_location("stainless_dmg_layout", SCRIPT)
LAYOUT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LAYOUT)
LAYOUT.load_generator(SOURCE)
from ds_store import DSStore
from mac_alias import Alias


class FinderLayoutTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "Stainless.app").mkdir()
        (self.root / "Applications").symlink_to("/Applications")
        (self.root / ".background").mkdir()
        shutil.copyfile(BACKGROUND, self.root / ".background/background.png")

    def test_round_trip_preserves_large_draggable_items_and_background(self):
        LAYOUT.prepare_layout(SOURCE, self.root, "Stainless Fictional Preview")
        LAYOUT.validate_layout(self.root, "Stainless Fictional Preview")
        with DSStore.open(str(self.root / ".DS_Store"), "r") as store:
            bounds = [int(value) for value in re.findall(r"\d+", store["."]["bwsp"]["WindowBounds"])]
            self.assertGreaterEqual(bounds[2], 540 + 64)
            self.assertGreaterEqual(bounds[3], 200 + 128)
            self.assertFalse(store["."]["bwsp"]["ShowToolbar"])
            self.assertEqual(store["."]["icvp"]["iconSize"], 128)
            self.assertEqual(store["."]["icvp"]["textSize"], 16)

    def test_wrong_install_destination_is_rejected_before_metadata_write(self):
        (self.root / "Applications").unlink()
        (self.root / "Applications").symlink_to("/fictional-not-applications")
        with self.assertRaisesRegex(ValueError, "Applications link"):
            LAYOUT.prepare_layout(SOURCE, self.root, "Stainless Fictional Preview")
        self.assertFalse((self.root / ".DS_Store").exists())
        self.assertTrue((self.root / "Stainless.app").is_dir())
        self.assertEqual((self.root / ".background/background.png").read_bytes(), BACKGROUND.read_bytes())

    def test_background_alias_for_another_volume_is_rejected(self):
        LAYOUT.prepare_layout(SOURCE, self.root, "Stainless Fictional Preview")
        with DSStore.open(str(self.root / ".DS_Store"), "r+") as store:
            options = store["."]["icvp"]
            alias = Alias.from_bytes(options["backgroundImageAlias"])
            alias.volume.name = "Fictional Wrong Volume"
            options["backgroundImageAlias"] = alias.to_bytes()
            store["."]["icvp"] = options
        with self.assertRaisesRegex(ValueError, "background alias"):
            LAYOUT.validate_layout(self.root, "Stainless Fictional Preview")


if __name__ == "__main__":
    unittest.main()
