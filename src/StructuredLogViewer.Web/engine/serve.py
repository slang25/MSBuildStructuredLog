#!/usr/bin/env python3
"""Tiny static server for the published engine (application/wasm MIME, no caching)."""
import http.server, sys, functools
root = sys.argv[1]; port = int(sys.argv[2]) if len(sys.argv) > 2 else 8940
class H(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      '.wasm': 'application/wasm', '.js': 'text/javascript', '.mjs': 'text/javascript',
                      '.json': 'application/json', '.binlog': 'application/octet-stream'}
    def end_headers(self):
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()
    def log_message(self, fmt, *args): pass
http.server.ThreadingHTTPServer(('127.0.0.1', port), functools.partial(H, directory=root)).serve_forever()
