"""Dump every draw of a RenderDoc D3D11 capture to JSON (+ shader DXBC, textures, final image). Runs INSIDE
qrenderdoc's Python:

    set WACCA_RDC_DUMP=<capture.rdc>;<out_dir>
    qrenderdoc.exe --python tools\\research\\rdc_dump.py

Per draw (`draws.json`): eventId, action name, index/vertex/instance counts, VS/PS DXBC sha1 (files in
`<out>/shaders/<sha1>.dxbc`, match against ue4_shaders.py `index.json` to get the material), VS/PS constant
buffers (raw float4 rows, saved once per content in `<out>/cbuffers/<sha1>.bin` and referenced), PS SRVs
(resource id, size, format), blend / depth / raster state, viewport, render targets, vertex/index buffer bindings.
Also `textures.json` (all textures) and `<out>/tex/<id>.png` for every SRV a PS reads (first time seen),
`final.png` (back buffer at the end) and `rt/<eid>.png` for the render target after the last draw into it.
Read-only on the capture.
"""
import hashlib
import json
import os
import struct
import traceback

import renderdoc as rd

CAP, OUT = os.environ["WACCA_RDC_DUMP"].split(";")
os.makedirs(OUT, exist_ok=True)
for d in ("shaders", "cbuffers", "tex", "rt"):
    os.makedirs(os.path.join(OUT, d), exist_ok=True)
LOG = open(os.path.join(OUT, "dump.log"), "w", encoding="utf-8", buffering=1)


def log(*a):
    LOG.write(" ".join(str(x) for x in a) + "\n")


def rid(x):
    try:
        return int(x)
    except Exception:  # noqa: BLE001
        return str(x)


def enum_name(v):
    s = str(v)
    return s.split(".")[-1]


def save_blob(sub, data):
    h = hashlib.sha1(data).hexdigest()[:16]
    p = os.path.join(OUT, sub, h + (".dxbc" if sub == "shaders" else ".bin"))
    if not os.path.exists(p):
        with open(p, "wb") as f:
            f.write(data)
    return h


def flatten(actions):
    for a in actions:
        yield a
        yield from flatten(a.children)


def stage_info(ctl, st, saved_tex, texdesc):
    out = {}
    try:
        out["shader"] = rid(st.resourceId)
        refl = st.reflection
        if refl is not None and len(refl.rawBytes):
            out["dxbc"] = save_blob("shaders", bytes(refl.rawBytes))
    except Exception as e:  # noqa: BLE001
        out["shader_err"] = repr(e)
    cbs = []
    try:
        for slot, cb in enumerate(st.constantBuffers):
            res = cb.resourceId
            if res == rd.ResourceId.Null():
                continue
            off = cb.vecOffset * 16
            n = cb.vecCount * 16
            data = bytes(ctl.GetBufferData(res, off, n)) if n else bytes(ctl.GetBufferData(res, 0, 0))
            cbs.append({"slot": slot, "res": rid(res), "bytes": len(data), "blob": save_blob("cbuffers", data)})
    except Exception as e:  # noqa: BLE001
        out["cb_err"] = repr(e)
    out["cbs"] = cbs
    srvs = []
    try:
        for slot, v in enumerate(st.srvs):
            res = getattr(v, "resourceResourceId", None) or getattr(v, "resource", None)
            if res is None or res == rd.ResourceId.Null():
                continue
            e = {"slot": slot, "res": rid(res)}
            td = texdesc.get(rid(res))
            if td is not None:
                e.update(td)
                if rid(res) not in saved_tex and td.get("dim") == 2:
                    saved_tex.add(rid(res))
                    try:
                        s = rd.TextureSave()
                        s.resourceId = res
                        s.destType = rd.FileType.PNG
                        s.mip = 0
                        s.alpha = rd.AlphaMapping.Preserve
                        ctl.SaveTexture(s, os.path.join(OUT, "tex", "%s.png" % rid(res)))
                    except Exception as ex:  # noqa: BLE001
                        e["save_err"] = repr(ex)
            srvs.append(e)
    except Exception as e:  # noqa: BLE001
        out["srv_err"] = repr(e)
    out["srvs"] = srvs
    try:
        out["samplers"] = [{"slot": i, "addr": [enum_name(s.addressU), enum_name(s.addressV)],
                            "filter": str(s.filter.minify) + "/" + str(s.filter.magnify)}
                           for i, s in enumerate(st.samplers) if s.resourceId != rd.ResourceId.Null()]
    except Exception as e:  # noqa: BLE001
        out["sampler_err"] = repr(e)
    return out


def blend_info(om):
    out = []
    try:
        for i, b in enumerate(om.blendState.blends):
            if i > 0 and not b.enabled:
                continue
            out.append({"rt": i, "enabled": b.enabled,
                        "color": [enum_name(b.colorBlend.source), enum_name(b.colorBlend.destination),
                                  enum_name(b.colorBlend.operation)],
                        "alpha": [enum_name(b.alphaBlend.source), enum_name(b.alphaBlend.destination),
                                  enum_name(b.alphaBlend.operation)],
                        "mask": int(b.writeMask)})
    except Exception as e:  # noqa: BLE001
        out.append({"err": repr(e)})
    return out


def main():
    cap = rd.OpenCaptureFile()
    res = cap.OpenFile(CAP, "", None)
    if res != rd.ResultCode.Succeeded:
        log("open failed", res)
        return
    res, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    if res != rd.ResultCode.Succeeded:
        log("replay failed", res)
        return
    texdesc = {}
    for t in ctl.GetTextures():
        texdesc[rid(t.resourceId)] = {"w": t.width, "h": t.height, "depth": t.depth, "mips": t.mips,
                                      "fmt": t.format.Name(), "dim": t.dimension, "ms": t.msSamp}
    names = {}
    for r in ctl.GetResources():
        names[rid(r.resourceId)] = r.name
    with open(os.path.join(OUT, "textures.json"), "w") as f:
        json.dump({str(k): dict(v, name=names.get(k)) for k, v in texdesc.items()}, f, indent=0)
    saved_tex = set()
    draws = []
    actions = list(flatten(ctl.GetRootActions()))
    last_rt_eid = {}
    for a in actions:
        flags = a.flags
        is_draw = bool(flags & rd.ActionFlags.Drawcall)
        is_clear = bool(flags & rd.ActionFlags.Clear)
        is_other = bool(flags & (rd.ActionFlags.Dispatch | rd.ActionFlags.Copy | rd.ActionFlags.Resolve |
                                 rd.ActionFlags.Present))
        if not (is_draw or is_clear or is_other):
            continue
        name = a.GetName(ctl.GetStructuredFile())
        ctl.SetFrameEvent(a.eventId, True)
        d = {"eid": a.eventId, "name": name, "flags": int(flags)}
        if is_draw:
            d.update(indices=a.numIndices, instances=a.numInstances, index_offset=a.indexOffset,
                     base_vertex=a.baseVertex, vertex_offset=a.vertexOffset)
            try:
                s = ctl.GetD3D11PipelineState()
                d["vs"] = stage_info(ctl, s.vertexShader, saved_tex, texdesc)
                d["ps"] = stage_info(ctl, s.pixelShader, saved_tex, texdesc)
                om = s.outputMerger
                d["blend"] = blend_info(om)
                try:
                    ds = om.depthStencilState
                    d["depth"] = {"enable": ds.depthEnable, "func": enum_name(ds.depthFunction),
                                  "write": ds.depthWrites, "stencil": ds.stencilEnable}
                except Exception as e:  # noqa: BLE001
                    d["depth"] = {"err": repr(e)}
                try:
                    rs = s.rasterizer.state
                    d["raster"] = {"cull": enum_name(rs.cullMode), "front_ccw": rs.frontCCW,
                                   "fill": enum_name(rs.fillMode), "ms": rs.multisampleEnable}
                    vp = s.rasterizer.viewports[0]
                    d["viewport"] = [vp.x, vp.y, vp.width, vp.height, vp.minDepth, vp.maxDepth]
                except Exception as e:  # noqa: BLE001
                    d["raster"] = {"err": repr(e)}
                try:
                    rts = []
                    for v in om.renderTargets:
                        r = getattr(v, "resourceResourceId", None) or getattr(v, "resource", None)
                        if r is not None and r != rd.ResourceId.Null():
                            rts.append(rid(r))
                    d["rts"] = rts
                    for r in rts:
                        last_rt_eid[r] = a.eventId
                    dv = om.depthTarget
                    r = getattr(dv, "resourceResourceId", None) or getattr(dv, "resource", None)
                    d["dsv"] = rid(r) if r is not None and r != rd.ResourceId.Null() else None
                except Exception as e:  # noqa: BLE001
                    d["rt_err"] = repr(e)
                try:
                    ia = s.inputAssembly
                    d["topology"] = enum_name(ia.topology)
                    d["ib"] = {"res": rid(ia.indexBuffer.resourceId), "off": ia.indexBuffer.byteOffset,
                               "fmt": ia.indexBuffer.byteStride if hasattr(ia.indexBuffer, "byteStride") else None}
                    d["vbs"] = [{"slot": i, "res": rid(vb.resourceId), "stride": vb.byteStride,
                                 "off": vb.byteOffset}
                                for i, vb in enumerate(ia.vertexBuffers) if vb.resourceId != rd.ResourceId.Null()]
                except Exception as e:  # noqa: BLE001
                    d["ia_err"] = repr(e)
            except Exception:  # noqa: BLE001
                d["err"] = traceback.format_exc()
        draws.append(d)
    with open(os.path.join(OUT, "draws.json"), "w") as f:
        json.dump(draws, f, indent=0)
    log("draws", len(draws))
    # render targets after their last draw
    for r, eid in last_rt_eid.items():
        try:
            ctl.SetFrameEvent(eid, True)
            s = rd.TextureSave()
            s.resourceId = [x for x in ctl.GetTextures() if rid(x.resourceId) == r][0].resourceId
            s.destType = rd.FileType.PNG
            s.alpha = rd.AlphaMapping.Discard
            ctl.SaveTexture(s, os.path.join(OUT, "rt", "%s_after_%d.png" % (r, eid)))
        except Exception as e:  # noqa: BLE001
            log("rt save", r, repr(e))
    ctl.Shutdown()
    cap.Shutdown()
    log("done")


try:
    main()
except Exception:  # noqa: BLE001
    log(traceback.format_exc())
LOG.close()
os._exit(0)
