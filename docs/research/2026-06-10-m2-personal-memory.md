# M2-3 实测：个人记忆库（即时学习闭环）

- 日期：2026-06-10（晚，第三个 session）
- 结论：**"纠正一次，立刻学会"第一次真实发生**。rime_console 里选一次
  "随机梯度"，下一次裸打 `suijitidu` 首选即出"随机梯度"；重启 brain 记忆仍在。
  解码延迟无回归（个人层加载后中位 ≤226µs，5ms 份额的 ~4.5%），与 Python
  参考实现 top-1 一致 12/12。

## 闭环链路

```
用户上屏（选字/回车）
  → librime Context::commit_notifier（Clear 之前，text/input 都还在）
      → MochiTranslator::OnCommit                    [src/ime-plugin]
          → ipc-v0 commit {text, input, scene}        同管道，响应须读走保持同步
              → PersonalStore::commit                 [src/brain/src/personal.rs]
                  ① 贪心分词 + 拼音对齐 → 个人 n-gram（全局 + 场景分桶）
                  ② 个人词库计数（λ_lex bonus）+ 英文大小写习惯
                  ③ 词典外短语促升 → 个人词图弧（PyPersonal）
                  ④ 追加 user_data/commits.jsonl（重启回放，按天衰减 0.98）
  → 下一个按键的 query 即见更新（RwLock 写后读）
```

打分按实验 001 获胜配置固定：μ_g=0.65、μ_p=0.35、λ_lex=0.8；场景阶梯
（ADR-004）= 场景桶认识该词用场景桶，否则回退全局个人层，再由 μ_g 项兜底通用层。

## 实测

### 锚点用例（真实产物 96MB，全新空记忆）

| 步骤 | top1 | quality |
|---|---|---|
| query `suijitidu`（纠正前） | 随即梯度（"随机梯度"排第 3） | -11.6 |
| commit 随机梯度 ×1 | — | brain 侧学习耗时 437µs |
| query `suijitidu`（纠正后） | **随机梯度**（次选"随即梯度"-6.1，拉开 5 个量级） | -0.91 |
| 重启 brain 后再查 | 随机梯度，分数逐位一致（jsonl 回放） | -0.91 |

### 插件 e2e（rime_console，全新记忆）

输入两行：`suijitidu3`（数字选第 3 候选=随机梯度）、`suijitidu`（裸打）。
stdout 两行都是"随机梯度"——第二行就是"它马上学会了"。第二次 commit 还把
4 字短语"随机梯度"促升成个人词条（对齐出 `sui ji ti du`，单弧入图）。

### 延迟（--bench，200 次/长度，release）

| 配置 | len10 中位 | len20 中位 | 最大 |
|---|---|---|---|
| 空个人层 | 113µs | 172µs | 875µs |
| 加载 personal.tsv（6361 行） | 146µs | 199µs | 829µs |

插值改为 powf 路径 + 个人层查询 + RwLock 读，合计 ~15-30% 开销，绝对值仍
只占 5ms 解码份额的 ~4.5%，热路径无忧。commit 侧学习 ~400µs（写锁内）。

### Python 一致性（parity_personal.py）

两侧同一通用产物 + 同一个人层（Python 全语料 build_personal_layer ↔ Rust 载入
export_personal.py 导出的 personal.tsv），full 配置解码 12 条键流（4 通用 +
8 条个人语料抽样），**top-1 一致 12/12**。

### 冷启动（文档导入）

`export_personal.py`：6 篇个人文档 → 559 子句 → personal.tsv 6361 行
（词库 1162、中文词典外词 111、个人 LM 861/1720/1854）。brain 载入后
en_arcs=195（带大小写习惯）、zh_arcs=7——111 个"OOV"里只有 7 个真不在
rime 词典（Python 侧 zh_oov_words 没查词典，Rust 端用 dict_tokens 过滤掉
已有弧的词是对的，避免重复弧）。

## 设计取舍（与 STATUS 旧计划的差异）

1. **持久化用追加式 commits.jsonl，不用 rusqlite**：个人 IME 规模（万级
   commit）回放只要毫秒级；纯文本日志用户可直接查看/删行（"记忆可编辑"
   原则的最低成本实现）；少一个 C 依赖。等规模或查询需求超出（M5 检索）再
   引入 DB，事件日志本身就是迁移源。
2. **commit 分词用贪心最长匹配（词典+已学词），不是 jieba**：Rust 侧无 jieba；
   单条 commit 文本短，贪心误差可接受。系统性偏差由两个机制兜底：冷启动走
   Python jieba 导出；整短语促升机制专门捕捉贪心永远切不出的新词（jieba 能
   发现新词，贪心不能——所以 2-6 字全中文 commit 出现 ≥2 次且词典无此词时，
   整体促升为个人词 + 回填累计计数）。
3. **拼音对齐代替 pypinyin**：commit 自带用户实际敲的键流，对齐
   （汉字按字读音表回溯匹配）比查注音表更准——多音字直接由用户输入消歧
   （"行" háng/xíng 不会错）。对齐失败（编辑过/部分上屏）只跳过建弧，
   n-gram 学习照常。
4. **时间衰减按天 0.98（半衰期 ~34 天）**：启动时相对"今天"一次性折算，
   运行期内当天权重 1.0；personal.tsv 计数为导出时已衰减值，载入后不再衰减。

## 交付物

- `src/brain/src/personal.rs`（新，含 6 单测）；lm/lattice/decoder/engine/
  protocol/main 接入；合计 37 单测全过，release 零警告
- `src/ime-plugin/`：commit_notifier 挂接（e2e 报告遗留 3 清掉）
- `experiments/001/export_personal.py` + `parity_personal.py`
- 规格：lm-artifacts-v0.md 新增 §5 personal-artifacts-v0；约束节改为
  commits.jsonl 运行时状态
- CLI：`mochi-brain --user-data <dir> | --no-user-data`（默认 `<repo>/user_data`）

## 遗留

1. **commit 学习不区分"选字纠正"与"默认上屏"**：两者都学（用户接受即信号），
   但纠正应该权重更高/默认上屏可能强化错误首选——需要插件侧把
   GetSelectedCandidate 的序号带进 commit（协议加可选字段），M2-4 观察真实
   打字后定。
2. **scene 仍是占位 `{}`**：插件在 rime_console 里没有真实 app 身份，场景分桶
   逻辑已就绪（pipe 实测过），等 M2-4 进 Weasel 接 Tier 0 信号。
3. 多次 commit 同一短语会让 jsonl 无限增长——需要定期 compact（把旧事件折算
   进快照），规模到了再做。
4. 管道抢注防护（FIRST_PIPE_INSTANCE + ACL）仍是产品化前置项（继承自 e2e 报告）。
