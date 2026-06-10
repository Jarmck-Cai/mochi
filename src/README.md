# src

产品代码目录（M2 起），按 docs/DESIGN.md 进程拓扑划分：

```
src/
  ime-plugin/   librime merged-plugin（C++，极薄）：MochiTranslator + IPC 客户端
                + 15ms 硬超时 + 降级。从 experiments/003-translator-poc/mochi 演进而来
  brain/        Brain 常驻服务（Rust，ADR-003）：IPC 服务端、词图解码、个人记忆库、
                策略层；后续接 ONNX(ort)、UIA、学习循环
  shared/       无共享代码（ADR-003 三语分工）；接口契约见 docs/specs/ipc-v0.md
```

## M2 实施顺序

1. **E2E 链路**（当前）：brain echo 候选经命名管道进 rime_console 显示，双侧延迟日志，超时降级实测
2. 词图 + 解码移植：experiments/001 的 Python 解码器 → brain（Rust），词典/通用 LM 离线构建（Python）
3. 个人记忆库：场景分桶 + 时间衰减计数 + commit 即时更新（rusqlite + 内存缓存）
4. 部署进 Weasel 真实打字，对比微软拼音（M2 验收标准见 project/ROADMAP.md）
