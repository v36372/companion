#!/usr/bin/env python3
"""Loopback-only OpenRouter SSE fixture for --features test-mock-provider.

No credentials or third-party Python packages. Never use this as a proxy.
"""
import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Mock(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/api/v1/chat/completions":
            self.send_error(404)
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert self.headers["Authorization"] == "Bearer test-openrouter-key"
        assert "BEGIN SELECTED PDF QUOTE" in json.dumps(body)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        reply = (
            "The selected passage greets the reader: Hello companion. "
            "It is a short fixture for exploring this PDF together. "
            "Your quote came from page 1, and the occurrence identifies "
            "the exact matching passage. Asking does not create a highlight. "
            "This response is streamed from a local test server, without an API key."
        )
        try:
            for word in reply.split():
                chunk = {"id": "local-smoke", "object": "chat.completion.chunk", "choices": [
                    {"index": 0, "delta": {"content": word + " "}, "finish_reason": None}]}
                self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
                self.wfile.flush()
                time.sleep(0.15)
            self.wfile.write(b'data: {"id":"local-smoke","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18596)
    args = parser.parse_args()
    print(f"Local mock: http://127.0.0.1:{args.port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Mock).serve_forever()
