# IPC 协议 v0：ime-plugin ↔ brain

> M2 端到端打通用的最小协议。原则：先 JSON 跑通（消息小、预算内），需要时再换二进制——协议带版本号，演进安全。

## 传输层

- Windows 命名管道：`\\.\pipe\mochi-brain-v0`
- 消息模式：`PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE`（消息天然成帧，无需长度前缀）
- 单条消息上限 64KB
- brain 为服务端（多实例监听，支持并发客户端）；插件为客户端，连接建立后复用（避免每键 connect）

## 时序约束（硬性）

- 插件侧总预算 **15ms**（写请求 + 读响应，OVERLAPPED + WaitForSingleObject 实现超时）
- 超时或管道错误 → 本次返回空候选（librime 走 schema 内其他 translator 或无候选），标记 brain 不可用，**2s 退避**后重试连接——绝不阻塞按键
- brain 不可用期间插件不得累积请求（每键即时放弃）

## 消息格式（UTF-8 JSON）

### query（每键调用）

请求：
```json
{
  "v": 1,
  "method": "query",
  "input": "nihao",
  "seg": [0, 5],
  "scene": {"app": "weixin.exe", "title": "..."},
  "session": "weasel-session-id"
}
```

响应：
```json
{
  "v": 1,
  "candidates": [
    {"text": "你好", "comment": "", "preedit": "ni hao", "quality": 1.0}
  ],
  "elapsed_us": 1234
}
```

- `candidates` 可为空数组（brain 无建议）
- v0 阶段 brain 返回 echo 候选 `MOCHI_BRAIN:<input>` 证明链路

### commit（上屏通知，fire-and-forget，为即时学习预留）

```json
{"v": 1, "method": "commit", "text": "你好", "input": "nihao", "scene": {"app": "..."}}
```

响应：`{"v": 1, "ok": true}`（插件不等待也不重试）

## 错误与版本

- brain 收到不认识的 `v`/`method` → `{"v":1,"error":"unsupported"}`
- 任何解析失败按超时处理（降级），不崩溃、不重试风暴

## 双侧日志约定

两端都记录 per-query 延迟（插件记端到端，brain 记处理耗时），用于验证 15ms 预算的真实分布。
