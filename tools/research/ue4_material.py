#!/usr/bin/env python3
"""Dump a cooked UE4.19 Material / MaterialInstanceConstant / MaterialFunction / MaterialParameterCollection.

The cooked packages keep the material expression objects, so the node graph can be printed:
one line per expression (class, name, parameters, inputs as `<- ExpressionName[:output]`).

Usage: python -I -X utf8 ue4_material.py <x.uasset> [--json | --uniforms]
  --uniforms prints the FUniformExpressionSet(s): which Material uniform-buffer float4 slot / component
  (cb `Material` = the material's own constant buffer in the disassembly) holds which parameter expression.
Cooked FExpressionInput layout (40 bytes): OutputIndex i32, InputName FName, Mask i32, MaskR/G/B/A i32,
ExpressionName FName; FMaterialInput<T> appends UseConstant (i32 bool) + Constant (FColor 4 / float 4 /
FVector 12 / FVector2D 8). Read-only.
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ue4_datatable import Reader, fmt_float  # noqa: E402
from ue4_widget_dump import Package as _Package  # noqa: E402

_INPUT_EXTRA = {"ExpressionInput": 0, "ColorMaterialInput": 8, "ScalarMaterialInput": 8,
                "VectorMaterialInput": 16, "Vector2MaterialInput": 12, "MaterialAttributesInput": 4}


class Package(_Package):
    def read_struct(self, r, sname, size):
        if sname in _INPUT_EXTRA and size in (None, 40 + _INPUT_EXTRA[sname]):
            out_idx = r.i32()
            r.raw(8)  # InputName
            mask = [r.i32() for _ in range(5)]
            expr = self.read_fname(r)
            v = {"expr": expr, "out": out_idx}
            if mask[0]:
                v["mask"] = "".join(c for c, m in zip("RGBA", mask[1:]) if m)
            extra = _INPUT_EXTRA[sname]
            if extra:
                use = r.i32()
                raw = r.raw(extra - 4)
                if use:
                    v["const"] = [fmt_float(x) for x in struct.unpack("<%df" % (len(raw) // 4), raw)] \
                        if sname != "ColorMaterialInput" else raw.hex()
            return v
        if sname == "FunctionExpressionInput" or sname == "FunctionExpressionOutput":
            return self.read_props(r, None if size is None else r.p + size)
        return super().read_struct(r, sname, size)


def load(path):
    p = Package(path)
    r = Reader(p.ua, p.import_off)
    imports = []
    for _ in range(p.import_count):
        r.raw(16)
        r.i32()
        imports.append(p.read_fname(r))
    r = Reader(p.ua, p.export_off)
    exports = []
    for _ in range(p.export_count):
        cls, _sup, _tmpl, outer = struct.unpack("<iiii", r.raw(16))
        name = p.read_fname(r)
        r.u32()
        size, off = struct.unpack("<qq", r.raw(16))
        r.raw(104 - 16 - 8 - 4 - 16)
        exports.append(dict(cls=cls, outer=outer, name=name, size=size, off=off))

    def ref(idx):
        if idx == 0:
            return None
        return imports[-idx - 1] if idx < 0 else exports[idx - 1]["name"]

    hdr = len(p.ua)
    objs = []
    for e in exports:
        rr = Reader(p.ux, e["off"] - hdr)
        try:
            props = p.read_props(rr, e["off"] - hdr + e["size"])
        except Exception as ex:  # noqa: BLE001
            props = {"__error__": str(ex)}
        objs.append({"name": e["name"], "class": ref(e["cls"]), "props": resolve(props, ref)})
    return objs


def resolve(v, ref):
    if isinstance(v, dict):
        if set(v) == {"object_index"}:
            return ref(v["object_index"])
        return {k: resolve(x, ref) for k, x in v.items()}
    if isinstance(v, list):
        return [resolve(x, ref) for x in v]
    return v


SKIP = {"EditorX", "EditorY", "MaterialExpressionEditorX", "MaterialExpressionEditorY", "MaterialExpressionGuid",
        "ExpressionGUID", "bCollapsed", "Material", "Function", "Desc", "bCommentBubbleVisible", "Outputs",
        "ExpressionInputId", "ExpressionOutputId", "StateId", "Id", "ParameterId", "bRealtimePreview"}


def short(v):
    if isinstance(v, dict) and "expr" in v:
        s = "<- " + str(v["expr"])
        if v.get("out"):
            s += ":%d" % v["out"]
        if v.get("mask"):
            s += "." + v["mask"]
        if "const" in v:
            s += " const=" + str(v["const"])
        return s
    if isinstance(v, dict):
        return "{" + ", ".join("%s: %s" % (k, short(x)) for k, x in v.items() if k not in SKIP) + "}"
    if isinstance(v, list):
        return "[" + ", ".join(short(x) for x in v) + "]"
    return str(v)


# ---------------------------------------------------------------- uniform expression set (Material UB layout)
# FUniformExpressionSet (4.19 cooked): vector exprs, scalar exprs, 2D textures, then 2 more texture arrays
# (cube, external; empty in this game) and ParameterCollections (TArray<FGuid>).
# Material uniform buffer: float4 slot i = vector expr i; scalars packed after them, 4 per slot.

class _UErr(Exception):
    pass


def _param_info(p, r):
    name = p.read_fname(r)
    r.u8()  # Association
    idx = r.i32()
    return name if idx == -1 else "%s[%d]" % (name, idx)


def read_uexpr(p, r):
    t = p.read_fname(r)
    if not t.startswith("FMaterialUniformExpression"):
        raise _UErr("not an expression type: %s at 0x%x" % (t, r.p))
    t = t[len("FMaterialUniformExpression"):]
    if t == "VectorParameter":
        n = _param_info(p, r)
        return "Vec(%s=%s)" % (n, [fmt_float(r.f32()) for _ in range(4)])
    if t == "ScalarParameter":
        n = _param_info(p, r)
        return "Scalar(%s=%s)" % (n, fmt_float(r.f32()))
    if t == "Constant":
        v = [fmt_float(r.f32()) for _ in range(4)]
        r.u8()
        return "Const(%s)" % v
    if t == "ComponentSwizzle":
        x = read_uexpr(p, r)
        idx = list(r.raw(4))
        n = r.u8()
        return "%s.%s" % (x, "".join("xyzw"[i] for i in idx[:n] if i < 4))
    if t == "FoldedMath":
        a = read_uexpr(p, r)
        b = read_uexpr(p, r)
        op = r.u8()
        r.u32()  # value type
        return "(%s %s %s)" % (a, {0: "+", 1: "-", 2: "*", 3: "/", 4: "dot"}.get(op, "op%d" % op), b)
    if t in ("Abs", "Ceil"):
        return "%s(%s)" % (t.lower(), read_uexpr(p, r))
    if t in ("Fmod", "Max"):
        return "%s(%s, %s)" % (t.lower(), read_uexpr(p, r), read_uexpr(p, r))
    if t == "Clamp":
        return "clamp(%s, %s, %s)" % (read_uexpr(p, r), read_uexpr(p, r), read_uexpr(p, r))
    if t == "AppendVector":
        a, b = read_uexpr(p, r), read_uexpr(p, r)
        r.u32()
        return "append(%s, %s)" % (a, b)
    if t == "TrigMath":
        x = read_uexpr(p, r)
        op = r.u8()
        return "%s(%s)" % ({0: "sin", 1: "cos", 2: "tan", 3: "asin", 4: "acos", 5: "atan"}.get(op, "trig%d" % op), x)
    if t in ("Time", "RealTime"):
        return t
    if t == "Texture":
        return "Tex(index=%d, sampler_src=%d)" % (r.i32(), r.i32())
    if t == "TextureParameter":
        n = _param_info(p, r)
        return "TexParam(%s, index=%d, sampler_src=%d)" % (n, r.i32(), r.i32())
    raise _UErr("unhandled uniform expression %s" % t)


def uniform_sets(path):
    """All FUniformExpressionSets in the package (one per inline FMaterialResource)."""
    p = _Package(path)
    d = p.ux
    type_idx = {i for i, n in enumerate(p.names) if n.startswith("FMaterialUniformExpression")}
    out, pos = [], 0
    while pos < len(d) - 12:
        cnt, ti, num = struct.unpack_from("<iii", d, pos)
        if 0 < cnt < 256 and ti in type_idx and num == 0:
            r = Reader(d, pos)
            try:
                arrays = []
                for _ in range(3):
                    arrays.append([read_uexpr(p, r) for _ in range(r.i32())])
                for _ in range(2):
                    if r.i32() != 0:
                        raise _UErr("non-empty extra texture/per-frame array")
                ncol = r.i32()
                cols = [r.raw(16).hex() for _ in range(ncol)]
                out.append({"offset": pos, "vectors": arrays[0], "scalars": arrays[1], "textures2d": arrays[2],
                            "collections": cols})
                pos = r.p
                continue
            except (_UErr, Exception):  # noqa: BLE001
                pass  # false positive start; keep scanning
        pos += 1
    return out


def print_uniform_sets(path):
    for s in uniform_sets(path):
        nv = len(s["vectors"])
        print("uniform set @0x%x (Material UB: %d vector slots, %d scalars, %d textures, %d MPCs)"
              % (s["offset"], nv, len(s["scalars"]), len(s["textures2d"]), len(s["collections"])))
        for i, v in enumerate(s["vectors"]):
            print("  Material[%d]      = %s" % (i, v))
        for j, v in enumerate(s["scalars"]):
            print("  Material[%d].%s    = %s" % (nv + j // 4, "xyzw"[j % 4], v))
        for k, v in enumerate(s["textures2d"]):
            print("  Texture2D #%d     = %s" % (k, v))


def main(argv):
    if not argv:
        print(__doc__)
        return 1
    if "--uniforms" in argv:
        print_uniform_sets(argv[0])
        return 0
    objs = load(argv[0])
    if "--json" in argv:
        print(json.dumps(objs, ensure_ascii=False, indent=1, default=str))
        return 0
    for o in objs:
        props = {k: v for k, v in o["props"].items() if k not in SKIP}
        if o["class"] in ("Material", "MaterialFunction") and "Expressions" in props:
            props["Expressions"] = "%d refs" % len(props["Expressions"])
        if "FunctionExpressions" in props:
            props["FunctionExpressions"] = "%d refs" % len(props["FunctionExpressions"])
        print("%-48s %-40s %s" % (o["name"], o["class"], short(props)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
