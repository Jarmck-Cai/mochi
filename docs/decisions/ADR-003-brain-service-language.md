# ADR-003: Brain 服务实现语言

- 状态：Accepted（2026-06-10 用户拍板：Rust）
- 日期：2026-06-10
- 决策者：用户 + Claude

## 背景

Brain 服务是常驻本地进程，承载词图解码（热路径 ~5ms 份额）、个人记忆库（即时更新 + 场景分桶）、ONNX 推理、UIA 采集、策略层、学习循环、LLM 调度。要求：低延迟、长期稳定、与 C++ librime 插件经命名管道 IPC（15-20ms 硬超时）。

## 选项与评估

| 维度 | C++ | Rust | C#/.NET |
|---|---|---|---|
| 热路径性能 | ✓ | ✓ | GC 暂停对 15ms 预算构成持续风险 |
| 常驻服务内存安全 | 人工负担重；静默损坏比崩溃更难排查 | 编译期消灭整类问题 | ✓（GC 换来的） |
| Windows 依赖管理 | 高成本（librime 编译五轮调试为证） | cargo 近零成本 | NuGet ✓ |
| ONNX Runtime | 原生 | `ort` crate 成熟 | 一等公民 |
| UIA | COM 手写 | `windows-rs` + `uiautomation` crate | 最舒服 |
| KenLM | 原生 | 无绑定——**但不需要**：个人 LM 要求可变计数+衰减+场景分桶，KenLM 本就只能服务通用 LM；通用 LM 由 Python 离线构建自定义紧凑格式，Rust 只读加载（实验 001 已验证手写 n-gram 足够） |
| AI 实现者适配 | UB 类错误逃逸编译期 | **编译器是 AI 生成代码的第一道审查** | 居中 |

## 决策

**Brain 用 Rust。** 三语分工，进程边界天然隔离：

```
ime-plugin   C++（被迫且极薄）：librime 接口 + IPC 客户端 + 超时降级，接口冻结后基本不动
brain        Rust（核心）：所有智能与状态
experiments/ Python：算法原型、语料处理、离线建模
```

决定性理由：① 常驻服务的内存安全 + AI 生成代码需要严格编译器兜底；② cargo 对比今日亲历的 Windows C++ 依赖成本；③ 生态逐项核对无缺口（KenLM 缺口经分析为伪需求）。

## 后果

- C++/Rust 无共享代码：IPC 协议（docs/specs/ipc-v0.md）是唯一接口，两端各自实现序列化——本来就是进程边界，影响极小
- 需安装 Rust 工具链（rustup + stable-msvc，复用已装的 MSVC 链接器）
- LLM 进程经 HTTP（llama.cpp server）调用，语言无关，不受影响
