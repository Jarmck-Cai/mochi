# mochi-brain（Brain 服务 v0）

Mochi 的常驻 Brain 服务（Rust，ADR-003）。v0 只做一件事：实现
[ipc-v0 协议](../../docs/specs/ipc-v0.md) 的命名管道服务端，对 `query`
返回 echo 候选 `MOCHI_BRAIN:<input>` 证明 ime-plugin ↔ brain 链路。

## 行为

- 监听 `\\.\pipe\mochi-brain-v0`，消息模式（`PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE`），单条消息上限 64KB
- 循环 accept：每个客户端一个服务线程，客户端断开后服务端继续监听（插件 kill/重连随意）
- `query` → `[{text: "MOCHI_BRAIN:<input>", comment: "echo", preedit: "«input»", quality: 1.0}]`，`elapsed_us` 真实计时
- `commit` → 记日志，回 `{"v":1,"ok":true}`
- 未知 `v`/`method`/解析失败 → `{"v":1,"error":"unsupported"}`，不崩溃
- 日志：每条请求一行到 stderr（method、input、处理耗时 µs）

依赖选型：`serde`/`serde_json` + `windows-sys`（而非 `interprocess`：
消息模式成帧是 ipc-v0 契约的一部分，需要直接控制管道标志；windows-sys
是纯声明薄层，依赖面最小）。理由详见 `Cargo.toml` 注释。

## 一键命令

```powershell
# 构建（cargo 在 %USERPROFILE%\.cargo\bin，PATH 没有时用全路径）
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release --manifest-path E:\Projects\P029_ai-ime\src\brain\Cargo.toml

# 单测（协议序列化/反序列化，9 个用例）
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --release --manifest-path E:\Projects\P029_ai-ime\src\brain\Cargo.toml

# 运行（前台，stderr 即日志；Ctrl+C 退出）
E:\Projects\P029_ai-ime\src\brain\target\release\mochi-brain.exe
```

## 代码结构

```
src/main.rs      管道服务端：循环 accept + 每客户端一线程
src/protocol.rs  ipc-v0 消息类型（serde）+ dispatch + 单测
```

## 后续（M2 余下步骤，见 src/README.md）

词图解码、个人记忆库、commit 即时学习都加在 `protocol::handle_message`
之后的分发层；协议带版本号，需要二进制化时再升 v。
