#!/usr/bin/env python3
"""Build the per-user Windows installer, Herdr-VERSION-TARGET-setup.exe.

  install-iscc DESTINATION_DIR
      Install the pinned Inno Setup compiler into DESTINATION_DIR after checking its SHA-256, and print the path of ISCC.exe.
  build [--sign] VERSION TARGET ZIP OUTPUT_DIR ISCC
      Compile herdr-gpui.iss over the tree of the release ZIP that
      package-windows.py wrote, so the installer holds exactly the zip's files,
      and print the installer's path. With --sign, ISCC Authenticode-signs the
      uninstaller it embeds and Setup.exe through windows-sign-file.ps1, which
      only the release workflow's windows-sign job can run; local and pull
      request builds leave it out and stay unsigned.
  smoke [--signed] INSTALLER
      On Windows: install silently into a scratch directory with the PATH
      task, check the files, their subsystems, the console copy's output,
      that the console copy opening the app does not hold its caller's pipes,
      the user PATH, and the entry Settings lists under Installed apps, then
      uninstall and check all of it is gone again. With --signed, also
      require valid timestamped signatures from one signer on the installer
      and on everything it installed that runs, the uninstaller included.
      Changes this user's PATH, and opens the app (which may start a Herdr
      daemon), while it runs: for CI runners only. Under a process a packaged
      (MSIX) app started, Windows redirects HKCU into that app's private hive,
      so the checks pass while Settings and new terminals see neither entry.

Pure standard library, like package-windows.py, for the Windows runner.
"""
import argparse
import hashlib
import importlib.util
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
SIGN_SCRIPT = ROOT / "scripts/release/windows-sign-file.ps1"
# ISCC replaces $q with a quote and $f with the quoted file to sign. Quotes
# inside the argument itself would not survive Windows command-line parsing.
SIGN_TOOL = ("herdr", f"pwsh.exe -NoLogo -NoProfile -NonInteractive -File $q{SIGN_SCRIPT}$q $f")
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


def iscc_arguments(version, target, source, output_dir, sign=False):
    """The compiler's command line after ISCC itself."""
    if not re.fullmatch(r"[1-9][0-9]{7}\.[1-9][0-9]*", version):
        raise ValueError("Version must be YYYYMMDD.COUNTER (no v prefix, counter from 1, no leading zeros)")
    if target not in ARCHITECTURES:
        raise ValueError("Unsupported Windows target")
    signing = [f"/S{SIGN_TOOL[0]}={SIGN_TOOL[1]}", f"/DSignTool={SIGN_TOOL[0]}"] if sign else []
    return ["/Q", f"/DVersion={version}", f"/DTarget={target}",
            f"/DArchitecture={ARCHITECTURES[target]}", f"/DSourceDir={source}",
            f"/DOutputDir={output_dir}", *signing, str(SCRIPT)]


def build(version, target, archive, output_dir, iscc, sign=False):
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
        arguments = iscc_arguments(version, target, source, output_dir, sign)
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


def processes_at(path):
    """Ids of running processes started from exactly `path`."""
    query = ("Get-CimInstance Win32_Process -Filter \"Name='herdr-gpui.exe'\" | "
             f"Where-Object ExecutablePath -eq '{path}' | ForEach-Object ProcessId")
    output = subprocess.run(["powershell", "-NoProfile", "-Command", query],
                            capture_output=True, text=True, check=True, timeout=60).stdout
    return [int(line) for line in output.split()]


def stop_processes_at(path):
    for pid in processes_at(path):
        subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True, timeout=60)


def check_opens_app_without_holding_pipes(cli, gui):
    """The console copy with no command starts the app and returns. A caller
    reading its output through pipes must see them close then, not when the
    app exits, or an agent's shell would hang on `herdr-gpui`."""
    started = subprocess.Popen([str(cli)], stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        # A timeout here, not in subprocess.run, which waits on the pipes
        # again without one after killing the child.
        started.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        started.kill()
        stop_processes_at(gui)
        started.communicate()
        raise ValueError("Console copy with no command held its caller's pipes open")
    finally:
        stop_processes_at(gui)
    if started.returncode != 0:
        raise ValueError(f"Console copy with no command exited with {started.returncode}")


def app_id():
    """The installer's AppId, without the brace Inno Setup escapes it with."""
    return re.search(r"^AppId=\{(\{[0-9A-F-]{36}\})$", SCRIPT.read_text(), re.MULTILINE)[1]


def uninstall_entry():
    """The values of the Uninstall key that Settings and Programs and Features
    list the installation by, or None when it is missing."""
    import winreg
    name = rf"Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_id()}_is1"
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, name) as key:
            count = winreg.QueryInfoKey(key)[1]
            return dict(winreg.EnumValue(key, index)[:2] for index in range(count))
    except FileNotFoundError:
        return None


def check_uninstall_entry(app):
    entry = uninstall_entry()
    if entry is None:
        raise ValueError("Installer wrote no Uninstall entry")
    # Settings hides entries without a name, system components, and updates
    # of another entry.
    if entry.get("DisplayName") != "Herdr UI" or entry.get("SystemComponent") or entry.get("ParentKeyName"):
        raise ValueError(f"Settings would not list the Uninstall entry: {entry}")
    if Path(entry.get("UninstallString", "").strip('"')) != app / "unins000.exe":
        raise ValueError(f"Uninstall entry does not run the uninstaller: {entry}")


def check_signed(paths):
    spec = importlib.util.spec_from_file_location("windows_signing", ROOT / "scripts/release/windows-signing.py")
    signing = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(signing)
    signing.check_signed(paths)


def smoke(installer, signed=False):
    with tempfile.TemporaryDirectory(prefix="herdr-installer-smoke-") as temp:
        app = Path(temp) / "Herdr UI"
        subprocess.run([str(installer), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-",
                        "/TASKS=addtopath", f"/DIR={app}", f"/LOG={Path(temp) / 'setup.log'}"],
                       check=True, timeout=600)
        # Uninstall even when a check fails, so the user PATH and the
        # uninstall entry never point into the deleted scratch directory.
        try:
            check_installation(app, installer if signed else None)
        finally:
            uninstall(app)


def check_installation(app, signed_installer):
    """`signed_installer` is the installer whose signatures to require along
    with the installed files', or None to skip signature checks."""
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
    if signed_installer is not None:
        check_signed([signed_installer.resolve(), gui, cli, app / "unins000.exe"])
    check_opens_app_without_holding_pipes(cli, gui)
    if str(app / "bin").lower() not in user_path_entries():
        raise ValueError("Installer did not add bin to the user PATH")
    check_uninstall_entry(app)


def uninstall(app):
    subprocess.run([str(app / "unins000.exe"), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"],
                   check=True, timeout=600)
    # The uninstaller hands off to a copy of itself and returns early.
    bin_entry = str(app / "bin").lower()
    deadline = time.monotonic() + 120
    while app.exists() or bin_entry in user_path_entries() or uninstall_entry() is not None:
        if time.monotonic() > deadline:
            raise ValueError("Uninstaller left the app directory, its PATH entry, or its Uninstall entry behind")
        time.sleep(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    install = commands.add_parser("install-iscc")
    install.add_argument("destination", type=Path)
    compile_ = commands.add_parser("build")
    compile_.add_argument("--sign", action="store_true")
    compile_.add_argument("version")
    compile_.add_argument("target")
    compile_.add_argument("archive", type=Path)
    compile_.add_argument("output_dir", type=Path)
    compile_.add_argument("iscc", type=Path)
    check = commands.add_parser("smoke")
    check.add_argument("--signed", action="store_true")
    check.add_argument("installer", type=Path)
    args = parser.parse_args()
    if args.command == "install-iscc":
        print(install_iscc(args.destination))
    elif args.command == "build":
        print(build(args.version, args.target, args.archive, args.output_dir, args.iscc, args.sign))
    else:
        smoke(args.installer, args.signed)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
