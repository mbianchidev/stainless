#!/usr/bin/env python3
"""Prepare Finder metadata using the pinned Chromium installer libraries."""

import argparse
import importlib.util
import os
from pathlib import Path
import struct


def load_generator(source):
    script = source / "chrome/installer/mac/generate_dsstore.py"
    spec = importlib.util.spec_from_file_location("stainless_dsstore_generator", script)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Cannot load pinned Chromium DMG generator: {script}")
    generator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generator)
    return generator


def validate_layout(root, volume_name):
    from ds_store import DSStore
    from mac_alias import Alias

    if (
        not (root / "Applications").is_symlink()
        or os.readlink(root / "Applications") != "/Applications"
    ):
        raise ValueError("Installer Applications link must point to /Applications")
    if not (root / "Stainless.app").is_dir():
        raise ValueError("Installer is missing its actual Stainless.app bundle")
    png = (root / ".background/background.png").read_bytes()
    if (
        len(png) < 24
        or png[:8] != b"\x89PNG\r\n\x1a\n"
        or struct.unpack(">II", png[16:24]) != (1440, 960)
    ):
        raise ValueError("Installer background must be a 1440x960 Retina PNG")
    with DSStore.open(str(root / ".DS_Store"), "r") as store:
        options = store["."]["icvp"]
        if store["."]["icvl"] != (b"type", b"icnv") or options["arrangeBy"] != "none":
            raise ValueError("Installer must retain icon view without automatic arrangement")
        if options["iconSize"] < 128 or options["textSize"] < 16:
            raise ValueError("Installer icons or labels are too small")
        if (
            store["Stainless.app"]["Iloc"] != (180, 200)
            or store["Applications"]["Iloc"] != (540, 200)
        ):
            raise ValueError("Installer must position the app left of Applications")
        alias = Alias.from_bytes(options["backgroundImageAlias"])
        if (
            options["backgroundType"] != 2
            or alias.volume.name != volume_name
            or alias.target.posix_path != "/.background/background.png"
        ):
            raise ValueError("Installer background alias does not match its volume and hidden image")


def prepare_layout(source, root, volume_name):
    if (
        not (root / "Applications").is_symlink()
        or os.readlink(root / "Applications") != "/Applications"
    ):
        raise ValueError("Refusing to prepare an installer with an invalid Applications link")
    generator = load_generator(source)
    generator.main([
        str(root / ".DS_Store"),
        "--volume-name", volume_name,
        "--app-name", "Stainless.app",
        "--window-width", "720", "--window-height", "480",
        "--icon-size", "128", "--text-size", "16",
        "--app-x", "180", "--app-y", "200",
        "--link-x", "540", "--link-y", "200",
    ])
    from ds_store import DSStore

    with DSStore.open(str(root / ".DS_Store"), "r+") as store:
        # Chromium's template uses an unlabeled link; Stainless retains its name.
        store["Applications"]["Iloc"] = store[" "]["Iloc"]
        del store[" "]["Iloc"]
    validate_layout(root, volume_name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("image_root", type=Path)
    parser.add_argument("volume_name")
    args = parser.parse_args()
    prepare_layout(args.source, args.image_root, args.volume_name)


if __name__ == "__main__":
    main()
