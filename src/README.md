# src

产品代码目录，M2 启动后按 docs/DESIGN.md 的进程拓扑划分：

```
src/
  ime-plugin/   librime 自定义 translator 插件（C++，极轻，绝不崩溃）
  brain/        Brain 常驻服务（语言见 ADR-003，未定）
  llm-host/     LLM 进程封装与调度
  shared/       IPC 协议定义
```

当前为空——M1（实验 001）通过前不写产品代码。实验原型代码放 experiments/。
