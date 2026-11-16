#!/usr/bin/env python3
"""Independent sender for the explicit owned native Autopush browser fixture."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parents[1]


def read_json(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 65536:
        raise ValueError(f"Expected a bounded regular fixture file: {path.name}")
    return json.loads(path.read_text())


def write_json(path, value):
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        json.dump(value, stream, indent=2)
        stream.write("\n")
    try:
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def origin(value, scheme):
    parsed = urlsplit(value)
    if (
        parsed.scheme != scheme or parsed.hostname != "localhost" or
        parsed.port is None or parsed.username or parsed.password or parsed.fragment
    ):
        raise ValueError("Only the owned loopback TLS fixture is supported")
    return parsed.scheme, parsed.hostname, parsed.port


def run(args):
    from cryptography.hazmat.primitives import serialization
    from py_vapid import Vapid02
    from pywebpush import WebPushException, webpush
    import requests

    fixture = read_json(args.fixture_info)
    connection = fixture["connection_url"]
    publisher = fixture["publishing_origin"]
    origin(connection, "wss")
    publishing_origin = origin(publisher, "https")
    certificate = Path(fixture["tls_test_root"])
    if certificate.is_symlink() or not certificate.is_file():
        raise ValueError("Missing regular fixture certificate")
    certificate_hash = hashlib.sha256(certificate.read_bytes()).hexdigest()
    if certificate_hash != fixture["tls_test_root_sha256"]:
        raise ValueError("Fixture TLS certificate differs")
    if (
        not args.workdir.is_absolute() or args.workdir.is_symlink() or
        ".." in args.workdir.parts or args.workdir.resolve().is_relative_to(ROOT)
    ):
        raise ValueError("Use a new private absolute fixture directory outside the checkout")
    args.workdir.mkdir(mode=0o700)
    subprocess.run(
        [sys.executable, str(ROOT / "tools/push_fixture/fixture.py"), "ready"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=60,
    )
    vapid = Vapid02()
    vapid.generate_keys()
    private = args.workdir / "vapid-private.pem"
    vapid.save_key(str(private))
    public = base64.urlsafe_b64encode(vapid.public_key.public_bytes(
        serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint
    )).decode().rstrip("=")
    config = {
        "connection_url": connection, "publishing_origin": publisher,
        "certificate_file": str(certificate),
        "certificate_sha256": certificate_hash,
        "coordination_dir": str(args.workdir),
        "vapid_public": public,
    }
    write_json(args.workdir / "config.json", config)
    report = {
        "scope": "Real patched Autopush/Caddy/Postgres plus independent VAPID sender and actual native test",
        "certificate_sha256": certificate_hash,
        "tls_verification_enabled": True,
        "statuses": {},
        "passed": False,
    }
    deadline = time.monotonic() + 180

    def wait_phase(expected):
        while time.monotonic() < deadline:
            path = args.workdir / "native.json"
            if path.exists():
                state = read_json(path)
                if state.get("phase") == expected:
                    return state
            time.sleep(0.05)
        raise TimeoutError(f"Native fixture did not reach phase: {expected}")

    class FixtureSession(requests.Session):
        def request(self, method, url, **kwargs):
            if origin(url, "https") != publishing_origin:
                raise ValueError("Sender destination escaped the owned publisher")
            kwargs["allow_redirects"] = False
            kwargs["verify"] = str(certificate)
            try:
                return super().request(method, url, **kwargs)
            except requests.RequestException as error:
                raise RuntimeError(
                    f"Fixture HTTPS request failed ({type(error).__name__})"
                ) from None

    print("Independent sender ready; native config.json is private and complete", flush=True)
    with FixtureSession() as session:
        session.trust_env = False
        state = wait_phase("subscribed")
        subscription = state["subscription"]
        if origin(subscription["endpoint"], "https") != publishing_origin:
            raise ValueError("Native subscription has a foreign publishing origin")

        def send(data, key):
            try:
                response = webpush(
                    subscription_info=subscription, data=data,
                    vapid_private_key=key,
                    vapid_claims={"sub": "mailto:fixture@invalid.example.test"},
                    content_encoding="aes128gcm", ttl=60, timeout=15,
                    requests_session=session,
                )
                return response.status_code
            except WebPushException as error:
                if error.response is None:
                    raise RuntimeError("Independent sender failed before receiving HTTP status") from None
                return error.response.status_code

        wrong = Vapid02()
        wrong.generate_keys()
        report["statuses"]["wrong_vapid"] = send("Fictional unauthorized parcel", wrong)
        if report["statuses"]["wrong_vapid"] not in (401, 403):
            raise ValueError("Publisher did not reject a different VAPID key")
        report["statuses"]["encrypted"] = send("Fictional encrypted parcel", vapid)
        report["statuses"]["payloadless"] = send(None, vapid)
        if any(report["statuses"][name] != 201 for name in ("encrypted", "payloadless")):
            raise ValueError("Publisher did not accept both valid notification forms")
        wait_phase("received-initial")
        restart = subprocess.run(
            [sys.executable, str(ROOT / "tools/push_fixture/fixture.py"), "restart"],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=90,
        )
        (args.workdir / "restart.log").write_text(restart.stdout)
        if restart.returncode:
            raise RuntimeError(f"Owned fixture restart failed with exit {restart.returncode}")
        report["statuses"]["after_restart"] = send("Fictional parcel after restart", vapid)
        if report["statuses"]["after_restart"] != 201:
            raise ValueError("Original native endpoint failed after retained-state restart")
        wait_phase("unsubscribed")
        report["statuses"]["after_unsubscribe"] = send("Fictional expired endpoint", vapid)
        if report["statuses"]["after_unsubscribe"] not in (404, 410):
            raise ValueError("Publisher retained the unregistered native channel")
    report["passed"] = True
    write_json(args.workdir / "sender.json", report)
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture-info", type=Path, required=True)
    parser.add_argument("--workdir", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        run(args)
    except (OSError, ValueError, RuntimeError, TimeoutError, KeyError,
            ImportError, subprocess.SubprocessError) as error:
        # HTTP exceptions may contain endpoint bearer tokens; never print them.
        if isinstance(error, subprocess.CalledProcessError):
            message = f"Owned fixture command exited {error.returncode}"
        else:
            message = str(error)
        print(f"Native Push sender failed: {message}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
