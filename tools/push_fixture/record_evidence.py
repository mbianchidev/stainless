#!/usr/bin/env python3
"""Capture sanitized evidence only after the real fixture suite and cleanup pass."""

import argparse
import datetime as dt
import json
from pathlib import Path
import re
import uuid

import fixture


def binary_hashes(built: dict) -> dict:
    # Porto implements docker cp via exec and cannot inspect a stopped scratch
    # image that has no tar utility. Copy only its binaries into a pinned,
    # network-none checksum stage, never into a host installation.
    locked = fixture.pins()
    image = json.loads(fixture.docker("image", "inspect", built["tag"]))[0]
    if image["Id"] != built["image"]:
        raise fixture.FixtureError("Local image tag changed before provenance capture")
    context = fixture.WORK / "binary-hash-context"
    context.mkdir(exist_ok=True)
    if any(context.iterdir()):
        raise fixture.FixtureError("Checksum build context must be empty")
    tag = "stainless-push-hash:" + uuid.uuid4().hex[:12]
    resource = {"tag": tag, "context": "porto", "purpose": "bounded binary checksums", "removed": False}
    fixture.write_json(fixture.WORK / "binary-hash-resource.json", resource)
    log = fixture.WORK / "binary-hash.log"
    ownership = fixture.state()
    label_args = [
        argument for key, value in fixture.labels(ownership).items()
        for argument in ("--label", key + "=" + value)
    ]
    try:
        fixture.run(
            [
                "docker", "--context", "porto", "build", "--no-cache", "--network", "none",
                "--platform", locked["platform"], "--progress", "plain",
                "--build-arg", "SERVER=" + built["tag"],
                "--build-arg", "BUILDER=" + locked["images"]["builder"],
                "--label", fixture.OWNER + ".purpose=binary-checksums",
                *label_args,
                "-f", str(fixture.HERE / "hash-binaries.Dockerfile"), "-t", tag, str(context),
            ],
            log=log,
        )
        contents = log.read_text()
        hashes = dict(
            (name, value) for value, name in re.findall(
                r"\b([a-f0-9]{64})\s+/fixture/(autoconnect|autoendpoint)\b", contents
            )
        )
        sizes = dict(
            (name, int(value)) for value, name in re.findall(
                r"\b(\d+)\s+/fixture/(autoconnect|autoendpoint)\b", contents
            )
        )
        if set(hashes) != {"autoconnect", "autoendpoint"} or set(sizes) != set(hashes):
            raise fixture.FixtureError("Incomplete binary provenance")
        return {name: {"sha256": value, "bytes": sizes[name]} for name, value in hashes.items()}
    finally:
        ids = fixture.docker("image", "ls", "--quiet", "--no-trunc", "--filter", "reference=" + tag).split()
        for identity in set(ids):
            candidate = json.loads(fixture.docker("image", "inspect", identity))[0]
            labels = candidate["Config"].get("Labels") or {}
            fixture.verify_labels(labels, ownership)
            if labels.get(fixture.OWNER + ".purpose") != "binary-checksums":
                raise fixture.FixtureError("Checksum image ownership mismatch")
            resource["image"] = candidate["Id"]
            fixture.docker("image", "rm", "--no-prune", candidate["Id"])
        if fixture.docker("image", "ls", "--quiet", "--filter", "reference=" + tag):
            raise fixture.FixtureError("Checksum image cleanup incomplete")
        resource["removed"] = True
        fixture.write_json(fixture.WORK / "binary-hash-resource.json", resource)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    args = parser.parse_args()
    locked = fixture.pins()
    source = fixture.prepare(args.cache.resolve())
    report_path = fixture.WORK / "protocol-report-final.json"
    report = fixture.read_json(report_path)
    built = fixture.read_json(fixture.WORK / "build.json")
    audit = fixture.read_json(fixture.WORK / "cargo-audit.json")
    python_audit = fixture.read_json(fixture.WORK / "python-audit-final.json")
    lifecycle_path = fixture.WORK / "lifecycle-final.json"
    lifecycle = fixture.read_json(lifecycle_path)
    tests = fixture.read_json(fixture.WORK / "source-tests.json")
    value = fixture.state()
    containers = fixture.owned_containers(value)
    names = {item["name"] for item in report["cases"]}
    required = {
        "runtime_scope_and_private_admin", "tls_verification", "wire_shapes_and_reregistration",
        "encrypted_and_vapid", "payloadless_and_unregister", "unacked_redelivery",
        "every_ack_consumes", "partial_batch_blocks", "offline_expiry",
        "resume_registration_and_rollback", "restart_and_key_rotation",
        "fresh_init_refuses_existing_data", "backlog_reset",
        "hello_and_json_heartbeat_deadlines", "real_control_ping_pong",
        "logs_before_rotation", "logs_exclude_credentials_and_payloads",
    }
    if not required <= names or any(item["status"] != "passed" for item in report["cases"]):
        raise fixture.FixtureError("Complete, successful real-server evidence is required")
    if report["image"] != built["image"] or report["inputs_sha256"] != fixture.digest(fixture.HERE / "inputs.json"):
        raise fixture.FixtureError("Report belongs to another image or input revision")
    if (
        lifecycle["image"] != built["image"] or not lifecycle["cases"]
        or any(item["status"] != "passed" for item in lifecycle["cases"])
    ):
        raise fixture.FixtureError("Final lifecycle regression evidence is required")
    if (
        tests.get("passed") is not True
        or tests["inputs_sha256"] != fixture.digest(fixture.HERE / "inputs.json")
        or tests["source_sha256"] != locked["patched_tree_sha256"]
    ):
        raise fixture.FixtureError("Run source_checks.py for this exact input/source revision")
    if (
        audit["vulnerabilities"]["found"] or audit["warnings"]
        or audit["settings"]["ignore"] or audit["settings"]["target_arch"]
        or audit["settings"]["target_os"]
    ):
        raise fixture.FixtureError("Unfiltered clean Cargo audit evidence is required")
    if any(item.get("vulns") or item.get("skip_reason") for item in python_audit["dependencies"]):
        raise fixture.FixtureError("Complete Python advisory evidence is required")
    if any(item["State"]["Running"] for item in containers):
        raise fixture.FixtureError("Stop owned services before capturing final evidence")
    if (fixture.WORK / "relay.json").exists() or (fixture.WORK / "run/relay-control.sock").exists():
        raise fixture.FixtureError("Owned relay shutdown has not completed")
    evidence = {
        "schema_version": 1,
        "stage": "local server/raw-UA interoperability only; not a release gate",
        "captured_at": dt.datetime.now(dt.UTC).isoformat(),
        "source": {
            "base": locked["source"],
            "identity": "Autopush 1.86.0 + Stainless downstream security/integrity patches",
            "patched_tree_sha256": locked["patched_tree_sha256"],
            "patches": locked["patches"],
            "cargo_lock_sha256": fixture.digest(source / "Cargo.lock"),
            "fresh_schema_sha256": locked["fresh_schema_sha256"],
        },
        "build": {
            "platform": locked["platform"], "context": "porto",
            "images": locked["images"], "server_image": built["image"],
            "cargo_jobs": 2, "no_cache": True, "default_features": False,
            "features": ["postgres"], "postgres_status": "enterprise-beta",
            "binaries": binary_hashes(built),
            "warnings": [
                "Upstream deprecated singular crypto_key compatibility field; fixture uses crypto_keys.",
                "Docker lint: required BUILDER ARG has no implicit default; build supplies the immutable pin.",
            ],
        },
        "audits": {
            "cargo_audit": {
                "version": locked["cargo_audit_version"], "deny": "warnings",
                "dependencies": audit["lockfile"]["dependency-count"],
                "database": audit["database"], "vulnerabilities": 0, "warnings": 0,
                "ignores": [], "target_filters": [],
                "report_sha256": fixture.digest(fixture.WORK / "cargo-audit.json"),
            },
            "python": {
                "distributions": len(python_audit["dependencies"]), "known_vulnerabilities": 0,
                "report_sha256": fixture.digest(fixture.WORK / "python-audit-final.json"),
            },
        },
        "verification": {
            "upstream_tests": tests["upstream"],
            "fixture_lifecycle_tests": tests["lifecycle"],
            "source_test_report_sha256": fixture.digest(fixture.WORK / "source-tests.json"),
            "real_protocol_cases": report["cases"],
            "post_suite_lifecycle_cases": lifecycle["cases"],
            "post_suite_lifecycle_report_sha256": fixture.digest(lifecycle_path),
            "observations": report["observations"],
            "report_sha256": fixture.digest(report_path),
            "test_removals": "None. Original WebSocket/metrics tests use an HTTP/1-only harness; optional reliability assertions are feature-correct.",
        },
        "cleanup": {
            "owned_containers_running": 0, "loopback_relay_running": False,
            "database_and_secret_ring": "Retained locally for parent browser tests; no blanket reset or volume deletion.",
        },
        "limitations": report["limitations"] + [
            "Porto internal:true is not relied on; actual network-none namespace and fixed byte relays are used.",
            "No per-container log-rotation capability in Porto; long-running production retention is not certified.",
            "JWT expiration uses the tagged validator's 60s clock-skew allowance; rejection is tested at minus 600s.",
            "No high-concurrency/millisecond sort-key collision or production load/abuse guarantee.",
            "No production OS-image vulnerability/aggregate licensing approval or publication is claimed.",
        ],
        "files_sha256_at_capture": {
            str(path.relative_to(fixture.HERE)): fixture.digest(path)
            for path in sorted(fixture.HERE.rglob("*"))
            if path.is_file()
            and path.relative_to(fixture.HERE).parts[0] not in {".work", "__pycache__"}
            and path.name != "evidence.json"
        },
        "operator_guide_sha256": fixture.digest(
            fixture.HERE.parent.parent / "docs/web-push-self-hosting.md"
        ),
    }
    fixture.write_json(fixture.HERE / "evidence.json", evidence)
    print(json.dumps({"captured": True, "real_protocol_cases": len(report["cases"]),
                      "server_image": built["image"]}, indent=2))


if __name__ == "__main__":
    main()
