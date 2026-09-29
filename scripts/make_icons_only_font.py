#!/usr/bin/env python3
"""Regenerate the PUA-only Phosphor icon font (PhosphorIcons.ttf).

Rebuilds the cmap of the source Phosphor.ttf keeping ONLY icon codepoints
(>= U+E000), so the resulting font can be placed FIRST in egui's font
stack without stealing ASCII glyphs (space, hyphen, 0-9, a-f) from the
real UI font. Icons then always resolve to real icon glyphs even when the
system font family covers some Phosphor codepoints.

Usage:
    python make_icons_only_font.py <src Phosphor.ttf> <dst PhosphorIcons.ttf>
"""
import struct
import sys


def read_code_to_gid(data, t):
    """code -> gid map of one format-4 or format-12 cmap subtable."""
    m = {}
    fmt = struct.unpack(">H", data[t:t+2])[0]
    if fmt == 4:
        segs = struct.unpack(">H", data[t+6:t+8])[0] // 2
        ends = struct.unpack(">%dH" % segs, data[t+14:t+14+2*segs])
        starts = struct.unpack(">%dH" % segs, data[t+16+2*segs:t+16+4*segs])
        deltas = struct.unpack(">%dH" % segs, data[t+16+4*segs:t+16+6*segs])
        rngoffs = struct.unpack(">%dH" % segs, data[t+16+6*segs:t+16+8*segs])
        for s, e, d, r in zip(starts, ends, deltas, rngoffs):
            if s == 0xFFFF:
                continue
            for c in range(s, e + 1):
                if r == 0:
                    g = (c + d) & 0xFFFF
                else:
                    g = 0  # format-4 indirect segments unused by Phosphor; skip
                if g != 0:
                    m[c] = g
    elif fmt == 12:
        g_count = struct.unpack(">I", data[t+12:t+16])[0]
        for j in range(g_count):
            o2 = t + 16 + j * 12
            s, e, g0 = struct.unpack(">III", data[o2:o2+12])
            for c in range(s, e + 1):
                m[c] = g0 + (c - s)
    return m


def emit_format4(code_to_gid):
    """Build a format-4 subtable from a code->gid map."""
    codes = sorted(code_to_gid)
    # group into runs of consecutive codes with consecutive gids
    segs = []  # (start, end, delta)
    for c in codes:
        g = code_to_gid[c]
        if segs and segs[-1][1] == c - 1 and (c + segs[-1][2]) & 0xFFFF == g:
            segs[-1][1] = c
        else:
            segs.append([c, c, (g - c) & 0xFFFF])
    segs.append([0xFFFF, 0xFFFF, 1])
    seg_count = len(segs)
    seg_x2 = seg_count * 2
    exp = 0
    while (1 << (exp + 1)) <= seg_count:
        exp += 1
    search_range = (1 << exp) * 2
    body = struct.pack(">HHHHHHH", 4, 16 + 8 * seg_count, 0, seg_x2,
                       search_range, exp, seg_x2 - search_range)
    body += b"".join(struct.pack(">H", s[1]) for s in segs)
    body += b"\x00\x00"
    body += b"".join(struct.pack(">H", s[0]) for s in segs)
    body += b"".join(struct.pack(">H", s[2]) for s in segs)
    body += b"\x00" * seg_x2
    return body


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]

    data = bytearray(open(src, "rb").read())
    nt = struct.unpack(">H", data[4:6])[0]

    # locate cmap table
    cmap_off = None
    for i in range(nt):
        o = 12 + i * 16
        if data[o:o+4] == b"cmap":
            cmap_off = struct.unpack(">I", data[o+8:o+12])[0]

    co = cmap_off
    n_sub = struct.unpack(">H", data[co+2:co+4])[0]
    subtables = []  # (platform, encoding)
    for i in range(n_sub):
        o = co + 4 + i * 8
        pl, en, so = struct.unpack(">HHI", data[o:o+8])
        subtables.append((pl, en, co + so))

    # rebuild each subtable with only PUA (>= U+E000) mappings
    new_subs = []
    for pl, en, t in subtables:
        m = read_code_to_gid(data, t)
        kept = {c: g for c, g in m.items() if c >= 0xE000}
        dropped = len(m) - len(kept)
        new_subs.append((pl, en, emit_format4(kept), len(m), dropped))
        print("subtable p=%d e=%d: kept %d, dropped %d" % (pl, en, len(kept), dropped))

    # assemble new cmap table
    header_len = 4 + 8 * len(new_subs)
    offsets = []
    cur = header_len
    for pl, en, body, _, _ in new_subs:
        offsets.append((pl, en, cur))
        cur += len(body)
    cmap = struct.pack(">HH", 0, len(new_subs))
    for pl, en, off in offsets:
        cmap += struct.pack(">HHI", pl, en, off)
    for pl, en, body, _, _ in new_subs:
        cmap += body

    # append new cmap at end of file (parsers only follow the table directory)
    for i in range(nt):
        o = 12 + i * 16
        if data[o:o+4] == b"cmap":
            new_off = (len(data) + 3) & ~3          # 4-byte align
            data += b"\x00" * (new_off - len(data))
            data += cmap
            struct.pack_into(">I", data, o + 8, new_off)
            struct.pack_into(">I", data, o + 12, len(cmap))
            print("cmap new=%d bytes, appended at offset %d" % (len(cmap), new_off))

    # zero head.checkSumAdjustment for cleanliness
    for i in range(nt):
        o = 12 + i * 16
        if data[o:o+4] == b"head":
            head_off = struct.unpack(">I", data[o+8:o+12])[0]
            struct.pack_into(">I", data, head_off + 8, 0)

    open(dst, "wb").write(bytes(data))
    print("WROTE", dst, len(data), "bytes")


if __name__ == "__main__":
    main()
