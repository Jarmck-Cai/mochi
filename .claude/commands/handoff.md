---
description: session 收工：固化本次进展，为下个 session 留好上下文
---

执行收工流程：

1. 回顾本 session 的全部工作（完成的、进行中的、新决策、新发现的风险）
2. 更新 project/STATUS.md：已完成 / 进行中 / 下一步 / 阻塞项，更新"最后更新"日期
3. 写 project/devlog/YYYY-MM-DD-<主题>.md：本次做了什么、关键讨论结论、为什么这么决定、下个 session 从哪继续（写给一个没有本对话记忆的人）
4. 如有未落盘的重要决策 → 补 ADR；如有未落盘的调研结论 → 补 docs/research/
5. git status 检查未提交变更，向用户确认是否提交（commit message 中文，按逻辑变更分组）

最后向用户确认：STATUS 与 devlog 内容是否准确，有无遗漏。
