#!/usr/bin/env python3
"""Tiny static server for the built app (correct MIME for .wasm, no caching). Not part of the interop path."""
import http.server, os, sys, functools
root = sys.argv[1]; port = int(sys.argv[2]) if len(sys.argv) > 2 else 8765
class H(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      '.wasm': 'application/wasm', '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json'}
    def end_headers(self):
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        super().end_headers()
    def log_message(self, fmt, *args): pass
http.server.ThreadingHTTPServer(('127.0.0.1', port), functools.partial(H, directory=root)).serve_forever()
