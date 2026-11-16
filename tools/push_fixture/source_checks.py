#!/usr/bin/env python3
"""Run and record real fixture/upstream source tests, without starting services."""

import argparse
import contextlib
import datetime as dt
import io
import os
from pathlib import Path
import re
import subprocess
import unittest

import fixture


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    args = parser.parse_args()
    cache = args.cache.resolve()
    source = fixture.prepare(cache)
    fixture.WORK.mkdir(exist_ok=True)
    report_path = fixture.WORK / "source-tests.json"
    report = {
        "passed": False,
        "inputs_sha256": fixture.digest(fixture.HERE / "inputs.json"),
        "source_sha256": fixture.pins()["patched_tree_sha256"],
        "started_at": dt.datetime.now(dt.UTC).isoformat(),
        "upstream": {},
        "logs_sha256": {},
    }
    fixture.write_json(report_path, report)
    output = io.StringIO()
    suite = unittest.defaultTestLoader.discover(str(fixture.HERE), pattern="test_fixture.py")
    with contextlib.redirect_stdout(output):
        result = unittest.TextTestRunner(stream=output, verbosity=0).run(suite)
    report["lifecycle"] = {
        "ran": result.testsRun, "failures": len(result.failures), "errors": len(result.errors),
        "skipped": len(result.skipped),
    }
    fixture.write_json(report_path, report)
    if not result.wasSuccessful() or not result.testsRun or result.skipped:
        raise fixture.FixtureError("Fixture source tests failed; no passing evidence recorded")
    target = cache / "build" / "push-fixture-native"
    target.mkdir(parents=True, exist_ok=True)
    environment = {
        **os.environ, "CARGO_TARGET_DIR": str(target), "SENTRY_DSN": "", "RUST_LOG": "error",
    }
    groups = [
        ("settings_web", ["-p", "autoconnect_settings", "-p", "autoconnect_web", "--lib"], [4, 8]),
        ("endpoint", ["-p", "autoendpoint", "--bin", "autoendpoint"], [64]),
    ]
    for name, selection, expected_counts in groups:
        log = fixture.WORK / ("source-" + name + ".log")
        command = [
            "cargo", "test", "--locked", "--no-default-features",
            "--features", "postgres", "--jobs", "2", *selection,
        ]
        with log.open("w") as stream:
            completed = subprocess.run(
                command, cwd=source, env=environment, stdout=stream, stderr=subprocess.STDOUT,
            )
        matches = re.findall(
            r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;",
            log.read_text(),
        )
        counts = [int(item[0]) for item in matches]
        if completed.returncode or counts != expected_counts or any(
            int(value) for match in matches for value in match[1:]
        ):
            raise fixture.FixtureError(f"Original upstream {name} tests failed or were skipped; inspect {log}")
        report["upstream"][name] = counts
        report["logs_sha256"][name] = fixture.digest(log)
        fixture.write_json(report_path, report)
    report.update(
        passed=True, completed_at=dt.datetime.now(dt.UTC).isoformat(),
        rustc=fixture.run(["rustc", "--version"]), cargo=fixture.run(["cargo", "--version"]),
    )
    fixture.write_json(report_path, report)
    print(f"Source checks passed: {result.testsRun} fixture tests, {sum(sum(x) for x in report['upstream'].values())} upstream tests.")


if __name__ == "__main__":
    main()
