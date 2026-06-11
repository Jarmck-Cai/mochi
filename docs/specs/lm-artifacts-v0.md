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

### 5. personal.tsv —— 个人冷启动产物（personal-artifacts-v0，M2-3 新增）

文档导入冷启动的交换格式：`export_personal.py`（jieba 分词 + 按文档时间衰减）→
`user_data/personal.tsv` → brain 启动时载入 PersonalStore。**这是私有用户数据，
user_data/ 永不入 git。**

```
# personal-artifacts-v0
lex	hfo	12.5            ← 个人词库：词<TAB>衰减计数（ASCII 视为英文，否则中文）
case	resnet	ResNet      ← 大小写习惯：小写形<TAB>用户偏好写法
zhword	随机梯度	sui ji ti du  ← 词典外中文词：词<TAB>空格分隔音节（生成个人词图弧）
1	随机	4.0             ← 个人 n-gram，与 ngram.tsv 同构（计数已衰减，载入后不再衰减）
2	<s> 随机	4.0
3	<s> 随机 梯度	2.0
```

## 约束

- 个人记忆的**运行时**状态不走本格式——brain 自有 `user_data/commits.jsonl`
  追加日志（每次上屏一行 JSON 事件），启动时按天衰减回放，用户可直接查看/删行；
  personal.tsv 只承载一次性的文档导入冷启动
- 产物目录 `artifacts/v0/` 不入 git（可由流水线重建）；meta.json 入 git 供溯源
