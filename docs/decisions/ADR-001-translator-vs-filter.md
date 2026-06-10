# ADR-001: RIME 接入深度——filter 还是 translator

- 状态：Proposed（2026-06-10 源码调研已支持方案 B；待最小 translator 插件编译实测后定案）
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
- ⏳ 定案前剩余实证：最小 translator 插件的编译、注册与延迟手感实测
