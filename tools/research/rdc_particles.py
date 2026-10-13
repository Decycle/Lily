"""Dump particle draws of a RenderDoc D3D11 capture: per draw the raw vertex-buffer record(s) of slot 0 (Cascade
mesh particles: one 96-byte FMeshParticleInstanceVertex per draw = colour, 3 transform rows, velocity, SubUV,
relative time) and the post-VS output (positions + all VS outputs, raw floats with the output signature).
Runs INSIDE qrenderdoc's Python:

    set WACCA_RDC_PARTICLES=<capture.rdc>;<out.json>;<eid>,<eid>,...
    qrenderdoc.exe --python tools\\research\\rdc_particles.py

Read-only on the capture.
"""
import json
import os
import struct
import traceback

import renderdoc as rd

CAP, OUT, EIDS = os.environ["WACCA_RDC_PARTICLES"].split(";")
EIDS = [int(x) for x in EIDS.split(",") if x]
LOG = open(OUT + ".log", "w", encoding="utf-8", buffering=1)


def log(*a):
    LOG.write(" ".join(str(x) for x in a) + "\n")


def floats(data):
    n = len(data) // 4
    return list(struct.unpack("<%df" % n, data[: n * 4]))


def dump_draw(ctl, eid):
    ctl.SetFrameEvent(eid, True)
    d = {"eid": eid}
    st = ctl.GetD3D11PipelineState()
    vbs = st.inputAssembly.vertexBuffers
    if len(vbs) and vbs[0].resourceId != rd.ResourceId.Null():
        vb = vbs[0]
        size = vb.byteStride if vb.byteStride else 96
        d["vb0"] = {"stride": vb.byteStride, "off": vb.byteOffset,
                    "floats": floats(bytes(ctl.GetBufferData(vb.resourceId, vb.byteOffset, 4 * size)))}
    pipe = ctl.GetPipelineState()
    refl = pipe.GetShaderReflection(rd.ShaderStage.Vertex)
    d["vs_out"] = [{"name": s.semanticName, "index": s.semanticIndex, "comps": s.compCount, "reg": s.regIndex,
                    "sys": str(s.systemValue)} for s in refl.outputSignature]
    pv = ctl.GetPostVSData(0, 0, rd.MeshDataStage.VSOut)
    d["post_vs"] = {"stride": pv.vertexByteStride, "count": pv.numIndices}
    raw = bytes(ctl.GetBufferData(pv.vertexResourceId, pv.vertexByteOffset, 0))
    d["post_vs"]["floats"] = floats(raw[: pv.vertexByteStride * max(pv.numIndices, 1) * 2])
    if pv.indexResourceId != rd.ResourceId.Null():
        ib = bytes(ctl.GetBufferData(pv.indexResourceId, pv.indexByteOffset, 0))
        fmt = "<%dI" if pv.indexByteStride == 4 else "<%dH"
        n = min(len(ib) // max(pv.indexByteStride, 1), pv.numIndices)
        d["post_vs"]["indices"] = list(struct.unpack(fmt % n, ib[: n * pv.indexByteStride]))
    return d


def main():
    cap = rd.OpenCaptureFile()
    if cap.OpenFile(CAP, "", None) != rd.ResultCode.Succeeded:
        log("open failed")
        return
    res, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    if res != rd.ResultCode.Succeeded:
        log("replay failed", res)
        return
    out = []
    for eid in EIDS:
        try:
            out.append(dump_draw(ctl, eid))
        except Exception:  # noqa: BLE001
            out.append({"eid": eid, "err": traceback.format_exc()})
        log("eid", eid, "done")
    with open(OUT, "w") as f:
        json.dump(out, f)
    ctl.Shutdown()
    cap.Shutdown()


try:
    main()
except Exception:  # noqa: BLE001
    log(traceback.format_exc())
LOG.close()
os._exit(0)
