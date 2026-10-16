"""The checked-in Windows icons, and the AppUserModelID shared by the app and its shortcuts."""
from pathlib import Path
import re
import runpy
import struct
import unittest

ROOT = Path(__file__).resolve().parents[3]
WINDOWS = ROOT / "assets/icons/windows"
GENERATOR = runpy.run_path(str(ROOT / "scripts/generate-windows-icons.py"))
ISS = ROOT / "scripts/release/herdr-gpui.iss"


def ico_entries(data):
    """(size, PNG data) for each image in an .ico."""
    reserved, kind, count = struct.unpack_from("<HHH", data)
    if (reserved, kind) != (0, 1):
        raise ValueError("Not an icon file")
    entries = []
    for index in range(count):
        width, height, _, _, _, _, length, offset = struct.unpack_from("<BBBBHHII", data, 6 + 16 * index)
        if width != height:
            raise ValueError("Icon images must be square")
        entries.append((width or 256, data[offset:offset + length]))
    return entries


class WindowsIconTests(unittest.TestCase):
    def test_icons_hold_every_shell_size_as_matching_pngs(self):
        for name in GENERATOR["ICONS"]:
            with self.subTest(icon=name):
                entries = ico_entries((WINDOWS / name).read_bytes())
                self.assertEqual([size for size, _ in entries], list(GENERATOR["ICON_SIZES"]))
                for size, png in entries:
                    self.assertEqual(GENERATOR["png_size"](png), (size, size))

    def test_packing_round_trips_and_rejects_a_mismatched_size(self):
        images = {size: png for size, png in ico_entries((WINDOWS / "herdr.ico").read_bytes())}
        self.assertEqual(GENERATOR["ico"](images), (WINDOWS / "herdr.ico").read_bytes())
        with self.assertRaises(ValueError):
            GENERATOR["ico"]({32: images[16]})

    def test_installer_images_exist_at_their_sizes(self):
        for prefix, sizes in (("wizard-small", GENERATOR["WIZARD_SMALL"]), ("wizard", GENERATOR["WIZARD_LARGE"])):
            for size in sizes:
                with self.subTest(image=f"{prefix}-{size}"):
                    png = (WINDOWS / f"{prefix}-{size}.png").read_bytes()
                    self.assertEqual(GENERATOR["png_size"](png), (size, size))

    def test_installer_names_only_checked_in_images(self):
        script = ISS.read_text()
        icons = re.search(r'^#define Icons "([^"]+)"$', script, re.M).group(1)
        directory = (ISS.parent / icons.replace("\\", "/")).resolve()
        self.assertEqual(directory, WINDOWS.resolve())
        for directive in ("SetupIconFile", "WizardSmallImageFile", "WizardImageFile"):
            with self.subTest(directive=directive):
                value = re.search(rf"^{directive}=(.+)$", script, re.M).group(1)
                for path in value.split(","):
                    self.assertTrue(path.startswith("{#Icons}\\"), path)
                    self.assertTrue((WINDOWS / path.removeprefix("{#Icons}\\")).is_file(), path)

    def test_shortcuts_carry_the_apps_own_user_model_id(self):
        constants = (ROOT / "crates/herdr-gpui/src/constants.rs").read_text()
        app_id = re.search(r'^pub\(crate\) const APP_ID: &str = "([^"]+)";$', constants, re.M).group(1)
        script = ISS.read_text()
        self.assertIn(f'#define AppUserModelID "{app_id}"\n', script)
        shortcuts = [line for line in script.splitlines() if line.startswith('Name: "{user')]
        self.assertEqual(len(shortcuts), 2)
        for line in shortcuts:
            self.assertIn('AppUserModelID: "{#AppUserModelID}"', line)
        install = (ROOT / "site/install.ps1").read_text()
        self.assertIn(f"[HerdrShortcut]::SetAppUserModelId($shortcut, '{app_id}')", install)


if __name__ == "__main__":
    unittest.main()
