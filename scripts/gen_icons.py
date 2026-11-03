#!/usr/bin/env python3
"""Generate DefReal app icons: a cyan proximity arc + violet mic diamond on dark bg."""
import struct, zlib, os, math

def png_bytes(w, h, px):
    """Encode RGBA pixel array (list of rows of (r,g,b,a)) as PNG."""
    raw = b"".join(b"\x00" + b"".join(struct.pack("4B", *p) for p in row) for row in px)
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))

def render(size):
    px = []
    cx = cy = size / 2
    # colors
    bg = (10, 10, 15)
    cyan = (62, 230, 216)
    violet = (139, 124, 246)
    for y in range(size):
        row = []
        for x in range(size):
            dx, dy = x - cx, y - cy
            r = math.hypot(dx, dy)
            # rounded-square dark bg
            c = list(bg)
            a = 255
            # outer ring (mic pickup arc) - partial circle
            ring_r = size * 0.38
            ring_w = max(2, size * 0.045)
            if abs(r - ring_r) < ring_w:
                ang = math.degrees(math.atan2(dy, dx))
                if -150 < ang < 150:
                    t = 1 - abs(r - ring_r) / ring_w
                    c = [int(bg[i] + (cyan[i] - bg[i]) * t) for i in range(3)]
            # center diamond (mic)
            dr = abs(dx) + abs(dy)
            if dr < size * 0.16:
                c = list(violet)
            row.append(tuple(c) + (a,))
        px.append(row)
    return png_bytes(size, size, px)

os.makedirs("icons", exist_ok=True)
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 256)]:
    data = render(size)
    open(os.path.join("icons", name), "wb").write(data)
    print(f"wrote icons/{name} ({size}x{size}, {len(data)} bytes)")

# ICO: embed the 256 PNG + a 32 BMP-ish (use 32 PNG too; Vista+ accepts PNG frames)
png256 = render(256)
png32 = render(32)
png16 = render(16)

entries = []
data = b""
for img in [png16, png32, png256]:
    w = h = 256 if len(img) > 20000 else 0  # placeholder, fix below
    entries.append((len(img), img))
    data += img

# rebuild properly
header = struct.pack("<HHH", 0, 1, 3)
body = b""
off = 6 + 16 * 3
dirents = b""
for size_px, img in [(16, png16), (32, png32), (256, png256)]:
    b_w = size_px if size_px < 256 else 0
    dirents += struct.pack("<BBBBHHII", b_w, b_w, 0, 0, 1, 32, len(img), off)
    body += img
    off += len(img)
open("icons/icon.ico", "wb").write(header + dirents + body)
print("wrote icons/icon.ico")
