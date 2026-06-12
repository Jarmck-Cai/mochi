# Mochi 卸载脚本：还原官方小狼毫，移除 brain 自启。
# 个人记忆数据（%LOCALAPPDATA%\Mochi\user_data）保留不删——那是你的数据；
# 想彻底清除请手动删除整个 %LOCALAPPDATA%\Mochi 目录。
#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"

# 定位小狼毫
$weaselDir = $null
foreach ($k in "HKLM:\SOFTWARE\Rime\Weasel", "HKLM:\SOFTWARE\WOW6432Node\Rime\Weasel") {
    if (Test-Path $k) { $weaselDir = (Get-ItemProperty $k -ErrorAction SilentlyContinue).WeaselRoot }
    if ($weaselDir) { break }
}
if (-not $weaselDir -and (Test-Path "C:\Program Files\Rime")) {
    $weaselDir = Get-ChildItem "C:\Program Files\Rime" -Directory -Filter "weasel-*" |
        Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $weaselDir) { throw "未找到小狼毫安装目录" }

# 停服务、停 brain
if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
    & "$weaselDir\WeaselServer.exe" /q 2>$null
    Start-Sleep -Milliseconds 1200
    if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
        Stop-Process -Name WeaselServer -Force -Confirm:$false
        Start-Sleep -Milliseconds 500
    }
}
Get-Process mochi-brain -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false

# 还原官方二进制
foreach ($name in "rime.dll", "WeaselServer.exe", "weasel.dll", "weaselx64.dll", "WeaselDeployer.exe") {
    $dst = Join-Path $weaselDir $name
    if (Test-Path "$dst.stock") {
        Copy-Item "$dst.stock" $dst -Force
        Remove-Item "$dst.stock" -Confirm:$false
    }
}
Write-Host "官方 Weasel 组件已还原"

# 移除自启
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" `
    -Name "MochiBrain" -ErrorAction SilentlyContinue
Remove-Item "$env:LOCALAPPDATA\Mochi\start-brain.vbs" -ErrorAction SilentlyContinue -Confirm:$false

# 还原 Rime 用户配置
$rimeUser = "$env:APPDATA\Rime"
foreach ($name in "default.custom.yaml", "weasel.custom.yaml") {
    $dst = Join-Path $rimeUser $name
    if (Test-Path "$dst.pre-mochi") {
        Copy-Item "$dst.pre-mochi" $dst -Force
        Remove-Item "$dst.pre-mochi" -Confirm:$false
    } elseif (Test-Path $dst) {
        Remove-Item $dst -Confirm:$false   # 安装前不存在，直接移除
    }
}
Remove-Item "$rimeUser\mochi.schema.yaml" -ErrorAction SilentlyContinue -Confirm:$false

# 重建索引并重启服务
& "$weaselDir\WeaselDeployer.exe" /deploy 2>$null
Start-Sleep -Seconds 2
explorer.exe "$weaselDir\WeaselServer.exe"

Write-Host ""
Write-Host "=== 卸载完成，小狼毫已还原 ==="
Write-Host "个人数据仍保留在 $env:LOCALAPPDATA\Mochi\user_data（如需彻底清除请手动删除）"
