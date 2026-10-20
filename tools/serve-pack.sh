#!/bin/sh
# Shares a lily-pack.tar on the local network (with resumable downloads) and prints the address
# to enter in Lily's setup on the iPad or Android tablet. Stop with Ctrl+C when it says SETUP
# COMPLETE. An Android emulator on this computer reaches it at http://10.0.2.2:<port>/.
#
#   tools/serve-pack.sh [lily-pack.tar] [port]
set -eu
pack=${1:-lily-pack.tar}
port=${2:-8000}
[ -f "$pack" ] || { echo "error: $pack not found (make it with tools/make-pack.sh)" >&2; exit 1; }
ip=$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null \
    || hostname -I 2>/dev/null | cut -d' ' -f1 || true)
echo "On the tablet, enter:  http://${ip:-YOUR-COMPUTER-IP}:$port/$(basename "$pack")"
exec python3 -I - "$pack" "$port" <<'PY'
# A file server for one file, with HTTP Range (http.server's has none, and resuming matters for
# 7 GB over Wi-Fi).
import http.server, os, re, sys

path, port = sys.argv[1], int(sys.argv[2])
name = "/" + os.path.basename(path)

class Handler(http.server.BaseHTTPRequestHandler):
    def do_HEAD(self):
        self.send(head_only=True)

    def do_GET(self):
        self.send(head_only=False)

    def send(self, head_only):
        if self.path.split("?")[0] != name:
            self.send_error(404)
            return
        size = os.path.getsize(path)
        start, end = 0, size - 1
        m = re.fullmatch(r"bytes=(\d+)-(\d*)", self.headers.get("Range", ""))
        if m:
            start = int(m.group(1))
            end = int(m.group(2)) if m.group(2) else size - 1
            if start >= size:
                self.send_error(416)
                return
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        self.send_header("Content-Type", "application/x-tar")
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        if head_only:
            return
        with open(path, "rb") as f:
            f.seek(start)
            left = end - start + 1
            while left > 0:
                chunk = f.read(min(1 << 20, left))
                if not chunk:
                    break
                try:
                    self.wfile.write(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    return
                left -= len(chunk)

http.server.ThreadingHTTPServer(("", port), Handler).serve_forever()
PY
