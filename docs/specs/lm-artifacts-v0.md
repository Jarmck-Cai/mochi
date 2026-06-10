# 离线语言模型产物 v0：Python 流水线 → Rust brain

> M2-2 契约。原则：跨语言交换面用最笨的 TSV 文本（可 diff、可抽查），
> 高效二进制格式是 Rust brain 的私有实现（`brain build-artifacts` 离线编译，运行时 mmap/加载）。

## 流向

```
experiments/001 流水线（Python，已有训练代码）
  └─ export_artifacts.py → artifacts/v0/*.tsv （文本交换格式，本规格）
       └─ brain build-artifacts → brain 私有二进制（格式不在本规格内，随实现演进）
            └─ brain 运行时加载
```

## 文本交换格式（UTF-8，制表符分隔，# 开头为注释行）

### 1. dict.tsv —— 拼音词典（来自 rime 明月词典 + 后续可扩展）

```
# key<TAB>text<TAB>weight
ni hao	你好	38129
yuan	源	1024
```

- key：空格分隔的无调拼音音节序列；text：汉字词；weight：原始频次（解码端自行归一化）

### 2. ngram.tsv —— 通用 trigram（stupid backoff，对应实验 001 的 BackoffTrigramLM）

```
# order<TAB>token1[ token2[ token3]]<TAB>count
1	你好	5821
2	你好 世界	17
3	我 爱 你	42
```

- token 为词（jieba 粒度，与训练一致）；count 为加权计数（浮点允许）
- 文件头注释必须含：`# total_tokens=<N>`、`# backoff=stupid:0.4`（解码端读取超参）

### 3. english.tsv —— 通用英文词表

```
# word<TAB>rank
the	1
```

### 4. meta.json —— 版本与来源清单

```json
{"v": 0, "built_at": "...", "sources": {"dict": "rime pinyin_simp", "corpus": "SIGHAN pku+msr"}, "counts": {"dict": 65121, "ngram1": 90392, "ngram2": 954793, "ngram3": 235545}}
```

## 约束

- 个人记忆（个人 LM/词库/场景分桶）**不走本格式**——那是 brain 运行时自有状态（rusqlite），由 commit 流即时更新；本格式只承载只读的通用层
- 产物目录 `artifacts/v0/` 不入 git（可由流水线重建）；meta.json 入 git 供溯源
