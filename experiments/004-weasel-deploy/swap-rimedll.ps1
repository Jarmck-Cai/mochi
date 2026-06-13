# swap-rimedll.ps1 - ADMIN-ONLY minimal step: stop the (elevated) WeaselServer
# and replace Program Files rime.dll with the Mochi fork. Touches nothing in any
# user profile, so it is safe to run as a different admin account on a machine
# whose interactive user is a standard (non-admin) user.
# All per-user setup (Mochi home, rime config, autostart) is done separately as
# the interactive user. ASCII-only comments (PS 5.1 ANSI-reads BOM-less files).
$ErrorActionPreference = "Stop"

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
if (-not $isAdmin) {
    Start-Process powershell -Verb RunAs -ArgumentList @("-ExecutionPolicy","Bypass","-NoProfile","-File","`"$PSCommandPath`"")
    exit
}

$done = "C:\Users\Jarmck\AppData\Local\Temp\mochi-swap.done"
Remove-Item $done -ErrorAction SilentlyContinue

$weaselDir = "C:\Program Files\Rime\weasel-0.17.4"
$forkRime  = "E:\Projects\mochi\experiments\weasel-fork-probe\output\rime.dll"

# stop the running WeaselServer (it holds rime.dll open); it may be elevated
if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
    & "$weaselDir\WeaselServer.exe" /q 2>$null
    Start-Sleep -Milliseconds 1200
    Get-Process WeaselServer -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
    Start-Sleep -Milliseconds 500
}

# back up official rime.dll once, then swap in the fork
$dst = "$weaselDir\rime.dll"
if ((Test-Path $dst) -and -not (Test-Path "$dst.stock")) { Copy-Item $dst "$dst.stock" }
Copy-Item $forkRime $dst -Force

"SWAP_OK $((Get-Item $dst).Length)" | Out-File $done -Encoding utf8
