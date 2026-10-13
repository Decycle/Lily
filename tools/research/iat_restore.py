#!/usr/bin/env python3
"""Snapshot / restore import-address-table entries of a running process's main module (Windows, ctypes, stdlib).

Why: RenderDoc and segatools both hook LoadLibrary*/GetProcAddress by IAT patching. Injected after segatools,
RenderDoc replaces segatools' hooks in the game's IAT and calls the real kernel32 functions, so segatools'
DLL redirection (e.g. the emulated LED board `USBIntLED.DLL`, error 1202-1) stops working. Injected before
segatools, the two hook chains recurse (stack overflow). Working order (doc 11 §2): start the game normally,
`snapshot` the IAT once segatools is initialised, inject RenderDoc before UE creates its D3D11 device, then
`restore` the LoadLibrary*/GetProcAddress entries to segatools' values. RenderDoc keeps its D3D11/DXGI hooks.

Usage:
  python -I iat_restore.py snapshot <pid> <snap.json>
  python -I iat_restore.py restore <pid> <snap.json> [Func ...]   (default: LoadLibrary{A,W,ExA,ExW}, GetProcAddress)
Only entries whose current value differs from the snapshot are written; prints what changed.
"""
import ctypes
import ctypes.wintypes as wt
import json
import struct
import sys

PROCESS_ALL = 0x0010 | 0x0020 | 0x0008 | 0x0400  # VM_READ | VM_WRITE | VM_OPERATION | QUERY_INFORMATION
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.ReadProcessMemory.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                  ctypes.POINTER(ctypes.c_size_t)]
k32.WriteProcessMemory.argtypes = k32.ReadProcessMemory.argtypes
k32.VirtualProtectEx.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_size_t, wt.DWORD, ctypes.POINTER(wt.DWORD)]
psapi.EnumProcessModulesEx.argtypes = [wt.HANDLE, ctypes.POINTER(ctypes.c_void_p), wt.DWORD,
                                       ctypes.POINTER(wt.DWORD), wt.DWORD]
DEFAULT = ["LoadLibraryA", "LoadLibraryW", "LoadLibraryExA", "LoadLibraryExW", "GetProcAddress"]


def read(h, addr, n):
    buf = ctypes.create_string_buffer(n)
    got = ctypes.c_size_t()
    if not k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, n, ctypes.byref(got)):
        raise OSError("ReadProcessMemory 0x%x: %d" % (addr, ctypes.get_last_error()))
    return buf.raw[:got.value]


def main_module(h):
    mods = (ctypes.c_void_p * 1)()
    need = wt.DWORD()
    if not psapi.EnumProcessModulesEx(h, mods, ctypes.sizeof(mods), ctypes.byref(need), 3):
        raise OSError("EnumProcessModulesEx: %d" % ctypes.get_last_error())
    return mods[0]


def imports(h, base):
    """[(dll, func, iat_address, value)] for named imports of the main module."""
    lfanew = struct.unpack("<I", read(h, base + 0x3C, 4))[0]
    opt = base + lfanew + 24
    imp_rva, _ = struct.unpack("<II", read(h, opt + 112 + 8, 8))  # PE32+: DataDirectory at 112, [1] = import
    out = []
    p = base + imp_rva
    while True:
        oft, _, _, name_rva, ft = struct.unpack("<IIIII", read(h, p, 20))
        if not (oft or ft):
            break
        dll = read(h, base + name_rva, 64).split(b"\0")[0].decode("latin-1")
        i = 0
        while True:
            ent = struct.unpack("<Q", read(h, base + (oft or ft) + 8 * i, 8))[0]
            if ent == 0:
                break
            if not ent >> 63:
                fname = read(h, base + (ent & 0x7FFFFFFF) + 2, 80).split(b"\0")[0].decode("latin-1")
                iat = base + ft + 8 * i
                out.append((dll, fname, iat, struct.unpack("<Q", read(h, iat, 8))[0]))
            i += 1
        p += 20
    return out


def main(argv):
    if len(argv) < 3 or argv[0] not in ("snapshot", "restore"):
        print(__doc__)
        return 1
    pid = int(argv[1])
    h = k32.OpenProcess(PROCESS_ALL, False, pid)
    if not h:
        print("OpenProcess failed: %d" % ctypes.get_last_error())
        return 1
    base = main_module(h)
    entries = imports(h, base)
    if argv[0] == "snapshot":
        with open(argv[2], "w") as f:
            json.dump([{"dll": d, "func": fn, "iat": a, "value": v} for d, fn, a, v in entries], f)
        print("snapshot: %d entries, base 0x%x" % (len(entries), base))
        return 0
    with open(argv[2]) as f:
        snap = {(e["dll"].lower(), e["func"]): e for e in json.load(f)}
    wanted = set(argv[3:] or DEFAULT)
    for d, fn, a, v in entries:
        e = snap.get((d.lower(), fn))
        if fn not in wanted or e is None or e["value"] == v:
            continue
        old = wt.DWORD()
        k32.VirtualProtectEx(h, ctypes.c_void_p(a), 8, 0x04, ctypes.byref(old))  # PAGE_READWRITE
        data = struct.pack("<Q", e["value"])
        ok = k32.WriteProcessMemory(h, ctypes.c_void_p(a), data, 8, None)
        k32.VirtualProtectEx(h, ctypes.c_void_p(a), 8, old.value, ctypes.byref(wt.DWORD()))
        print("%s!%s: 0x%x -> 0x%x %s" % (d, fn, v, e["value"], "ok" if ok else "FAILED"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
