# 实验 003：最小 translator 插件编译实测（ADR-001 方案 B 最后一环）

- 日期：2026-06-10
- 状态：**编译实测通过，ADR-001 定案 Accepted**（实测数据见文末与 ADR-001"编译实测结果"节）
- 目标：把 MochiTranslator（对任意输入返回固定候选 `MOCHI_POC`）静态合并进
  rime.dll，用 rime_api_console 验证候选接管、Query 调用频率、单次耗时、
  以及模拟 15ms 同步延迟下的手感。

## 目录结构

```
003-translator-poc/
├── mochi/                  ← MochiTranslator 插件源码（single source of truth）
│   ├── CMakeLists.txt      ← librime 标准 merged-plugin 布局
│   └── src/
│       ├── mochi_module.cc      ← RIME_REGISTER_MODULE(mochi) + 组件注册
│       ├── mochi_translator.h
│       └── mochi_translator.cc  ← Query：固定候选 + preedit 控制 + 计数/耗时日志
├── rime-data/              ← 最小 RIME 数据（default.yaml + mochi.schema.yaml）
├── build-all.ps1           ← 一键：工具链检查→Boost→deps→librime→部署→启动 console
├── librime/                ← shallow clone（master @ d71168e，deps 子模块已 init）
│   ├── env.bat             ← 已写好（x64 / VS2022 / v143）
│   └── plugins/mochi/      ← mochi/ 的拷贝（构建时被根 CMake 自动 glob 收编）
└── README.md
```

## 前置工具链（本机目前缺的就是这些）

| 工具 | 版本要求 | 安装方式 |
|------|---------|---------|
| VS 2022 Build Tools | MSVC v143 + Win11 SDK | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"` |
| CMake | ≥ 3.10 | `winget install Kitware.CMake`（或 VS 内置 CMake 组件） |
| Python | ≥ 2.7（OpenCC 词典） | `winget install Python.Python.3.12`（当前 PATH 里是 Store 占位 stub，不可用） |
| git | 任意近期版本 | 已有（`C:\Program Files\Git\cmd\git.exe`） |

Boost **不需要**预装：`build-all.ps1` 会自动下载 boost 1.89.0 源码到
`librime\deps\boost-1.89.0` 并 `b2 headers`（librime 自带的 `install-boost.bat`
依赖 aria2c + 7z，本脚本改用 PowerShell `Invoke-WebRequest` + Windows 自带 tar，
不引入额外依赖）。

## 复现步骤（从零到跑通）

```powershell
# 0. 安装上表工具链（仅首次）

# 1. 获取代码（本目录已完成，重做时参考）
git clone --depth 1 https://github.com/rime/librime experiments\003-translator-poc\librime
git -C experiments\003-translator-poc\librime submodule update --init --depth 1

# 2. 一键构建 + 启动测试 console（boost 下载 + deps + librime，首次约 30-60 分钟）
cd experiments\003-translator-poc
powershell -ExecutionPolicy Bypass -File build-all.ps1

# 3.（加分项）模拟 Query 内 15ms 同步延迟再跑一次
powershell -ExecutionPolicy Bypass -File build-all.ps1 -SkipBuild -DelayMs 15
```

手工等价命令（不用脚本时，在 *x64 Native Tools Command Prompt for VS 2022* 中）：

```bat
cd experiments\003-translator-poc\librime
rem env.bat 已写好（x64/VS2022）；boost 放 deps\boost-1.89.0
install-boost.bat        rem 需要 aria2c + 7z；或按上文用脚本下载
build.bat deps           rem glog/leveldb/yaml-cpp/gtest/marisa/opencc
build.bat librime        rem 产出 build\bin\Release\rime.dll + rime_api_console.exe
copy ..\rime-data\*.yaml build\bin\
cd build\bin
Release\rime_api_console.exe
```

## 验证方法

console 起来后（首次会自动部署 schema，留意 stderr 出现
`[mochi] module 'mochi' initialized`）：

1. 输入 `nihao` 回车 → 候选 1 应为 `MOCHI_POC`，编码区显示 `«nihao»`
   （证明候选接管 + 逐候选 preedit 控制；schema 未配置 script_translator）
2. stderr 每行 `[mochi] Query #N input='...' seg=[a,b) Xus`：
   - N 的增长速度 = 每键 Query 次数（预期 1 次/键；console 是整行
     `simulate_key_sequence`，逐字符模拟按键，仍可数出每字符的调用数）
   - `Xus` = 单次 Query 往返耗时
3. `-DelayMs 15` 模式下重复，观察整行回显延迟是否 ≈ 15ms × 字符数
   （console 一次喂整行，等价于连续快速按键的最坏情形）

## MochiTranslator 源码要点

- `mochi/src/mochi_translator.cc`：`Query()` 只响应 `abc` tag 的 segment；
  `SimpleCandidate("mochi", start, end, "MOCHI_POC", comment)` +
  `set_preedit("«input»")`；`steady_clock` 计时 + 调用计数，stderr 与
  glog 双路输出；环境变量 `MOCHI_QUERY_DELAY_MS` 注入模拟延迟。
- `mochi/src/mochi_module.cc`：`Registry::Register("mochi_translator", ...)`，
  `RIME_REGISTER_MODULE(mochi)`。merged-plugin 构建下模块名被 CMake 注入
  `RIME_EXTRA_MODULES`（librime 根 `CMakeLists.txt` L263-271 →
  `src/rime/setup.cc` `kDefaultModules`），随 `RimeInitialize` 自动加载，
  **无需改 librime 任何源文件**。
- `rime-data/mochi.schema.yaml`：`engine/translators` 只列 `mochi_translator`。

## 注意事项

- `librime/` 整个目录是实验产物（含未来的 build/、deps/boost），不要提交。
- 改插件源码请改 `mochi/`，`build-all.ps1` 每次会同步拷贝到
  `librime/plugins/mochi/`。
- librime 官方模板 `env.bat.template` 默认 `ARCH=Win32`，本实验 `env.bat`
  已改为 `x64`——weasel 生产构建需要 x64 + Win32 双架构，到 M2 再处理。

## 实测结果（2026-06-10）

构建：`cmd /c build-steps.cmd all`（VS Build Tools 2022 + VS 内置 CMake 3.31）。
环境坑三枚：ps1 要 UTF-8 BOM；vcvars/VsDevCmd 会切工作目录（cd 放其后）；
开发会话注入 `NoDefaultCurrentDirectoryInExePath=1`（build-steps.cmd 内已清除）。

测试：`rime_console.exe < testinput.txt`（注意 **rime_api_console 的 line_editor
不兼容重定向 stdin**，会读到乱码键流；管道测试必须用 rime_console）。

| 验证项 | 结果 |
|---|---|
| 静态注册（.CRT$XCU） | ✓ `[mochi] module 'mochi' initialized` |
| 候选接管 + preedit 控制 | ✓ 候选 1 = MOCHI_POC，编码区 «input» |
| 每键 Query 次数 | ✓ 恰好 1 次（n→ni→nih→niha→nihao 增量） |
| 单次 Query 插件开销 | ✓ <1μs |
| 15ms 模拟延迟 | ✓ 每键 15.5-16.5ms，偶发 30ms 尖刺（commit 后首键）；整行连续键流无阻塞无崩溃 |
