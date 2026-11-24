<#
  Smoke test the Windows installer the way a recipient would use it:
  silent per-user install, then run the installed app's pipeline on known speech.
  Usage: scripts/ci/smoke-windows.ps1 -Installer <setup.exe> -Model <ggml-model.bin>
#>
param([Parameter(Mandatory)] [string]$Installer, [Parameter(Mandatory)] [string]$Model)
$ErrorActionPreference = "Stop"
$Work = Join-Path $env:RUNNER_TEMP "cn-smoke"
New-Item -ItemType Directory -Force -Path $Work | Out-Null

Write-Host "Installing $Installer"
$p = Start-Process -FilePath $Installer -ArgumentList "/S" -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "installer exit code $($p.ExitCode)" }
$Exe = Get-ChildItem -Path "$env:LOCALAPPDATA\CaptionNest", "$env:LOCALAPPDATA\Programs\CaptionNest" -Filter *.exe -Recurse -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -notmatch "uninstall" -and $_.DirectoryName -notmatch "tools" } | Select-Object -First 1
if (-not $Exe) { throw "installed app not found" }
Write-Host "Installed: $($Exe.FullName)"
Get-ChildItem (Join-Path $Exe.DirectoryName "tools") | Format-Table Name, Length

Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$synth.SetOutputToWaveFile("$Work\speech.wav")
$synth.Speak("Hello from the CaptionNest smoke test. The quick brown fox jumps over the lazy dog.")
$synth.Dispose()

$env:CAPTIONNEST_HOME = "$Work\home"
$proc = Start-Process -FilePath $Exe.FullName -ArgumentList @("--self-test", "`"$Work\speech.wav`"", "--model", "`"$Model`"", "--out", "`"$Work\out.srt`"", "--language", "en") -Wait -PassThru
Get-Content "$Work\out.json"
$r = Get-Content "$Work\out.json" | ConvertFrom-Json
if (-not $r.ok) { throw "self-test failed" }
$text = $r.text.ToLower()
if (-not ($text.Contains("fox") -and $text.Contains("dog"))) { throw "unexpected transcript: $text" }
Write-Host "smoke test passed: $($r.cues) captions in $([math]::Round($r.seconds,1)) s, GPU: $($r.usedGpu)"
Get-Content "$Work\out.srt" -TotalCount 8
