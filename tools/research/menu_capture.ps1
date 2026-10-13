# Screenshot one menu screen of our app without any input: starts the release build with
# `--screen <name>` (fullscreen), waits, grabs its window with PrintWindow, quits it.
# usage: powershell -File tools/research/menu_capture.ps1 -Screen mode -Out runs/cap/mode.png
#        [-Wait 7] [-Save runs/parity-save.json] [-Data data] [-Assets <WACCA assets>]
# The save should be a temp file (doc 14 §1 has the one used for the parity runs).
param(
  [Parameter(Mandatory = $true)][string]$Screen,
  [Parameter(Mandatory = $true)][string]$Out,
  [double]$Wait = 7,
  [string]$Save = "runs/parity-save.json",
  [string]$Data = "data",
  [string]$Assets = "assets"
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class MenuCap {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int L, T, R, B; }
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  // The top-level window titled `title` that belongs to process `pid` (zero if none yet).
  public static IntPtr Find(uint pid, string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      var s = new System.Text.StringBuilder(256); GetWindowText(h, s, 256);
      if (p == pid && s.ToString() == title) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
[MenuCap]::SetProcessDPIAware() | Out-Null
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$exe = "$root\target\release\wacca-app.exe"
$a = "--assets `"$Assets`" --data `"$root\$Data`" --save `"$root\$Save`" --screen $Screen"
$p = Start-Process -FilePath $exe -ArgumentList $a -WorkingDirectory $root -PassThru
# The window of the process started here (never another running instance, e.g. the user's).
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 100 -and $h -eq [IntPtr]::Zero -and -not $p.HasExited; $i++) {
  Start-Sleep -Milliseconds 200
  $h = [MenuCap]::Find([uint32]$p.Id, "WACCA_slim")
}
if ($h -eq [IntPtr]::Zero) {
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  throw "no window from wacca-app (pid $($p.Id))"
}
Start-Sleep -Milliseconds ([int]($Wait * 1000))
if ($p.HasExited) { throw "wacca-app (pid $($p.Id)) exited before the capture" }
$r = New-Object MenuCap+RECT
[MenuCap]::GetClientRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$dc = $g.GetHdc()
# 3 = PW_CLIENTONLY | PW_RENDERFULLCONTENT (captures the GPU-presented content).
[MenuCap]::PrintWindow($h, $dc, 3) | Out-Null
$g.ReleaseHdc($dc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Stop-Process -Id $p.Id -Force
