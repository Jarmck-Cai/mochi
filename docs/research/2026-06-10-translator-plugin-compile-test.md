# 实测报告：最小 translator 插件编译实测（ADR-001 定案前最后一环）

- 日期：2026-06-10
- 角色：implementer
- 实验目录：`experiments/003-translator-poc/`
- 前置：`docs/research/2026-06-10-librime-translator-feasibility.md`（源码调研）
- **结论先行：本机无任何 C++ 工具链（无 MSVC/LLVM/MinGW、无 CMake），按任务约束未做系统级安装，编译与运行实测未能执行。** 已完成全部可离线完成的准备：librime 源码树（含 deps 子模块）就位、MochiTranslator 插件源码写齐并按 merged-plugin 机制挂入 `plugins/mochi/`、最小 schema 与一键构建脚本就绪——装好工具链后预计一条命令、30-60 分钟出结果。

---

## 一、各验证目标结果

### 目标 1：工具链检查 —— ✓ 完成（结果：缺核心工具链）

| 工具 | 状态 | 证据 |
|------|------|------|
| MSVC / VS Build Tools | **✗ 无** | `vswhere.exe` 不存在；`C:\Program Files\Microsoft Visual Studio`、`(x86)` 同名目录、`C:\BuildTools` 均不存在；`cl`/`clang-cl` 不在 PATH |
| LLVM / MinGW | **✗ 无** | `C:\Program Files\LLVM`、`C:\msys64`、`C:\mingw64` 均不存在；`gcc`/`g++`/`clang` 不在 PATH |
| CMake | **✗ 无** | PATH 与 `C:\Program Files\CMake` 均无 |
| Python | **✗ 仅 Store 占位 stub** | `python --version` 退出码 49（WindowsApps stub）；OpenCC 词典构建需要真 Python |
| git | ✓ | `C:\Program Files\Git\cmd\git.exe` |
| winget | ✓ | v1.28.240（可用于装上述全部） |

**需安装清单**（按 librime README-windows.md 要求：VS2022 或 LLVM16、Boost≥1.83、CMake≥3.10、Python）：

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
winget install Kitware.CMake
winget install Python.Python.3.12
```

Boost 无需预装（构建脚本自动下载源码并 `b2 headers`，librime 以 header + 自动 b2 方式用 Boost）。

### 目标 2：librime 构建 —— ✗ 未执行（工具链缺失）；源码与脚手架 100% 就绪

已完成的部分：

- shallow-clone librime master @ `d71168e`（2026-06-09，与前期调研同一 commit，结论可直接沿用）到 `experiments/003-translator-poc/librime/`
- 六个 deps 子模块全部 `--depth 1` init 成功：glog / googletest / leveldb / marisa-trie / opencc / yaml-cpp
- 核对官方构建路径（`README-windows.md` + `build.bat` 逐行读过）：`install-boost.bat`（注意：依赖 aria2c+7z，本机也没有，已在 `build-all.ps1` 里改用 Invoke-WebRequest + 系统自带 tar 替代）→ `build.bat deps` → `build.bat librime`，产物 `build\bin\Release\rime.dll` + `rime_api_console.exe`
- `env.bat` 已写好（官方模板默认 `ARCH=Win32`/VS2019，已改 x64/VS2022/v143——这是个容易踩的坑）
- 一键脚本 `build-all.ps1`：工具链探测（vswhere）→ Boost 下载 → vcvars64 环境下跑官方 build.bat → 部署最小 schema → 启动 console

### 目标 3：最小 translator —— ✓ 源码完成（未编译验证）

源码在 `experiments/003-translator-poc/mochi/`（构建时同步拷贝到 `librime/plugins/mochi/`）：

- `MochiTranslator::Query`：对 `abc` tag 的 segment 返回 `UniqueTranslation(SimpleCandidate("mochi", start, end, "MOCHI_POC", "len=N"))`，并 `set_preedit("«input»")` 顺带验证逐候选 preedit 控制；`steady_clock` 计时 + 实例级调用计数，stderr 与 glog 双路日志；环境变量 `MOCHI_QUERY_DELAY_MS` 注入模拟同步延迟（目标 5 用）
- 注册机制按调研报告落地并经本次源码二次核对确认：`plugins/<dir>/` 被根 CMake `file(GLOB)` 自动收编（`plugins/CMakeLists.txt` L33-40）；插件 CMake 导出 `plugin_modules "mochi"` → 根 `CMakeLists.txt` L263-271 拼成 `RIME_SETUP_EXTRA_MODULES` → `src/CMakeLists.txt` L173-175 注入编译宏 `RIME_EXTRA_MODULES` → `setup.cc` L36-39 并入 `kDefaultModules`、`rime_api.cc` L46/54 静态强引用模块符号。**即：放目录即自动注册+自动加载，零 librime 源文件改动**——比调研报告"加进默认 module 列表"的预期更干净
- `RIME_REGISTER_MODULE` 的 MSVC `.CRT$XCU` 段机制已在 `rime_api.h` 确认存在，但**未经实际 MSVC 编译链接验证**（静态初始化在 MSVC 下被链接器丢弃是经典坑，`rime_api.cc` 的强引用机制理论上规避了它，需实测确认）

### 目标 4：运行验证 —— ✗ 未执行

最小 schema 已备好（`rime-data/`）：`mochi.schema.yaml` 的 `engine/translators` 只列 `mochi_translator`（不含 script_translator，候选应 100% 出自我们）；`default.yaml` 只挂 mochi 方案。验证步骤已写入实验 README（候选出现 / stderr 数 Query 次数 / 读单次耗时）。

### 目标 5：15ms 延迟手感 —— ✗ 未执行

机制已内置（`MOCHI_QUERY_DELAY_MS=15`，`build-all.ps1 -DelayMs 15`）。注意 rime_api_console 是整行 `simulate_key_sequence`，等价于"连续极速按键"的最坏情形——n 字符行回显延迟 ≈ n×15ms，这其实是比真人打字更严苛的压力测试；真实手感仍需挂 weasel 实测。

## 二、对 ADR-001 定案的建议

**本次实验没有产生任何反向证据，但"待编译实测后定案"的条件严格说未满足。** 两个选项：

1. **（建议）带条件定案**：源码层面五项控制点（前期调研）+ 本次插件机制全链路二次核对（GLOB→EXTRA_MODULES→kDefaultModules→静态强引用）均无阻塞迹象，且官方 release CI 长期以同一机制合并 lua/octagram/predict 三插件（Windows 在内），机制本身经过大规模生产验证。可将 ADR-001 置为 Accepted，附注"编译实测因本机工具链缺失顺延至 M2 第一项任务，脚手架已就绪（experiments/003），若实测翻车（概率很低）则回退 Proposed"。
2. 保持 Proposed，装好工具链（约 10 分钟 winget + 30-60 分钟构建）跑完 `build-all.ps1` 再定案。

真正剩余的不确定性只有两点，且都不是架构级风险：MSVC 下 `.CRT$XCU` 模块自注册的实际行为（有 rime_api.cc 强引用兜底）、15ms 同步延迟的真实手感（有 g_api_mutex 串行的已知约束，设计上已按 15-20ms 硬超时规划）。

## 三、对 M2 工程的具体建议（构建系统组织）

1. **插件源码独立目录 + 构建期拷贝/junction 进 `librime/plugins/`**：本实验采用"`mochi/` 为 single source of truth，脚本同步到 `librime/plugins/mochi/`"模式，M2 沿用——我们的 translator 仓库目录不该埋在 librime fork 里，CI 一步 `Copy-Item`（或 `New-Item -ItemType Junction`）即可参与 merged 构建。librime 也支持 `RIME_PLUGINS` 环境变量按 `owner/repo` 列表自动 clone（CI 用法，见 `action-install-plugins-windows.bat`）
2. **不要用官方 `install-boost.bat`**（依赖 aria2c+7z），用 `build-all.ps1` 的 Invoke-WebRequest+tar 方案或 CI cache；Boost 现行版本 1.89.0（README 写 ≥1.83）
3. **`env.bat` 必须显式写 `ARCH=x64`**：官方模板默认 Win32。M2 接 weasel 时按其 `build.bat` 双架构流程出 x64+Win32 两份 rime.dll；本 PoC 只跑 x64
4. **MSVC 静态运行时**：librime 官方 Windows 构建用 `/MT`（`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded`，build.bat L108）。我们插件内引入的任何第三方库（将来 Brain IPC 客户端）必须同样 /MT，否则链接冲突——选依赖时提前过滤
5. **日志通道**：glog 在 librime 内已配好（`ENABLE_LOGGING=ON`），插件内 `LOG(INFO)` 直接可用；生产环境建议插件自有日志走独立文件，避免淹没在 rime 日志里
6. **构建时间预算**：deps（glog/leveldb/yaml-cpp/gtest/marisa/opencc）+ librime 本体在普通开发机约 20-40 分钟，Boost headers 另需下载 140MB；CI 必须 cache deps 安装产物（librime 官方 CI 即如此）

## 四、留档：关键命令序列

```powershell
# 工具链探测（本次实测均为阴性）
Get-Command cmake,cl,clang-cl,gcc -ErrorAction SilentlyContinue   # → 全部 NOT FOUND
Test-Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"  # → False

# 源码准备（已执行成功）
git clone --depth 1 https://github.com/rime/librime experiments\003-translator-poc\librime
#   → master @ d71168e9 (2026-06-09)
git -C experiments\003-translator-poc\librime submodule update --init --depth 1
#   → glog/googletest/leveldb/marisa-trie/opencc/yaml-cpp 全部 checked out
Copy-Item -Recurse mochi librime\plugins\mochi

# 待工具链就绪后（未执行）
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
winget install Kitware.CMake; winget install Python.Python.3.12
powershell -ExecutionPolicy Bypass -File experiments\003-translator-poc\build-all.ps1
powershell -ExecutionPolicy Bypass -File experiments\003-translator-poc\build-all.ps1 -SkipBuild -DelayMs 15
```

## 五、证据来源（本次新核对的源码位置，均为 librime master @ d71168e 本地克隆）

- `plugins/CMakeLists.txt` L33-40（目录 GLOB 自动收编）、L48-52（merged 分支聚合 objs/modules）
- `CMakeLists.txt` L263-271（plugin_modules → RIME_SETUP_EXTRA_MODULES）
- `src/CMakeLists.txt` L173-175（RIME_EXTRA_MODULES 编译宏注入）
- `src/rime/setup.cc` L36-39（kDefaultModules 并入 extra modules）
- `src/rime_api.cc` L44-54（静态库下模块符号强引用，规避链接器丢弃）
- `sample/`（插件 CMake 布局与 trivial_translator 范例，MochiTranslator 的直接参照）
- `build.bat`、`install-boost.bat`、`env.bat.template`、`env.vs2022.bat`、`README-windows.md`（构建流程与 ARCH=Win32 默认值之坑）
- `tools/rime_api_console.cc` L219-260（traits 不设数据目录→工作目录部署；整行 simulate_key_sequence）
