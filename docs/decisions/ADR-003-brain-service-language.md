# ADR-003: Brain 服务实现语言

- 状态：Open
- 日期：2026-06-10
- 决策者：待定

## 背景

Brain 服务是常驻本地进程，承载词图解码、记忆库、ONNX 推理、UIA 采集、学习循环。要求：低延迟（热路径 <20ms 份额）、内存可控、长期稳定运行、与 C++ 的 librime 插件 IPC。

## 选项

- **C++**：与 librime/Weasel 生态同构，ONNX Runtime / KenLM 原生集成；内存安全负担重
- **Rust**：内存安全 + 性能，ort / kenlm 绑定可用，windows-rs 对 UIA 支持好；与 C++ 插件需跨语言 IPC（本来就是进程边界，影响小）
- **混合**：热路径核心 C++/Rust，学习循环与教练用 Python（训练生态最好），以独立进程或定时任务形式存在

## 决策

未定。倾向：Brain 用 Rust 或 C++（待团队熟悉度评估），训练/实验脚本一律 Python（experiments/ 已默认）。

## 后果

（定案后补充）
