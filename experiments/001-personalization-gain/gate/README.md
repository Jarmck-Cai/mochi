# 通用层回归门禁（gate）

相位 2（通用层语料现代化）的安全锁。重训 artifacts 后，**全自动**裁决新版能否换上：
对照集不退 + 目标集提升，才放行。无人工复查（这是"教练离线自动更新快模块"的前提）。

设计与背景：`docs/research/2026-06-12-离线教练分层与快慢协同.md`。

## 用法

```powershell
# 跑门禁（日常）：只需 mochi-brain.exe + 本目录两个 tsv
.\.venv\Scripts\python.exe gate.py <old_artifacts_dir> [new_artifacts_dir]
#   new 省略时默认 ..\..\artifacts\v0
#   退出码 0=PASS / 1=FAIL；摘要打印到 stdout，逐条翻转写 gate/last-report.md(.json)

# 重新冻结对照集（偶尔，需 data/general/pku_training.utf8 在场）
.\.venv\Scripts\python.exe gate.py --rebuild-control [--holdout 400] [--max 200] [--seed 29]
```

## 裁决规则（退出码即结论）

| 条件 | 含义 |
|---|---|
| A: `control_Δ ≥ 0` | 公共对照集 top-1 净值不下降（不许把通用输入弄差） |
| B: `target_Δ > 0`  | 目标集 top-1 净值提升（这次改动确实修到了想修的过时词） |

`exit 0` 当且仅当 A 且 B，否则 `exit 1`。

> ⚠️ 净值规则会**掩盖个别回退**：净 +1 可能是 5 赢 4 输。这是"全自动无人工"的取舍。
> 逐条 lost 全在 `last-report.md` 里供失败排查，但**不是审批步骤**。建门禁时实测见过此情形
> （essay λ=1 vs λ=0：净 +1 但 4 条 lost）。若日后要更严，可加"无新增 lost"的可选硬条件。

## 两个测试集

- **control.tsv**（公共，入 git，勿手改）：PKU held-out 末 400 句 → clause 切分 → 拼音键流，
  seed=29 取 200 条。与 M1"通用对照"同方法论（run_experiment.py）。冻结成文件 = 可复现、
  与 gitignore 的原始语料解耦。重建用 `--rebuild-control`。
- **target.tsv**（手维，入 git）：当前通用层因 SIGHAN(2005) 过时而首选错的 case，
  `keys<TAB>gold`。**只放纯语料过时 case**（补全/分词/模糊音噪声归 misfire-log，别混进来）。
  相位 1 攒够后再做"从 project/misfire-log.md 自动抽"（本期手维）。

## 相位 2 闭环：重训 → 门禁 → 放行

机制已打通并端到端验证（2026-06-13）。给 `export_artifacts.py` 注入现代语料重训，
再过门禁裁决：

```powershell
# 1) 重训：把现代语料(同 SIGHAN 分词格式，空格分词)注入 trigram
.\.venv\Scripts\python.exe export_artifacts.py --out artifacts-cand `
    --extra-corpus gate\demo-corpus.txt --extra-weight 50
#   SIGHAN 体量大，少量现代语料需 weight>1 才压得动；meta.json 记 extra_corpus/weight

# 2) 门禁裁决候选能否换上
.\.venv\Scripts\python.exe gate.py ..\..\artifacts\v0 artifacts-cand
#   PASS(exit 0) 才把 artifacts-cand 内容覆盖 artifacts/v0
```

`gate/demo-corpus.txt` 是格式样例 + 闭环自测：8 句含「打字」的现代语料，weight=50
即把 `dazi` 首选从「大字」翻成「打字」（候选本就存在，排第二），门禁 control 不退、
target 0/1→1/1 → PASS。**云 LLM 的角色仅是产出更大更好的 `--extra-corpus` 文件**，
下游全机械化。注：LLM 生成的是原始句，需先分词成空格格式（分词器选型属后续内容问题）。

## 注意

- 门禁只测**通用层**（始终 `--no-user-data`），个人层泛化另测。
- top-1 命中 = 首选文本与 gold **精确整句相等**。注意补全候选会让首选带尾巴
  （如 suijitidu→"随机梯度下降"），这类应进 misfire-log 的 [补全] 而非此目标集。
- `last-report.*` 是每次运行的产物，已 gitignore。
