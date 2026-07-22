#!/usr/bin/env python3
"""
generate_assets.py
Run from the root of your linuxpet repo:
  python3 generate_assets.py

Creates:
  assets/cat/   idle walk sleep interact alert
  assets/dog/   idle walk sleep interact alert
  assets/fish/  idle swim alert
  assets/fonts/ JetBrainsMono-Regular.ttf  (downloaded)
"""

import os
import struct
import urllib.request

# ── Minimal valid GIF89a builder ──────────────────────────────────────────────
# Produces a 64x64 single-frame GIF with a solid colour.
# The image crate can decode this fine as a 1-frame animation.

def make_gif(r, g, b, w=64, h=64):
    """Return bytes of a minimal valid GIF89a with one solid-colour frame."""
    data = bytearray()

    # Header
    data += b'GIF89a'

    # Logical Screen Descriptor
    data += struct.pack('<HH', w, h)
    data += bytes([
        0b10000001,   # GCT present, colour depth = 2 bits (4 entries)
        0,            # background colour index
        0,            # pixel aspect ratio
    ])

    # Global Colour Table (4 entries = 12 bytes)
    # index 0 = transparent placeholder, index 1 = the fill colour
    data += bytes([0, 0, 0])          # index 0: black (unused)
    data += bytes([r, g, b])          # index 1: fill colour
    data += bytes([0, 0, 0])          # index 2: unused
    data += bytes([0, 0, 0])          # index 3: unused

    # Graphic Control Extension (100 ms frame delay, transparent index 0)
    data += bytes([
        0x21, 0xF9, 0x04,
        0b00000001,   # transparent colour flag
        10, 0,        # delay = 10 * 10ms = 100ms
        0,            # transparent colour index
        0x00,         # block terminator
    ])

    # Image Descriptor
    data += bytes([0x2C])
    data += struct.pack('<HHHH', 0, 0, w, h)
    data += bytes([0x00])   # no local colour table, not interlaced

    # Image Data — LZW compressed
    # For a solid image (all pixels = index 1) we use a hand-crafted LZW stream.
    # LZW minimum code size = 2
    # This stream encodes w*h pixels all set to colour index 1.
    # Pre-computed valid LZW for a 64x64 solid image (index 1):
    lzw_min = 2
    # Build raw pixel data: all pixels = 1
    pixels = bytes([1] * (w * h))
    lzw_data = _lzw_compress(pixels, lzw_min)

    data += bytes([lzw_min])
    # Split into sub-blocks (max 255 bytes each)
    i = 0
    while i < len(lzw_data):
        chunk = lzw_data[i:i+255]
        data += bytes([len(chunk)])
        data += chunk
        i += 255
    data += bytes([0x00])   # block terminator

    # GIF Trailer
    data += bytes([0x3B])
    return bytes(data)


def _lzw_compress(data, min_code_size):
    """Minimal LZW encoder for GIF image data."""
    clear_code = 1 << min_code_size
    eoi_code   = clear_code + 1

    code_size  = min_code_size + 1
    next_code  = eoi_code + 1

    table = {bytes([i]): i for i in range(clear_code)}

    output_bits = []

    def emit(code):
        b = code
        for _ in range(code_size):
            output_bits.append(b & 1)
            b >>= 1

    emit(clear_code)

    buf = bytes()
    for byte in data:
        candidate = buf + bytes([byte])
        if candidate in table:
            buf = candidate
        else:
            emit(table[buf])
            table[candidate] = next_code
            next_code += 1
            buf = bytes([byte])

            if next_code > (1 << code_size) and code_size < 12:
                code_size += 1

    if buf:
        emit(table[buf])

    emit(eoi_code)

    # Pack bits into bytes (LSB first)
    result = bytearray()
    for i in range(0, len(output_bits), 8):
        byte = 0
        for j, bit in enumerate(output_bits[i:i+8]):
            byte |= bit << j
        result.append(byte)
    return bytes(result)


# ── Asset definitions ─────────────────────────────────────────────────────────

ASSETS = {
    "assets/cat/idle.gif":     make_gif(180, 140, 100),   # warm tan
    "assets/cat/walk.gif":     make_gif(180, 140, 100),
    "assets/cat/sleep.gif":    make_gif(140, 110,  80),   # darker, sleepy
    "assets/cat/interact.gif": make_gif(220, 180, 120),   # brighter, excited
    "assets/cat/alert.gif":    make_gif(220,  80,  80),   # red-ish, panic

    "assets/dog/idle.gif":     make_gif(160, 120,  80),   # brown
    "assets/dog/walk.gif":     make_gif(160, 120,  80),
    "assets/dog/sleep.gif":    make_gif(120,  90,  60),
    "assets/dog/interact.gif": make_gif(200, 160, 100),
    "assets/dog/alert.gif":    make_gif(220,  80,  80),

    "assets/fish/idle.gif":    make_gif( 80, 160, 220),   # blue
    "assets/fish/swim.gif":    make_gif( 80, 180, 240),
    "assets/fish/alert.gif":   make_gif(220,  80,  80),
}

FONT_URL  = "https://github.com/JetBrains/JetBrainsMono/raw/master/fonts/ttf/JetBrainsMono-Regular.ttf"
FONT_PATH = "assets/fonts/JetBrainsMono-Regular.ttf"

# ── Write files ───────────────────────────────────────────────────────────────

def main():
    for path, data in ASSETS.items():
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)
        print(f"  created  {path}  ({len(data)} bytes)")

    # Download font
    os.makedirs("assets/fonts", exist_ok=True)
    if os.path.exists(FONT_PATH) and os.path.getsize(FONT_PATH) > 10_000:
        print(f"  exists   {FONT_PATH}  (skipping download)")
    else:
        print(f"  downloading {FONT_PATH} ...")
        urllib.request.urlretrieve(FONT_URL, FONT_PATH)
        print(f"  done     {FONT_PATH}  ({os.path.getsize(FONT_PATH):,} bytes)")

    print("\n✅ All assets ready. Run: cargo build")

if __name__ == "__main__":
    main()
