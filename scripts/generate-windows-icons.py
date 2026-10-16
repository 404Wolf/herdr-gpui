#!/usr/bin/env python3
"""Generate the Windows icons and installer images from the circular Linux SVGs.

Rasterizes each size directly from the vector artwork with `rsvg-convert`
(`brew install librsvg`) or `resvg` (`cargo install resvg`), rather than
downsampling one PNG, then packs the PNGs into .ico files with the standard
library. The outputs are checked in, so ordinary builds need no renderer.
"""
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile

ASSETS = Path(__file__).resolve().parents[1] / "assets/icons"
OUTPUT = ASSETS / "windows"
# The Windows shell picks from these: 16-48 for lists, menus, and the taskbar
# at each DPI scale, 64 and 256 for Explorer's larger views.
ICON_SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
ICONS = {"herdr.ico": "herdr-linux.svg", "herdr-worktree.ico": "herdr-linux-worktree.svg"}
# Inno Setup picks the file nearest the DPI scale: the small image is 55px at
# 100%, and the large one is centered, unstretched, on the welcome page.
WIZARD_SMALL = (55, 69, 83, 110, 138)
WIZARD_LARGE = (128, 160, 192, 256)
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def renderer():
    for name, arguments in (
        ("rsvg-convert", lambda size, source, output: ["--width", str(size), "--height", str(size),
                                                       "--output", str(output), str(source)]),
        ("resvg", lambda size, source, output: ["-w", str(size), "-h", str(size),
                                                str(source), str(output)]),
    ):
        if path := shutil.which(name):
            return lambda size, source, output: subprocess.run(
                [path, *arguments(size, source, output)], check=True)
    raise ValueError("Install rsvg-convert (librsvg) or resvg to rasterize the icons")


def png_size(data):
    if data[:8] != PNG_SIGNATURE or data[12:16] != b"IHDR":
        raise ValueError("Rasterizer did not write a PNG")
    return struct.unpack(">II", data[16:24])


def ico(images):
    """An .ico holding `images`, PNG data keyed by square size. Every entry is
    PNG-compressed, which Windows has read since Vista."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, payload = b"", b""
    for size, data in sorted(images.items()):
        if png_size(data) != (size, size):
            raise ValueError(f"Expected a {size}px PNG")
        # Width and height 0 mean 256.
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data),
                               offset + len(payload))
        payload += data
    return header + entries + payload


def main():
    render = renderer()
    OUTPUT.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        def raster(source, size):
            output = Path(temp) / f"{size}.png"
            render(size, ASSETS / source, output)
            return output.read_bytes()

        for name, source in ICONS.items():
            (OUTPUT / name).write_bytes(ico({size: raster(source, size) for size in ICON_SIZES}))
            print(f"Generated assets/icons/windows/{name}")
        for prefix, sizes in (("wizard-small", WIZARD_SMALL), ("wizard", WIZARD_LARGE)):
            for size in sizes:
                name = f"{prefix}-{size}.png"
                (OUTPUT / name).write_bytes(raster(ICONS["herdr.ico"], size))
                print(f"Generated assets/icons/windows/{name}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
