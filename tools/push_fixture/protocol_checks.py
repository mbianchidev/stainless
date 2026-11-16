#!/usr/bin/env python3
"""Source-informed server interoperability checks; NOT browser/SW evidence.

All receivers, sender keys and payloads are fictional and scoped to run.json.
No endpoint, UAID, channel, authentication material or payload enters the report.
"""

from __future__ import annotations

import argparse
import base64
from collections import Counter, deque
import datetime as dt
import hashlib
import json
import logging
import os
from pathlib import Path
import re
import secrets
import socket
import ssl
import struct
import subprocess
import sys
import time
import traceback
from urllib.parse import urlsplit
import uuid

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec
import http_ece
from py_vapid import Vapid
from pywebpush import WebPusher, WebPushException, webpush
import requests
from websockets.exceptions import ConnectionClosed
from websockets.sync.client import connect

import fixture

logging.disable(logging.CRITICAL)
SENSITIVE = set()


class ContractFailure(AssertionError):
    pass


def check(condition: bool, message: str) -> None:
    if not condition:
        raise ContractFailure(message)


def b64(value: bytes) -> str:
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode()


def unb64(value: str) -> bytes:
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


def public_key(key: ec.EllipticCurvePrivateKey) -> str:
    return b64(key.public_key().public_bytes(
        serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint
    ))


class Sender:
    def __init__(self, value: dict):
        self.value = value
        self.private = ec.generate_private_key(ec.SECP256R1())
        self.public = public_key(self.private)
        SENSITIVE.add(self.public)
        self.vapid = Vapid(private_key=self.private)
        self.session = requests.Session()
        self.session.trust_env = False
        self.session.verify = value["certificate"]
        self.statuses: Counter = Counter()

    def claims(self, **overrides) -> dict:
        return {
            "sub": "mailto:fixture@example.invalid",
            "aud": self.value["publishing_origin"],
            "exp": int(time.time()) + 600,
            **overrides,
        }

    def send(self, recipient: Recipient, data: str | None = "fictional blue square",
             *, ttl: int = 120, vapid: Vapid | None = None,
             claims: dict | None = None, encoding: str = "aes128gcm") -> int:
        try:
            response = webpush(
                recipient.subscription, data=data,
                vapid_private_key=vapid or self.vapid,
                vapid_claims=claims or self.claims(),
                ttl=ttl, timeout=10, content_encoding=encoding,
                requests_session=self.session,
            )
            status = response.status_code
        except WebPushException as error:
            if error.response is None:
                raise AssertionError("Independent sender failed before receiving HTTP status") from None
            status = error.response.status_code
        self.statuses[status] += 1
        return status

    def expired(self, recipient: Recipient) -> int:
        # webpush() intentionally refreshes expired claims. Sign directly with
        # the same independent VAPID library to exercise a genuinely old token.
        headers = self.vapid.sign(self.claims(exp=int(time.time()) - 600))
        headers["TTL"] = "120"
        encoded = WebPusher(recipient.subscription).encode(b"fictional expired request")
        headers["Content-Encoding"] = "aes128gcm"
        response = self.session.post(
            recipient.subscription["endpoint"], data=encoded["body"],
            headers=headers, timeout=10, allow_redirects=False,
        )
        self.statuses[response.status_code] += 1
        return response.status_code


class Recipient:
    def __init__(self):
        self.channel = str(uuid.uuid4())
        self.private = ec.generate_private_key(ec.SECP256R1())
        self.auth = secrets.token_bytes(16)
        SENSITIVE.update((self.channel, b64(self.auth), public_key(self.private)))
        self.subscription = {
            "endpoint": "",
            "keys": {"p256dh": public_key(self.private), "auth": b64(self.auth)},
        }

    def plaintext(self, notification: dict) -> bytes:
        check(notification.get("channelID") == self.channel, "Wrong recipient channel")
        check(notification.get("headers", {}).get("encoding") == "aes128gcm",
              "Unexpected encrypted content encoding")
        return http_ece.decrypt(
            unb64(notification["data"]), private_key=self.private,
            auth_secret=self.auth, version="aes128gcm",
        )


class UA:
    def __init__(self, value: dict, uaid: str | None = None, *, channel_ids=None):
        self.value = value
        self.pending = deque()
        self.connection = connect(
            value["connection_url"],
            ssl=ssl.create_default_context(cafile=value["certificate"]),
            compression=None, proxy=None, ping_interval=None,
            open_timeout=10, close_timeout=3, max_size=65536, max_queue=64,
            user_agent_header="Stainless-fictional-fixture/1",
        )
        self.socket = self.connection.__enter__()
        hello = {"messageType": "hello", "use_webpush": True}
        if uaid:
            hello["uaid"] = uaid
        if channel_ids is not None:
            hello["channelIDs"] = channel_ids
        try:
            response = self.request(hello)
            check(response.get("status") == 200 and response.get("use_webpush") is True,
                  "Server did not accept Web Push hello")
            check(type(response["status"]) is int and isinstance(response.get("broadcasts"), dict),
                  "Unexpected hello field types")
            self.uaid = response["uaid"]
            SENSITIVE.add(self.uaid)
            check(isinstance(self.uaid, str) and uuid.UUID(self.uaid).hex == self.uaid,
                  "Hello UAID is not lowercase unhyphenated UUID text")
        except BaseException:
            self.close()
            raise

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def close(self):
        self.connection.__exit__(None, None, None)

    def send(self, value: dict):
        self.socket.send(json.dumps(value))

    def request(self, value: dict) -> dict:
        self.send(value)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            result = json.loads(self.socket.recv(timeout=max(0.01, deadline - time.monotonic())))
            if result.get("messageType") == "notification":
                self.pending.append(result)
            elif result.get("messageType") == value.get("messageType"):
                return result
            else:
                raise AssertionError("Unexpected server control response")
        raise AssertionError("Server control response deadline exceeded")

    def register(self, recipient: Recipient, sender: Sender, *, retain_endpoint=False):
        result = self.request({
            "messageType": "register", "channelID": recipient.channel, "key": sender.public,
        })
        check(result.get("status") == 200 and result.get("channelID") == recipient.channel,
              "Registration failed")
        endpoint = urlsplit(result["pushEndpoint"])
        SENSITIVE.add(result["pushEndpoint"])
        expected = urlsplit(self.value["publishing_origin"])
        check(
            endpoint.scheme == "https"
            and endpoint.netloc == expected.netloc
            and endpoint.username is None and endpoint.fragment == ""
            and endpoint.path.startswith("/wpush/"),
            "Registration returned an unexpected publishing origin",
        )
        if not retain_endpoint:
            recipient.subscription["endpoint"] = result["pushEndpoint"]

    def unregister(self, recipient: Recipient):
        result = self.request({
            "messageType": "unregister", "channelID": recipient.channel, "code": 200,
        })
        check(result.get("status") == 200 and result.get("channelID") == recipient.channel,
              "Unregister failed")

    def notification(self, timeout=10) -> dict:
        if self.pending:
            return self.pending.popleft()
        result = json.loads(self.socket.recv(timeout=timeout))
        check(result.get("messageType") == "notification", "Expected notification frame")
        return result

    def no_notification(self, timeout=0.5):
        try:
            self.notification(timeout)
        except TimeoutError:
            return
        raise AssertionError("Unexpected retained or rejected notification")

    def ack(self, notification: dict, code=100):
        self.send({
            "messageType": "ack",
            "updates": [{
                "channelID": notification["channelID"],
                "version": notification["version"], "code": code,
            }],
        })

    def barrier(self, recipient: Recipient, sender: Sender):
        self.register(recipient, sender, retain_endpoint=True)


class RawWSS:
    """Small RFC 6455 framing probe, solely for real ping/pong deadlines."""

    def __init__(self, value):
        port = urlsplit(value["connection_url"]).port
        context = ssl.create_default_context(cafile=value["certificate"])
        self.socket = context.wrap_socket(
            socket.create_connection(("127.0.0.1", port), timeout=10), server_hostname="localhost"
        )
        nonce = base64.b64encode(secrets.token_bytes(16)).decode()
        self.socket.sendall((
            f"GET / HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: Upgrade\r\n"
            f"Upgrade: websocket\r\nSec-WebSocket-Key: {nonce}\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        ).encode())
        headers = bytearray()
        while not headers.endswith(b"\r\n\r\n"):
            part = self.socket.recv(1)
            check(bool(part) and len(headers) < 8192, "Invalid WebSocket upgrade")
            headers.extend(part)
        expected = base64.b64encode(
            hashlib.sha1((nonce + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        ).decode()
        check(headers.startswith(b"HTTP/1.1 101 "), "WebSocket upgrade was rejected")
        check(expected.encode() in headers, "WebSocket upgrade accept did not match")
        self.send(1, b'{"messageType":"hello","use_webpush":true}')
        opcode, payload = self.receive(10)
        hello = json.loads(payload)
        check(opcode == 1 and hello.get("status") == 200, "Raw hello failed")
        SENSITIVE.add(hello["uaid"])

    def close(self):
        try:
            self.send(8, struct.pack("!H", 1000))
            self.receive(3)
        except (OSError, ContractFailure):
            pass
        self.socket.close()

    def send(self, opcode, payload):
        check(len(payload) < 126, "Probe frame too large")
        mask = secrets.token_bytes(4)
        encoded = bytes(item ^ mask[index % 4] for index, item in enumerate(payload))
        self.socket.sendall(bytes([0x80 | opcode, 0x80 | len(payload)]) + mask + encoded)

    def receive(self, timeout):
        self.socket.settimeout(timeout)

        def exact(count):
            output = bytearray()
            while len(output) < count:
                part = self.socket.recv(count - len(output))
                check(bool(part), "Unexpected EOF before WebSocket close frame")
                output.extend(part)
            return bytes(output)

        first, second = exact(2)
        check(not second & 0x80, "Server illegally masked its frame")
        size = second & 0x7F
        if size == 126:
            size = struct.unpack("!H", exact(2))[0]
        elif size == 127:
            size = struct.unpack("!Q", exact(8))[0]
        check(size <= 65536, "Oversized server frame")
        return first & 0x0F, exact(size)


class Checks:
    def __init__(self, value: dict):
        self.value = value
        self.sender = Sender(value)
        self.observations = {}

    def runtime_scope_and_private_admin(self):
        fixture.verify_config(self.value)
        fixture.owned_storage(self.value)
        containers = fixture.owned_containers(self.value)
        fixture.verify_network_none(self.value, containers)
        for item in containers:
            bindings = item["HostConfig"].get("PortBindings") or {}
            check(not bindings, "Isolated container unexpectedly published a host port")
        metadata = fixture.read_json(fixture.WORK / "relay.json")
        check(metadata["run_id"] == self.value["run_id"], "Loopback relay ownership changed")
        for origin in [
            self.value["connection_url"].replace("wss:", "https:").rstrip("/"),
            self.value["publishing_origin"],
        ]:
            response = self.sender.session.get(origin + "/metrics", timeout=5, allow_redirects=False)
            check(response.status_code == 404, "Private metrics were exposed through the TLS proxy")
        self.observations["isolation_evidence"] = (
            "Runtime ownership, actual loopback-only/no-route namespace, fixed loopback byte relays, "
            "no container-published ports and denied public metrics; "
            "not a packet-capture or all-time egress claim."
        )

    def tls_verification(self):
        port = urlsplit(self.value["connection_url"]).port
        for context, hostname in [
            (ssl.create_default_context(), "localhost"),
            (ssl.create_default_context(cafile=self.value["certificate"]), "wrong.example.invalid"),
        ]:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=5) as raw:
                    with context.wrap_socket(raw, server_hostname=hostname):
                        raise AssertionError("Untrusted or wrong-host TLS certificate was accepted")
            except ssl.SSLCertVerificationError:
                pass

    def encrypted_and_vapid(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            ua.register(recipient, self.sender)
            check(self.sender.send(recipient) == 201, "Valid independent VAPID sender rejected")
            notification = ua.notification()
            check(recipient.plaintext(notification) == b"fictional blue square",
                  "RFC 8291 plaintext did not match")
            ua.ack(notification)
            wrong = Vapid(private_key=ec.generate_private_key(ec.SECP256R1()))
            check(self.sender.send(recipient, vapid=wrong) in {401, 403}, "Wrong VAPID key accepted")
            check(self.sender.send(
                recipient, claims=self.sender.claims(aud="https://wrong.example.invalid")
            ) in {401, 403}, "Wrong VAPID audience accepted")
            check(self.sender.expired(recipient) in {401, 403}, "Expired VAPID assertion accepted")
            ua.no_notification()
            ua.unregister(recipient)

    def wire_shapes_and_reregistration(self):
        recipient = Recipient()
        other = Sender(self.value)
        try:
            for invalid in [uuid.UUID(recipient.channel).hex, recipient.channel.upper()]:
                SENSITIVE.add(invalid)
                with UA(self.value) as ua:
                    ua.send({"messageType": "register", "channelID": invalid, "key": self.sender.public})
                    try:
                        ua.socket.recv(timeout=3)
                        raise ContractFailure("Noncanonical register channel UUID was accepted")
                    except ConnectionClosed:
                        pass
            with UA(self.value) as ua:
                reply = ua.request({
                    "messageType": "register",
                    "channelID": recipient.channel,
                    "key": self.sender.public,
                })
                check(reply.get("status") == 200 and reply.get("channelID") == recipient.channel,
                      "Register did not preserve canonical lowercase hyphenated channel UUID")
                check(type(reply["status"]) is int and isinstance(reply.get("pushEndpoint"), str),
                      "Unexpected register response field types")
                recipient.subscription["endpoint"] = reply["pushEndpoint"]
                SENSITIVE.add(reply["pushEndpoint"])
                original = reply["pushEndpoint"]
                ua.register(recipient, self.sender, retain_endpoint=True)
                check(self.sender.send(recipient) == 201, "Same-key re-registration broke original endpoint")
                notification = ua.notification()
                check(isinstance(notification.get("version"), str), "Notification version is not a string")
                check(isinstance(notification.get("data"), str), "Encrypted data is not a string")
                check(bool(re.fullmatch(r"[A-Za-z0-9_-]+={0,2}", notification["data"])),
                      "Encrypted data is not base64url")
                check(notification.get("headers") == {"encoding": "aes128gcm"},
                      "Modern encoding header has unexpected spelling or shape")
                check(recipient.plaintext(notification) == b"fictional blue square",
                      "Canonical-channel encrypted delivery failed")
                ua.ack(notification)
                ua.register(recipient, other)
                switched = recipient.subscription["endpoint"]
                check(self.sender.send(recipient) in {401, 403}, "New endpoint did not bind its VAPID key")
                check(other.send(recipient) == 201, "Re-registration with a new VAPID key failed")
                ua.ack(ua.notification())
                recipient.subscription["endpoint"] = original
                check(other.send(recipient) in {401, 403}, "Original endpoint lost its old VAPID binding")
                check(self.sender.send(recipient) == 201,
                      "Key re-registration unexpectedly revoked the original opaque endpoint")
                ua.ack(ua.notification())
                ua.unregister(recipient)
                self.observations["wire_contract"] = {
                    "hello": {
                        "messageType": "hello", "status": "JSON integer 200",
                        "uaid": "32 lowercase hex digits, no hyphens",
                        "use_webpush": True, "broadcasts": "JSON object",
                    },
                    "register": {
                        "input_rule": "strict lowercase hyphenated UUID; simple/uppercase input closes socket",
                        "channelID": "lowercase hyphenated UUID string",
                        "status": "JSON integer 200", "pushEndpoint": "HTTPS string",
                    },
                    "notification": {
                        "channelID": "lowercase hyphenated UUID string",
                        "version": "opaque JSON string; do not parse as an integer",
                        "data": "base64url string",
                        "headers": {"encoding": "aes128gcm"},
                    },
                    "reregister_same_channel": {
                        "same_key_status": 200, "original_endpoint_remains_usable": True,
                        "different_key_status": 200,
                        "different_key_produces_different_endpoint": switched != original,
                        "old_and_new_endpoints_keep_their_own_vapid_binding": True,
                        "key_change_does_not_revoke_old_endpoint": True,
                    },
                }
        finally:
            self.sender.statuses.update(other.statuses)
            other.session.close()

    def payloadless_and_unregister(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            ua.register(recipient, self.sender)
            check(self.sender.send(recipient, None) == 201, "Payload-less send failed")
            notification = ua.notification()
            check(notification.get("channelID") == recipient.channel, "Wrong payload-less channel")
            check(not notification.get("data"), "Payload-less notification contains data")
            self.observations["payloadless_wire"] = {
                "keys": sorted(notification),
                "data_present": "data" in notification,
                "headers_present": "headers" in notification,
                "headers_empty": not bool(notification.get("headers")),
            }
            ua.ack(notification)
            ua.unregister(recipient)
            check(self.sender.send(recipient) == 410, "Unregistered endpoint did not return 410")
            ua.no_notification()

    def unacked_redelivery(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            uaid = ua.uaid
            ua.register(recipient, self.sender)
            check(self.sender.send(recipient) == 201, "Send failed")
            original = ua.notification()
            check(recipient.plaintext(original) == b"fictional blue square", "Bad first delivery")
        with UA(self.value, uaid) as ua:
            check(ua.uaid == uaid, "Unacked reconnect did not resume UAID")
            replay = ua.notification()
            check(replay["version"] == original["version"] and replay["data"] == original["data"],
                  "Unacknowledged ciphertext was not redelivered")
            ua.ack(replay)
            ua.barrier(recipient, self.sender)
        with UA(self.value, uaid) as ua:
            ua.no_notification()
            ua.unregister(recipient)

    def every_ack_consumes(self):
        for code in (100, 101, 102):
            recipient = Recipient()
            with UA(self.value) as ua:
                uaid = ua.uaid
                ua.register(recipient, self.sender)
            check(self.sender.send(recipient) == 201, "Offline stored send failed")
            with UA(self.value, uaid) as ua:
                notification = ua.notification()
                ua.ack(notification, code)
                ua.barrier(recipient, self.sender)
            with UA(self.value, uaid) as ua:
                ua.no_notification()
                ua.unregister(recipient)

    def partial_batch_blocks(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            uaid = ua.uaid
            ua.register(recipient, self.sender)
        for index in range(12):
            check(self.sender.send(recipient, f"fictional batch {index}") == 201,
                  "Stored batch send failed")
        with UA(self.value, uaid) as ua:
            first = []
            while len(first) < 12:
                try:
                    first.append(ua.notification(0.5))
                except TimeoutError:
                    break
            check(len(first) == 10, "Pinned server did not deliver its documented 10-item stored batch")
            for notification in first[:-1]:
                ua.ack(notification)
            ua.no_notification()
            ua.ack(first[-1])
            rest = [ua.notification(), ua.notification()]
            all_messages = first + rest
            check(len({item["version"] for item in all_messages}) == 12, "Batch duplicated a version")
            check(
                {recipient.plaintext(item) for item in all_messages}
                == {f"fictional batch {index}".encode() for index in range(12)},
                "Stored batch lost or corrupted a message",
            )
            for notification in rest:
                ua.ack(notification)
            ua.barrier(recipient, self.sender)
            ua.unregister(recipient)

    def offline_expiry(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            uaid = ua.uaid
            ua.register(recipient, self.sender)
        check(self.sender.send(recipient, "fictional expires", ttl=1) == 201,
              "Expiring offline send failed")
        time.sleep(2.1)
        with UA(self.value, uaid) as ua:
            check(ua.uaid == uaid, "Expiry unexpectedly reset UAID")
            ua.no_notification(1)
            ua.unregister(recipient)

    def resume_registration_and_rollback(self):
        a, b, c = Recipient(), Recipient(), Recipient()
        with UA(self.value) as ua:
            uaid = ua.uaid
            ua.register(a, self.sender)
            ua.register(b, self.sender)
            ua.unregister(a)
            ua.register(c, self.sender)
        # A/C is a synthetic state rollback, not a browser profile/OS-crypt test.
        with UA(self.value, uaid, channel_ids=[a.channel, b.channel]) as ua:
            check(ua.uaid == uaid, "Rollback could not resume UAID")
            ua.register(a, self.sender, retain_endpoint=True)
            ua.register(b, self.sender, retain_endpoint=True)
            check(self.sender.send(a) == 201, "Restored channel's original endpoint no longer works")
            ua.ack(ua.notification())
            check(self.sender.send(c) == 201, "hello.channelIDs unexpectedly deleted unknown channel")
            notification = ua.notification()
            check(notification["channelID"] == c.channel, "Unknown server-only channel not observed")
            ua.ack(notification, 102)
            ua.unregister(c)
            check(self.sender.send(c) == 410, "Unknown channel unregister did not take effect")
            ua.unregister(a)
            ua.unregister(b)

    def restart_and_key_rotation(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            uaid = ua.uaid
            ua.register(recipient, self.sender)
        for action in (fixture.restart, fixture.rotate):
            action(self.value)
            with UA(self.value, uaid) as ua:
                check(ua.uaid == uaid, "Server restart/rotation did not retain UAID")
                ua.register(recipient, self.sender, retain_endpoint=True)
                check(self.sender.send(recipient) == 201,
                      "Original endpoint broke after retained-key restart/rotation")
                notification = ua.notification()
                check(recipient.plaintext(notification) == b"fictional blue square",
                      "Encrypted delivery broke after restart/rotation")
                ua.ack(notification)
                ua.barrier(recipient, self.sender)
        with UA(self.value, uaid) as ua:
            ua.unregister(recipient)

    def fresh_init_refuses_existing_data(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            ua.register(recipient, self.sender)
            database = next(
                item for item in fixture.owned_containers(self.value)
                if item["Config"]["Labels"]["com.docker.compose.service"] == "database"
            )
            result = subprocess.run(
                [
                    "docker", "--context", "porto", "exec", "-i", database["Id"],
                    "psql", "--no-psqlrc", "--set", "ON_ERROR_STOP=1",
                    "-U", "autopush", "-d", "autopush",
                ],
                input=(fixture.HERE / "fresh-init.sql").read_text(),
                capture_output=True, text=True, timeout=15,
            )
            check(result.returncode != 0 and "Refusing to initialize a nonempty schema" in result.stderr,
                  "Fresh-init schema did not refuse an existing database")
            check(self.sender.send(recipient) == 201, "Fresh-init refusal changed an existing subscription")
            ua.ack(ua.notification())
            ua.unregister(recipient)

    def backlog_reset(self):
        recipient = Recipient()
        with UA(self.value) as ua:
            original = ua.uaid
            ua.register(recipient, self.sender)
        for _ in range(151):
            check(self.sender.send(recipient, None, ttl=600) == 201,
                  "Backlog send failed before the 150-message threshold test")
        delivered = 0
        with UA(self.value, original) as ua:
            try:
                while delivered <= 151:
                    ua.ack(ua.notification())
                    delivered += 1
            except ConnectionClosed:
                pass
        check(delivered == 150, "Pinned msg_limit did not terminate at the expected stored boundary")
        with UA(self.value, original) as ua:
            check(ua.uaid != original, "Exceeded stored backlog did not reset UAID")
            check(self.sender.send(recipient) == 410, "Reset UAID's endpoint remained usable")
            ua.register(recipient, self.sender)
            check(self.sender.send(recipient) == 201, "Re-subscribe after reset failed")
            ua.ack(ua.notification())
            ua.unregister(recipient)
        self.observations["stored_messages_delivered_before_reset"] = delivered

    def hello_and_json_heartbeat_deadlines(self):
        context = ssl.create_default_context(cafile=self.value["certificate"])
        start = time.monotonic()
        with connect(
            self.value["connection_url"], ssl=context, proxy=None, compression=None,
            ping_interval=None, close_timeout=3, max_size=65536,
        ) as ws:
            try:
                ws.recv(timeout=8)
                raise AssertionError("Missing hello was not rejected")
            except ConnectionClosed:
                check(4 <= time.monotonic() - start <= 8, "Unexpected hello timeout")
        with UA(self.value) as ua:
            ua.send({})
            check(json.loads(ua.socket.recv(timeout=3)) == {}, "JSON heartbeat did not complete")
            ua.send({})
            try:
                ua.socket.recv(timeout=3)
                raise AssertionError("Excessive JSON heartbeat was accepted")
            except ConnectionClosed:
                pass

    def real_control_ping_pong(self):
        positive = RawWSS(self.value)
        negative = RawWSS(self.value)
        start = time.monotonic()
        try:
            opcode, payload = positive.receive(315)
            check(opcode == 9, "Server did not send a control ping")
            positive.send(10, payload)
            opcode, _ = negative.receive(15)
            check(opcode == 9, "Second server control ping missing")
            ping_at = time.monotonic()
            check(295 <= ping_at - start <= 315, "Unexpected server control-ping interval")
            opcode, _ = negative.receive(8)
            check(opcode == 8, "Missing control pong did not close the socket")
            elapsed = time.monotonic() - ping_at
            check(3 <= elapsed <= 8, "Unexpected control-pong deadline")
            positive.send(1, b"{}")
            opcode, payload = positive.receive(5)
            check(opcode == 1 and json.loads(payload) == {}, "Pong-resumed socket did not remain usable")
            self.observations["control_ping_elapsed_seconds"] = round(ping_at - start, 3)
            self.observations["missing_pong_close_seconds"] = round(elapsed, 3)
        finally:
            positive.close()
            negative.close()

    def logs_exclude_credentials_and_payloads(self):
        for key in fixture.read_json(
            fixture.WORK / "run" / "secrets" / "key-ring.json"
        )["keys"]:
            SENSITIVE.add(key)
        SENSITIVE.update(fixture.read_json(
            fixture.WORK / "run" / "secrets" / "bridge-auth.json"
        )["keys"])
        SENSITIVE.add((fixture.WORK / "run" / "secrets" / "database-password").read_text().strip())
        SENSITIVE.update(("fictional blue square", "fictional batch", "fictional expires"))
        summaries = []
        for item in fixture.owned_containers(self.value):
            output = subprocess.run(
                ["docker", "--context", "porto", "logs", item["Id"]],
                capture_output=True, timeout=15, check=True,
            )
            raw = output.stdout + output.stderr
            check(len(raw) < 4 * 1024 * 1024, "Fixture logs exceeded the bounded capture budget")
            text = raw.decode(errors="replace")
            check(not any(value in text for value in SENSITIVE),
                  "A protocol identifier, credential or plaintext payload was logged")
            summaries.append({
                "service": item["Config"]["Labels"]["com.docker.compose.service"],
                "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(),
            })
        self.observations.setdefault("log_scans", []).append(summaries)

    def logs_before_rotation(self):
        self.logs_exclude_credentials_and_payloads()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timing", action="store_true", help="Also wait for real 300s/4s ping deadlines")
    parser.add_argument("--keep-running", action="store_true", help="Explicitly retain services for another consumer")
    parser.add_argument("--case", action="append", help="Run only a named contract case (repeatable)")
    parser.add_argument("--report", default="protocol-report.json", help="Report basename under owned .work")
    args = parser.parse_args()
    value = fixture.state()
    containers = fixture.owned_containers(value)
    check(len(containers) == 4 and all(item["State"]["Running"] for item in containers),
          "Start the four owned fixture services before running protocol checks")
    fixture.ready(value)
    checks = Checks(value)
    cases = [
        "runtime_scope_and_private_admin", "tls_verification",
        "wire_shapes_and_reregistration", "encrypted_and_vapid", "payloadless_and_unregister",
        "unacked_redelivery", "every_ack_consumes", "partial_batch_blocks",
        "offline_expiry", "resume_registration_and_rollback", "logs_before_rotation",
        "restart_and_key_rotation",
        "fresh_init_refuses_existing_data", "backlog_reset", "hello_and_json_heartbeat_deadlines",
    ]
    if args.timing:
        cases.append("real_control_ping_pong")
    cases.append("logs_exclude_credentials_and_payloads")
    if args.case:
        check(all(name in cases for name in args.case), "Unknown or unapproved protocol case")
        cases = args.case
    check(Path(args.report).name == args.report and args.report.endswith(".json"), "Invalid report basename")
    report_path = fixture.WORK / args.report
    report = {
        "evidence_class": "source-informed real server/raw-UA protocol; not a browser/SW/native-notification test",
        "started_at": dt.datetime.now(dt.UTC).isoformat(),
        "inputs_sha256": fixture.digest(fixture.HERE / "inputs.json"),
        "source_sha256": fixture.pins()["patched_tree_sha256"],
        "image": value["image"], "project": value["project"],
        "cases": [], "observations": checks.observations,
        "limitations": [
            "No Stainless browser, permission, service worker, notification, Keychain, installation or update was exercised.",
            "A/C rollback is synthetic UA state, not browser profile snapshot/OS-crypt evidence.",
            "Server reset was observed; browser getSubscription(null) requires the parent's native/browser tests.",
            "Compiled optional bridges are audited but inactive; no cloud/mobile feature is tested or enabled.",
        ],
    }
    try:
        for name in cases:
            start = time.monotonic()
            result = {"name": name, "status": "passed"}
            try:
                getattr(checks, name)()
            except Exception as error:
                result.update(
                    status="failed", error_type=type(error).__name__,
                    location=[
                        f"{Path(frame.filename).name}:{frame.lineno}"
                        for frame in traceback.extract_tb(error.__traceback__)[-3:]
                    ],
                )
                if isinstance(error, ContractFailure):
                    result["contract"] = str(error)
            result["seconds"] = round(time.monotonic() - start, 3)
            report["cases"].append(result)
            fixture.write_json(report_path, report)
            print(name + ": " + result["status"], flush=True)
    finally:
        checks.sender.session.close()
        report["sender_http_statuses"] = dict(checks.sender.statuses)
        if not args.keep_running:
            fixture.stop(value)
            report["cleanup"] = "Owned containers stopped; database/key ring retained for parent reuse."
        else:
            report["cleanup"] = "Explicitly retained for another consumer; fixture.py stop is required."
        report["completed_at"] = dt.datetime.now(dt.UTC).isoformat()
        fixture.write_json(report_path, report)
    return int(any(item["status"] != "passed" for item in report["cases"]))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        # Exception strings from HTTP/WebSocket libraries can contain endpoints.
        print("Fixture protocol runner failed: " + type(error).__name__, file=sys.stderr)
        sys.exit(1)
