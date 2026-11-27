# Serves a folder (the argument, or the current one) to the test VMs and takes their logs back:
# PUT /logs/<name> writes <folder>/logs/<name>. Bound to 127.0.0.1 only; VirtualBox 7 NAT reaches
# it as 10.0.2.2 once the VM has --nat-localhostreachable1 on.
import http.server
import os
import re
import sys

ROOT = sys.argv[1] if len(sys.argv) > 1 else os.getcwd()
LOGS = os.path.join(ROOT, "logs")
os.makedirs(LOGS, exist_ok=True)


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=ROOT, **kw)

    def do_PUT(self):
        m = re.fullmatch(r"/logs/([A-Za-z0-9_.-]{1,80})", self.path)
        if not m:
            self.send_error(403)
            return
        n = int(self.headers.get("Content-Length", "0"))
        if n > 50 * 1024 * 1024:
            self.send_error(413)
            return
        with open(os.path.join(LOGS, m.group(1)), "wb") as f:
            f.write(self.rfile.read(n))
        self.send_response(201)
        self.end_headers()


http.server.ThreadingHTTPServer(("127.0.0.1", 8765), Handler).serve_forever()
