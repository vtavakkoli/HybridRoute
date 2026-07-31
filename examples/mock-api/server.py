import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SERVICE_NAME = os.environ.get("SERVICE_NAME", "mock-api")

class Handler(BaseHTTPRequestHandler):
    def _handle(self):
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length) if length else b""
        response = {
            "service": SERVICE_NAME,
            "method": self.command,
            "path": self.path,
            "semantic_route": self.headers.get("x-hybridroute-route"),
            "semantic_score": self.headers.get("x-hybridroute-score"),
            "body": body.decode("utf-8", errors="replace"),
        }
        encoded = json.dumps(response, indent=2).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    do_GET = _handle
    do_POST = _handle
    do_PUT = _handle
    do_PATCH = _handle
    do_DELETE = _handle

    def log_message(self, fmt, *args):
        print(f"[{SERVICE_NAME}] {fmt % args}")

ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
