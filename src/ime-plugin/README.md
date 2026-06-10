# ime-plugin（librime merged-plugin，C++）

Mochi 的 librime 侧薄插件（ADR-001 方案 B + ADR-003 三语分工）。
`MochiTranslator::Query` 把每键输入经命名管道发给 brain
（[ipc-v0](../../docs/specs/ipc-v0.md)），把返回的 candidates 转成 RIME
候选（text/comment/preedit/quality）。本目录自 M2 起是插件源码的
**single source of truth**（experiments/003 的 mochi/ 已冻结为历史 PoC）。

## 热路径保证（brain_client.cc）

- **15ms 硬超时**：OVERLAPPED I/O + `WaitForSingleObject`，写请求+读响应共用一个 deadline；超时则 `CancelIoEx` 并收割未决 I/O 后立即返回
- **降级**：超时/管道错误 → 本键返回空候选，标记 brain 不可用，armed **2s 退避**——退避期内每键瞬时放弃（实测 0-1µs），绝不阻塞按键、不堆积请求
- **连接复用**：一次 `CreateFile`，跨按键复用；退避到期后下一键自动重连
- 超时后丢弃连接重建（避免迟到响应与后续请求错位）

JSON 解析为手写极简实现（`mini_json.h`，约 200 行）：librime 自带
yaml-cpp 无 JSON 库，v0 只需解析一个小响应对象，不值得引入第三方
单头文件库。支持 object/array/string（含 `\uXXXX` → UTF-8）/number。

环境变量 `MOCHI_QUERY_DELAY_MS`（PoC 兼容）：在 Query 内注入额外同步
延迟，用于手感实验。

日志：每键一行到 stderr/glog —— `e2e=<µs>`（插件端到端）、
`brain=up|down`、`brain_us`（brain 自报处理耗时）、`cands`。

## 构建（一键）

依赖 experiments/003 已就绪的 librime 工作区（deps 已构建）：

```powershell
# 同步本目录到 librime/plugins/mochi 并构建 rime.dll（含三个环境坑的处理）
cmd /c E:\Projects\P029_ai-ime\experiments\003-translator-poc\build-steps.cmd librime
```

首次从零构建（deps 也要编）用 `build-steps.cmd all`。脚本已处理三个
环境坑：ps1 的 UTF-8 BOM、vcvars 切工作目录、`NoDefaultCurrentDirectoryInExePath`。

## 运行 / E2E 验证

```powershell
# 1. 起 brain
Start-Process E:\Projects\P029_ai-ime\src\brain\target\release\mochi-brain.exe

# 2. 喂键流（必须用 rime_console；rime_api_console 的 line_editor 不兼容管道 stdin）
cd E:\Projects\P029_ai-ime\experiments\003-translator-poc\librime\build\bin
Copy-Item ..\..\..\rime-data\*.yaml .
"nihao" | .\Release\rime_console.exe
# 期望 stdout：MOCHI_BRAIN:nihao；stderr 每键一行 [mochi] Query ... e2e=...us brain=up
```

## 代码结构

```
CMakeLists.txt        librime 标准 merged-plugin 布局（构建时被根 CMake 自动收编）
src/mochi_module.cc   RIME_REGISTER_MODULE(mochi) 组件注册
src/mochi_translator.{h,cc}  Query：IPC 调 brain → RIME 候选 + 双侧日志
src/brain_client.{h,cc}      命名管道客户端：硬超时/退避/连接复用
src/mini_json.h       手写极简 JSON 解析 + 转义
```
