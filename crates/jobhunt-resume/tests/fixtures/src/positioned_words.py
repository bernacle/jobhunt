#!/usr/bin/env python3
"""Writes ../positioned_words.pdf: a two-page resume laid out the way TeX
and some PDF exporters do it. Every word is drawn on its own at an
absolute position (there are no space characters in the file), list
bullets are real glyphs, one bullet wraps onto a second line, dates are
right-aligned, and every page has a running footer with the page number.
Standard Helvetica (no embedded font), WinAnsi encoding.

Run from this directory: python3 positioned_words.py
"""

WIDTHS = {32: 278, 33: 278, 34: 355, 35: 556, 36: 556, 37: 889, 38: 667, 39: 191, 40: 333, 41: 333, 42: 389, 43: 584, 44: 278, 45: 333, 46: 278, 47: 278, 48: 556, 49: 556, 50: 556, 51: 556, 52: 556, 53: 556, 54: 556, 55: 556, 56: 556, 57: 556, 58: 278, 59: 278, 60: 584, 61: 584, 62: 584, 63: 556, 64: 1015, 65: 667, 66: 667, 67: 722, 68: 722, 69: 667, 70: 611, 71: 778, 72: 722, 73: 278, 74: 500, 75: 667, 76: 556, 77: 833, 78: 722, 79: 778, 80: 667, 81: 778, 82: 722, 83: 667, 84: 611, 85: 722, 86: 667, 87: 944, 88: 667, 89: 667, 90: 611, 91: 278, 92: 278, 93: 278, 94: 469, 95: 556, 96: 333, 97: 556, 98: 556, 99: 500, 100: 556, 101: 556, 102: 278, 103: 556, 104: 556, 105: 222, 106: 222, 107: 500, 108: 222, 109: 833, 110: 556, 111: 556, 112: 556, 113: 556, 114: 333, 115: 500, 116: 278, 117: 556, 118: 500, 119: 722, 120: 500, 121: 500, 122: 500, 123: 334, 124: 260, 125: 334, 126: 584, 149: 350, 150: 556, 151: 1000, 183: 278}  # Helvetica advance widths (1/1000 em), WinAnsi codes

PAGES = [
    [
        (18, 72, 780, "Ana Lima"),
        (10, 72, 760, "Backend Engineer \xb7 Lisbon, Portugal \xb7 ana@example.org"),
        (12, 72, 725, "EXPERIENCE"),
        (10, 72, 705, "Acme Payments"),
        (10, None, 705, "Jan 2021 \x96 Present"),
        (10, 72, 691, "Staff Software Engineer"),
        (10, 82, 673, "\x95 Led the redesign of the settlement service, cutting batch time by 60%."),
        (10, 82, 659, "\x95 Mentored five engineers and introduced design reviews for every service"),
        (10, 90, 645, "that handles money movement."),
        (10, 72, 615, "Globex"),
        (10, None, 615, "2015 \x96 2020"),
        (10, 72, 601, "Backend Developer"),
        (10, 82, 583, "\x95 Maintained billing APIs in Python and Django for 40 countries."),
    ],
    [
        (12, 72, 780, "EDUCATION"),
        (10, 72, 760, "University of Porto \x97 BSc in Informatics"),
        (10, None, 760, "2012 \x96 2015"),
        (12, 72, 725, "SKILLS"),
        (10, 72, 705, "Languages: Go, Python, SQL"),
    ],
]
RIGHT_EDGE = 540


def width(text, size):
    return sum(WIDTHS.get(ord(c), 556) for c in text) * size / 1000


def draw(size, x, y, text):
    """One BT/ET block per word; x=None right-aligns the text."""
    if x is None:
        x = RIGHT_EDGE - width(text, size)
    ops = []
    space = WIDTHS[32] * size / 1000
    for word in text.split(" "):
        escaped = word.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
        ops.append(f"BT /F1 {size} Tf {x:.2f} {y} Td ({escaped}) Tj ET")
        x += width(word, size) + space
    return ops


def build():
    objects = []

    def add(body):
        objects.append(body)
        return len(objects)

    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
    pages_id = len(objects) + 1 + 2 * len(PAGES)
    kids = []
    for number, lines in enumerate(PAGES, start=1):
        ops = []
        for size, x, y, text in lines:
            ops += draw(size, x, y, text)
        ops += draw(8, 72, 40, f"Ana Lima \x97 page {number} of {len(PAGES)}")
        stream = "\n".join(ops).encode("latin-1")
        content = add(b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream")
        kids.append(add(
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 842] "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>" % (pages_id, font, content)
        ))
    assert add(b"<< /Type /Pages /Kids [%s] /Count %d >>" % (
        b" ".join(b"%d 0 R" % k for k in kids), len(kids))) == pages_id
    catalog = add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id)

    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objects) + 1, catalog, xref)
    return bytes(out)


if __name__ == "__main__":
    with open("../positioned_words.pdf", "wb") as f:
        f.write(build())
