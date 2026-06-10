# mochi-brain（Brain 服务）

Mochi 的常驻 Brain 服务（Rust，ADR-003）。实现
[ipc-v0 协议](../../docs/specs/ipc-v0.md) 的命名管道服务端；`query` 走真实
拼音词图解码（λ₁ 通用层：词典词图 + trigram stupid backoff + beam search，
算法对齐实验 001 的 Python 流水线），数据来自
[lm-artifacts-v0](../../docs/specs/lm-artifacts-v0.md) 的 `artifacts/v0/*.tsv`。

## 行为

- 监听 `\\.\pipe\mochi-brain-v0`，消息模式（`PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE`），单条消息上限 64KB
- 循环 accept：每个客户端一个服务线程（解码引擎只读共享），客户端断开后继续监听
- `query` → 词图解码 top-N 候选（`text`/`comment`/`preedit`/`quality`），
  preedit 为音节切分（如 `ni hao`），quality 为模型 log10 总分，`elapsed_us` 真实计时
- `commit` → 记日志，回 `{"v":1,"ok":true}`（M2-3 即时学习挂在这里）
- 未知 `v`/`method`/解析失败 → `{"v":1,"error":"unsupported"}`，不崩溃
- 启动时加载 artifacts（实测 ~0.5s / 估算 ~41MB，远低于 3s/500MB 阈值，
  故无二进制缓存层）；加载耗时与内存估算打到 stderr

## 一键命令

```powershell
# 构建（cargo 在 %USERPROFILE%\.cargo\bin，PATH 没有时用全路径）
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release --manifest-path E:\Projects\P029_ai-ime\src\brain\Cargo.toml

# 单测（协议 + 音节切分 + 词图 + backoff 打分 + 解码，26 个用例，内嵌小词典不依赖 artifacts）
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --release --manifest-path E:\Projects\P029_ai-ime\src\brain\Cargo.toml

# 运行（前台，stderr 即日志；Ctrl+C 退出）
# --artifacts 缺省时自动找仓库根的 artifacts\v0（先 cwd，再从 exe 路径向上找）
E:\Projects\P029_ai-ime\src\brain\target\release\mochi-brain.exe [--artifacts <dir>] [--beam 12] [--topn 5]

# bench：长度 1-20 键流各 200 次解码，输出中位/最大耗时（µs）与 top-1
E:\Projects\P029_ai-ime\src\brain\target\release\mochi-brain.exe --bench

# 一次性解码（调试/与 Python 流水线对照；keys<TAB>#序<TAB>text<TAB>preedit<TAB>score<TAB>耗时）
E:\Projects\P029_ai-ime\src\brain\target\release\mochi-brain.exe --decode nihao woyaoceshi
```

Python 侧对照：`experiments/001-personalization-gain/decode_from_artifacts.py`
（同一份 artifacts + 同一算法的 Python 实现，top-1 应一致）。

## 代码结构

```
src/main.rs       管道服务端（accept + 每客户端一线程）+ CLI + bench
src/protocol.rs   ipc-v0 消息类型（serde）+ dispatch
src/engine.rs     查询引擎：artifacts + scorer + decoder 拼装
src/artifacts.rs  lm-artifacts-v0 TSV 加载（dict/ngram/english）
src/interner.rs   token 驻留（字符串 → u32，n-gram 表全部用整数键）
src/lm.rs         trigram stupid backoff（λ₁）+ PersonalLayer trait（λ₂ 接口，M2-3 实现）
src/lattice.rs    词图：拼音词/字弧 + 英文词弧 + 字母兜底弧；音节切分
src/decoder.rs    beam search（宽度默认 12）+ 弧类型先验 + n-best 回溯
```

## 打分公式（对齐 DESIGN.md §3 与实验 001 decoder.py）

```
P(w|h) = μ_g·P_general(w|h) + μ_p·P_personal(w|h)      ← μ_p=0（M2-3 接入）
score += log10 P(w|h) + prior[弧类型] + λ_lex·lexicon_bonus(w)
```

弧类型先验（log10）：词 0 / 单字 -0.6 / 英文 -3.0 / 兜底字母 -12.0。
backoff 因子 α 与 total_tokens 从 ngram.tsv 头注释读取。

## 后续（M2-3）

`PersonalLayer` trait（lm.rs）即 λ₂ 插槽：个人 n-gram（时间衰减、场景分桶）
与个人词库 bonus 在 commit 流上即时更新；解码端已按线性插值形式接好。
