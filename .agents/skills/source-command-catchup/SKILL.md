---
name: "source-command-catchup"
description: "session 开工：恢复项目上下文"
---

# source-command-catchup

Use this skill when the user asks to run the migrated source command `catchup`.

## Command Template

执行开工流程，恢复跨 session 上下文：

1. 读 project/STATUS.md（当前状态、下一步、阻塞项）
2. 读 project/devlog/ 中最新的 1-2 篇日志
3. 如 STATUS 提到未定案的 ADR，读对应 docs/decisions/ 文件
4. 用 git log --oneline -10 查看最近提交

然后向用户汇报：≤5 句话总结"上次做到哪、这次建议做什么"，列出待用户决策的事项（如有），等用户确认方向后开工。
