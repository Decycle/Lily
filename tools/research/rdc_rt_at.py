"""Save textures of a RenderDoc capture as PNG at chosen events (e.g. the back buffer right after the
main view's tonemap, before the HUD). Runs INSIDE qrenderdoc's Python:

    set WACCA_RDC_RT=<capture.rdc>;<out_dir>;<eid>:<resource id>[,<eid>:<resource id>...]
    qrenderdoc.exe --python tools\\research\\rdc_rt_at.py

Writes `<out_dir>/<resource>_at_<eid>.png` (and `rt_at.log`). Read-only on the capture.
"""
import os
import traceback

import renderdoc as rd

CAP, OUT, SPEC = os.environ["WACCA_RDC_RT"].split(";")
os.makedirs(OUT, exist_ok=True)
LOG = open(os.path.join(OUT, "rt_at.log"), "w", encoding="utf-8", buffering=1)


def main():
    cap = rd.OpenCaptureFile()
    if cap.OpenFile(CAP, "", None) != rd.ResultCode.Succeeded:
        LOG.write("open failed\n")
        return
    res, ctl = cap.OpenCapture(rd.ReplayOptions(), None)
    if res != rd.ResultCode.Succeeded:
        LOG.write("replay failed\n")
        return
    textures = {int(t.resourceId): t.resourceId for t in ctl.GetTextures()}
    for item in SPEC.split(","):
        eid, res_id = (int(x) for x in item.split(":"))
        ctl.SetFrameEvent(eid, True)
        s = rd.TextureSave()
        s.resourceId = textures[res_id]
        s.destType = rd.FileType.PNG
        s.alpha = rd.AlphaMapping.Discard
        path = os.path.join(OUT, "%d_at_%d.png" % (res_id, eid))
        ctl.SaveTexture(s, path)
        LOG.write("saved %s\n" % path)
    ctl.Shutdown()
    cap.Shutdown()
    LOG.write("done\n")


try:
    main()
except Exception:  # noqa: BLE001
    LOG.write(traceback.format_exc())
LOG.close()
os._exit(0)
