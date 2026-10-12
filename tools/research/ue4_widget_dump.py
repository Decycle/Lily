#!/usr/bin/env python3
"""Dump UMG widget-tree exports (slots/images/text) from a cooked UE4.19 WidgetBlueprint.

Usage: python -I ue4_widget_dump.py <BP_x.uasset> [--all]
Prints, per export: name, class, and its tagged properties (LayoutData offsets/anchors,
Brush texture refs, RenderTransform...). --all also prints non-widget exports
(animations, MovieScene tracks). Read-only. Reuses ue4_datatable.Package for tagged props.
Export entry (UE4.19, 104 bytes): Class, Super, Template, Outer (i32 each), ObjectName FName,
 ObjectFlags u32, SerialSize i64, SerialOffset i64, 3x i32 bools, Guid[16], PackageFlags u32,
 2x i32 bools, FirstExportDependency i32 + 4x i32 dependency counts.
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ue4_datatable import Package as _Package, Reader  # noqa: E402


class Package(_Package):
    """FMargin is tag-serialized inside cooked UMG objects (size != 16), unlike DataTables."""

    def read_struct(self, r, sname, size):
        if sname == "Margin" and size not in (None, 16):
            return self.read_props(r, r.p + size)
        return super().read_struct(r, sname, size)

WIDGET_HINT = ("Slot", "Image", "TextBlock", "CanvasPanel", "Overlay", "Border", "Box", "Widget", "Button")


def obj_name(p, idx, imports):
    if idx == 0:
        return None
    if idx < 0:
        return imports[-idx - 1]
    return p.exports[idx - 1]["name"]


def main(path, show_all):
    p = Package(path)
    r = Reader(p.ua, p.import_off)
    imports = []
    for _ in range(p.import_count):
        r.raw(16)  # ClassPackage, ClassName FNames
        r.i32()    # Outer
        imports.append(p.read_fname(r))
    r = Reader(p.ua, p.export_off)
    p.exports = []
    for _ in range(p.export_count):
        cls, sup, tmpl, outer = struct.unpack("<iiii", r.raw(16))
        name = p.read_fname(r)
        r.u32()
        size, off = struct.unpack("<qq", r.raw(16))
        r.raw(104 - 16 - 8 - 4 - 16)
        p.exports.append(dict(cls=cls, outer=outer, name=name, size=size, off=off))
    hdr = len(p.ua)
    for e in p.exports:
        cname = obj_name(p, e["cls"], imports) or "?"
        if not show_all and not any(h in cname for h in WIDGET_HINT):
            continue
        rr = Reader(p.ux, e["off"] - hdr)
        try:
            props = p.read_props(rr, e["off"] - hdr + e["size"])
        except Exception as ex:  # noqa: BLE001
            props = {"__error__": str(ex)}
        print("## %s : %s (outer=%s)" % (e["name"], cname, obj_name(p, e["outer"], imports)))
        print(json.dumps(props, ensure_ascii=False, default=str)[:1500])


if __name__ == "__main__":
    main(sys.argv[1], "--all" in sys.argv)
