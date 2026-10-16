#!/usr/bin/env python3
"""Build the per-user Windows installer, Herdr-VERSION-TARGET-setup.exe.

  install-iscc DESTINATION_DIR
      Install the pinned Inno Setup compiler into DESTINATION_DIR after checking its SHA-256, and print the path of ISCC.exe.
  build VERSION TARGET ZIP OUTPUT_DIR ISCC
      Compile herdr-gpui.iss over the tree of the release ZIP that
      package-windows.py wrote, so the installer holds exactly the zip's files,
      and print the installer's path.
  smoke INSTALLER
      On Windows: install silently into a scratch directory with the PATH
      task, check the files, their subsystems, the console copy's output,
      and the user PATH, then uninstall and check both are gone again. Changes
      this user's PATH while it runs: for CI runners only.

Pure standard library, like package-windows.py, for the Windows runner.
"""
import argparse
import hashlib
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/release/herdr-gpui.iss"
# Inno Setup's own architecture identifiers for each release target.
ARCHITECTURES = {"x86_64-pc-windows-msvc": "x64compatible", "aarch64-pc-windows-msvc": "arm64"}
# The digest was taken from the release asset when the pin was chosen; bump
# both together. Inno Setup's x86 compiler runs on ARM64 Windows too.
INNO_VERSION = "6.7.3"
INNO_URL = ("https://github.com/jrsoftware/issrc/releases/download/"
            f"is-{INNO_VERSION.replace('.', '_')}/innosetup-{INNO_VERSION}.exe")
INNO_SHA256 = "9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732"


def install_iscc(destination):
    destination = destination.resolve()
    with tempfile.TemporaryDirectory() as temp:
        setup = Path(temp) / f"innosetup-{INNO_VERSION}.exe"
        with urllib.request.urlopen(INNO_URL, timeout=120) as response:
            setup.write_bytes(response.read())
        actual = hashlib.sha256(setup.read_bytes()).hexdigest()
        if actual != INNO_SHA256:
            raise ValueError(f"Inno Setup installer digest mismatch: {actual}")
        subprocess.run([str(setup), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-",
                        f"/DIR={destination}"], check=True, timeout=600)
    iscc = destination / "ISCC.exe"
    if not iscc.is_file():
        raise ValueError(f"Inno Setup did not install {iscc}")
    return iscc


def iscc_arguments(version, target, source, output_dir):
    """The compiler's command line after ISCC itself."""
    if not re.fullmatch(r"[1-9][0-9]{7}\.[1-9][0-9]*", version):
        raise ValueError("Version must be YYYYMMDD.COUNTER (no v prefix, counter from 1, no leading zeros)")
    if target not in ARCHITECTURES:
        raise ValueError("Unsupported Windows target")
    return ["/Q", f"/DVersion={version}", f"/DTarget={target}",
            f"/DArchitecture={ARCHITECTURES[target]}", f"/DSourceDir={source}",
            f"/DOutputDir={output_dir}", str(SCRIPT)]


def build(version, target, archive, output_dir, iscc):
    arguments = iscc_arguments(version, target, "", "")
    if not archive.is_file() or not output_dir.is_dir():
        raise ValueError("Release zip and existing output directory required")
    output_dir = output_dir.resolve()
    output = output_dir / f"Herdr-{version}-{target}-setup.exe"
    if output.exists() or output.is_symlink():
        raise ValueError(f"Output already exists: {output}")
    root = f"Herdr-{version}-{target}"
    with tempfile.TemporaryDirectory() as temp:
        with zipfile.ZipFile(archive) as release:
            names = release.namelist()
            if any(not name.startswith(root + "/") or ".." in name.split("/") for name in names):
                raise ValueError("Release zip has entries outside its root")
            release.extractall(temp)
        source = Path(temp) / root
        for required in ("herdr-gpui.exe", "bin/herdr-gpui.exe"):
            if not (source / required).is_file():
                raise ValueError(f"Release zip has no {required}")
        arguments = iscc_arguments(version, target, source, output_dir)
        subprocess.run([str(iscc), *arguments], check=True, timeout=1800)
    if not output.is_file() or not output.stat().st_size:
        raise ValueError(f"Inno Setup did not write {output}")
    return output


def subsystem(path):
    data = path.read_bytes()
    optional = struct.unpack_from("<I", data, 0x3C)[0] + 24
    return struct.unpack_from("<H", data, optional + 68)[0]


def user_path_entries():
    import winreg  # Windows only, like the rest of the smoke test.
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        try:
            value = winreg.QueryValueEx(key, "Path")[0]
        except FileNotFoundError:
            return []
    return [entry.rstrip("\\").lower() for entry in value.split(";") if entry]


def smoke(installer):
    with tempfile.TemporaryDirectory(prefix="herdr-installer-smoke-") as temp:
        app = Path(temp) / "Herdr UI"
        subprocess.run([str(installer), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-",
                        "/TASKS=addtopath", f"/DIR={app}", f"/LOG={Path(temp) / 'setup.log'}"],
                       check=True, timeout=600)
        gui, cli = app / "herdr-gpui.exe", app / "bin" / "herdr-gpui.exe"
        for path in (gui, cli, app / "licenses" / "THIRD-PARTY-NOTICES.txt"):
            if not path.is_file():
                raise ValueError(f"Installer did not install {path}")
        # 2 is the GUI subsystem, 3 the console one (package-windows.py).
        if (subsystem(gui), subsystem(cli)) != (2, 3):
            raise ValueError("Installed executables have the wrong subsystems")
        info = subprocess.run([str(cli), "--build-info"], capture_output=True, text=True,
                              check=True, timeout=60).stdout
        # Printed through a pipe by the console copy, as an agent's shell reads it.
        if not info.startswith("worktree="):
            raise ValueError(f"Installed console copy printed no build info:\n{info}")
        bin_entry = str(app / "bin").lower()
        if bin_entry not in user_path_entries():
            raise ValueError("Installer did not add bin to the user PATH")
        subprocess.run([str(app / "unins000.exe"), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"],
                       check=True, timeout=600)
        # The uninstaller hands off to a copy of itself and returns early.
        deadline = time.monotonic() + 120
        while gui.exists() or bin_entry in user_path_entries():
            if time.monotonic() > deadline:
                raise ValueError("Uninstaller left the app or its PATH entry behind")
            time.sleep(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    install = commands.add_parser("install-iscc")
    install.add_argument("destination", type=Path)
    compile_ = commands.add_parser("build")
    compile_.add_argument("version")
    compile_.add_argument("target")
    compile_.add_argument("archive", type=Path)
    compile_.add_argument("output_dir", type=Path)
    compile_.add_argument("iscc", type=Path)
    check = commands.add_parser("smoke")
    check.add_argument("installer", type=Path)
    args = parser.parse_args()
    if args.command == "install-iscc":
        print(install_iscc(args.destination))
    elif args.command == "build":
        print(build(args.version, args.target, args.archive, args.output_dir, args.iscc))
    else:
        smoke(args.installer)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
