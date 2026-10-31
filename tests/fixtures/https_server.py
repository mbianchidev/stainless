"""Loopback-only HTTPS fixture with a synthetic, self-signed certificate."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ssl


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"synthetic Stainless HTTPS fixture\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--certificate", required=True)
    parser.add_argument("--key", required=True)
    args = parser.parse_args()

    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(args.certificate, args.key)
    with ThreadingHTTPServer(("127.0.0.1", args.port), Handler) as server:
        server.socket = context.wrap_socket(server.socket, server_side=True)
        server.serve_forever()


if __name__ == "__main__":
    main()
