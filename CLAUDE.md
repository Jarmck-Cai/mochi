# P029 — AI 输入法（个人记忆输入法）

中英混打、懂用户的 Windows 输入法。产品愿景见 [README.md](README.md)，技术方案见 [docs/DESIGN.md](docs/DESIGN.md)。

## 当前阶段

设计阶段（尚未写代码）。**最新进展永远以 [project/STATUS.md](project/STATUS.md) 为准——每个 session 开始先读它。**

## Session 协议（跨 session 连续开发的关键）

- **开工**：运行 `/catchup`（读 STATUS.md + 最新 devlog，恢复上下文）
- **收工**：运行 `/handoff`（更新 STATUS.md，写当日 devlog）
- **做了重要技术决策** → 在 `docs/decisions/` 写 ADR（模板：ADR-000）。推翻旧决策时新建 ADR 并标记旧的为 Superseded，不要修改旧 ADR 正文
- **实验/调研结论** → 落到 `experiments/<编号>/` 或 `docs/research/`，不要只留在对话里
- 提交粒度：一个逻辑变更一个 commit，commit message 用中文

## 目录地图

```
README.md            产品愿景与方案（v2）
CLAUDE.md            本文件，session 入口
docs/
  DESIGN.md          技术方案总纲（架构、热路径、异步路径、风险）
  decisions/         ADR 决策记录（含未定案的 Open 决策）
  research/          调研报告
project/
  STATUS.md          ★ 项目当前状态（done / doing / next / blocked）
  ROADMAP.md         MVP 里程碑（按验证顺序）
  devlog/            每个 session 一篇开发日志 YYYY-MM-DD-主题.md
experiments/         离线实验（每个实验独立目录，含 README + 结论）
src/                 产品代码（未开始）
.claude/
  agents/            自定义 subagent（researcher / implementer / reviewer）
  commands/          斜杠命令（/catchup /handoff）
```

## 多 agent 协作约定

- 并行的探索/调研任务用 **researcher** agent（只读），并行实现任务用 **implementer** + worktree 隔离，合入前用 **reviewer** 审一遍
- 每个 agent 的任务描述必须自包含（指明读哪些文档、产出写到哪个文件）——agent 没有本对话的记忆
- agent 产出一律落盘（docs/research/ 或 experiments/），由主会话汇总进 STATUS.md

## 技术约定

- 文档用中文；代码、标识符、代码注释用英文
- 平台：Windows 优先；PowerShell 环境
- 核心架构决策状态见 `docs/decisions/`（ADR-001 translator 路线已建议待定案，ADR-003 Brain 服务语言未定）
- 硬约束：按键热路径 <50ms（预算 ~25ms）；学习必须即时生效
