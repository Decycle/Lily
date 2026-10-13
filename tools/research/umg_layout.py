#!/usr/bin/env python3
"""Lay out a cooked UE4.19 widget blueprint's tree (static, no animations) and list what it draws.

Usage: python -I -X utf8 umg_layout.py <BP_x.uasset> [--root W H] [--all] [--grep TEXT]

Walks the cooked widget tree (the archetype's `WidgetTree_0` when present, else the class's
tree), recursing into nested user widgets whose trees are cooked into the same package, and
computes for every Image / TextBlock its four corners in the root's coordinates with the origin
at the root's centre (default root 1080 x 1080, i.e. the menus' design units, y down).
Implements: CanvasPanelSlot anchors / offsets / alignment (cooked defaults: offsets
(0, 0, 100, 30), anchors 0, alignment 0), Overlay / Border / SizeBox-like single-child slots with
padding and H/V alignment, widget RenderTransform (scale, shear, angle, translation about the
pivot, UE order), RenderOpacity, ColorAndOpacity, Visibility (Collapsed / Hidden skipped unless
--all). Desired sizes: Image = Brush.ImageSize, panels = 0 (enough for the menus' "null" canvases
centred in borders). One line per drawable, back to front:
  index  path  kind  resource  tint(linear rgba, x opacity)  corners TL TR BR BL  [tiling/draw-as]
Read-only.
"""
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ue4_widget_dump import Package, Reader, obj_name  # noqa: E402

ALIGN = {"HAlign_Fill": None, "HAlign_Left": 0.0, "HAlign_Center": 0.5, "HAlign_Right": 1.0,
         "VAlign_Fill": None, "VAlign_Top": 0.0, "VAlign_Center": 0.5, "VAlign_Bottom": 1.0}


class Tree:
    def __init__(self, path):
        p = Package(path)
        r = Reader(p.ua, p.import_off)
        self.imports = []
        for _ in range(p.import_count):
            r.raw(16)
            r.i32()
            self.imports.append(p.read_fname(r))
        r = Reader(p.ua, p.export_off)
        p.exports = []
        for _ in range(p.export_count):
            cls, _sup, _tmpl, outer = struct.unpack("<iiii", r.raw(16))
            name = p.read_fname(r)
            r.u32()
            size, off = struct.unpack("<qq", r.raw(16))
            r.raw(104 - 16 - 8 - 4 - 16)
            p.exports.append(dict(cls=cls, outer=outer, name=name, size=size, off=off))
        self.p = p
        self.hdr = len(p.ua)
        self.cache = {}

    def name(self, i):
        return self.p.exports[i - 1]["name"] if i > 0 else self.imports[-i - 1]

    def cls(self, i):
        return obj_name(self.p, self.p.exports[i - 1]["cls"], self.imports) or "?"

    def props(self, i):
        if i not in self.cache:
            e = self.p.exports[i - 1]
            rr = Reader(self.p.ux, e["off"] - self.hdr)
            try:
                self.cache[i] = self.p.read_props(rr, e["off"] - self.hdr + e["size"])
            except Exception as ex:  # noqa: BLE001
                self.cache[i] = {"__error__": str(ex)}
        return self.cache[i]

    def root(self):
        trees = [i for i in range(1, len(self.p.exports) + 1) if self.cls(i) == "WidgetTree"]
        for i in trees:  # the archetype's cooked tree first
            outer = self.p.exports[i - 1]["outer"]
            if outer > 0 and self.name(outer) == "WidgetArchetype":
                return self.props(i).get("RootWidget", {}).get("object_index")
        for i in trees:
            outer = self.p.exports[i - 1]["outer"]
            if outer > 0 and self.cls(outer) == "WidgetBlueprintGeneratedClass":
                return self.props(i).get("RootWidget", {}).get("object_index")
        return None


# 2D affine: (a, b, c, d, tx, ty): x' = a x + c y + tx, y' = b x + d y + ty
def mul(m, n):
    """m after n."""
    a, b, c, d, tx, ty = n
    A, B, C, D, TX, TY = m
    return (A * a + C * b, B * a + D * b, A * c + C * d, B * c + D * d,
            A * tx + C * ty + TX, B * tx + D * ty + TY)


def translate(x, y):
    return (1, 0, 0, 1, x, y)


def apply(m, pt):
    a, b, c, d, tx, ty = m
    return (a * pt[0] + c * pt[1] + tx, b * pt[0] + d * pt[1] + ty)


def render_transform(props, size):
    rt = props.get("RenderTransform")
    if not rt:
        return translate(0, 0)
    sx, sy = rt.get("Scale", [1.0, 1.0])
    shx, shy = rt.get("Shear", [0.0, 0.0])
    ang = math.radians(rt.get("Angle", 0.0))
    tx, ty = rt.get("Translation", [0.0, 0.0])
    tan = lambda deg: 0.0 if deg == 0 else math.tan(math.radians(max(-89.0, min(89.0, deg))))
    scale = (sx, 0, 0, sy, 0, 0)
    shear = (1, tan(shy), tan(shx), 1, 0, 0)
    rot = (math.cos(ang), math.sin(ang), -math.sin(ang), math.cos(ang), 0, 0)
    m = mul(translate(tx, ty), mul(rot, mul(shear, scale)))
    px, py = props.get("RenderTransformPivot", [0.5, 0.5])
    pv = (px * size[0], py * size[1])
    return mul(translate(*pv), mul(m, translate(-pv[0], -pv[1])))


def desired(t, w):
    pr = t.props(w)
    if "Brush" in pr:
        return pr["Brush"].get("ImageSize", [32.0, 32.0])
    return [0.0, 0.0]


def single_child(size, child_size, pad, ha, va):
    pad = pad or {}
    l, tp = pad.get("Left", 0.0), pad.get("Top", 0.0)
    r, b = pad.get("Right", 0.0), pad.get("Bottom", 0.0)
    inner = (size[0] - l - r, size[1] - tp - b)
    out = []
    for k, al, lo in ((0, ha, l), (1, va, tp)):
        if al is None:
            out.append((lo, inner[k]))
        else:
            out.append((lo + (inner[k] - child_size[k]) * al, child_size[k]))
    return (out[0][0], out[1][0]), (out[0][1], out[1][1])


def walk(t, w, m, size, opacity, path, out, show_all, depth=0):
    if not w or w <= 0 or depth > 60:
        return
    pr = t.props(w)
    cls = t.cls(w)
    vis = pr.get("Visibility", "")
    hidden = "Collapsed" in vis or "Hidden" in vis
    if hidden and not show_all:
        return
    m = mul(m, render_transform(pr, size))
    opacity = opacity * pr.get("RenderOpacity", 1.0)
    path = path + "/" + t.name(w)
    if cls == "Image" or "TextBlock" in cls:
        c = pr.get("ColorAndOpacity", [1.0, 1.0, 1.0, 1.0])
        if isinstance(c, dict):
            c = c.get("SpecifiedColor", [1.0, 1.0, 1.0, 1.0])
        brush = pr.get("Brush", {})
        bt = brush.get("TintColor", {}).get("SpecifiedColor", [1.0, 1.0, 1.0, 1.0])
        tint = [c[i] * bt[i] for i in range(4)]
        tint[3] *= opacity
        res = brush.get("ResourceObject", {}).get("object_index")
        resname = t.name(res) if res else ("text:" + str(pr.get("Text", ""))[:40] if "TextBlock" in cls else "-")
        corners = [apply(m, p) for p in ((0, 0), (size[0], 0), (size[0], size[1]), (0, size[1]))]
        extra = " ".join(str(brush[k]).split("::")[-1] for k in ("Tiling", "DrawAs") if k in brush)
        out.append((path, cls, resname, tint, corners, extra + (" HIDDEN" if hidden else "")))
        return
    slots = pr.get("Slots") or []
    content = pr.get("Content")
    if cls.endswith("_C") and "WidgetTree" in pr:  # nested user widget
        root = t.props(pr["WidgetTree"]["object_index"]).get("RootWidget", {}).get("object_index")
        walk(t, root, m, size, opacity, path, out, show_all, depth + 1)
        return
    if not slots and content:
        slots = [content]
    if not slots and "Content" not in pr:
        # Border / SizeBox etc. keep their single slot under "Slots" too; nothing else to do.
        return
    for s in slots:
        si = s["object_index"]
        sp = t.props(si)
        child = sp.get("Content", {}).get("object_index")
        if not child:
            continue
        scls = t.cls(si)
        if scls == "CanvasPanelSlot":
            ld = sp.get("LayoutData", {})
            off = {"Left": 0.0, "Top": 0.0, "Right": 100.0, "Bottom": 30.0}
            off.update(ld.get("Offsets", {}))
            an = ld.get("Anchors", {})
            amin = an.get("Minimum", [0.0, 0.0])
            amax = an.get("Maximum", [0.0, 0.0])
            al = ld.get("Alignment", [0.0, 0.0])
            pos, csize = [0.0, 0.0], [0.0, 0.0]
            for k, (lo, hi) in enumerate((("Left", "Right"), ("Top", "Bottom"))):
                if amin[k] == amax[k]:
                    csize[k] = off[hi]
                    pos[k] = amin[k] * size[k] + off[lo] - al[k] * csize[k]
                else:
                    csize[k] = (amax[k] - amin[k]) * size[k] - off[lo] - off[hi]
                    pos[k] = amin[k] * size[k] + off[lo] - al[k] * csize[k]
        else:
            ha = ALIGN.get(str(sp.get("HorizontalAlignment", pr.get("HorizontalAlignment", "HAlign_Fill"))).split("::")[-1])
            va = ALIGN.get(str(sp.get("VerticalAlignment", pr.get("VerticalAlignment", "VAlign_Fill"))).split("::")[-1])
            if scls == "OverlaySlot":
                ha = ALIGN.get(str(sp.get("HorizontalAlignment", "HAlign_Left")).split("::")[-1])
                va = ALIGN.get(str(sp.get("VerticalAlignment", "VAlign_Top")).split("::")[-1])
            pos, csize = single_child(size, desired(t, child), sp.get("Padding"), ha, va)
        cm = mul(m, translate(*pos))
        walk(t, child, cm, csize, opacity, path, out, show_all, depth + 1)


def main(argv):
    if not argv:
        print(__doc__)
        return 1
    t = Tree(argv[0])
    w, h = 1080.0, 1080.0
    if "--root" in argv:
        i = argv.index("--root")
        w, h = float(argv[i + 1]), float(argv[i + 2])
    grep = argv[argv.index("--grep") + 1] if "--grep" in argv else None
    out = []
    walk(t, t.root(), translate(-w / 2, -h / 2), (w, h), 1.0, "", out, "--all" in argv)
    for i, (path, cls, res, tint, corners, extra) in enumerate(out):
        line = "%3d %s %s %s tint=(%s) %s %s" % (
            i, path, cls, res, ", ".join("%.3f" % v for v in tint),
            " ".join("(%.0f,%.0f)" % c for c in corners), extra)
        if grep is None or grep in line:
            print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
