<#
  Build whisper-cli.exe and whisper-vad.exe for Windows x64 with MSVC.

  - CPU backends for every instruction-set level (ggml picks the best one at runtime),
    so the app runs on old and new processors alike.
  - Vulkan GPU backend (ggml-vulkan.dll) when the Vulkan SDK is installed; it loads only
    if the PC's graphics driver provides Vulkan, otherwise the CPU is used.
  - Static MSVC runtime: recipients don't need the Visual C++ Redistributable.

  Output: build/tools/x86_64-pc-windows-msvc/{whisper-cli.exe, whisper-vad.exe, *.dll}
#>
param([switch]$NoVulkan)
$ErrorActionPreference = "Stop"
$Root = (Resolve-Path "$PSScriptRoot/../..").Path
$Env = @{}
Get-Content "$Root/scripts/tools/versions.env" | Where-Object { $_ -match "^[A-Z_]+=" } | ForEach-Object {
  $k, $v = $_ -split "=", 2; $Env[$k] = $v.Trim()
}
$Triple = "x86_64-pc-windows-msvc"
$Src = "$Root/build/src"
$Out = "$Root/build/tools/$Triple"
$Work = "$Root/build/work/whisper-win64"
New-Item -ItemType Directory -Force -Path $Src, $Out | Out-Null

$Tar = "$Src/whisper.cpp-$($Env.WHISPER_VERSION).tar.gz"
if (-not (Test-Path $Tar) -or (Get-FileHash $Tar -Algorithm SHA256).Hash -ne $Env.WHISPER_SHA256.ToUpper()) {
  Invoke-WebRequest -Uri $Env.WHISPER_URL -OutFile $Tar
  if ((Get-FileHash $Tar -Algorithm SHA256).Hash -ne $Env.WHISPER_SHA256.ToUpper()) { throw "whisper.cpp source hash mismatch" }
}
$Vad = "$Src/$($Env.VAD_MODEL_NAME)"
if (-not (Test-Path $Vad) -or (Get-FileHash $Vad -Algorithm SHA256).Hash -ne $Env.VAD_MODEL_SHA256.ToUpper()) {
  Invoke-WebRequest -Uri $Env.VAD_MODEL_URL -OutFile $Vad
  if ((Get-FileHash $Vad -Algorithm SHA256).Hash -ne $Env.VAD_MODEL_SHA256.ToUpper()) { throw "VAD model hash mismatch" }
}

if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Force -Path "$Work/src" | Out-Null
tar -xzf $Tar -C "$Work/src" --strip-components=1

$Vulkan = if ($NoVulkan -or -not $env:VULKAN_SDK) { "OFF" } else { "ON" }
Write-Host "Vulkan backend: $Vulkan"
cmake -S "$Work/src" -B "$Work/build" -G "Visual Studio 17 2022" -A x64 `
  -DCMAKE_BUILD_TYPE=Release `
  -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded `
  -DBUILD_SHARED_LIBS=ON `
  -DGGML_NATIVE=OFF `
  -DGGML_BACKEND_DL=ON `
  -DGGML_CPU_ALL_VARIANTS=ON `
  -DGGML_VULKAN=$Vulkan `
  -DWHISPER_BUILD_TESTS=OFF `
  -DWHISPER_BUILD_SERVER=OFF `
  -DWHISPER_SDL2=OFF `
  -DWHISPER_CURL=OFF
if ($LASTEXITCODE -ne 0) { throw "cmake configure failed" }
# Build everything: backend DLLs aren't link dependencies of whisper-cli in DL mode.
cmake --build "$Work/build" --config Release -j $env:NUMBER_OF_PROCESSORS
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$Bin = "$Work/build/bin/Release"
Copy-Item "$Bin/whisper-cli.exe" "$Out/whisper-cli.exe" -Force
Copy-Item "$Bin/whisper-vad-speech-segments.exe" "$Out/whisper-vad.exe" -Force
Get-ChildItem "$Bin/*.dll" | Copy-Item -Destination $Out -Force
"whisper.cpp $($Env.WHISPER_VERSION) ($($Env.WHISPER_URL)), Vulkan=$Vulkan, CPU variants, target $Triple" | Set-Content "$Out/WHISPER_BUILD.txt"
Get-ChildItem $Out | Format-Table Name, Length
