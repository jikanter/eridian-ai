#!/usr/bin/env python3
"""Tiny HTTP server used by tests/integration/extensions.sh.

Usage:
    extensions-capture-server.py <port> <capture_file>

Accepts POST to /v1/chat/completions, writes the JSON body to <capture_file>,
and responds with a minimal OpenAI-compatible non-streaming completion. Also
answers GET /health with 200 so the test harness can poll for readiness.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer


def build_handler(capture_path):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args, **kwargs):
            pass  # silence access log

        def do_GET(self):
            if self.path == "/health":
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.end_headers()
                self.wfile.write(b"ok")
            else:
                self.send_response(404)
                self.end_headers()

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length) if length else b"{}"
            try:
                parsed = json.loads(raw.decode("utf-8"))
            except Exception:
                parsed = {"_raw": raw.decode("utf-8", "replace")}
            with open(capture_path, "w") as f:
                json.dump(parsed, f)

            response = {
                "id": "chatcmpl-capture",
                "object": "chat.completion",
                "created": 0,
                "model": parsed.get("model", "test-model"),
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "ok"},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2,
                },
            }
            body = json.dumps(response).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return Handler


def main():
    if len(sys.argv) != 3:
        print("usage: extensions-capture-server.py <port> <capture_file>", file=sys.stderr)
        sys.exit(2)
    port = int(sys.argv[1])
    capture_path = sys.argv[2]
    server = HTTPServer(("127.0.0.1", port), build_handler(capture_path))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
