"""Serve a built Shredini web app on localhost, a secure browser context."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parent.parent


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm", ".js": "text/javascript"}

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Permissions-Policy", "microphone=(self), midi=(self), speaker-selection=(self)")
        super().end_headers()


def serve():
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=ROOT / "dist" / f"Shredini-{version}-web")
    parser.add_argument("--port", type=int, default=8080)
    args = parser.parse_args()
    directory = args.directory.resolve()
    if not (directory / "index.html").is_file() or not (directory / "pkg/shredini_bg.wasm").is_file():
        raise SystemExit("Build the web app first: python scripts/build-web.py")
    with ThreadingHTTPServer(("127.0.0.1", args.port), partial(Handler, directory=str(directory))) as server:
        print(f"Shredini: http://localhost:{args.port} (Ctrl+C to stop)", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    serve()
