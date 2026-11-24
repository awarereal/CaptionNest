<#
  Copy the built Windows tools into src-tauri/resources so Tauri bundles them.
  Usage: scripts/tools/stage-tools.ps1
#>
$ErrorActionPreference = "Stop"
$Root = (Resolve-Path "$PSScriptRoot/../..").Path
$Env = @{}
Get-Content "$Root/scripts/tools/versions.env" | Where-Object { $_ -match "^[A-Z_]+=" } | ForEach-Object {
  $k, $v = $_ -split "=", 2; $Env[$k] = $v.Trim()
}
$SrcDir = "$Root/build/tools/x86_64-pc-windows-msvc"
$Res = "$Root/src-tauri/resources"
foreach ($f in "ffmpeg.exe", "ffprobe.exe", "whisper-cli.exe", "whisper-vad.exe") {
  if (-not (Test-Path "$SrcDir/$f")) { throw "missing $SrcDir/$f - build the tools first" }
}
foreach ($d in "tools", "bundled-models") { if (Test-Path "$Res/$d") { Remove-Item -Recurse -Force "$Res/$d" } }
New-Item -ItemType Directory -Force -Path "$Res/tools", "$Res/bundled-models", "$Res/licenses" | Out-Null
Get-ChildItem $SrcDir -Include *.exe, *.dll -File -Recurse | Copy-Item -Destination "$Res/tools" -Force
Copy-Item "$Root/build/src/$($Env.VAD_MODEL_NAME)" "$Res/bundled-models/" -Force
Copy-Item "$SrcDir/FFMPEG_CONFIGURE.txt" "$Res/licenses/FFMPEG_BUILD.txt" -Force
Get-ChildItem "$Res/tools", "$Res/bundled-models" | Format-Table Name, Length
