"""Run: python3 -m unittest discover -s scripts/release/tests -v"""
import importlib.util
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile

from test_release import windows_executable

SCRIPT = Path(__file__).resolve().parents[1] / "package-windows.py"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PACKAGE = load("package_windows", SCRIPT)
INSTALLER_SCRIPT = SCRIPT.parent / "windows-installer.py"
INSTALLER = load("windows_installer", INSTALLER_SCRIPT)
VERSION = "20260920.3"
TARGET = "x86_64-pc-windows-msvc"
# Stands in for ISCC.exe: checks it was given the zip's tree and writes the
# installer where the defines say.
MOCK_ISCC = """#!/usr/bin/env python3
import pathlib, sys
defines = dict(arg[2:].split("=", 1) for arg in sys.argv[1:] if arg.startswith("/D"))
source = pathlib.Path(defines["SourceDir"])
assert (source / "herdr-gpui.exe").is_file() and (source / "bin/herdr-gpui.exe").is_file()
assert sys.argv[-1].endswith("herdr-gpui.iss")
name = f"Herdr-{defines['Version']}-{defines['Target']}-setup.exe"
(pathlib.Path(defines["OutputDir"]) / name).write_bytes(b"setup " + defines["Architecture"].encode())
"""


class ConsoleCopyTests(unittest.TestCase):
    def test_only_the_subsystem_changes(self):
        app = windows_executable()
        cli = PACKAGE.console_copy(app)
        self.assertEqual(len(cli), len(app))
        changed = [i for i, (a, b) in enumerate(zip(app, cli)) if a != b]
        field = 0x40 + 24 + PACKAGE.SUBSYSTEM_OFFSET
        self.assertEqual(changed, [field])
        self.assertEqual(struct.unpack_from("<H", cli, field)[0], PACKAGE.SUBSYSTEM_CONSOLE)

    def test_pe32_images_are_patched_too(self):
        app = bytearray(windows_executable())
        struct.pack_into("<H", app, 0x40 + 24, 0x10B)
        cli = PACKAGE.console_copy(bytes(app))
        self.assertEqual(struct.unpack_from("<H", cli, 0x40 + 24 + 68)[0], 3)

    def test_rejects_what_is_not_a_gui_release_build(self):
        app = windows_executable()
        bad_magic = bytearray(app)
        struct.pack_into("<H", bad_magic, 0x40 + 24, 0x107)
        far_header = bytearray(app)
        struct.pack_into("<I", far_header, 0x3C, len(app))
        cases = {
            "empty": b"",
            "not MZ": b"ZM" + app[2:],
            "no PE signature": app[:0x40] + b"NE\0\0" + app[0x44:],
            "header past the end": bytes(far_header),
            "truncated": app[:0x40 + 24 + 60],
            "unknown optional header": bytes(bad_magic),
            # A debug build keeps the console subsystem; packaging it would
            # ship an app that opens a console window.
            "console build": windows_executable(3),
        }
        for name, data in cases.items():
            with self.subTest(name), self.assertRaises(ValueError):
                PACKAGE.console_copy(data)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="herdr-installer-test-")
        self.addCleanup(temp.cleanup)
        self.work = Path(temp.name)
        self.iscc = self.work / "iscc"
        self.iscc.write_text(MOCK_ISCC)
        self.iscc.chmod(0o755)

    def release_zip(self, entries):
        path = self.work / f"Herdr-{VERSION}-{TARGET}.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for name in entries:
                archive.writestr(name, b"data")
        return path

    def build(self, archive, success=True):
        result = subprocess.run([sys.executable, str(INSTALLER_SCRIPT), "build", VERSION, TARGET,
                                 str(archive), str(self.work), str(self.iscc)],
                                capture_output=True, text=True, timeout=60,
                                env={"PATH": os.environ["PATH"]})
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_builds_from_the_zip_tree(self):
        root = f"Herdr-{VERSION}-{TARGET}/"
        archive = self.release_zip([root + "herdr-gpui.exe", root + "bin/herdr-gpui.exe",
                                    root + "licenses/LICENSE"])
        output = Path(self.build(archive).stdout.strip())
        self.assertEqual(output, self.work.resolve() / f"Herdr-{VERSION}-{TARGET}-setup.exe")
        self.assertEqual(output.read_bytes(), b"setup x64compatible")
        result = self.build(archive, success=False)
        self.assertIn("Output already exists", result.stderr)

    def test_rejects_a_zip_without_the_layout(self):
        root = f"Herdr-{VERSION}-{TARGET}/"
        cases = {
            "no console copy": [root + "herdr-gpui.exe"],
            "outside the root": [root + "herdr-gpui.exe", root + "bin/herdr-gpui.exe", "other/x"],
            "escaping the root": [root + "herdr-gpui.exe", root + "bin/herdr-gpui.exe", root + "../x"],
        }
        for name, entries in cases.items():
            with self.subTest(name):
                self.build(self.release_zip(entries), success=False)
        self.assertEqual(list(self.work.glob("*-setup.exe")), [])

    def test_compiler_arguments(self):
        arguments = INSTALLER.iscc_arguments(VERSION, "aarch64-pc-windows-msvc", "src", "out")
        self.assertIn("/DArchitecture=arm64", arguments)
        self.assertIn("/DSourceDir=src", arguments)
        self.assertEqual(Path(arguments[-1]), INSTALLER.SCRIPT)
        self.assertTrue(INSTALLER.SCRIPT.is_file())
        for version, target in [("20260920.03", TARGET), ("v20260920.3", TARGET),
                                (VERSION, "x86_64-pc-windows-gnu")]:
            with self.subTest(version=version, target=target), self.assertRaises(ValueError):
                INSTALLER.iscc_arguments(version, target, "src", "out")

    def test_the_inno_setup_pin_is_one_exact_release(self):
        self.assertRegex(INSTALLER.INNO_SHA256, r"^[0-9a-f]{64}$")
        self.assertIn(f"/is-{INSTALLER.INNO_VERSION.replace('.', '_')}/", INSTALLER.INNO_URL)
        self.assertTrue(INSTALLER.INNO_URL.startswith("https://github.com/jrsoftware/issrc/releases/download/"))


if __name__ == "__main__":
    unittest.main()
