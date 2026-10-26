"""Build the shared Rust engine plus a self-contained static browser app."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def build():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bindgen", help="Path to a matching wasm-bindgen executable")
    args = parser.parse_args()
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    version = next(p["version"] for p in lock["package"] if p["name"] == "wasm-bindgen")
    local_tool = ROOT / "target" / "tools" / "bin" / ("wasm-bindgen.exe" if os.name == "nt" else "wasm-bindgen")
    tool = args.bindgen or shutil.which("wasm-bindgen") or (str(local_tool) if local_tool.exists() else None)
    if not tool:
        raise SystemExit(f"Install the web tool first:\n  rustup target add wasm32-unknown-unknown\n  cargo install wasm-bindgen-cli --version {version} --locked --root target/tools")
    actual = subprocess.check_output([tool, "--version"], text=True).strip().split()[-1]
    if actual != version:
        raise SystemExit(f"wasm-bindgen {actual} does not match Cargo.lock ({version}). Install the matching version.")
    subprocess.run(["cargo", "build", "--release", "--locked", "--lib", "--target", "wasm32-unknown-unknown"], cwd=ROOT, check=True)
    output = ROOT / "dist" / f"Shredini-{manifest['package']['version']}-web"
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run([tool, str(ROOT / "target/wasm32-unknown-unknown/release/shredini_core.wasm"), "--target", "web", "--out-dir", str(output / "pkg"), "--out-name", "shredini"], check=True)
    for name in ("index.html", "studio.css", "studio.js", "state.js", "worklet.js", "worklet-utf8.js", "decoder.js", "validate-font.js"):
        shutil.copy2(ROOT / "web" / name, output / name)
    (output / "assets").mkdir(exist_ok=True)
    for name in ("GeneralUser-GS.sf2", "GeneralUser-GS-LICENSE.txt"):
        shutil.copy2(ROOT / "assets" / name, output / "assets" / name)
    for name in ("LICENSE", "README.md", "THIRD_PARTY_NOTICES.md", "VALIDATION.md"):
        shutil.copy2(ROOT / name, output / name)
    shutil.make_archive(str(output), "zip", root_dir=output)
    print(f"Web build: {output}")
    print(f"Preview: python scripts/serve-web.py --directory \"{output}\"")


if __name__ == "__main__":
    build()
