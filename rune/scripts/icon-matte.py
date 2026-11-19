#!/usr/bin/env python3
"""Difference matte: the same RGBA-opaque image rendered on black and on white -> one PNG with
true alpha (alpha = 1 - (white - black), colour = black / alpha). Standard library only.

    python3 -I icon-matte.py on-black.png on-white.png out.png
"""
import struct
import sys
import zlib


def read_png(path):
    d = open(path, 'rb').read()
    assert d[:8] == b'\x89PNG\r\n\x1a\n', path
    i, idat = 8, b''
    while i < len(d):
        n, = struct.unpack('>I', d[i:i + 4])
        t, c = d[i + 4:i + 8], d[i + 8:i + 8 + n]
        i += 12 + n
        if t == b'IHDR':
            w, h, bd, ct = struct.unpack('>IIBB', c[:10])
            assert bd == 8 and ct in (2, 6), (bd, ct)
            bpp = 4 if ct == 6 else 3
        elif t == b'IDAT':
            idat += c
    raw = zlib.decompress(idat)
    stride = w * bpp
    out = bytearray(h * stride)
    prev = bytearray(stride)
    pos = 0
    for y in range(h):
        f = raw[pos]
        line = bytearray(raw[pos + 1:pos + 1 + stride])
        pos += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return w, h, bpp, out


def write_png(path, w, h, rgba):
    rows = b''.join(b'\x00' + bytes(rgba[y * w * 4:(y + 1) * w * 4]) for y in range(h))

    def chunk(t, c):
        return struct.pack('>I', len(c)) + t + c + struct.pack('>I', zlib.crc32(t + c) & 0xffffffff)

    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b'')
    open(path, 'wb').write(png)


def main():
    wb, hb, pb, black = read_png(sys.argv[1])
    ww, hw, pw, white = read_png(sys.argv[2])
    assert (wb, hb) == (ww, hw)
    out = bytearray(wb * hb * 4)
    for i in range(wb * hb):
        kb, kw = i * pb, i * pw
        diff = sum(white[kw + j] - black[kb + j] for j in range(3)) / 3.0
        alpha = max(0.0, min(1.0, 1.0 - diff / 255.0))
        o = i * 4
        if alpha > 0:
            for j in range(3):
                out[o + j] = max(0, min(255, round(black[kb + j] / alpha)))
        out[o + 3] = round(alpha * 255)
    write_png(sys.argv[3], wb, hb, out)


if __name__ == '__main__':
    main()
