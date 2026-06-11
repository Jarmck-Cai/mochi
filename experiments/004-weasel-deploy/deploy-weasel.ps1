# Deploy Mochi into an installed Weasel (M2-4). Run elevated (rime.dll swap
# lives under Program Files). Idempotent: safe to re-run after every rebuild.
#
#   powershell -ExecutionPolicy Bypass -File deploy-weasel.ps1 [-Restore]
#
# -Restore puts the original rime.dll back (instant rollback to stock Weasel).
param([switch]$Restore)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path

# locate the Weasel install dir (registry first, then default path)
$weaselDir = $null
foreach ($k in "HKLM:\SOFTWARE\Rime\Weasel", "HKLM:\SOFTWARE\WOW6432Node\Rime\Weasel") {
    if (Test-Path $k) { $weaselDir = (Get-ItemProperty $k -ErrorAction SilentlyContinue).WeaselRoot }
    if ($weaselDir) { break }
}
if (-not $weaselDir) {
    $weaselDir = Get-ChildItem "C:\Program Files\Rime" -Directory -Filter "weasel-*" |
        Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $weaselDir) { throw "Weasel install dir not found" }
Write-Host "Weasel: $weaselDir"

$ourDll = "$repo\experiments\003-translator-poc\librime\dist\lib\rime.dll"
$dll = "$weaselDir\rime.dll"
$bak = "$weaselDir\rime.dll.stock"

# WeaselServer holds rime.dll; stop it first (it restarts on next input)
$server = Get-Process WeaselServer -ErrorAction SilentlyContinue
if ($server) {
    & "$weaselDir\WeaselServer.exe" /q 2>$null   # graceful quit
    Start-Sleep -Milliseconds 800
    if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
        Stop-Process -Name WeaselServer -Force -Confirm:$false
        Start-Sleep -Milliseconds 500
    }
}

if ($Restore) {
    if (-not (Test-Path $bak)) { throw "no stock backup at $bak" }
    Copy-Item $bak $dll -Force
    Write-Host "restored stock rime.dll"
} else {
    if (-not (Test-Path $ourDll)) { throw "build librime first: $ourDll missing" }
    if (-not (Test-Path $bak)) { Copy-Item $dll $bak; Write-Host "stock rime.dll backed up" }
    Copy-Item $ourDll $dll -Force
    Write-Host "mochi rime.dll deployed ($((Get-Item $dll).LastWriteTime))"

    # user-dir config: mochi schema + schema list patch
    $rimeUser = "$env:APPDATA\Rime"
    New-Item -ItemType Directory -Force $rimeUser | Out-Null
    Copy-Item "$PSScriptRoot\mochi.schema.yaml" $rimeUser -Force
    Copy-Item "$PSScriptRoot\default.custom.yaml" $rimeUser -Force
    Write-Host "schema deployed to $rimeUser"
}

# rebuild the rime cache and restart the server
& "$weaselDir\WeaselDeployer.exe" /deploy 2>$null
Start-Process "$weaselDir\WeaselServer.exe"
Write-Host "WeaselServer restarted; remember to start mochi-brain:"
Write-Host "  $repo\src\brain\target\release\mochi-brain.exe"
