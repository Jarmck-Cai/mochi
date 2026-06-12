# Mochi 安装脚本（Windows x64，好友试用版）。
# 前提：已安装官方小狼毫 Weasel 0.17.4
#   https://github.com/rime/weasel/releases/tag/v0.17.4
# 用法：右键本文件 -> 使用 PowerShell 运行（会请求管理员权限），或在管理员
# PowerShell 中执行：powershell -ExecutionPolicy Bypass -File install.ps1
#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$kit = $PSScriptRoot

# 下载的 zip 解压后文件带 Mark-of-the-Web，先解除，否则 dll 可能被拒载
Get-ChildItem $kit -Recurse -File | Unblock-File -ErrorAction SilentlyContinue

# ---- 1. 定位小狼毫安装目录 ----
$weaselDir = $null
foreach ($k in "HKLM:\SOFTWARE\Rime\Weasel", "HKLM:\SOFTWARE\WOW6432Node\Rime\Weasel") {
    if (Test-Path $k) { $weaselDir = (Get-ItemProperty $k -ErrorAction SilentlyContinue).WeaselRoot }
    if ($weaselDir) { break }
}
if (-not $weaselDir -and (Test-Path "C:\Program Files\Rime")) {
    $weaselDir = Get-ChildItem "C:\Program Files\Rime" -Directory -Filter "weasel-*" |
        Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $weaselDir) {
    throw "未找到小狼毫。请先安装官方 Weasel 0.17.4：https://github.com/rime/weasel/releases/tag/v0.17.4"
}
if ((Split-Path $weaselDir -Leaf) -ne "weasel-0.17.4") {
    throw "本包基于 Weasel 0.17.4 构建，检测到 $weaselDir。请安装 0.17.4 版本后重试。"
}
Write-Host "小狼毫目录：$weaselDir"

# ---- 2. 停止 WeaselServer（占用待替换的 dll）----
if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
    & "$weaselDir\WeaselServer.exe" /q 2>$null
    Start-Sleep -Milliseconds 1200
    if (Get-Process WeaselServer -ErrorAction SilentlyContinue) {
        Stop-Process -Name WeaselServer -Force -Confirm:$false
        Start-Sleep -Milliseconds 500
    }
}
Get-Process mochi-brain -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false

# ---- 3. 替换 Weasel 二进制（原版备份为 *.stock，卸载脚本可还原）----
$forkFiles = "rime.dll", "WeaselServer.exe", "weasel.dll", "weaselx64.dll", "WeaselDeployer.exe"
foreach ($name in $forkFiles) {
    $dst = Join-Path $weaselDir $name
    if ((Test-Path $dst) -and -not (Test-Path "$dst.stock")) { Copy-Item $dst "$dst.stock" }
    Copy-Item (Join-Path "$kit\weasel" $name) $dst -Force
}
Write-Host "Mochi 定制 Weasel 组件已安装（原版备份为 *.stock）"

# ---- 4. Mochi 主目录（brain + 语言模型 + 个人记忆）----
$mochiHome = "$env:LOCALAPPDATA\Mochi"
New-Item -ItemType Directory -Force "$mochiHome\bin", "$mochiHome\artifacts\v0", "$mochiHome\user_data" | Out-Null
Copy-Item "$kit\bin\mochi-brain.exe" "$mochiHome\bin\" -Force
Copy-Item "$kit\artifacts\v0\*" "$mochiHome\artifacts\v0\" -Force
Write-Host "Mochi 主目录：$mochiHome"

# ---- 5. Rime 用户配置（已有同名文件先备份为 *.pre-mochi）----
$rimeUser = "$env:APPDATA\Rime"
New-Item -ItemType Directory -Force $rimeUser | Out-Null
foreach ($name in "default.custom.yaml", "weasel.custom.yaml") {
    $dst = Join-Path $rimeUser $name
    if ((Test-Path $dst) -and -not (Test-Path "$dst.pre-mochi")) { Copy-Item $dst "$dst.pre-mochi" }
}
Copy-Item "$kit\rime-config\*" $rimeUser -Force
Write-Host "输入方案已部署到 $rimeUser"

# ---- 6. brain 开机自启（隐藏窗口，日志写 brain.log）----
$exe = "$mochiHome\bin\mochi-brain.exe"
$log = "$mochiHome\brain.log"
$cmd = "cmd /c """"$exe"" --artifacts ""$mochiHome\artifacts\v0"" --user-data ""$mochiHome\user_data"" >> ""$log"" 2>&1"""
$vbs = "$mochiHome\start-brain.vbs"
# VBS 字符串里双引号需成对转义
$vbsBody = 'CreateObject("WScript.Shell").Run "' + ($cmd -replace '"', '""') + '", 0, False'
Set-Content -Path $vbs -Value $vbsBody -Encoding Default
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" `
    -Name "MochiBrain" -Value "wscript.exe `"$vbs`""
Write-Host "brain 已注册开机自启（HKCU Run / MochiBrain）"

# ---- 7. 立即启动：brain -> 重建索引 -> 启动输入法服务 ----
wscript.exe $vbs
Start-Sleep -Seconds 2
& "$weaselDir\WeaselDeployer.exe" /deploy 2>$null
Start-Sleep -Seconds 3
# 经 explorer 启动，让服务回到普通用户权限（直接启动会继承本脚本的管理员权限）
explorer.exe "$weaselDir\WeaselServer.exe"

Write-Host ""
Write-Host "=== 安装完成 ==="
Write-Host "1. Win+空格 切到「小狼毫」；若候选框上方不是 Mochi，按 F4 选「Mochi」"
Write-Host "2. 直接混打中英文试试：zheshiyigetest -> 这是一个test"
Write-Host "3. 你的个人记忆（全程本地）：$mochiHome\user_data"
Write-Host "4. 卸载/还原：管理员运行同目录 uninstall.ps1"
