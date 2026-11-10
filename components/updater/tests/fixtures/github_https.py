"""Loopback-only HTTPS GitHub protocol fixture; never uses real credentials."""

import argparse
import hashlib
import json
import socket
import ssl
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


ROOT = "/repos/mbianchidev/stainless/releases"
API = "https://api.github.com" + ROOT
AUTHORIZATION = "Bearer synthetic-update-authorization"
MAX_METADATA_BYTES = 1024 * 1024
PAYLOAD = b"abc"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        self.request.settimeout(5)
        super().setup()

    def do_CONNECT(self):
        if self.path not in (
            "api.github.com:443",
            "release-assets.githubusercontent.com:443",
        ):
            self.send_error(502, "Fixture refuses external destinations")
            return
        self.send_response(200, "Connection established")
        self.end_headers()
        self.wfile.flush()
        self.connection = self.server.tls.wrap_socket(
            self.connection, server_side=True
        )
        self.rfile = self.connection.makefile("rb")
        self.wfile = self.connection.makefile("wb", buffering=0)
        self.close_connection = False

    def reply(self, body, status=200, declared_size=None, location=None):
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header(
            "Content-Length", str(len(body) if declared_size is None else declared_size)
        )
        self.send_header("Connection", "close")
        if location is not None:
            self.send_header("Location", location)
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()
        self.close_connection = True

    def feed(self):
        scenario = self.server.scenario
        return {
            "schema_version": 1,
            "product": "stainless",
            "release_tag": "stainless-preview-v0.1.0-preview.2",
            "release": {
                "version": "0.1.0-preview.2",
                "build": 1 if scenario == "stale_build" else 2,
                "channel": "preview",
                "profile_schema": 2 if scenario == "wrong_schema" else 1,
            },
            "os": "macos",
            "arch": "x86_64" if scenario == "wrong_target" else "arm64",
            "minimum_macos": "13.0",
            "chromium_revision": "a" * 40,
            "chromium_version": "156.0.8000.1",
            "asset": {
                "name": "Stainless-0.1.0-preview.2-macos-arm64.zip",
                "size": len(PAYLOAD),
                "sha256": (
                    "0" * 64
                    if scenario == "bad_hash"
                    else hashlib.sha256(PAYLOAD).hexdigest()
                ),
            },
        }

    def do_GET(self):
        scenario = self.server.scenario
        authorization = self.headers.get("Authorization")
        if self.headers.get("Host") == "release-assets.githubusercontent.com":
            if authorization is not None:
                self.reply(b"Credential was forwarded to the CDN", 400)
            else:
                self.reply(PAYLOAD)
            return
        if authorization != AUTHORIZATION:
            self.reply(b"Synthetic authorization required", 401)
            return
        if scenario == "offline":
            self.connection.shutdown(socket.SHUT_RDWR)
            self.close_connection = True
            return
        if scenario in ("denied_401", "denied_403"):
            self.reply(
                b"Synthetic denial reflecting " + authorization.encode(),
                int(scenario[-3:]),
            )
            return
        path = urlsplit(self.path).path
        feed = self.feed()
        metadata = b"{" if scenario == "bad_feed" else json.dumps(feed).encode()
        if path == ROOT:
            release = {
                "tag_name": feed["release_tag"],
                "draft": False,
                "prerelease": True,
                "assets": [
                    {
                        "name": "stainless-preview.json",
                        "url": API + "/assets/1",
                        "size": len(metadata),
                    },
                    {
                        "name": feed["asset"]["name"],
                        "url": API + "/assets/2",
                        "size": len(PAYLOAD),
                    },
                ],
            }
            body = json.dumps([release]).encode()
            if scenario == "bad_json":
                body = b"{"
            elif scenario.startswith("metadata_"):
                limit = MAX_METADATA_BYTES + (scenario == "metadata_over_limit")
                body += b" " * (limit - len(body))
            self.reply(body)
        elif path == ROOT + "/assets/1":
            self.reply(metadata)
        elif path == ROOT + "/assets/2":
            if scenario == "redirect":
                self.reply(
                    b"", 302,
                    location="https://release-assets.githubusercontent.com/fixture.zip",
                )
            elif scenario == "untrusted_redirect":
                self.reply(b"", 302, location="https://untrusted.invalid/fixture.zip")
            elif scenario == "downgrade_redirect":
                self.reply(
                    b"", 302,
                    location="http://release-assets.githubusercontent.com/fixture.zip",
                )
            elif scenario == "excessive_redirects":
                self.reply(b"", 302, location=API + "/assets/2")
            elif scenario == "truncated":
                self.reply(PAYLOAD[:-1], declared_size=len(PAYLOAD))
            elif scenario == "oversized":
                self.reply(PAYLOAD + b"x")
            else:
                self.reply(PAYLOAD)
        else:
            self.reply(b"Unknown synthetic endpoint", 404)


parser = argparse.ArgumentParser()
parser.add_argument("--scenario", required=True)
args = parser.parse_args()
server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.daemon_threads = True
server.scenario = args.scenario
server.tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
server.tls.load_cert_chain("server.pem", "server-key.pem")
Path("port.tmp").write_text(str(server.server_port), encoding="ascii")
Path("port.tmp").replace("port")
server.serve_forever()
