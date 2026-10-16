#!/usr/bin/env python3
"""Package the experimental Windows executable as Herdr-VERSION-TARGET.zip.

Pure standard library so the Windows runner needs no zip tool. The layout
mirrors package-linux.sh, the executable plus the same license and notice
files, with one addition. As with Zed, the release build is a GUI-subsystem
program, so opening it never shows a console window, and `bin/herdr-gpui.exe`
is a console-subsystem copy for terminals and agents: a shell waits for it and
sees its exit code. The copy differs only in the PE header's subsystem field,
the change `editbin /SUBSYSTEM:CONSOLE` makes; the app relies on the layout
(`crates/herdr-gpui/src/console.rs`).
"""
import argparse
from pathlib import Path
import re
import struct
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
TARGETS = ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")
LICENSES = ("LICENSE", "NOTICE", "assets/icons/LICENSE-octicons",
            "crates/herdr-protocol/LICENSE-APACHE", "crates/herdr-protocol/NOTICE.md",
            "crates/herdr-gpui/SOUND-NOTICE.md")
# IMAGE_OPTIONAL_HEADER.Subsystem values, at the same offset in PE32 and PE32+.
SUBSYSTEM_GUI = 2
SUBSYSTEM_CONSOLE = 3
SUBSYSTEM_OFFSET = 68
# Zip cannot store dates before 1980; a fixed stamp keeps entries independent
# of checkout and build times.
TIMESTAMP = (1980, 1, 1, 0, 0, 0)


def add(archive, name, data):
    info = zipfile.ZipInfo(name, TIMESTAMP)
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = 0o644 << 16
    archive.writestr(info, data)


def console_copy(data):
    """`data`, a GUI-subsystem PE image, as a console-subsystem one."""
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError("Binary is not a Windows executable")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    optional = pe + 24
    field = optional + SUBSYSTEM_OFFSET
    if data[pe:pe + 4] != b"PE\0\0" or len(data) < field + 2:
        raise ValueError("Binary is not a Windows executable")
    if struct.unpack_from("<H", data, optional)[0] not in (0x10B, 0x20B):
        raise ValueError("Binary has no PE32 or PE32+ optional header")
    if struct.unpack_from("<H", data, field)[0] != SUBSYSTEM_GUI:
        raise ValueError("Binary must be a GUI-subsystem release build")
    copy = bytearray(data)
    struct.pack_into("<H", copy, field, SUBSYSTEM_CONSOLE)
    return bytes(copy)


def package(version, target, binary, output_dir, notices):
    if not re.fullmatch(r"[1-9][0-9]{7}\.[1-9][0-9]*", version):
        raise ValueError("Version must be YYYYMMDD.COUNTER (no v prefix, counter from 1, no leading zeros)")
    if target not in TARGETS:
        raise ValueError("Unsupported Windows target")
    if not binary.is_file() or not output_dir.is_dir():
        raise ValueError("Binary file and existing output directory required")
    if not notices.is_file() or not notices.stat().st_size:
        raise ValueError("Nonempty third-party notices file required")
    app = binary.read_bytes()
    cli = console_copy(app)
    name = f"Herdr-{version}-{target}"
    output = output_dir.resolve() / f"{name}.zip"
    if output.exists() or output.is_symlink():
        raise ValueError(f"Output already exists: {output}")
    # Build beside the output and rename, so a failure never leaves a partial zip.
    with tempfile.NamedTemporaryFile(dir=output.parent, prefix=".herdr-windows.", delete=False) as temp:
        partial = Path(temp.name)
    try:
        with zipfile.ZipFile(partial, "w") as archive:
            add(archive, f"{name}/herdr-gpui.exe", app)
            add(archive, f"{name}/bin/herdr-gpui.exe", cli)
            for source in LICENSES:
                add(archive, f"{name}/licenses/{Path(source).name}", (ROOT / source).read_bytes())
            add(archive, f"{name}/licenses/THIRD-PARTY-NOTICES.txt", notices.read_bytes())
        if output.exists() or output.is_symlink():
            raise ValueError(f"Output already exists: {output}")
        partial.replace(output)
    finally:
        partial.unlink(missing_ok=True)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("target")
    parser.add_argument("binary", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("notices", type=Path)
    args = parser.parse_args()
    print(package(args.version, args.target, args.binary, args.output_dir, args.notices))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
