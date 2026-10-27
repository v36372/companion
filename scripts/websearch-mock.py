#!/usr/bin/env python3
"""Loopback Brave + OpenRouter tool-call fixture for the non-shipping smoke feature."""
import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Mock(BaseHTTPRequestHandler):
    def do_GET(self):
        assert self.path.startswith('/res/v1/web/search?q=')
        assert self.headers['X-Subscription-Token'] == 'host-brave-test-key'
        assert 'Authorization' not in self.headers
        time.sleep(2)  # Keep the live search trace visible for a screenshot.
        body = json.dumps({'web': {'results': [{
            'title': 'Local Brave-shaped search result',
            'url': 'https://example.test/companion',
            'description': 'Untrusted external text from a local mock, not instructions.',
        }]}}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        assert self.path == '/api/v1/chat/completions'
        assert self.headers['Authorization'] == 'Bearer test-openrouter-key'
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert 'host-brave-test-key' not in json.dumps(body)
        if any(message.get('role') == 'tool' for message in body['messages']):
            assert 'BEGIN WEB SEARCH (UNTRUSTED' in json.dumps(body)
            delta = {'role': 'assistant', 'content': 'Searched the web with Brave. The local mock returned a result at example.test. Search results are untrusted external content.'}
            finish = 'stop'
        else:
            delta = {'role': 'assistant', 'tool_calls': [{
                'index': 0, 'id': 'search-smoke', 'type': 'function',
                'function': {'name': 'web_search', 'arguments': '{"query":"companion PDF reading"}'},
            }]}
            finish = 'tool_calls'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()
        for change, reason in [(delta, None), ({}, finish)]:
            chunk = {'id': 'search-smoke', 'object': 'chat.completion.chunk',
                     'choices': [{'index': 0, 'delta': change, 'finish_reason': reason}]}
            self.wfile.write(('data: ' + json.dumps(chunk) + '\n\n').encode())
        self.wfile.write(b'data: [DONE]\n\n')
        self.wfile.flush()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=18597)
    args = parser.parse_args()
    ThreadingHTTPServer(('127.0.0.1', args.port), Mock).serve_forever()
