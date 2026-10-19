# Builds a shareable portable folder, dist\Lily\, with lily.exe, 7-Zip's 7z.exe + 7z.dll
# (LGPL, with its License.txt), Lily's LICENSE and NOTICE.md, README-portable.txt and the
# portable.txt marker, and zips it to dist\Lily-windows-x64.zip. It contains NO game data: on
# first run the app asks for the player's own WACCA files and sets them up.
#
#   powershell -ExecutionPolicy Bypass -File tools\make-portable.ps1 [-SevenZip "C:\Program Files\7-Zip"] [-NoBuild]
param(
    [string]$SevenZip = "C:\Program Files\7-Zip",
    [switch]$NoBuild
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root "dist\Lily"
$zip = Join-Path $root "dist\Lily-windows-x64.zip"

if (-not $NoBuild) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = "2" }
    & cargo build --locked --release -p wacca-app --bin lily --manifest-path (Join-Path $root "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
$exe = Join-Path $root "target\release\lily.exe"
if (-not (Test-Path $exe)) { throw "$exe not found (build first)" }
foreach ($f in "7z.exe", "7z.dll", "License.txt") {
    if (-not (Test-Path (Join-Path $SevenZip $f))) { throw "$f not found in $SevenZip (install 7-Zip or pass -SevenZip)" }
}

if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
if (Test-Path $zip) { Remove-Item -Force $zip }
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item $exe $dist
Copy-Item (Join-Path $SevenZip "7z.exe") $dist
Copy-Item (Join-Path $SevenZip "7z.dll") $dist
Copy-Item (Join-Path $SevenZip "License.txt") (Join-Path $dist "7-Zip-License.txt")
Copy-Item (Join-Path $root "LICENSE") (Join-Path $dist "LICENSE.txt")
Copy-Item (Join-Path $root "NOTICE.md") (Join-Path $dist "NOTICE.md")
Set-Content -Encoding utf8 (Join-Path $dist "portable.txt") "This file keeps the save in save\save.json next to lily.exe."

$readme = @'
Lily (portable)
===============

A native reimplementation of the WACCA rhythm game for Windows touchscreen PCs.
Lily is unofficial and not affiliated with SEGA or Marvelous.
This folder contains NO game files.

You need your own copy of WACCA Reverse (3.00.00 or newer): the 3.00.00 base archive
plus any updates you have (.7z files; updates add songs) or an installed/unpacked game folder.

First run
---------
1. Start lily.exe.
2. Setup asks for your WACCA files: choose the folder that holds the .7z archives,
   or the game folder (the one containing WindowsNoEditor).
3. Choose whether to include the background videos (about 20 GB more), then START.
   Setup writes assets\ and data\ into this folder. This takes a while.
4. When it is done, the title screen appears. Tap to play.

Run setup again at any time with the SETUP button at the top of the title screen
(or F2) to add the background videos or repair the files.

Files in this folder
--------------------
lily.exe               the game
7z.exe, 7z.dll         7-Zip, used by setup to unpack the archives
7-Zip-License.txt      7-Zip's licence (GNU LGPL + unRAR restriction); source: www.7-zip.org
LICENSE.txt            Lily's licence (MIT)
NOTICE.md              trademarks, game content and third-party licences
portable.txt           keeps your save in save\save.json here (delete it to use %APPDATA%\lily)
assets\, data\         created by setup (your game files; do not share them)

Background videos (optional)
----------------------------
Playing the background videos needs FFmpeg: install it with
  winget install Gyan.FFmpeg
or put ffmpeg.exe next to lily.exe, or set WACCA_FFMPEG to its path.
Without FFmpeg the songs play without videos.

Keys
----
F11 or Alt+Enter: fullscreen on/off.  Esc: back (on the title screen: quit).
Start with --windowed for a window.
'@
Set-Content -Encoding utf8 (Join-Path $dist "README-portable.txt") $readme

# The zip holds the Lily\ folder itself, so it unpacks to Lily\.
Compress-Archive -Path $dist -DestinationPath $zip

Get-ChildItem $dist | Format-Table Name, Length
Write-Host ("Portable folder ready: $dist`nZip: $zip ({0:N1} MB)" -f ((Get-Item $zip).Length / 1MB))
