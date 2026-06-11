# 项目状态

> 每个 session 收工时更新本文件。历史细节见 devlog/。
> 最后更新：2026-06-11（第七次更新：Weasel 部署成功 + 真实使用反馈驱动 essay 底座上线）

## 当前阶段

**M2-4 部署完成，真实使用中**（剩：对比微软拼音的正式验收 + brain 自启）

## 已完成

- [x] 产品愿景与方案 v2（README.md），项目定名 **Mochi**
- [x] 技术方案总纲（docs/DESIGN.md），已按调研结果修正进程拓扑与风险清单
- [x] 开发目录脚手架（session 协议、ADR、devlog、agent、git）
- [x] **librime translator 可行性调研**（docs/research/2026-06-10-librime-translator-feasibility.md）：方案 B 成立；Windows 需静态合并进 rime.dll；WeaselServer 全局锁 → Brain IPC 须 15-20ms 硬超时
- [x] **实验 001 流水线**（experiments/001-personalization-gain/）：端到端跑通，替身语料 +16.7pp 整句命中、+17.6pp 术语命中、通用对照回退 <1pp
- [x] **UIA 实测**（experiments/002-uia-probe/ + docs/research/2026-06-10-uia-context-readability.md）：微信 4.x 完全可读（对话历史+输入框），最大风险初步解除；caret rect 可行
- [x] **GitHub**：https://github.com/Jarmck-Cai/mochi （main 分支已推送）
- [x] **实验 001 真实语料初跑**：用户语料为英文论文（HFO/癫痫领域）→ 流水线增强为全文英文术语提取（文档导入冷启动路径）。提取 854 词个人词库（HFOs/pHFOs/EEG/SOZ，含大小写习惯），混打演示：baseline "监测俄eg信号" → +personal "监测EEG信号" 全部正确。**M1 门禁指标（中文个性化排序）需要中文个人语料，待用户提供**
- [x] **ADR-004（Accepted）**：上下文三级阶梯 + 三点补充——冷启动素材完全可选；场景与语料绑定（个人记忆按场景分桶，层级回退）；Tier 2 读屏为备选/补强，专注 M2-M4。场景分桶已写入 DESIGN 打分公式

- [x] **M1 门禁通过（2026-06-10）**：用户中文语料（微信沟通+电话纪要）实测，整句首选命中率 41.6%→55.1%（**+13.5pp ≥ 10pp**），字级 +4.9pp，41 赢/4 输，通用对照无回退。仅 285 条训练子句即达标——记忆假设强验证。正式报告：experiments/001-personalization-gain/report.md
- [x] **ADR-001 实测定案（条件解除）**：librime+MochiTranslator 编译成功（VS Build Tools 2022），候选接管 ✓、每键恰 1 次 Query ✓、插件开销 <1μs ✓、15ms 模拟延迟下每键 15.5-16.5ms 无阻塞 ✓。实测数据见 ADR-001 与 experiments/003-translator-poc/README.md

- [x] **ADR-003 定案（Accepted）**：Brain 用 **Rust**（内存安全 + cargo + 编译器为 AI 代码兜底；KenLM 缺口经分析为伪需求）。三语分工：C++ 薄插件 / Rust brain / Python 实验工具
- [x] **IPC 协议 v0**（docs/specs/ipc-v0.md）：命名管道消息模式 + JSON，query/commit 两个方法，插件侧 15ms 硬超时 + 2s 退避降级

- [x] **M2-1 E2E 链路 ✅（2026-06-10）**：src/brain（Rust，9 单测过）+ src/ime-plugin（C++，成为插件唯一源）。实测：echo 候选经管道上屏、稳态 e2e 中位 0.26-1.2ms（预算余量 >5 倍）、brain 死 23µs 检出不卡键、重启自愈。主会话复测修复 GetTickCount64 量化 bug（伪超时，换 QPC）。报告：docs/research/2026-06-10-m2-e2e-ipc.md（含 5 个遗留问题，管道抢注防护最重要）
- [x] **lm-artifacts-v0 导出 ✅**：dict 65,121 / ngram 128 万 / english 1 万，21.6s 可重建（experiments/001 export_artifacts.py）

- [x] **M2-2 解码移植 ✅（2026-06-10）**：Rust brain 真实拼音解码上线——`nihao`→你好、`zheshiyigetest`→这是一个test（混打直接工作）。解码中位 ≤200µs/最大 847µs（5ms 预算的 4%）；artifacts 加载 ~600ms/96MB；与 Python 参考 top-1 一致 10/10；26 单测过。又修一个真 bug：WaitForSingleObject 超时被时钟节拍量化提前返回（3030 键压测 133 次误判→0），QPC 重等待兜底。报告：docs/research/2026-06-10-m2-decoder-port.md。**已知锚点：`suijitidu`→"随即梯度"（通用 LM 错），M2-3 个人层接入后应变"随机梯度"**

- [x] **M2-3 个人记忆库 ✅（2026-06-10 晚）**：锚点通过——rime_console 选一次"随机梯度"，下一次裸打 `suijitidu` 首选即出；重启记忆仍在（commits.jsonl 回放，按天衰减 0.98）。场景分桶（pipe 实测）+ 贪心分词/拼音对齐 + 词典外短语促升建弧 + 英文大小写习惯 + 文档导入冷启动（export_personal.py → user_data/personal.tsv，6361 行）。持久化用**追加 jsonl 而非 rusqlite**（用户可查看/删行，理由见报告）。Python full 配置 top-1 一致 12/12；延迟无回归（个人层加载后 len20 中位 199µs）；37 单测全过。插件 commit_notifier 已挂（e2e 遗留 3 清掉）。报告：docs/research/2026-06-10-m2-personal-memory.md

- [x] **M2-4 代码侧 ✅（2026-06-11）**：① 不完整音节尾部修复——纯 fallback 后缀标 FallbackTail（先验 -1.5），`woyaoc`→"我要c"不再"我压oc"（根因：英文垃圾短词弧便宜），39 单测过；② 真实 scene 信号——插件 SceneProbe 前台进程名（HWND 缓存）接入 query/commit，brain 日志带 app=；③ 部署套件 experiments/004-weasel-deploy/（schema v0.2 +标点+Shift 切换、与明月并存 F4 兜底、deploy-weasel.ps1 自动备份/一键回滚、README 含 M2 验收协议）

- [x] **M2-4 部署成功 + 首日真实使用（2026-06-11）**：Weasel 0.17.4 + 自建 rime.dll，模块注册实证；用户真实打字，场景信号（notepad/terminal）与即时学习闭环全工作（用户用 Mochi 打出了反馈消息本身）。首日反馈两项改进当天上线：① 候选 5→24 + 翻页（schema v0.3）；② **essay 八股文进通用层**（繁→简+注音，词典 6.5万→37.3万，unigram λ=1 混权）——"有些常用词"案例修复，80 句 A/B 通用层 top-1 42.5%→46.2%（5胜2负）；加载 11.8s→1.15s（前缀集哈希化）。详见 experiments/004 README 结论

## 进行中

- **M2 正式验收**：真实使用对比微软拼音（协议见 experiments/004 README：20 句混打切换次数 + 整句准确率，建议 ≥3 天）

- [x] **第二轮反馈闭环（2026-06-11 下午）**：① 英文连写——连续英文弧切换成本只收一次（en_continue -0.5）+ 相邻英文词自动空格，`timewindow`→"time window"、`deeplearning`→"deep learning"；② **纠正信号检测**——同输入改选 = 纠正：新选择 3 倍加权、被否定事件按记录的精确增量回撤（RecentCommit 存实际计入的 token 句子，免疫短语促升导致的分词漂移），journal 仍存原始事实、回放重新解释（确定性测试过）。"桔子案例"翻回"句子" ✓。42 单测全过
- [x] **M3 打磨包第一期（2026-06-11 第三轮反馈闭环）**：① **尾部补全弧**（音节/词 key/英文词/个人词四路，惩罚 -2.0）——`congrat`→congratulations、`separ`→separate、`shangxiaw`→上下文、`eeg` 次选 EEGLab（个人补全）；② **模糊音**（z/zh c/ch s/sh in/ing en/eng an/ang，单音节变体，惩罚 -1.5）——`pinying`→拼音；③ **英文底座**：wordfreq top-50k + zipf 频率 λ=0.3 混入 unigram（英文词第一次有彼此排序）。44 单测全过。详见 devlog 2026-06-11-打磨包第一期
- [x] **第四轮反馈闭环（2026-06-11 晚）**：① **补全可视化**——补全候选 comment 标 `+«补入字母»`（如 自动补全后 `+ou`）、模糊候选标 `~`，Weasel 浅色渲染，解决"不知道打到哪了"的认知错位（真 ghost 内联仍归 M5）；② **放弃输入防污染**——≥6 字母且完整切为 ≥3 拼音音节的 ASCII 上屏视为放弃的原始输入，不学进英文词库（"zidongbuquanhou"曾被学成英文词并参与补全，回放即自愈）。46 单测全过

## 下一步（按优先级）

1. 真实使用观察打磨包体感（补全噪声水平、模糊音误报、50k 词表低频词噪声——必要时按 rank 加先验斜坡）
2. brain 自动拉起（插件 EnsureConnected 失败时 CreateProcess，或注册开机启动）——已经历一次"重启后 brain 没了"，体验前置项
3. 加载时间正常功耗复测（低功耗态 24.6s/归一化 ≈2s）；超 3s 则上二进制缓存。底座变更前先备份 artifacts 供 A/B（这轮漏了）
4. 中期：现代大语料重训 trigram（M3 二期，"整句准确率 ≥ 微软拼音"必要条件）；键内 typo 容错（`seperate`/`wn`）归 M4 与置信度同批；ghost 灰色内联归 M5（补全候选 = 其阶段 1）
5. 管道抢注防护（FIRST_PIPE_INSTANCE + ACL，产品化前）；UIA 补测（M5 前）

## 阻塞 / 待用户决策

- 仓库中其他工具生成的 AGENTS.md/.codex/.agents 是否保留（不阻塞）

## 关键约束备忘

- 按键热路径 <50ms（预算 ~25ms）；Brain IPC 硬超时 15-20ms（WeaselServer 全局锁）；学习必须即时生效
- 自动上屏指标：覆盖率下错误率 <1%，不是平均准确率
- 暂不考虑成本因素（用户明确）；隐私优先全本地；个人打字数据永不入库
