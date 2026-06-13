# WARNING: this script runs EVERYTHING elevated. On a machine whose interactive
# user is a STANDARD (non-admin) user, UAC elevates to a DIFFERENT admin account,
# so %LOCALAPPDATA% / %APPDATA% / HKCU here point at the ADMIN profile and all
# per-user setup lands in the wrong place. For such machines use the split flow:
# do per-user setup AS the interactive user, then run swap-rimedll.ps1 (admin) for
# the Program Files rime.dll swap only. See devlog 2026-06-12. This script is only
# safe when the interactive user IS a local admin (same-profile elevation).
#
# deploy-local.ps1 - local/dev deploy: stock Weasel 0.17.4 + Mochi fork rime.dll.
# Friend-style layout (%LOCALAPPDATA%\Mochi) but keeps the official Weasel
# binaries; only rime.dll (the MochiTranslator plugin carrier) is swapped, so
# everything works except the candidate-internal gray ghost rendering.
# Self-elevates (needs admin for Program Files + restarting the elevated server).
# ASCII-only comments on purpose (PS 5.1 reads BOM-less files as ANSI).
$ErrorActionPreference = "Stop"

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
if (-not $isAdmin) {
    Start-Process powershell -Verb RunAs -ArgumentList @("-ExecutionPolicy","Bypass","-NoProfile","-File","`"$PSCommandPath`"")
    exit
}

$done = "$env:TEMP\mochi-deploy.done"
Remove-Item $done -ErrorAction SilentlyContinue
Start-Transcript -Path "$env:TEMP\mochi-deploy.log" -Force | Out-Null

$repo = "E:\Projects\mochi"
$weaselDir = "C:\Program Files\Rime\weasel-0.17.4"
$forkRime = "$repo\experiments\weasel-fork-probe\output\rime.dll"

# ---- 1. stop services ----
if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
    & "$weaselDir\WeaselServer.exe" /q 2>$null
    Start-Sleep -Milliseconds 1200
    Get-Process WeaselServer -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
    Start-Sleep -Milliseconds 500
}
Get-Process mochi-brain -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false

# ---- 2. swap rime.dll (back up official as .stock once) ----
$dst = "$weaselDir\rime.dll"
if ((Test-Path $dst) -and -not (Test-Path "$dst.stock")) { Copy-Item $dst "$dst.stock" }
Copy-Item $forkRime $dst -Force
Write-Host "rime.dll swapped to Mochi fork (official backed up as rime.dll.stock)"

# ---- 3. Mochi home (brain + artifacts + personal memory) ----
$mochiHome = "$env:LOCALAPPDATA\Mochi"
New-Item -ItemType Directory -Force "$mochiHome\bin","$mochiHome\artifacts\v0","$mochiHome\user_data" | Out-Null
Copy-Item "$repo\src\brain\target\release\mochi-brain.exe" "$mochiHome\bin\" -Force
Copy-Item "$repo\artifacts\v0\*" "$mochiHome\artifacts\v0\" -Force
Write-Host "Mochi home: $mochiHome"

# ---- 4. rime config (back up existing as .pre-mochi once) ----
$rimeUser = "$env:APPDATA\Rime"
New-Item -ItemType Directory -Force $rimeUser | Out-Null
foreach ($n in "default.custom.yaml","weasel.custom.yaml") {
    $d2 = Join-Path $rimeUser $n
    if ((Test-Path $d2) -and -not (Test-Path "$d2.pre-mochi")) { Copy-Item $d2 "$d2.pre-mochi" }
}
foreach ($n in "mochi.schema.yaml","default.custom.yaml","weasel.custom.yaml") {
    Copy-Item "$repo\experiments\004-weasel-deploy\$n" $rimeUser -Force
}
Write-Host "Rime config deployed to $rimeUser"

# ---- 5. brain autostart (hidden window via vbs) ----
$exe = "$mochiHome\bin\mochi-brain.exe"
$logb = "$mochiHome\brain.log"
$cmd = "cmd /c """"$exe"" --artifacts ""$mochiHome\artifacts\v0"" --user-data ""$mochiHome\user_data"" >> ""$logb"" 2>&1"""
$vbs = "$mochiHome\start-brain.vbs"
$vbsBody = 'CreateObject("WScript.Shell").Run "' + ($cmd -replace '"','""') + '", 0, False'
Set-Content -Path $vbs -Value $vbsBody -Encoding Default
Set-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "MochiBrain" -Value "wscript.exe `"$vbs`""
Write-Host "brain registered for autostart (HKCU Run / MochiBrain)"

# ---- 6. start brain -> deploy schema -> start server at user level ----
wscript.exe $vbs
Start-Sleep -Seconds 2
& "$weaselDir\WeaselDeployer.exe" /deploy 2>$null
Start-Sleep -Seconds 3
explorer.exe "$weaselDir\WeaselServer.exe"

"DEPLOY_OK $(Get-Date -Format o)" | Out-File $done -Encoding utf8
Write-Host "=== deploy complete ==="
Stop-Transcript | Out-Null
