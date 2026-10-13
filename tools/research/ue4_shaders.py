#!/usr/bin/env python3
"""Extract and disassemble the compiled D3D11 shaders inlined in cooked UE4.19 material packages.

WACCA cooks with bShareMaterialShaderCode=False, so every UMaterial / UMaterialInstance .uexp carries its
own inline FMaterialShaderMap(s). Each FShaderResource's code is zlib-compressed (78 xx stream preceded by
`u32 uncompressed_size, u32 0, u32 compressed_size`); the decompressed blob is UE's D3D11 code container
(a small header with uniform-buffer names, then the DXBC container, then packed resource counts).

We do not parse the shader-map structure; instead every zlib stream is inflated and searched for `DXBC`,
and each blob is tagged with the nearest preceding FName reference (int32 name index + int32 0) whose name
looks like a shader type (T*/F* ...PS/VS/...) and vertex-factory type (*VertexFactory), and with the shader map's platform (the FMaterialShaderMap debug
description "Compiling <mat>: Platform=PCD3D_SM5, ..." precedes each map; every material has an SM5 and an
SM4 map, only SM5 is extracted - the game runs D3D11 SM5). Good enough to
pick the BasePass VS/PS of a given vertex factory.

Usage:
  python -I ue4_shaders.py extract <out_dir> <file.uasset|GlobalShaderCache-*.bin|dir> [...]   (writes <pkg>/<NN>_<type>_<vf>.{dxbc,asm} + index.json)
  python -I ue4_shaders.py disasm <file.dxbc>                             (prints disassembly)
Disassembly uses C:\\Windows\\System32\\d3dcompiler_47.dll D3DDisassemble via ctypes. Read-only on inputs.
"""
import ctypes
import hashlib
import json
import os
import re
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ue4_datatable import Package  # noqa: E402

_D3D = None


def disassemble(blob, flags=0):
    """D3DDisassemble(pSrc, size, flags, comments, ID3DBlob**) -> str."""
    global _D3D
    if _D3D is None:
        _D3D = ctypes.WinDLL(r"C:\Windows\System32\d3dcompiler_47.dll")
        _D3D.D3DDisassemble.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint, ctypes.c_char_p,
                                        ctypes.POINTER(ctypes.c_void_p)]
        _D3D.D3DDisassemble.restype = ctypes.c_long
    out = ctypes.c_void_p()
    buf = ctypes.create_string_buffer(blob, len(blob))
    hr = _D3D.D3DDisassemble(buf, len(blob), flags, None, ctypes.byref(out))
    if hr != 0 or not out.value:
        return "; D3DDisassemble failed hr=0x%08x\n" % (hr & 0xFFFFFFFF)
    vtbl = ctypes.cast(ctypes.cast(out, ctypes.POINTER(ctypes.c_void_p))[0], ctypes.POINTER(ctypes.c_void_p))
    proto_ptr = ctypes.WINFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p)
    proto_size = ctypes.WINFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p)
    proto_rel = ctypes.WINFUNCTYPE(ctypes.c_ulong, ctypes.c_void_p)
    ptr = proto_ptr(vtbl[3])(out)
    size = proto_size(vtbl[4])(out)
    text = ctypes.string_at(ptr, size).decode("latin-1").rstrip("\0")
    proto_rel(vtbl[2])(out)
    return text


def dxbc_kind(blob):
    """'vs'/'ps'/... from the SHEX/SHDR program-type token."""
    n = struct.unpack_from("<I", blob, 28)[0]
    for i in range(n):
        off = struct.unpack_from("<I", blob, 32 + 4 * i)[0]
        fourcc = blob[off:off + 4]
        if fourcc in (b"SHEX", b"SHDR"):
            tok = struct.unpack_from("<I", blob, off + 8)[0]
            return {0: "ps", 1: "vs", 2: "gs", 3: "hs", 4: "ds", 5: "cs"}.get(tok >> 16, "?%d" % (tok >> 16))
    return "?"


def name_refs(pkg, pattern):
    """Byte offsets in .uexp where an FName (idx, 0) of a name matching `pattern` is stored."""
    hits = []
    for idx, n in enumerate(pkg.names):
        if re.search(pattern, n):
            needle = struct.pack("<ii", idx, 0)
            for m in re.finditer(re.escape(needle), pkg.ux):
                hits.append((m.start(), n))
    hits.sort()
    return hits


def last_before(hits, off):
    best = None
    for o, n in hits:
        if o < off:
            best = n
        else:
            break
    return best


def scan(path):
    if path.endswith(".bin"):  # Engine/GlobalShaderCache-PCD3D_SM5.bin: type names are plain strings
        pkg = None
        with open(path, "rb") as fh:
            d = fh.read()
        types = [(m.start(), m.group(0).decode()) for m in
                 re.finditer(rb"[TF][A-Za-z0-9_]+(?:<[A-Za-z0-9_,]*>)?(?:PS|VS|GS|CS)(?:<[A-Za-z0-9_,]*>)?", d)]
        vfs = []
        plat = re.search(r"(PCD3D_\w+)", path)
        platforms = [(0, plat.group(1) if plat else None)]
    else:
        pkg = Package(path)
        d = pkg.ux
        types = name_refs(pkg, r"^(T|F)\w*(PS|VS|GS|HS|DS|CS)(<.*>)?$|^T\w+Policy|^TDepthOnly|^FHitProxy|^FVelocity")
        vfs = name_refs(pkg, r"VertexFactory$")
        platforms = [(m.start(), m.group(1).decode()) for m in re.finditer(rb"Platform=(PCD3D_\w+)", d)]
    out = []
    pos = 0
    for m in re.finditer(rb"\x78[\x01\x5e\x9c\xda]", d):
        o = m.start()
        if o < pos or o < 12:
            continue
        usize, zero, csize = struct.unpack_from("<IIi", d, o - 12)
        if zero != 0 or csize <= 0 or o + csize > len(d):
            continue
        try:
            raw = zlib.decompress(d[o:o + csize])
        except zlib.error:
            continue
        if len(raw) != usize:
            continue
        pos = o + csize
        k = raw.find(b"DXBC")
        if k < 0:
            continue
        total = struct.unpack_from("<I", raw, k + 24)[0]
        blob = raw[k:k + total]
        out.append({
            "uexp_offset": o,
            "kind": dxbc_kind(blob),
            "platform": last_before(platforms, o),
            "shader_type": last_before(types, o),
            "vertex_factory": last_before(vfs, o),
            "sha1": hashlib.sha1(blob).hexdigest()[:16],
            "size": len(blob),
            "blob": blob,
        })
    return pkg, out


def safe(s):
    return re.sub(r"[^A-Za-z0-9_]+", "_", s or "none")[:60]


def cmd_extract(out_dir, inputs, platform="PCD3D_SM5"):
    files = []
    for p in inputs:
        if os.path.isdir(p):
            for root, _, fs in os.walk(p):
                files += [os.path.join(root, f) for f in fs if f.endswith((".uasset", ".bin"))]
        else:
            files.append(p)
    index = {}
    for f in sorted(files):
        try:
            _, shaders = scan(f)
            shaders = [s for s in shaders if platform in (None, s["platform"])]
        except Exception as e:  # noqa: BLE001
            print("skip %s: %s" % (f, e))
            continue
        if not shaders:
            continue
        name = os.path.splitext(os.path.basename(f))[0]
        pdir = os.path.join(out_dir, name)
        os.makedirs(pdir, exist_ok=True)
        entries = []
        for i, s in enumerate(shaders):
            base = "%03d_%s_%s_%s" % (i, s["kind"], safe(s["shader_type"]), safe(s["vertex_factory"]))
            with open(os.path.join(pdir, base + ".dxbc"), "wb") as fh:
                fh.write(s["blob"])
            with open(os.path.join(pdir, base + ".asm"), "w", encoding="utf-8") as fh:
                fh.write(disassemble(s["blob"]))
            e = {k: v for k, v in s.items() if k != "blob"}
            e["file"] = base
            entries.append(e)
        index[name] = {"source": f, "shaders": entries}
        print("%-50s %3d shaders" % (name, len(entries)))
    with open(os.path.join(out_dir, "index.json"), "w", encoding="utf-8") as fh:
        json.dump(index, fh, indent=1)


def main(argv):
    if len(argv) >= 3 and argv[0] == "extract":
        cmd_extract(argv[1], argv[2:])
        return 0
    if len(argv) == 2 and argv[0] == "disasm":
        with open(argv[1], "rb") as fh:
            print(disassemble(fh.read()))
        return 0
    print(__doc__)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
