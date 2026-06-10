# Mochi translator PoC 一键构建脚本（在装好 VS2022 Build Tools + CMake 后运行）
# 用法：powershell -ExecutionPolicy Bypass -File build-all.ps1
# 可选：-DelayMs 15  （运行 console 时模拟 Query 内 15ms 同步延迟）
param(
    [int]$DelayMs = 0,
    [switch]$SkipBuild   # 已构建过，只刷数据并启动 console
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$librime = Join-Path $root 'librime'

# ---- 0. 工具链检查 ----
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) {
    throw "未找到 Visual Studio。请安装 VS2022 Build Tools（含 C++ 工作负载），见 README.md 工具链一节。"
}
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { throw "VS 已装但缺少 C++ (MSVC v143) 工作负载。" }
"VS: $vsPath"
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    # VS 自带 CMake 兜底
    $vsCMake = Join-Path $vsPath 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
    if (Test-Path "$vsCMake\cmake.exe") { $env:PATH = "$env:PATH;$vsCMake" }
    else { throw "未找到 CMake（PATH 与 VS 内置均无）。" }
}
cmake --version | Select-Object -First 1

# ---- 1. 同步插件源码（M2 起 single source of truth 为 src/ime-plugin；本目录 mochi/ 是冻结的历史 PoC）----
$pluginSrc = Join-Path $root '..\..\src\ime-plugin'
Remove-Item -Recurse -Force (Join-Path $librime 'plugins\mochi') -ErrorAction SilentlyContinue
Copy-Item -Recurse -Force $pluginSrc (Join-Path $librime 'plugins\mochi')

# ---- 2. Boost ----
# librime 的 install-boost.bat 依赖 aria2c + 7z。若没有，这里用 PowerShell 直接下载解压。
$boostVer = '1.89.0'
$boostDir = Join-Path $librime "deps\boost-$boostVer"
if (-not (Test-Path "$boostDir\boost")) {
    $tarball = "boost_$($boostVer -replace '\.','_')"
    $url = "https://archives.boost.io/release/$boostVer/source/$tarball.tar.gz"
    $tgz = Join-Path $env:TEMP "$tarball.tar.gz"
    "下载 Boost $boostVer （约 140MB）..."
    Invoke-WebRequest -Uri $url -OutFile $tgz
    "解压..."
    tar -xzf $tgz -C (Join-Path $librime 'deps')
    Rename-Item (Join-Path $librime "deps\$tarball") "boost-$boostVer"
    # cd /d inside cmd: do not rely on cwd inheritance (breaks in nested hosts)
    cmd /c "cd /d `"$boostDir`" && bootstrap.bat && b2 headers"
    if ($LASTEXITCODE) { throw "Boost bootstrap/headers failed (exit $LASTEXITCODE)" }
}

# ---- 3. 构建 deps + librime（Developer 环境下跑官方 build.bat）----
if (-not $SkipBuild) {
    $vcvars = Join-Path $vsPath 'VC\Auxiliary\Build\vcvars64.bat'
    # vcvars (VsDevCmd) may change the working directory -> cd AFTER it, not before
    $env:VSCMD_START_DIR = $librime
    cmd /c "`"$vcvars`" && cd /d `"$librime`" && build.bat deps"
    if ($LASTEXITCODE) { throw "deps 构建失败（exit $LASTEXITCODE）" }
    cmd /c "`"$vcvars`" && cd /d `"$librime`" && build.bat librime"
    if ($LASTEXITCODE) { throw "librime 构建失败（exit $LASTEXITCODE）" }
}

# ---- 4. 部署最小 schema 并启动 rime_api_console ----
$bin = Join-Path $librime 'build\bin'
if (-not (Test-Path "$bin\Release\rime_api_console.exe")) { throw "未找到 rime_api_console.exe" }
Copy-Item -Force (Join-Path $root 'rime-data\*.yaml') $bin
# 清掉旧部署产物，保证 schema 重新编译
Remove-Item -Recurse -Force "$bin\build" -ErrorAction SilentlyContinue

if ($DelayMs -gt 0) { $env:MOCHI_QUERY_DELAY_MS = "$DelayMs" } else { Remove-Item Env:MOCHI_QUERY_DELAY_MS -ErrorAction SilentlyContinue }
"启动 rime_api_console（工作目录 $bin）。输入字母键流（如 nihao）回车；exit 退出。"
Push-Location $bin
& "$bin\Release\rime_api_console.exe"
Pop-Location
