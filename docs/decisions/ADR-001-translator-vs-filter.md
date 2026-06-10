# ADR-001: RIME 接入深度——filter 还是 translator

- 状态：Accepted（2026-06-10 定案；当日编译实测通过，条件解除，见"编译实测结果"）
- 日期：2026-06-10
- 决策者：用户 + Claude

## 背景

基于 RIME/Weasel 构建输入法，存在两个接入深度，决定了核心转换能力归谁所有。中英混打是第一优先级痛点（README 第二节）。

## 选项

**方案 A（filter）**：RIME 自带翻译器产出候选，我们做重排 + 个人词注入。
- 优点：实现快，改动小
- 缺点：明月拼音整句转换基线弱于主流商业输入法，基线差则个性化层救不回来；中英混打词图不在我们手里

**方案 B（translator）**：接管核心转换——自建词图、自己解码，RIME 只当壳（按键处理、候选窗、TSF 对接）。
- 优点：词图、个性化打分、置信度输出全部可控
- 缺点：工作量大一截，整句转换基线质量需要自己负责（KenLM 语料与调参）

## 决策（建议）

**方案 B。** 决定性理由：`ai`/`gan`/`fan`/`sun`/`bang`/`man` 等串既是合法拼音又是英文单词，"我想用gan"是"敢"还是"GAN"必须在解码词图内部让拼音弧与英文术语弧用统一打分函数竞争才能做对——filter 架构做不到，而这正是第一优先级痛点的架构前提。

## 后果

- 承担整句转换基线质量的全部责任（风险清单第 1 项）
- 若 librime 插件机制存在硬限制，备选路径是自研 TSF 输入法（工作量大幅上升，需新 ADR）

## 验证补充（2026-06-10 源码调研，详见 docs/research/2026-06-10-librime-translator-feasibility.md）

- ✅ `rime::Translator::Query` + Registry/Module 注册机制支持候选全接管（schema 不配置明月拼音即可绕过）、逐候选 preedit 控制
- ✅ librime 单线程同步模型，每键约调用 translator 1 次，Query 内同步 IPC 可行
- ⚠️ Windows 下运行时外置插件 DLL 未实现（源码明文 TODO）：自研 translator 必须**静态合并构建进 rime.dll**
- ⚠️ WeaselServer 全局 `g_api_mutex` 串行处理所有应用按键：Brain IPC 必须带 15-20ms 硬超时 + 无 Brain 降级，否则冻结全系统输入
- ℹ️ 拓扑修正：插件运行在 WeaselServer.exe 常驻进程而非宿主应用内，崩溃不带崩宿主；异步续写预览可在同进程 WeaselUI 层绘制
- ✅ 插件机制二次核对（2026-06-10 编译实测准备，experiments/003-translator-poc/）：merged-plugin 目录 GLOB → RIME_EXTRA_MODULES → kDefaultModules → 静态强引用全链路零阻塞，放目录即自动注册，**无需修改 librime 源文件**；官方 release CI 长期以同一机制在 Windows 合并三方插件
- ✅ **编译实测通过（2026-06-10，experiments/003-translator-poc/）**：
  - MochiTranslator 静态合并进 rime.dll（2.8MB），`RIME_REGISTER_MODULE` 自注册生效（`[mochi] module 'mochi' initialized`），MSVC .CRT$XCU 疑虑解除
  - 候选完全接管：schema 只列 mochi_translator，候选 1 = MOCHI_POC，逐候选 preedit 控制生效
  - **每键恰好 1 次 Query**（增量调用 n→ni→nih→niha→nihao），单次 Query 插件自身开销 <1μs
  - 15ms 模拟延迟下：每键稳定 15.5-16.5ms（偶发 30ms 尖刺，commit 后首键），整行连续键流无阻塞无崩溃——Brain IPC 15-20ms 硬超时预算成立
  - 环境坑实录：ps1 需 UTF-8 BOM；VsDevCmd 切换工作目录；开发会话注入 NoDefaultCurrentDirectoryInExePath=1 需局部清除；rime_api_console 的 line_editor 不兼容重定向 stdin（测试用 rime_console.exe）
