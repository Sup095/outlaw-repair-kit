"""Draw the application icon.

Kept as a script rather than a checked-in binary blob so the mark can be
changed without a design tool, and so anyone can see exactly what the icon
is. Run it from this directory:

    python generate_icons.py

It writes icon.png (512x512) and icon.ico (256x256, PNG-encoded inside the
ICO container, which Windows has accepted since Vista).
"""

import struct
import zlib

BACKGROUND = (12, 14, 20)
AMBER = (255, 176, 0)
CYAN = (34, 224, 226)


def draw(size):
    """Return RGBA pixel rows: a ring with a slash through it."""
    centre = (size - 1) / 2
    outer = size * 0.40
    inner = size * 0.29
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            dx, dy = x - centre, y - centre
            distance = (dx * dx + dy * dy) ** 0.5
            colour, alpha = BACKGROUND, 255
            if distance > size * 0.47:
                alpha = 0
            elif inner <= distance <= outer:
                colour = AMBER
            # The slash: one continuous diagonal band straight through the
            # ring, so it reads as a struck-through O rather than two nicks.
            if abs(dx + dy) < size * 0.06 and distance <= outer:
                colour = CYAN
            row += bytes(colour) + bytes([alpha])
        rows.append(bytes(row))
    return rows


def png(size):
    rows = draw(size)
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(png_bytes, size):
    entry = struct.pack(
        "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png_bytes), 22
    )
    return struct.pack("<HHH", 0, 1, 1) + entry + png_bytes


if __name__ == "__main__":
    with open("icon.png", "wb") as handle:
        handle.write(png(512))
    small = png(256)
    with open("icon.ico", "wb") as handle:
        handle.write(ico(small, 256))
    print("wrote icon.png and icon.ico")
