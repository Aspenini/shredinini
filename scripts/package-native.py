"""Build a native Linux archive or macOS app bundle on its target OS."""
import argparse
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def package():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    system = platform.system()
    if system not in ("Linux", "Darwin"):
        raise SystemExit("On Windows use scripts/build.ps1 -Package (or -NoAsio -Package).")
    if system == "Darwin":
        os.environ.setdefault("MACOSX_DEPLOYMENT_TARGET", "12.0")
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--release", "--locked"], cwd=ROOT, check=True)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    arch = {"aarch64": "arm64", "x86_64": "x64", "arm64": "arm64"}.get(platform.machine(), platform.machine())
    name = f"Shredini-{version}-{'macos' if system == 'Darwin' else 'linux'}-{arch}"
    destination = ROOT / "dist" / name
    destination.mkdir(parents=True, exist_ok=True)
    if system == "Darwin":
        contents = destination / "Shredini.app/Contents"
        (contents / "MacOS").mkdir(parents=True, exist_ok=True)
        executable = contents / "MacOS/shredini"
        with (contents / "Info.plist").open("wb") as file:
            plistlib.dump({"CFBundleName": "Shredini", "CFBundleDisplayName": "Shredini", "CFBundleIdentifier": "com.shredini.studio", "CFBundleExecutable": "shredini", "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version, "CFBundleVersion": version, "LSMinimumSystemVersion": "12.0", "NSHighResolutionCapable": True, "NSMicrophoneUsageDescription": "Shredini processes the guitar or instrument audio input you select while monitoring is started.", "NSPrincipalClass": "NSApplication"}, file)
    else:
        executable = destination / "shredini"
    build_target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not build_target.is_absolute():
        build_target = ROOT / build_target
    shutil.copy2(build_target / "release/shredini", executable)
    executable.chmod(executable.stat().st_mode | 0o111)
    for document in ("README.md", "VALIDATION.md", "LICENSE", "THIRD_PARTY_NOTICES.md"):
        shutil.copy2(ROOT / document, destination / document)
    shutil.copy2(ROOT / "assets/GeneralUser-GS-LICENSE.txt", destination)
    if system == "Darwin":
        resources = destination / "Shredini.app/Contents/Resources"
        resources.mkdir(exist_ok=True)
        for document in ("LICENSE", "THIRD_PARTY_NOTICES.md", "GeneralUser-GS-LICENSE.txt"):
            shutil.copy2(destination / document, resources / document)
        # Development signature, not Developer ID signing or notarization.
        subprocess.run(["codesign", "--force", "--sign", "-", str(destination / "Shredini.app")], check=True)
        archive = shutil.make_archive(str(destination), "zip", root_dir=destination)
    else:
        archive = shutil.make_archive(str(destination), "gztar", root_dir=destination.parent, base_dir=destination.name)
    print(f"Native package: {archive}")


if __name__ == "__main__":
    package()
