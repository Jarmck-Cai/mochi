# Mochi Windows 发布打包（开发机用）。从各构建产物收集文件，产出
# dist\mochi-windows-x64-<ver>.zip。个人数据（user_data、personal.tsv、
# commits.jsonl）有防呆检查，永不入包。
param([string]$Version = "v0.2.0-alpha")
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..").Path

$name = "mochi-windows-x64-$Version"
$stage = "$repo\dist\$name"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force -Confirm:$false }
New-Item -ItemType Directory -Force "$stage\bin", "$stage\weasel", "$stage\artifacts\v0", "$stage\rime-config" | Out-Null

# brain
Copy-Item "$repo\src\brain\target\release\mochi-brain.exe" "$stage\bin\"

# 定制 weasel 组件：fork 构建产物 + 合并了 mochi 插件的 rime.dll
$forkOut = "$repo\experiments\weasel-fork-probe\output"
foreach ($f in "WeaselServer.exe", "weasel.dll", "weaselx64.dll", "WeaselDeployer.exe") {
    Copy-Item "$forkOut\$f" "$stage\weasel\"
}
Copy-Item "$repo\experiments\003-translator-poc\librime\dist\lib\rime.dll" "$stage\weasel\"

# 通用语言模型（meta.json 注明全部来自公共语料）
Copy-Item "$repo\artifacts\v0\*" "$stage\artifacts\v0\"

# 输入方案与配色
foreach ($f in "mochi.schema.yaml", "default.custom.yaml", "weasel.custom.yaml") {
    Copy-Item "$repo\experiments\004-weasel-deploy\$f" "$stage\rime-config\"
}

# 安装套件
Copy-Item "$PSScriptRoot\windows\install.ps1", "$PSScriptRoot\windows\uninstall.ps1", "$PSScriptRoot\windows\README.md" $stage

# 防呆：个人数据绝不入包
$leak = Get-ChildItem $stage -Recurse -File | Where-Object {
    $_.Name -in @("personal.tsv", "commits.jsonl") -or $_.FullName -match "user_data"
}
if ($leak) { throw "个人数据混入发布包：$($leak.FullName -join ', ')" }

$zip = "$repo\dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force -Confirm:$false }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
$size = (Get-Item $zip).Length / 1MB
Write-Host ("打包完成：{0}  ({1:N1} MB)" -f $zip, $size)
Write-Host "SHA256: $hash"
