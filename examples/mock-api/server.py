import json, os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
SERVICE = os.getenv("SERVICE_NAME", "unknown")
class Handler(BaseHTTPRequestHandler):
    def _send(self, status, payload):
        data=json.dumps(payload).encode(); self.send_response(status); self.send_header("content-type","application/json"); self.send_header("content-length",str(len(data))); self.end_headers(); self.wfile.write(data)
    def do_GET(self): self._send(200,{"status":"ok","service":SERVICE}) if self.path=="/healthz" else self._send(404,{"error":"not found"})
    def do_POST(self):
        if self.headers.get("x-force-failure") == "1": return self._send(503,{"service":SERVICE,"error":"forced failure"})
        n=int(self.headers.get("content-length","0")); body=self.rfile.read(n)
        try: body=json.loads(body or b"{}")
        except Exception: body={"raw":body.decode(errors="replace")}
        self._send(200,{"service":SERVICE,"body":body})
    def log_message(self,*args): pass
ThreadingHTTPServer(("0.0.0.0",8080),Handler).serve_forever()
