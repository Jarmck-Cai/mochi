# M2 解码器移植实测：brain query 从 echo 升级为真实拼音解码

- 日期：2026-06-10
- 结论：**解码正确、性能达标、与 Python 实现 top-1 完全一致（10/10）**。
  `nihao`→你好、`zhongwenshuru`→中文输入、`woyaoceshi`→我要测试、
  混打 `zheshiyigetest`→这是一个test 均经 rime_console 全链路上屏验证。
  单次解码（词图+beam search）中位 ≤200µs、最大 <1ms，远低于 5ms 预算。
  过程中定位并修复了插件侧一个真 bug（WaitForSingleObject 超时按系统
  时钟节拍量化提前到期，~4% 按键被误判超时降级）。

## 一、实现概要（src/brain，λ₁ 通用层）

```
src/engine.rs     查询引擎：artifacts + scorer + decoder
src/artifacts.rs  lm-artifacts-v0 TSV 加载（dict/ngram/english）
src/interner.rs   token 驻留：字符串 → u32，n-gram 键打包为 u64/u128 整数
src/lm.rs         trigram stupid backoff（移植 lm.py）+ PersonalLayer trait
src/lattice.rs    词图（移植 lattice.py）：拼音词/字弧 + 英文词弧 + 兜底字母弧
src/decoder.rs    beam search（移植 decoder.py）+ 弧类型先验 + n-best 回溯
```

- 打分 = `log10(μ_g·P_general + μ_p·P_personal) + 弧先验 + λ_lex·词库bonus`，
  与 decoder.py 完全同形；本次 μ_p=0、λ_lex=0，**`PersonalLayer` trait（λ₂）
  已预留**（`p(w,h2,h1)` 线性概率 + `lexicon_bonus(w)`，空实现 `NoPersonalLayer`），
  M2-3 在 commit 流上实现真实个人层即可插入，解码端无需再动
- 弧先验沿用实验 001：词 0 / 单字 -0.6 / 英文 -3.0 / 兜底 -12；
  backoff α=0.4 与 total_tokens 从 ngram.tsv 头注释读取
- 候选 top-N（默认 5，`--topn`）按终局 beam 的 (h2,h1) 态去重回溯；
  preedit 来自路径上各弧自带的音节切分（如 `ni hao`）；quality 填模型 log10 总分
- 协议不变（ipc-v0）；每连接一线程共享只读引擎
- 单测 26 个全过（音节切分、词图构弧、backoff 三级回退、解码 top-1、
  混打竞争、协议收发；用内嵌 23 词条小词典，不依赖全量 artifacts）

## 二、artifacts 加载（全量 v0）

| 项 | 值 |
|---|---|
| 数据量 | dict 65,121 / ngram 90,476+955,754+235,688 / english 9,974（26 条单字母等被过滤） |
| 加载耗时 | **425-789ms**（冷热文件缓存差异；阈值 3s，**未做二进制缓存**） |
| 内存估算（表容量法） | ~41MB |
| 进程工作集实测 | ~96MB（含分配器/运行时开销，目标 <500MB 余量充足） |

token 全部 intern 成 u32；trigram 表键为 u128 整数（FxHash），是加载快、
内存小的主因。Python 同口径加载需 2.0s。

## 三、bench（`mochi-brain --bench`，长度 1-20 各 200 次）

机器当时处于节能电源状态（绝对值偏保守）。耗时为完整 query 路径
（词图构建 + beam search + 候选物化），beam=12，top-5：

| len | 键流 | 中位 µs | 最大 µs | top-1 |
|---|---|---|---|---|
| 1 | w | 3 | 12 | w |
| 2 | wo | 9 | 154 | 我 |
| 5 | woyao | 83 | 767 | 我要 |
| 10 | woyaoceshi | 166 | 633 | 我要测试 |
| 15 | woyaoceshizhong | 200 | 763 | 我要测试中 |
| 18 | woyaoceshizhongwen | 182 | 462 | 我要测试中文 |
| 20 | woyaoceshizhongwensh | 122 | 294 | 我要测试中文sh |

全部长度中位 ≤200µs、最大 ≤847µs，**<5ms 预算余量 >5 倍**（预算的 ~4%）。
不完整音节尾部（len 6/8/9 等）top-1 含原始字母属预期（见遗留问题 1）。

## 四、与实验 001 Python 实现的一致性抽查

对照器：`experiments/001-personalization-gain/decode_from_artifacts.py`
（同一份 artifacts TSV + pipeline 原始类，通用层同配置）。**10/10 top-1 一致**：

| 键流 | Python top-1 | Rust top-1 | 一致 |
|---|---|---|---|
| nihao | 你好 | 你好 | ✓ |
| zhongwenshuru | 中文输入 | 中文输入 | ✓ |
| woyaoceshi | 我要测试 | 我要测试 | ✓ |
| jintiantianqihenhao | 今天天气很好 | 今天天气很好 | ✓ |
| woaibeijingtiananmen | 我爱北京天安门 | 我爱北京天安门 | ✓ |
| zhongguorenminyinhang | 中国人民银行 | 中国人民银行 | ✓ |
| xianzaikaishi | 现在开始 | 现在开始 | ✓ |
| zheshiyigetest（混打） | 这是一个test | 这是一个test | ✓ |
| womenyongwindows（混打） | 我们用windows | 我们用windows | ✓ |
| woyaoyonggan（gan 歧义） | 我要勇敢 | 我要勇敢 | ✓ |

Rust 单条解码 188-384µs；Python 同条 0-16ms。

## 五、rime_console 全链路实测

流程（每次 librime 重建后必做，后记坑 2）：拷
`experiments/003-translator-poc/rime-data/*.yaml` → `librime/build/bin`、
删 `bin\build` 缓存；测前 `taskkill mochi-brain`（遗留问题：旧实例抢管道）。

两轮输入共 8 行键流、110 个按键，**0 掉线**，上屏全部正确：

```
你好 / 中文输入 / 我要测试 / 我爱北京天安门 / 现在开始
这是一个test / 我们用windows / 今天天气很好
```

每键端到端延迟（插件侧计时，含 IPC 往返 + 解码 + JSON）：

| 轮 | 键数 | down | 中位 | p90 | 最大 |
|---|---|---|---|---|---|
| run1（5 行） | 61 | 0 | 925µs | 1420µs | 3564µs（首键含建连） |
| run2（3 行混打） | 49 | 0 | 936µs | — | 3873µs |

15ms 预算余量 ~4 倍（节能电源状态下）；brain 侧自报解码 6-220µs。

## 六、过程中发现并修复的插件 bug：等待超时被时钟节拍量化

**症状**：初版实测随机掉线——console 某键 `brain=down`（一次在 420µs 即失败，
远小于 15ms 预算），其后整行降级为原始字母；brain 日志却显示已正常解码并在
写响应时报 232（客户端已关管道）。

**排查链**：
1. PowerShell 管道客户端连发 2,020 次查询 → 0 失败，**排除 brain/服务端**；
2. 把 `brain_client.cc` 原样编进独立压测器（加 GetLastError 打点）重放打字
   模式 → **3,030 次失败 133 次（4.4%）**，全部 READ-WAIT err=995（被取消）；
3. 打点显示 `wait_ms=15` 的 `WaitForSingleObject` 实际只等了 ~0.5ms 就返回
   WAIT_TIMEOUT；把预算放宽到 200ms 复测 3,030 次 → 0 失败，真实往返
   p50=864µs / 最大 6.8ms——**等待本身提前到期，不是响应慢**。

**根因**：`WaitForSingleObject` 的毫秒超时按系统时钟节拍（10-16ms）量化，
恰在节拍边界前发起的 15ms 等待可提前近一整拍到期。e2e 报告后记的 QPC 修复
只治了 deadline 的**计算**（GetTickCount64 粒度），没治等待的**执行**；
当时复测样本小（9 键）未暴露。解码器响应（~1ms）比 echo（~0.1ms）慢，
撞节拍窗口概率放大到 4%，每次误判还附带 2s 退避，被本次 110 键实测放大出来。

**修复**（`src/ime-plugin/src/brain_client.cc::WaitOp`，协议不变，
这是任务中"如必须改插件需说明"的那一处）：截止时刻只信 QPC——
WAIT_TIMEOUT 返回后若 QPC 未过 deadline 则按剩余时间继续等，而非直接判死。
修复后：压测器 15ms 预算 3,030 次 **0 失败**；console 110 键 0 掉线。

## 七、遗留问题

1. **不完整音节尾部的候选形态**：`woyaoc` → top-1 "我压oc"（尾部字母走
   兜底弧拼进 text）。正确性无碍（完整音节时收敛），但产品形态应是
   "已转换部分 + 原始尾巴"的 composition 展示，属 M2-4 schema/UI 议题；
   也可给"以合法音节前缀结尾"的路径加先验。
2. **quality 字段语义**（协议/格式问题，按要求记报告不改 spec）：
   现填模型 log10 总分（负数），ipc-v0 示例是 1.0，spec 未定义语义。
   插件 FifoTranslation 按序展示不受影响；建议 spec v1 明确为
   "排序用相对分，无绝对语义"。
3. **管道名可被抢注 / 旧实例抢连接**（沿袭 e2e 报告遗留 1）：本次实测
   再次撞上——后台残留的旧 echo brain 接走了 console 连接。测前必须
   `taskkill /f /im mochi-brain.exe`；产品化前加
   `FILE_FLAG_FIRST_PIPE_INSTANCE` + SECURITY_ATTRIBUTES。
4. **解码无键间增量**：每键对全前缀重解码（rime 每键全量 Query），20 键
   ~200µs 尚无压力；键流更长或换低端机时可加前缀缓存/增量 beam（结构上
   beams 数组已按位置组织，改造路径清晰）。
5. **english.tsv 的 rank 未参与打分**：英文弧吃统一 -3.0 先验 + LM OOV 地板，
   词频排名信息浪费；M2-3 可把 rank 折算成英文词 unigram 先验。
6. **meta.json 未做加载校验**：english 计数 9,974 vs meta 10,000（26 条
   单字母/含连字符词被规则过滤，符合 lattice.py 语义）；建议加载时与
   meta.counts 对账并打告警。
7. 压测器（instrumented brain_client + 打字模式重放）在 `%TEMP%\mochi_pipe_harness`，
   未入库；若 WaitOp 类问题复发可按本报告第六节方法重建。

## 交付物

- `src/brain/`：解码引擎 + 26 单测 + `--bench`/`--decode` 子命令；README 已更新
- `src/ime-plugin/src/brain_client.cc`：WaitOp 节拍量化修复（唯一插件改动）
- `experiments/001-personalization-gain/decode_from_artifacts.py`：Python 对照器
- 本报告
