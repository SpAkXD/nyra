"""Static file server for the web A/B test, like a production nginx: gzip for text, no caching, no extra headers
(a CSP of either side comes from the page's own <meta>; Nyra's `_headers` file is not applied).

    python bench/web/serve.py DIR PORT
"""
import gzip
import http.server
import mimetypes
import os
import sys

ROOT = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2])
TEXT = ("text/", "application/javascript", "application/json", "image/svg+xml", "application/xml")
mimetypes.add_type("application/javascript", ".js")
mimetypes.add_type("image/webp", ".webp")
mimetypes.add_type("image/avif", ".avif")


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        path = self.path.split("?")[0].split("#")[0]
        f = os.path.normpath(os.path.join(ROOT, path.lstrip("/")))
        if not (f == ROOT or f.startswith(ROOT + os.sep)):
            self.send_error(403)
            return
        if os.path.isdir(f):
            if not path.endswith("/"):
                self.send_response(301)
                self.send_header("Location", path + "/")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            f = os.path.join(f, "index.html")
        if not os.path.isfile(f):
            self.send_error(404)
            return
        with open(f, "rb") as fh:
            data = fh.read()
        ctype = mimetypes.guess_type(f)[0] or "application/octet-stream"
        self.send_response(200)
        self.send_header("Content-Type", ctype + ("; charset=utf-8" if ctype.startswith("text/") or ctype == "application/javascript" else ""))
        self.send_header("Cache-Control", "no-store")
        if ctype.startswith(TEXT) and "gzip" in self.headers.get("Accept-Encoding", ""):
            data = gzip.compress(data, 6)
            self.send_header("Content-Encoding", "gzip")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
