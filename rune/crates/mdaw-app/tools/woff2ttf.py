#!/usr/bin/env python3
"""WOFF 1.0 -> TTF/OTF (sfnt), stdlib only.

The Electron app bundles its fonts as @fontsource WOFF files; Rune embeds sfnt bytes, so the same
files are unwrapped (zlib per table) into TTF. Usage: woff2ttf.py in.woff out.ttf

The @fontsource static faces name each non-RIBBI weight as its own legacy family (name ID 1
"Manrope SemiBold", ID 2 "Regular") and carry no typographic names (IDs 16 / 17). A browser
groups faces by the CSS `@font-face` family, but fontique (Rune's font index) groups by ID 16,
else ID 1, so "Manrope SemiBold" became a family of its own and weight 600 resolved to the
"Manrope" family's 700. The converter therefore adds ID 16 (the family without the weight
suffix) and ID 17 (the weight's style name, from OS/2 `usWeightClass`) to every Windows-platform
English name set. Glyph data is untouched; the `name` checksum and `head.checkSumAdjustment`
are recomputed.
"""
import struct
import sys
import zlib

STYLE_NAMES = {
    100: "Thin",
    200: "ExtraLight",
    300: "Light",
    400: "Regular",
    500: "Medium",
    600: "SemiBold",
    700: "Bold",
    800: "ExtraBold",
    900: "Black",
}
WEIGHT_SUFFIXES = [n for w, n in STYLE_NAMES.items() if w not in (400, 700)] + ["Semi Bold", "Extra Bold"]


def checksum(body: bytes) -> int:
    body = body + b"\0" * ((4 - len(body) % 4) % 4)
    return sum(struct.unpack(">%dI" % (len(body) // 4), body)) & 0xFFFFFFFF


def read_names(name: bytes):
    _fmt, count, string_off = struct.unpack(">HHH", name[:6])
    records = []
    for i in range(count):
        pid, eid, lid, nid, length, off = struct.unpack(">HHHHHH", name[6 + 12 * i:18 + 12 * i])
        records.append(((pid, eid, lid, nid), name[string_off + off:string_off + off + length]))
    return records


def write_names(records) -> bytes:
    records = sorted(records, key=lambda r: r[0])
    head = bytearray(struct.pack(">HHH", 0, len(records), 6 + 12 * len(records)))
    strings = bytearray()
    for (pid, eid, lid, nid), raw in records:
        head += struct.pack(">HHHHHH", pid, eid, lid, nid, len(raw), len(strings))
        strings += raw
    return bytes(head + strings)


def add_typographic_names(name: bytes, weight: int) -> bytes:
    records = [r for r in read_names(name) if r[0][3] not in (16, 17)]
    style = STYLE_NAMES.get(round(weight / 100) * 100, "Regular")
    for (pid, eid, lid, nid), raw in list(records):
        if pid != 3 or nid != 1:
            continue
        legacy = raw.decode("utf-16-be")
        family = legacy
        for suffix in WEIGHT_SUFFIXES:
            if family.endswith(" " + suffix):
                family = family[: -len(suffix) - 1]
                break
        records.append(((pid, eid, lid, 16), family.encode("utf-16-be")))
        records.append(((pid, eid, lid, 17), style.encode("utf-16-be")))
    return write_names(records)


def convert(data: bytes) -> bytes:
    sig, flavor, _length, num_tables = struct.unpack(">IIIH", data[:14])
    if sig != 0x774F4646:
        raise SystemExit("not a WOFF 1.0 file")
    tables = {}
    for i in range(num_tables):
        tag, off, comp_len, orig_len, _checksum = struct.unpack(">4sIII I", data[44 + i * 20:64 + i * 20])
        raw = data[off:off + comp_len]
        body = zlib.decompress(raw) if comp_len < orig_len else raw
        assert len(body) == orig_len, tag
        tables[tag] = body
    weight = struct.unpack(">H", tables[b"OS/2"][4:6])[0]
    tables[b"name"] = add_typographic_names(tables[b"name"], weight)
    head = bytearray(tables[b"head"])
    head[8:12] = b"\0\0\0\0"
    tables[b"head"] = bytes(head)

    tags = sorted(tables)
    n = len(tags)
    es = max(i for i in range(17) if (1 << i) <= n)
    search = (1 << es) * 16
    out = bytearray(struct.pack(">IHHHH", flavor, n, search, es, n * 16 - search))
    offset = 12 + 16 * n
    dir_entries = bytearray()
    bodies = bytearray()
    head_at = 0
    for tag in tags:
        body = tables[tag]
        if tag == b"head":
            head_at = offset + len(bodies)
        dir_entries += struct.pack(">4sIII", tag, checksum(body), offset + len(bodies), len(body))
        bodies += body
        bodies += b"\0" * ((4 - len(body) % 4) % 4)
    font = bytearray(out + dir_entries + bodies)
    adjust = (0xB1B0AFBA - checksum(bytes(font))) & 0xFFFFFFFF
    font[head_at + 8:head_at + 12] = struct.pack(">I", adjust)
    return bytes(font)


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, "rb") as f:
        data = f.read()
    with open(dst, "wb") as f:
        f.write(convert(data))
