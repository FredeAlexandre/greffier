"""Builds Greffier's mark (the cat peeking over the meeting table) as SVG files in `sources/`.

    python3 assets/brand/tools/marks.py

The mark is drawn on a 256 × 256 canvas from a few primitives: the head is one polygon with filleted
corners (two ears, a rounded dome cut flat at the bottom), the table is a rounded bar, the eyes are
holes. Each file holds a single black path; `render.swift` recolours it.

Writes:
    sources/greffier-mark.svg                    eyes closed (the cat listens): app icon, logo
    sources/greffier-mark-recording.svg          eyes open
    sources/greffier-mark-small.svg              small-size cut (≤ 32 px, menu bar): bigger gap, thicker eyes
    sources/greffier-mark-small-recording.svg    small-size cut, eyes open
"""
import math
from pathlib import Path

SOURCES = Path(__file__).resolve().parent.parent / "sources"


def fmt(v):
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def polar(c, r, deg):
    a = math.radians(deg)
    return (c[0] + r * math.cos(a), c[1] + r * math.sin(a))


class Contour:
    """A closed contour of lines and circular arcs (angles in degrees, y down: increasing = clockwise)."""

    def __init__(self, start):
        self.start, self.segs = start, []

    def line(self, p):
        self.segs.append(("L", p))
        return self

    def arc(self, c, r, a0, a1):
        self.segs.append(("A", c, r, a0, a1))
        return self

    def _points(self):
        pts = [self.start]
        for s in self.segs:
            if s[0] == "L":
                pts.append(s[1])
            else:
                _, c, r, a0, a1 = s
                n = max(4, int(abs(a1 - a0) / 3))
                pts += [polar(c, r, a0 + (a1 - a0) * i / n) for i in range(1, n + 1)]
        return pts

    def clockwise(self):
        p = self._points()
        return sum(p[i][0] * p[i - 1][1] - p[i - 1][0] * p[i][1] for i in range(len(p))) < 0

    def reversed(self):
        nodes, cur = [], self.start
        for s in self.segs:
            nodes.append((cur, s))
            cur = s[1] if s[0] == "L" else polar(s[1], s[2], s[4])
        r = Contour(cur)
        for before, s in reversed(nodes):
            r.line(before) if s[0] == "L" else r.arc(s[1], s[2], s[4], s[3])
        return r

    def oriented(self, clockwise):
        """Filled shapes run clockwise, holes counter-clockwise (non-zero fill rule)."""
        return self if self.clockwise() == clockwise else self.reversed()

    def d(self):
        out, cur = [f"M{fmt(self.start[0])} {fmt(self.start[1])}"], self.start
        for s in self.segs:
            if s[0] == "L":
                p = s[1]
                if math.dist(p, cur) > 1e-6:
                    out.append(f"L{fmt(p[0])} {fmt(p[1])}")
                cur = p
            else:
                _, c, r, a0, a1 = s
                spans = [(a0, (a0 + a1) / 2), ((a0 + a1) / 2, a1)] if abs(a1 - a0) >= 359.99 else [(a0, a1)]
                for x0, x1 in spans:
                    e = polar(c, r, x1)
                    out.append(f"A{fmt(r)} {fmt(r)} 0 {int(abs(x1 - x0) > 180)} {int(x1 > x0)} {fmt(e[0])} {fmt(e[1])}")
                cur = polar(c, r, a1)
        return "".join(out) + "Z"


def circle(c, r):
    return Contour(polar(c, r, 0)).arc(c, r, 0, 360)


def filleted(points, radii):
    """Polygon (convex or concave corners) with every corner rounded by its radius."""
    n = len(points)
    area = sum(points[i][0] * points[(i + 1) % n][1] - points[(i + 1) % n][0] * points[i][1] for i in range(n))
    if area < 0:
        points, radii = points[::-1], radii[::-1]
    corners = []
    for i in range(n):
        p0, p1, p2 = points[i - 1], points[i], points[(i + 1) % n]
        u1 = ((p0[0] - p1[0]) / math.dist(p0, p1), (p0[1] - p1[1]) / math.dist(p0, p1))
        u2 = ((p2[0] - p1[0]) / math.dist(p2, p1), (p2[1] - p1[1]) / math.dist(p2, p1))
        ang = math.acos(max(-1, min(1, u1[0] * u2[0] + u1[1] * u2[1])))
        r = radii[i]
        t1 = (p1[0] + u1[0] * r / math.tan(ang / 2), p1[1] + u1[1] * r / math.tan(ang / 2))
        t2 = (p1[0] + u2[0] * r / math.tan(ang / 2), p1[1] + u2[1] * r / math.tan(ang / 2))
        bis = (u1[0] + u2[0], u1[1] + u2[1])
        k = r / math.sin(ang / 2) / math.hypot(*bis)
        c = (p1[0] + bis[0] * k, p1[1] + bis[1] * k)
        a0 = math.degrees(math.atan2(t1[1] - c[1], t1[0] - c[0]))
        a1 = math.degrees(math.atan2(t2[1] - c[1], t2[0] - c[0]))
        convex = (p1[0] - p0[0]) * (p2[1] - p1[1]) - (p1[1] - p0[1]) * (p2[0] - p1[0]) > 0
        if convex:
            while a1 < a0: a1 += 360
            while a1 - a0 > 180: a1 -= 360
        else:
            while a1 > a0: a1 -= 360
            while a0 - a1 > 180: a1 += 360
        corners.append((t1, c, r, a0, a1))
    s = Contour(corners[0][0])
    for i, (t1, c, r, a0, a1) in enumerate(corners):
        if i:
            s.line(t1)
        s.arc(c, r, a0, a1)
    return s.line(corners[0][0])


def stroke_arc(c, r, w, a0, a1):
    """A stroke of width w along an arc (a0 < a1), as an outline with round caps."""
    h = w / 2
    return (Contour(polar(c, r + h, a0)).arc(c, r + h, a0, a1)
            .arc(polar(c, r, a1), h, a1, a1 + 180)
            .arc(c, r - h, a1, a0)
            .arc(polar(c, r, a0), h, a0 + 180, a0 + 360))


def mark(eyes_open=False, small=False):
    dy = 2 if small else 6          # optical centre: the whole mark sits a little above the middle
    gap = 22 if small else 14       # between the head and the table
    eye_w = 17 if small else 13     # closed eyes: stroke width
    head = filleted(
        [(60, 30 + dy), (104, 76 + dy), (152, 76 + dy), (196, 30 + dy),   # ear, notch, notch, ear
         (232, 116 + dy), (228, 156 + dy), (28, 156 + dy), (24, 116 + dy)],  # dome, cut flat at the bottom
        [12, 8, 8, 12, 90, 3, 3, 90])
    table_y = 156 + gap + dy
    table = filleted([(20, table_y), (236, table_y), (236, table_y + 36), (20, table_y + 36)], [18] * 4)
    if eyes_open:
        eyes = [circle((128 + s * 36, 126 + dy), 15 if small else 12) for s in (-1, 1)]
    else:
        eyes = [stroke_arc((128 + s * 36, 122 + dy), 17, eye_w, 25, 155) for s in (-1, 1)]
    contours = [head.oriented(True), table.oriented(True)] + [e.oriented(False) for e in eyes]
    title = "Greffier" + (" (enregistrement)" if eyes_open else "")
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256" role="img">'
            f'<title>{title}</title><path fill="#000000" d="{"".join(c.d() for c in contours)}"/></svg>\n')


if __name__ == "__main__":
    SOURCES.mkdir(exist_ok=True)
    for name, kw in [("greffier-mark", {}), ("greffier-mark-recording", {"eyes_open": True}),
                     ("greffier-mark-small", {"small": True}),
                     ("greffier-mark-small-recording", {"small": True, "eyes_open": True})]:
        (SOURCES / f"{name}.svg").write_text(mark(**kw))
    print(f"wrote {SOURCES}")
