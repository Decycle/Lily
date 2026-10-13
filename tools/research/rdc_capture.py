"""RenderDoc target-control driver for real-game captures. Runs INSIDE qrenderdoc's Python:

    set WACCA_RDC_CFG=<cfg.json>
    qrenderdoc.exe --python tools\\research\\rdc_capture.py

The game must already be (being) launched under RenderDoc, e.g. `renderdoccmd capture --opt-hook-children -w
-c <dir>\\wacca inject.exe -d -k mercuryhook.dll ...Mercury-Win64-Shipping.exe ...` (see doc 11 §2).

cfg.json:
  {"status": "<file to append progress lines to>",
   "target": "Mercury",                       substring of the target (exe) name to connect to
   "connect_timeout_s": 300,
   "log": "<run_experiment stderr log to tail>",   optional
   "chart_times": [9.4, 12.0, ...],          captures at these chart seconds, scheduled from the
                                              run_experiment line "sync: song start ... found N ms later"
   "cmd": "<file>"}                           optional: each new line "capture" triggers one capture now,
                                              "quit" ends the script
Each capture is logged as "capture <frame> <path> chart_t=<t>". The script ends when the target disconnects,
on "quit", or 30 s after the last scheduled capture. Read-only towards the game; it only sends TriggerCapture.
"""
import json
import os
import re
import time

import renderdoc as rd

CFG = json.load(open(os.environ["WACCA_RDC_CFG"], encoding="utf-8"))
STATUS = open(CFG["status"], "a", encoding="utf-8", buffering=1)


def say(msg):
    STATUS.write("%.3f %s\n" % (time.time(), msg))


def connect():
    deadline = time.time() + CFG.get("connect_timeout_s", 300)
    while time.time() < deadline:
        ident = rd.EnumerateRemoteTargets("", 0)
        while ident:
            tc = rd.CreateTargetControl("", ident, "wacca-rdc", True)
            if tc is not None:
                name = tc.GetTarget()
                if CFG.get("target", "Mercury") in name:
                    say("connected ident %d target %s pid %d api %s" % (ident, name, tc.GetPID(), tc.GetAPI()))
                    return tc
                tc.Shutdown()
            ident = rd.EnumerateRemoteTargets("", ident)
        time.sleep(1.0)
    say("no target found")
    return None


def main():
    tc = connect()
    if tc is None:
        return
    log_pos = 0
    cmd_pos = 0
    pending = []  # (wall time, chart t)
    synced = False
    last_sched = None
    pending_label = []
    while True:
        msg = tc.ReceiveMessage(None)
        if msg.type == rd.TargetControlMessageType.Disconnected:
            say("target disconnected")
            break
        if msg.type == rd.TargetControlMessageType.NewCapture:
            c = msg.newCapture
            label = pending_label.pop(0) if pending_label else "?"
            say("capture frame %d path %s chart_t=%s" % (c.frameNumber, c.path, label))
        if msg.type == rd.TargetControlMessageType.RegisterAPI:
            say("api %s" % msg.apiUse.name)
        now = time.time()
        if CFG.get("log") and not synced and os.path.exists(CFG["log"]):
            with open(CFG["log"], "r", encoding="utf-8", errors="replace") as f:
                f.seek(log_pos)
                text = f.read()
                log_pos = f.tell()
            m = re.search(r"sync: song start at epoch [\d.]+ ms .*found (\d+) ms later", text)
            if m:
                synced = True
                chart_now = int(m.group(1)) / 1000.0
                pending = sorted((now + (t - chart_now), t) for t in CFG.get("chart_times", []) if t > chart_now + 0.2)
                last_sched = pending[-1][0] if pending else now
                say("synced: chart t %.3f now; %d captures scheduled" % (chart_now, len(pending)))
        while pending and pending[0][0] <= now:
            _, t = pending.pop(0)
            tc.TriggerCapture(1)
            pending_label.append("%.2f" % t)
            say("trigger chart_t=%.2f (late %.0f ms)" % (t, (now - _) * 1000))
        if CFG.get("cmd") and os.path.exists(CFG["cmd"]):
            with open(CFG["cmd"], "r", encoding="utf-8") as f:
                f.seek(cmd_pos)
                lines = f.read().splitlines()
                cmd_pos = f.tell()
            for ln in lines:
                if ln.strip() == "capture":
                    tc.TriggerCapture(1)
                    pending_label.append("manual@%.1f" % now)
                    say("trigger manual")
                elif ln.strip() == "quit":
                    say("quit")
                    tc.Shutdown()
                    return
        if last_sched is not None and not pending and now > last_sched + 30:
            say("done")
            break
        time.sleep(0.005)
    tc.Shutdown()


try:
    main()
except Exception as e:  # noqa: BLE001
    say("error %r" % e)
STATUS.close()
os._exit(0)
