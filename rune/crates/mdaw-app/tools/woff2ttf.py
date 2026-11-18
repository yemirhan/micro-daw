#!/usr/bin/env python3
"""WOFF 1.0 -> TTF/OTF (sfnt), stdlib only.

The Electron app bundles its fonts as @fontsource WOFF files; Rune embeds sfnt bytes, so the same
files are unwrapped (zlib per table) into TTF. Usage: woff2ttf.py in.woff out.ttf
"""
import struct
import sys
import zlib


def convert(data: bytes) -> bytes:
    sig, flavor, _length, num_tables = struct.unpack(">IIIH", data[:14])
    if sig != 0x774F4646:
        raise SystemExit("not a WOFF 1.0 file")
    tables = []
    for i in range(num_tables):
        tag, off, comp_len, orig_len, checksum = struct.unpack(">4sIIII", data[44 + i * 20:64 + i * 20])
        raw = data[off:off + comp_len]
        body = zlib.decompress(raw) if comp_len < orig_len else raw
        assert len(body) == orig_len, tag
        tables.append((tag, checksum, body))
    tables.sort(key=lambda t: t[0])
    n = len(tables)
    es = max(i for i in range(17) if (1 << i) <= n)
    search = (1 << es) * 16
    out = bytearray(struct.pack(">IHHHH", flavor, n, search, es, n * 16 - search))
    offset = 12 + 16 * n
    dir_entries = bytearray()
    bodies = bytearray()
    for tag, checksum, body in tables:
        dir_entries += struct.pack(">4sIII", tag, checksum, offset + len(bodies), len(body))
        bodies += body
        bodies += b"\0" * ((4 - len(body) % 4) % 4)
    return bytes(out + dir_entries + bodies)


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, "rb") as f:
        data = f.read()
    with open(dst, "wb") as f:
        f.write(convert(data))
