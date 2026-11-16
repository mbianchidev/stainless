#!/usr/bin/env python3
"""Owned, opt-in Autopush/Postgres interoperability fixture (not a deployment)."""

from __future__ import annotations

import argparse
import asyncio
import base64
import datetime as dt
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import tarfile
import time
import urllib.request
import uuid

HERE = Path(__file__).resolve().parent
WORK = HERE / ".work"
OWNER = "org.stainless.fixture"
SERVICES = {"database", "connect", "endpoint", "proxy"}


class FixtureError(RuntimeError):
    pass


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text())


def write_json(path: Path, value: object, *, private: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise FixtureError("Refusing to overwrite a symlink")
    pending = path.with_name("." + path.name + ".next-" + uuid.uuid4().hex)
    descriptor = os.open(pending, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    try:
        with os.fdopen(descriptor, "w") as stream:
            stream.write(json.dumps(value, indent=2, sort_keys=True) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        pending.chmod(0o600 if private else 0o644)
        pending.replace(path)
    finally:
        if pending.exists():
            pending.unlink()


def run(args: list[str], *, cwd: Path | None = None, log: Path | None = None) -> str:
    if log:
        with log.open("w") as stream:
            result = subprocess.run(args, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT)
        if result.returncode:
            raise FixtureError(f"Command failed ({result.returncode}); inspect {log}")
        return ""
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True)
    if result.returncode:
        # Container inspection/config output can contain secrets. Never echo it.
        text = result.stdout + "\n" + result.stderr
        rings = [
            WORK / "run" / "secrets" / name
            for name in ("key-ring.json", "bridge-auth.json")
        ]
        password = WORK / "run" / "secrets" / "database-password"
        for ring in rings:
            if ring.is_file():
                for key in read_json(ring)["keys"]:
                    text = text.replace(key, "[redacted key]")
        if password.is_file():
            text = text.replace(password.read_text().strip(), "[redacted password]")
        text = re.sub(r"postgres(?:ql)?://[^\s\"']+", "[redacted database URL]", text)
        write_json(
            WORK / "last-command-error.json",
            {"command": args[:5], "exit_code": result.returncode, "diagnostic": text[-12000:]},
            private=True,
        )
        raise FixtureError(f"Command failed ({result.returncode}); see .work/last-command-error.json")
    return result.stdout.strip()


def docker(*args: str) -> str:
    return run(["docker", "--context", "porto", *args])


def pinned_image(reference: str) -> str:
    expected = reference.rsplit("@", 1)[1]
    docker("pull", "--platform", pins()["platform"], reference)
    try:
        image = json.loads(docker("image", "inspect", reference))[0]
    except FixtureError:
        # Porto 1.10 indexes digest pulls by their immutable manifest ID.
        image = json.loads(docker("image", "inspect", expected))[0]
    if not any(item.endswith("@" + expected) for item in image.get("RepoDigests", [])):
        raise FixtureError("Runtime did not confirm the pinned OCI digest")
    if image.get("Os") != "linux" or image.get("Architecture") != "arm64":
        raise FixtureError("Runtime image platform does not match the reviewed pin")
    return image["Id"]


def pins() -> dict:
    value = read_json(HERE / "inputs.json")
    if value["container_context"] != "porto" or value["platform"] != "linux/arm64":
        raise FixtureError("Review context/platform changes before using this fixture")
    return value


def tree_digest(root: Path) -> str:
    result = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if relative.parts[0] in {"target", ".git"}:
            continue
        if path.is_symlink():
            raise FixtureError("Unexpected symlink in source tree")
        if path.is_file():
            result.update(relative.as_posix().encode() + b"\0")
            result.update(bytes.fromhex(digest(path)))
    return result.hexdigest()


def fetch(destination: Path, item: dict) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if not destination.exists():
        partial = destination.with_suffix(destination.suffix + ".partial")
        if partial.exists():
            raise FixtureError(f"Unfinished download requires inspection: {partial}")
        with urllib.request.urlopen(item["url"], timeout=60) as response:
            with partial.open("xb") as output:
                shutil.copyfileobj(response, output)
        if digest(partial) != item["sha256"] or partial.stat().st_size != item["bytes"]:
            raise FixtureError(f"Downloaded input does not match pin: {partial}")
        partial.rename(destination)
    if destination.is_symlink() or digest(destination) != item["sha256"]:
        raise FixtureError(f"Existing input does not match pin: {destination}")
    if destination.stat().st_size != item["bytes"]:
        raise FixtureError(f"Input size does not match pin: {destination}")


def extract(archive: Path, destination: Path, expected_root: str) -> None:
    with tarfile.open(archive) as source:
        for member in source.getmembers():
            path = Path(member.name)
            if (
                path.is_absolute()
                or ".." in path.parts
                or not path.parts
                or path.parts[0] != expected_root
                or not (member.isfile() or member.isdir())
            ):
                raise FixtureError("Unsafe member in source archive")
        source.extractall(destination, filter="data")


def prepare(cache: Path) -> Path:
    locked = pins()
    target = cache / "sources" / ("push-fixture-autopush-" + locked["patched_tree_sha256"][:16])
    if target.exists():
        if tree_digest(target) != locked["patched_tree_sha256"]:
            raise FixtureError(f"Prepared source was changed; preserving it: {target}")
        return target
    upstream = locked["source"]
    archive = cache / "downloads" / "autopush-60841760.tar.gz"
    fetch(archive, upstream)
    staging = cache / "sources" / ("push-fixture-prepare-" + locked["patched_tree_sha256"][:16])
    staging.mkdir(parents=True, exist_ok=False)
    extract(archive, staging, upstream["root"])
    root = staging / upstream["root"]
    for name, item in locked["patched_dependencies"].items():
        archive = cache / "downloads" / f"push-fixture-{name}.crate"
        fetch(archive, item)
        extract(archive, root / "vendor", name)
    for name, item in locked["build_tools"].items():
        archive = cache / "downloads" / f"push-fixture-{name}.tar.gz"
        fetch(archive, item)
        (root / ".fixture-tools").mkdir(exist_ok=True)
        shutil.copyfile(archive, root / ".fixture-tools" / (name + ".tar.gz"))
    for item in locked["patches"]:
        patch = HERE / item["path"]
        if digest(patch) != item["sha256"]:
            raise FixtureError("Downstream patch digest mismatch")
        patch_root = root / item["root"]
        run(["git", "apply", "--check", str(patch)], cwd=patch_root)
        run(["git", "apply", str(patch)], cwd=patch_root)
    if tree_digest(root) != locked["patched_tree_sha256"]:
        raise FixtureError(f"Patched source digest mismatch; retained for inspection: {root}")
    root.rename(target)
    staging.rmdir()
    return target


def audit(cache: Path, source: Path) -> dict:
    version = pins()["cargo_audit_version"]
    binary = cache / "tools" / f"push-fixture-audit-{version}" / "bin" / "cargo-audit"
    if not binary.is_file() or run([str(binary), "--version"]).split()[-1] != version:
        raise FixtureError(
            f"Install cargo-audit {version} --locked --root {binary.parent.parent} "
            "(do not overwrite the global tool)"
        )
    WORK.mkdir(exist_ok=True)
    result = subprocess.run(
        [
            str(binary), "audit", "--file", str(source / "Cargo.lock"),
            "--db", str(cache / "sources" / "push-fixture-advisory-db"),
            "--deny", "warnings", "--json",
        ],
        cwd=source, text=True, capture_output=True,
    )
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise FixtureError("Audit did not produce evidence; no server may start") from error
    write_json(WORK / "cargo-audit.json", report)
    if (
        result.returncode
        or report["vulnerabilities"]["found"]
        or report["warnings"]
        or report["settings"]["ignore"]
        or report["settings"]["target_arch"]
        or report["settings"]["target_os"]
    ):
        raise FixtureError("Audit failed; inspect .work/cargo-audit.json; no server may start")
    evidence = {
        "version": version,
        "checked_at": dt.datetime.now(dt.UTC).isoformat(),
        "lock_sha256": digest(source / "Cargo.lock"),
        "report_sha256": digest(WORK / "cargo-audit.json"),
        "database": report["database"],
    }
    write_json(WORK / "audit.json", evidence)
    return evidence


def build(cache: Path) -> None:
    locked = pins()
    source = prepare(cache)
    evidence = audit(cache, source)
    tag = "stainless-push-fixture:" + locked["patched_tree_sha256"][:16]
    run(
        [
            "docker", "--context", "porto", "build", "--no-cache", "--pull",
            "--platform", locked["platform"],
            "--build-arg", "BUILDER=" + locked["images"]["builder"],
            "--label", OWNER + "=web-push",
            "--label", OWNER + ".source=" + locked["patched_tree_sha256"],
            "-f", str(HERE / "Dockerfile"), "-t", tag, str(source),
        ],
        log=WORK / "build.log",
    )
    image = json.loads(docker("image", "inspect", tag))[0]
    write_json(
        WORK / "build.json",
        {
            "image": image["Id"], "tag": tag, "source": str(source),
            "inputs_sha256": digest(HERE / "inputs.json"),
            "dockerfile_sha256": digest(HERE / "Dockerfile"), "audit": evidence,
        },
    )
    print(json.dumps({"image": image["Id"], "build_log": str(WORK / "build.log")}))


def free_ports() -> tuple[int, int]:
    with socket.socket() as first, socket.socket() as second:
        first.bind(("127.0.0.1", 0))
        second.bind(("127.0.0.1", 0))
        return first.getsockname()[1], second.getsockname()[1]


def state() -> dict:
    value = read_json(WORK / "run.json")
    if value["workspace"] != str(HERE.parent.parent):
        raise FixtureError("Run belongs to another workspace")
    if not re.fullmatch(r"stainless-push-[0-9a-f]{12}", value["project"]):
        raise FixtureError("Invalid owned Compose project")
    return value


def compose(value: dict, *args: str) -> str:
    return docker(
        "compose", "--project-name", value["project"],
        "--project-directory", str(WORK / "run"),
        "--file", str(WORK / "run" / "compose.json"), *args,
    )


def labels(value: dict) -> dict:
    return {
        OWNER: "web-push",
        OWNER + ".run": value["run_id"],
        OWNER + ".workspace": hashlib.sha256(value["workspace"].encode()).hexdigest(),
    }


def verify_labels(actual: dict, value: dict) -> None:
    if any(actual.get(key) != expected for key, expected in labels(value).items()):
        raise FixtureError("Ownership mismatch; refusing to act on this resource")


def owned_containers(value: dict) -> list[dict]:
    ids = docker(
        "ps", "-aq", "--filter", "label=com.docker.compose.project=" + value["project"]
    ).split()
    if not ids:
        return []
    containers = json.loads(docker("inspect", *ids))
    for container in containers:
        actual = container["Config"]["Labels"]
        verify_labels(actual, value)
        if actual.get("com.docker.compose.service") not in SERVICES:
            raise FixtureError("Unknown service in project; refusing whole-project cleanup")
    return containers


def owned_storage(value: dict) -> None:
    for kind, suffix in (("volume", "_database"),):
        name = value["project"] + suffix
        existing = docker(kind, "ls", "--format", "{{.Name}}", "--filter", "name=" + name).splitlines()
        if name in existing:
            item = json.loads(docker(kind, "inspect", name))[0]
            verify_labels(item.get("Labels") or {}, value)


def verify_network_none(value: dict, containers: list[dict] | None = None) -> None:
    containers = owned_containers(value) if containers is None else containers
    proxy = next(
        item for item in containers
        if item["Config"]["Labels"]["com.docker.compose.service"] == "proxy"
    )
    for item in containers:
        mode = item["HostConfig"]["NetworkMode"]
        if item["Id"] == proxy["Id"]:
            if mode != "none":
                raise FixtureError("Proxy network must be none, not a routable bridge")
        elif mode != "container:" + proxy["Id"]:
            raise FixtureError("Server/database did not join the owned isolated namespace")
        if item["HostConfig"].get("PortBindings"):
            raise FixtureError("An isolated container unexpectedly published a host port")
    interfaces = docker("exec", proxy["Id"], "cat", "/proc/net/dev")
    names = {line.split(":")[0].strip() for line in interfaces.splitlines() if ":" in line}
    if names != {"lo"}:
        raise FixtureError("Actual namespace has a non-loopback network interface")
    routes = docker("exec", proxy["Id"], "cat", "/proc/net/route").splitlines()[1:]
    if any(line.split() and line.split()[0] != "lo" for line in routes):
        raise FixtureError("Actual namespace contains an external route")
    status = docker("exec", proxy["Id"], "cat", "/proc/1/status")
    fields = dict(line.split(":", 1) for line in status.splitlines() if ":" in line)
    if fields.get("NoNewPrivs", "").strip() != "1":
        raise FixtureError("Runtime did not apply no-new-privileges")


def verify_config(value: dict) -> None:
    for name, expected in value["config_sha256"].items():
        root = HERE if name in {"Caddyfile", "fresh-init.sql"} else WORK / "run"
        if digest(root / name) != expected:
            raise FixtureError("Run configuration changed; preserving it without starting services")


def record_resources(value: dict) -> list[dict]:
    containers = owned_containers(value)
    current = state()
    if current["run_id"] != value["run_id"]:
        raise FixtureError("Run ownership changed while recording resources")
    current["containers"] = [
        {
            "id": item["Id"], "created": item["Created"],
            "service": item["Config"]["Labels"]["com.docker.compose.service"],
            "image": item["Image"], "running": item["State"]["Running"],
        }
        for item in containers
    ]
    write_json(WORK / "run.json", current, private=True)
    value.clear()
    value.update(current)
    return containers


def render(value: dict) -> None:
    root = WORK / "run"
    keys = read_json(root / "secrets" / "key-ring.json")["keys"]
    auth_keys = read_json(root / "secrets" / "bridge-auth.json")["keys"]
    password = (root / "secrets" / "database-password").read_text().strip()
    common = (
        f'db_dsn = "postgresql://autopush:{password}@127.0.0.1/autopush"\n'
        'db_settings = "{}"\n'
        f'crypto_keys = "[{",".join(keys)}]"\n'
        "disable_sentry = true\nhuman_logs = false\n"
    )
    connect = common + (
        'hostname = "0.0.0.0"\nport = 8080\n'
        'router_hostname = "127.0.0.1"\nrouter_port = 8081\n'
        'endpoint_scheme = "https"\nendpoint_hostname = "localhost"\n'
        f'endpoint_port = {value["publishing_port"]}\n'
        "msg_limit = 150\nactix_workers = 2\n"
        "auto_ping_interval = 300\nauto_ping_timeout = 4\n"
        "open_handshake_timeout = 5\n"
    )
    endpoint = common + (
        'host = "0.0.0.0"\nport = 8082\n'
        f'auth_keys = "[{",".join(auth_keys)}]"\n'
        f'endpoint_url = "{value["publishing_origin"]}"\n'
        "max_data_bytes = 16384\n"
        '[fcm]\ncredentials = "{}"\nbase_url = "https://127.0.0.1:9"\n'
        '[apns]\nchannels = "{}"\n'
    )
    for name, text in [("connect", connect), ("endpoint", endpoint)]:
        path = root / (name + ".toml")
        if path.exists():
            path.chmod(0o600)
        path.write_text(text)
        # The parent directory is 0700. Bind mounts expose only this file to
        # the unprivileged container UID, not its private host parent.
        path.chmod(0o444)
    owned = labels(value)
    shared = {
        "platform": pins()["platform"],
        "labels": owned,
        "restart": "no",
        "security_opt": ["no-new-privileges"],
        "network_mode": "service:proxy",
    }
    app = {
        **shared, "image": value["image"], "pull_policy": "never",
        "read_only": True, "cap_drop": ["ALL"],
        "environment": {"RUST_LOG": "error", "SENTRY_DSN": ""},
        "depends_on": {"database": {"condition": "service_healthy"}},
        "mem_limit": "768m", "cpus": 2, "pids_limit": 128,
    }
    services = {
        "database": {
            **shared, "image": value["database_image"], "pull_policy": "never",
            "environment": {
                "POSTGRES_USER": "autopush", "POSTGRES_DB": "autopush",
                "POSTGRES_PASSWORD_FILE": "/run/secrets/database-password",
            },
            "volumes": [
                "database:/var/lib/postgresql",
                "./secrets/database-password:/run/secrets/database-password:ro",
                str(HERE / "fresh-init.sql") + ":/docker-entrypoint-initdb.d/10-autopush.sql:ro",
            ],
            "healthcheck": {
                "test": ["CMD-SHELL", "pg_isready -U autopush -d autopush"],
                "interval": "1s", "timeout": "3s", "retries": 60,
            },
            "mem_limit": "512m", "cpus": 2, "pids_limit": 128,
            "depends_on": {"proxy": {"condition": "service_started"}},
        },
        "connect": {
            **app, "command": ["/bin/autoconnect", "--config", "/run/push/config.toml"],
            "volumes": ["./connect.toml:/run/push/config.toml:ro"],
        },
        "endpoint": {
            **app, "command": ["/bin/autoendpoint", "--config", "/run/push/config.toml"],
            "volumes": ["./endpoint.toml:/run/push/config.toml:ro"],
        },
        "proxy": {
            **shared, "image": value["proxy_image"], "pull_policy": "never",
            "network_mode": "none",
            "user": f"{value['uid']}:{value['gid']}", "read_only": True, "cap_drop": ["ALL"],
            "command": ["caddy", "run", "--config", "/etc/caddy/Caddyfile"],
            "volumes": [
                str(HERE / "Caddyfile") + ":/etc/caddy/Caddyfile:ro",
                value["certificate"] + ":/run/tls/cert.pem:ro",
                value["private_key"] + ":/run/tls/key.pem:ro",
            ],
            "tmpfs": ["/data:size=8m,mode=1777", "/config:size=8m,mode=1777"],
            "mem_limit": "128m", "cpus": 1, "pids_limit": 64,
        },
    }
    write_json(
        root / "compose.json",
        {
            "services": services,
            "volumes": {"database": {"labels": owned}},
        },
        private=True,
    )
    value["config_sha256"] = {
        name: digest(root / name) for name in ("compose.json", "connect.toml", "endpoint.toml")
    }
    value["config_sha256"].update(
        {name: digest(HERE / name) for name in ("Caddyfile", "fresh-init.sql")}
    )
    write_json(WORK / "run.json", value, private=True)


def initialize(args: argparse.Namespace) -> None:
    if (WORK / "run.json").exists() or (WORK / "run").exists():
        raise FixtureError("Run already exists; preserve its database and key ring")
    built = read_json(WORK / "build.json")
    if built["inputs_sha256"] != digest(HERE / "inputs.json"):
        raise FixtureError("Build input pins have changed; rebuild first")
    root = WORK / "run"
    (root / "secrets").mkdir(parents=True, mode=0o700)
    if args.test_certificate:
        run(
            [
                "openssl", "req", "-x509", "-newkey", "ec",
                "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "7",
                "-subj", "/CN=localhost",
                "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1",
                "-keyout", str(root / "secrets" / "tls-key.pem"),
                "-out", str(root / "secrets" / "tls-cert.pem"),
            ]
        )
        certificate = root / "secrets" / "tls-cert.pem"
        private_key = root / "secrets" / "tls-key.pem"
    elif args.cert and args.key:
        certificate = args.cert.resolve(strict=True)
        private_key = args.key.resolve(strict=True)
    else:
        raise FixtureError("Supply --cert and --key, or explicitly request --test-certificate")
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, private_key)
    password = root / "secrets" / "database-password"
    password.write_text(secrets.token_hex(32) + "\n")
    password.chmod(0o600)
    private_key.chmod(0o600) if args.test_certificate else None
    write_json(
        root / "secrets" / "key-ring.json",
        {"keys": [base64.urlsafe_b64encode(secrets.token_bytes(32)).decode()]},
        private=True,
    )
    write_json(
        root / "secrets" / "bridge-auth.json",
        {"keys": [base64.urlsafe_b64encode(secrets.token_bytes(32)).decode()]},
        private=True,
    )
    first, second = free_ports()
    identifier = uuid.uuid4().hex
    value = {
        "workspace": str(HERE.parent.parent), "run_id": identifier,
        "project": "stainless-push-" + identifier[:12],
        "connection_port": first, "publishing_port": second,
        "connection_url": f"wss://localhost:{first}/",
        "publishing_origin": f"https://localhost:{second}",
        "certificate": str(certificate), "private_key": str(private_key),
        "test_certificate": args.test_certificate,
        "uid": os.getuid(), "gid": os.getgid(),
        "database_image": pinned_image(pins()["images"]["database"]),
        "proxy_image": pinned_image(pins()["images"]["proxy"]),
        "image": built["image"], "containers": [],
    }
    write_json(WORK / "run.json", value, private=True)
    render(value)
    print(json.dumps(public_state(value), indent=2))


def public_state(value: dict) -> dict:
    return {
        key: value[key] for key in
        ["project", "connection_url", "publishing_origin", "certificate", "test_certificate"]
    }


def ready(value: dict, *, timeout: float = 60) -> dict:
    context = ssl.create_default_context(cafile=value["certificate"])
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context)
    )
    start = time.monotonic()
    last_error = "not started"
    urls = [
        value["connection_url"].replace("wss:", "https:").rstrip("/") + "/__heartbeat__",
        value["publishing_origin"] + "/__heartbeat__",
    ]
    while time.monotonic() - start < timeout:
        try:
            for url in urls:
                with opener.open(url, timeout=3) as response:
                    if response.status != 200:
                        raise FixtureError("Health endpoint not ready")
                    payload = json.load(response)
                    if payload.get("status") != "OK" or payload.get("version") != "1.86.0":
                        raise FixtureError("Server/database health is not OK")
                    if "routers" in payload and payload["routers"] != {"apns": False, "fcm": False}:
                        raise FixtureError("Unexpected active mobile bridge")
            return {"ready": True, **public_state(value)}
        except (OSError, ValueError, FixtureError) as error:
            last_error = type(error).__name__
            time.sleep(0.25)
    raise FixtureError(f"TLS/server/database readiness failed ({last_error}); inspect owned logs")


def start(value: dict, cache: Path) -> None:
    if (WORK / "relay.json").exists() or (WORK / "run" / "relay-control.sock").exists():
        raise FixtureError("An existing relay must be stopped before starting another owner")
    built = read_json(WORK / "build.json")
    source = prepare(cache)
    if built["inputs_sha256"] != digest(HERE / "inputs.json"):
        raise FixtureError("Build inputs changed")
    if built["dockerfile_sha256"] != digest(HERE / "Dockerfile"):
        raise FixtureError("Build definition changed")
    if digest(HERE / "fresh-init.sql") != pins()["fresh_schema_sha256"]:
        raise FixtureError("Fresh-init schema changed")
    audit(cache, source)
    if any(item["State"]["Running"] for item in owned_containers(value)):
        raise FixtureError("Existing fixture services must be stopped before assigning a new relay owner")
    owned_storage(value)
    verify_config(value)
    for name in ("database", "proxy"):
        if pinned_image(pins()["images"][name]) != value[name + "_image"]:
            raise FixtureError("Pinned runtime image identity changed")
    try:
        recreate_containers(value)
    except BaseException:
        record_resources(value)
        stop_containers(value)
        raise


def stop_containers(value: dict) -> None:
    containers = owned_containers(value)
    if containers:
        # Operate on inspected IDs, not orphan/prune flags or names.
        docker("stop", "--time", "20", *[item["Id"] for item in containers])
    if any(item["State"]["Running"] for item in record_resources(value)):
        raise FixtureError("Some owned containers are still running")
    print(json.dumps({"stopped": True, "project": value["project"], "database_retained": True}))


def recreate_containers(value: dict) -> None:
    containers = owned_containers(value)
    if any(item["State"]["Running"] for item in containers):
        raise FixtureError("Refusing to remove an in-use fixture container")
    for service in ("connect", "endpoint", "database", "proxy"):
        ids = [
            item["Id"] for item in containers
            if item["Config"]["Labels"]["com.docker.compose.service"] == service
        ]
        if ids:
            docker("rm", *ids)
    # Porto resolves a shared namespace when creating, not starting, a child.
    # Start its owned network-none anchor before creating the other services.
    compose(value, "up", "--detach", "--wait", "--wait-timeout", "30", "proxy")
    compose(value, "up", "--detach", "--wait", "--wait-timeout", "90")
    record_resources(value)
    verify_network_none(value)


def stop(value: dict) -> None:
    control = WORK / "run" / "relay-control.sock"
    if control.exists():
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(5)
            connection.connect(os.path.relpath(control))
            connection.sendall(json.dumps({"action": "stop", "run_id": value["run_id"]}).encode() + b"\n")
            if connection.recv(64) != b"stopping\n":
                raise FixtureError("Relay ownership handshake failed")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            containers = record_resources(value)
            if not (WORK / "relay.json").exists() and not any(
                item["State"]["Running"] for item in containers
            ):
                print(json.dumps({"stopped": True, "relay_stopped": True, "database_retained": True}))
                return
            time.sleep(0.5)
        raise FixtureError("Owned relay/containers have not completed graceful shutdown")
    if (WORK / "relay.json").exists():
        raise FixtureError("Relay metadata exists without a control socket; inspect its exact PID/start identity")
    stop_containers(value)


def restart(value: dict) -> None:
    verify_config(value)
    owned_storage(value)
    containers = owned_containers(value)
    if {item["Config"]["Labels"]["com.docker.compose.service"] for item in containers} != SERVICES:
        raise FixtureError("Restart requires the existing four owned services")
    stop_containers(value)
    recreate_containers(value)
    ready(value)


def rotate(value: dict) -> None:
    verify_config(value)
    containers = owned_containers(value)
    if len(containers) != 4 or not all(item["State"]["Running"] for item in containers):
        raise FixtureError("Key rotation requires the four running owned services")
    path = WORK / "run" / "secrets" / "key-ring.json"
    ring = read_json(path)
    ring["keys"].insert(0, base64.urlsafe_b64encode(secrets.token_bytes(32)).decode())
    write_json(path, ring, private=True)
    render(value)
    compose(value, "up", "--detach", "--force-recreate", "connect", "endpoint")
    record_resources(value)
    ready(value)
    print(json.dumps({"rotated": True, "retained_keys": len(ring["keys"])}))


def main() -> None:
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=WORK / "cache")
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ["prepare", "audit", "build", "adopt-build", "up", "serve", "ready", "status", "stop", "restart", "rotate"]:
        commands.add_parser(command)
    init = commands.add_parser("init")
    init.add_argument("--test-certificate", action="store_true")
    init.add_argument("--cert", type=Path)
    init.add_argument("--key", type=Path)
    args = parser.parse_args()
    cache = args.cache.resolve()
    WORK.mkdir(exist_ok=True)
    if args.command == "ready":
        print(json.dumps(ready(state()), indent=2))
        return
    if args.command == "status":
        value = state()
        containers = owned_containers(value)
        print(json.dumps({
            **public_state(value),
            "containers": [
                {"id": item["Id"], "running": item["State"]["Running"],
                 "service": item["Config"]["Labels"]["com.docker.compose.service"]}
                for item in containers
            ],
        }, indent=2))
        return
    serve_value = None
    with (WORK / "lifecycle.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if args.command == "prepare":
            print(prepare(cache))
        elif args.command == "audit":
            print(json.dumps(audit(cache, prepare(cache)), indent=2))
        elif args.command == "build":
            build(cache)
        elif args.command == "init":
            initialize(args)
        else:
            value = state()
            if args.command == "adopt-build":
                if any(item["State"]["Running"] for item in owned_containers(value)):
                    raise FixtureError("Stop the fixture before adopting a new build")
                built = read_json(WORK / "build.json")
                if built["inputs_sha256"] != digest(HERE / "inputs.json"):
                    raise FixtureError("Build input pins changed")
                value["image"] = built["image"]
                render(value)
            elif args.command in {"up", "serve"}:
                start(value, cache)
                serve_value = value
            elif args.command == "ready":
                print(json.dumps(ready(value), indent=2))
            elif args.command == "status":
                record_resources(value)
                print(json.dumps({**public_state(value), "containers": value["containers"]}, indent=2))
            elif args.command == "stop":
                stop(value)
            elif args.command == "restart":
                restart(value)
            elif args.command == "rotate":
                rotate(value)
    if serve_value is not None:
        import relay
        try:
            asyncio.run(relay.serve(serve_value))
        finally:
            stop_containers(serve_value)


if __name__ == "__main__":
    try:
        main()
    except (FixtureError, OSError, ValueError) as error:
        print(f"Fixture error: {error}", file=sys.stderr)
        sys.exit(1)
