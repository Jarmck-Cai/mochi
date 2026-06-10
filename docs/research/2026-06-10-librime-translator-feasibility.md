# 调研报告：基于 librime 自研 translator 插件的技术可行性（ADR-001 方案 B 验证）

- 日期：2026-06-10
- 角色：researcher
- 验证方式：shallow-clone librime（master @ d71168e，2026-06-09，1.17.0 之后）与 weasel（master @ 93eec2d，2026-03-06，0.17.x）源码逐文件核对 + 官方 README/CI 配置交叉验证
- 标注约定：**[源码验证]** = 直接读到代码/官方文档；**[推测]** = 基于经验的估计，未实证

---

## 〇、问题

ADR-001 建议方案 B（自研 translator 接管核心转换，RIME 只当壳）。需验证 librime 插件机制是否支持：候选完全接管、preedit 控制、同步 IPC 出口、异步更新可能性，以及 Weasel 侧配套与替代路径成本。

---

## 一、librime 插件机制：如何注册和加载 **[源码验证]**

### 1.1 组件接口

核心接口在 `src/rime/translator.h`：

```cpp
class Translator : public Class<Translator, const Ticket&> {
 public:
  virtual an<Translation> Query(const string& input, const Segment& segment) = 0;
 protected:
  Engine* engine_;   // 可访问 Context（编辑状态、commit history、caret）
  string name_space_;
};
```

- 返回值 `Translation`（`src/rime/translation.h`）是**惰性候选生成器**（`Next()`/`Peek()`），有现成实现：`UniqueTranslation`、`FifoTranslation`、`UnionTranslation`、`CacheTranslation` 等。
- 候选对象 `Candidate`（`src/rime/candidate.h`）：`text()`（上屏文本）、`comment()`（注释）、`preedit()`（**替换编码区显示的文本**）、`quality()`（参与多 translation 合并排序）。`SimpleCandidate` 直接可用。

### 1.2 注册三层结构：Component → Module → Plugin

1. **Component 注册**（`src/rime/registry.h`、`component.h`）：
   `Registry::instance().Register("ai_translator", new Component<AiTranslator>);`
2. **Module 声明**（`src/rime_api.h` L541 起）：`RIME_REGISTER_MODULE(name)` 宏生成 `RimeModule` 结构（`module_name` / `initialize` / `finalize`），通过 CRT 初始化段自动调用 `RimeRegisterModule()`（MSVC 用 `.CRT$XCU` section，宏已处理，见 rime_api.h L527-532）。模块的 `rime_<name>_initialize()` 里做组件注册。官方最小范例：[librime-sample](https://github.com/rime/librime-sample)（`sample/src/sample_module.cc` 注册 `trivial_translator`，全部代码约 100 行）。
3. **Plugin 加载**，三种方式：
   - **静态合并**（CMake `BUILD_MERGED_PLUGINS=ON`，默认）：把插件源码目录放进 `librime/plugins/<name>/`，根 CMake 自动 glob 收编，编进 rime.dll。librime 官方 release CI 即此路线，固定合并 `hchunhui/librime-lua lotem/librime-octagram rime/librime-predict`（`.github/workflows/release-ci.yml`）。
   - 运行时扫描外置动态库（`ENABLE_EXTERNAL_PLUGINS`，`plugins/plugins_module.cc` 的 `PluginManager::LoadPlugins` 用 boost::dll 加载）。
   - 宿主程序在 `RimeTraits::modules` 显式指定模块列表（rime_api.h L100-101），或 C++ 侧 `rime::LoadModules()`。

### 1.3 关键发现：Windows 不支持运行时外置插件 DLL **[源码验证]**

`plugins/plugins_module.cc` 明文：

```cpp
#ifdef _WIN32
// TODO: implement this when ready to support DLL plugins on Windows.
inline static rime::path current_module_path() { return rime::path{}; }
```

即 Windows 下 plugins 目录自动扫描机制未实现。**Windows 生产路线 = 静态合并构建自己的 rime.dll**（weasel 官方也是同仓构建 librime 子模块，见 weasel `build.bat` 的 `:build_librime_platform`）。这不是阻塞——DESIGN.md 本来就计划 fork——但意味着"我们的 translator"是**编进 rime.dll 的内置模块**，不是即插即用 DLL；升级 librime 需要重新合并构建。

### 1.4 现有第三方插件参照 **[源码/文档验证]**

| 插件 | 注册的组件 | 对我们的参考价值 |
|------|-----------|----------------|
| [librime-lua](https://github.com/hchunhui/librime-lua) | processor/segmentor/translator/filter 全四类 | 证明四类组件都可由插件提供 |
| [librime-predict](https://github.com/rime/librime-predict) | `predictor`(processor) + `predict_translator` | 上屏后注入预测候选——与我们的"续写"同构，可借鉴其触发机制 |
| [librime-octagram](https://github.com/lotem/librime-octagram) | `Grammar` 组件（librime 核心扩展点 `src/rime/gear/grammar.h`：`Query(context, word, is_rear) -> double`） | 语言模型打分插件先例；若将来想只换打分不换词图，这是另一个挂点 |
| [librime-proto](https://github.com/lotem/librime-proto) | CapnProto IPC | 插件内做 IPC 的先例 |

---

## 二、控制能力边界 **[源码验证]**

### 2.1 能否完全接管候选生成（绕过明月拼音）——**能**

`src/rime/engine.cc` `ConcreteEngine::InitializeComponents()`：translator 列表完全由 schema 的 `engine/translators` 配置决定，逐项 `Translator::Require(klass)->Create(ticket)`。**schema 里不写 `script_translator`（明月拼音），它就完全不存在**。我们的 schema 只列 `ai_translator`（+ `punct_translator` 处理标点），候选 100% 出自我们。

`TranslateSegments()`（engine.cc）：对每个未确认 segment，依次调每个 translator 的 `Query`，结果灌进 `Menu`，多个 translation 按 `Translation::Compare`（默认按 quality）交错合并。单一主 translator 时无竞争问题。

### 2.2 能否控制 preedit（嵌入式编码区）——**能**

- `Candidate::preedit()`：注释原文 *"text shown in the preedit area, replacing input string (optional)"*（candidate.h）。
- `src/rime/composition.cc` `Composition::GetPreedit()` 证实：高亮候选的 `preedit()` 非空时**直接替换**该段输入串的显示，还支持 `\t` 分隔光标前/提示后缀。已确认段显示已选候选的 `text()`。
- `Segment::prompt`（segmentation.h）可额外加段级提示文字。
- 结论：编码区想显示"拼音切分注音 / 中英混排提示"完全可控，逐候选粒度。

### 2.3 词图入口的边界：segmentation 与 speller **[源码验证]**

接管不是只有 translator 一件事：
- **speller**（processor）决定哪些按键进入 composition（`speller/alphabet` 配置），标准配置即可让全部字母流进来。
- **segmentor** 决定输入串怎么切段、打什么 tag；translator 的 `Query` 收到的是**一个 segment 的子串**。`abc_segmentor` 对连续字母串给单一 segment（tag "abc"）。中英混打要求整段键流进我们的词图——纯字母键流下成立；若要把数字/符号也纳入词图，需自写 segmentor（同样的组件注册机制，接口 `Segmentor::Proceed(Segmentation*)`，segmentor.h）。**建议默认自带一个极简 segmentor，把"整个未确认输入"打成一个 segment，杜绝切分歧义**。
- 翻页/选择由 `selector`/`navigator` processor 处理，无需动。

### 2.4 filter 与 translator 的流水线位置差异 **[源码验证]**

```
按键 → processors(speller…) → Context 变化 → ConcreteEngine::Compose
  → CalculateSegmentation（segmentors 链）
  → TranslateSegments：每个 segment { 每个 translator → Query → Menu 合并 }
  → Menu 之上挂 filters 链（Filter::Apply，对合并后的候选流做变换）
  → UI 拉取分页（Menu::CreatePage，惰性求值）
```

filter 的接口是 `Apply(an<Translation>, CandidateList*)`——**只能对已存在的候选重排/改写/过滤，不能产生新的切分路径**。"gan→敢 还是 GAN"需要拼音弧与英文弧在词图内统一打分竞争，filter 层拿到的已经是按明月拼音词图解码完的结果，源头丢失。**ADR-001 对 filter 天花板的判断在源码层面成立。**

---

## 三、线程模型与进程外 IPC **[源码验证 + 少量推测]**

### 3.1 librime 调用线程模型——同步、无内部线程，允许阻塞调用

- `RimeProcessKey`（rime_api_impl.h L171-178）→ `Session::ProcessKey` → `ConcreteEngine::ProcessKey` → processor 链 → context update notifier → `Compose()` → `TranslateSegments()` → 我们的 `Query()`。**整条链在调用方线程同步执行，librime 引擎不开自己的线程**（仅 deployer 维护任务有后台线程）。
- 因此 `Query()` 内做同步命名管道/共享内存调用 Brain 是**允许的**，没有回调线程/重入限制。`Service` 的 mutex 只保护 session 表。

### 3.2 每次按键 translator 被调用几次

- 每个**改变 composition 的按键**触发一次 `Compose()`；`Compose` 对每个未确认 segment × 每个 translator 调一次 `Query`。我们的配置下通常 = **每键 1 次**。
- 翻页/高亮移动**不**重新 `Query`——Menu 从既有 Translation 对象惰性拉取（`Menu::Prepare`）。
- 选词确认一段后，剩余未确认段会再触发 Compose/Query。
- Translation 惰性求值意味着：`Query` 可以立刻返回轻量对象，候选在 `Peek/Next`（UI 取页时）才真正计算。理论上可用于延迟取数，但 UI 取页紧跟在同一消息处理内，**实际收益为零，建议 Query 内同步取回 top-N 一次完成**。

### 3.3 Weasel 侧的串行化——本调研最重要的工程警告

`WeaselIPCServer/WeaselServerImpl.cpp` L164-176：

```cpp
static std::mutex g_api_mutex;
auto listener = [this](PipeMessage msg, PipeServer::Respond resp) {
  std::lock_guard guard(g_api_mutex);
  HandlePipeMessage(msg, resp);
};
```

每个客户端连接一个 pipe 线程（`PipeServer::Listen`），但**所有消息处理共用一把全局锁**。我们的 `Query` 若阻塞 200ms，**所有应用的输入全部卡 200ms**。硬性结论：
- Brain IPC 必须**硬超时**（建议 15-20ms，对齐 DESIGN.md 热路径预算）；
- 超时/Brain 不在 → 立即降级（返回字母原文或本地兜底词典），不可无限等；
- 共享内存 + 事件通知优于命名管道往返（DESIGN.md 已是此方案，<1ms 可达成 **[推测：量级合理，未实测]**）。

### 3.4 有没有异步更新候选的机制——librime 没有，但有更好的出路

- librime 核心**没有**"事后推送刷新候选"的机制：候选只在 Compose 时生成；`RimeSetNotificationHandler` 的消息通道（service.h）只用于 deploy/schema/option 类通知。
- weasel 的 UI 刷新（`RimeWithWeaselHandler::_UpdateUI`）只在处理一条 IPC 消息（按键、focus 等）后调用。
- **但是**：WeaselUI（候选窗）就活在 WeaselServer.exe 进程内（见下节）。**异步续写预览根本不必经过 librime 的候选机制**——Brain 算完后直接通知 WeaselServer 进程内我们的代码，更新 UI 数据结构并触发重绘即可（fork 内改动）。需要注意 UI 窗口线程亲和性（pipe 线程 → UI 线程 marshal）**[推测：需要实验确认 weasel UI 的线程约束]**。
- 整句重排的"温和提示"同理可走 UI 层，或等下一键自然刷新。

---

## 四、Weasel 侧 **[源码验证]**

### 4.1 重要拓扑更正（建议回写 DESIGN.md）

DESIGN.md 第一节把 "Weasel(TSF) + librime + 我们的插件" 画在**每个宿主应用进程内**——**与实际不符**。weasel 实际架构：

```
宿主应用进程：WeaselTSF.dll（薄 TSF 客户端）
      │ 命名管道（WeaselIPC，全局锁串行）
WeaselServer.exe（单一常驻进程）：
      RimeWithWeaselHandler → rime.dll（librime + 我们的 translator）
      WeaselUI（候选窗，DirectWrite 自绘，也在此进程）
      │ 这里再 IPC → Brain 服务
```

证据：`RimeWithWeasel/RimeWithWeasel.cpp` `ProcessKeyEvent` 在 server 进程内同步调 `rime_api->process_key`；WeaselTSF 只做按键转发与上屏。影响：
- 我们的插件崩溃**不会带崩宿主应用**，只崩输入法 server（自动重启可恢复）——比 DESIGN.md 假设的风险更低；
- Brain IPC 连接数 = 1（server↔brain），不是每应用一条；
- 三进程拓扑实际是四进程（宿主内 TSF 薄层 + WeaselServer + Brain + LLM）。

### 4.2 候选窗自定义能力——加灰色续写预览行：可行，fork 级改动

- UI 代码：`WeaselUI/`（`WeaselPanel.cpp` + `StandardLayout`/`HorizontalLayout`/`VerticalLayout`/`VHorizontalLayout`/`FullScreenLayout`，DirectWrite/Direct2D 渲染，支持色彩方案、圆角、阴影、字体配置）。
- 数据结构：`include/WeaselIPCData.h` `weasel::Context{ preedit, aux, cinfo }`，`CandidateInfo{ candies[], comments[], labels[] }`——候选窗已支持每候选的注释（comment）与标签。
- 加一行灰色续写预览 = 扩展 `Context`（如加 `ghost_text` 字段）+ 在 `WeaselPanel`/Layout 里多画一行灰字。全部在我们 fork 的进程内代码，**零系统兼容性风险**，与 DESIGN.md "阶段 1：候选窗内预览" 的判断一致。短期甚至可以先借用首候选 `comment` 渲染验证效果。

### 4.3 版本配套与 Windows 构建工具链

- **配套方式**：weasel 以 git submodule 固定 librime（master 当前 pin 在 commit `1c23358`），`build.bat all` 同仓先构建 librime（x64 + Win32 双架构）再构建 weasel；也可用 `get-rime.ps1` 拉 librime 官方预编译 release（含插件合并版）。weasel 0.16.0 → librime 1.11.2，weasel 最新 0.17.4（2025-06）；librime 最新 1.17.0（2026-06-05）。rime C API 带版本化 struct（`data_size` 协议），小版本升级平滑。
- **librime 要求**（README-windows.md）：VS2022 或 LLVM 16，Boost ≥1.83，CMake ≥3.10，Python（OpenCC 词典）。
- **weasel 要求**（INSTALL.md）：VS2017+ 且需 **ATL/MFC** 组件（仓库内置 env.vs2019.bat / env.vs2022.bat），Boost ≥1.60，cmake，clang-format ≥17.0.6，NSIS（装包），Git-bash（plum 数据）。
- **注意双架构**：weasel 必须同时出 x64 和 Win32（32 位宿主应用仍在），我们的 translator 及其 Brain-IPC 客户端代码须 x86/x64 双编译；另有 ARM64（arm64x_wrapper/，可后置）。

---

## 五、替代路径成本：自研 TSF 输入法

**[文档验证 + 推测]**

- 参照 [PIME](https://github.com/EasyIME/PIME)：C++ 的 libIME（TSF wrapper）+ PIMETextService 骨架进程内 TSF 服务，后端 Python/Node 经命名管道。证明"薄 TSF 壳 + 进程外大脑"模式成立；其 repo 47% 是 C++（TSF 层不薄）。
- weasel 自己的 `WeaselTSF/` 也是一套完整 TSF 实现（composition 管理、各兼容模式、UWP 沙箱应用支持等数十个源文件）。
- **量级估计 [推测]**：从零做到"日用可靠"的 TSF 文本服务（composition 生命周期、候选窗定位、UWP/管理员进程/游戏全屏等兼容长尾、安装注册签名）约 3-6 人月起步，长尾兼容性再数月；且要重建 weasel 已免费提供的全部（候选窗渲染、配置、部署、托盘、更新）。
- **更现实的 Plan B**：若 librime 出现硬阻塞，不必从零做 TSF——**fork weasel 后摘除 librime**，让 WeaselServer 直接对接 Brain（保留 WeaselTSF + WeaselIPC + WeaselUI）。工作量远小于自研 TSF。本次调研未发现需要走到这一步的阻塞。

---

## 六、对 ADR-001 的最终建议

**方案 B 成立，建议定案。** 所需控制点全部在源码层面验证可达：

| 需求 | 结论 |
|------|------|
| 候选完全接管（绕过明月拼音） | ✔ schema 只列自家 translator 即可 |
| preedit/编码区控制 | ✔ `Candidate::preedit()` 逐候选控制 |
| 插件注册机制 | ✔ Registry/Module 机制成熟，librime-sample 全套范例 |
| Query 内同步 IPC | ✔ 同步单线程模型，无重入限制 |
| 异步续写显示 | librime 无此机制，但 WeaselUI 同进程，绕过 librime 直接画 UI 即可 |

**定案附带的注意事项（按重要性）：**

1. **Windows 无运行时插件 DLL**：必须 fork/同仓构建 rime.dll（静态合并），接受"升级 librime = 重新合并构建"的维护成本。weasel 官方构建流程现成支持。
2. **`g_api_mutex` 全局串行**：Query 内阻塞会冻结所有应用的输入。Brain IPC 硬超时（15-20ms）+ 无 Brain 降级是不可妥协的设计约束。
3. **续写预览走 UI 层而非候选机制**：异步结果直接驱动 WeaselUI 重绘，注意 UI 线程 marshal。
4. **更新 DESIGN.md 拓扑**：librime 与我们的插件运行在 WeaselServer.exe，不在宿主应用内（风险更低，IPC 更简单）。
5. **自带极简 segmentor**：保证整段键流进单一 segment，词图切分主权完全收归 Brain。

## 七、遗留的未验证项

1. **未实际编译运行**最小 translator 插件——建议立项实验（合并 librime-sample 改造版进 rime.dll，挂 weasel 实测：注册生效、Query 调用频率、preedit 行为、带 20ms 人工延迟时的打字手感）。
2. `abc_segmentor` 对长混合键流（含数字、撇号分隔）的实际切分行为未实测。
3. weasel UI 的线程约束（pipe 线程直接调 `m_ui->Update` 是否安全）未深查，影响异步续写刷新的实现方式。
4. librime 1.17.0 与 weasel master 的精确 API 兼容性未逐版本核对（weasel pin 的 submodule commit 与 1.17.0 的差异）。
5. 自研 TSF 的 3-6 人月估计为经验推测，未做任务分解。
6. ARM64 构建链未验证（可后置）。

## 附：证据来源

- librime 源码（master @ d71168e）：`src/rime/translator.h`、`filter.h`、`component.h`、`registry.h`、`module.h`、`engine.cc`、`candidate.h`、`composition.cc`、`segmentation.h`、`segmentor.h`、`translation.h`、`menu.h`、`service.h`、`rime_api.h`、`rime_api_impl.h`、`plugins/plugins_module.cc`、`plugins/CMakeLists.txt`、`sample/`、`src/rime/gear/grammar.h`、`.github/workflows/release-ci.yml`、`README-windows.md`
- weasel 源码（master @ 93eec2d）：`RimeWithWeasel/RimeWithWeasel.cpp`、`WeaselIPCServer/WeaselServerImpl.cpp`、`include/WeaselIPCData.h`、`WeaselUI/`、`INSTALL.md`、`build.bat`、`get-rime.ps1`
- 在线：[librime](https://github.com/rime/librime) · [librime-sample](https://github.com/rime/librime-sample) · [librime-lua](https://github.com/hchunhui/librime-lua) · [librime-predict](https://github.com/rime/librime-predict) · [librime-octagram](https://github.com/lotem/librime-octagram) · [weasel](https://github.com/rime/weasel) · [weasel INSTALL.md](https://github.com/rime/weasel/blob/master/INSTALL.md) · [PIME](https://github.com/EasyIME/PIME) · [weasel 0.16.0 release notes](https://newreleases.io/project/github/rime/weasel/release/0.16.0)
