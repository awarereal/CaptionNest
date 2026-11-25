<#
  Copy the built Windows tools into src-tauri/resources so Tauri bundles them.
  Usage: scripts/tools/stage-tools.ps1
#>
$ErrorActionPreference = "Stop"
$Root = (Resolve-Path "$PSScriptRoot/../..").Path
$Env = @{}
Get-Content "$Root/scripts/tools/versions.env" | Where-Object { $_ -match "^[A-Z0-9_]+=" } | ForEach-Object {
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
# The bundled voice-activity model: fetched if missing and always hash-checked before it ships.
$Vad = "$Root/build/src/$($Env.VAD_MODEL_NAME)"
if (-not (Test-Path "$Root/build/src")) { New-Item -ItemType Directory "$Root/build/src" | Out-Null }
if (-not (Test-Path $Vad) -or (Get-FileHash $Vad -Algorithm SHA256).Hash -ne $Env.VAD_MODEL_SHA256.ToUpper()) {
  Invoke-WebRequest -Uri $Env.VAD_MODEL_URL -OutFile $Vad
}
if ((Get-FileHash $Vad -Algorithm SHA256).Hash -ne $Env.VAD_MODEL_SHA256.ToUpper()) { throw "VAD model hash mismatch" }
Copy-Item $Vad "$Res/bundled-models/" -Force
Copy-Item "$SrcDir/FFMPEG_CONFIGURE.txt" "$Res/licenses/FFMPEG_BUILD.txt" -Force
Get-ChildItem "$Res/tools", "$Res/bundled-models" | Format-Table Name, Length
