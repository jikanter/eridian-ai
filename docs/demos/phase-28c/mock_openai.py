#!/usr/bin/env python3
"""Tiny OpenAI-compatible echo server.

Returns the assistant message as: ``ECHO[<role>]: <user-content>``
so every step's user prompt is visible in the next step's prev-output.
The role prefix records the role/agent label injected by the test (we set
the system message inside the macro). Streaming is supported because aichat
defaults to SSE for chat completions.
"""
from __future__ import annotations

import json
import sys
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args, **kwargs):  # silence default access log
        return

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length).decode("utf-8", errors="replace")
        try:
            payload = json.loads(body)
        except json.JSONDecodeError:
            self.send_response(400)
            self.end_headers()
            return

        messages = payload.get("messages", [])
        last_user = next(
            (m.get("content", "") for m in reversed(messages) if m.get("role") == "user"),
            "",
        )
        if isinstance(last_user, list):
            last_user = "".join(p.get("text", "") for p in last_user if isinstance(p, dict))
        system_msg = next(
            (m.get("content", "") for m in messages if m.get("role") == "system"),
            "",
        )
        if isinstance(system_msg, list):
            system_msg = "".join(p.get("text", "") for p in system_msg if isinstance(p, dict))
        tag = system_msg.strip() or "no-role"
        reply = f"ECHO[{tag}]: {last_user}"

        stream = bool(payload.get("stream"))
        if stream:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()
            chunk_id = f"chatcmpl-{uuid.uuid4().hex[:8]}"
            first = {
                "id": chunk_id,
                "object": "chat.completion.chunk",
                "model": payload.get("model", "mock-model"),
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": reply}}],
            }
            done = {
                "id": chunk_id,
                "object": "chat.completion.chunk",
                "model": payload.get("model", "mock-model"),
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            }
            self.wfile.write(b"data: " + json.dumps(first).encode() + b"\n\n")
            self.wfile.write(b"data: " + json.dumps(done).encode() + b"\n\n")
            self.wfile.write(b"data: [DONE]\n\n")
            return

        response = {
            "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
            "object": "chat.completion",
            "model": payload.get("model", "mock-model"),
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": reply},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        }
        data = json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18790
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"mock-openai listening on 127.0.0.1:{port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
